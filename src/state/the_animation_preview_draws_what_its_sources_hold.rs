//! The Animation preview draws a skeleton source's bones as edited, and a mesh source's meshes
//! with its own embedded textures, whichever document is active.

use std::path::PathBuf;

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::anim::{format_type, track};
use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::{Gen8, Gen9, Keyed, Open};
use or_io::desc::ScalarValue;
use or_io::gta5::{
    AtMap, AtMapEntry, AtString, CrAnimDictionary, CrAnimation, CrBoneData, CrClip,
    CrClipAnimation, CrClipAny, CrClipDictionary, CrSkeletonData, Dof, DrawableTarget, EClipType,
    File, GtaDrawable, Vec4V,
};
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8, TARGET_GEN9};
use or_transport::TimeRange;

use super::an_edit_stays_where_it_was_made::open;
use super::opening_a_ydd::finish_load;
use super::texture_resolution::{report, resolve, ResolutionRule};
use super::textures::{textures_in, TextureListing};
use super::{AppState, DocPayload, DocumentId, Game, MeshSource, SkeletonSource};
use crate::render::meshes::build_meshes_for_active_document;
use crate::render::viewport::{bound_texture, MeshCacheKey};
use crate::ui::dock::Workspace;

pub(super) const CLIP: u32 = 0x1234;
pub(super) const OTHER_CLIP: u32 = 0x1235;
pub(super) const ANIMATION: u32 = 0x1000;
/// The bone tags of [`rigged`], root first.
const TAGS: [u16; 2] = [100, 200];
const TARGETS: [(&str, Game); 2] = [
    (TARGET_GEN8, Game::GtaVLegacy),
    (TARGET_GEN9, Game::GtaVEnhanced),
];

fn one_entry<T: or_io::decl::Owned>(key: u32, data: T) -> AtMap<T>
where
    or_io::decl::Raw<T>: or_io::decl::Record,
{
    AtMap {
        hash: vec![Some(Box::new(AtMapEntry {
            key,
            data: Some(Box::new(data)),
            ..AtMapEntry::default()
        }))],
        ..AtMap::default()
    }
}

/// A dictionary of two clips, [`CLIP`] and [`OTHER_CLIP`], that rotate the bones of [`TAGS`].
pub(super) fn a_clip_over_the_rig() -> CrClipDictionary {
    let dofs = TAGS
        .iter()
        .map(|tag| Dof {
            id: *tag,
            r#type: format_type::QUATERNION,
            track: track::BONE_ROTATION,
            ..Dof::default()
        })
        .collect();
    let animation = CrAnimation {
        num_frames: 2,
        frames_per_chunk: 127,
        duration: 1.0,
        dofs,
        ..CrAnimation::default()
    };
    let clip = |name: &str| {
        CrClipAny::CrClipAnimation(CrClipAnimation {
            base: CrClip {
                r#type: Open::new(EClipType::Animation),
                name: AtString::new(name),
                ..CrClip::default()
            },
            animation: Keyed::to(ANIMATION),
            end_time: 1.0,
            rate: 1.0,
            ..CrClipAnimation::default()
        })
    };
    let mut clips = one_entry(CLIP, clip("idle"));
    clips.hash.extend(one_entry(OTHER_CLIP, clip("walk")).hash);
    CrClipDictionary {
        anim_dictionary: Some(Box::new(CrAnimDictionary {
            animations: one_entry(ANIMATION, animation),
            ..CrAnimDictionary::default()
        })),
        clips,
        ..CrClipDictionary::default()
    }
}

