//! A `.yld` built as an owned value with invented content: one cloth whose
//! controller's first level holds three vertices and two edges and whose
//! second level slot is empty, saved, read typed and through the tree to a
//! fixed point, and refused where the game would misread it. Needs no game
//! file.

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::decl::{Aligned, File, Raw, Resource, View};
use or_io::desc::Image;
use or_io::gta5::cloth_dict::*;
use or_io::gta5::formats;
use or_io::gta5::shared::{PgDictionary, Vec3V, Vec4V};
use or_io::Generation;

fn binding(bone: i32) -> BindingInfo {
    BindingInfo {
        weights: Vec4V {
            x: 1.0,
            ..Default::default()
        },
        blend_indices: [bone, 0, 0, 0],
        ..Default::default()
    }
}

fn authored() -> PgDictionary<CharacterCloth> {
    let edge = |a, b| PhEdgeData {
        vert_indices: [a, b],
        edge_length2: 1.0,
        ..Default::default()
    };
    let level = PhVerletCloth {
        cloth_data: PhClothData {
            vertex_positions: vec![Vec3V::default(); 3],
            ..Default::default()
        },
        edge_data: vec![edge(0, 1), edge(1, 2)],
        ..Default::default()
    };
    let mut controller = CharacterClothController {
        bone_index_map: vec![7, 8],
        bone_id_map: vec![0x1234, 0x5678],
        binding_info: vec![binding(1)],
        ..Default::default()
    };
    controller.base.name[..5].copy_from_slice(b"skirt");
    controller.base.cloth_0 = Some(Box::new(level));
    let mut cloth = CharacterCloth {
        cloth_controller: Some(Box::new(controller)),
        ..Default::default()
    };
    cloth.parent_matrix.col0.x = 1.0;
    PgDictionary {
        codes: vec![0x1234_5678],
        entries: vec![Some(Box::new(cloth))],
        ..Default::default()
    }
}

fn payload(file: &[u8]) -> Vec<u8> {
    let o = Rsc7::open(file).unwrap();
    let mut out = o.seal.header_bytes();
    out.extend_from_slice(&o.data);
    out
}

fn level(dict: &mut PgDictionary<CharacterCloth>) -> &mut PhVerletCloth {
    dict.entries[0]
        .as_mut()
        .unwrap()
        .cloth_controller
        .as_mut()
        .unwrap()
        .base
        .cloth_0
        .as_deref_mut()
        .unwrap()
}

#[test]
fn an_authored_yld_reads_typed() {
    let file = authored().to_bytes().unwrap();
    let peek = PgDictionary::<CharacterCloth>::peek(&file).unwrap();
    let dict = peek.root().unwrap();
    let cloth = dict.entries().unwrap().follow(0).unwrap().unwrap();
    assert_eq!(cloth.parent_matrix.col0.x, 1.0);
    let controller = cloth.cloth_controller().unwrap().unwrap();
    assert!(
        controller.base().cloth_1().unwrap().is_none(),
        "the empty slot stays null"
    );
    let level = controller.base().cloth_0().unwrap().unwrap();
    assert_eq!(level.edge_data().unwrap()[1].vert_indices, [1, 2]);
    let back = PgDictionary::<CharacterCloth>::from_bytes(&file).unwrap();
    let controller = back.entries[0]
        .as_ref()
        .unwrap()
        .cloth_controller
        .as_ref()
        .unwrap();
    assert_eq!(controller.base.name(), b"skirt");
    assert_eq!(back, authored());
}

#[test]
fn an_authored_yld_saves_whole_and_reaches_a_fixed_point() {
    let yld = formats::format("yld").unwrap();
    let first = authored().to_bytes().unwrap();
    let o = Rsc7::open(&first).unwrap();
    assert_eq!(yld.target_of_version(o.version).unwrap(), Generation::Gen8);
    let again = Aligned::from_bytes(&o.data);
    let img = Image::new(again.bytes(), o.virtual_size as usize);
    let second = yld
        .save(Generation::Gen8, &yld.read(Generation::Gen8, img).unwrap())
        .unwrap();
    assert_eq!(payload(&second), payload(&first));
}

