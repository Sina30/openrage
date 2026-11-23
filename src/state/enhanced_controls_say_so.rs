//! A control an Enhanced file cannot use yet is greyed with the reason: an Enhanced `.ypt`'s
//! transport, Drivers rail, pin rail and the Shaded bar's Exposure, and the texture preview's Apply, Replace from image, Embed and Unembed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use or_io::decl::{Gen8, Gen9};

use super::textures::{textures_in, TextureListing};
use super::the_panels_around_the_viewer_take_both_targets::{a_fragment, a_list, Fixture};
use super::{AppState, DocPayload, Prefs};
use crate::ui::dock::Workspace;
use crate::ui::panels::texture_preview::ENHANCED_TEXTURES;
use crate::ui::panels::timeline::NOT_SIMULATED;

/// What a probe of one control found: whether it is greyed, and every text a hover painted that a
/// frame with the pointer away did not, so a line painted all the time is not a hover text.
pub(super) struct Control {
    pub(super) disabled: bool,
    said: BTreeSet<String>,
}

impl Control {
    pub(super) fn says(&self, text: &str) -> bool {
        self.said.contains(text)
    }
}

/// One frame of `draw` at `time`: the centre and greyed state of the control labelled `label`, and
/// how many times each text was painted.
pub(super) fn frame(
    ctx: &egui::Context,
    draw: fn(&mut egui::Ui, &mut AppState),
    state: &mut AppState,
    label: &str,
    time: f64,
    events: Vec<egui::Event>,
) -> (Option<(egui::Pos2, bool)>, BTreeMap<String, usize>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 700.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| draw(ui, state));
    let found = (output.platform_output.accesskit_update.as_ref()).and_then(|update| {
        update.nodes.iter().find_map(|(_, node)| {
            let text = node.label().or_else(|| node.value())?;
            let bounds = node.bounds().filter(|_| text == label)?;
            let at = egui::pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            );
            Some((at, node.is_disabled()))
        })
    });
    let mut painted = BTreeMap::new();
    let mut shapes: Vec<&egui::Shape> =
        output.shapes.iter().map(|clipped| &clipped.shape).collect();
    while let Some(shape) = shapes.pop() {
        match shape {
            egui::Shape::Text(text) => {
                *painted.entry(text.galley.text().to_string()).or_insert(0) += 1;
            }
            egui::Shape::Vec(inner) => shapes.extend(inner),
            _ => {}
        }
    }
    (found, painted)
}

/// The control labelled `label` in what `draw` paints, found with the pointer away, then hovered for long enough for a tooltip.
pub(super) fn control(
    draw: fn(&mut egui::Ui, &mut AppState),
    state: &mut AppState,
    label: &str,
) -> Control {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut found = None;
    let mut away = BTreeMap::new();
    for step in 0..4 {
        let (now, painted) = frame(&ctx, draw, state, label, f64::from(step) / 10.0, Vec::new());
        found = now.or(found);
        away = painted;
    }
    let (at, _) = found.unwrap_or_else(|| panic!("no control named `{label}` is drawn"));
    let hover = vec![egui::Event::PointerMoved(at)];
    frame(&ctx, draw, state, label, 1.0, hover);
    let mut said = BTreeSet::new();
    let mut disabled = false;
    for step in 1..=15 {
        let time = 1.0 + f64::from(step) / 10.0;
        let (now, painted) = frame(&ctx, draw, state, label, time, Vec::new());
        for (text, times) in painted {
            if times > away.get(&text).copied().unwrap_or(0) {
                said.insert(text);
            }
        }
        disabled = now.is_some_and(|(_, disabled)| disabled);
    }
    Control { disabled, said }
}

fn opened_as(path: &str, payload: DocPayload) -> AppState {
    let mut state = AppState::from_prefs(Prefs::default());
    state.insert_document(PathBuf::from(path), payload, None, None);
    state
}

