//! A skeleton is seen without an animation: the viewport's Bones toggle draws a viewed drawable's
//! bones at their rest pose, the bone selected in the Outline is highlighted and a click on a bone's
//! line selects its row, the Animation preview draws the chosen skeleton's bones before any clip is
//! chosen or binds, and Show bones is on by default.

use egui::accesskit::{Role, Toggled};
use glam::Vec3;
use or_anim::{Skeleton, NO_PARENT};
use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{CrSkeletonData, Mat34V};
use or_renderer::LineVertex;
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8, TARGET_GEN9};

use super::an_edit_stays_where_it_was_made::{texts, Inspector};
use super::bone_numbers::{skeleton_of, BoneAddress, BoneEdit};
use super::prefs::{load_prefs_from, Prefs};
use super::the_animation_preview_draws_what_its_sources_hold::{a_clip_over_the_rig, rigged, CLIP};
use super::the_panels_around_the_viewer_take_both_targets::{a_list, Fixture, MODEL};
use super::the_viewport_picks_what_it_draws::{picked, ray, Held, Holder, MIDDLE_OF_THE_BONE};
use super::viewport_basics_for_inspecting_assets::{
    click_in_the_row, opened, the_outline_shows, the_row_shows,
};
use super::{AppState, DocPayload, DocumentId};
use crate::render::skeleton_lines::{BONE, SELECTED};
use crate::render::viewport::bones;
use crate::render::viewport::pick::select_under;
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, ypt_model_id, DOC_ROOT_SEL, YPT_MODEL_KIND_BONE,
};
use crate::ui::panels::viewport_layout::overlay_text;

/// How far, in metres, a line's end may be from the rest pose a file states.
const EPS: f32 = 1e-3;

/// A segment of the lines the viewport draws, from where it starts to where it ends.
type Segment = (Vec3, Vec3);

/// The segments among `lines` that are drawn in `colour`.
fn drawn_in(lines: &[LineVertex], colour: [f32; 4]) -> Vec<Segment> {
    (lines.as_chunks::<2>().0.iter())
        .filter(|pair| pair[0].colour == colour)
        .map(|pair| (Vec3::from(pair[0].position), Vec3::from(pair[1].position)))
        .collect()
}

/// Each of `wanted` matched to a segment of `drawn` of its own; what is left of `drawn`, or the
/// first of `wanted` that has none.
fn match_up(mut drawn: Vec<Segment>, wanted: &[Segment]) -> Result<Vec<Segment>, Segment> {
    for want in wanted {
        let same = |d: &Segment| d.0.abs_diff_eq(want.0, EPS) && d.1.abs_diff_eq(want.1, EPS);
        let at = drawn.iter().position(same).ok_or(*want)?;
        drawn.swap_remove(at);
    }
    Ok(drawn)
}

/// A document of a skeleton, seen in the Model workspace.
struct Rig {
    label: String,
    state: AppState,
    doc: DocumentId,
    holder: Holder,
    /// The bones that have a parent, each with the segment from its parent's rest position to its own.
    rest: Vec<(usize, Segment)>,
}

impl Rig {
    fn segments(&self) -> Vec<Segment> {
        self.rest.iter().map(|(_, segment)| *segment).collect()
    }
}

/// What `rigged` rests at: its root a metre up and its second bone a metre above that.
fn the_rigs_segment() -> Segment {
    (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 2.0, 0.0))
}

/// The rig of `rigged` held as `holder` holds a drawable, open and active, its rest pose derived so
/// that a bone of it can be edited.
fn viewed<T: Held>(holder: Holder) -> Rig {
    let mut drawable = rigged::<T>();
    let skeleton = drawable.base.skeleton.as_mut().expect("a rig");
    skeleton.default_transforms = vec![Mat34V::default(); 2];
    skeleton.inverse_transforms = vec![Mat34V::default(); 2];
    skeleton.rebuild_rest_pose(0).expect("the rig derives");
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(
        holder.file().into(),
        DocPayload::Resource(holder.asset(drawable)),
        None,
        None,
    );
    assert_eq!(state.active_workspace, Workspace::Model);
    Rig {
        label: format!("{holder:?}"),
        state,
        doc,
        holder,
        rest: vec![(1, the_rigs_segment())],
    }
}