/// A drawable of target `T` whose skeleton has a bone for each of [`TAGS`], each a metre above its parent.
// The struct update the lint asks for does not compile: the drawable holds private fields.
#[allow(clippy::field_reassign_with_default)]
pub(super) fn rigged<T: DrawableTarget>() -> GtaDrawable<T> {
    let bone = |index: usize| CrBoneData {
        rotation: Vec4V {
            w: 1.0,
            ..Vec4V::default()
        },
        translation: Vec4V {
            y: 1.0,
            ..Vec4V::default()
        },
        scale: Vec4V {
            x: 1.0,
            y: 1.0,
            z: 1.0,
            ..Vec4V::default()
        },
        next_index: -1,
        parent_index: index as i16 - 1,
        name: format!("BONE_{index}"),
        index: index as u16,
        bone_id: TAGS[index],
        ..CrBoneData::default()
    };
    let mut drawable = GtaDrawable::<T>::default();
    drawable.base.skeleton = Some(Box::new(CrSkeletonData {
        parent_indices: (0..TAGS.len()).map(|i| i as i16 - 1).collect(),
        bones: (0..TAGS.len()).map(bone).collect(),
        ..CrSkeletonData::default()
    }));
    drawable
}

/// The rigged drawable of one target open as the preview's skeleton source, a clip over its bones chosen and active.
struct Previewing {
    _dir: tempfile::TempDir,
    state: AppState,
    drawable: DocumentId,
    clips: DocumentId,
}

fn previewing<T: DrawableTarget>(game: Game) -> Previewing
where
    GtaDrawable<T>: File,
{
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("rig.ydr");
    std::fs::write(&path, rigged::<T>().to_bytes().expect("a rig saves")).expect("written");
    let mut state = AppState::new();
    state.documents.clear();
    // ⚠ Under Legacy, opening an Enhanced file asks to switch first.
    state.prefs.active_game = Some(game);
    let drawable = open(&mut state, &path);
    let clips = DocPayload::Resource(or_io::Asset::Clips(a_clip_over_the_rig()));
    let clips = state.insert_document(dir.path().join("clips.ycd"), clips, None, None);
    state.active_workspace = Workspace::Animation;
    state.preview_scene.set_clip(clips, CLIP);
    state
        .preview_scene
        .pin_skeleton(Some(SkeletonSource::Drawable(drawable)));
    Previewing {
        _dir: dir,
        state,
        drawable,
        clips,
    }
}

/// The row of bone `bone`'s translation along x in `doc`'s outline: its label and its edit.
fn bone_x(state: &AppState, doc: DocumentId, bone: usize) -> (String, or_editor::Edit) {
    fn find(node: &or_editor::Node, bone: usize) -> Option<&or_editor::Node> {
        let it = node.type_name.as_deref() == Some("CrBoneData")
            && node.hop == Some(or_editor::Hop::Index(bone));
        it.then_some(node)
            .or_else(|| node.children.iter().find_map(|child| find(child, bone)))
    }
    let doc = state.document(doc).expect("open");
    let outline = crate::ui::panels::hierarchy::described_outline(doc).expect("an outline");
    let node = find(&outline.root, bone).expect("the bone is in the outline");
    let (pack, _) = super::described_root(doc).expect("a drawable is described");
    let image = doc.resource.as_ref().expect("the image it was read from");
    let (type_name, at) = (node.type_name.as_deref().expect("typed"), node.at);
    let summary = or_editor::Summary::of(&pack, type_name, image.view(), at).expect("a summary");
    let row = (summary.rows.iter())
        .find(|row| row.label == "Translation > X")
        .expect("a bone has a translation");
    (row.label.clone(), row.edit.expect("an editable row"))
}

/// What the preview poses of bone 1: its bind translation along x, and where its world matrix puts it along x.
fn posed(state: &AppState) -> (f32, f32) {
    let skeleton = state.anim.skeleton().expect("a skeleton is bound");
    (
        skeleton.bones[1].bind_translation.x,
        state.anim.world_matrices[1].w_axis.x,
    )
}

impl Previewing {
    fn needs_rebuild(&self) -> bool {
        let epoch = self.state.skeleton_epoch();
        (self.state.anim).needs_rebuild(&self.state.preview_scene, epoch)
    }
}

fn previewing_a_rig(game: Game) -> Previewing {
    match game {
        Game::GtaVLegacy => previewing::<Gen8>(game),
        _ => previewing::<Gen9>(game),
    }
}

