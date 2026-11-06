//! Module Texte : la conduite écrite, sauvegardée avec le projet.
//!
//! Le texte reste une simple chaîne avec un balisage léger, lisible même hors de
//! l'application (et compatible avec les conduites existantes, sans balise) :
//!
//! - `**gras**`, `*italique*`, `__souligné__`, `~~barré~~` ;
//! - surlignage : `{rouge}…{/rouge}`, `{bleu}…{/bleu}`, `{vert}…{/vert}`, `{rose}…{/rose}`.
//!
//! Les balises ne valent que sur une ligne. `*`, `**`, `__` et `~~` ne comptent comme
//! balises que collés au texte (`**mot**`, pas `5 * 3` ni une puce `* point`) et
//! refermés plus loin sur la même ligne ; sinon ils restent du texte ordinaire.
//! À l'affichage, les balises sont dessinées petites et grisées.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{vec2, Color32, FontFamily, FontId, Id, Key, Modifiers, Rect, RichText, Stroke};

use super::dialogs::{Kind, Pick};
use super::theme;
use super::JingleStarApp;
use star_core::i18n::tr;

/// Taille du texte de la conduite.
const TEXT_SIZE: f32 = 15.0;
/// Zoom du texte : bornes et pas des boutons − / +.
const ZOOM_MIN: f32 = 0.6;
const ZOOM_MAX: f32 = 3.0;
const ZOOM_STEP: f32 = 0.2;
/// Taille des balises : minuscules et transparentes, on ne voit que la mise en forme.
const MARKER_SIZE: f32 = 0.5;
/// Famille de police grasse, chargée depuis les polices du système (la police par défaut d'egui n'a pas de gras).
const BOLD_FAMILY: &str = "notes_bold";

/// Couleur de surlignage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hl {
    Rouge,
    Bleu,
    Vert,
    Rose,
}

impl Hl {
    pub(crate) const ALL: [Hl; 4] = [Hl::Rouge, Hl::Bleu, Hl::Vert, Hl::Rose];

    /// Nom de la balise (`{rouge}` … `{/rouge}`), le même quelle que soit la langue.
    fn tag(self) -> &'static str {
        match self {
            Hl::Rouge => "rouge",
            Hl::Bleu => "bleu",
            Hl::Vert => "vert",
            Hl::Rose => "rose",
        }
    }

    /// Fond de surlignage : assez soutenu pour le thème sombre, le texte clair reste lisible dessus.
    fn color(self) -> Color32 {
        match self {
            Hl::Rouge => Color32::from_rgb(0x9a, 0x2c, 0x2c),
            Hl::Bleu => Color32::from_rgb(0x2a, 0x58, 0xa6),
            Hl::Vert => Color32::from_rgb(0x2a, 0x7a, 0x3a),
            Hl::Rose => Color32::from_rgb(0xa8, 0x36, 0x84),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Hl::Rouge => tr("Surligner en rouge", "Highlight in red"),
            Hl::Bleu => tr("Surligner en bleu", "Highlight in blue"),
            Hl::Vert => tr("Surligner en vert", "Highlight in green"),
            Hl::Rose => tr("Surligner en rose", "Highlight in pink"),
        }
    }

    fn open(self) -> String {
        format!("{{{}}}", self.tag())
    }

    fn close(self) -> String {
        format!("{{/{}}}", self.tag())
    }
}

/// Mise en forme simple (gras, italique, souligné, barré).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flag {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl Flag {
    const PARSE_ORDER: [Flag; 4] = [Flag::Bold, Flag::Underline, Flag::Strike, Flag::Italic];

    /// Nom de la balise : `{b}` … `{/b}`. Des balises entre accolades, invisibles à l'écran,
    /// ne se confondent pas entre elles (contrairement à `*` et `**`).
    fn tag(self) -> &'static str {
        match self {
            Flag::Bold => "b",
            Flag::Italic => "i",
            Flag::Underline => "u",
            Flag::Strike => "s",
        }
    }

    fn open(self) -> String {
        format!("{{{}}}", self.tag())
    }

    fn close(self) -> String {
        format!("{{/{}}}", self.tag())
    }
}

