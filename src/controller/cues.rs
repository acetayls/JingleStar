//! Module Cuelist (mode GO) : conduite de spectacle à la façon de QLab.
//!
//! Les cues sont joués dans l'ordre : GO lance le cue en attente et passe au suivant.
//! Chaque cue a les réglages d'un jingle (volume, points, boucle, fondus, égaliseur) et,
//! à sa fin, enchaîne sur le suivant ou attend le prochain GO.
//! Plusieurs cues peuvent sonner ensemble ; ils utilisent les emplacements de voix
//! des jingles du module (inutilisés en mode GO).
//!
//! Un cue « Enchaîner » dont le suivant est déjà chargé confie l'enchaînement au mixeur
//! (`PlayAfter`) : le suivant part à l'image près, sans dépendre de l'interface.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::audio::buffer::db_to_gain;
use crate::audio::mixer::{Command, FollowOn, JINGLE_SLOTS};
use crate::project::{Cue, CueFollow, ModuleMode, SoundSettings};

use super::{collect_audio, sound_region, Controller};

/// État de lecture d'une conduite (non sauvegardé).
#[derive(Debug, Default, Clone)]
pub struct GoRuntime {
    /// Cue en attente : le prochain GO le lance. `== len` : fin de la conduite.
    pub standby: usize,
    /// Cue joué par chaque emplacement de voix, avec l'identifiant de la voix.
    pub(super) slots: [Option<(usize, u64)>; JINGLE_SLOTS],
    /// Emplacements arrêtés à la main : leur fin ne fait pas enchaîner le cue suivant.
    stopping: [bool; JINGLE_SLOTS],
    /// Cues lancés avant la fin de leur chargement.
    pending: Vec<usize>,
    last_go: Option<Instant>,
    /// En pause : les cues qui sonnaient gardent leur place et reprennent sans clic.
    pub paused: bool,
    /// Enchaînements confiés au mixeur : à la fin de la voix `after_id`, le cue `index`
    /// part sur `slot` avec la voix `id`. `active` : encore prévu (sinon gardé seulement
    /// pour reconnaître une voix partie juste avant l'annulation).
    pub(super) planned: Vec<Planned>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Planned {
    pub(super) after_id: u64,
    after_slot: usize,
    pub(super) index: usize,
    pub(super) slot: usize,
    pub(super) id: u64,
    active: bool,
}

impl GoRuntime {
    /// Renumérote les cues après un changement de la liste ; `None` : cue supprimé.
    fn remap(&mut self, f: impl Fn(usize) -> Option<usize>) {
        for s in self.slots.iter_mut() {
            *s = s.and_then(|(i, id)| f(i).map(|j| (j, id)));
        }
        self.pending = self.pending.iter().filter_map(|&i| f(i)).collect();
        self.planned.retain_mut(|p| match f(p.index) {
            Some(j) => {
                p.index = j;
                true
            }
            None => false,
        });
    }

    /// Emplacements réservés à un enchaînement prévu.
    fn reserved(&self, slot: usize) -> bool {
        self.planned.iter().any(|p| p.active && p.slot == slot)
    }
}

/// Nouvel index du cue `i` quand le cue `from` est déplacé avant la position `to` (0..=len).
pub fn moved_index(i: usize, from: usize, to: usize) -> usize {
    let dest = if to > from { to - 1 } else { to };
    if i == from {
        dest
    } else if from < i && i <= dest {
        i - 1
    } else if dest <= i && i < from {
        i + 1
    } else {
        i
    }
}

impl Controller {
    pub fn cues(&self, m: usize) -> &[Cue] {
        &self.project.modules[m].cues
    }

    /// Premier module en mode GO (cible de la barre d'espace et de `/go`).
    pub fn first_go_module(&self) -> Option<usize> {
        (0..self.project.module_count).find(|&m| self.project.modules[m].mode == ModuleMode::Go)
    }