#[test]
fn an_edit_to_a_bone_of_the_skeleton_source_poses_the_skeleton_again() {
    for (target, game) in TARGETS {
        let mut p = previewing_a_rig(game);
        p.state.tick_animation();
        assert_eq!(posed(&p.state), (0.0, 0.0), "{target}: the rig as authored");
        assert!(
            !p.needs_rebuild(),
            "{target}: the session is built again with nothing changed"
        );

        let (label, edit) = bone_x(&p.state, p.drawable, 1);
        assert!(p
            .state
            .patch_scalar(p.drawable, &label, edit, ScalarValue::F32(5.0)));
        // A drag writes the image each frame, and the contents are read from it once the gesture ends.
        assert!(
            !p.needs_rebuild(),
            "{target}: the session is built again for each frame of a drag"
        );
        p.state.seal_edits();
        assert!(
            p.needs_rebuild(),
            "{target}: the edit of the contents asks for no new session"
        );
        p.state.tick_animation();
        assert_eq!(
            posed(&p.state),
            (5.0, 5.0),
            "{target}: the preview poses the bone as it was before the edit"
        );
        assert!(
            !p.needs_rebuild(),
            "{target}: the session is built again with nothing changed"
        );

        // The undo is posed as well, though the clip dictionary is the active document.
        p.state.set_active_document(p.drawable);
        assert!(p.state.undo().is_some());
        p.state.set_active_document(p.clips);
        p.state.tick_animation();
        assert_eq!(
            posed(&p.state),
            (0.0, 0.0),
            "{target}: the undo of the edit is not posed"
        );
    }
}

/// ⚠ A frame ticks the animation first and draws last, and an edit lands between them: the menu's Undo
/// does, and the preview's session is still the one built before it when the viewport builds its meshes.
#[test]
fn the_meshes_are_built_again_once_the_session_that_poses_them_is() {
    for (target, game) in TARGETS {
        let mut p = previewing_a_rig(game);
        p.state.tick_animation();
        let key = |p: &Previewing| MeshCacheKey::of(&p.state, p.clips, 0);

        let (label, edit) = bone_x(&p.state, p.drawable, 1);
        assert!(p
            .state
            .patch_scalar(p.drawable, &label, edit, ScalarValue::F32(5.0)));
        p.state.seal_edits();
        let beside_the_old_session = key(&p);
        p.state.tick_animation();
        assert_eq!(posed(&p.state), (5.0, 5.0), "{target}: the session");
        assert!(
            key(&p) != beside_the_old_session,
            "{target}: the meshes stay retargeted onto the skeleton before the edit"
        );

        p.state.set_active_document(p.drawable);
        assert!(p.state.undo().is_some());
        p.state.set_active_document(p.clips);
        let beside_the_old_session = key(&p);
        p.state.tick_animation();
        assert_eq!(posed(&p.state), (0.0, 0.0), "{target}: the session");
        assert!(
            key(&p) != beside_the_old_session,
            "{target}: the meshes stay retargeted onto the skeleton before the undo"
        );

        // Nothing changed, so nothing is built again.
        let drawn = key(&p);
        p.state.tick_animation();
        assert!(key(&p) == drawn, "{target}: the meshes are built per frame");
    }
}

/// The clip's own working range and Loop, trimmed and turned on: what a user sets, and the preview must keep.
fn trimmed_and_looping(p: &mut Previewing, target: &str) -> TimeRange {
    assert!(
        !p.state.anim_transport.is_looping(),
        "{target}: the clip's own loop"
    );
    let trimmed = TimeRange::new(0.25, 0.75);
    p.state.anim_transport.set_range(trimmed);
    p.state.anim_transport.set_looping(true);
    trimmed
}

/// ⚠ The transport holds what the user set: a working range, and Loop. An edit of a bone changes
/// neither the clip nor its length, so it must not hand them back to the clip's own.
#[test]
fn an_edit_to_the_skeleton_source_keeps_the_transports_working_range_and_loop() {
    for (target, game) in TARGETS {
        let mut p = previewing_a_rig(game);
        p.state.tick_animation();
        let trimmed = trimmed_and_looping(&mut p, target);

        let (label, edit) = bone_x(&p.state, p.drawable, 1);
        assert!(p
            .state
            .patch_scalar(p.drawable, &label, edit, ScalarValue::F32(5.0)));
        p.state.seal_edits();
        p.state.tick_animation();
        assert_eq!(posed(&p.state), (5.0, 5.0), "{target}: the session");
        assert_eq!(
            p.state.anim_transport.range, trimmed,
            "{target}: the working range after an edit of a bone"
        );
        assert!(
            p.state.anim_transport.is_looping(),
            "{target}: Loop after an edit of a bone"
        );
    }
}

