//! No text the shell paints reads under 4.5:1 against what is under it, or is
//! set smaller than 10 points, in either theme: every panel of every
//! workspace, the command palette, the menus and every Settings page are
//! drawn, and each run of text is judged against the fills painted before it.
//!
//! ⚠ A text over an image is not judged: its backdrop is not a colour. The
//! viewport is taken at its default background, which the user can change,
//! and the boxes over it are filled so that it hardly shows through. A glyph
//! from the icon font is judged at 3:1, which is what a graphic needs.
//!
//! ⚠ A disabled control is exempt, and egui bakes its fade into the text's
//! colour, so the exemption is by widget: a text is exempt only inside the
//! bounds AccessKit gives a disabled widget, and at the fade's alpha there.

use std::collections::BTreeMap;

use egui::epaint::{ClippedShape, TextShape};
use egui::{Color32, Rect, Rgba, Shape};

use super::frames_of_the_shell::{key, Shell};
use crate::state::{AppearancePrefs, ThemeChoice};
use crate::ui::dock::Workspace;
use crate::ui::panels::settings::SettingsCategory;
use crate::ui::theme::{self, contrast, AA};

/// What a graphic needs against its background.
const GRAPHIC: f32 = theme::GRAPHIC;

/// The smallest a text the shell paints may be set, in points.
const SMALLEST: f32 = 10.0;

/// The viewport's default background.
const VIEWPORT: Color32 = Color32::from_rgb(0x2d, 0x2d, 0x30);

/// `top` over an opaque `under` as the app's framebuffer blends them.
fn over(top: Color32, under: Color32) -> Color32 {
    let kept = 1.0 - f32::from(top.a()) / 255.0;
    Color32::from(Rgba::from(top) + Rgba::from(under) * kept)
}

/// What a later text can be painted over.
enum Under {
    Fill {
        rect: Rect,
        clip: Rect,
        colour: Color32,
    },
    /// An image or a mesh: nothing says what colour it is.
    Unknown { rect: Rect, clip: Rect },
}

/// One colour of one glyph kind in one row of a painted text.
#[derive(Debug)]
struct Run {
    sample: String,
    colour: Color32,
    icon: bool,
    /// The height of a row of its font, in points.
    height: f32,
    /// What it is painted on, when that is a colour.
    backdrop: Option<Color32>,
    /// The middle of the row it is in that shows.
    at: egui::Pos2,
}