    /// GO : lance le cue en attente et met le suivant en attente.
    /// En pause, GO reprend d'abord les cues en pause (rien ne reste figé), puis lance le cue en attente.
    pub fn cue_go(&mut self, m: usize) {
        // Anti double-GO : un second GO trop rapproché est ignoré.
        let guard = Duration::from_millis(self.project.preferences.double_go_ms as u64);
        if self.gos[m].last_go.is_some_and(|t| t.elapsed() < guard) {
            return;
        }
        let resumed = self.gos[m].paused;
        self.cue_set_paused(m, false);
        let i = self.gos[m].standby;
        if i >= self.cues(m).len() {
            if resumed {
                self.gos[m].last_go = Some(Instant::now());
            }
            return;
        }
        self.gos[m].last_go = Some(Instant::now());
        self.gos[m].standby = i + 1;
        self.start_cue(m, i);
    }

    /// GO possible : un cue est en attente, ou une pause à reprendre.
    pub fn cue_go_ready(&self, m: usize) -> bool {
        self.gos[m].paused || self.gos[m].standby < self.cues(m).len()
    }

    /// Lance un cue tout de suite, sans changer le cue en attente (reprend d'abord une pause).
    pub fn play_cue(&mut self, m: usize, i: usize) {
        self.cue_set_paused(m, false);
        self.start_cue(m, i);
    }

    /// Des cues sonnent (ou attendent leur chargement) dans le module.
    pub fn cues_sounding(&self, m: usize) -> bool {
        let rt = &self.gos[m];
        !rt.pending.is_empty() || (0..JINGLE_SLOTS).any(|s| rt.slots[s].is_some() && !rt.stopping[s] && self.engine.telemetry.position(m, s).is_some())
    }

    /// Pause ou reprise de tous les cues du module, sans perdre leur place.
    /// Les enchaînements attendent : une voix en pause ne finit pas.
    pub fn cue_set_paused(&mut self, m: usize, paused: bool) {
        if paused == self.gos[m].paused || (paused && !self.cues_sounding(m)) {
            return;
        }
        self.gos[m].paused = paused;
        for slot in 0..JINGLE_SLOTS {
            self.engine.send(Command::SetPaused { module: m, slot, paused });
        }
    }

    pub fn cue_toggle_pause(&mut self, m: usize) {
        let paused = self.gos[m].paused;
        self.cue_set_paused(m, !paused);
    }

    /// Lance le cue `i` ; si la conduite est en pause (cue chargé ou enchaîné entre-temps),
    /// il part en pause avec les autres et reprendra avec eux.
    fn start_cue(&mut self, m: usize, i: usize) {
        let Some(cue) = self.cues(m).get(i).cloned() else { return };
        let Some(buf) = self.cache.get(&cue.path).cloned() else {
            if !self.gos[m].pending.contains(&i) {
                self.gos[m].pending.push(i);
            }
            self.request_urgent_load(&cue.path);
            return;
        };
        let slot = self.free_cue_slot(m, None);
        let id = self.voice_id();
        self.gos[m].slots[slot] = Some((i, id));
        self.gos[m].stopping[slot] = false;
        let next = self.cue_voice(&cue, slot, id, buf);
        self.engine.send(Command::Play { module: m, slot, id, buffer: next.buffer, gain: next.gain, region: next.region, eq: next.eq });
        if self.gos[m].paused {
            self.engine.send(Command::SetPaused { module: m, slot, paused: true });
        }
        self.plan_follow(m, i, slot, id);
    }

    /// Un emplacement libre (ni en lecture ni réservé), sinon celui du son lancé il y a le plus longtemps.
    fn free_cue_slot(&self, m: usize, except: Option<usize>) -> usize {
        let rt = &self.gos[m];
        let tel = &self.engine.telemetry;
        let usable = |s: usize| Some(s) != except && !rt.reserved(s);
        (0..JINGLE_SLOTS)
            .find(|&s| usable(s) && (rt.slots[s].is_none() || tel.position(m, s).is_none()))
            .unwrap_or_else(|| {
                (0..JINGLE_SLOTS).filter(|&s| usable(s)).min_by_key(|&s| rt.slots[s].map_or(0, |(_, id)| id)).unwrap_or(0)
            })
    }