/// A key on bone 1's rotation at frame 0: what Insert key records, and what a bake encodes.
fn a_key() -> (or_anim::EditKey, or_anim::EditValue) {
    let key = or_anim::EditKey {
        bone: 1,
        track: or_anim::EditTrack::Rotation,
        frame: 0,
    };
    (
        key,
        or_anim::EditValue::Rotation(glam::Quat::from_rotation_z(0.5)),
    )
}

/// What the Insert key button does, on the clip's document.
fn insert_a_key(p: &mut Previewing) {
    let before = p.state.anim_edits.clone();
    let (key, value) = a_key();
    assert!(p.state.anim_edits.set(key, value));
    p.state.push_anim_edit("insert key", before);
    p.state.anim.invalidate();
}

/// What the Edit clip fields section does when `edit` changes the clip's fields.
fn edit_the_clip_fields(
    p: &mut Previewing,
    edit: impl FnOnce(&mut crate::state::ClipFieldSnapshot),
) {
    let before = (p.state.clip_field_snapshot(p.clips, CLIP)).expect("the clip");
    let mut edited = before.clone();
    edit(&mut edited);
    assert!(p.state.apply_clip_fields(p.clips, CLIP, &edited));
    let change = crate::state::Change::ClipFields {
        clip_key: CLIP,
        other: before,
    };
    p.state.record(p.clips, "Edit clip fields", change);
    p.state.anim.invalidate();
}

fn rename_the_clip(p: &mut Previewing) {
    edit_the_clip_fields(p, |fields| fields.name = "run".into());
}