/// `payload` open under `path`, with the texture workspace on screen and its first texture selected.
fn showing_first_texture(path: &str, payload: DocPayload) -> AppState {
    let mut state = opened_as(path, payload);
    let doc = state.active_document.expect("opened");
    let TextureListing::Textures { textures, .. } =
        textures_in(&state.document(doc).expect("open").payload, None)
    else {
        panic!("{path} lists no textures");
    };
    state.selection.select(doc, textures[0].selection_id);
    state.active_workspace = Workspace::Texture;
    state
}

const PROBED: &str = "the reason a probe looks for";

fn a_greyed_button_and_a_line_painted_beside_it(ui: &mut egui::Ui, _: &mut AppState) {
    ui.add_enabled(false, egui::Button::new("Go"));
    ui.label(PROBED);
}

fn a_greyed_button_with_a_hover_text(ui: &mut egui::Ui, _: &mut AppState) {
    ui.add_enabled(false, egui::Button::new("Go"))
        .on_disabled_hover_text(PROBED);
}

fn a_greyed_button_with_a_hover_text_and_a_line_beside_it(ui: &mut egui::Ui, _: &mut AppState) {
    ui.add_enabled(false, egui::Button::new("Go"))
        .on_disabled_hover_text(PROBED);
    ui.label(PROBED);
}

#[test]
fn the_probe_tells_a_hover_text_from_a_line_painted_all_the_time() {
    let says = |draw: fn(&mut egui::Ui, &mut AppState)| {
        let mut state = AppState::from_prefs(Prefs::default());
        let go = control(draw, &mut state, "Go");
        assert!(go.disabled, "the probed button is not greyed");
        go.says(PROBED)
    };
    assert!(
        !says(a_greyed_button_and_a_line_painted_beside_it),
        "a line painted beside the button is read as its hover text"
    );
    assert!(
        says(a_greyed_button_with_a_hover_text),
        "a hover text is not read"
    );
    assert!(
        says(a_greyed_button_with_a_hover_text_and_a_line_beside_it),
        "a hover text that repeats a line painted beside it is not read"
    );
}

const TRANSPORT: [&str; 3] = ["▶", "Playback rate", "Loop"];
const DRIVERS: [&str; 3] = ["Time of day", "Weather", "Exposure"];

#[test]
fn an_enhanced_particle_files_timeline_greys_play_with_the_reason_the_effect_is_not_simulated() {
    let draw = crate::ui::panels::timeline::draw;
    let mut legacy = opened_as("probe.ypt", Gen8::particles(a_list::<Gen8>()));
    assert!(
        !control(draw, &mut legacy, "▶").disabled,
        "Play is greyed for a Legacy effect"
    );

    let mut enhanced = opened_as("probe.ypt", Gen9::particles(a_list::<Gen9>()));
    let play = control(draw, &mut enhanced, "▶");
    assert!(play.disabled, "Play is offered for an Enhanced effect");
    assert!(play.says(NOT_SIMULATED), "Play says: {:?}", play.said);
}

#[test]
fn an_enhanced_particle_files_transport_and_drivers_rail_are_greyed_and_say_nothing_plays() {
    let draw = crate::ui::panels::timeline::draw;
    for label in TRANSPORT.into_iter().chain(DRIVERS) {
        let mut legacy = opened_as("probe.ypt", Gen8::particles(a_list::<Gen8>()));
        assert!(
            !control(draw, &mut legacy, label).disabled,
            "{label} is greyed for a Legacy effect"
        );

        let mut enhanced = opened_as("probe.ypt", Gen9::particles(a_list::<Gen9>()));
        let found = control(draw, &mut enhanced, label);
        assert!(found.disabled, "{label} is offered for an Enhanced effect");
        assert!(found.says(NOT_SIMULATED), "{label} says: {:?}", found.said);
    }
}

