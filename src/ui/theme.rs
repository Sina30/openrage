use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{PoisonError, RwLock};

use egui::{Color32, CornerRadius, Stroke, Style, Visuals};

use crate::state::{AppearancePrefs, ThemeChoice};

/// A colour the chosen theme decides.
///
/// ⚠ **Not a constant.** Each read resolves against the active palette; a
/// constant would leave the hundreds of `theme::TEXT_PRIMARY` reads drawing
/// light-grey text on a light theme's white.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Themed(Slot);

impl Themed {
    pub fn get(self) -> Color32 {
        ACTIVE.read().unwrap_or_else(PoisonError::into_inner)[self.0 as usize]
    }

    pub fn gamma_multiply(self, factor: f32) -> Color32 {
        self.get().gamma_multiply(factor)
    }

    pub fn linear_multiply(self, factor: f32) -> Color32 {
        self.get().linear_multiply(factor)
    }
}

impl From<Themed> for Color32 {
    fn from(themed: Themed) -> Self {
        themed.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    BgL0,
    BgL1,
    BgL2,
    BgL3,
    Border,
    TextPrimary,
    TextSecondary,
    TextStrong,
    Accent,
    AccentHover,
    AccentText,
    OnAccent,
    Success,
    Warning,
    Error,
    Info,
    SelectionBg,
    SyntaxComment,
    SyntaxString,
    SyntaxNumber,
    SyntaxKeyword,
    SyntaxTag,
    SyntaxAttr,
    SyntaxPunct,
    SearchMatchBg,
    SearchMatchActive,
    SearchMatchEdge,
    FoldGuide,
    FoldCollapsedBg,
}

const SLOTS: usize = Slot::FoldCollapsedBg as usize + 1;

type Palette = [Color32; SLOTS];

// ⚠ Every text slot reaches `AA` on the panels it is drawn on, which
// `every_text_colour_reads_at_4_5_to_1` checks; a colour changed here is
// judged there. The selection and the find matches are opaque so that the
// colour text sits on is known.
const DARK: Palette = [
    Color32::from_rgb(0x1a, 0x1c, 0x20),
    Color32::from_rgb(0x21, 0x24, 0x29),
    Color32::from_rgb(0x2a, 0x2d, 0x33),
    Color32::from_rgb(0x38, 0x3c, 0x44),
    Color32::from_rgb(0x38, 0x3c, 0x44),
    Color32::from_rgb(0xcc, 0xcc, 0xcc),
    Color32::from_rgb(0xaa, 0xaa, 0xaa),
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0x00, 0x78, 0xd4),
    Color32::from_rgb(0x1c, 0x86, 0xe0),
    Color32::from_rgb(0x68, 0xaf, 0xe5),
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0x4e, 0xc4, 0x69),
    Color32::from_rgb(0xcc, 0xa7, 0x00),
    Color32::from_rgb(0xf6, 0x87, 0x87),
    Color32::from_rgb(0x4e, 0xc4, 0xff),
    Color32::from_rgb(0x18, 0x3c, 0x59),
    Color32::from_rgb(0x88, 0xae, 0x78),
    Color32::from_rgb(0xd0, 0x96, 0x7e),
    Color32::from_rgb(0xb5, 0xce, 0xa8),
    Color32::from_rgb(0x6c, 0xa9, 0xdb),
    Color32::from_rgb(0x4e, 0xc9, 0xb0),
    Color32::from_rgb(0x9c, 0xdc, 0xfe),
    Color32::from_rgb(0xd4, 0xd4, 0xd4),
    Color32::from_rgb(0x36, 0x30, 0x12),
    Color32::from_rgb(0x3f, 0x36, 0x14),
    Color32::from_rgb(0xb8, 0x96, 0x2e),
    Color32::from_rgb(0x4a, 0x4a, 0x4a),
    Color32::from_rgb(0x3a, 0x3a, 0x3e),
];