/// Style d'un caractère de la conduite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub hl: Option<Hl>,
}

impl Style {
    fn get(&self, f: Flag) -> bool {
        match f {
            Flag::Bold => self.bold,
            Flag::Italic => self.italic,
            Flag::Underline => self.underline,
            Flag::Strike => self.strike,
        }
    }

    fn set(&mut self, f: Flag, on: bool) {
        match f {
            Flag::Bold => self.bold = on,
            Flag::Italic => self.italic = on,
            Flag::Underline => self.underline = on,
            Flag::Strike => self.strike = on,
        }
    }

    fn with(mut self, f: Flag, on: bool) -> Self {
        self.set(f, on);
        self
    }
}

/// Un morceau du texte : du texte stylé, ou une balise (`marker`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    /// Plage en octets dans le texte complet.
    pub range: Range<usize>,
    pub style: Style,
    pub marker: bool,
}

/// Découpe tout le texte en morceaux stylés et balises.
pub(crate) fn parse(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut off = 0;
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        parse_line(line, off, &mut out);
        off += line.len();
        if lines.peek().is_some() {
            out.push(Span { range: off..off + 1, style: Style::default(), marker: false });
            off += 1;
        }
    }
    out
}

fn parse_line(line: &str, base: usize, out: &mut Vec<Span>) {
    let mut style = Style::default();
    let mut i = 0;
    let mut start = 0;
    while i < line.len() {
        if let Some((len, next)) = marker_at(line, i, style) {
            if start < i {
                out.push(Span { range: base + start..base + i, style, marker: false });
            }
            // La balise garde le fond du surlignage s'il continue de part et d'autre.
            let hl = if style.hl == next.hl { style.hl } else { None };
            out.push(Span { range: base + i..base + i + len, style: Style { hl, ..Style::default() }, marker: true });
            style = next;
            i += len;
            start = i;
        } else {
            i += line[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    if start < line.len() {
        out.push(Span { range: base + start..base + line.len(), style, marker: false });
    }
}

/// Une balise commence-t-elle à l'octet `i` de la ligne ? Renvoie sa longueur et le style qui suit.
fn marker_at(line: &str, i: usize, s: Style) -> Option<(usize, Style)> {
    let rest = &line[i..];

    // Fermetures d'abord : `***` referme le gras puis l'italique.
    if let Some(h) = s.hl {
        let close = h.close();
        if rest.starts_with(&close) {
            return Some((close.len(), Style { hl: None, ..s }));
        }
    }
    for f in Flag::PARSE_ORDER {
        let close = f.close();
        if s.get(f) && rest.starts_with(&close) {
            return Some((close.len(), s.with(f, false)));
        }
    }

    // Ouvertures.
    for h in Hl::ALL {
        let open = h.open();
        if rest.starts_with(&open) {
            return Some((open.len(), Style { hl: Some(h), ..s }));
        }
    }
    for f in Flag::PARSE_ORDER {
        let open = f.open();
        if !s.get(f) && rest.starts_with(&open) {
            return Some((open.len(), s.with(f, true)));
        }
    }
    None
}

/// Styles communs à tout le texte sélectionné (plage en caractères) ; sans sélection, ceux
/// du texte juste avant le curseur. Sert à allumer les boutons de la barre.
pub(crate) fn active_style(text: &str, sel: Range<usize>) -> Style {
    let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(b, _)| b);
    let (a, b) = (byte(sel.start), byte(sel.end));
    let spans: Vec<Span> = parse(text).into_iter().filter(|sp| !sp.marker).collect();
    if a == b {
        return spans.iter().filter(|sp| sp.range.start < a).last().map(|sp| sp.style).unwrap_or_default();
    }
    let mut common: Option<Style> = None;
    for sp in &spans {
        let (s0, s1) = (sp.range.start.max(a), sp.range.end.min(b));
        if s0 >= s1 || text[s0..s1].trim().is_empty() {
            continue;
        }
        let st = sp.style;
        common = Some(match common {
            None => st,
            Some(c) => Style {
                bold: c.bold && st.bold,
                italic: c.italic && st.italic,
                underline: c.underline && st.underline,
                strike: c.strike && st.strike,
                hl: if c.hl == st.hl { c.hl } else { None },
            },
        });
    }
    common.unwrap_or_default()
}

/// Mise en page du texte pour le `TextEdit` : styles appliqués, balises invisibles.
fn layout_job(text: &str, text_color: Color32, bold_font: bool, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let regular = FontId::proportional(size);
    for span in parse(text) {
        let piece = &text[span.range.clone()];
        let st = span.style;
        let background = st.hl.map_or(Color32::TRANSPARENT, Hl::color);
        let format = if span.marker {
            TextFormat {
                font_id: FontId::proportional(MARKER_SIZE),
                color: Color32::TRANSPARENT,
                background,
                ..Default::default()
            }
        } else {
            let font_id = if st.bold && bold_font {
                FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()))
            } else {
                regular.clone()
            };
            // Sans police grasse disponible, le gras ressort en blanc pur.
            let color = if st.hl.is_some() || (st.bold && !bold_font) { Color32::WHITE } else { text_color };
            let line = Stroke::new(1.0, color);
            TextFormat {
                font_id,
                color,
                background,
                italics: st.italic,
                underline: if st.underline { line } else { Stroke::NONE },
                strikethrough: if st.strike { line } else { Stroke::NONE },
                ..Default::default()
            }
        };
        job.append(piece, 0.0, format);
    }
    job
}