/// What a panel or a command does to the preview, by name.
type Action = (&'static str, fn(&mut Previewing));

/// What the preview's panels and commands do to the session short of choosing another clip or
/// skeleton source: each asks for it to be built again, for the clip and skeleton it plays still.
fn the_rebuilds_that_choose_nothing() -> Vec<Action> {
    vec![
        ("Insert key", insert_a_key),
        ("Clear key", |p| {
            insert_a_key(p);
            let before = p.state.anim_edits.clone();
            assert!(p.state.anim_edits.remove(&a_key().0));
            p.state.push_anim_edit("clear key", before);
            p.state.anim.invalidate();
        }),
        ("Discard edits", |p| {
            insert_a_key(p);
            let before = p.state.anim_edits.clone();
            p.state.anim_edits.clear();
            p.state.push_anim_edit("discard edits", before);
            p.state.anim.invalidate();
        }),
        ("the root-motion toggle", |p| {
            p.state.preview_scene.apply_root_motion ^= true;
            p.state.anim.invalidate();
        }),
        ("Allow mismatched skeleton", |p| {
            p.state.preview_scene.allow_mismatched_skeleton ^= true;
            p.state.anim.invalidate();
        }),
        ("a mesh source added", |p| {
            let source = MeshSource {
                document: p.drawable,
                entry: None,
            };
            p.state.preview_scene.add_mesh(source);
            p.state.anim.invalidate();
        }),
        ("a mesh source removed", |p| {
            let source = MeshSource {
                document: p.drawable,
                entry: None,
            };
            p.state.preview_scene.add_mesh(source);
            p.state.preview_scene.remove_mesh(source);
            p.state.anim.invalidate();
        }),
        ("a clip field edit", rename_the_clip),
        ("the undo of a clip field edit", |p| {
            rename_the_clip(p);
            p.state.set_active_document(p.clips);
            assert!(p.state.undo().is_some());
        }),
        ("the undo of a pose edit", |p| {
            insert_a_key(p);
            p.state.set_active_document(p.clips);
            assert!(p.state.undo().is_some());
        }),
        ("a bake", |p| {
            insert_a_key(p);
            let baked = p.state.bake_anim_edits();
            assert!(matches!(baked, Ok(Some(_))), "{baked:?}");
        }),
        ("the clip chosen again", |p| {
            p.state.preview_scene.set_clip(p.clips, CLIP);
            p.state.anim.invalidate();
        }),
        ("Choose automatically landing on the same skeleton", |p| {
            p.state.preview_scene.unpin_skeleton();
            p.state.anim.invalidate();
        }),
    ]
}

/// ⚠ `invalidate` asks for a build, not for a new choice: every one of these calls it with the clip
/// and the skeleton source unchanged, and none of them may hand the transport back.
#[test]
fn a_rebuild_that_chooses_nothing_new_keeps_the_transports_working_range_and_loop() {
    for (name, rebuild) in the_rebuilds_that_choose_nothing() {
        for (target, game) in TARGETS {
            let mut p = previewing_a_rig(game);
            p.state.tick_animation();
            let trimmed = trimmed_and_looping(&mut p, target);

            rebuild(&mut p);
            assert!(
                p.needs_rebuild(),
                "{target}: {name} asks for the session to be built again"
            );
            p.state.tick_animation();
            assert!(!p.needs_rebuild(), "{target}: {name} is built");
            assert_eq!(
                p.state.anim_transport.range, trimmed,
                "{target}: the working range after {name}"
            );
            assert!(
                p.state.anim_transport.is_looping(),
                "{target}: Loop after {name}"
            );
        }
    }
}

/// A second copy of the rig open and chosen as the skeleton source: another source over the same bones.
fn choose_another_rig(p: &mut Previewing) {
    let path = p._dir.path().join("rig2.ydr");
    let rig = match p.state.prefs.active_game {
        Some(Game::GtaVLegacy) => rigged::<Gen8>().to_bytes(),
        _ => rigged::<Gen9>().to_bytes(),
    };
    std::fs::write(&path, rig.expect("a rig saves")).expect("written");
    let second = open(&mut p.state, &path);
    let source = SkeletonSource::Drawable(second);
    p.state.preview_scene.pin_skeleton(Some(source));
    p.state.anim.invalidate();
}

/// ⚠ What a rebuild keeps is for the clip and the skeleton source it plays: choosing either is a
/// new choice, and plays the clip's own range and Loop.
#[test]
fn choosing_another_clip_or_skeleton_source_hands_the_transport_back_to_the_clips_own() {
    let choices: [Action; 2] = [
        ("another clip", |p| {
            p.state.preview_scene.set_clip(p.clips, OTHER_CLIP);
            p.state.anim.invalidate();
        }),
        ("another skeleton source", choose_another_rig),
    ];
    for (name, choose) in choices {
        for (target, game) in TARGETS {
            let mut p = previewing_a_rig(game);
            p.state.tick_animation();
            let clips_own = p.state.anim_transport;
            trimmed_and_looping(&mut p, target);

            choose(&mut p);
            p.state.tick_animation();
            assert_eq!(
                p.state.anim_transport.range, clips_own.range,
                "{target}: the working range after choosing {name}"
            );
            assert!(
                !p.state.anim_transport.is_looping(),
                "{target}: Loop after choosing {name}"
            );

            // The transport is the new choice's now, and a rebuild for it keeps what is set.
            let trimmed = trimmed_and_looping(&mut p, target);
            p.state.anim.invalidate();
            p.state.tick_animation();
            assert_eq!(
                p.state.anim_transport.range, trimmed,
                "{target}: the working range on {name}, rebuilt"
            );
        }
    }
}

/// ⚠ A clip's length belongs to the clip, not to the user: a rebuild for the same clip follows it,
/// and keeps the user's range inside it. A range that was never trimmed is the clip's own, so it follows too.
#[test]
fn a_clip_field_edit_moves_the_transports_length_and_keeps_the_users_range_and_loop() {
    for (target, game) in TARGETS {
        let mut p = previewing_a_rig(game);
        p.state.tick_animation();
        let set_the_end = |p: &mut Previewing, end_time: f32| {
            let mut fields = (p.state.clip_field_snapshot(p.clips, CLIP)).expect("the clip");
            fields.end_time = end_time;
            assert!(p.state.apply_clip_fields(p.clips, CLIP, &fields));
            p.state.anim.invalidate();
            p.state.tick_animation();
        };

        p.state.anim_transport.set_looping(true);
        set_the_end(&mut p, 0.5);
        assert_eq!(
            p.state.anim_transport.range,
            TimeRange::from_duration(0.5),
            "{target}: an untrimmed range after the clip is shortened"
        );
        set_the_end(&mut p, 1.0);
        assert_eq!(
            p.state.anim_transport.range,
            TimeRange::from_duration(1.0),
            "{target}: an untrimmed range after the clip is lengthened"
        );

        p.state.anim_transport.set_range(TimeRange::new(0.25, 0.75));
        set_the_end(&mut p, 0.5);
        assert_eq!(
            p.state.anim_transport.bounds,
            TimeRange::from_duration(0.5),
            "{target}: the length after the clip is shortened"
        );
        assert_eq!(
            p.state.anim_transport.range,
            TimeRange::new(0.25, 0.5),
            "{target}: a trimmed range after the clip is shortened"
        );
        assert!(p.state.anim_transport.is_looping(), "{target}: Loop");
    }
}

/// ⚠ The clip's Looped field is the clip's own word, not the user's: the Loop checkbox in Preview
/// settings, its undo and its redo reach the transport, and a rebuild that leaves the field alone does not.
#[test]
fn the_clips_looped_field_sets_the_transports_loop_when_it_changes_and_not_otherwise() {
    for (target, game) in TARGETS {
        let mut p = previewing_a_rig(game);
        p.state.tick_animation();
        let trimmed = trimmed_and_looping(&mut p, target);
        p.state.anim_transport.set_looping(false);
        let loops = |p: &Previewing| p.state.anim_transport.is_looping();

        // ⚠ A frame seals the edits, or the three that follow fold into one step of the history.
        edit_the_clip_fields(&mut p, |fields| fields.looped = true);
        p.state.seal_edits();
        p.state.tick_animation();
        assert!(loops(&p), "{target}: Loop once the field is ticked");
        assert_eq!(
            p.state.anim_transport.range, trimmed,
            "{target}: the working range once the field is ticked"
        );

        // The Loop button is the user's while the field stays as it is.
        p.state.anim_transport.set_looping(false);
        rename_the_clip(&mut p);
        p.state.seal_edits();
        p.state.tick_animation();
        assert!(!loops(&p), "{target}: Loop after a rename, field ticked");

        // The field changed, so it wins over the button.
        p.state.anim_transport.set_looping(true);
        edit_the_clip_fields(&mut p, |fields| fields.looped = false);
        p.state.seal_edits();
        p.state.tick_animation();
        assert!(!loops(&p), "{target}: Loop once the field is cleared");

        p.state.set_active_document(p.clips);
        assert!(p.state.undo().is_some(), "{target}: undo");
        p.state.tick_animation();
        assert!(loops(&p), "{target}: Loop after the undo of the clear");
        assert!(p.state.redo().is_some(), "{target}: redo");
        p.state.tick_animation();
        assert!(!loops(&p), "{target}: Loop after the redo of the clear");

        assert_eq!(
            p.state.anim_transport.range, trimmed,
            "{target}: the working range after the edits of the field"
        );
    }
}

/// A staged drawable of `target` whose meshes ask for textures it embeds: its path and those names.
fn embedding(target: &str, game: Game) -> Option<(PathBuf, Vec<String>)> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
        .target(target)
        .find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files.into_iter().find_map(|path| {
        let mut state = AppState::new();
        state.documents.clear();
        state.prefs.active_game = Some(game);
        state.load_file(path.clone());
        let drawable = finish_load(&mut state).ok()?;
        state.active_workspace = Workspace::Animation;
        state.preview_scene.add_mesh(MeshSource {
            document: drawable,
            entry: None,
        });
        let names = embedded_and_asked_for(&state, drawable);
        (!names.is_empty()).then_some((path, names))
    })
}

