//! Raccourcis clavier des jingles : choix dans l'éditeur, refus d'une touche déjà prise,
//! déclenchement hors saisie de texte. Un raccourci s'écrit « Ctrl+Shift+F5 » dans le projet.

use egui::{Event, Key, Modifiers, RichText, Ui};

use super::theme::*;
use super::JingleStarApp;
use crate::controller::Controller;
use crate::i18n::tr;
use crate::project::ModuleMode;

/// Forme enregistrée d'une touche avec ses modificateurs.
pub(super) fn format_hotkey(mods: Modifiers, key: Key) -> String {
    let mut s = String::new();
    if mods.command || mods.ctrl {
        s.push_str("Ctrl+");
    }
    if mods.alt {
        s.push_str("Alt+");
    }
    if mods.shift {
        s.push_str("Shift+");
    }
    s.push_str(key.name());
    s
}

/// Forme affichée : « Maj » au lieu de « Shift » en français.
pub(super) fn display_hotkey(h: &str) -> String {
    h.replace("Shift+", &format!("{}+", tr("Maj", "Shift")))
}

/// Raccourci déjà pris par JingleStar : ce qu'il fait.
fn reserved(mods: Modifiers, key: Key) -> Option<&'static str> {
    let ctrl = mods.command || mods.ctrl;
    if !ctrl && !mods.alt {
        // Sans Ctrl ni Alt, Maj ne change rien pour ces touches : elles restent prises.
        return match key {
            Key::Space => Some(tr("GO de la Cuelist", "Cuelist GO")),
            Key::P => Some(tr("pause de la Cuelist", "Cuelist pause")),
            Key::ArrowUp | Key::ArrowDown => Some(tr("cue précédent / suivant", "previous / next cue")),
            Key::ArrowLeft | Key::ArrowRight => Some(tr("flèches réservées", "reserved arrows")),
            Key::Escape => Some(tr("tout arrêter", "stop all")),
            Key::F1 => Some(tr("aide", "help")),
            Key::Plus | Key::Minus | Key::Equals => Some(tr("volume du fader", "fader volume")),
            _ => None,
        };
    }
    if ctrl && !mods.alt {
        return match key {
            Key::S if mods.shift => Some(tr("enregistrer sous", "save as")),
            Key::S => Some(tr("enregistrer", "save")),
            Key::O => Some(tr("ouvrir un projet", "open a project")),
            Key::N | Key::W => Some(tr("fermer le projet", "close the project")),
            Key::B => Some(tr("texte en gras", "bold text")),
            Key::I => Some(tr("texte en italique", "italic text")),
            Key::U => Some(tr("texte souligné", "underlined text")),
            _ => None,
        };
    }
    None
}

/// Premier appui de touche de l'image, retiré de la file avec tous les autres appuis.
fn take_key_press(ctx: &egui::Context) -> Option<(Modifiers, Key)> {
    ctx.input_mut(|i| {
        let first = i.events.iter().find_map(|e| match e {
            Event::Key { key, pressed: true, modifiers, .. } => Some((*modifiers, *key)),
            _ => None,
        });
        if first.is_some() {
            i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
        }
        first
    })
}

impl JingleStarApp {
    /// Pendant le choix d'un raccourci : prend la touche suivante. Renvoie vrai tant que
    /// le choix est en cours (les autres raccourcis sont alors ignorés).
    pub(super) fn capture_hotkey(&mut self, ctx: &egui::Context) -> bool {
        let Some((m, s)) = self.hotkey_capture else { return false };
        if self.ctrl.jingle(m, s).is_none() {
            self.hotkey_capture = None;
            return false;
        }
        let Some((mods, key)) = take_key_press(ctx) else { return true };
        self.hotkey_capture = None;
        if key == Key::Escape && mods.is_none() {
            self.hotkey_msg = None;
            return true;
        }
        let h = format_hotkey(mods, key);
        if let Some(what) = reserved(mods, key) {
            self.hotkey_msg = Some(format!("{} {} : {what}.", display_hotkey(&h), tr("est déjà utilisé par JingleStar", "is already used by JingleStar")));
            return true;
        }
        if let Some((om, os, name)) = self.jingle_with_hotkey(&h, Some((m, s))) {
            self.hotkey_msg = Some(format!(
                "{} {} {}.{} « {name} ».",
                display_hotkey(&h),
                tr("est déjà utilisé par le jingle", "is already used by jingle"),
                om + 1,
                os + 1
            ));
            return true;
        }
        self.hotkey_msg = None;
        self.ctrl.update_jingle(m, s, |j| j.hotkey = Some(h));
        true
    }