/// Each bone with a parent, with the segment the file's own rest pose puts it at: a bone rests where
/// the inverse of the inverse transform the file ships for it is, which no derivation of ours made.
fn rest_of(data: &CrSkeletonData) -> Option<Vec<(usize, Segment)>> {
    let skeleton = Skeleton::build(data);
    let inverse = &skeleton.file_inverse_bind;
    if inverse.len() != data.bones.len() {
        return None;
    }
    let at = |bone: usize| inverse[bone].inverse().w_axis.truncate();
    let rest: Vec<_> = (skeleton.parents.iter().enumerate())
        .filter(|(_, parent)| **parent != NO_PARENT)
        .map(|(bone, parent)| (bone, (at(*parent as usize), at(bone))))
        .collect();
    (!rest.is_empty()).then_some(rest)
}

/// Every staged file of `extension` and `target`.
fn staged(extension: &str, target: &str) -> Vec<std::path::PathBuf> {
    Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(target)
        .find()
}

/// The smallest staged file of `extension` and `target` that holds a skeleton the file states a rest
/// pose of, open in the Model workspace.
fn staged_rig(extension: &str, target: &str, holder: Holder) -> Option<Rig> {
    let mut files = staged(extension, target);
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files.into_iter().find_map(|path| {
        let payload = DocPayload::Resource(or_io::open(&path).ok()?);
        let rest = rest_of(skeleton_of(&payload, 0)?)?;
        let DocPayload::Resource(asset) = payload else {
            return None;
        };
        let (state, doc) = opened(&path, asset);
        assert_eq!(state.active_workspace, Workspace::Model);
        Some(Rig {
            label: format!("{target} {}", path.display()),
            state,
            doc,
            holder,
            rest,
        })
    })
}

/// Every rig the tests run over: that of `rigged` held every way, of each target, and the smallest
/// staged drawable and fragment of each target.
fn rigs(what: &str) -> Vec<Rig> {
    let mut rigs = Vec::new();
    for holder in Holder::ALL {
        rigs.push(viewed::<Gen8>(holder));
        rigs.push(viewed::<Gen9>(holder));
    }
    for target in [TARGET_GEN8, TARGET_GEN9] {
        for (extension, holder) in [("ydr", Holder::Drawable), ("yft", Holder::Fragment)] {
            match staged_rig(extension, target, holder) {
                Some(rig) => rigs.push(rig),
                // The files staged hold no rest pose: the rig of `rigged`, held every way above, stands in for them
                None if !staged(extension, target).is_empty() => {}
                None => skip_corpus(what, format_args!("no .{extension} of {target} is staged")),
            }
        }
    }
    rigs
}

#[test]
fn the_bones_toggle_draws_one_line_for_each_bone_with_a_parent_from_its_parents_rest_position() {
    const WHAT: &str = "the_bones_toggle_draws_one_line_for_each_bone_with_a_parent_from_its_parents_rest_position";
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        assert!(
            bones::lines(&rig.state).is_empty(),
            "{label}: the bones draw with the toggle off"
        );

        rig.state.preview_scene.show_rest_bones = true;
        let drawn = drawn_in(&bones::lines(&rig.state), BONE);
        assert_eq!(
            drawn.len(),
            rig.rest.len(),
            "{label}: not one line for each bone with a parent"
        );
        assert_eq!(
            match_up(drawn, &rig.segments()),
            Ok(Vec::new()),
            "{label}: a line is not from its parent's rest position to the bone's"
        );
    }
}

