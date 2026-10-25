//! Module GO : conduite de spectacle à la façon de QLab.
//!
//! Les cues sont joués dans l'ordre : GO lance le cue en attente et passe au suivant.
//! Plusieurs cues peuvent sonner ensemble ; ils utilisent les emplacements de voix
//! des jingles du module (inutilisés en mode GO).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::audio::buffer::db_to_gain;
use crate::audio::eq::EqCoeffs;
use crate::audio::mixer::{Command, Region, JINGLE_SLOTS};
use crate::project::{Cue, ModuleMode};

use super::{collect_audio, Controller};

/// État de lecture d'une conduite (non sauvegardé).
#[derive(Debug, Default, Clone)]
pub struct GoRuntime {
    /// Cue en attente : le prochain GO le lance. `== len` : fin de la conduite.
    pub standby: usize,
    /// Cue joué par chaque emplacement de voix, avec l'identifiant de la voix.
    slots: [Option<(usize, u64)>; JINGLE_SLOTS],
    /// Cues lancés avant la fin de leur chargement.
    pending: Vec<usize>,
    last_go: Option<Instant>,
}

impl GoRuntime {
    /// Renumérote les cues après un changement de la liste ; `None` : cue supprimé.
    fn remap(&mut self, f: impl Fn(usize) -> Option<usize>) {
        for s in self.slots.iter_mut() {
            *s = s.and_then(|(i, id)| f(i).map(|j| (j, id)));
        }
        self.pending = self.pending.iter().filter_map(|&i| f(i)).collect();
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
    pub fn cue_go(&mut self, m: usize) {
        // Anti double-GO : un second GO trop rapproché est ignoré.
        let guard = Duration::from_millis(self.project.preferences.double_go_ms as u64);
        if self.gos[m].last_go.is_some_and(|t| t.elapsed() < guard) {
            return;
        }
        let i = self.gos[m].standby;
        if i >= self.cues(m).len() {
            return;
        }
        self.gos[m].last_go = Some(Instant::now());
        self.gos[m].standby = i + 1;
        self.play_cue(m, i);
    }

    /// Lance un cue tout de suite, sans changer le cue en attente.
    pub fn play_cue(&mut self, m: usize, i: usize) {
        let Some(cue) = self.cues(m).get(i).cloned() else { return };
        let Some(buf) = self.cache.get(&cue.path).cloned() else {
            if !self.gos[m].pending.contains(&i) {
                self.gos[m].pending.push(i);
            }
            self.request_jingle_load(&cue.path);
            return;
        };
        // Un emplacement libre, sinon celui du son lancé il y a le plus longtemps.
        let rt = &self.gos[m];
        let tel = &self.engine.telemetry;
        let slot = (0..JINGLE_SLOTS)
            .find(|&s| rt.slots[s].is_none() || tel.position(m, s).is_none())
            .unwrap_or_else(|| (0..JINGLE_SLOTS).min_by_key(|&s| rt.slots[s].map_or(0, |(_, id)| id)).unwrap_or(0));
        let id = self.voice_id();
        self.gos[m].slots[slot] = Some((i, id));
        let region = Region::full(&buf);
        self.engine.send(Command::Play { module: m, slot, id, buffer: buf, gain: db_to_gain(cue.volume_db), region, eq: EqCoeffs::flat() });
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
                    self.play_cue(m, i);
                }
            }
        }
    }

    pub(super) fn on_cue_ended(&mut self, m: usize, slot: usize, id: u64) {
        if let Some(s) = self.gos[m].slots.get_mut(slot) {
            if s.is_some_and(|(_, sid)| sid == id) {
                *s = None;
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
        let rt = &self.gos[m];
        self.cue_slots(m, i)
            .into_iter()
            .max_by_key(|&s| rt.slots[s].map_or(0, |(_, id)| id))
            .and_then(|s| self.engine.telemetry.position(m, s))
    }

    pub fn cue_loading(&self, m: usize, i: usize) -> bool {
        self.gos[m].pending.contains(&i)
    }

    pub fn stop_cue(&mut self, m: usize, i: usize) {
        self.gos[m].pending.retain(|&p| p != i);
        for slot in self.cue_slots(m, i) {
            self.engine.send(Command::Stop { module: m, slot });
        }
    }

    /// Arrête tous les cues du module, sans toucher au cue en attente.
    pub fn stop_cues(&mut self, m: usize) {
        self.gos[m].pending.clear();
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
        self.mark_dirty();
    }

    pub fn set_cue_volume(&mut self, m: usize, i: usize, db: f32) {
        let Some(c) = self.project.modules[m].cues.get_mut(i) else { return };
        c.volume_db = db;
        for slot in self.cue_slots(m, i) {
            self.engine.send(Command::SetSlotGain { module: m, slot, gain: db_to_gain(db) });
        }
        self.mark_dirty();
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
        rt.pending.clear();
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
