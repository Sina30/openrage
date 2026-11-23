//! A bone's name, position, rotation and scale can be edited in the Inspector: an edit derives the rest
//! pose of the bone and of every bone under it again, keeps every bone's id and the bone-id table, is
//! one step of the document's history, and saves.

use std::path::PathBuf;

use or_anim::Skeleton;
use or_io::decl::{Bool8, Gen8, Gen9};
use or_io::gta5::{
    CrBoneData, CrSkeletonData, DrawableTarget, EnvironmentCloth, File, FragDrawable, FragType,
    GtaDrawable, GtaFragType, Mat34V, Vec4V,
};
use or_io::Asset;
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8, TARGET_GEN9};

use super::an_edit_stays_where_it_was_made::{open, texts, Inspector};
use super::bone_numbers::{bone_selected, euler_degrees, skeleton_of, BoneAddress, BoneEdit};
use super::nested_archives_survive_a_save::save;
use super::textures::FRAGMENT_ENTRY_HASH;
use super::the_animation_preview_draws_what_its_sources_hold::{a_clip_over_the_rig, rigged, CLIP};
use super::{AppState, DocPayload, DocumentId, Game, SkeletonSource};
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, pack_index, SECT_SKEL, YPT_MODEL_KIND_BONE,
};

const TARGETS: [(&str, Game); 2] = [
    (TARGET_GEN8, Game::GtaVLegacy),
    (TARGET_GEN9, Game::GtaVEnhanced),
];

fn lanes(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
    Vec4V {
        x,
        y,
        z,
        w,
        ..Vec4V::default()
    }
}

/// A drawable of target `T` of four bones: 0 the root, 1 and 3 under it, 2 under 1, each a metre along y
/// from its parent and the second turned a little, their rest poses derived.
// The struct update the lint asks for does not compile: the drawable holds private fields.
#[allow(clippy::field_reassign_with_default)]
fn four_bones<T: DrawableTarget>() -> GtaDrawable<T> {
    let bone = |index: u16, parent_index, next_index, rotation| CrBoneData {
        name: format!("BONE_{index}"),
        bone_id: [0, 0x2e28, 0xe0fd, 0x5c01][index as usize],
        index,
        parent_index,
        next_index,
        rotation,
        translation: lanes(0.0, 1.0, 0.0, 0.0),
        scale: lanes(1.0, 1.0, 1.0, 0.0),
        ..CrBoneData::default()
    };
    let straight = lanes(0.0, 0.0, 0.0, 1.0);
    let turned = lanes(0.0, 0.0, 0.258_819, 0.965_926);
    let mut skeleton = CrSkeletonData {
        bones: vec![
            bone(0, -1, -1, straight.clone()),
            bone(1, 0, 3, turned),
            bone(2, 1, -1, straight.clone()),
            bone(3, 0, -1, straight),
        ],
        parent_indices: vec![-1, 0, 1, 0],
        default_transforms: vec![Mat34V::default(); 4],
        inverse_transforms: vec![Mat34V::default(); 4],
        bone_tag_slot_count: 3,
        ..CrSkeletonData::default()
    };
    skeleton.rebuild_rest_pose(0).expect("the rig derives");
    let mut drawable = GtaDrawable::<T>::default();
    drawable.base.skeleton = Some(Box::new(skeleton));
    drawable
}

/// The same rig as a fragment's common drawable.
fn four_bones_in_a_fragment<T: DrawableTarget>() -> GtaFragType<T> {
    GtaFragType {
        base: FragType {
            common_drawable: Some(Box::new(FragDrawable {
                base: four_bones::<T>().base,
                load_skeleton: Bool8::new(true),
                ..FragDrawable::default()
            })),
            ..FragType::default()
        },
        ..GtaFragType::default()
    }
}

/// How many drawables of a document may hold a skeleton: a dictionary's entries, else one.
fn drawable_count(payload: &DocPayload) -> u32 {
    match payload {
        DocPayload::Resource(Asset::DrawableDict(dict)) => dict.entries.len() as u32,
        DocPayload::Resource(Asset::DrawableDictGen9(dict)) => dict.entries.len() as u32,
        _ => 1,
    }
}

