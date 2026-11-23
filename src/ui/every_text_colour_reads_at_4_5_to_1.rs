//! Every text colour reaches WCAG AA's 4.5:1 against the panel colours it is
//! drawn on, in both themes and with any accent.
//!
//! ⚠ The app's surface is sRGB, so a translucent colour blends the way
//! [`over`] says and not the way egui blends on the CPU.

use egui::{Color32, Rgba};

use crate::state::{AppearancePrefs, ThemeChoice};
use crate::ui::theme::{self, contrast, AA};

/// `top` over an opaque `under` as the app's framebuffer blends them: the shader linearises the premultiplied colour and the hardware adds the backdrop's light.
fn over(top: Color32, under: Color32) -> Color32 {
    let kept = 1.0 - f32::from(top.a()) / 255.0;
    Color32::from(Rgba::from(top) + Rgba::from(under) * kept)
}

type Surfaces = Vec<(&'static str, Color32)>;

/// The four colours a panel is filled with, from the editor's background to a hovered widget's.
fn panels() -> Surfaces {
    vec![
        ("the editor background", theme::BG_L0.get()),
        ("the panel", theme::BG_L1.get()),
        ("a raised widget", theme::BG_L2.get()),
        ("a hovered widget", theme::BG_L3.get()),
    ]
}

/// What a row is drawn on once it is selected.
fn selected(ctx: &egui::Context) -> (&'static str, Color32) {
    let fill = ctx.global_style().visuals.selection.bg_fill;
    ("a selected row", over(fill, theme::BG_L1.get()))
}

fn judge(found: &mut Vec<String>, what: &str, text: Color32, surfaces: &[(&str, Color32)]) {
    for (name, surface) in surfaces {
        let ratio = contrast(over(text, *surface), *surface);
        if ratio < AA {
            found.push(format!("{what} on {name}: {ratio:.2}:1"));
        }
    }
}

/// The surfaces text in the editor is drawn on: its background, selected text, a find match and the current one.
fn editor() -> Surfaces {
    let under = theme::BG_L0.get();
    vec![
        ("the editor background", under),
        ("selected text", over(theme::SELECTION_BG.get(), under)),
        ("a find match", over(theme::SEARCH_MATCH_BG.get(), under)),
        (
            "the current find match",
            over(theme::SEARCH_MATCH_ACTIVE.get(), under),
        ),
    ]
}

/// Each text colour the theme names, with the surfaces it is drawn on.
fn findings(ctx: &egui::Context) -> Vec<String> {
    let mut found = Vec::new();
    let style = ctx.global_style();
    let visuals = &style.visuals;
    let mut on_panels = panels();
    on_panels.push(selected(ctx));
    for (name, colour) in [
        ("TEXT_PRIMARY", theme::TEXT_PRIMARY),
        ("TEXT_SECONDARY", theme::TEXT_SECONDARY),
        ("TEXT_DIM", theme::TEXT_DIM),
        ("TEXT_DISABLED", theme::TEXT_DISABLED),
        ("TEXT_STRONG", theme::TEXT_STRONG),
        ("ACCENT_TEXT", theme::ACCENT_TEXT),
        ("SUCCESS", theme::SUCCESS),
        ("WARNING", theme::WARNING),
        ("ERROR", theme::ERROR),
        ("INFO", theme::INFO),
    ] {
        judge(&mut found, name, colour.get(), &on_panels);
    }

    for (name, colour) in [
        ("SYNTAX_COMMENT", theme::SYNTAX_COMMENT),
        ("SYNTAX_STRING", theme::SYNTAX_STRING),
        ("SYNTAX_NUMBER", theme::SYNTAX_NUMBER),
        ("SYNTAX_KEYWORD", theme::SYNTAX_KEYWORD),
        ("SYNTAX_TAG", theme::SYNTAX_TAG),
        ("SYNTAX_ATTR", theme::SYNTAX_ATTR),
        ("SYNTAX_PUNCT", theme::SYNTAX_PUNCT),
        ("TEXT_PRIMARY", theme::TEXT_PRIMARY),
    ] {
        judge(&mut found, name, colour.get(), &editor());
    }
    let pill = over(theme::FOLD_COLLAPSED_BG.get(), theme::BG_L0.get());
    judge(
        &mut found,
        "TEXT_DIM",
        theme::TEXT_DIM.get(),
        &[("a folded range's pill", pill)],
    );
    judge(
        &mut found,
        "ON_ACCENT",
        theme::ON_ACCENT.get(),
        &[("the accent", theme::ACCENT.get())],
    );

    // The colours egui reads for the text it styles itself.
    for (name, colour) in [
        ("egui's weak text", visuals.weak_text_color()),
        ("egui's strong text", visuals.strong_text_color()),
        ("egui's hyperlinks", visuals.hyperlink_color),
        ("egui's selected text", visuals.selection.stroke.color),
        ("egui's warnings", visuals.warn_fg_color),
        ("egui's errors", visuals.error_fg_color),
    ] {
        judge(&mut found, name, colour, &on_panels);
    }
    let widgets = &visuals.widgets;
    for (state, widget) in [
        ("a label", widgets.noninteractive),
        ("a button", widgets.inactive),
        ("a hovered button", widgets.hovered),
        ("a pressed or focused button", widgets.active),
        ("an open menu's button", widgets.open),
    ] {
        let fills = [
            ("its fill", over(widget.bg_fill, theme::BG_L1.get())),
            (
                "its flat fill",
                over(widget.weak_bg_fill, theme::BG_L1.get()),
            ),
        ];
        judge(
            &mut found,
            &format!("{state}'s text"),
            widget.fg_stroke.color,
            &fills,
        );
    }

    // `ui.disable()` fades a widget into what is behind it. WCAG exempts a disabled control, so only the palette's row
    // under the highlight is held to 4.5:1, at the opacity it is drawn at.
    let faded = theme::TEXT_PRIMARY
        .get()
        .gamma_multiply(theme::disabled_alpha_under_highlight(visuals.dark_mode));
    let behind: Surfaces = on_panels
        .iter()
        .copied()
        .filter(|(name, _)| *name != "a hovered widget")
        .collect();
    judge(&mut found, "disabled text", faded, &behind);
    found
}

/// A colour a user can pick for the accent, from the page's own swatches to the extremes.
fn accents() -> Vec<[u8; 3]> {
    vec![
        [0x00, 0x99, 0x88],
        [0x3a, 0x9a, 0x3a],
        [0xd0, 0x8a, 0x00],
        [0xd0, 0x3a, 0x3a],
        [0x86, 0x4c, 0xd8],
        [0xd0, 0x4a, 0x9a],
        [0x00, 0x00, 0x00],
        [0xff, 0xff, 0xff],
        [0xff, 0xd7, 0x00],
        [0x80, 0x80, 0x80],
        [0x00, 0xff, 0x00],
        [0xff, 0x00, 0xff],
    ]
}

fn with_theme(
    ctx: &egui::Context,
    choice: ThemeChoice,
    accent: [u8; 3],
    check: impl FnOnce(&egui::Context) -> Vec<String>,
) -> Vec<String> {
    theme::apply_theme(
        ctx,
        &AppearancePrefs {
            theme: choice,
            accent,
            ..AppearancePrefs::default()
        },
    );
    let [r, g, b] = accent;
    check(ctx)
        .into_iter()
        .map(|line| format!("{choice:?} with accent #{r:02x}{g:02x}{b:02x}: {line}"))
        .collect()
}

#[test]
fn every_text_colour_reads_at_4_5_to_1_on_the_panels_it_is_drawn_on_in_both_themes() {
    let _serial = theme::serial();
    let ctx = egui::Context::default();
    let mut found = Vec::new();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        found.extend(with_theme(
            &ctx,
            choice,
            AppearancePrefs::DEFAULT_ACCENT,
            findings,
        ));
    }
    theme::apply_theme(&ctx, &AppearancePrefs::default());
    assert!(
        found.is_empty(),
        "{} text colours read under {AA}:1:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn text_reads_at_4_5_to_1_whatever_accent_the_user_picks() {
    let _serial = theme::serial();
    let ctx = egui::Context::default();
    let mut found = Vec::new();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        for accent in accents() {
            found.extend(with_theme(&ctx, choice, accent, findings));
        }
    }
    theme::apply_theme(&ctx, &AppearancePrefs::default());
    assert!(
        found.is_empty(),
        "{} text colours read under {AA}:1:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// How many times clearer an enabled control's text reads than a disabled one's, at the least.
const FADE: f32 = 1.5;

/// Egui's own fade for a disabled control is left alone, so the cue that a control is disabled is a fade that shows.
fn faded_enough(ctx: &egui::Context) -> Vec<String> {
    let alpha = ctx.global_style().visuals.disabled_alpha();
    let faded = theme::TEXT_PRIMARY.get().gamma_multiply(alpha);
    panels()
        .into_iter()
        .filter_map(|(name, panel)| {
            let enabled = contrast(theme::TEXT_PRIMARY.get(), panel);
            let disabled = contrast(over(faded, panel), panel);
            (enabled < disabled * FADE).then(|| {
                format!("text on {name} is {enabled:.2}:1 enabled and {disabled:.2}:1 disabled")
            })
        })
        .collect()
}

#[test]
fn a_disabled_control_still_reads_as_disabled_in_both_themes() {
    let _serial = theme::serial();
    let ctx = egui::Context::default();
    let mut found = Vec::new();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        found.extend(with_theme(
            &ctx,
            choice,
            AppearancePrefs::DEFAULT_ACCENT,
            faded_enough,
        ));
    }
    theme::apply_theme(&ctx, &AppearancePrefs::default());
    assert!(
        found.is_empty(),
        "a disabled control's text is not {FADE} times fainter than an enabled one's:\n{}",
        found.join("\n")
    );
}
