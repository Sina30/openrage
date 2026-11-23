//! The engine's shared primitives, embedded in an invented record: each type's
//! description is what `offset_of!` says, each round-trips owned, and each
//! lays out its leaves as the private description's type of the same name.

use std::mem::{offset_of, size_of};
use std::sync::Arc;

use or_io::decl::{self, Aligned, Cx, Doc, Image, Owned, Raw};
use or_io::desc::{FieldRepr, Pack};
use or_io::gta5::shared::{
    AtBitSet, DatBase, Mat33V, Mat34V, PgBaseRefCounted, QuatV, SpdAABB, SpdSphere, Vec4V,
};

mod specimen {
    use or_io::decl::*;
    use or_io::gta5::shared::*;

    /// Every shared primitive once, by value; nothing here is a game's.
    #[record(size = 0xe0)]
    pub struct Specimen {
        pub counted: PgBaseRefCounted,
        pub data: DatBase,
        pub rotation: QuatV,
        pub basis: Mat33V,
        pub transform: Mat34V,
        pub sphere: SpdSphere,
        pub aabb: SpdAABB,
        pub pins: AtBitSet,
    }
}
use specimen::Specimen;

const SHARED: [&str; 8] = [
    "PgBaseRefCounted",
    "DatBase",
    "QuatV",
    "Mat33V",
    "Mat34V",
    "SpdSphere",
    "SpdAABB",
    "AtBitSet",
];

fn description() -> Arc<Pack> {
    Arc::new(
        decl::describe("synthetic", "gen8", &[decl::root::<Raw<Specimen>>]).expect("describes"),
    )
}

fn v(x: f32, y: f32, z: f32, w: f32) -> Vec4V {
    Vec4V {
        x,
        y,
        z,
        w,
        ..Default::default()
    }
}

fn authored() -> Specimen {
    let pins = AtBitSet {
        bits: vec![0b1011, 0x8000_0000],
        bit_size: 40,
        ..Default::default()
    };
    Specimen {
        rotation: QuatV {
            x: 0.0,
            y: 0.0,
            z: 0.6,
            w: 0.8,
            ..Default::default()
        },
        basis: Mat33V {
            col0: v(1.0, 0.0, 0.0, 0.0),
            col1: v(0.0, 1.0, 0.0, 0.0),
            col2: v(0.0, 0.0, 1.0, 0.0),
            ..Default::default()
        },
        transform: Mat34V {
            col0: v(0.0, 1.0, 0.0, 0.0),
            col1: v(-1.0, 0.0, 0.0, 0.0),
            col2: v(0.0, 0.0, 1.0, 0.0),
            col3: v(10.0, 20.0, 30.0, 1.0),
            ..Default::default()
        },
        sphere: SpdSphere {
            center_and_radius: v(1.0, 2.0, 3.0, 4.5),
            ..Default::default()
        },
        aabb: SpdAABB {
            min: v(-1.0, -2.0, -3.0, f32::from_bits(7)),
            max: v(1.0, 2.0, 3.0, f32::from_bits(9)),
            ..Default::default()
        },
        pins,
        ..Default::default()
    }
}

/// The canonical save of `value`, as an aligned image and the root's offset in it.
fn save(pack: &Arc<Pack>, value: &Specimen) -> (Aligned, usize, usize) {
    let tree = value.to_element(&Cx::new(pack.plans().clone())).unwrap();
    let built = or_io::write::build(pack, &tree).unwrap();
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    let root = (built.root - or_io::constants::FIXED_VIRTUAL_BASE) as usize;
    (Aligned::from_bytes(&bytes), virtual_size, root)
}

fn open(pack: &Arc<Pack>, image: Image<'_>, root: usize) -> Specimen {
    let doc = Doc::<Raw<Specimen>>::read(pack.clone(), image, root).unwrap();
    Specimen::from_element(doc.value()).unwrap()
}

