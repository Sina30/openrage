//! Uses and Placed by open a file a DLC's sub-pack holds, and a map's Uses names
//! each static bound with where it sits against the map's streaming box.
//!
//! ⚠ The installs are built here, as [`super::uses_and_used_by_in_the_app`]'s
//! is, so a clean checkout runs them with no schema snapshot and no game.

use std::path::Path;

use egui::PointerButton;
use or_io::decl::File as _;
use or_io::gta5::shared::Vec4V;
use or_io::gta5::{PhBoundAny, PhBoundSphere};
use or_io::meta::{MetaInstance, Value};
use or_title::{ContentXml, DlcList, SetupData};

use super::document_io::empty_file_bytes_for_kind;
use super::nested_archives_survive_a_save::archive;
use super::uses_and_used_by_in_the_app::{
    clis_set, cold, hashed, listed, opened_entries, placing, pso, types, Screen, PROPS,
};
use super::DocumentKind;

/// Where `testpack`'s `dlc1.rpf` sits in the install.
const SUB_PACK: &str = "x64/dlcpacks/testpack/dlc1.rpf";
pub(super) const MAPS: &str = "x64e.rpf/levels/gta5";

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
    std::fs::write(path, bytes).expect("a file");
}

/// A `common.rpf` naming `testpack`, which a mount needs to find the pack.
fn common() -> Vec<u8> {
    let list = DlcList {
        paths: vec!["dlcpacks:/testpack/".to_string().into()],
    };
    archive(&[("data/dlclist.xml", list.to_xml().as_bytes())])
}

/// An install whose base game declares `prop_bench`, textured by `props_txd`,
/// and holds neither that dictionary nor any map placing it: `testpack`'s
/// `dlc1.rpf` holds the dictionary and `dlc_props.ymap`, which places it.
fn install_with_a_dlc() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("an install folder");
    let ydr = empty_file_bytes_for_kind(DocumentKind::Ydr).expect("a blank drawable");
    let ytd = empty_file_bytes_for_kind(DocumentKind::Ytd).expect("a blank texture dictionary");
    let props = archive(&[("prop_bench.ydr", &ydr), ("v_props.ytyp", &types())]);
    let setup = SetupData {
        name_hash: "testpack".into(),
        device_name: "dlc_testpack".into(),
        dat_file: "content.xml".into(),
        sub_pack_count: 1,
        ..Default::default()
    };
    let dlc = archive(&[
        ("setup2.xml", setup.to_xml().as_bytes()),
        ("content.xml", ContentXml::default().to_xml().as_bytes()),
    ]);
    let sub = archive(&[
        ("props_txd.ytd", &ytd),
        ("dlc_props.ymap", &placing("dlc_props")),
    ]);
    write(&dir.path().join("common.rpf"), &common());
    write(
        &dir.path().join("x64e.rpf"),
        &archive(&[("levels/gta5/props.rpf", &props)]),
    );
    write(&dir.path().join("x64/dlcpacks/testpack/dlc.rpf"), &dlc);
    write(&dir.path().join("x64/dlcpacks/testpack/dlc1.rpf"), &sub);
    dir
}

/// `plain_map.ymap`, streaming from -100 to 100 on each axis.
pub(super) fn plain_map() -> Vec<u8> {
    let mut root = MetaInstance::new("CMapData");
    root.set("name", hashed("plain_map"));
    root.set("entities", Value::Array(Vec::new()));
    root.set("streamingExtentsMin", Value::Vec3([-100.0; 3]));
    root.set("streamingExtentsMax", Value::Vec3([100.0; 3]));
    pso(&root)
}

/// A `.ybn` whose header box spans `min` to `max`.
pub(super) fn bound_spanning(min: [f32; 3], max: [f32; 3]) -> Vec<u8> {
    let mut sphere = PhBoundSphere::default();
    sphere.base.bounding_box_max_xyz_margin_w = Vec4V {
        x: max[0],
        y: max[1],
        z: max[2],
        ..Default::default()
    };
    sphere.base.set_bounding_box_min(min);
    PhBoundAny::PhBoundSphere(sphere)
        .to_bytes()
        .expect("a bound")
}