/// The textures `drawable` embeds, which the preview decodes and its meshes in the preview ask for.
fn embedded_and_asked_for(state: &AppState, drawable: DocumentId) -> Vec<String> {
    let doc = state.document(drawable).expect("open");
    let TextureListing::Textures { textures, .. } = textures_in(&doc.payload, None) else {
        return Vec::new();
    };
    let held = |name: &String| {
        (textures.iter())
            .any(|t| t.format != GrcTextureFormat::None && t.name.eq_ignore_ascii_case(name))
    };
    let mut asked: Vec<String> = (build_meshes_for_active_document(state).into_iter())
        .filter_map(|upload| upload.texture_name)
        .filter(held)
        .collect();
    asked.sort();
    asked.dedup();
    asked
}

#[test]
fn a_mesh_source_is_drawn_with_the_textures_it_embeds_while_a_clip_dictionary_is_active() {
    const WHAT: &str =
        "a_mesh_source_is_drawn_with_the_textures_it_embeds_while_a_clip_dictionary_is_active";
    for (target, game) in TARGETS {
        let Some((path, names)) = embedding(target, game) else {
            skip_corpus(
                WHAT,
                format_args!("no {target} .ydr whose meshes ask for a texture it embeds"),
            );
            continue;
        };
        let mut state = AppState::new();
        state.documents.clear();
        state.prefs.active_game = Some(game);
        let drawable = open(&mut state, &path);
        let clips = DocPayload::Resource(or_io::Asset::Clips(Default::default()));
        let clips = state.insert_document("clips.ycd".into(), clips, None, None);
        state.active_workspace = Workspace::Animation;
        state.preview_scene.add_mesh(MeshSource {
            document: drawable,
            entry: None,
        });
        assert_eq!(state.active_document, Some(clips), "{target}: the focus");

        let drawn = build_meshes_for_active_document(&state);
        let asking: Vec<_> = (drawn.iter())
            .filter(|u| u.texture_name.as_ref().is_some_and(|n| names.contains(n)))
            .collect();
        assert!(
            !asking.is_empty(),
            "{path:?}: the preview draws no mesh of it"
        );
        for upload in &asking {
            let name = upload.texture_name.as_deref().expect("named");
            let found = bound_texture(&state, upload.source, None, name);
            assert!(
                found.is_some_and(|(texture, _)| texture.has_pixel_data()),
                "{target}: a mesh of {path:?} draws without {name}, which it embeds"
            );
        }

        // What the viewport's badge says of them agrees.
        let wanted = drawn
            .iter()
            .filter_map(|u| Some((u.source, u.texture_name.clone()?)));
        for outcome in report(&state, None, wanted, 0).outcomes {
            if !names.contains(&outcome.name) {
                continue;
            }
            let origin = outcome.found.expect("the badge names a source");
            assert_eq!(
                (origin.document, origin.rule),
                (drawable, ResolutionRule::Embedded),
                "{target}: where the badge says {} comes from",
                outcome.name
            );
        }
    }
}