/// Bone `bone` and every bone under it.
fn subtree(skeleton: &CrSkeletonData, bone: usize) -> Vec<usize> {
    let count = skeleton.bones.len();
    let under = |first: usize| {
        let mut at = first;
        for _ in 0..=count {
            if at == bone {
                return true;
            }
            match usize::try_from(skeleton.bones[at].parent_index) {
                Ok(parent) if parent < count => at = parent,
                _ => return false,
            }
        }
        false
    };
    (0..count).filter(|&b| under(b)).collect()
}

/// A bone with a bone under it, in a skeleton whose runs are one entry to a bone; one with another bone beside or above it where there is one.
fn editable_bone(skeleton: &CrSkeletonData) -> Option<usize> {
    let count = skeleton.bones.len();
    if skeleton.default_transforms.len() != count || skeleton.inverse_transforms.len() != count {
        return None;
    }
    let held = |bone| subtree(skeleton, bone).len();
    (1..count)
        .find(|&bone| held(bone) > 1 && held(bone) < count)
        .or_else(|| (0..count).find(|&bone| held(bone) > 1))
}

/// The Outline row, or the viewport pick, that names the bone at `at`.
fn outline_row(payload: &DocPayload, at: BoneAddress) -> u64 {
    let bone = at.bone as u32;
    let entry =
        |codes: &[u32]| dict_entry_part_id(codes[at.owner as usize], YPT_MODEL_KIND_BONE, bone);
    match payload {
        DocPayload::Resource(Asset::Drawable(_) | Asset::DrawableGen9(_)) => {
            pack_index(SECT_SKEL, bone)
        }
        DocPayload::Resource(Asset::DrawableDict(dict)) => entry(&dict.codes),
        DocPayload::Resource(Asset::DrawableDictGen9(dict)) => entry(&dict.codes),
        DocPayload::Resource(Asset::Fragment(_) | Asset::FragmentGen9(_)) => {
            dict_entry_part_id(FRAGMENT_ENTRY_HASH, YPT_MODEL_KIND_BONE, bone)
        }
        _ => panic!("the document holds no skeleton"),
    }
}

/// A document with a skeleton, open from a scratch folder, and the bone the tests edit.
struct Rig {
    label: String,
    _dir: tempfile::TempDir,
    path: PathBuf,
    state: AppState,
    doc: DocumentId,
    at: BoneAddress,
    row: u64,
}

impl Rig {
    fn open(label: String, name: &str, bytes: &[u8], game: Game, at: BoneAddress) -> Self {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("written");
        let mut state = AppState::new();
        state.documents.clear();
        // ⚠ Under Legacy, opening an Enhanced file asks to switch first.
        state.prefs.active_game = Some(game);
        let doc = open(&mut state, &path);
        let payload = &state.document(doc).expect("open").payload;
        let row = outline_row(payload, at);
        assert_eq!(
            bone_selected(payload, row),
            Some(at),
            "{label}: the row names the bone"
        );
        Self {
            label,
            _dir: dir,
            path,
            state,
            doc,
            at,
            row,
        }
    }

    fn skeleton(&self) -> &CrSkeletonData {
        let payload = &self.state.document(self.doc).expect("open").payload;
        skeleton_of(payload, self.at.owner).expect("a skeleton")
    }

    /// The skeleton the saved file holds, read from it.
    fn on_disk(&self) -> CrSkeletonData {
        let asset = or_io::open(&self.path).expect("what was saved opens");
        let payload = DocPayload::Resource(asset);
        skeleton_of(&payload, self.at.owner)
            .expect("a skeleton")
            .clone()
    }

    /// The file as a save writes it, whether or not anything was edited, and what it holds.
    fn save(&mut self) -> Vec<u8> {
        // ⚠ This forgets the history, so it is for a save nothing is to undo.
        self.state
            .document_mut(self.doc)
            .expect("open")
            .mark_edited();
        save(&mut self.state, self.doc);
        std::fs::read(&self.path).expect("saved")
    }