    fn cue_voice(&self, cue: &Cue, slot: usize, id: u64, buf: std::sync::Arc<crate::audio::buffer::AudioBuffer>) -> FollowOn {
        // Pas de boucle pour les cues pour l'instant : un cue joue toujours une fois.
        let region = sound_region(&SoundSettings { looping: false, ..cue.sound.clone() }, &buf);
        FollowOn { slot, id, gain: db_to_gain(cue.sound.volume_db), region, eq: cue.sound.eq.coeffs(self.rate()), buffer: buf }
    }

    /// Cue `i` lancé sur `slot` (voix `id`) : s'il enchaîne et que le suivant est chargé,
    /// le mixeur lancera le suivant à l'image près.
    fn plan_follow(&mut self, m: usize, i: usize, slot: usize, id: u64) {
        let cues = self.cues(m);
        let follows = cues.get(i).is_some_and(|c| c.follow == CueFollow::Continue);
        let Some(next) = cues.get(i + 1).cloned().filter(|_| follows && self.project.modules[m].mode == ModuleMode::Go) else { return };
        let Some(buf) = self.cache.get(&next.path).cloned() else {
            // Pas encore chargé : l'enchaînement se fera à la fin du cue, dès que possible.
            self.request_jingle_load(&next.path);
            return;
        };
        let next_slot = self.free_cue_slot(m, Some(slot));
        let next_id = self.voice_id();
        let voice = self.cue_voice(&next, next_slot, next_id, buf);
        self.gos[m].planned.push(Planned { after_id: id, after_slot: slot, index: i + 1, slot: next_slot, id: next_id, active: true });
        self.engine.send(Command::PlayAfter { module: m, after_slot: slot, after_id: id, next: voice });
    }

    /// Les cues ont changé (réglages, ordre, chargement) : les enchaînements prévus sont refaits.
    pub(super) fn replan_follows(&mut self, m: usize) {
        if self.gos[m].planned.iter().all(|p| !p.active) && self.gos[m].slots.iter().all(|s| s.is_none()) {
            return;
        }
        for p in self.gos[m].planned.iter_mut().filter(|p| p.active) {
            p.active = false;
            self.engine.send(Command::CancelFollow { module: m, slot: p.after_slot });
        }
        for slot in 0..JINGLE_SLOTS {
            let rt = &self.gos[m];
            let Some((i, id)) = rt.slots[slot] else { continue };
            if !rt.stopping[slot] && self.engine.telemetry.position(m, slot).is_some() {
                self.plan_follow(m, i, slot, id);
            }
        }
    }

    pub(super) fn replan_all_follows(&mut self) {
        for m in 0..self.project.module_count {
            if self.project.modules[m].mode == ModuleMode::Go {
                self.replan_follows(m);
            }
        }
    }

    /// Lance les cues qui attendaient ce son.
    pub(super) fn start_pending_cues(&mut self, path: &Path, loaded: bool) {
        for m in 0..self.project.module_count {
            let waiting: Vec<usize> = self.gos[m]
                .pending
                .iter()
                .copied()
                .filter(|&i| self.cues(m).get(i).is_some_and(|c| c.path == path))
                .collect();
            self.gos[m].pending.retain(|i| !waiting.contains(i));
            if loaded {
                for i in waiting {
                    self.start_cue(m, i);
                }
            }
        }
    }