#[test]
fn the_bone_selected_in_the_outline_is_drawn_in_the_highlight_colour() {
    const WHAT: &str = "the_bone_selected_in_the_outline_is_drawn_in_the_highlight_colour";
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        rig.state.preview_scene.show_rest_bones = true;
        let plain = bones::lines(&rig.state);
        assert!(
            drawn_in(&plain, SELECTED).is_empty(),
            "{label}: a bone is highlighted with nothing selected"
        );

        let (bone, segment) = *rig.rest.last().expect("a bone with a parent");
        rig.state
            .selection
            .select(rig.doc, rig.holder.bone(bone as u32));
        let lines = bones::lines(&rig.state);
        // Its line, and its joint, which sits at the bone's own end of it.
        let left = match_up(drawn_in(&lines, SELECTED), &[segment]);
        assert!(
            left.as_ref().is_ok_and(|left| (left.iter())
                .all(|(a, b)| a.distance(segment.1) < 0.02 && b.distance(segment.1) < 0.02)),
            "{label}: the selected bone's line is not the one in the highlight colour: {left:?}"
        );
        let others: Vec<_> = (rig.rest.iter())
            .filter(|(other, _)| *other != bone)
            .map(|(_, segment)| *segment)
            .collect();
        assert_eq!(
            match_up(drawn_in(&lines, BONE), &others),
            Ok(Vec::new()),
            "{label}: the bones not selected are not left as they were"
        );

        rig.state.selection.select(rig.doc, DOC_ROOT_SEL);
        assert!(
            drawn_in(&bones::lines(&rig.state), SELECTED).is_empty(),
            "{label}: a row that is not a bone's highlights one"
        );
    }
}

fn clicking_a_bones_line<T: Held>() {
    for holder in Holder::ALL {
        let Rig { mut state, doc, .. } = viewed::<T>(holder);
        let click = |shift| ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, shift);

        select_under(&mut state, &click(egui::Vec2::ZERO));
        assert_ne!(
            state.selection.selected(doc),
            Some(holder.bone(1)),
            "{holder:?}: a click picked a bone the viewport does not draw"
        );

        state.preview_scene.show_rest_bones = true;
        select_under(&mut state, &click(egui::Vec2::ZERO));
        assert_eq!(
            state.selection.selected(doc),
            Some(holder.bone(1)),
            "{holder:?}: a click on the bone's line missed its row"
        );
        assert!(
            the_outline_shows(&mut state, "BONE_1 (1)"),
            "{holder:?}: the Outline does not show the bone clicked"
        );

        // Twenty points is thirty pixels on this screen, past the bone's reach.
        select_under(&mut state, &click(egui::vec2(0.0, 20.0)));
        assert_ne!(
            state.selection.selected(doc),
            Some(holder.bone(1)),
            "{holder:?}: a click thirty pixels from the line still picked the bone"
        );
    }
}

#[test]
fn a_click_on_a_bones_line_in_the_model_workspace_selects_its_row() {
    clicking_a_bones_line::<Gen8>();
    clicking_a_bones_line::<Gen9>();
}

fn editing_a_bone<T: Held>() {
    let at = BoneAddress { owner: 0, bone: 1 };
    for holder in Holder::ALL {
        let Rig { mut state, doc, .. } = viewed::<T>(holder);
        state.preview_scene.show_rest_bones = true;
        let before = drawn_in(&bones::lines(&state), BONE);

        // A metre along x from the root, which rests at (0, 1, 0).
        let edit = BoneEdit::Position([1.0, 0.0, 0.0]);
        state.edit_bone(doc, at, edit).expect("the position edits");
        let moved = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
        let after = drawn_in(&bones::lines(&state), BONE);
        assert_eq!(
            match_up(after, &[moved]),
            Ok(Vec::new()),
            "{holder:?}: the line did not move with the bone"
        );

        assert!(state.undo().is_some(), "{holder:?}: the edit undoes");
        assert_eq!(
            match_up(drawn_in(&bones::lines(&state), BONE), &before),
            Ok(Vec::new()),
            "{holder:?}: the undo did not put the line back"
        );
    }
}

#[test]
fn a_bones_position_edited_in_the_inspector_moves_its_line() {
    editing_a_bone::<Gen8>();
    editing_a_bone::<Gen9>();
}