    /// What the user does to see the bone's fields: the Outline row selected in the active document.
    fn select(&mut self) {
        self.state.set_active_document(self.doc);
        self.state.selection.select(self.doc, self.row);
    }

    fn steps(&self) -> usize {
        self.state
            .document(self.doc)
            .expect("open")
            .history
            .done()
            .len()
    }
}

/// The rig as a `.ydr` and as a `.yft` of target `T`.
fn synthetic<T: DrawableTarget>() -> [(&'static str, Vec<u8>); 2]
where
    GtaDrawable<T>: File,
    GtaFragType<T>: File,
{
    [
        (
            "rig.ydr",
            four_bones::<T>().to_bytes().expect("a rig saves"),
        ),
        (
            "rig.yft",
            four_bones_in_a_fragment::<T>()
                .to_bytes()
                .expect("a rig fragment saves"),
        ),
    ]
}

/// Every document the tests run over: a rig of each target, and the smallest staged drawable, dictionary
/// and fragment of each target that holds a bone with bones under it and others beside it.
fn rigs(what: &str) -> Vec<Rig> {
    let mut rigs = Vec::new();
    let at = BoneAddress { owner: 0, bone: 1 };
    for (target, game) in TARGETS {
        let made = match game {
            Game::GtaVLegacy => synthetic::<Gen8>(),
            _ => synthetic::<Gen9>(),
        };
        for (name, bytes) in &made {
            rigs.push(Rig::open(format!("{target} {name}"), name, bytes, game, at));
        }

        let held = rigs.len();
        for ext in ["ydr", "ydd", "yft"] {
            if let Some((path, at)) = staged(ext, target) {
                let bytes = std::fs::read(&path).expect("a staged file reads");
                let name = path.file_name().and_then(|n| n.to_str()).expect("a name");
                rigs.push(Rig::open(
                    format!("{target} {name}"),
                    name,
                    &bytes,
                    game,
                    at,
                ));
            }
        }
        if rigs.len() == held {
            skip_corpus(
                what,
                format_args!("no staged {target} file holds a bone with a bone under it"),
            );
        }
    }
    rigs
}

/// The smallest staged file of `ext` and `target` with a bone to edit, and that bone.
fn staged(ext: &str, target: &str) -> Option<(PathBuf, BoneAddress)> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
        .target(target)
        .find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files.into_iter().find_map(|path| {
        let payload = DocPayload::Resource(or_io::open(&path).ok()?);
        (0..drawable_count(&payload)).find_map(|owner| {
            let bone = editable_bone(skeleton_of(&payload, owner)?)?;
            Some((path.clone(), BoneAddress { owner, bone }))
        })
    })
}

/// Everything of a bone a save writes, in lanes.
fn numbers(skeleton: &CrSkeletonData, bone: usize) -> Vec<f32> {
    let flat = |m: &Mat34V| {
        [&m.col0, &m.col1, &m.col2, &m.col3]
            .iter()
            .flat_map(|c| [c.x, c.y, c.z, c.w])
            .collect::<Vec<_>>()
    };
    let b = &skeleton.bones[bone];
    let mut all = flat(&skeleton.default_transforms[bone]);
    all.extend(flat(&skeleton.inverse_transforms[bone]));
    for v in [&b.rotation, &b.translation, &b.scale] {
        all.extend([v.x, v.y, v.z, v.w]);
    }
    all
}

#[test]
fn a_bones_position_dragged_in_the_inspector_is_saved_with_its_rest_pose_derived_again() {
    const WHAT: &str =
        "a_bones_position_dragged_in_the_inspector_is_saved_with_its_rest_pose_derived_again";
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        // The file as it opened, which still holds the image the panel's structure is drawn from: the
        // fields are on the first screen of the panel and do not wait below that structure.
        rig.select();
        let shapes = Inspector::default().frame(&mut rig.state, Vec::new());
        let fields = texts(&shapes)
            .into_iter()
            .find(|(text, _)| text == "Position");
        let top = fields.map(|(_, rect)| rect.min.y);
        assert!(
            top.is_some_and(|y| y < 800.0),
            "{label}: the fields are at {top:?}"
        );

        rig.save();
        let before = rig.on_disk();
        let at = rig.at.bone;
        let under = subtree(&before, at);
        assert!(
            Skeleton::build(&before)
                .inverse_bind_deviation()
                .is_some_and(|d| d < 1e-4),
            "{label}: the file as it opened is the derivation's oracle"
        );

        let steps = rig.steps();
        Inspector::default().drag(&mut rig.state, "Position", 40.0);
        rig.state.seal_edits();
        assert_eq!(rig.steps(), steps + 1, "{label}: a drag is one step");
        let moved = rig.skeleton().bones[at].translation.clone();
        let was = &before.bones[at].translation;
        assert!(moved.x != was.x, "{label}: the drag moved x");
        assert_eq!((moved.y, moved.z), (was.y, was.z), "{label}");

        rig.save();
        let after = rig.on_disk();
        assert_eq!(
            after.bones[at].translation, moved,
            "{label}: the file holds the new position"
        );

        let oracle = Skeleton::build(&after);
        for (what, off) in [
            ("inverse", oracle.inverse_bind_deviation()),
            ("default", oracle.local_bind_deviation()),
        ] {
            let off = off.expect("the file holds both runs");
            assert!(
                off < 1e-4,
                "{label}: a {what} transform is {off} from what the bones derive"
            );
        }
        for &bone in &under {
            assert_ne!(
                after.inverse_transforms[bone], before.inverse_transforms[bone],
                "{label}: bone {bone} is under the bone moved, so its inverse moved"
            );
        }
        for bone in (0..before.bones.len()).filter(|b| !under.contains(b)) {
            assert_eq!(
                numbers(&after, bone),
                numbers(&before, bone),
                "{label}: bone {bone} is not under the bone moved"
            );
        }
        // Every bone keeps its name, id, place in the order and parents, and the table that finds a bone by its id.
        let places = |s: &CrSkeletonData| {
            let bones = s.bones.iter();
            let place = |b: &CrBoneData| (b.index, b.parent_index, b.next_index, b.mirror_index);
            let kept: Vec<_> = bones
                .map(|b| (b.name.clone(), b.bone_id, b.dofs, place(b)))
                .collect();
            (kept, s.parent_indices.clone(), s.bone_tag_slot_count)
        };
        assert_eq!(places(&after), places(&before), "{label}: ids and places");
        assert_eq!(
            (after.signature, after.signature_non_chiral),
            (before.signature, before.signature_non_chiral),
            "{label}: what animations check is the ids and the dofs"
        );
        assert_ne!(
            after.signature_comprehensive, before.signature_comprehensive,
            "{label}: the pose is in the comprehensive signature"
        );
    }
}