    pub(super) fn on_cue_ended(&mut self, m: usize, slot: usize, id: u64, follow: Option<(usize, u64)>) {
        // Le mixeur a déjà lancé le cue suivant, à l'image près.
        let planned: Vec<Planned> = self.gos[m].planned.iter().copied().filter(|p| p.after_id == id).collect();
        self.gos[m].planned.retain(|p| p.after_id != id);
        if let Some(p) = follow.and_then(|(fs, fid)| planned.iter().find(|p| p.slot == fs && p.id == fid)) {
            if let Some(s) = self.gos[m].slots.get_mut(slot).filter(|s| s.is_some_and(|(_, sid)| sid == id)) {
                *s = None;
            }
            self.gos[m].stopping[slot] = false;
            self.gos[m].slots[p.slot] = Some((p.index, p.id));
            self.gos[m].stopping[p.slot] = false;
            // Parti juste avant la pause : il la rejoint.
            if self.gos[m].paused {
                self.engine.send(Command::SetPaused { module: m, slot: p.slot, paused: true });
            }
            if self.gos[m].standby == p.index {
                self.gos[m].standby = p.index + 1;
            }
            self.plan_follow(m, p.index, p.slot, p.id);
            return;
        }
        let Some(s) = self.gos[m].slots.get_mut(slot) else { return };
        let Some((i, _)) = s.filter(|&(_, sid)| sid == id) else { return };
        *s = None;
        let stopped = std::mem::take(&mut self.gos[m].stopping[slot]);
        // Fin naturelle d'un cue « Enchaîner » : le suivant part tout seul.
        let follow = self.cues(m).get(i).is_some_and(|c| c.follow == CueFollow::Continue);
        if follow && !stopped && self.project.modules[m].mode == ModuleMode::Go && i + 1 < self.cues(m).len() {
            if self.gos[m].standby == i + 1 {
                self.gos[m].standby = i + 2;
            }
            self.start_cue(m, i + 1);
        }
        // Plus rien ne sonne ni n'attend : la pause n'a plus d'objet.
        let rt = &mut self.gos[m];
        if rt.slots.iter().all(|s| s.is_none()) && rt.pending.is_empty() {
            rt.paused = false;
        }
    }

    /// Les cues qui jouent dans le module s'arrêtent à la main : pas d'enchaînement à leur fin.
    pub(super) fn mark_cues_stopping(&mut self, m: usize) {
        let rt = &mut self.gos[m];
        rt.paused = false;
        for s in 0..JINGLE_SLOTS {
            if rt.slots[s].is_some() {
                rt.stopping[s] = true;
            }
        }
    }

    /// Emplacements qui jouent le cue `i`.
    fn cue_slots(&self, m: usize, i: usize) -> Vec<usize> {
        (0..JINGLE_SLOTS)
            .filter(|&s| self.gos[m].slots[s].is_some_and(|(c, _)| c == i) && self.engine.telemetry.position(m, s).is_some())
            .collect()
    }

    /// Position (images) du dernier lancement du cue `i`, s'il joue.
    pub fn cue_position(&self, m: usize, i: usize) -> Option<(u64, u64)> {
        self.cue_playing_slot(m, i).and_then(|s| self.engine.telemetry.position(m, s))
    }

    /// Cue en cours : le dernier lancé qui sonne encore, avec sa position et sa longueur (images).
    pub fn cue_running(&self, m: usize) -> Option<(usize, u64, u64)> {
        let rt = &self.gos[m];
        (0..JINGLE_SLOTS)
            .filter_map(|s| {
                let (i, id) = rt.slots[s]?;
                let (pos, len) = self.engine.telemetry.position(m, s)?;
                (!rt.stopping[s]).then_some((id, i, pos, len))
            })
            .max_by_key(|&(id, ..)| id)
            .map(|(_, i, pos, len)| (i, pos, len))
    }

    pub fn cue_loading(&self, m: usize, i: usize) -> bool {
        self.gos[m].pending.contains(&i)
    }

    pub fn stop_cue(&mut self, m: usize, i: usize) {
        self.gos[m].pending.retain(|&p| p != i);
        for slot in self.cue_slots(m, i) {
            self.gos[m].stopping[slot] = true;
            self.engine.send(Command::Stop { module: m, slot });
        }
    }

