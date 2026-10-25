//! Fenêtre d'aide : affiche le manuel de `docs/` (aide.md ou help.md selon la langue).
//!
//! Le manuel est écrit dans un Markdown simple, interprété ici : titres, paragraphes,
//! listes, tableaux, blocs de code, **gras**, `code` et liens internes (#ancre).

use std::sync::OnceLock;

use egui::{vec2, Align, Color32, CornerRadius, FontId, RichText, Stroke};

use super::theme::*;
use crate::i18n::{is_en, tr};

const AIDE_FR: &str = include_str!("../../docs/aide.md");
const HELP_EN: &str = include_str!("../../docs/help.md");

#[derive(Debug, Clone, PartialEq)]
enum Span {
    Text(String),
    Bold(String),
    Code(String),
    /// Lien vers une ancre de titre (`#prise-en-main`) ou une adresse web.
    Link(String, String),
}

#[derive(Debug, Clone, PartialEq)]
enum Block {
    Heading(u8, String),
    Paragraph(Vec<Span>),
    /// Puce (`None`) ou numéro.
    Item(Option<String>, Vec<Span>),
    Table(Vec<Vec<Vec<Span>>>),
    Code(String),
}

/// État de la fenêtre d'aide.
#[derive(Default)]
pub struct HelpWindow {
    pub open: bool,
    /// Titre vers lequel faire défiler (index dans les blocs).
    jump: Option<usize>,
    /// Texte recherché ; vide = manuel complet.
    query: String,
}

impl HelpWindow {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let blocks = doc();
        let mut open = true;
        egui::Window::new(tr("Aide", "Help"))
            .id(egui::Id::unique("help-window"))
            .open(&mut open)
            .default_size([860.0, 640.0])
            .min_width(520.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                // Recherche : n'affiche que les passages qui contiennent les mots tapés.
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(tr("Rechercher dans l'aide…", "Search the help…"))
                            .desired_width(320.0),
                    );
                    if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                        field.request_focus();
                    }
                    if !self.query.is_empty() && ui.small_button("✕").on_hover_text(tr("Effacer", "Clear")).clicked() {
                        self.query.clear();
                    }
                });
                ui.add_space(4.0);
                let words = search_words(&self.query);
                let height = ui.available_height().max(300.0);
                ui.horizontal_top(|ui| {
                    // Sommaire.
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        egui::ScrollArea::vertical().id_salt("help-toc").max_height(height).show(ui, |ui| {
                            for (i, b) in blocks.iter().enumerate() {
                                if let Block::Heading(2, title) = b {
                                    if ui.add(egui::Button::new(RichText::new(title).color(TEXT)).frame(false)).clicked() {
                                        self.query.clear();
                                        self.jump = Some(i);
                                    }
                                }
                            }
                        });
                    });
                    ui.separator();
                    // Contenu.
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical().id_salt("help-body").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_max_width(760.0);
                            if !words.is_empty() {
                                if let Some(target) = render_results(ui, blocks, &words) {
                                    // Lien suivi depuis un résultat : retour au manuel complet.
                                    self.query.clear();
                                    self.jump = find_anchor(blocks, &target);
                                }
                                return;
                            }
                            let jump = self.jump.take();
                            for (i, b) in blocks.iter().enumerate() {
                                if let Some(target) = render_block(ui, b, jump == Some(i)) {
                                    self.jump = find_anchor(blocks, &target);
                                }
                            }
                        });
                    });
                });
            });
        if !open {
            self.open = false;
        }
    }
}

/// Manuel de la langue courante, interprété une seule fois.
fn doc() -> &'static [Block] {
    static FR: OnceLock<Vec<Block>> = OnceLock::new();
    static EN: OnceLock<Vec<Block>> = OnceLock::new();
    if is_en() {
        EN.get_or_init(|| parse(HELP_EN))
    } else {
        FR.get_or_init(|| parse(AIDE_FR))
    }
}

/// Mots recherchés, en minuscules et sans accents.
fn search_words(query: &str) -> Vec<String> {
    fold(query).split_whitespace().map(str::to_owned).collect()
}

/// Minuscules sans accents, pour qu'« equalizeur », « Égaliseur » ou « egaliseur » se trouvent.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(span_text).collect::<Vec<_>>().join("")
}