#[test]
fn renaming_a_bone_keeps_its_id_and_the_bone_tag_table() {
    const WHAT: &str = "renaming_a_bone_keeps_its_id_and_the_bone_tag_table";
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        rig.save();
        let before = rig.on_disk();
        let at = rig.at;

        rig.state
            .edit_bone(rig.doc, at, BoneEdit::Name("SKEL_Renamed".into()))
            .expect("a name is edited");
        assert_eq!(rig.skeleton().bones[at.bone].name, "SKEL_Renamed");
        rig.save();

        let mut after = rig.on_disk();
        assert_eq!(after.bones[at.bone].name, "SKEL_Renamed", "{label}");
        assert_eq!(
            after.bones[at.bone].bone_id, before.bones[at.bone].bone_id,
            "{label}"
        );
        // Nothing else of the skeleton differs, which a whole-value comparison reaches the hidden table with.
        after.bones[at.bone].name = before.bones[at.bone].name.clone();
        assert_eq!(after, before, "{label}: a name is all that changed");
    }
}

#[test]
fn a_rotation_in_degrees_is_a_unit_quaternion_turned_about_x_then_y_then_z() {
    const WHAT: &str = "a_rotation_in_degrees_is_a_unit_quaternion_turned_about_x_then_y_then_z";
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        let at = rig.at;
        // A quarter about X, then a quarter about Y: the axes land as X then Y puts them, and not as Y then X.
        rig.state
            .edit_bone(rig.doc, at, BoneEdit::Rotation([90.0, 90.0, 0.0]))
            .expect("a rotation is edited");
        let bone = &rig.skeleton().bones[at.bone];
        let q = [
            bone.rotation.x,
            bone.rotation.y,
            bone.rotation.z,
            bone.rotation.w,
        ];
        let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert!(near(&q, &[0.5, 0.5, -0.5, 0.5]), "{label}: {q:?}");
        let m = &rig.skeleton().default_transforms[at.bone];
        let axes = [&m.col0, &m.col1, &m.col2].map(|c| [c.x, c.y, c.z]);
        assert!(
            near(&axes[0], &[0.0, 0.0, -1.0]),
            "{label}: x lands at {:?}",
            axes[0]
        );
        assert!(
            near(&axes[1], &[1.0, 0.0, 0.0]),
            "{label}: y lands at {:?}",
            axes[1]
        );
        assert!(
            near(&axes[2], &[0.0, -1.0, 0.0]),
            "{label}: z lands at {:?}",
            axes[2]
        );

        // A drag across the first field turns about X alone: the other two read as they did, and what is
        // stored is a unit quaternion.
        rig.state
            .edit_bone(rig.doc, at, BoneEdit::Rotation([30.0, 20.0, 10.0]))
            .expect("a rotation is edited");
        rig.select();
        Inspector::default().drag(&mut rig.state, "Rotation (°, XYZ)", 40.0);
        rig.state.seal_edits();
        let bone = &rig.skeleton().bones[at.bone];
        let length = (bone.rotation.x.powi(2) + bone.rotation.y.powi(2))
            + (bone.rotation.z.powi(2) + bone.rotation.w.powi(2));
        assert!(
            (length - 1.0).abs() < 1e-5,
            "{label}: a unit quaternion, not {length}"
        );
        let shown = euler_degrees(&bone.rotation);
        assert!(
            shown[0] > 31.0,
            "{label}: the drag turned the first field on, to {shown:?}"
        );
        assert!(
            (shown[1] - 20.0).abs() < 0.01 && (shown[2] - 10.0).abs() < 0.01,
            "{label}: the others read {shown:?}"
        );
    }
}