    /// Arrête tous les cues du module, sans toucher au cue en attente.
    pub fn stop_cues(&mut self, m: usize) {
        self.gos[m].pending.clear();
        self.mark_cues_stopping(m);
        for slot in 0..JINGLE_SLOTS {
            self.engine.send(Command::Stop { module: m, slot });
        }
    }

    /// Déplace le cue en attente (précédent / suivant).
    pub fn cue_step(&mut self, m: usize, delta: isize) {
        let len = self.cues(m).len() as isize;
        let cur = (self.gos[m].standby as isize).min(len);
        self.gos[m].standby = (cur + delta).clamp(0, (len - 1).max(0)) as usize;
    }

    pub fn set_standby(&mut self, m: usize, i: usize) {
        self.gos[m].standby = i.min(self.cues(m).len());
    }

    /// Ajoute des sons à la fin de la conduite (un dossier ajoute ses fichiers audio), ou avant `at`.
    pub fn add_cues(&mut self, m: usize, paths: Vec<PathBuf>, at: Option<usize>) {
        let mut added = Vec::new();
        for p in paths {
            if p.is_dir() {
                collect_audio(&p, &mut added);
            } else if crate::audio::decode::is_audio_file(&p) {
                added.push(p);
            }
        }
        if added.is_empty() {
            return;
        }
        let len = self.cues(m).len();
        let at = at.unwrap_or(len).min(len);
        let n = added.len();
        for p in &added {
            self.request_jingle_load(p);
        }
        let cues = &mut self.project.modules[m].cues;
        cues.splice(at..at, added.into_iter().map(Cue::new));
        let rt = &mut self.gos[m];
        rt.remap(|i| Some(if i >= at { i + n } else { i }));
        // Le cue en attente reste le même son ; à la fin d'une conduite terminée,
        // le premier son ajouté passe en attente.
        if rt.standby > at || (rt.standby == at && at < len) {
            rt.standby += n;
        }
        self.replan_follows(m);
        self.mark_dirty();
    }

    pub fn remove_cue(&mut self, m: usize, i: usize) {
        if i >= self.cues(m).len() {
            return;
        }
        self.stop_cue(m, i);
        self.project.modules[m].cues.remove(i);
        let rt = &mut self.gos[m];
        rt.remap(|c| match c.cmp(&i) {
            std::cmp::Ordering::Less => Some(c),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(c - 1),
        });
        if rt.standby > i {
            rt.standby -= 1;
        }
        self.replan_follows(m);
        self.mark_dirty();
    }

    pub fn clear_cues(&mut self, m: usize) {
        self.stop_cues(m);
        self.project.modules[m].cues.clear();
        self.gos[m] = GoRuntime::default();
        self.mark_dirty();
    }

    /// Glisser-déposer : le cue `from` va avant la position `to` (0..=len).
    /// Le cue en attente reste le même son.
    pub fn move_cue(&mut self, m: usize, from: usize, to: usize) {
        let len = self.cues(m).len();
        if from >= len || to > len || to == from || to == from + 1 {
            return;
        }
        let cue = self.project.modules[m].cues.remove(from);
        let dest = if to > from { to - 1 } else { to };
        self.project.modules[m].cues.insert(dest, cue);
        let rt = &mut self.gos[m];
        rt.remap(|i| Some(moved_index(i, from, to)));
        if rt.standby < len {
            rt.standby = moved_index(rt.standby, from, to);
        }
        self.replan_follows(m);
        self.mark_dirty();
    }

    pub fn set_cue_volume(&mut self, m: usize, i: usize, db: f32) {
        let Some(db) = super::clean_db(db, crate::osc::JINGLE_DB) else { return };
        let Some(c) = self.project.modules[m].cues.get_mut(i) else { return };
        c.sound.volume_db = db;
        for slot in self.cue_slots(m, i) {
            self.engine.send(Command::SetSlotGain { module: m, slot, gain: db_to_gain(db) });
        }
        self.replan_follows(m);
        self.mark_dirty();
    }

