//! A tagged base generic over its target, on invented types: one variant
//! points by key at a record each target lays out its own way, as a particle
//! shader variable points at a texture of its list's dictionary, and two tags
//! select one layout, as its vector tags do. For each target the root opens
//! owned, saves byte-identical to the tree's save of the same bytes and
//! reaches a fixed point.

use std::fmt::Debug;
use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::*;
use or_io::desc::{self, Pack};

pub mod gen8 {
    use or_io::decl::*;

    #[record(size = 0x08, target = "gen8", key = "hash")]
    pub struct Swatch {
        pub hash: Hash32,
        pub width: u16,
        pub height: u16,
    }
}

pub mod gen9 {
    use or_io::decl::*;

    #[record(size = 0x10, target = "gen9", key = "hash")]
    pub struct Swatch {
        pub hash: Hash32,
        pub depth: u32,
        pub width: u16,
        #[pad_after(4)]
        pub height: u16,
    }
}

/// What each target lays out its own way; a brush points at it by key.
pub trait Stock: TargetMarker {
    type Swatch: Owned<Raw: Record> + Default + Debug + Clone + PartialEq;
}

impl Stock for Gen8 {
    type Swatch = gen8::Swatch;
}

impl Stock for Gen9 {
    type Swatch = gen9::Swatch;
}

/// A tagged base none of whose own fields names its target.
#[record(
    size = 0x08,
    tag = "var_type",
    variant(1 => BrushScalar<T>),
    variant(6 => BrushSwatch<T>)
)]
pub struct Brush<T: Stock> {
    pub name_hash: Hash32,
    pub var_type: u32,
}

#[record(size = 0x10)]
pub struct BrushScalar<T: Stock> {
    pub base: Brush<T>,
    #[pad_after(4)]
    pub value: f32,
}

/// The variant whose pointer lands on a swatch of the easel's own list.
#[record(size = 0x10)]
pub struct BrushSwatch<T: Stock> {
    pub base: Brush<T>,
    pub swatch: Keyed<T::Swatch>,
}

/// A root generic over its target, Legacy by default.
#[record(size = 0x20)]
pub struct Easel<T: Stock = Gen8> {
    pub swatches: Vec<Option<Box<T::Swatch>>>,
    pub brushes: Vec<Option<Box<BrushAny<T>>>>,
}

fn pack(target: &str) -> Arc<Pack> {
    let roots: &[fn(&mut Reach)] = match target {
        "gen8" => &[root::<Raw<Easel<Gen8>>>],
        _ => &[root::<Raw<Easel<Gen9>>>],
    };
    Arc::new(describe("synthetic", target, roots).expect("describes"))
}

/// `value` laid out by the canonical writer: the image, its virtual size and where its root is.
fn build<T: Owned>(target: &str, value: &T) -> (Vec<u8>, usize, usize) {
    let pack = pack(target);
    let tree = value.to_element(&Cx::new(pack.plans().clone())).unwrap();
    image_of(or_io::write::build(&pack, &tree).unwrap())
}

fn image_of(built: or_io::write::Built) -> (Vec<u8>, usize, usize) {
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    (
        bytes,
        virtual_size,
        (built.root - FIXED_VIRTUAL_BASE) as usize,
    )
}

/// The owned value `bytes` holds, and the tree's own save of the same bytes.
fn open<T: Owned>(target: &str, bytes: &[u8], virtual_size: usize, root: usize) -> (T, Vec<u8>)
where
    Raw<T>: Record,
{
    let pack = pack(target);
    let aligned = Aligned::from_bytes(bytes);
    let image = desc::Image::new(aligned.bytes(), virtual_size);
    let doc = Doc::<Raw<T>>::read(pack.clone(), image, root).unwrap();
    let owned = T::from_element(doc.value()).unwrap();
    let tree = image_of(or_io::write::build(&pack, doc.value()).unwrap()).0;
    (owned, tree)
}

fn easel<T: Stock>(swatch: impl Fn(&str) -> T::Swatch) -> Easel<T> {
    let brush = |name: &str, var_type| Brush {
        name_hash: Hash32::of(name),
        var_type,
        ..Default::default()
    };
    Easel {
        swatches: vec![
            Some(Box::new(swatch("canvas"))),
            Some(Box::new(swatch("linen"))),
        ],
        brushes: vec![
            Some(Box::new(BrushAny::BrushScalar(BrushScalar {
                base: brush("opacity", 1),
                value: 0.75,
                ..Default::default()
            }))),
            Some(Box::new(BrushAny::BrushSwatch(BrushSwatch {
                base: brush("diffuse", 6),
                swatch: Keyed::to(Hash32::of("linen").get()),
                ..Default::default()
            }))),
            Some(Box::new(BrushAny::Base(brush("unnamed", 3)))),
            None,
        ],
        ..Default::default()
    }
}

fn legacy() -> Easel<Gen8> {
    easel::<Gen8>(|name| gen8::Swatch {
        hash: Hash32::of(name),
        width: 4,
        height: 2,
        ..Default::default()
    })
}

fn enhanced() -> Easel<Gen9> {
    easel::<Gen9>(|name| gen9::Swatch {
        hash: Hash32::of(name),
        depth: 1,
        width: 8,
        height: 2,
        ..Default::default()
    })
}