fn matches(text: &str, words: &[String]) -> bool {
    let t = fold(text);
    words.iter().all(|w| t.contains(w.as_str()))
}

/// Résultats de recherche : chaque passage trouvé, sous le titre de sa section
/// (et, pour un tableau, seulement les lignes qui correspondent).
fn render_results(ui: &mut egui::Ui, blocks: &[Block], words: &[String]) -> Option<String> {
    let mut clicked = None;
    let mut section: Option<&Block> = None;
    let mut shown_section = false;
    let mut count = 0;
    for b in blocks {
        let hit: Option<Block> = match b {
            Block::Heading(..) => {
                section = Some(b);
                shown_section = false;
                if matches(&heading_text(b), words) {
                    shown_section = true;
                    count += 1;
                    if let Some(t) = render_block(ui, b, false) {
                        clicked = Some(t);
                    }
                }
                continue;
            }
            Block::Paragraph(s) | Block::Item(_, s) => matches(&spans_text(s), words).then(|| b.clone()),
            Block::Code(c) => matches(c, words).then(|| b.clone()),
            Block::Table(rows) => {
                let found: Vec<_> = rows.iter().skip(1).filter(|r| matches(&r.iter().map(|c| spans_text(c)).collect::<Vec<_>>().join(" "), words)).cloned().collect();
                (!found.is_empty()).then(|| Block::Table(rows.iter().take(1).cloned().chain(found).collect()))
            }
        };
        if let Some(hit) = hit {
            count += 1;
            if !shown_section {
                shown_section = true;
                if let Some(h) = section {
                    render_block(ui, h, false);
                }
            }
            if let Some(t) = render_block(ui, &hit, false) {
                clicked = Some(t);
            }
        }
    }
    if count == 0 {
        ui.add_space(20.0);
        ui.label(RichText::new(tr("Aucun résultat.", "No results.")).color(TEXT_DIM));
    }
    clicked
}

fn heading_text(b: &Block) -> String {
    match b {
        Block::Heading(_, t) => t.clone(),
        _ => String::new(),
    }
}

fn find_anchor(blocks: &[Block], target: &str) -> Option<usize> {
    let anchor = target.strip_prefix('#')?;
    blocks.iter().position(|b| matches!(b, Block::Heading(_, t) if slug(t) == anchor))
}

/// Ancre d'un titre, comme sur GitHub : minuscules, ponctuation retirée, espaces en tirets.
fn slug(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------------------------ affichage

/// Affiche un bloc ; renvoie la cible d'un lien cliqué.
fn render_block(ui: &mut egui::Ui, block: &Block, scroll_here: bool) -> Option<String> {
    let mut clicked = None;
    match block {
        Block::Heading(level, text) => {
            let (size, space) = match level {
                1 => (24.0, 4.0),
                2 => (19.0, 14.0),
                _ => (15.5, 8.0),
            };
            ui.add_space(space);
            let color = if *level == 2 { ACCENT } else { Color32::WHITE };
            let resp = ui.label(RichText::new(text).font(FontId::proportional(size)).strong().color(color));
            if scroll_here {
                resp.scroll_to_me(Some(Align::TOP));
            }
            if *level <= 2 {
                let y = ui.cursor().top();
                let r = ui.max_rect();
                ui.painter().hline(r.left()..=r.right(), y, Stroke::new(1.0, BORDER));
                ui.add_space(4.0);
            }
        }
        Block::Paragraph(spans) => {
            clicked = inline(ui, spans);
            ui.add_space(6.0);
        }
        Block::Item(marker, spans) => {
            ui.horizontal_top(|ui| {
                ui.add_space(8.0);
                let m = marker.clone().unwrap_or_else(|| "•".into());
                ui.add_sized(vec2(18.0, 0.0), egui::Label::new(RichText::new(m).color(ACCENT)));
                ui.vertical(|ui| clicked = inline(ui, spans));
            });
            ui.add_space(2.0);
        }
        Block::Table(rows) => {
            ui.add_space(4.0);
            if let Some(t) = table(ui, rows) {
                clicked = Some(t);
            }
            ui.add_space(8.0);
        }
        Block::Code(code) => {
            egui::Frame::new()
                .fill(Color32::from_rgb(0x14, 0x14, 0x16))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(code).monospace().color(GO));
                });
            ui.add_space(8.0);
        }
    }
    clicked
}