fn is_icon(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// The colour `backdrop` becomes under the fills that cover `at`, or `None` over something that is not a colour.
fn backdrop_at(at: egui::Pos2, unders: &[Under], floor: Color32) -> Option<Color32> {
    let mut colour = Some(floor);
    for under in unders {
        match under {
            Under::Fill {
                rect,
                clip,
                colour: fill,
            } if rect.contains(at) && clip.contains(at) => {
                // A fill that is not opaque leaves an unknown backdrop unknown.
                colour = match colour {
                    Some(under) => Some(over(*fill, under)),
                    None => (fill.a() == 255).then_some(*fill),
                };
            }
            Under::Unknown { rect, clip } if rect.contains(at) && clip.contains(at) => {
                colour = None
            }
            _ => {}
        }
    }
    colour
}

fn runs(
    text: &TextShape,
    clip: Rect,
    backdrop_of: &dyn Fn(egui::Pos2) -> Option<Color32>,
) -> Vec<Run> {
    let mut found = Vec::new();
    for placed in &text.galley.rows {
        let row = placed.rect().translate(text.pos.to_vec2());
        let seen = clip.intersect(row);
        if !seen.is_positive() || placed.row.visuals.glyph_vertex_range.start > 0 {
            continue;
        }
        let backdrop = backdrop_of(seen.center());
        let mut row_runs: Vec<Run> = Vec::new();
        for glyph in &placed.row.glyphs {
            if glyph.chr.is_whitespace() {
                continue;
            }
            let Some(vertex) = placed
                .row
                .visuals
                .mesh
                .vertices
                .get(glyph.first_vertex as usize)
            else {
                continue;
            };
            let mut colour = match text.override_text_color {
                Some(over_all) => over_all,
                None if vertex.color == Color32::PLACEHOLDER => text.fallback_color,
                None => vertex.color,
            };
            if text.opacity_factor < 1.0 {
                colour = colour.gamma_multiply(text.opacity_factor);
            }
            let icon = is_icon(glyph.chr);
            match row_runs
                .iter_mut()
                .find(|run| run.colour == colour && run.icon == icon)
            {
                Some(run) => run.sample.push(glyph.chr),
                None => row_runs.push(Run {
                    sample: glyph.chr.to_string(),
                    colour,
                    icon,
                    height: glyph.font_height,
                    backdrop,
                    at: seen.center(),
                }),
            }
        }
        found.extend(row_runs);
    }
    found
}

/// Every run of text in `shapes`.
fn runs_in(shapes: &[ClippedShape], floor: Color32) -> Vec<Run> {
    fn walk(
        shape: &Shape,
        clip: Rect,
        unders: &mut Vec<Under>,
        floor: Color32,
        out: &mut Vec<Run>,
    ) {
        match shape {
            Shape::Vec(shapes) => shapes
                .iter()
                .for_each(|shape| walk(shape, clip, unders, floor, out)),
            Shape::Rect(rect) if rect.brush.is_some() => unders.push(Under::Unknown {
                rect: rect.rect,
                clip,
            }),
            Shape::Rect(rect) if rect.fill.a() > 0 => unders.push(Under::Fill {
                rect: rect.rect,
                clip,
                colour: rect.fill,
            }),
            Shape::Mesh(mesh) => unders.push(Under::Unknown {
                rect: mesh.calc_bounds(),
                clip,
            }),
            Shape::Callback(callback) => unders.push(Under::Fill {
                rect: callback.rect,
                clip,
                colour: VIEWPORT,
            }),
            Shape::Text(text) => {
                let backdrop_of = |at| backdrop_at(at, unders, floor);
                out.extend(runs(text, clip, &backdrop_of));
            }
            _ => {}
        }
    }
    let mut unders = Vec::new();
    let mut out = Vec::new();
    for clipped in shapes {
        walk(
            &clipped.shape,
            clipped.clip_rect,
            &mut unders,
            floor,
            &mut out,
        );
    }
    out
}

/// One screen of the shell: every run of text on it, and the bounds of the widgets AccessKit called disabled.
struct Page {
    name: String,
    runs: Vec<Run>,
    disabled: Vec<Rect>,
}

/// What the shell paints in one theme.
struct Painted {
    pages: Vec<Page>,
    /// The height of a row of the smallest size allowed.
    smallest: f32,
    /// The alpha egui fades a disabled control's text to.
    fade: u8,
}

/// The alpha a text colour is faded to when its control is disabled at `opacity`.
fn faded_to(opacity: f32) -> u8 {
    Color32::WHITE.gamma_multiply(opacity).a()
}

/// The bounds of the widgets `shell`'s last frame described as disabled.
fn disabled_in(shell: &Shell) -> Vec<Rect> {
    (shell.described.iter())
        .filter(|widget| widget.disabled)
        .map(|widget| widget.bounds)
        .collect()
}

/// Whether WCAG exempts `run`: a disabled widget draws it, at the fade egui gives a disabled control's text.
///
/// ⚠ Not by alpha alone: a text whose literal alpha is the fade's is no more disabled than any other.
fn is_exempt(run: &Run, disabled: &[Rect], fade: u8) -> bool {
    run.colour.a() == fade && disabled.iter().any(|bounds| bounds.contains(run.at))
}

fn painted(choice: ThemeChoice) -> Painted {
    let mut shell = Shell::new(choice);
    let floor = theme::BG_L1.get();
    let mut pages: Vec<(String, Vec<ClippedShape>, Vec<Rect>)> = Vec::new();

    for workspace in Workspace::ALL {
        shell.state.active_workspace = workspace;
        pages.push((
            format!("the {workspace:?} workspace"),
            shell.settled(),
            disabled_in(&shell),
        ));
    }

    // The Particle workspace's Inspector leaf holds a second tab, and the pointer is over it.
    shell.state.active_workspace = Workspace::Particle;
    shell.settled();
    shell.draw(vec![egui::Event::PointerMoved(egui::pos2(1512.0, 61.0))]);
    pages.push((
        "an inactive tab under the pointer".to_string(),
        shell.settled(),
        disabled_in(&shell),
    ));
    shell.draw(vec![egui::Event::PointerGone]);

    crate::ui::panels::command_palette::open(&mut shell.state);
    shell.draw(Vec::new());
    shell.draw(vec![egui::Event::Text("undo".into())]);
    pages.push((PALETTE.to_string(), shell.settled(), disabled_in(&shell)));
    shell.draw(vec![key(egui::Key::Escape)]);

    // A folder open with its rows read, and one picked.
    let _folder = shell.open_a_folder();
    for at in [egui::pos2(120.0, 266.0), egui::pos2(453.0, 194.0)] {
        shell.click(at);
    }
    pages.push((
        "a folder open with a row picked".to_string(),
        shell.settled(),
        disabled_in(&shell),
    ));

    // Each of the menu bar's menus open.
    for (name, x) in [
        ("File", 19.0),
        ("Edit", 53.0),
        ("View", 91.0),
        ("Project", 138.0),
        ("Automate", 200.0),
        ("Help", 256.0),
    ] {
        shell.click(egui::pos2(x, 12.0));
        pages.push((
            format!("the {name} menu"),
            shell.settled(),
            disabled_in(&shell),
        ));
    }
    shell.draw(vec![key(egui::Key::Escape)]);

    for category in SettingsCategory::ALL {
        pages.push((
            format!("Settings, {}", category.label()),
            shell.settings(category),
            disabled_in(&shell),
        ));
    }

    pages.push((
        "a clip's timeline".to_string(),
        shell.a_clips_timeline(),
        disabled_in(&shell),
    ));

    // A project open: its workspaces with garments, problems and a failed file, and the questions it asks.
    let folder = tempfile::tempdir().expect("a scratch folder");
    let mut project = super::the_project_screens_on_screen::a_project_shell(folder.path(), choice);
    for workspace in [Workspace::Project, Workspace::Wardrobe] {
        project.state.active_workspace = workspace;
        pages.push((
            format!("the {workspace:?} workspace with a project open"),
            project.settled(),
            disabled_in(&project),
        ));
    }
    project.state.active_workspace = Workspace::Project;
    for (title, ask) in super::the_project_screens_on_screen::the_questions(&project) {
        ask(&mut project);
        // ⚠ A question fades in, and a text mid-fade is not the colour it will be.
        for _ in 0..3 {
            project.settled();
        }
        pages.push((
            format!("the question `{title}`"),
            project.settled(),
            disabled_in(&project),
        ));
        project.state.workbench.asking = None;
    }

    let smallest = shell.ctx.fonts_mut(|fonts| {
        [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
            .map(|family| fonts.row_height(&egui::FontId::new(SMALLEST, family)))
            .into_iter()
            .fold(f32::MAX, f32::min)
    });
    let pages = pages
        .into_iter()
        .map(|(name, shapes, disabled)| Page {
            name,
            runs: runs_in(&shapes, floor),
            disabled,
        })
        .collect();
    Painted {
        pages,
        smallest,
        fade: faded_to(shell.ctx.global_style().visuals.disabled_alpha()),
    }
}

/// The page whose disabled row under the highlight is held to 4.5:1.
const PALETTE: &str = "the command palette, searched";

/// The first run of a colour on a colour that reads under its threshold, and how many there were.
struct Faint {
    page: String,
    run: Run,
    ratio: f32,
    count: usize,
}

/// One line for each colour of text on each colour that reads under its threshold, with where it was first seen.
fn faint(choice: ThemeChoice) -> Vec<String> {
    let mut distinct: BTreeMap<(bool, [u8; 4], [u8; 4]), Faint> = BTreeMap::new();
    let seen = painted(choice);
    let mut found = Vec::new();
    // WCAG exempts a disabled control, so the text egui fades is judged only where a row is held to it: the palette's highlighted one.
    let row = faded_to(theme::disabled_alpha_under_highlight(
        choice == ThemeChoice::Dark,
    ));
    let palette_draws_one = seen
        .pages
        .iter()
        .any(|page| page.name == PALETTE && page.runs.iter().any(|run| run.colour.a() == row));
    if !palette_draws_one {
        found.push(format!(
            "{choice:?}: {PALETTE} paints no disabled row, so the one that must read under the highlight is not judged"
        ));
    }
    for Page {
        name: page,
        runs,
        disabled,
    } in seen.pages
    {
        for run in runs {
            let Some(backdrop) = run.backdrop else {
                continue;
            };
            if is_exempt(&run, &disabled, seen.fade) {
                continue;
            }
            let ratio = contrast(over(run.colour, backdrop), backdrop);
            if ratio >= if run.icon { GRAPHIC } else { AA } {
                continue;
            }
            let key = (run.icon, run.colour.to_array(), backdrop.to_array());
            distinct
                .entry(key)
                .and_modify(|seen| seen.count += 1)
                .or_insert(Faint {
                    page: page.clone(),
                    run,
                    ratio,
                    count: 1,
                });
        }
    }
    found.extend(distinct.into_values().map(|Faint { page, run, ratio, count }| {
            let what = if run.icon { "icon" } else { "text" };
            format!(
                "{choice:?}: {what} {:?} on {:?} is {ratio:.2}:1 ({count} run(s); first in {page}, as {:?})",
                run.colour,
                run.backdrop.unwrap_or_default(),
                run.sample.chars().take(28).collect::<String>(),
            )
    }));
    found
}

/// One line for each size of text under the smallest, with where it was first seen.
fn small(choice: ThemeChoice) -> Vec<String> {
    let Painted {
        pages, smallest, ..
    } = painted(choice);
    let mut distinct: BTreeMap<u32, (String, Vec<String>, usize)> = BTreeMap::new();
    for Page { name, runs, .. } in pages {
        for run in runs.iter().filter(|run| run.height < smallest - 0.01) {
            let entry = distinct.entry((run.height * 100.0) as u32).or_insert((
                name.clone(),
                Vec::new(),
                0,
            ));
            entry.2 += 1;
            if entry.1.len() < 4 {
                entry.1.push(run.sample.chars().take(24).collect());
            }
        }
    }
    distinct
        .into_iter()
        .map(|(height, (page, samples, count))| {
            format!(
                "{choice:?}: rows {:.1} points high where {smallest:.1} is the least ({count} run(s); first in {page}, as {samples:?})",
                height as f32 / 100.0
            )
        })
        .collect()
}

#[test]
fn no_text_the_shell_paints_reads_under_4_5_to_1_in_either_theme() {
    let _serial = theme::serial();
    let mut found = Vec::new();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        found.extend(faint(choice));
    }
    crate::ui::theme::apply_theme(&egui::Context::default(), &AppearancePrefs::default());
    assert!(
        found.is_empty(),
        "{} colours of text read under {AA}:1 (icons under {GRAPHIC}:1):
{}",
        found.len(),
        found.join(
            "
"
        )
    );
}

#[test]
fn no_text_the_shell_paints_is_set_under_10_points() {
    let _serial = theme::serial();
    let mut found = Vec::new();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        found.extend(small(choice));
    }
    crate::ui::theme::apply_theme(&egui::Context::default(), &AppearancePrefs::default());
    assert!(
        found.is_empty(),
        "text set under {SMALLEST} points, which is too small to be all that says something:
{}",
        found.join(
            "
"
        )
    );
}