fn opens_saves_as_the_tree_and_reaches_a_fixed_point<T: Stock>(target: &str, value: Easel<T>)
where
    Easel<T>: Owned + Debug + PartialEq,
    Raw<Easel<T>>: Record,
{
    let (first, virtual_size, root) = build(target, &value);
    let (again, tree): (Easel<T>, _) = open(target, &first, virtual_size, root);
    assert_eq!(again, value);
    assert_eq!(
        again.brushes[1].as_swatch().map(|s| s.swatch.key),
        Some(Some(Hash32::of("linen").get()))
    );
    assert_eq!(again.brushes[0].as_scalar().map(|s| s.value), Some(0.75));
    let (second, ..) = build(target, &again);
    assert_eq!(second, tree, "the owned save is the tree's save");
    assert_eq!(second, first, "a fixed point");
}

#[test]
fn a_generic_tagged_base_opens_owned_and_saves_as_the_tree_for_legacy() {
    opens_saves_as_the_tree_and_reaches_a_fixed_point("gen8", legacy());
}

#[test]
fn a_generic_tagged_base_opens_owned_and_saves_as_the_tree_for_enhanced() {
    opens_saves_as_the_tree_and_reaches_a_fixed_point("gen9", enhanced());
}

#[test]
fn a_generic_tagged_base_names_its_variants_per_target() {
    assert_eq!(
        <Raw<BrushSwatch<Gen9>> as VariantOf<Raw<Brush<Gen9>>>>::TAGS,
        &[6]
    );
    assert_eq!(
        <Raw<BrushScalar<Gen8>> as VariantOf<Raw<Brush<Gen8>>>>::TAGS,
        &[1]
    );
    for target in ["gen8", "gen9"] {
        let pack = pack(target);
        let base = pack
            .types
            .iter()
            .find(|t| t.name == "Brush")
            .expect("Brush");
        let selector = base.selector.as_ref().expect("a selector");
        assert_eq!(
            selector.variants,
            [
                (1, "BrushScalar".to_string()),
                (6, "BrushSwatch".to_string())
            ]
        );
        let swatch = pack
            .types
            .iter()
            .find(|t| t.name == "Swatch")
            .expect("Swatch");
        assert_eq!(swatch.size, if target == "gen8" { 0x08 } else { 0x10 });
    }
}

/// A tagged base two of whose tags select one layout, as a particle shader variable's vector tags do.
#[record(
    size = 0x08,
    tag = "kind",
    variant(1 => DabWide<T>),
    variant(2 => DabWide<T>),
    variant(3 => DabFine<T>)
)]
pub struct Dab<T: Stock> {
    pub id: u32,
    pub kind: u32,
}

#[record(size = 0x10)]
pub struct DabWide<T: Stock> {
    pub base: Dab<T>,
    #[pad_after(4)]
    pub width: f32,
}

#[record(size = 0x10)]
pub struct DabFine<T: Stock> {
    pub base: Dab<T>,
    pub swatch: Keyed<T::Swatch>,
}

#[record(size = 0x20)]
pub struct Palette<T: Stock = Gen8> {
    pub swatches: Vec<Option<Box<T::Swatch>>>,
    pub dabs: Vec<Option<Box<DabAny<T>>>>,
}

#[test]
fn tags_that_select_one_layout_open_as_one_variant() {
    let pack = Arc::new(describe("synthetic", "gen8", &[root::<Raw<Palette<Gen8>>>]).unwrap());
    let wide = |id, kind| {
        Some(Box::new(DabAny::DabWide(DabWide {
            base: Dab {
                id,
                kind,
                ..Default::default()
            },
            width: id as f32,
            ..Default::default()
        })))
    };
    let palette = Palette::<Gen8> {
        swatches: vec![Some(Box::new(gen8::Swatch {
            hash: Hash32::of("chalk"),
            ..Default::default()
        }))],
        dabs: vec![
            wide(1, 1),
            wide(2, 2),
            Some(Box::new(DabAny::DabFine(DabFine {
                base: Dab {
                    id: 3,
                    kind: 3,
                    ..Default::default()
                },
                swatch: Keyed::to(Hash32::of("chalk").get()),
                ..Default::default()
            }))),
        ],
        ..Default::default()
    };
    assert_eq!(
        <Raw<DabWide<Gen8>> as VariantOf<Raw<Dab<Gen8>>>>::TAGS,
        &[1, 2]
    );
    let tree = palette.to_element(&Cx::new(pack.plans().clone())).unwrap();
    let (bytes, virtual_size, root) = image_of(or_io::write::build(&pack, &tree).unwrap());
    let aligned = Aligned::from_bytes(&bytes);
    let doc = Doc::<Raw<Palette<Gen8>>>::read(
        pack.clone(),
        desc::Image::new(aligned.bytes(), virtual_size),
        root,
    )
    .unwrap();
    let again = Palette::<Gen8>::from_element(doc.value()).unwrap();
    assert_eq!(again, palette);
    assert_eq!(again.dabs[1].as_wide().map(|d| d.base.kind), Some(2));
}