/// Tableau : colonnes à leur largeur naturelle, la plus large rétrécie (et son texte
/// renvoyé à la ligne) si le tout dépasse la largeur disponible.
fn table(ui: &mut egui::Ui, rows: &[Vec<Vec<Span>>]) -> Option<String> {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return None;
    }
    let (gap, pad) = (14.0, 6.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let mut widths = vec![0.0f32; cols];
    for row in rows {
        for (c, cell) in row.iter().enumerate() {
            let w: f32 = cell
                .iter()
                .map(|s| {
                    let f = if matches!(s, Span::Code(_)) { mono.clone() } else { font.clone() };
                    ui.fonts_mut(|fonts| fonts.layout_no_wrap(span_text(s).to_owned(), f, TEXT).size().x)
                })
                .sum();
            widths[c] = widths[c].max(w.ceil() + 2.0);
        }
    }
    let avail = ui.available_width() - 2.0 * pad - gap * (cols as f32 - 1.0);
    let total: f32 = widths.iter().sum();
    if total > avail {
        let widest = (0..cols).max_by(|&a, &b| widths[a].total_cmp(&widths[b])).unwrap_or(0);
        widths[widest] = (widths[widest] - (total - avail)).max(120.0);
    }

    let mut clicked = None;
    let frame = egui::Frame::new().stroke(Stroke::new(1.0, BORDER)).corner_radius(4).inner_margin(pad);
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (r, row) in rows.iter().enumerate() {
            let bg = ui.painter().add(egui::Shape::Noop);
            let resp = ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (c, w) in widths.iter().enumerate() {
                    ui.allocate_ui_with_layout(vec2(*w, 0.0), egui::Layout::top_down(Align::Min), |ui| {
                        ui.set_width(*w);
                        ui.add_space(4.0);
                        let cell = row.get(c).map(|v| v.as_slice()).unwrap_or(&[]);
                        if r == 0 {
                            let text: String = cell.iter().map(span_text).collect();
                            ui.label(RichText::new(text).strong().color(Color32::WHITE));
                        } else if let Some(t) = inline(ui, cell) {
                            clicked = Some(t);
                        }
                        ui.add_space(4.0);
                    });
                }
            });
            let rect = resp.response.rect.expand2(vec2(pad, 0.0));
            if r == 0 {
                ui.painter().set(bg, egui::Shape::line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, BORDER)));
            } else if r % 2 == 0 {
                ui.painter().set(bg, egui::Shape::rect_filled(rect, 0, PANEL_HI));
            }
        }
    });
    clicked
}

fn span_text(s: &Span) -> &str {
    match s {
        Span::Text(t) | Span::Bold(t) | Span::Code(t) | Span::Link(t, _) => t,
    }
}

/// Texte courant avec mise en forme ; renvoie la cible d'un lien cliqué.
fn inline(ui: &mut egui::Ui, spans: &[Span]) -> Option<String> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in spans {
            match s {
                Span::Text(t) => {
                    ui.label(t);
                }
                Span::Bold(t) => {
                    ui.label(RichText::new(t).strong().color(Color32::WHITE));
                }
                Span::Code(t) => {
                    ui.label(RichText::new(t).monospace().color(ACCENT).background_color(PANEL_HI));
                }
                Span::Link(t, target) => {
                    if target.starts_with('#') {
                        if ui.link(t).clicked() {
                            clicked = Some(target.clone());
                        }
                    } else {
                        ui.hyperlink_to(t, target);
                    }
                }
            }
        }
    });
    clicked
}

// ---------------------------------------------------------------- lecture