#[test]
fn each_description_is_what_offset_of_says() {
    let pack = description();
    let ty = |n: &str| pack.type_by_name(n).unwrap_or_else(|| panic!("{n}"));
    let at = |t: &str, f: &str| {
        ty(t)
            .field_by_name(f)
            .unwrap_or_else(|| panic!("{t}.{f}"))
            .offset
    };
    for (name, size) in [
        ("PgBaseRefCounted", size_of::<Raw<PgBaseRefCounted>>()),
        ("DatBase", size_of::<Raw<DatBase>>()),
        ("QuatV", size_of::<Raw<QuatV>>()),
        ("Mat33V", size_of::<Raw<Mat33V>>()),
        ("Mat34V", size_of::<Raw<Mat34V>>()),
        ("SpdSphere", size_of::<Raw<SpdSphere>>()),
        ("SpdAABB", size_of::<Raw<SpdAABB>>()),
        ("AtBitSet", size_of::<Raw<AtBitSet>>()),
        ("Specimen", size_of::<Raw<Specimen>>()),
    ] {
        assert_eq!(ty(name).size, size, "{name}");
    }
    for (f, o) in [
        ("x", offset_of!(Raw<QuatV>, x)),
        ("y", offset_of!(Raw<QuatV>, y)),
        ("z", offset_of!(Raw<QuatV>, z)),
        ("w", offset_of!(Raw<QuatV>, w)),
    ] {
        assert_eq!(at("QuatV", f), o, "QuatV.{f}");
    }
    for (f, o) in [
        ("col0", offset_of!(Raw<Mat33V>, col0)),
        ("col1", offset_of!(Raw<Mat33V>, col1)),
        ("col2", offset_of!(Raw<Mat33V>, col2)),
    ] {
        assert_eq!(at("Mat33V", f), o, "Mat33V.{f}");
    }
    for (f, o) in [
        ("col0", offset_of!(Raw<Mat34V>, col0)),
        ("col1", offset_of!(Raw<Mat34V>, col1)),
        ("col2", offset_of!(Raw<Mat34V>, col2)),
        ("col3", offset_of!(Raw<Mat34V>, col3)),
    ] {
        assert_eq!(at("Mat34V", f), o, "Mat34V.{f}");
    }
    assert_eq!(
        at("SpdSphere", "center_and_radius"),
        offset_of!(Raw<SpdSphere>, center_and_radius)
    );
    assert_eq!(at("SpdAABB", "min"), offset_of!(Raw<SpdAABB>, min));
    assert_eq!(at("SpdAABB", "max"), offset_of!(Raw<SpdAABB>, max));
    assert_eq!(at("AtBitSet", "bits"), offset_of!(Raw<AtBitSet>, bits));
    assert_eq!(
        at("AtBitSet", "bit_size"),
        offset_of!(Raw<AtBitSet>, bit_size)
    );
    for name in SHARED {
        assert!(
            ty("Specimen")
                .fields
                .iter()
                .any(|f| matches!(&f.repr, FieldRepr::Inline { type_name } if type_name == name)),
            "{name} is embedded by value"
        );
    }
}

#[test]
fn a_record_embedding_every_primitive_round_trips_owned_to_a_fixed_point() {
    let pack = description();
    let value = authored();
    let (first, size, root) = save(&pack, &value);
    let reopened = open(&pack, Image::new(first.bytes(), size), root);
    assert_eq!(reopened.counted, value.counted);
    assert_eq!(reopened.data, value.data);
    assert_eq!(reopened.rotation, value.rotation);
    assert_eq!(reopened.basis, value.basis);
    assert_eq!(reopened.transform, value.transform);
    assert_eq!(reopened.sphere, value.sphere);
    assert_eq!(reopened.aabb, value.aabb);
    assert_eq!(
        reopened.aabb.min.w.to_bits(),
        7,
        "a packed fourth lane is carried"
    );
    assert_eq!(reopened.pins.bits, value.pins.bits);
    assert_eq!(reopened.pins.bit_size, 40);
    let (second, size, root) = save(&pack, &reopened);
    assert_eq!(second.bytes(), first.bytes());
    assert_eq!(
        open(&pack, Image::new(second.bytes(), size), root),
        reopened
    );
}

#[test]
fn a_word_no_field_shows_is_carried_and_a_bit_set_counts_its_own_words() {
    let pack = description();
    let (saved, size, root) = save(&pack, &authored());
    let at = |t: &str, f: &str| {
        pack.type_by_name(t)
            .unwrap()
            .field_by_name(f)
            .unwrap()
            .offset
    };
    let counted = root + offset_of!(Raw<Specimen>, counted);
    let vft = root + offset_of!(Raw<Specimen>, data) + at("DatBase", "vft");
    let ref_count = counted + at("PgBaseRefCounted", "ref_count");
    let words = root + offset_of!(Raw<Specimen>, pins) + at("AtBitSet", "size");
    assert_eq!(
        u16::from_le_bytes(saved.bytes()[words..words + 2].try_into().unwrap()),
        2,
        "the word count is written from the words"
    );

    let mut bytes = saved.bytes().to_vec();
    bytes[vft..vft + 8].copy_from_slice(&0x1234_5678_9abc_u64.to_le_bytes());
    bytes[ref_count..ref_count + 4].copy_from_slice(&3u32.to_le_bytes());
    let poked = Aligned::from_bytes(&bytes);
    let value = open(&pack, Image::new(poked.bytes(), size), root);
    let (again, ..) = save(&pack, &value);
    assert_eq!(again.bytes()[vft..vft + 8], bytes[vft..vft + 8]);
    assert_eq!(
        again.bytes()[ref_count..ref_count + 4],
        bytes[ref_count..ref_count + 4]
    );
}