#[test]
fn an_enhanced_particle_files_timeline_stops_a_clock_that_was_playing() {
    let mut state = opened_as("probe.ypt", Gen9::particles(a_list::<Gen9>()));
    state.sim.manager.clock.playing = true;
    control(crate::ui::panels::timeline::draw, &mut state, "Loop");
    assert!(
        !state.sim.manager.clock.playing,
        "the clock plays on with nothing simulated"
    );
}

/// Whether the timeline's clock plays once Space is pressed with the pointer over it.
fn space_starts_the_clock(mut state: AppState) -> bool {
    let ctx = egui::Context::default();
    let space = egui::Event::Key {
        key: egui::Key::Space,
        physical_key: Some(egui::Key::Space),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let over = egui::Event::PointerMoved(egui::pos2(400.0, 300.0));
    for (time, events) in [(0.0, vec![over]), (0.1, Vec::new()), (0.2, vec![space])] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            crate::ui::panels::timeline::draw(ui, &mut state)
        });
    }
    state.sim.manager.clock.playing
}

#[test]
fn space_over_an_enhanced_particle_files_timeline_starts_nothing() {
    let legacy = opened_as("probe.ypt", Gen8::particles(a_list::<Gen8>()));
    assert!(
        space_starts_the_clock(legacy),
        "Space plays no Legacy effect"
    );
    let enhanced = opened_as("probe.ypt", Gen9::particles(a_list::<Gen9>()));
    assert!(
        !space_starts_the_clock(enhanced),
        "Space plays an Enhanced effect, which nothing simulates"
    );
}

/// The texture preview's Apply, its encode inputs, Replace from image, Embed and Unembed over the first texture of the document `payload` makes.
fn texture_controls(path: &str, payload: impl Fn() -> DocPayload) -> Vec<(&'static str, Control)> {
    [
        "Apply",
        "Texture format",
        "Texture quality",
        "Mips to keep",
        "sRGB",
        "Replace from image…",
        "Embed",
        "Unembed",
    ]
    .into_iter()
    .map(|label| {
        let mut state = showing_first_texture(path, payload());
        let draw = crate::ui::panels::texture_preview::draw;
        (label, control(draw, &mut state, label))
    })
    .collect()
}

#[test]
fn the_texture_preview_offers_import_embed_and_unembed_over_a_legacy_fragments_texture() {
    for (label, control) in texture_controls("probe.yft", || Gen8::fragment(a_fragment::<Gen8>())) {
        assert!(!control.disabled, "{label} is greyed over a Legacy texture");
    }
}

#[test]
fn the_texture_preview_greys_apply_import_embed_and_unembed_over_an_enhanced_texture_and_says_why()
{
    type Make = fn() -> DocPayload;
    let particles: Make = || Gen9::particles(a_list::<Gen9>());
    let fragment: Make = || Gen9::fragment(a_fragment::<Gen9>());
    for (what, path, payload) in [
        ("particle file", "probe.ypt", particles),
        ("fragment", "probe.yft", fragment),
    ] {
        for (label, control) in texture_controls(path, payload) {
            let over = format!("{label} over an Enhanced {what}'s texture");
            assert!(control.disabled, "{over} is offered");
            assert!(
                control.says(ENHANCED_TEXTURES),
                "{over} says: {:?}",
                control.said
            );
        }
    }
}

#[test]
fn the_constant_that_says_a_target_is_simulated_is_what_tick_sim_does_with_its_list() {
    use super::particle_target::ParticleTarget;
    let mut legacy = opened_as("probe.ypt", Gen8::particles(a_list::<Gen8>()));
    legacy.tick_sim();
    assert_eq!(
        legacy.sim.last_built.is_some(),
        <Gen8 as ParticleTarget>::SIMULATED,
        "a Legacy list is simulated or not against what its target says"
    );
    assert!(
        legacy.sim.last_built.is_some(),
        "the Legacy list was not simulated, so the comparison above proves nothing"
    );

    let mut enhanced = opened_as("probe.ypt", Gen9::particles(a_list::<Gen9>()));
    enhanced.tick_sim();
    assert_eq!(
        enhanced.sim.last_built.is_some(),
        <Gen9 as ParticleTarget>::SIMULATED,
        "an Enhanced list is simulated or not against what its target says: the controls grey on the constant and say nothing plays"
    );
    assert!(
        enhanced.sim.last_built.is_none(),
        "the Enhanced list was simulated"
    );
}