/// Action de la barre d'outils.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Toggle(Flag),
    Color(Hl),
}

/// Texte d'une ligne sans ses balises (caractère + style), et pour chaque position de
/// caractère de la ligne (fin comprise), la position correspondante dans ce contenu.
fn line_content(line: &str) -> (Vec<(char, Style)>, Vec<usize>) {
    let mut spans = Vec::new();
    parse_line(line, 0, &mut spans);
    let mut content = Vec::new();
    let mut pos_map = Vec::new();
    for span in spans {
        for c in line[span.range].chars() {
            pos_map.push(content.len());
            if !span.marker {
                content.push((c, span.style));
            }
        }
    }
    pos_map.push(content.len());
    (content, pos_map)
}

/// Retire gras/italique/souligné/barré des espaces en bord de zone : les balises doivent
/// être collées au texte pour être reconnues.
fn normalize(content: &mut [(char, Style)]) {
    for f in [Flag::Bold, Flag::Italic, Flag::Underline, Flag::Strike] {
        let mut k = 0;
        while k < content.len() {
            if !content[k].1.get(f) {
                k += 1;
                continue;
            }
            let start = k;
            while k < content.len() && content[k].1.get(f) {
                k += 1;
            }
            let mut a = start;
            while a < k && content[a].0.is_whitespace() {
                content[a].1.set(f, false);
                a += 1;
            }
            let mut b = k;
            while b > a && content[b - 1].0.is_whitespace() {
                content[b - 1].1.set(f, false);
                b -= 1;
            }
        }
    }
}

/// Balises pour passer du style `a` au style `b`. Renvoie le nombre de caractères écrits.
fn transitions(a: Style, b: Style, out: &mut String) -> usize {
    let before = out.len();
    if a.hl != b.hl {
        if let Some(h) = a.hl {
            out.push_str(&h.close());
        }
    }
    for f in [Flag::Strike, Flag::Underline, Flag::Italic, Flag::Bold] {
        if a.get(f) && !b.get(f) {
            out.push_str(&f.close());
        }
    }
    for f in [Flag::Bold, Flag::Italic, Flag::Underline, Flag::Strike] {
        if !a.get(f) && b.get(f) {
            out.push_str(&f.open());
        }
    }
    if a.hl != b.hl {
        if let Some(h) = b.hl {
            out.push_str(&h.open());
        }
    }
    // Les balises sont en ASCII : octets = caractères.
    out.len() - before
}