#[test]
fn a_scaled_and_turned_bones_rest_pose_is_what_the_animation_skeleton_builds() {
    const WHAT: &str = "a_scaled_and_turned_bones_rest_pose_is_what_the_animation_skeleton_builds";
    let scale = [1.5, 0.8, 1.25];
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        let at = rig.at;
        // A turn about all three axes and a scale that differs along each: stretching the bone's own axes
        // and stretching its parent's land on different matrices only for a bone that is both.
        for edit in [
            BoneEdit::Rotation([35.0, 50.0, 70.0]),
            BoneEdit::Scale(scale),
        ] {
            rig.state
                .edit_bone(rig.doc, at, edit)
                .expect("a number is edited");
        }
        rig.save();
        let after = rig.on_disk();

        let oracle = Skeleton::build(&after);
        for (what, off) in [
            ("inverse", oracle.inverse_bind_deviation()),
            ("default", oracle.local_bind_deviation()),
        ] {
            let off = off.expect("the file holds both runs");
            assert!(
                off < 1e-4,
                "{label}: a {what} transform is {off} from what the bones derive"
            );
        }
        let m = &after.default_transforms[at.bone];
        for (k, column) in [&m.col0, &m.col1, &m.col2].into_iter().enumerate() {
            let length = (column.x.powi(2) + column.y.powi(2) + column.z.powi(2)).sqrt();
            assert!(
                (length - scale[k]).abs() < 1e-5,
                "{label}: column {k} of the default is {length} long, not {}",
                scale[k]
            );
        }
    }
}