const LIGHT: Palette = [
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0xf3, 0xf3, 0xf3),
    Color32::from_rgb(0xe8, 0xe8, 0xe8),
    Color32::from_rgb(0xd6, 0xd6, 0xd6),
    Color32::from_rgb(0xc8, 0xc8, 0xc8),
    Color32::from_rgb(0x1f, 0x1f, 0x1f),
    Color32::from_rgb(0x5c, 0x5c, 0x5c),
    Color32::from_rgb(0x00, 0x00, 0x00),
    Color32::from_rgb(0x00, 0x78, 0xd4),
    Color32::from_rgb(0x00, 0x5f, 0xb8),
    Color32::from_rgb(0x00, 0x5e, 0xa6),
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0x27, 0x69, 0x2a),
    Color32::from_rgb(0x7e, 0x54, 0x00),
    Color32::from_rgb(0xb2, 0x24, 0x24),
    Color32::from_rgb(0x03, 0x59, 0xbb),
    Color32::from_rgb(0xc2, 0xda, 0xed),
    Color32::from_rgb(0x00, 0x6d, 0x00),
    Color32::from_rgb(0xa3, 0x15, 0x15),
    Color32::from_rgb(0x07, 0x6a, 0x45),
    Color32::from_rgb(0x00, 0x00, 0xff),
    Color32::from_rgb(0x80, 0x00, 0x00),
    Color32::from_rgb(0xba, 0x0e, 0x1d),
    Color32::from_rgb(0x33, 0x33, 0x33),
    Color32::from_rgb(0xfb, 0xf0, 0xc2),
    Color32::from_rgb(0xf8, 0xe3, 0xa0),
    Color32::from_rgb(0xa6, 0x7c, 0x00),
    Color32::from_rgb(0xc0, 0xc0, 0xc0),
    Color32::from_rgb(0xe4, 0xe4, 0xea),
];

static ACTIVE: RwLock<Palette> = RwLock::new(DARK);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// WCAG AA's contrast ratio for text.
pub const AA: f32 = 4.5;
/// What a graphic that carries meaning, such as a kind's icon, needs against its background.
pub const GRAPHIC: f32 = 3.0;

fn luminance(c: Color32) -> f32 {
    let c = egui::Rgba::from(c);
    0.2126 * c.r() + 0.7152 * c.g() + 0.0722 * c.b()
}

/// WCAG 2's contrast ratio between two opaque colours, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `colour` moved toward `toward` until it reaches `min` on every surface, or `toward` when nothing short of it does.
///
/// ⚠ A hair over `min`, so that rounding to 8 bits cannot land a colour under it.
fn legible_on(colour: Color32, toward: Color32, surfaces: &[Color32], min: f32) -> Color32 {
    const STEPS: u8 = 32;
    (0..=STEPS)
        .map(|step| colour.lerp_to_gamma(toward, f32::from(step) / f32::from(STEPS)))
        .find(|c| surfaces.iter().all(|s| contrast(*c, *s) >= min + 0.05))
        .unwrap_or(toward)
}

/// The fill of a box drawn over the viewport, which text sits on: the panel's colour, nearly opaque, so that what it holds reads as it does on a panel.
pub fn overlay_bg() -> Color32 {
    let panel = BG_L1.get();
    Color32::from_rgba_unmultiplied(panel.r(), panel.g(), panel.b(), 235)
}

/// The opacity of a disabled row whose text must still reach [`AA`] on the highlight; egui's 0.5 fades primary text below that, which any other disabled control keeps.
pub fn disabled_alpha_under_highlight(dark: bool) -> f32 {
    if dark {
        0.82
    } else {
        0.88
    }
}