    /// Le jingle (module, emplacement, nom) qui porte ce raccourci, sauf `except`.
    fn jingle_with_hotkey(&self, h: &str, except: Option<(usize, usize)>) -> Option<(usize, usize, String)> {
        let p = &self.ctrl.project;
        p.modules.iter().enumerate().take(p.module_count).find_map(|(m, module)| {
            module.jingles.iter().enumerate().find_map(|(s, j)| {
                let j = j.as_ref()?;
                (Some((m, s)) != except && j.hotkey.as_deref() == Some(h)).then(|| (m, s, j.name.clone()))
            })
        })
    }

    /// Hors saisie de texte : un appui sur le raccourci d'un jingle le lance (ou l'arrête).
    pub(super) fn jingle_hotkeys(&mut self, ctx: &egui::Context) {
        let p = &self.ctrl.project;
        let any = p.modules.iter().take(p.module_count).any(|m| m.mode == ModuleMode::Jingle && m.jingles.iter().flatten().any(|j| j.hotkey.is_some()));
        if !any {
            return;
        }
        let presses: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some(format_hotkey(*modifiers, *key)),
                    _ => None,
                })
                .collect()
        });
        for h in presses {
            let hit = self.jingle_with_hotkey(&h, None).filter(|(m, _, _)| self.ctrl.project.modules[*m].mode == ModuleMode::Jingle);
            if let Some((m, s, _)) = hit {
                self.ctrl.trigger_jingle(m, s);
            }
        }
    }

    /// Ligne « Raccourci » de l'éditeur de jingle.
    pub(super) fn hotkey_row(&mut self, ui: &mut Ui, m: usize, s: usize, hotkey: &mut Option<String>) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if self.hotkey_capture == Some((m, s)) {
                    ui.label(RichText::new(tr("Appuyez sur une touche… (Échap annule)", "Press a key… (Esc cancels)")).color(ACCENT));
                    if ui.button(tr("Annuler", "Cancel")).clicked() {
                        self.hotkey_capture = None;
                    }
                } else {
                    let label = hotkey.as_deref().map(display_hotkey).unwrap_or_else(|| tr("Aucun", "None").to_string());
                    ui.label(RichText::new(label).monospace().strong());
                    if ui.button(tr("Choisir…", "Choose…")).on_hover_text(tr("Puis appuyez sur la touche voulue", "Then press the key you want")).clicked() {
                        self.hotkey_capture = Some((m, s));
                        self.hotkey_msg = None;
                    }
                    if hotkey.is_some() && ui.button(tr("Effacer", "Clear")).clicked() {
                        *hotkey = None;
                        self.hotkey_msg = None;
                    }
                }
            });
            if let Some(msg) = &self.hotkey_msg {
                ui.label(RichText::new(format!("ⓘ {msg} {}", tr("Choisissez-en un autre.", "Pick another one."))).color(ACCENT));
            }
        });
    }
}

/// Liste des raccourcis de jingles, pour l'onglet Raccourcis des Télécommandes.
pub(super) fn jingle_hotkeys_list(ui: &mut Ui, ctrl: &Controller) {
    ui.label(RichText::new(tr("Jingles", "Jingles")).strong());
    let p = &ctrl.project;
    let mut rows = Vec::new();
    for (m, module) in p.modules.iter().enumerate().take(p.module_count) {
        for (s, j) in module.jingles.iter().enumerate() {
            if let Some(h) = j.as_ref().and_then(|j| j.hotkey.as_deref().map(|h| (h, &j.name))) {
                rows.push((m, s, display_hotkey(h.0), h.1.clone()));
            }
        }
    }
    if rows.is_empty() {
        ui.label(RichText::new(tr(
            "Aucun jingle n'a de raccourci. Pour en donner un : mode édition, clic sur le jingle, ligne « Raccourci ».",
            "No jingle has a shortcut. To set one: edit mode, click the jingle, « Shortcut » row.",
        )).small().color(TEXT_DIM));
        return;
    }
    egui::Grid::new("jingle-hotkeys").num_columns(2).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
        for (m, s, h, name) in rows {
            ui.label(RichText::new(h).monospace().strong().color(ACCENT));
            ui.label(format!("{} {}.{} « {name} »", tr("Jingle", "Jingle"), m + 1, s + 1));
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_are_written_and_reserved_keys_refused() {
        assert_eq!(format_hotkey(Modifiers::NONE, Key::F5), "F5");
        assert_eq!(format_hotkey(Modifiers::COMMAND | Modifiers::SHIFT, Key::A), "Ctrl+Shift+A");
        assert!(reserved(Modifiers::NONE, Key::Space).is_some());
        assert!(reserved(Modifiers::SHIFT, Key::P).is_some(), "Maj+P déclencherait aussi la pause");
        assert!(reserved(Modifiers::COMMAND, Key::S).is_some());
        assert!(reserved(Modifiers::NONE, Key::F5).is_none());
        assert!(reserved(Modifiers::COMMAND, Key::F5).is_none());
    }
}