    /// Modifie un cue ; s'il joue, volume, égaliseur et points s'appliquent tout de suite.
    pub fn update_cue(&mut self, m: usize, i: usize, f: impl FnOnce(&mut Cue)) {
        let rate = self.rate();
        let Some(c) = self.project.modules[m].cues.get_mut(i) else { return };
        let before = c.clone();
        f(c);
        if !c.sound.volume_db.is_finite() {
            c.sound.volume_db = before.sound.volume_db;
        }
        if *c == before {
            return;
        }
        let c = c.clone();
        for slot in self.cue_slots(m, i) {
            self.engine.send(Command::SetSlotGain { module: m, slot, gain: db_to_gain(c.sound.volume_db) });
            if c.sound.eq != before.sound.eq {
                self.engine.send(Command::SetSlotEq { module: m, slot, eq: c.sound.eq.coeffs(rate) });
            }
            if let Some(buf) = self.cache.get(&c.path) {
                self.engine.send(Command::SetRegion { module: m, slot, region: sound_region(&c.sound, buf) });
            }
        }
        if c.path != before.path {
            self.request_jingle_load(&c.path);
        }
        // Point d'entrée ou boucle déplacés : prêts en mémoire tout de suite.
        self.update_stream_pins();
        self.replan_follows(m);
        self.mark_dirty();
    }

    /// Lecture en cours du cue `i` : déplacement à l'image `frame`, avec un micro-fondu.
    pub fn cue_seek(&mut self, m: usize, i: usize, frame: usize) {
        if let Some(slot) = self.cue_playing_slot(m, i) {
            self.engine.send(Command::Seek { module: m, slot, frame });
        }
    }

    /// Emplacement du dernier lancement du cue `i`, s'il joue.
    fn cue_playing_slot(&self, m: usize, i: usize) -> Option<usize> {
        let rt = &self.gos[m];
        self.cue_slots(m, i).into_iter().max_by_key(|&s| rt.slots[s].map_or(0, |(_, id)| id))
    }

    pub fn rename_cue(&mut self, m: usize, i: usize, name: String) {
        if let Some(c) = self.project.modules[m].cues.get_mut(i) {
            c.name = name;
            self.mark_dirty();
        }
    }