/// An install holding `plain_map.ymap`, no imap group naming it, and the three
/// `.ybn` its stem names: one inside its streaming box, one outside it, and
/// one that is no bound at all.
pub(super) fn install_with_a_map() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("an install folder");
    let x64e = archive(&[
        ("levels/gta5/plain_map.ymap", &plain_map()),
        (
            "levels/gta5/plain_map_0.ybn",
            &bound_spanning([0.0; 3], [10.0; 3]),
        ),
        (
            "levels/gta5/ma@plain_map_1.ybn",
            &bound_spanning([500.0; 3], [510.0; 3]),
        ),
        ("levels/gta5/plain_map_2.ybn", b"not a bound"),
    ]);
    write(
        &dir.path().join("common.rpf"),
        &archive(&[("data/readme.txt", b"c")]),
    );
    write(&dir.path().join("x64e.rpf"), &x64e);
    dir
}

/// Open the first item of `submenu` on `file` whose label starts with `item`, and wait for what it opens.
fn open_from(root: &Path, file: &str, submenu: &str, item: &str, opens: &str) {
    let (mut state, tree) = cold(root);
    let screen = Screen::new();
    screen.submenu(&mut state, tree, root, file, submenu);
    let drawn = screen.until(
        &mut state,
        &format!("{submenu} never listed {item}"),
        |_, drawn| listed(drawn).iter().any(|d| d.label.starts_with(item)),
    );
    let at = (listed(&drawn).into_iter())
        .find(|d| d.label.starts_with(item))
        .map(|d| d.at)
        .expect("listed above");
    screen.click(&mut state, at, PointerButton::Primary);
    // ⚠ A toast ends the wait: a walk that finds nothing says so, and never opens.
    screen.until(&mut state, &format!("{opens} never opened"), |state, _| {
        opened_entries(state, tree).contains(&opens.to_string())
            || state.toasts.iter().next().is_some()
    });
    let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
    assert!(
        opened_entries(&state, tree).contains(&opens.to_string()),
        "{item} did not open {opens}; the app said {said:?}; it opened {:?}",
        opened_entries(&state, tree)
    );
}

#[test]
fn a_file_a_dlc_sub_pack_holds_opens_from_uses() {
    let root = install_with_a_dlc();
    let clis = clis_set(root.path(), "prop_bench.ydr");
    assert!(
        clis.contains(&"props_txd.ytd".to_string()),
        "the command's set has the dictionary the sub-pack holds: {clis:?}"
    );
    open_from(
        root.path(),
        &format!("{PROPS}/prop_bench.ydr"),
        "Uses",
        "props_txd.ytd",
        &format!("{SUB_PACK}/props_txd.ytd"),
    );
}

#[test]
fn a_map_a_dlc_sub_pack_holds_opens_from_placed_by() {
    let root = install_with_a_dlc();
    open_from(
        root.path(),
        &format!("{PROPS}/prop_bench.ydr"),
        "Placed by",
        "dlc_props.ymap",
        &format!("{SUB_PACK}/dlc_props.ymap"),
    );
}

#[test]
fn a_maps_uses_names_each_bound_with_where_it_sits_against_its_streaming_box() {
    let root = install_with_a_map();
    let (mut state, tree) = cold(root.path());
    let screen = Screen::new();

    let map = format!("{MAPS}/plain_map.ymap");
    screen.submenu(&mut state, tree, root.path(), &map, "Uses");
    let drawn = screen.until(&mut state, "Uses never listed the bounds", |_, drawn| {
        listed(drawn).len() == 3
    });
    let shown = listed(&drawn);
    let label_of = |bound: &str| -> String {
        (shown.iter())
            .find(|d| d.label.starts_with(&format!("{bound} ")))
            .map(|d| d.label.clone())
            .unwrap_or_else(|| {
                panic!(
                    "no {bound}: {:?}",
                    shown.iter().map(|d| &d.label).collect::<Vec<_>>()
                )
            })
    };
    for (bound, says) in [
        ("plain_map_0.ybn", "inside the map's streaming box"),
        ("ma@plain_map_1.ybn", "OUTSIDE the map's streaming box"),
        ("plain_map_2.ybn", "not cross-checked, a body did not read"),
    ] {
        let label = label_of(bound);
        assert!(
            label.contains("by filename convention") && label.contains(says),
            "{bound} is a convention bound {says}: {label:?}"
        );
    }
}