fn row_of_controls<T: Held>() {
    for holder in Holder::ALL {
        let Rig { mut state, doc, .. } = viewed::<T>(holder);
        assert!(
            the_row_shows(&mut state, "Bones"),
            "{holder:?}: the viewport offers no Bones toggle beside a skeleton"
        );
        assert!(!state.preview_scene.show_rest_bones);

        assert!(click_in_the_row(&mut state, "Bones"));
        assert!(
            state.preview_scene.show_rest_bones,
            "{holder:?}: the toggle did not turn the bones on"
        );
        assert!(click_in_the_row(&mut state, "Bones"));
        assert!(
            !state.preview_scene.show_rest_bones,
            "{holder:?}: the toggle did not turn them off"
        );

        let bare = state.insert_document(
            holder.file().into(),
            DocPayload::Resource(holder.asset::<T>(Default::default())),
            None,
            None,
        );
        assert_ne!(bare, doc);
        assert!(
            !the_row_shows(&mut state, "Bones"),
            "{holder:?}: a drawable with no skeleton offers the toggle"
        );
    }
}

#[test]
fn the_viewport_offers_a_bones_toggle_beside_a_skeleton_and_not_beside_none() {
    row_of_controls::<Gen8>();
    row_of_controls::<Gen9>();
}

/// The Animation workspace over a clip dictionary, which is the active document, with the rig as the
/// skeleton source and no clip chosen.
struct Preview {
    state: AppState,
    rig: DocumentId,
    clips: DocumentId,
    holder: Holder,
}

fn previewing_the_rig<T: Held>(holder: Holder, tags_apart: bool) -> Preview {
    let mut drawable = rigged::<T>();
    if tags_apart {
        let skeleton = drawable.base.skeleton.as_mut().expect("a rig");
        (skeleton.bones.iter_mut()).for_each(|bone| bone.bone_id += 1000);
    }
    let mut state = AppState::from_prefs(Prefs::default());
    let rig = state.insert_document(
        holder.file().into(),
        DocPayload::Resource(holder.asset(drawable)),
        None,
        None,
    );
    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    let clips = state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
    assert_eq!(state.active_workspace, Workspace::Animation);
    state.preview_scene.pin_skeleton(Some(holder.skeleton(rig)));
    state.tick_animation();
    Preview {
        state,
        rig,
        clips,
        holder,
    }
}

fn a_skeleton_chosen<T: Held>() {
    for holder in Holder::ALL {
        let mut p = previewing_the_rig::<T>(holder, false);
        assert!(p.state.preview_scene.clip.is_none());
        assert!(
            p.state.anim.skeleton().is_none(),
            "no clip binds a skeleton"
        );

        let drawn = drawn_in(&bones::lines(&p.state), BONE);
        assert_eq!(
            match_up(drawn, &[the_rigs_segment()]),
            Ok(Vec::new()),
            "{holder:?}: the skeleton chosen with no clip draws no bone at rest"
        );
        let said = overlay_text(&p.state, None);
        assert!(
            said.contains("Bones of "),
            "{holder:?}: the corner box does not say the bones are drawn: {said}"
        );

        // ⚠ The pick names the rig's document, not the clip dictionary the app activated.
        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            picked(&p.state),
            Some((p.rig, "BONE_1 (1)")),
            "{holder:?}: a click on a bone at rest missed it"
        );

        p.state.preview_scene.show_bones = false;
        assert!(
            bones::lines(&p.state).is_empty(),
            "{holder:?}: the bones draw with Show bones off"
        );
    }
}

#[test]
fn a_skeleton_chosen_in_the_animation_workspace_draws_its_bones_with_no_clip_chosen() {
    a_skeleton_chosen::<Gen8>();
    a_skeleton_chosen::<Gen9>();
}

fn a_clip_that_does_not_bind<T: Held>() {
    for holder in Holder::ALL {
        let mut p = previewing_the_rig::<T>(holder, true);
        p.state.preview_scene.set_clip(p.clips, CLIP);
        p.state.tick_animation();
        assert!(
            p.state.anim.player.is_none() && p.state.anim.world_matrices.is_empty(),
            "{:?}: the clip over other bones poses the skeleton",
            p.holder
        );

        let drawn = drawn_in(&bones::lines(&p.state), BONE);
        assert_eq!(
            match_up(drawn, &[the_rigs_segment()]),
            Ok(Vec::new()),
            "{holder:?}: a clip that does not bind leaves no bone at rest"
        );
    }
}

#[test]
fn a_clip_that_does_not_bind_leaves_the_skeleton_chosen_drawn_at_rest() {
    a_clip_that_does_not_bind::<Gen8>();
    a_clip_that_does_not_bind::<Gen9>();
}