/// `num_edges` and `num_vertices` are written from the runs they count.
#[test]
fn an_edited_level_saves_its_counts_from_its_runs() {
    let mut dict = authored();
    let l = level(&mut dict);
    l.edge_data.pop();
    l.cloth_data.vertex_positions.push(Vec3V::default());
    let file = dict.to_bytes().unwrap();
    let peek = PgDictionary::<CharacterCloth>::peek(&file).unwrap();
    let controller = peek
        .root()
        .unwrap()
        .entries()
        .unwrap()
        .follow(0)
        .unwrap()
        .unwrap()
        .cloth_controller()
        .unwrap()
        .unwrap();
    let saved = controller.base().cloth_0().unwrap().unwrap();
    let words = or_io::decl::Plain::as_bytes(View::get(&saved));
    let word = |at: usize| i32::from_le_bytes(words[at..at + 4].try_into().unwrap());
    assert_eq!(
        (
            word(std::mem::offset_of!(Raw<PhVerletCloth>, num_edges)),
            word(std::mem::offset_of!(Raw<PhVerletCloth>, num_vertices))
        ),
        (1, 4),
        "num_edges, then num_vertices"
    );
}

fn refused(dict: &PgDictionary<CharacterCloth>) -> String {
    dict.to_bytes().expect_err("refused").to_string()
}

#[test]
fn a_dictionary_with_fewer_hashes_than_cloths_is_refused() {
    let mut dict = authored();
    dict.codes.clear();
    assert!(refused(&dict).contains("holds 0 hashes and 1 cloths"));
}

#[test]
fn a_binding_past_the_bone_map_is_refused_and_a_tag_stores_its_index() {
    let mut dict = authored();
    let c = dict.entries[0]
        .as_mut()
        .unwrap()
        .cloth_controller
        .as_mut()
        .unwrap();
    let tags = c.vertex_weight_tags(0).unwrap();
    assert_eq!(tags.tags[0], Some(0x5678));
    assert!(tags.is_fully_tagged());
    let stored = c
        .binding_for_tags(&ClothVertexTags {
            tags: [Some(0x1234), None, None, None],
            weights: [1.0, 0.0, 0.0, 0.0],
        })
        .unwrap();
    assert_eq!(stored.blend_indices[0], 0);
    assert!(c
        .binding_for_tags(&ClothVertexTags {
            tags: [None, None, None, None],
            weights: [1.0, 0.0, 0.0, 0.0],
        })
        .is_none());
    c.binding_info[0] = binding(2);
    assert!(refused(&dict).contains("bone-map entry 2 of 2"));
}

#[test]
fn an_edge_past_the_vertices_is_refused() {
    let mut dict = authored();
    level(&mut dict).edge_data[1].vert_indices[1] = 3;
    assert!(refused(&dict).contains("joins vertices 1 and 3 of 3"));
}

#[test]
fn more_pinned_vertices_than_the_level_holds_are_refused() {
    let mut dict = authored();
    level(&mut dict).num_pinned_verts = 4;
    assert!(refused(&dict).contains("4 pinned vertex/vertices of 3"));
}

#[test]
fn a_bridge_whose_weights_and_inflation_disagree_is_refused() {
    let mut dict = authored();
    let c = dict.entries[0]
        .as_mut()
        .unwrap()
        .cloth_controller
        .as_mut()
        .unwrap();
    let mut bridge = ClothBridgeSimGfx::default();
    *bridge.vertex_weight_mut(3).unwrap() = vec![1.0; 3];
    *bridge.inflation_scale_mut(3).unwrap() = vec![1.0; 2];
    c.base.bridge = Some(Box::new(bridge));
    assert!(refused(&dict).contains("LOD 3 states 3 simulation weight(s) and 2 inflation"));
}

#[test]
fn two_bone_maps_of_different_lengths_are_refused() {
    let mut dict = authored();
    let c = dict.entries[0]
        .as_mut()
        .unwrap()
        .cloth_controller
        .as_mut()
        .unwrap();
    c.bone_id_map.push(0x9abc);
    assert!(refused(&dict).contains("maps 2 bone index/indices and 3 bone id(s)"));
}

/// ⚠ The fourth level is reachable, and CodeWalker's reader never gets to it.
#[test]
fn the_fourth_lod_is_reachable() {
    let mut bridge = ClothBridgeSimGfx::default();
    for lod in 0..LOD_COUNT {
        assert!(
            bridge.vertex_weight_mut(lod).is_some()
                && bridge.pin_radius_mut(lod).is_some()
                && bridge.inflation_scale_mut(lod).is_some(),
            "level {lod} of {LOD_COUNT} is not reachable"
        );
    }
    assert!(bridge.vertex_weight_mut(LOD_COUNT).is_none());
    assert_eq!(ClothController::default().cloth().len(), 4);
}

#[test]
fn a_yld_opens_as_its_owned_root() {
    let file = authored().to_bytes().unwrap();
    assert_eq!(<PgDictionary<CharacterCloth> as Resource>::EXT, "yld");
    let or_io::Asset::ClothDict(dict) = or_io::from_bytes(&file).unwrap() else {
        panic!("not a cloth dictionary");
    };
    assert_eq!(dict, authored());
}