/// `colour` as it must be drawn to reach `min` ([`AA`] for text, [`GRAPHIC`] for an icon) on every panel and a selected row of the active theme: itself when it does, else lightened (dark theme) or darkened (light) until it does.
pub fn legible(colour: Color32, min: f32) -> Color32 {
    let palette = *ACTIVE.read().unwrap_or_else(PoisonError::into_inner);
    let surfaces = [
        Slot::BgL0,
        Slot::BgL1,
        Slot::BgL2,
        Slot::BgL3,
        Slot::SelectionBg,
    ]
    .map(|s| palette[s as usize]);
    legible_on(colour, toward_text(&palette), &surfaces, min)
}

/// `colour` as a mark that carries meaning must be drawn on a panel: [`legible`] at [`GRAPHIC`].
pub fn graphic(colour: Color32) -> Color32 {
    legible(colour, GRAPHIC)
}

/// The outline of a control that is nothing but its outline, a checkbox, a radio button or a place to drop: the border, as dark or as light as it takes to reach [`GRAPHIC`] on every panel.
///
/// ⚠ [`BORDER`] stays what separates panels: at 3 to 1 every divider would shout.
pub fn outline() -> Color32 {
    graphic(BORDER.get())
}

/// White in a dark theme and black in a light one: the way text is further from the panel.
fn toward_text(palette: &Palette) -> Color32 {
    if luminance(palette[Slot::BgL1 as usize]) < 0.5 {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// The text colour that reads on `fill`: white while it reaches [`AA`], else black.
fn on(fill: Color32) -> Color32 {
    if contrast(Color32::WHITE, fill) >= AA {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// Changes every time an appearance is applied.
///
/// ⚠ **Anything that caches laid-out text keys on this.** A galley bakes in
/// both its colours and its font's glyphs, so a cache that outlives a theme or
/// font change keeps drawing the old one.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

fn palette_for(appearance: &AppearancePrefs) -> Palette {
    let palette = match appearance.theme {
        ThemeChoice::Dark => DARK,
        ThemeChoice::Light => LIGHT,
    };
    if appearance.accent == AppearancePrefs::DEFAULT_ACCENT {
        return palette;
    }
    let [r, g, b] = appearance.accent;
    with_accent(palette, appearance.theme, Color32::from_rgb(r, g, b))
}

/// `palette` with the slots the accent decides: the accent, its hover, the text that reads on it, the text in its hue and the selection.
fn with_accent(mut palette: Palette, theme: ThemeChoice, accent: Color32) -> Palette {
    let (toward, tint) = match theme {
        ThemeChoice::Dark => (Color32::WHITE, DARK_SELECTION_TINT),
        ThemeChoice::Light => (Color32::BLACK, LIGHT_SELECTION_TINT),
    };
    palette[Slot::Accent as usize] = accent;
    palette[Slot::AccentHover as usize] = accent.lerp_to_gamma(toward, 0.12);
    palette[Slot::OnAccent as usize] = on(accent);
    let selection = selection_for(&palette, accent, tint);
    palette[Slot::SelectionBg as usize] = selection;
    let surfaces = [Slot::BgL0, Slot::BgL1, Slot::BgL2, Slot::BgL3].map(|s| palette[s as usize]);
    palette[Slot::AccentText as usize] =
        legible_on(accent, toward, &[&surfaces[..], &[selection]].concat(), AA);
    palette
}

const DARK_SELECTION_TINT: f32 = 0.28;
const LIGHT_SELECTION_TINT: f32 = 0.20;

/// The panel with `tint` of `accent` in it, no lighter (dark theme) or darker (light) than the default accent's selection.
///
/// ⚠ That bound is what keeps every text colour readable on a selection whatever accent the user picks: contrast depends on luminance, and the palette's own selection is the one the text colours were fitted to.
fn selection_for(palette: &Palette, accent: Color32, tint: f32) -> Color32 {
    let panel = palette[Slot::BgL1 as usize];
    let bound = luminance(palette[Slot::SelectionBg as usize]);
    let dark = luminance(panel) < 0.5;
    let within = |c: Color32| {
        if dark {
            luminance(c) <= bound
        } else {
            luminance(c) >= bound
        }
    };
    (0..=14_i16)
        .map(|step| panel.lerp_to_gamma(accent, (tint - 0.02 * f32::from(step)).max(0.0)))
        .find(|c| within(*c))
        .unwrap_or(panel)
}

pub const BG_L0: Themed = Themed(Slot::BgL0);
pub const BG_L1: Themed = Themed(Slot::BgL1);
pub const BG_L2: Themed = Themed(Slot::BgL2);
pub const BG_L3: Themed = Themed(Slot::BgL3);

pub const BORDER: Themed = Themed(Slot::Border);

pub const TEXT_PRIMARY: Themed = Themed(Slot::TextPrimary);
pub const TEXT_SECONDARY: Themed = Themed(Slot::TextSecondary);
/// What egui draws `strong()` text in: the brightest text, so black on the light theme's white.
pub const TEXT_STRONG: Themed = Themed(Slot::TextStrong);

/// ⚠ The same colour as [`TEXT_SECONDARY`]: below [`AA`] muted text is unreadable, and above it nothing is left between the two.
pub const TEXT_DISABLED: Themed = TEXT_SECONDARY;
pub const TEXT_DIM: Themed = TEXT_SECONDARY;

/// The accent as a fill, a stroke or a graphic; text in the accent's hue is [`ACCENT_TEXT`].
pub const ACCENT: Themed = Themed(Slot::Accent);
pub const ACCENT_HOVER: Themed = Themed(Slot::AccentHover);
/// The accent's hue, lightened (dark theme) or darkened (light) until it reads on every panel and on a selection.
pub const ACCENT_TEXT: Themed = Themed(Slot::AccentText);
/// Text on an [`ACCENT`] fill.
pub const ON_ACCENT: Themed = Themed(Slot::OnAccent);

pub const SUCCESS: Themed = Themed(Slot::Success);
pub const WARNING: Themed = Themed(Slot::Warning);
pub const ERROR: Themed = Themed(Slot::Error);

pub const INFO: Themed = Themed(Slot::Info);

pub const SELECTION_BG: Themed = Themed(Slot::SelectionBg);

pub const SYNTAX_COMMENT: Themed = Themed(Slot::SyntaxComment);
pub const SYNTAX_STRING: Themed = Themed(Slot::SyntaxString);
pub const SYNTAX_NUMBER: Themed = Themed(Slot::SyntaxNumber);
pub const SYNTAX_KEYWORD: Themed = Themed(Slot::SyntaxKeyword);
pub const SYNTAX_TAG: Themed = Themed(Slot::SyntaxTag);
pub const SYNTAX_ATTR: Themed = Themed(Slot::SyntaxAttr);
pub const SYNTAX_PUNCT: Themed = Themed(Slot::SyntaxPunct);

pub const SYNTAX_LEVEL_ERROR: Themed = ERROR;
pub const SYNTAX_LEVEL_WARN: Themed = WARNING;
pub const SYNTAX_LEVEL_INFO: Themed = INFO;

pub const SEARCH_MATCH_BG: Themed = Themed(Slot::SearchMatchBg);
pub const SEARCH_MATCH_ACTIVE: Themed = Themed(Slot::SearchMatchActive);
/// The outline every find match carries, so that a fill dark enough to read text on is still seen.
pub const SEARCH_MATCH_EDGE: Themed = Themed(Slot::SearchMatchEdge);

pub const FOLD_GUIDE: Themed = Themed(Slot::FoldGuide);
pub const FOLD_COLLAPSED_BG: Themed = Themed(Slot::FoldCollapsedBg);

pub const KIND_YPT: Color32 = Color32::from_rgb(0x4e, 0xc9, 0xb0);
pub const KIND_YTD: Color32 = Color32::from_rgb(0xc5, 0x86, 0xc0);
pub const KIND_YDR: Color32 = Color32::from_rgb(0x6a, 0xc8, 0x6a);
pub const KIND_YDD: Color32 = Color32::from_rgb(0x4f, 0xa8, 0x7c);
pub const KIND_YFT: Color32 = Color32::from_rgb(0x3d, 0xb8, 0xa0);
pub const KIND_YBN: Color32 = Color32::from_rgb(0xe6, 0x9b, 0x4a);
pub const KIND_YLD: Color32 = Color32::from_rgb(0x5a, 0xb8, 0xc8);
pub const KIND_YCD: Color32 = Color32::from_rgb(0xa8, 0x8b, 0xe0);
pub const KIND_YED: Color32 = Color32::from_rgb(0xb4, 0x9a, 0xe0);
pub const KIND_YFD: Color32 = Color32::from_rgb(0x8f, 0xa8, 0xe0);
pub const KIND_YND: Color32 = Color32::from_rgb(0xd8, 0xc8, 0x5a);
pub const KIND_YNV: Color32 = Color32::from_rgb(0x9a, 0xc8, 0x5a);
pub const KIND_YPDB: Color32 = Color32::from_rgb(0x7f, 0xc4, 0xd8);
pub const KIND_FXC: Color32 = Color32::from_rgb(0xd8, 0xa0, 0x6a);
pub const KIND_RPF: Color32 = Color32::from_rgb(0xe6, 0xc6, 0x4a);
pub const KIND_GAME_ROOT: Color32 = Color32::from_rgb(0x6a, 0xa9, 0xe9);
pub const KIND_TEXT: Color32 = Color32::from_rgb(0x9c, 0xd1, 0xff);
pub const KIND_META: Color32 = Color32::from_rgb(0xc4, 0xa7, 0xe7);
pub const KIND_GXT2: Color32 = Color32::from_rgb(0x56, 0xc8, 0xa8);
pub const KIND_FOLDER: Color32 = Color32::from_rgb(0x73, 0x9c, 0xd7);
pub const KIND_UNKNOWN: Color32 = Color32::from_rgb(0x5a, 0x5a, 0x5a);

pub const STATE_DIRTY: Themed = WARNING;
pub const STATE_ERROR: Themed = ERROR;
pub const STATE_LOADING: Themed = INFO;
pub const STATE_RECOVERED: Themed = SUCCESS;
pub const STATE_NEW: Themed = SUCCESS;

pub const ICON_GUTTER: f32 = 4.0;

pub const SPACING_HAIR: f32 = 2.0;
pub const SPACING_TIGHT: f32 = 4.0;
pub const SPACING_SNUG: f32 = 6.0;
pub const SPACING_DEFAULT: f32 = 8.0;
pub const SPACING_LOOSE: f32 = 10.0;
pub const SPACING_RELAXED: f32 = 12.0;
pub const SPACING_WIDE: f32 = 16.0;
pub const SPACING_SECTION: f32 = 24.0;

pub const RADIUS_TINY: u8 = 3;
pub const RADIUS_SMALL: u8 = 4;
pub const RADIUS_MEDIUM: u8 = 6;
pub const RADIUS_LARGE: u8 = 8;

/// The size of `small()` text, in points.
pub const SMALL_TEXT: f32 = 10.5;

pub const TAB_STRIP_HEIGHT: f32 = 28.0;
pub const TAB_STRIP_BG: Themed = BG_L0;
pub const WORKSPACE_BUTTON_WIDTH: f32 = 88.0;
pub const TAB_STRIP_BUTTON_HEIGHT: f32 = 22.0;

pub fn workspace_button_fill(active: bool) -> Color32 {
    if active {
        ACCENT.get()
    } else {
        Color32::TRANSPARENT
    }
}

pub fn workspace_button_fg(active: bool) -> Color32 {
    if active {
        ON_ACCENT.get()
    } else {
        TEXT_PRIMARY.get()
    }
}

pub fn document_chip_fill(active: bool) -> Color32 {
    if active {
        BG_L2.get()
    } else {
        BG_L1.get()
    }
}

/// The accent line along the top of the tab on show.
pub fn paint_active_tab_line(painter: &egui::Painter, rect: egui::Rect) {
    let line =
        egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.right(), rect.top() + 2.0));
    let corner = CornerRadius {
        nw: RADIUS_SMALL,
        ne: RADIUS_SMALL,
        sw: 0,
        se: 0,
    };
    painter.rect_filled(line, corner, ACCENT.get());
}

/// A bordered card that lifts while the pointer is over it.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let frame = egui::Frame::group(ui.style()).corner_radius(RADIUS_MEDIUM);
    let margin = frame.inner_margin.leftf();
    let mut prepared = frame.begin(ui);
    let inner = add(&mut prepared.content_ui);
    if ui.rect_contains_pointer(prepared.content_ui.min_rect().expand(margin)) {
        prepared.frame = prepared
            .frame
            .fill(BG_L2.get())
            .stroke(Stroke::new(1.0_f32, outline()));
    }
    prepared.end(ui);
    inner
}

/// What a panel with nothing to show says, in its middle.
///
/// ⚠ Never `centered_and_justified` around a label: a message that wraps is then justified, its words spread to both edges.
pub fn empty_message(ui: &mut egui::Ui, message: &str) {
    let room = ui.available_rect_before_wrap();
    let mut job = egui::text::LayoutJob::simple(
        message.to_string(),
        egui::TextStyle::Body.resolve(ui.style()),
        TEXT_SECONDARY.get(),
        (room.width() - 2.0 * SPACING_DEFAULT).max(0.0),
    );
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let at = egui::Rect::from_center_size(room.center(), galley.size());
    // A mark over the words, where the panel has the room for it.
    if room.height() > galley.size().y + 96.0 {
        ui.painter().text(
            at.center_top() - egui::vec2(0.0, 10.0),
            egui::Align2::CENTER_BOTTOM,
            egui_phosphor::regular::TRAY,
            egui::FontId::proportional(34.0),
            outline(),
        );
    }
    ui.put(at, egui::Label::new(galley));
    ui.allocate_rect(room, egui::Sense::hover());
}

pub fn document_chip_fg(active: bool) -> Color32 {
    if active {
        TEXT_PRIMARY.get()
    } else {
        TEXT_SECONDARY.get()
    }
}

/// Make `appearance`'s palette the one every [`Themed`] colour resolves to,
/// and style `ctx` from it.
///
/// ⚠ **Styling the context is the smaller half.** A light theme that set only
/// the style would leave every `theme::` read dark — light-grey text on a
/// white panel.
pub fn apply_theme(ctx: &egui::Context, appearance: &AppearancePrefs) {
    // ⚠ The palette is one for the whole process, so a test that holds the theme keeps every other test from changing it.
    #[cfg(test)]
    let _others_wait =
        (!HOLDING.get()).then(|| SERIAL.lock().unwrap_or_else(PoisonError::into_inner));
    *ACTIVE.write().unwrap_or_else(PoisonError::into_inner) = palette_for(appearance);
    GENERATION.fetch_add(1, Ordering::AcqRel);
    ctx.set_global_style(style_for(appearance.theme));
}

fn style_for(theme: ThemeChoice) -> Style {
    let mut style = Style::default();
    let mut visuals = match theme {
        ThemeChoice::Dark => Visuals::dark(),
        ThemeChoice::Light => Visuals::light(),
    };
    let (text, bg0, bg1, bg2, bg3, border) = (
        TEXT_PRIMARY.get(),
        BG_L0.get(),
        BG_L1.get(),
        BG_L2.get(),
        BG_L3.get(),
        BORDER.get(),
    );
    let accent = ACCENT.get();
    let (accent_text, selection, strong) =
        (ACCENT_TEXT.get(), SELECTION_BG.get(), TEXT_STRONG.get());

    visuals.window_fill = bg1;
    visuals.panel_fill = bg1;
    visuals.faint_bg_color = bg2;
    visuals.extreme_bg_color = bg0;

    visuals.widgets.noninteractive.bg_fill = bg2;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text);

    visuals.widgets.inactive.bg_fill = bg2;
    visuals.widgets.inactive.weak_bg_fill = bg2;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, outline());
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, text);

    visuals.widgets.hovered.bg_fill = bg3;
    visuals.widgets.hovered.weak_bg_fill = bg3;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, text);

    // ⚠ egui reads `strong()` text and a pressed or keyboard-focused widget's text from the same stroke, so
    // this one is the strongest text, and the fill under it is a tint that reads with it whatever the accent is.
    visuals.widgets.active.bg_fill = selection;
    visuals.widgets.active.weak_bg_fill = selection;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0_f32, accent_text);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, strong);

    visuals.widgets.open.bg_fill = bg3;
    visuals.widgets.open.weak_bg_fill = bg3;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, accent);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, text);

    visuals.selection.bg_fill = selection;
    visuals.selection.stroke = Stroke::new(1.0_f32, accent_text);

    visuals.hyperlink_color = accent_text;
    visuals.weak_text_color = Some(TEXT_SECONDARY.get());
    visuals.warn_fg_color = WARNING.get();
    visuals.error_fg_color = ERROR.get();

    visuals.window_stroke = Stroke::new(1.0_f32, border);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_LARGE);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_MEDIUM);
    let widgets = &mut visuals.widgets;
    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(RADIUS_SMALL);
    }
    let dark = matches!(theme, ThemeChoice::Dark);
    let shadow = |blur, alpha| egui::Shadow {
        offset: [0, 6],
        blur,
        spread: 0,
        color: Color32::from_black_alpha(alpha),
    };
    visuals.window_shadow = shadow(24, if dark { 110 } else { 50 });
    visuals.popup_shadow = shadow(14, if dark { 90 } else { 40 });
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    visuals.text_cursor.stroke = Stroke::new(2.0_f32, accent_text);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.menu_margin = egui::Margin::same(6);

    style.interaction.selectable_labels = false;
    // ⚠ egui's 9 point default is too small to be all that says something.
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(SMALL_TEXT),
    );
    style
}