#[test]
fn an_undone_edit_saves_the_bytes_the_file_had_before_it() {
    const WHAT: &str = "an_undone_edit_saves_the_bytes_the_file_had_before_it";
    for mut rig in rigs(WHAT) {
        let label = rig.label.clone();
        let before = rig.save();
        rig.select();
        for (field, edit) in [
            ("position", BoneEdit::Position([3.0, 4.0, 5.0])),
            ("rotation", BoneEdit::Rotation([10.0, 20.0, 30.0])),
            ("scale", BoneEdit::Scale([1.0, 2.0, 1.0])),
            ("name", BoneEdit::Name("SKEL_Renamed".into())),
        ] {
            let steps = rig.steps();
            rig.state.edit_bone(rig.doc, rig.at, edit).expect("an edit");
            assert_eq!(
                rig.steps(),
                steps + 1,
                "{label}: a {field} edit is one step"
            );
            assert!(
                rig.state.undo().is_some(),
                "{label}: the {field} edit undoes"
            );
            assert_eq!(rig.steps(), steps, "{label}");
            let bytes = rig.save();
            assert!(
                bytes == before,
                "{label}: the save after the undo of a {field} edit differs"
            );
        }
    }
}

#[test]
fn an_edit_a_skeleton_cannot_take_changes_nothing() {
    let mut rig = Rig::open(
        "rig".into(),
        "rig.ydr",
        &four_bones::<Gen8>().to_bytes().expect("a rig saves"),
        Game::GtaVLegacy,
        BoneAddress { owner: 0, bone: 1 },
    );
    let (doc, at) = (rig.doc, rig.at);
    let held = rig.skeleton().clone();
    let steps = rig.steps();
    let refused = [
        (at, BoneEdit::Position([f32::NAN, 0.0, 0.0])),
        (at, BoneEdit::Rotation([0.0, f32::INFINITY, 0.0])),
        (at, BoneEdit::Name("a\0b".into())),
        (at, BoneEdit::Scale([1.0, 0.0, 1.0])),
        (BoneAddress { owner: 0, bone: 9 }, BoneEdit::Scale([1.0; 3])),
        (BoneAddress { owner: 1, bone: 1 }, BoneEdit::Scale([1.0; 3])),
    ];
    for (at, edit) in refused {
        assert!(
            rig.state.edit_bone(doc, at, edit.clone()).is_err(),
            "{edit:?}"
        );
    }
    assert_eq!(rig.skeleton(), &held);
    assert_eq!(rig.steps(), steps);
}

/// The two-bone rig the preview poses, with its rest pose derived, a clip over its bones chosen.
fn previewing(game: Game) -> (tempfile::TempDir, AppState, DocumentId) {
    fn rig<T: DrawableTarget>() -> Vec<u8>
    where
        GtaDrawable<T>: File,
    {
        let mut drawable = rigged::<T>();
        let skeleton = drawable.base.skeleton.as_deref_mut().expect("a skeleton");
        let count = skeleton.bones.len();
        skeleton.default_transforms = vec![Mat34V::default(); count];
        skeleton.inverse_transforms = vec![Mat34V::default(); count];
        skeleton.rebuild_rest_pose(0).expect("the rig derives");
        drawable.to_bytes().expect("a rig saves")
    }
    let bytes = match game {
        Game::GtaVLegacy => rig::<Gen8>(),
        _ => rig::<Gen9>(),
    };
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("rig.ydr");
    std::fs::write(&path, bytes).expect("written");
    let mut state = AppState::new();
    state.documents.clear();
    state.prefs.active_game = Some(game);
    let drawable = open(&mut state, &path);
    let clips = DocPayload::Resource(Asset::Clips(a_clip_over_the_rig()));
    let clips = state.insert_document(dir.path().join("clips.ycd"), clips, None, None);
    state.active_workspace = Workspace::Animation;
    state.preview_scene.set_clip(clips, CLIP);
    state
        .preview_scene
        .pin_skeleton(Some(SkeletonSource::Drawable(drawable)));
    (dir, state, drawable)
}