/// Réécrit une ligne à partir de son contenu stylé. Renvoie la ligne, et pour chaque
/// position du contenu, la position (en caractères) juste après les balises ouvrantes
/// (`starts`) et juste avant les balises fermantes (`ends`).
fn serialize(content: &[(char, Style)]) -> (String, Vec<usize>, Vec<usize>) {
    let mut out = String::new();
    let mut n = 0;
    let mut cur = Style::default();
    let mut starts = Vec::with_capacity(content.len() + 1);
    let mut ends = Vec::with_capacity(content.len() + 1);
    for &(c, st) in content {
        ends.push(n);
        n += transitions(cur, st, &mut out);
        cur = st;
        starts.push(n);
        out.push(c);
        n += 1;
    }
    ends.push(n);
    starts.push(n);
    transitions(cur, Style::default(), &mut out);
    (out, starts, ends)
}

/// Nombre de caractères visibles (hors balises) avant la position `raw` (en caractères).
fn visible_before(text: &str, raw: usize) -> usize {
    let spans = parse(text);
    let mut n = 0;
    for (k, (b, _)) in text.char_indices().enumerate() {
        if k >= raw {
            break;
        }
        if !spans.iter().any(|sp| sp.marker && sp.range.contains(&b)) {
            n += 1;
        }
    }
    n
}

/// Position (en caractères) juste après le `n`-ième caractère visible et les balises qui le suivent
/// côté ouvrant : le curseur tombe à l'intérieur du style.
fn raw_of_visible(text: &str, n: usize) -> usize {
    let spans = parse(text);
    let is_marker = |b: usize| spans.iter().any(|sp| sp.marker && sp.range.contains(&b));
    let mut seen = 0;
    for (k, (b, _)) in text.char_indices().enumerate() {
        if is_marker(b) {
            continue;
        }
        if seen == n {
            return k;
        }
        seen += 1;
    }
    text.chars().count()
}

/// Applique une action de mise en forme à la sélection `sel` (en caractères).
/// Renvoie le nouveau texte et la nouvelle sélection.
pub(crate) fn apply_format(text: &str, sel: Range<usize>, action: Action) -> (String, Range<usize>) {
    let total = text.chars().count();
    let sel = sel.start.min(total)..sel.end.min(total);

    if sel.is_empty() {
        // Sans sélection : le mot sous le curseur (des balises vides resteraient visibles).
        let chars: Vec<char> = text.chars().collect();
        let start = chars[..sel.start].iter().rposition(|c| c.is_whitespace()).map_or(0, |p| p + 1);
        let end = chars[sel.start..].iter().position(|c| c.is_whitespace()).map_or(total, |p| sel.start + p);
        if start == end {
            return (text.to_owned(), sel);
        }
        let (out, _) = apply_format(text, start..end, action);
        // Le curseur reste au même endroit du texte visible.
        let cursor = raw_of_visible(&out, visible_before(text, sel.start));
        return (out, cursor..cursor);
    }

    // Lignes touchées par la sélection : contenu sans balises et plage sélectionnée dans ce contenu.
    let lines: Vec<&str> = text.split('\n').collect();
    let mut touched: Vec<Option<(Vec<(char, Style)>, Range<usize>)>> = Vec::with_capacity(lines.len());
    let mut ls = 0;
    for line in &lines {
        let le = ls + line.chars().count();
        let a = sel.start.max(ls);
        let b = sel.end.min(le);
        if a < b {
            let (content, pos_map) = line_content(line);
            touched.push(Some((content, pos_map[a - ls]..pos_map[b - ls])));
        } else {
            touched.push(None);
        }
        ls = le + 1;
    }

    // Bascule : si tout le texte sélectionné a déjà ce style, on l'enlève.
    let mut selected = touched.iter().flatten().flat_map(|(c, r)| c[r.clone()].iter()).filter(|(c, _)| !c.is_whitespace());
    let all = match action {
        Action::Toggle(f) => selected.all(|(_, st)| st.get(f)),
        Action::Color(h) => selected.all(|(_, st)| st.hl == Some(h)),
    };
    let new_style = |st: Style| -> Style {
        match action {
            Action::Toggle(f) => st.with(f, !all),
            Action::Color(h) => Style { hl: if all { None } else { Some(h) }, ..st },
        }
    };

    let mut out = String::with_capacity(text.len() + 16);
    let mut new_sel: Option<Range<usize>> = None;
    let mut chars_out = 0;
    for (k, (line, t)) in lines.iter().zip(touched).enumerate() {
        if k > 0 {
            out.push('\n');
            chars_out += 1;
        }
        match t {
            None => {
                out.push_str(line);
                chars_out += line.chars().count();
            }
            Some((mut content, range)) => {
                for item in &mut content[range.clone()] {
                    item.1 = new_style(item.1);
                }
                normalize(&mut content);
                let (new_line, starts, ends) = serialize(&content);
                let start = chars_out + starts[range.start];
                let end = (chars_out + ends[range.end]).max(start);
                new_sel = Some(match new_sel {
                    None => start..end,
                    Some(r) => r.start..end,
                });
                chars_out += new_line.chars().count();
                out.push_str(&new_line);
            }
        }
    }
    (out, new_sel.unwrap_or(sel))
}