#[cfg(test)]
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
thread_local! {
    /// Whether this thread's test holds the theme.
    static HOLDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A test's hold on the theme, released when it ends.
#[cfg(test)]
pub(crate) struct Serial(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
impl Drop for Serial {
    fn drop(&mut self) {
        HOLDING.set(false);
    }
}

/// Hold the theme: until the guard drops, no other test's `apply_theme` runs.
#[cfg(test)]
pub(crate) fn serial() -> Serial {
    let guard = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    HOLDING.set(true);
    Serial(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> u32 {
        u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b())
    }

    /// ⚠ Falsified by leaving `ACTIVE` untouched in `apply_theme`: every
    /// `theme::` read then stays dark whatever the user picked.
    #[test]
    fn a_chosen_theme_and_accent_reach_the_colours_panels_read() {
        let _serial = serial();
        let ctx = egui::Context::default();

        let light = AppearancePrefs {
            theme: ThemeChoice::Light,
            accent: [0xe0, 0x40, 0x10],
            ..AppearancePrefs::default()
        };
        apply_theme(&ctx, &light);
        let text: Color32 = TEXT_PRIMARY.into();
        let panel: Color32 = BG_L1.into();
        assert!(
            luminance(text) < luminance(panel),
            "light theme: text {text:?} should be darker than the panel {panel:?}"
        );
        assert_eq!(ACCENT.get(), Color32::from_rgb(0xe0, 0x40, 0x10));
        assert_eq!(
            ctx.global_style().visuals.hyperlink_color,
            ACCENT_TEXT.get()
        );
        assert!(!ctx.global_style().visuals.dark_mode);

        apply_theme(&ctx, &AppearancePrefs::default());
        assert!(luminance(TEXT_PRIMARY.get()) > luminance(BG_L1.get()));
        assert_eq!(ACCENT.get(), Color32::from_rgb(0x00, 0x78, 0xd4));
        assert!(ctx.global_style().visuals.dark_mode);
    }

    /// ⚠ Falsified by a text or selection colour in a palette that the derivation does not give the default accent: the text colours were fitted to a selection no accent could have.
    #[test]
    fn a_custom_accent_is_derived_the_way_the_default_accents_colours_were_chosen() {
        let default = Color32::from_rgb(0x00, 0x78, 0xd4);
        for (choice, shipped) in [(ThemeChoice::Dark, DARK), (ThemeChoice::Light, LIGHT)] {
            let derived = with_accent(shipped, choice, default);
            for slot in [Slot::AccentText, Slot::OnAccent, Slot::SelectionBg] {
                assert_eq!(
                    derived[slot as usize], shipped[slot as usize],
                    "{choice:?} {slot:?}"
                );
            }
        }
    }

    /// The stroke of the first shape `pick` finds in `shapes`, however deep a shape is nested.
    fn stroke_where(
        shapes: &[egui::epaint::ClippedShape],
        pick: &dyn Fn(&egui::Shape) -> Option<Stroke>,
    ) -> Option<Stroke> {
        fn find(
            shape: &egui::Shape,
            pick: &dyn Fn(&egui::Shape) -> Option<Stroke>,
        ) -> Option<Stroke> {
            match shape {
                egui::Shape::Vec(inner) => inner.iter().find_map(|s| find(s, pick)),
                other => pick(other),
            }
        }
        shapes.iter().find_map(|clipped| find(&clipped.shape, pick))
    }

    /// ⚠ Falsified by an outline at the border colour, which is 1.3 to 1 on the dark panel: a box nobody can see is not a checkbox.
    #[test]
    fn the_outline_of_a_checkbox_and_a_radio_button_reaches_3_to_1_on_every_panel_in_both_themes() {
        let _serial = serial();
        for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
            let ctx = egui::Context::default();
            apply_theme(
                &ctx,
                &AppearancePrefs {
                    theme: choice,
                    ..AppearancePrefs::default()
                },
            );
            let (mut on, mut off) = (false, false);
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.checkbox(&mut on, "Heels");
                ui.radio_value(&mut off, true, "Male");
            });
            let boxed = stroke_where(&output.shapes, &|shape| match shape {
                egui::Shape::Rect(rect) if rect.stroke.width > 0.0 && rect.rect.width() < 24.0 => {
                    Some(rect.stroke)
                }
                _ => None,
            })
            .expect("the checkbox draws a box");
            let round = stroke_where(&output.shapes, &|shape| match shape {
                egui::Shape::Circle(circle)
                    if circle.stroke.width > 0.0 && circle.radius < 12.0 =>
                {
                    Some(circle.stroke)
                }
                _ => None,
            })
            .expect("the radio button draws a circle");
            for (what, stroke) in [("checkbox", boxed), ("radio button", round)] {
                for panel in [BG_L0, BG_L1, BG_L2, BG_L3] {
                    let ratio = contrast(stroke.color, panel.get());
                    assert!(
                        ratio >= GRAPHIC,
                        "{choice:?} {what}: its outline is {ratio:.2} to 1 on {panel:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_default_appearance_is_the_palette_this_shipped_with() {
        let _serial = serial();
        apply_theme(&egui::Context::default(), &AppearancePrefs::default());
        assert_eq!(*ACTIVE.read().expect("palette"), DARK);
    }
}