    /// Le module s'arrête ou change d'index : plus rien ne joue ni n'attend son chargement.
    pub(super) fn reset_go_voices(&mut self, m: usize) {
        let rt = &mut self.gos[m];
        rt.slots = Default::default();
        rt.stopping = Default::default();
        rt.pending.clear();
        rt.planned.clear();
        rt.paused = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_a_cue_renumbers_the_others() {
        // [A B C D] : B (1) avant D (3) → [A C B D].
        let order: Vec<usize> = (0..4).map(|i| moved_index(i, 1, 3)).collect();
        assert_eq!(order, vec![0, 2, 1, 3]);
        // D (3) en tête → [D A B C].
        let order: Vec<usize> = (0..4).map(|i| moved_index(i, 3, 0)).collect();
        assert_eq!(order, vec![1, 2, 3, 0]);
        // A (0) à la fin → [B C D A].
        let order: Vec<usize> = (0..4).map(|i| moved_index(i, 0, 4)).collect();
        assert_eq!(order, vec![3, 0, 1, 2]);
    }

    /// Ordre obtenu en déplaçant `from` avant `to` dans [0, 1, …, len-1].
    fn reorder(len: usize, from: usize, to: usize) -> Vec<usize> {
        let mut order = vec![0; len];
        for i in 0..len {
            order[moved_index(i, from, to)] = i;
        }
        order
    }

    #[test]
    fn moved_index_in_every_direction() {
        // Vers le haut : C (2) avant B (1).
        assert_eq!(reorder(5, 2, 1), vec![0, 2, 1, 3, 4]);
        // Vers le bas : B (1) après D (3), donc avant E (4).
        assert_eq!(reorder(5, 1, 4), vec![0, 2, 3, 1, 4]);
        // Au début et à la fin.
        assert_eq!(reorder(5, 3, 0), vec![3, 0, 1, 2, 4]);
        assert_eq!(reorder(5, 1, 5), vec![0, 2, 3, 4, 1]);
        // Sur place (avant soi-même ou avant le suivant) : rien ne bouge.
        assert_eq!(reorder(5, 2, 2), vec![0, 1, 2, 3, 4]);
        assert_eq!(reorder(5, 2, 3), vec![0, 1, 2, 3, 4]);
        // Chaque déplacement est une permutation.
        for from in 0..5 {
            for to in 0..=5 {
                let mut o = reorder(5, from, to);
                o.sort();
                assert_eq!(o, vec![0, 1, 2, 3, 4], "from {from} to {to}");
            }
        }
    }

    fn show(n: usize) -> Controller {
        let mut p = crate::project::Project::default();
        p.modules[0].mode = ModuleMode::Go;
        p.modules[0].cues = (0..n).map(|i| Cue::new(format!("/introuvable/{i}.wav").into())).collect();
        Controller::with_project(p, None, || {})
    }

    fn names(c: &Controller) -> Vec<String> {
        c.cues(0).iter().map(|c| c.path.file_stem().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn move_cue_keeps_the_standby_cue() {
        let mut c = show(5);
        c.set_standby(0, 2);
        // Vers le bas, le cue en attente lui-même.
        c.move_cue(0, 2, 5);
        assert_eq!(names(&c), ["0", "1", "3", "4", "2"]);
        assert_eq!(c.gos[0].standby, 4);
        // Vers le haut, jusqu'au début.
        c.move_cue(0, 4, 0);
        assert_eq!(names(&c), ["2", "0", "1", "3", "4"]);
        assert_eq!(c.gos[0].standby, 0);
        // Un autre cue passe devant le cue en attente : il recule d'une place.
        c.set_standby(0, 2);
        c.move_cue(0, 4, 1);
        assert_eq!(names(&c), ["2", "4", "0", "1", "3"]);
        assert_eq!(c.gos[0].standby, 3, "le cue en attente reste « 1 »");
        // Sur place : rien ne change.
        c.move_cue(0, 1, 1);
        c.move_cue(0, 1, 2);
        assert_eq!(names(&c), ["2", "4", "0", "1", "3"]);
        // Fin de conduite : elle le reste.
        c.set_standby(0, 5);
        c.move_cue(0, 0, 5);
        assert_eq!(c.gos[0].standby, 5);
    }

    #[test]
    fn pause_is_cleared_by_go_and_stop() {
        let mut c = show(3);
        // Rien ne sonne : pas de pause.
        c.cue_toggle_pause(0);
        assert!(!c.gos[0].paused);
        // Un cue lancé (en chargement) : la pause prend.
        c.cue_go(0);
        assert_eq!(c.gos[0].standby, 1);
        c.cue_toggle_pause(0);
        assert!(c.gos[0].paused);
        assert!(c.cue_go_ready(0));
        c.cue_toggle_pause(0);
        assert!(!c.gos[0].paused);
        // GO pendant la pause : reprise, puis le cue en attente part.
        c.cue_toggle_pause(0);
        c.gos[0].last_go = None;
        c.cue_go(0);
        assert!(!c.gos[0].paused);
        assert_eq!(c.gos[0].standby, 2);
        // Arrêter lève la pause.
        c.cue_toggle_pause(0);
        assert!(c.gos[0].paused);
        c.stop_cues(0);
        assert!(!c.gos[0].paused);
    }

    #[test]
    fn remap_drops_removed_cues() {
        let mut rt = GoRuntime::default();
        rt.slots[0] = Some((2, 10));
        rt.slots[1] = Some((1, 11));
        rt.pending = vec![1, 3];
        rt.remap(|i| match i {
            1 => None,
            i if i > 1 => Some(i - 1),
            i => Some(i),
        });
        assert_eq!(rt.slots[0], Some((1, 10)));
        assert_eq!(rt.slots[1], None);
        assert_eq!(rt.pending, vec![2]);
    }
}