/// ⚠ egui bakes a disabled control's fade into the colour of its text, so an enabled text whose literal alpha is the fade's looks the same to a scan of colours.
#[test]
fn a_text_at_the_fades_alpha_is_judged_unless_a_disabled_widget_draws_it() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    let opacity = shell.ctx.global_style().visuals.disabled_alpha();
    let fade = faded_to(opacity);
    let literal = theme::TEXT_SECONDARY.get().gamma_multiply(opacity);
    let shapes = shell.scratch(|ui| {
        ui.label(egui::RichText::new("Literal fade").color(literal));
        ui.add_enabled(false, egui::Button::new("Disabled button"));
    });
    let disabled = disabled_in(&shell);
    let floor = theme::BG_L1.get();
    let runs = runs_in(&shapes, floor);
    crate::ui::theme::apply_theme(&egui::Context::default(), &AppearancePrefs::default());
    let find = |sample: &str| {
        runs.iter()
            .find(|run| run.sample.starts_with(sample))
            .unwrap_or_else(|| panic!("no run of {sample:?} among {runs:?}"))
    };
    let (enabled, off) = (find("Literal"), find("Disabled"));
    assert_eq!(
        enabled.colour.a(),
        fade,
        "the literal text is not at the fade"
    );
    assert_eq!(
        off.colour.a(),
        fade,
        "egui fades the disabled text to {fade}"
    );
    assert!(
        contrast(over(enabled.colour, floor), floor) < AA,
        "the literal text reads, so there is nothing to judge"
    );
    assert!(
        !is_exempt(enabled, &disabled, fade),
        "an enabled text at the fade's alpha is exempt from the bar"
    );
    assert!(
        is_exempt(off, &disabled, fade),
        "a disabled button's text is judged, though WCAG exempts it"
    );
}
