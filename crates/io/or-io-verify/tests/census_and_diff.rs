//! The alignment census and the structural diff, on the synthetic declarations.

#[path = "../../or-io/tests/synthetic/mod.rs"]
mod synthetic;

use std::sync::Arc;

use or_io::decl::{self, Aligned, Doc, Gen8, Gen9, Raw, View, ViewError};
use or_io::desc::{self, FieldRepr, Pack, Pointee};
use or_io_verify::{census, diff};
use synthetic::*;

fn legacy() -> Pack {
    decl::describe("synthetic", "gen8", &[decl::root::<Raw<Catalogue>>]).expect("describes")
}

#[test]
fn every_instance_of_the_catalogue_sits_at_its_alignment_before_and_after_a_save() {
    let pack = legacy();
    let image = catalogue();
    let bytes = Aligned::from_bytes(&image.bytes);
    let img = desc::Image::new(bytes.bytes(), image.virtual_size);
    let found = census::take(&pack, img, "Catalogue", 0);
    assert!(found.unreached.is_empty(), "{:?}", found.unreached);
    let count = |n: &str| found.types.get(n).map_or(0, |t| t.instances);
    assert_eq!(count("Catalogue"), 1);
    assert_eq!(count("Header"), 1);
    // three in the run, one primary, one held, and the channel's two members, once each
    assert_eq!(count("Point"), 7, "{:#?}", found.types);
    assert_eq!(
        count("Channel"),
        1,
        "two pointers at one channel are one instance"
    );
    assert_eq!(count("Holder<Point>"), 1);
    assert_eq!(count("Swatch"), 1);
    assert_eq!(found.misaligned().count(), 0, "{:#?}", found.types);
    assert_eq!(found.types["Holder<Point>"].alignment, 8);
    assert_eq!(found.types["Point"].alignment, 4);

    let arc = Arc::new(pack.clone());
    let saved = Doc::<Raw<Catalogue>>::read(arc, img, 0)
        .unwrap()
        .save()
        .unwrap();
    let after = census::take(&pack, saved.image(), "Catalogue", saved.root);
    assert_eq!(
        after.misaligned().count(),
        0,
        "the canonical writer places every instance aligned"
    );
    let total = |c: &census::Census| c.types.values().map(|t| t.instances).sum::<usize>();
    assert_eq!(
        total(&found),
        total(&after),
        "the save reaches the same instances"
    );
}

#[test]
fn a_census_finds_the_record_at_four_modulo_eight_and_the_view_refuses_it_unless_packed() {
    let pack = decl::describe(
        "synthetic",
        "gen8",
        &[decl::root::<Raw<Loose>>, decl::root::<Raw<Holder<Point>>>],
    )
    .unwrap();
    let mut w = Writer(vec![0; 0x40]);
    w.u32(0x04, 0xAB).ptr(0x08, 0x20);
    w.point(0x20, [1.0, 2.0, 3.0], 4);
    let bytes = Aligned::from_bytes(&w.0);
    let img = desc::Image::new(bytes.bytes(), w.0.len());

    let found = census::take(&pack, img, "Loose", 0x04);
    let loose = &found.types["Loose"];
    assert_eq!(
        (loose.instances, loose.misaligned, loose.alignment),
        (1, 1, 8)
    );
    assert_eq!(loose.examples, [0x04]);
    assert_eq!(found.types["Point"].misaligned, 0);

    let view = View::<Raw<Loose>>::at_offset(img, 0x04)
        .expect("a packed(4) record is viewable at four modulo eight");
    assert_eq!({ view.tag }, 0xAB);
    assert_eq!(view.link().unwrap().unwrap().weight, 4);
    assert!(matches!(
        View::<Raw<Holder<Point>>>::at_offset(img, 0x04),
        Err(ViewError::Misaligned {
            align: 8,
            offset: 0x04,
            ..
        })
    ));
}

#[test]
fn the_description_agrees_with_itself_and_renamed() {
    let pack = legacy();
    assert!(diff::structural(&pack, "Catalogue", &pack, "Catalogue").is_empty());

    // Every type and field renamed: the walk matches by position, so nothing differs.
    let mut renamed = pack.clone();
    let rename = |n: &str| format!("x_{}", n.replace(['<', '>', ',', ' '], "_"));
    for ty in &mut renamed.types {
        ty.name = rename(&ty.name);
        for f in &mut ty.fields {
            f.name = rename(&f.name);
            match &mut f.repr {
                FieldRepr::Inline { type_name } | FieldRepr::InlineArray { type_name, .. } => {
                    *type_name = rename(type_name)
                }
                FieldRepr::Ptr(p)
                | FieldRepr::AtArray(p)
                | FieldRepr::PtrSlots { pointee: p, .. } => match p {
                    Pointee::One { type_name }
                    | Pointee::Member { type_name }
                    | Pointee::Members { type_name, .. } => *type_name = rename(type_name),
                    Pointee::Many {
                        type_name: Some(t), ..
                    }
                    | Pointee::ManyPtr {
                        type_name: Some(t), ..
                    } => *t = rename(t),
                    _ => {}
                },
                _ => {}
            }
        }
    }
    let differences = diff::structural(&pack, "Catalogue", &renamed, "x_Catalogue");
    assert!(differences.is_empty(), "{differences:#?}");
}

#[test]
fn a_moved_offset_a_resized_type_and_a_retargeted_pointer_are_each_named() {
    let pack = legacy();
    let mut other = pack.clone();
    let ty = |p: &mut Pack, n: &str| p.types.iter_mut().position(|t| t.name == n).unwrap();
    let c = ty(&mut other, "Catalogue");
    other.types[c]
        .fields
        .iter_mut()
        .find(|f| f.name == "flags")
        .unwrap()
        .offset = 0x31;
    let p = ty(&mut other, "Point");
    other.types[p].size = 0x14;
    let h = ty(&mut other, "Holder<Point>");
    other.types[h].fields[0].repr = FieldRepr::Ptr(Pointee::Bytes {
        length: or_io::desc::CountSource::Field("count".into()),
    });

    let differences = diff::structural(&pack, "Catalogue", &other, "Catalogue");
    let said: Vec<String> = differences.iter().map(ToString::to_string).collect();
    assert!(
        said.iter().any(|d| d.starts_with("Catalogue.flags")),
        "{said:#?}"
    );
    assert!(
        said.contains(&"Catalogue.points->: size 0x10 against 0x14".to_string()),
        "{said:#?}"
    );
    assert!(
        said.iter()
            .any(|d| d.contains("items") && d.contains("points at pointer run against bytes")),
        "{said:#?}"
    );
}

#[test]
fn the_two_targets_differ_where_they_were_declared_to_and_nowhere_else() {
    let g8 = decl::describe("synthetic", "gen8", &[decl::root::<Raw<Frame<Gen8>>>]).unwrap();
    let g9 = decl::describe("synthetic", "gen9", &[decl::root::<Raw<Frame<Gen9>>>]).unwrap();
    let differences = diff::structural(&g8, "Frame", &g9, "Frame");
    assert!(!differences.is_empty());
    assert!(
        differences
            .iter()
            .all(|d| d.path.starts_with("Frame.swatch->")),
        "the spine agrees and only what it points at differs: {differences:#?}"
    );
}