#[test]
fn two_mesh_sources_embedding_one_name_are_each_drawn_with_their_own_pixels() {
    const WHAT: &str = "two_mesh_sources_embedding_one_name_are_each_drawn_with_their_own_pixels";
    let Some((path, names)) = embedding(TARGET_GEN8, Game::GtaVLegacy) else {
        skip_corpus(
            WHAT,
            "no Legacy .ydr whose meshes ask for a texture it embeds",
        );
        return;
    };
    let dir = tempfile::tempdir().expect("a scratch directory");
    let copy = |name: &str| -> PathBuf {
        let to = dir.path().join(name);
        std::fs::copy(&path, &to).expect("copied");
        to
    };
    let mut state = AppState::new();
    state.documents.clear();
    let (first, second) = (
        open(&mut state, &copy("a.ydr")),
        open(&mut state, &copy("b.ydr")),
    );
    let clips = DocPayload::Resource(or_io::Asset::Clips(Default::default()));
    state.insert_document(dir.path().join("clips.ycd"), clips, None, None);
    state.active_workspace = Workspace::Animation;
    for document in [first, second] {
        state.preview_scene.add_mesh(MeshSource {
            document,
            entry: None,
        });
    }

    // The second drawable's copy of the texture is encoded in another format.
    let name = &names[0];
    let doc = state.document(second).expect("open");
    let TextureListing::Textures { textures, source } = textures_in(&doc.payload, None) else {
        panic!("{path:?}: a drawable lists its textures");
    };
    let held = textures.iter().find(|t| t.name.eq_ignore_ascii_case(name));
    let format = match held.map(|t| t.format) {
        Some(GrcTextureFormat::DXT1) => GrcTextureFormat::DXT5,
        _ => GrcTextureFormat::DXT1,
    };
    let reencoded = state.reencode_texture_in_place(
        second,
        source,
        name,
        FormatChoice::Fixed(format),
        &TextureBuildOptions::default(),
    );
    reencoded.expect("the texture re-encodes");

    let stamp = |from: DocumentId| {
        let from = Some(MeshSource {
            document: from,
            entry: None,
        });
        bound_texture(&state, from, None, name).map(|(_, stamp)| stamp)
    };
    assert!(
        stamp(first) != stamp(second),
        "{path:?}: the two copies of {name} are one picture"
    );
    for upload in build_meshes_for_active_document(&state) {
        let (Some(source), Some(asked)) = (upload.source, upload.texture_name.as_deref()) else {
            continue;
        };
        if asked != name {
            continue;
        }
        let found = resolve(&state, upload.source, None, asked).expect("it resolves");
        assert_eq!(
            found.origin.document, source.document,
            "{path:?}: a mesh of document {} binds the {asked} of document {}",
            source.document, found.origin.document
        );
    }
}