#[test]
fn the_texture_preview_over_a_legacy_particle_files_texture_gives_the_reason_that_is_true_of_it() {
    for (label, control) in texture_controls("probe.ypt", || Gen8::particles(a_list::<Gen8>())) {
        assert!(
            control.disabled,
            "{label} is offered over a Legacy particle file's texture"
        );
        assert!(
            control.said.iter().all(|text| !text.contains("Enhanced")),
            "{label} blames the edition of a Legacy file: {:?}",
            control.said
        );
        assert!(!control.said.is_empty(), "{label} says nothing");
    }
    let (_, replace) = (texture_controls("probe.ypt", || Gen8::particles(a_list::<Gen8>())))
        .into_iter()
        .next()
        .expect("Replace from image");
    assert!(
        replace.said.iter().any(|text| text.contains("no editable")),
        "Replace says: {:?}",
        replace.said
    );
}

/// `payload` open with its first effect pinned: the state, and the labels of the rail's unpin button and Start offset.
fn pinned_first_effect(payload: DocPayload) -> (AppState, [String; 2]) {
    use crate::state::sim::{SerialisableTarget, SimBuildKey};
    let mut state = opened_as("probe.ypt", payload);
    let doc_id = state.active_document.expect("opened");
    let key = SimBuildKey {
        doc_id,
        target: SerialisableTarget::Effect(0),
    };
    let name = crate::ui::panels::timeline::pin_label(&state, &key);
    state.sim.toggle_pin(key);
    let unpin = format!("{name} {}", egui_phosphor::regular::X);
    (state, [unpin, "+0.00s".to_string()])
}

#[test]
fn an_enhanced_particle_files_pin_rail_is_greyed_and_says_nothing_plays() {
    let draw = crate::ui::panels::timeline::draw;
    for which in 0..2 {
        let (mut legacy, labels) = pinned_first_effect(Gen8::particles(a_list::<Gen8>()));
        let label = &labels[which];
        assert!(
            !control(draw, &mut legacy, label).disabled,
            "{label} is greyed for a Legacy effect"
        );

        let (mut enhanced, labels) = pinned_first_effect(Gen9::particles(a_list::<Gen9>()));
        let label = &labels[which];
        let found = control(draw, &mut enhanced, label);
        assert!(found.disabled, "{label} is offered for an Enhanced effect");
        assert!(found.says(NOT_SIMULATED), "{label} says: {:?}", found.said);
    }
}

fn the_shaded_bar(ui: &mut egui::Ui, state: &mut AppState) {
    let rect = ui.max_rect();
    crate::ui::panels::viewport::draw_shaded_hud_bar(ui, rect, rect.top(), state);
}

#[test]
fn the_shaded_bars_exposure_is_greyed_over_an_enhanced_particle_file_and_says_nothing_plays() {
    let mut legacy = opened_as("probe.ypt", Gen8::particles(a_list::<Gen8>()));
    assert!(
        !control(the_shaded_bar, &mut legacy, "Exposure").disabled,
        "Exposure is greyed for a Legacy effect"
    );

    let mut enhanced = opened_as("probe.ypt", Gen9::particles(a_list::<Gen9>()));
    let found = control(the_shaded_bar, &mut enhanced, "Exposure");
    assert!(found.disabled, "Exposure is offered for an Enhanced effect");
    assert!(found.says(NOT_SIMULATED), "Exposure says: {:?}", found.said);

    let mut fragment = opened_as("probe.yft", Gen9::fragment(a_fragment::<Gen9>()));
    assert!(
        !control(the_shaded_bar, &mut fragment, "Exposure").disabled,
        "Exposure is greyed for a document that holds no effect"
    );
}
