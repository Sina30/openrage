//! A `.yft` built as an owned value with invented content, for each target:
//! a common drawable with a two-bone skeleton, an extra drawable and its
//! name, one physics level of one named group and one child, and a vehicle
//! glass block of one pane. Saved, reopened equal, edited and saved to a fixed
//! point; the tables a save must keep read back from the saved file; the typed
//! open names the target a file holds. Needs no game file.

use std::path::{Path, PathBuf};

use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::fragment::vehicle_glass::{VEHICLE_GLASS_WINDOW_MAGIC, WINDOW_HEADER_LEN};
use or_io::gta5::*;
use or_io::{Asset, Error};

fn dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gta5_fragment");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn drawable<T: DrawableTarget>(name: &str) -> Box<FragDrawable<T>> {
    let bone = |name: &str, id, parent| CrBoneData {
        name: name.into(),
        bone_id: id,
        parent_index: parent,
        next_index: -1,
        ..Default::default()
    };
    Box::new(FragDrawable {
        base: RmcDrawable {
            skeleton: Some(Box::new(CrSkeletonData {
                bones: vec![bone("chassis", 0, -1), bone("door_dside_f", 0x4321, 0)],
                inverse_transforms: vec![Mat34V::default(); 2],
                default_transforms: vec![Mat34V::default(); 2],
                parent_indices: vec![-1, 0],
                bone_tag_slot_count: 3,
                ..Default::default()
            })),
            ..Default::default()
        },
        skeleton_type_name: name.into(),
        load_skeleton: or_io::decl::Bool8::new(true),
        ..Default::default()
    })
}

/// One pane with no shattermap: its header opens `VGWC` and states nothing else.
fn glass() -> FwVehicleGlassWindowData {
    let mut header = [0u8; WINDOW_HEADER_LEN];
    header[0x40..0x44].copy_from_slice(&VEHICLE_GLASS_WINDOW_MAGIC);
    FwVehicleGlassWindowData {
        windows: vec![VehicleGlassWindow {
            header,
            shatter_map: ShatterMap::default(),
        }],
        header_stride: WINDOW_HEADER_LEN as u16,
        after_offsets: 0,
    }
}

fn fragment<T: DrawableTarget>() -> GtaFragType<T> {
    let mut name = [0u8; 40];
    name[..4].copy_from_slice(b"door");
    let lod = FragPhysicsLOD {
        group_names: vec!["door".into()],
        groups: vec![Box::new(FragTypeGroup {
            debug_name: name,
            num_children: 1,
            parent_group_pointer_index: 0xFF,
            ..Default::default()
        })],
        children: vec![Box::new(FragTypeChild {
            bone_id: 0x4321,
            undamaged_mass: 20.0,
            undamaged_entity: Some(drawable("door")),
            ..Default::default()
        })],
        min_breaking_impulses: vec![1.0],
        undamaged_ang_inertia: vec![Vec3V::default()],
        damaged_ang_inertia: vec![Vec3V::default()],
        root_group_count: 1,
        num_bony_children: 1,
        ..Default::default()
    };
    GtaFragType {
        base: FragType {
            common_drawable: Some(drawable("police")),
            extra_drawables: vec![drawable("police_dam")],
            extra_drawable_names: vec!["police_dam".into()],
            damaged_drawable: 0,
            tune_name: "police".into(),
            physics_lod_group: Some(Box::new(FragPhysicsLODGroup {
                high_lod: Some(Box::new(lod)),
                ..Default::default()
            })),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// `value` saved to `path`, reopened as the same type and equal to it, and saved again to the same bytes.
fn round_trip<D>(value: &D, path: &Path) -> D
where
    D: File + PartialEq + std::fmt::Debug,
{
    value.save_as(path).unwrap();
    let reopened = D::open(path).unwrap();
    assert_eq!(&reopened, value);
    let first = std::fs::read(path).unwrap();
    reopened.save().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), first, "a fixed point");
    reopened
}

#[test]
fn a_legacy_fragment_built_owned_keeps_its_names_and_its_glass() {
    let path = dir().join("police.yft");
    let mut built = fragment::<Gen8>();
    built.vehicle_window_data = Some(Box::new(glass()));
    round_trip(&built, &path);

    let mut frag = GtaFragType::open(&path).unwrap();
    assert_eq!(frag.tune_name, "police");
    assert_eq!(frag.extra_drawable_names, ["police_dam"]);
    let lod = frag
        .physics_lod_group
        .as_ref()
        .unwrap()
        .lods()
        .next()
        .unwrap();
    assert_eq!(lod.group_names, ["door"]);
    assert_eq!(lod.groups[0].name(), b"door");
    assert_eq!(frag.vehicle_window_data.as_deref(), Some(&glass()));

    frag.tune_name = "police2".into();
    frag.extra_drawable_names[0] = "police2_dam".into();
    frag.vehicle_window_data.as_mut().unwrap().after_offsets = 7;
    frag.save().unwrap();

    let again = GtaFragType::open(&path).unwrap();
    assert_eq!(again.tune_name, "police2");
    assert_eq!(again.extra_drawable_names, ["police2_dam"]);
    assert_eq!(
        again.vehicle_window_data.as_deref().unwrap().after_offsets,
        7
    );
    round_trip(&again, &path);
}

#[test]
fn a_run_and_the_names_counted_with_it_must_agree_at_save() {
    let mut built = fragment::<Gen8>();
    built.extra_drawable_names.push("orphan".into());
    let why = built.to_bytes().unwrap_err().to_string();
    assert!(why.contains("num_extra_drawables"), "{why}");
}

#[test]
fn an_enhanced_fragment_opens_as_its_target() {
    let path = dir().join("police_enhanced.yft");
    round_trip(&fragment::<Gen9>(), &path);

    let why = GtaFragType::open(&path).unwrap_err();
    assert!(
        matches!(
            why,
            Error::WrongTarget {
                found: "Enhanced",
                ..
            }
        ),
        "{why:?}"
    );
    match or_io::open(&path).unwrap() {
        Asset::FragmentGen9(f) => assert_eq!(f.tune_name, "police"),
        other => panic!("{:?} opened as another asset", other.origin().path()),
    }
}

#[test]
fn a_legacy_fragment_opens_through_or_io_open() {
    let path = dir().join("asset.yft");
    fragment::<Gen8>().save_as(&path).unwrap();
    match or_io::open(&path).unwrap() {
        Asset::Fragment(f) => assert_eq!(f.tune_name, "police"),
        other => panic!("{:?} opened as another asset", other.origin().path()),
    }
}