/// What a ped's fragment makes the preview draw when it is the skeleton and no mesh is added.
#[test]
fn a_peds_fragment_chosen_alone_draws_the_one_plane_its_own_bounds_state() {
    const WHAT: &str = "a_peds_fragment_chosen_alone_draws_the_one_plane_its_own_bounds_state";
    let staged = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft").find();
    for name in ["a_m_y_skater_01.yft", "a_f_y_hipster_01.yft"] {
        let Some(path) = (staged.iter()).find(|p| p.file_name().is_some_and(|n| n == name)) else {
            skip_corpus(WHAT, format_args!("{name} is not staged"));
            continue;
        };
        let mut state = AppState::new();
        state.documents.clear();
        let fragment = open(&mut state, path);
        state.active_workspace = Workspace::Animation;
        state
            .preview_scene
            .pin_skeleton(Some(SkeletonSource::Fragment(fragment)));

        let drawn = build_meshes_for_active_document(&state);
        let [plane] = drawn.as_slice() else {
            panic!("{name}: the preview draws {} meshes", drawn.len());
        };
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for vertex in &plane.vertices {
            for axis in 0..3 {
                low[axis] = low[axis].min(vertex.position[axis]);
                high[axis] = high[axis].max(vertex.position[axis]);
            }
        }
        let DocPayload::Resource(or_io::Asset::Fragment(held)) =
            &state.document(fragment).expect("open").payload
        else {
            panic!("{name} is not a Legacy fragment");
        };
        let bounds = &held
            .common_drawable
            .as_deref()
            .expect("a drawable")
            .base
            .lod_group;
        let (min, max) = (&bounds.bounds_box.min, &bounds.bounds_box.max);
        assert_eq!(
            (low, high),
            ([min.x, min.y, min.z], [max.x, max.y, max.z]),
            "{name}: the plane is not what the file's bounds state"
        );
        let side = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
        assert_eq!(
            side,
            [25.0, 25.0, 0.0],
            "{name}: the plane's sides in metres"
        );
        let skinning = plane.skinning.as_ref().expect("the plane is skinned");
        assert_eq!(skinning.bones_used, [0], "{name}: the bones that skin it");
    }
}