/// Whether Preview settings draws its Show bones box ticked, over `state`'s clip dictionary.
fn show_bones_box_is_ticked(state: &mut AppState) -> bool {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut found = None;
    for frame in 0..3 {
        let input = egui::RawInput {
            time: Some(f64::from(frame) / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 4000.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            crate::ui::panels::preview_settings::draw(ui, state)
        });
        let nodes = output.platform_output.accesskit_update.map(|u| u.nodes);
        found = (nodes.unwrap_or_default().iter()).find_map(|(_, node)| {
            let named = node.role() == Role::CheckBox && node.label() == Some("Show bones");
            named.then(|| node.toggled() == Some(Toggled::True))
        });
    }
    found.expect("Preview settings draws a Show bones box")
}

#[test]
fn show_bones_is_on_for_a_fresh_preference_file() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let fresh = dir.path().join("prefs.json");
    assert!(!fresh.exists());
    let mut state = AppState::from_prefs(load_prefs_from(&fresh));
    assert!(
        state.preview_scene.show_bones,
        "a preview with no preferences yet starts with its bones hidden"
    );
    // The Model workspace's own toggle starts off, as Collision does.
    assert!(!state.preview_scene.show_rest_bones);

    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    state.insert_document("C:/mods/clips.ycd".into(), clips, None, None);
    assert!(
        show_bones_box_is_ticked(&mut state),
        "Preview settings draws the Show bones box unticked"
    );
}

#[test]
fn a_skeleton_at_rest_is_built_once_for_as_long_as_its_document_holds_it() {
    let mut rig = viewed::<Gen8>(Holder::Drawable);
    rig.state.preview_scene.show_rest_bones = true;
    let before = bones::builds();
    let first = bones::lines(&rig.state);
    for _ in 0..3 {
        assert_eq!(
            bones::lines(&rig.state).len(),
            first.len(),
            "the same bones, drawn again"
        );
    }
    assert_eq!(
        bones::builds() - before,
        1,
        "the viewport asks for the bones every frame, and a ped's takes 0.8 ms to build"
    );

    // A document that changed is built again
    rig.state
        .document_mut(rig.doc)
        .expect("open")
        .set_resource(None);
    bones::lines(&rig.state);
    assert_eq!(bones::builds() - before, 2, "an edit is a new skeleton");
}

fn a_bone_picked_in_the_animation_preview<T: Held>() {
    for holder in Holder::ALL {
        let mut p = previewing_the_rig::<T>(holder, false);
        assert!(
            drawn_in(&bones::lines(&p.state), SELECTED).is_empty(),
            "{holder:?}: a bone is highlighted before any is picked"
        );
        select_under(
            &mut p.state,
            &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
        );
        assert_eq!(
            picked(&p.state),
            Some((p.rig, "BONE_1 (1)")),
            "{holder:?}: a click on a bone missed it"
        );
        assert!(
            !drawn_in(&bones::lines(&p.state), SELECTED).is_empty(),
            "{holder:?}: the bone the click picked is not drawn as the picked one"
        );
    }
}

#[test]
fn a_bone_picked_in_the_animation_preview_is_drawn_in_the_highlight_colour() {
    a_bone_picked_in_the_animation_preview::<Gen8>();
    a_bone_picked_in_the_animation_preview::<Gen9>();
}

/// A particle effect list of target `T` whose model is the rig of `rigged`, open with the model selected.
fn a_particle_model_with_a_rig<T: Held + Fixture>() -> (AppState, DocumentId) {
    let mut drawable = rigged::<T>();
    let skeleton = drawable.base.skeleton.as_mut().expect("a rig");
    skeleton.default_transforms = vec![Mat34V::default(); 2];
    skeleton.inverse_transforms = vec![Mat34V::default(); 2];
    skeleton.rebuild_rest_pose(0).expect("the rig derives");
    let mut list = a_list::<T>();
    let models = list.model_dict.as_mut().expect("a model dictionary");
    models.entries[0] = Some(Box::new(drawable.base));
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document("C:/mods/held.ypt".into(), T::particles(list), None, None);
    state.active_workspace = Workspace::Model;
    state.selection.select(doc, ypt_model_id(MODEL));
    (state, doc)
}