#[test]
fn a_rotation_edited_in_a_bone_poses_the_previews_skeleton_again() {
    for (target, game) in TARGETS {
        let (_dir, mut state, drawable) = previewing(game);
        state.tick_animation();
        let epoch = state.skeleton_epoch();
        assert!(
            !state.anim.needs_rebuild(&state.preview_scene, epoch),
            "{target}: the session is built again with nothing changed"
        );
        let child = state.anim.world_matrices[1].w_axis;
        assert_eq!(
            (child.x, child.y),
            (0.0, 2.0),
            "{target}: the rig as authored"
        );

        // Both bones rest a metre along y of their parent. The root turned a quarter about Z carries its child
        // from a metre along y of it to a metre along -x of it, so to (-1, 1).
        let root = BoneAddress { owner: 0, bone: 0 };
        state
            .edit_bone(drawable, root, BoneEdit::Rotation([0.0, 0.0, 90.0]))
            .expect("a rotation is edited");
        let epoch = state.skeleton_epoch();
        assert!(
            state.anim.needs_rebuild(&state.preview_scene, epoch),
            "{target}: the edit asks for a new session"
        );
        state.tick_animation();
        let skeleton = state.anim.skeleton().expect("a skeleton is bound");
        let turned = skeleton.bones[0].bind_rotation;
        let quarter = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (turned.length() - 1.0).abs() < 1e-5,
            "{target}: a unit quaternion"
        );
        assert!(
            (turned.z - quarter).abs() < 1e-5 && (turned.w - quarter).abs() < 1e-5,
            "{target}: the preview poses the rotation as {turned:?}"
        );
        let child = state.anim.world_matrices[1].w_axis;
        assert!(
            (child.x + 1.0).abs() < 1e-5 && (child.y - 1.0).abs() < 1e-5,
            "{target}: the child is posed at ({}, {})",
            child.x,
            child.y
        );
    }
}

/// A fragment's common drawable's skeleton, and each other skeleton it saves: the other drawables' in their order, then each environment cloth's own.
fn fragment_skeletons(
    payload: &DocPayload,
) -> Option<(CrSkeletonData, Vec<Option<CrSkeletonData>>)> {
    fn of<T: DrawableTarget>(
        f: &GtaFragType<T>,
    ) -> Option<(CrSkeletonData, Vec<Option<CrSkeletonData>>)> {
        let own = |d: &FragDrawable<T>| d.base.skeleton.as_deref().cloned();
        let mut drawables = f.drawables().into_iter();
        let common = own(drawables.next()?)?;
        let cloth = (f.env_cloth.iter()).filter_map(|c| c.referenced_drawable.as_deref());
        Some((common, drawables.chain(cloth).map(own).collect()))
    }
    match payload {
        DocPayload::Resource(Asset::Fragment(f)) => of(f),
        DocPayload::Resource(Asset::FragmentGen9(f)) => of(f),
        _ => None,
    }
}

/// The four-bone fragment with the same skeleton again in an extra drawable and in the drawable a cloth names, and another extra drawable whose skeleton is its own.
fn fragment_with_copies<T: DrawableTarget>() -> GtaFragType<T> {
    let drawable = |own: bool| {
        let mut base = four_bones::<T>().base;
        if own {
            base.skeleton.as_deref_mut().expect("a skeleton").bones[0].name = "OWN_0".into();
        }
        Box::new(FragDrawable {
            base,
            load_skeleton: Bool8::new(true),
            ..FragDrawable::default()
        })
    };
    let mut fragment = four_bones_in_a_fragment::<T>();
    fragment.base.extra_drawables = vec![drawable(false), drawable(true)];
    fragment.base.extra_drawable_names = vec!["copy".into(), "own".into()];
    fragment.base.env_cloth = vec![Box::new(EnvironmentCloth {
        referenced_drawable: Some(drawable(false)),
        ..EnvironmentCloth::default()
    })];
    fragment
}