fn parse(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines = src.lines().peekable();
    // Paragraphe ou élément de liste en cours, à compléter par les lignes suivantes.
    let mut para: Option<(Option<Option<String>>, String)> = None;
    let flush = |para: &mut Option<(Option<Option<String>>, String)>, blocks: &mut Vec<Block>| {
        if let Some((marker, text)) = para.take() {
            let spans = parse_inline(&text);
            blocks.push(match marker {
                Some(m) => Block::Item(m, spans),
                None => Block::Paragraph(spans),
            });
        }
    };
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut para, &mut blocks);
        } else if trimmed.starts_with("```") {
            flush(&mut para, &mut blocks);
            let mut code = Vec::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
                code.push(l);
            }
            blocks.push(Block::Code(code.join("\n")));
        } else if let Some((level, title)) = heading(trimmed) {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Heading(level, title.to_string()));
        } else if trimmed.starts_with('|') {
            flush(&mut para, &mut blocks);
            let mut rows = vec![table_row(trimmed)];
            while let Some(l) = lines.peek().map(|l| l.trim()).filter(|l| l.starts_with('|')) {
                if !l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                    rows.push(table_row(l));
                }
                lines.next();
            }
            blocks.push(Block::Table(rows));
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            flush(&mut para, &mut blocks);
            para = Some((Some(None), rest.to_string()));
        } else if let Some((num, rest)) = numbered(trimmed) {
            flush(&mut para, &mut blocks);
            para = Some((Some(Some(num)), rest.to_string()));
        } else if let Some((_, text)) = para.as_mut() {
            text.push(' ');
            text.push_str(trimmed);
        } else {
            para = Some((None, trimmed.to_string()));
        }
    }
    flush(&mut para, &mut blocks);
    blocks
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    if (1..=4).contains(&level) {
        line[level..].strip_prefix(' ').map(|t| (level as u8, t.trim()))
    } else {
        None
    }
}

fn numbered(line: &str) -> Option<(String, &str)> {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    line[digits..].strip_prefix(". ").map(|rest| (line[..digits + 1].to_string(), rest))
}

fn table_row(line: &str) -> Vec<Vec<Span>> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| parse_inline(c.trim())).collect()
}

fn parse_inline(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    let push_plain = |plain: &mut String, spans: &mut Vec<Span>| {
        if !plain.is_empty() {
            spans.push(Span::Text(std::mem::take(plain)));
        }
    };
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                push_plain(&mut plain, &mut spans);
                spans.push(Span::Bold(after[..end].to_string()));
                rest = &after[end + 2..];
                continue;
            }
        } else if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                push_plain(&mut plain, &mut spans);
                spans.push(Span::Code(after[..end].to_string()));
                rest = &after[end + 1..];
                continue;
            }
        } else if let Some(after) = rest.strip_prefix('[') {
            if let Some((label, tail)) = after.split_once("](") {
                if let Some((target, tail)) = tail.split_once(')') {
                    push_plain(&mut plain, &mut spans);
                    spans.push(Span::Link(label.to_string(), target.to_string()));
                    rest = tail;
                    continue;
                }
            }
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    push_plain(&mut plain, &mut spans);
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_formatting() {
        let spans = parse_inline("Clic **droit** sur `/stop` puis [voir](#osc).");
        assert_eq!(
            spans,
            vec![
                Span::Text("Clic ".into()),
                Span::Bold("droit".into()),
                Span::Text(" sur ".into()),
                Span::Code("/stop".into()),
                Span::Text(" puis ".into()),
                Span::Link("voir".into(), "#osc".into()),
                Span::Text(".".into()),
            ]
        );
    }

    #[test]
    fn blocks_and_lists() {
        let blocks = parse("# Titre\n\nUn\nparagraphe.\n\n- puce\n  suite\n1. un\n\n| A | B |\n|---|---|\n| x | y |\n\n```\ncode\n```\n");
        assert_eq!(blocks[0], Block::Heading(1, "Titre".into()));
        assert_eq!(blocks[1], Block::Paragraph(vec![Span::Text("Un paragraphe.".into())]));
        assert_eq!(blocks[2], Block::Item(None, vec![Span::Text("puce suite".into())]));
        assert_eq!(blocks[3], Block::Item(Some("1.".into()), vec![Span::Text("un".into())]));
        assert!(matches!(&blocks[4], Block::Table(rows) if rows.len() == 2 && rows[1].len() == 2));
        assert_eq!(blocks[5], Block::Code("code".into()));
    }

    /// Les liens internes des deux manuels mènent tous à un titre.
    #[test]
    fn manual_links_resolve() {
        for src in [AIDE_FR, HELP_EN] {
            let blocks = parse(src);
            for b in &blocks {
                let spans: &[Span] = match b {
                    Block::Paragraph(s) | Block::Item(_, s) => s,
                    _ => continue,
                };
                for s in spans {
                    if let Span::Link(_, target) = s {
                        assert!(find_anchor(&blocks, target).is_some(), "lien sans titre : {target}");
                    }
                }
            }
        }
    }
}