fn bones_of_a_particle_model<T: Held + Fixture>() {
    let (mut state, doc) = a_particle_model_with_a_rig::<T>();
    assert!(
        bones::can_draw(&state),
        "a particle model with a rig offers no Bones"
    );
    assert!(
        bones::lines(&state).is_empty(),
        "the bones draw with the toggle off"
    );

    state.preview_scene.show_rest_bones = true;
    let drawn = drawn_in(&bones::lines(&state), BONE);
    assert_eq!(
        match_up(drawn, &[the_rigs_segment()]),
        Ok(Vec::new()),
        "the model's bones are not drawn from its rig"
    );

    let row = dict_entry_part_id(MODEL, YPT_MODEL_KIND_BONE, 1);
    select_under(
        &mut state,
        &ray(MIDDLE_OF_THE_BONE, Vec3::X, 2.0, egui::Vec2::ZERO),
    );
    assert_eq!(
        state.selection.selected(doc),
        Some(row),
        "a click on the bone's line missed its row"
    );
    assert!(
        the_outline_shows(&mut state, "BONE_1 (1)"),
        "the Outline does not show the bone clicked"
    );
    assert!(
        !drawn_in(&bones::lines(&state), SELECTED).is_empty(),
        "the bone picked is not drawn in the highlight colour, or the model stopped being drawn with it"
    );
}

#[test]
fn a_particle_models_bones_are_drawn_and_a_click_on_one_selects_its_outline_row() {
    bones_of_a_particle_model::<Gen8>();
    bones_of_a_particle_model::<Gen9>();
}

fn a_tiny_position_is_read_in_the_inspector<T: Held>() {
    let Rig {
        mut state,
        doc,
        holder,
        ..
    } = viewed::<T>(Holder::Drawable);
    state.set_active_document(doc);
    state.selection.select(doc, holder.bone(1));
    let at = BoneAddress { owner: 0, bone: 1 };
    state
        .edit_bone(doc, at, BoneEdit::Position([1.0e-5, 0.0, 0.0]))
        .expect("the position edits");
    let mut inspector = Inspector::default();
    inspector.frame(&mut state, Vec::new());
    let shown = texts(&inspector.frame(&mut state, Vec::new()));
    assert!(
        shown.iter().any(|(text, _)| text.contains("0.00001")),
        "a position of a hundred-thousandth of a metre does not read as one: {:?}",
        shown.iter().map(|(text, _)| text).collect::<Vec<_>>()
    );
}

#[test]
fn a_bones_position_of_a_hundred_thousandth_of_a_metre_is_read_as_that() {
    a_tiny_position_is_read_in_the_inspector::<Gen8>();
    a_tiny_position_is_read_in_the_inspector::<Gen9>();
}

fn the_fields_of_an_unedited_bone<T: Held>() {
    for holder in Holder::ALL {
        let Rig { mut state, doc, .. } = viewed::<T>(holder);
        state.set_active_document(doc);
        state.selection.select(doc, holder.bone(1));
        let mut inspector = Inspector::default();
        for _ in 0..4 {
            inspector.frame(&mut state, Vec::new());
        }
        let document = state.document(doc).expect("open");
        assert!(
            document.history.done().is_empty() && !document.dirty,
            "{holder:?}: the Inspector drew a bone and edited its document"
        );
    }
}

#[test]
fn drawing_the_fields_of_a_bone_nobody_edited_edits_nothing() {
    the_fields_of_an_unedited_bone::<Gen8>();
    the_fields_of_an_unedited_bone::<Gen9>();
}