/// The smallest staged fragment of `target` whose common drawable's skeleton is held again by another drawable or a cloth, and a bone of it to edit.
fn staged_fragment_with_copies(target: &str) -> Option<(PathBuf, BoneAddress)> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft")
        .target(target)
        .find();
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    files.into_iter().find_map(|path| {
        let payload = DocPayload::Resource(or_io::open(&path).ok()?);
        let (common, others) = fragment_skeletons(&payload)?;
        others.contains(&Some(common.clone())).then_some(())?;
        let bone = editable_bone(&common)?;
        Some((path, BoneAddress { owner: 0, bone }))
    })
}

/// Edits a bone of the rig's fragment and holds each skeleton the common drawable's was equal to, in the file saved and read again, to follow it, and each other to stay as it was; an undo saves the bytes before.
fn the_copies_follow(mut rig: Rig) {
    let label = rig.label.clone();
    let before = rig.save();
    let payload = &rig.state.document(rig.doc).expect("open").payload;
    let (common, others) = fragment_skeletons(payload).expect("a fragment");
    let held = |skeletons: &[Option<CrSkeletonData>]| -> Vec<bool> {
        skeletons
            .iter()
            .map(|s| s.as_ref() == Some(&common))
            .collect()
    };
    let equal = held(&others);
    assert!(
        equal.contains(&true),
        "{label}: a skeleton equal to the common one"
    );

    rig.select();
    let moved = BoneEdit::Position([3.0, 4.0, 5.0]);
    rig.state
        .edit_bone(rig.doc, rig.at, moved.clone())
        .expect("an edit");
    assert_eq!(rig.steps(), 1, "{label}: the edit is one step");
    assert!(rig.state.undo().is_some(), "{label}: the edit undoes");
    assert!(
        rig.save() == before,
        "{label}: the save after the undo differs from the one before the edit"
    );

    rig.state
        .edit_bone(rig.doc, rig.at, moved)
        .expect("an edit");
    rig.save();
    let asset = or_io::open(&rig.path).expect("what was saved opens");
    let (after, others_after) =
        fragment_skeletons(&DocPayload::Resource(asset)).expect("a fragment");
    let at = rig.at.bone;
    assert_ne!(
        after.bones[at].translation, common.bones[at].translation,
        "{label}: the common skeleton moved"
    );
    for (n, (was, now)) in others.iter().zip(&others_after).enumerate() {
        if equal[n] {
            assert!(
                now.as_ref() == Some(&after),
                "{label}: skeleton {n} was the common one's and follows it"
            );
        } else {
            assert!(now == was, "{label}: skeleton {n} is its own and stays");
        }
    }
}

#[test]
fn an_edit_of_a_fragments_bone_reaches_each_skeleton_equal_to_the_one_edited_and_no_other() {
    for (target, game) in TARGETS {
        let bytes = match game {
            Game::GtaVLegacy => fragment_with_copies::<Gen8>().to_bytes(),
            _ => fragment_with_copies::<Gen9>().to_bytes(),
        }
        .expect("a fragment saves");
        let at = BoneAddress { owner: 0, bone: 1 };
        the_copies_follow(Rig::open(
            format!("{target} copies.yft"),
            "copies.yft",
            &bytes,
            game,
            at,
        ));
    }
}

#[test]
fn an_edit_of_a_staged_fragments_bone_reaches_the_skeletons_its_drawables_save_as_copies_of_it() {
    const WHAT: &str = "an_edit_of_a_staged_fragments_bone_reaches_the_skeletons_its_drawables_save_as_copies_of_it";
    for (target, game) in TARGETS {
        let Some((path, at)) = staged_fragment_with_copies(target) else {
            skip_corpus(
                WHAT,
                format_args!("no staged {target} fragment holds its common skeleton again"),
            );
            continue;
        };
        let bytes = std::fs::read(&path).expect("a staged file reads");
        the_copies_follow(Rig::open(
            format!("{target} {}", path.display()),
            "copies.yft",
            &bytes,
            game,
            at,
        ));
    }
}