/// Ajoute un texte importé à la suite de la conduite, séparé par une ligne vide.
pub(crate) fn append_imported(existing: &str, imported: &str) -> String {
    if existing.trim().is_empty() {
        return imported.to_owned();
    }
    let mut out = existing.to_owned();
    while !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(imported);
    out
}

/// Décode un fichier texte : UTF-8 (BOM retiré), sinon Windows-1252 (anciens fichiers Windows).
/// Les fins de ligne sont ramenées à `\n`.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    };
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', //
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Police grasse du système, lue une seule fois.
fn bold_font_bytes() -> Option<&'static [u8]> {
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    BYTES
        .get_or_init(|| {
            let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
            let candidates = [
                format!("{windir}\\Fonts\\segoeuib.ttf"),
                format!("{windir}\\Fonts\\arialbd.ttf"),
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf".into(),
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".into(),
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf".into(),
            ];
            candidates.iter().find_map(|p| std::fs::read(p).ok())
        })
        .as_deref()
}

/// Installe la police grasse si besoin. Vrai si elle est utilisable dès cette image.
fn ensure_bold_font(ctx: &egui::Context) -> bool {
    let family = FontFamily::Name(BOLD_FAMILY.into());
    if ctx.fonts(|f| f.families().contains(&family)) {
        return true;
    }
    if let Some(bytes) = bold_font_bytes() {
        let mut defs = ctx.fonts(|f| f.definitions().clone());
        defs.font_data.insert(BOLD_FAMILY.into(), Arc::new(egui::FontData::from_static(bytes)));
        // Repli sur la police habituelle pour les caractères absents (émojis…).
        let mut chain = vec![BOLD_FAMILY.to_owned()];
        chain.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
        defs.families.insert(family, chain);
        ctx.set_fonts(defs);
    }
    false
}