fn editing_the_bone_of_a_particle_model<T: Held + Fixture>() {
    let (mut state, doc) = a_particle_model_with_a_rig::<T>();
    state.preview_scene.show_rest_bones = true;
    let at = BoneAddress { owner: 0, bone: 1 };
    let row = dict_entry_part_id(MODEL, YPT_MODEL_KIND_BONE, 1);
    let payload = &state.document(doc).expect("open").payload;
    assert_eq!(
        super::bone_numbers::bone_selected(payload, row),
        Some(at),
        "the bone row of a particle model names no bone"
    );
    let before = drawn_in(&bones::lines(&state), BONE);

    // A metre along x from the root, which rests at (0, 1, 0).
    state
        .edit_bone(doc, at, BoneEdit::Position([1.0, 0.0, 0.0]))
        .expect("the position of a particle model's bone edits");
    let moved = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
    assert_eq!(
        match_up(drawn_in(&bones::lines(&state), BONE), &[moved]),
        Ok(Vec::new()),
        "the line did not move with the bone"
    );

    assert!(state.undo().is_some(), "the edit undoes");
    assert_eq!(
        match_up(drawn_in(&bones::lines(&state), BONE), &before),
        Ok(Vec::new()),
        "the undo did not put the line back"
    );

    // The Inspector offers the fields once the bone is the row selected
    state.set_active_document(doc);
    state.selection.select(doc, row);
    let mut inspector = Inspector::default();
    inspector.frame(&mut state, Vec::new());
    let shown = texts(&inspector.frame(&mut state, Vec::new()));
    assert!(
        shown.iter().any(|(text, _)| text == "Rotation (°, XYZ)"),
        "the Inspector offers no fields for the bone of a particle model: {:?}",
        shown.iter().map(|(text, _)| text).collect::<Vec<_>>()
    );
}

#[test]
fn the_bone_of_a_particle_model_is_edited_from_the_inspector_and_undone() {
    editing_the_bone_of_a_particle_model::<Gen8>();
    editing_the_bone_of_a_particle_model::<Gen9>();
}

fn an_edited_particle_model_bone_is_saved<T: Held + Fixture>() {
    let (mut state, doc) = a_particle_model_with_a_rig::<T>();
    let at = BoneAddress { owner: 0, bone: 1 };
    state
        .edit_bone(doc, at, BoneEdit::Position([1.0, 0.0, 0.0]))
        .expect("the position edits");
    let DocPayload::Resource(asset) = &state.document(doc).expect("open").payload else {
        panic!("a particle file is a resource");
    };
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("held.ypt");
    std::fs::write(&path, asset.to_bytes().expect("the edited list saves")).expect("written");
    let reread = DocPayload::Resource(or_io::open(&path).expect("what was saved opens"));
    let saved = skeleton_of(&reread, 0).expect("the model keeps its skeleton");
    assert_eq!(
        saved.bones[1].translation.x, 1.0,
        "the saved file does not hold the edit"
    );
}

#[test]
fn an_edited_particle_model_bone_reaches_the_saved_file() {
    an_edited_particle_model_bone_is_saved::<Gen8>();
    an_edited_particle_model_bone_is_saved::<Gen9>();
}

fn the_fields_of_a_fragments_bone_say_its_physics_stays<T: Held>() {
    for holder in Holder::ALL {
        let Rig {
            mut state,
            doc,
            holder,
            ..
        } = viewed::<T>(holder);
        let note = |state: &mut AppState| {
            state.set_active_document(doc);
            state.selection.select(doc, holder.bone(1));
            let mut inspector = Inspector::default();
            inspector.frame(state, Vec::new());
            let shown = texts(&inspector.frame(state, Vec::new()));
            shown
                .iter()
                .any(|(text, _)| text == crate::ui::panels::inspector::PHYSICS_STAYS)
        };
        assert!(
            !note(&mut state),
            "{holder:?}: the note about physics is drawn over a bone of a document with none"
        );
        let gave = match state.document_mut(doc).map(|d| &mut d.payload) {
            Some(DocPayload::Resource(or_io::Asset::Fragment(f))) => {
                f.physics_lod_group = Some(Box::default());
                true
            }
            Some(DocPayload::Resource(or_io::Asset::FragmentGen9(f))) => {
                f.physics_lod_group = Some(Box::default());
                true
            }
            _ => false,
        };
        if gave {
            assert!(
                note(&mut state),
                "{holder:?}: a fragment with physics says nothing of it over a bone"
            );
        }
    }
}

#[test]
fn the_fields_of_a_fragments_bone_say_its_physics_bounds_stay_where_they_are() {
    the_fields_of_a_fragments_bone_say_its_physics_stays::<Gen8>();
    the_fields_of_a_fragments_bone_say_its_physics_stays::<Gen9>();
}
