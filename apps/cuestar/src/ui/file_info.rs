//! Fenêtre « Informations sur le fichier », aussi détaillée que MediaInfo.

use crate::media_info::{self, Report};
use egui::{RichText, Ui};
use star_core::i18n::tr;
use star_ui::theme::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

pub struct FileInfo {
    path: PathBuf,
    full: bool,
    rx: Option<Receiver<Result<Report, String>>>,
    result: Option<Result<Report, String>>,
    search: String,
}

impl FileInfo {
    pub fn open(path: &Path) -> Self {
        Self { path: path.to_path_buf(), full: false, rx: Some(media_info::fetch(path, false)), result: None, search: String::new() }
    }

    /// Affiche la fenêtre ; false quand elle est fermée.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(r) => {
                    self.result = Some(r);
                    self.rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.result = Some(Err(tr("l'analyse du fichier a échoué", "the file analysis failed").to_string()));
                    self.rx = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut open = true;
        egui::Window::new(format!("{} — {name}", tr("Informations", "Information")))
            .id(egui::Id::unique(("file-info", &self.path)))
            .open(&mut open)
            .default_width(560.0)
            .default_height((ctx.content_rect().height() - 120.0).clamp(200.0, 640.0))
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| self.body(ui));
        open
    }

    fn body(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(tr("Rechercher…", "Search…")).desired_width(180.0));
            let was = self.full;
            ui.checkbox(&mut self.full, tr("Tout afficher", "Show everything")).on_hover_text(tr(
                "Vue complète de MediaInfo : tous les champs, sous toutes leurs formes",
                "MediaInfo's complete view: every field, in every form",
            ));
            if self.full != was {
                self.rx = Some(media_info::fetch(&self.path, self.full));
            }
            if let Some(Ok(r)) = &self.result {
                if ui.button(tr("Copier", "Copy")).on_hover_text(tr("Copier le rapport en texte", "Copy the report as text")).clicked() {
                    ui.ctx().copy_text(r.to_text());
                }
            }
            if self.rx.is_some() {
                ui.spinner();
            }
        });
        ui.add(egui::Label::new(RichText::new(self.path.display().to_string()).small().color(TEXT_DIM)).truncate());
        ui.separator();
        match &self.result {
            None => {
                ui.label(RichText::new(tr("Analyse du fichier…", "Reading the file…")).color(TEXT_DIM));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("{} : {e}", tr("Impossible de lire le fichier", "Cannot read the file"))).color(STOP));
            }
            Some(Ok(r)) => {
                let needle = self.search.trim().to_lowercase();
                egui::ScrollArea::vertical().id_salt("file-info-scroll").auto_shrink([false, false]).show(ui, |ui| {
                    for (si, s) in r.sections.iter().enumerate() {
                        let rows: Vec<&(String, String)> = s
                            .fields
                            .iter()
                            .filter(|(k, v)| needle.is_empty() || k.to_lowercase().contains(&needle) || v.to_lowercase().contains(&needle))
                            .collect();
                        if rows.is_empty() {
                            continue;
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new(&s.title).strong().color(ACCENT));
                        egui::Grid::new(("file-info-grid", si)).num_columns(2).min_col_width(200.0).spacing([16.0, 2.0]).striped(true).show(ui, |ui| {
                            for (k, v) in rows {
                                ui.label(RichText::new(k).color(TEXT_DIM));
                                ui.add(egui::Label::new(v.as_str()).wrap());
                                ui.end_row();
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{} {}", tr("Source :", "Source:"), r.source)).small().color(TEXT_DIM));
                });
            }
        }
    }
}

/// Ouvre la fenêtre d'un fichier, sauf si elle l'est déjà.
pub fn open_once(list: &mut Vec<FileInfo>, path: &Path) {
    if !list.iter().any(|w| w.path == path) {
        list.push(FileInfo::open(path));
    }
}