impl JingleStarApp {
    /// Module Texte : une barre de mise en forme et un grand champ libre pour la conduite,
    /// sauvegardé avec le projet.
    pub(super) fn notes_view(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        let bold_font = ensure_bold_font(ui.ctx());
        let edit_id = Id::unique(("notes_edit", m));
        // Dernière sélection connue du champ : un clic sur la barre lui fait perdre le focus
        // (et egui réduit alors la sélection), on réutilise donc celle mémorisée.
        let sel_id = edit_id.with("selection");
        let had_focus = ui.memory(|mem| mem.has_focus(edit_id));

        let mut action = None;
        if had_focus {
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::COMMAND, Key::B) {
                    action = Some(Action::Toggle(Flag::Bold));
                } else if i.consume_key(Modifiers::COMMAND, Key::I) {
                    action = Some(Action::Toggle(Flag::Italic));
                } else if i.consume_key(Modifiers::COMMAND, Key::U) {
                    action = Some(Action::Toggle(Flag::Underline));
                }
            });
        }

        // Styles de la sélection (ou du curseur) : leurs boutons s'allument dans la barre.
        let cur_sel = if had_focus { egui::TextEdit::load_state(ui.ctx(), edit_id).and_then(|s| s.cursor.char_range()) } else { None }
            .map(|r| {
                let r = r.as_sorted_char_range();
                (r.start.0, r.end.0)
            })
            .or_else(|| ui.data(|d| d.get_temp::<(usize, usize)>(sel_id)));
        let on = cur_sel.map(|(a, b)| active_style(&self.ctrl.project.modules[m].notes, a..b)).unwrap_or_default();

        // Zoom enregistré dans le projet ; une valeur illisible (fichier modifié à la main) revient à 100 %.
        let saved = self.ctrl.project.modules[m].notes_zoom;
        let mut zoom = if saved.is_finite() { saved.clamp(ZOOM_MIN, ZOOM_MAX) } else { 1.0 };

        let top = ui.cursor().top();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let size = vec2(24.0, 22.0);
            let mut tool = |ui: &mut egui::Ui, label: RichText, tip: &str, a: Action, active: bool| {
                let mut b = egui::Button::new(label).min_size(size).selected(active);
                if active {
                    b = b.stroke(egui::Stroke::new(2.0, Color32::WHITE));
                }
                if ui.add(b).on_hover_text(tip).clicked() {
                    action = Some(a);
                }
            };
            tool(ui, RichText::new("G").strong(), tr("Gras (Ctrl+B)", "Bold (Ctrl+B)"), Action::Toggle(Flag::Bold), on.bold);
            tool(ui, RichText::new("I").italics(), tr("Italique (Ctrl+I)", "Italic (Ctrl+I)"), Action::Toggle(Flag::Italic), on.italic);
            tool(ui, RichText::new("S").underline(), tr("Souligné (Ctrl+U)", "Underline (Ctrl+U)"), Action::Toggle(Flag::Underline), on.underline);
            tool(ui, RichText::new("B").strikethrough(), tr("Barré", "Strikethrough"), Action::Toggle(Flag::Strike), on.strike);
            ui.add_space(6.0);
            for h in Hl::ALL {
                tool(ui, RichText::new("  ").background_color(h.color()), h.label(), Action::Color(h), on.hl == Some(h));
            }
            ui.add_space(10.0);
            // Zoom du texte : pour lire la conduite de loin.
            if ui.add(egui::Button::new("−").min_size(size)).on_hover_text(tr("Texte plus petit", "Smaller text")).clicked() {
                zoom = (zoom - ZOOM_STEP).max(ZOOM_MIN);
            }
            ui.label(RichText::new(format!("{:.0} %", zoom * 100.0)).small().color(theme::TEXT_DIM));
            if ui.add(egui::Button::new("+").min_size(size)).on_hover_text(tr("Texte plus grand", "Larger text")).clicked() {
                zoom = (zoom + ZOOM_STEP).min(ZOOM_MAX);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(tr("Importer…", "Import…"))
                    .on_hover_text(tr("Ajouter un fichier texte (.txt, .md) à la suite", "Append a text file (.txt, .md)"))
                    .clicked()
                {
                    self.dialogs.show(ui.ctx(), Pick::NotesImport(m), Kind::Text);
                }
            });
        });

        if let Some(action) = action {
            let text = self.ctrl.project.modules[m].notes.clone();
            let total = text.chars().count();
            let live = if had_focus { egui::TextEdit::load_state(ui.ctx(), edit_id).and_then(|s| s.cursor.char_range()) } else { None };
            let sel = live
                .map(|r| {
                    let r = r.as_sorted_char_range();
                    r.start.0..r.end.0
                })
                .or_else(|| ui.data(|d| d.get_temp::<(usize, usize)>(sel_id)).map(|(a, b)| a..b))
                .unwrap_or(total..total);
            let (new_text, new_sel) = apply_format(&text, sel, action);
            if new_text != text {
                self.ctrl.set_module_notes(m, new_text);
            }
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(new_sel.start), CCursor::new(new_sel.end))));
            egui::TextEdit::store_state(ui.ctx(), edit_id, state);
            ui.data_mut(|d| d.insert_temp(sel_id, (new_sel.start, new_sel.end)));
            ui.memory_mut(|mem| mem.request_focus(edit_id));
        }

        if zoom != saved {
            self.ctrl.set_notes_zoom(m, zoom);
        }
        let text_size = TEXT_SIZE * zoom;
        let height = (area.height() - (ui.cursor().top() - top)).max(40.0);
        let mut text = self.ctrl.project.modules[m].notes.clone();
        let mut focused = false;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = layout_job(buf.as_str(), ui.visuals().text_color(), bold_font, text_size);
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        egui::ScrollArea::vertical().id_salt(("notes", m)).auto_shrink([false, false]).max_height(height).show(ui, |ui| {
            let resp = ui.add_sized(
                vec2(area.width(), height),
                egui::TextEdit::multiline(&mut text)
                    .id(edit_id)
                    .font(FontId::proportional(text_size))
                    .hint_text(tr("Écrire la conduite ici…", "Write your running order here…"))
                    .desired_width(f32::INFINITY)
                    .lock_focus(true)
                    .layouter(&mut layouter),
            );
            focused = resp.has_focus();
            if resp.changed() {
                self.ctrl.set_module_notes(m, text);
            }
        });
        if focused {
            if let Some(r) = egui::TextEdit::load_state(ui.ctx(), edit_id).and_then(|s| s.cursor.char_range()) {
                let r = r.as_sorted_char_range();
                ui.data_mut(|d| d.insert_temp(sel_id, (r.start.0, r.end.0)));
            }
        }
        self.notes_focused |= focused;
    }

    /// Importe un fichier texte (.txt, .md) à la suite de la conduite.
    pub(super) fn import_notes(&mut self, m: usize, path: &std::path::Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(path) {
            Ok(bytes) => {
                let imported = decode_text(&bytes);
                let notes = append_imported(&self.ctrl.project.modules[m].notes, &imported);
                self.ctrl.set_module_notes(m, notes);
                self.ctrl.status = Some(format!("{} : {name}", tr("Texte importé", "Text imported")));
            }
            Err(e) => {
                let msg = format!("{} « {name} » : {e}", tr("Import du texte impossible", "Could not import text"));
                star_core::log::write(&msg);
                self.ctrl.status = Some(msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Caractères visibles (balises retirées) avec leur style.
    fn visible(text: &str) -> Vec<(char, Style)> {
        parse(text)
            .into_iter()
            .filter(|s| !s.marker)
            .flat_map(|s| text[s.range].chars().map(move |c| (c, s.style)).collect::<Vec<_>>())
            .collect()
    }

    fn plain(text: &str) -> String {
        visible(text).into_iter().map(|(c, _)| c).collect()
    }

    #[test]
    fn aucune_balise_visible_apres_deux_actions() {
        let acts = [
            Action::Toggle(Flag::Bold),
            Action::Toggle(Flag::Italic),
            Action::Toggle(Flag::Underline),
            Action::Toggle(Flag::Strike),
            Action::Color(Hl::Rouge),
            Action::Color(Hl::Bleu),
        ];
        let base = "ab cd ef";
        let n = base.chars().count();
        for a1 in acts {
            for s1 in 0..=n {
                for e1 in s1..=n {
                    let (t1, _) = apply_format(base, s1..e1, a1);
                    assert_eq!(plain(&t1), base, "{a1:?} [{s1},{e1}] -> {t1:?}");
                    let n1 = t1.chars().count();
                    for a2 in acts {
                        for s2 in 0..=n1 {
                            for e2 in (s2..=n1).step_by(3) {
                                let (t2, _) = apply_format(&t1, s2..e2, a2);
                                assert_eq!(plain(&t2), base, "{t1:?} puis {a2:?} [{s2},{e2}] -> {t2:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn active_style_follows_selection_and_cursor() {
        let t = "a {b}gras{/b} {rouge}x{/rouge}";
        // « gras » sélectionné.
        let g = t.find("gras").unwrap();
        assert!(active_style(t, g..g + 4).bold);
        // Sélection à cheval sur du texte normal : pas de gras commun.
        assert!(!active_style(t, 0..g + 4).bold);
        // Curseur juste après « x » surligné.
        let x = t.find("x{/").unwrap() + 1;
        assert_eq!(active_style(t, x..x).hl, Some(Hl::Rouge));
    }

    #[test]
    fn texte_sans_balise_inchange() {
        let t = "Ouverture\n* point de liste\n5 * 3 = 15\n**gras** snake_case et a~b {autre}";
        assert_eq!(plain(t), t);
        assert!(visible(t).iter().all(|(_, st)| *st == Style::default()));
        assert!(parse(t).iter().all(|s| !s.marker));
    }

    #[test]
    fn styles_de_base() {
        let t = "{b}g{/b} {i}i{/i} {u}s{/u} {s}x{/s}";
        let v = visible(t);
        let style_of = |c: char| v.iter().find(|(x, _)| *x == c).unwrap().1;
        assert!(style_of('g').bold);
        assert!(style_of('i').italic && !style_of('i').bold);
        assert!(style_of('s').underline);
        assert!(style_of('x').strike);
        assert_eq!(plain(t), "g i s x");
    }

    #[test]
    fn gras_italique_combines() {
        let (t, _) = apply_format("fort", 0..4, Action::Toggle(Flag::Bold));
        let (t, _) = apply_format(&t, 0..t.chars().count(), Action::Toggle(Flag::Italic));
        let v = visible(&t);
        assert_eq!(v.len(), 4);
        assert!(v.iter().all(|(_, st)| st.bold && st.italic));
    }

    #[test]
    fn surlignage() {
        let t = "avant {rouge}chaud {b}très{/b}{/rouge} après";
        assert_eq!(plain(t), "avant chaud très après");
        let v = visible(t);
        assert_eq!(v[6].1.hl, Some(Hl::Rouge));
        assert!(v[12].1.bold && v[12].1.hl == Some(Hl::Rouge));
        assert_eq!(v.last().unwrap().1.hl, None);
    }

    #[test]
    fn balises_limitees_a_la_ligne() {
        let v = visible("{b}début\nfin{/b}");
        assert!(v[0].1.bold);
        assert!(!v.last().unwrap().1.bold);
        let v = visible("{bleu}a\nb");
        assert_eq!(v[0].1.hl, Some(Hl::Bleu));
        assert_eq!(v[2].1.hl, None);
    }

    #[test]
    fn appliquer_et_retirer_le_gras() {
        let (t, sel) = apply_format("un mot ici", 3..6, Action::Toggle(Flag::Bold));
        assert_eq!(t, "un {b}mot{/b} ici");
        assert_eq!(sel, 6..9);
        let (t, _) = apply_format(&t, sel, Action::Toggle(Flag::Bold));
        assert_eq!(t, "un mot ici");
    }

    #[test]
    fn espaces_hors_des_balises() {
        let (t, _) = apply_format("un mot ici", 2..7, Action::Toggle(Flag::Italic));
        assert_eq!(t, "un {i}mot{/i} ici");
    }

    #[test]
    fn selection_sur_plusieurs_lignes() {
        let (t, _) = apply_format("ligne un\nligne deux", 6..14, Action::Color(Hl::Vert));
        assert_eq!(t, "ligne {vert}un{/vert}\n{vert}ligne{/vert} deux");
        assert_eq!(plain(&t), "ligne un\nligne deux");
    }

    #[test]
    fn sans_selection_le_mot_du_curseur() {
        // Curseur au milieu de « mot » : le mot entier, curseur au même endroit du texte visible.
        let (t, sel) = apply_format("un mot ici", 4..4, Action::Toggle(Flag::Strike));
        assert_eq!(t, "un {s}mot{/s} ici");
        assert_eq!(visible_before(&t, sel.start), 4);
        // Curseur dans un blanc : rien ne change, aucune balise vide.
        let (t, _) = apply_format("a  b", 2..2, Action::Color(Hl::Rouge));
        assert_eq!(t, "a  b");
        let (t, _) = apply_format("", 0..0, Action::Color(Hl::Rouge));
        assert_eq!(t, "");
    }

    #[test]
    fn import_et_encodage() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFl\xC3\xA0\r\nfin"), "là\nfin");
        assert_eq!(decode_text(b"caf\xE9 \x80"), "café €");
        assert_eq!(append_imported("", "x"), "x");
        assert_eq!(append_imported("a", "x"), "a\n\nx");
        assert_eq!(append_imported("a\n", "x"), "a\n\nx");
    }
}
