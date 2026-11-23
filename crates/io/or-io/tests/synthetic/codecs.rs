//! A `#[codec]` field on invented owned records shaped for the drawable
//! codecs: a Legacy parameter list, a bone-tag table and an Enhanced parameter
//! block. Each value built from scratch saves through the canonical writer,
//! reads back through the codecs' readers equal to itself, and saves again to
//! the same bytes; a save whose codec cannot state what it holds is refused.

use std::mem::{offset_of, size_of};
use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::container::paging::ResourceHeap;
use or_io::decl::{self, Aligned, Cx, Hash32, Owned, Plain, Raw, Record, Vec4};
use or_io::desc::{Pack, TypeName};
use or_io::gta5::drawable_codecs::gen8::{EntryData, GrcInstanceDataEntry};
use or_io::gta5::drawable_codecs::{self, gen9::Params};
use or_io::write::{build_into_with, read_using, ElementTypes};

pub mod legacy {
    use or_io::decl::*;
    use or_io::gta5::drawable_codecs::{gen8::GrcInstanceDataEntry, BoneTags};

    #[record(size = 0x10, target = "gen8")]
    pub struct Texture {
        pub name: Hash32,
        pub width: u16,
        pub height: u16,
        pub depth: u32,
        _pad_0x0c: u32,
    }

    #[record(size = 0x10, target = "gen8")]
    pub struct MaterialEntry {
        pub value_count: u8,
        pub register: u8,
        _pad_0x02: [u8; 6],
        pub data: Ptr<Opaque>,
    }

    #[record(size = 0x18, target = "gen8")]
    pub struct Material {
        #[codec(Ptr<[Raw<MaterialEntry>]>)]
        #[len(field = "entry_count")]
        pub entries: Vec<GrcInstanceDataEntry<Option<Box<Texture>>>>,
        pub shader: Hash32,
        entry_count: u8,
        texture_count: u8,
        hash_offset: u16,
        blob_size: u16,
        _pad_0x12: [u8; 6],
    }

    #[record(size = 0x10, target = "gen8")]
    pub struct Bone {
        pub tag: u16,
        pub parent: i16,
        pub flags: u32,
        pub weight: f32,
        _pad_0x0c: u32,
    }

    #[record(size = 0x18, target = "gen8")]
    pub struct Skeleton {
        #[codec(Ptr<Opaque>)]
        bone_tags: BoneTags,
        #[len(field = "bone_count")]
        pub bones: Vec<Bone>,
        #[init(7)]
        bone_tag_slots: u16,
        bone_count: u16,
        _pad_0x14: u32,
    }
}

pub mod enhanced {
    use or_io::decl::*;
    use or_io::gta5::drawable_codecs::gen9::Params;

    #[record(size = 0x08, target = "gen9")]
    pub struct Texture {
        pub name: Hash32,
        pub width: u16,
        pub height: u16,
    }

    #[record(size = 0x08, target = "gen9")]
    pub struct ParamInfos {
        buffer_count: u8,
        texture_count: u8,
        other_count: u8,
        sampler_count: u8,
        pub copies: u8,
        _pad_0x05: [u8; 3],
    }

    #[record(size = 0x30, target = "gen9")]
    pub struct Material {
        pub shader: Hash32,
        _pad_0x04: u32,
        #[codec(Ptr<Opaque>)]
        pub params: Params<Option<Box<Texture>>>,
        texture_slots: Ptr<Opaque>,
        other_slots: Ptr<Opaque>,
        pub param_infos: Box<ParamInfos>,
        param_data_size: u16,
        _pad_0x2a: [u8; 6],
    }
}

fn pack(target: &str) -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = if target == "gen8" {
        &[
            decl::root::<Raw<legacy::Material>>,
            decl::root::<Raw<legacy::Texture>>,
            decl::root::<Raw<legacy::Skeleton>>,
        ]
    } else {
        &[
            decl::root::<Raw<enhanced::Material>>,
            decl::root::<Raw<enhanced::Texture>>,
        ]
    };
    Arc::new(decl::describe("synthetic", target, roots).expect("describes"))
}

/// A value laid out by the canonical writer with the drawable codecs: the image, its virtual size and its root.
fn save<T: Owned>(target: &str, value: &T) -> Result<(Aligned, usize, usize), String> {
    let pack = pack(target);
    let tree = value
        .to_element(&Cx::new(pack.plans().clone()))
        .map_err(|e| e.to_string())?;
    let mut heap = ResourceHeap::new();
    let root = build_into_with(&mut heap, &pack, &tree, &drawable_codecs::writers())
        .map_err(|e| e.to_string())?;
    let (virtual_data, physical_data, _, _) = heap.finish_with_fixups();
    let virtual_size = virtual_data.len();
    let mut bytes = virtual_data;
    bytes.extend_from_slice(&physical_data);
    Ok((
        Aligned::from_bytes(&bytes),
        virtual_size,
        (root - FIXED_VIRTUAL_BASE) as usize,
    ))
}

fn open<T: Owned>(target: &str, saved: &(Aligned, usize, usize)) -> Result<T, String>
where
    Raw<T>: Record,
{
    let pack = pack(target);
    let image = or_io::desc::Image::new(saved.0.bytes(), saved.1);
    let name = TypeName(decl::intern(&<Raw<T> as decl::Described>::name()));
    let tree = read_using(
        &pack,
        name,
        image,
        saved.2,
        &ElementTypes::new(),
        &drawable_codecs::readers(),
    )
    .map_err(|e| e.to_string())?;
    T::from_element(&tree).map_err(|e| e.to_string())
}

/// `value` saved, read back equal, and saved again to the same bytes; the first save's bytes.
fn round_trip<T: Owned + PartialEq + std::fmt::Debug>(
    target: &str,
    value: &T,
) -> (Aligned, usize, usize)
where
    Raw<T>: Record,
{
    let first = save(target, value).unwrap();
    let again: T = open(target, &first).unwrap();
    assert_eq!(&again, value);
    let second = save(target, &again).unwrap();
    assert_eq!(second.0.bytes(), first.0.bytes(), "a fixed point");
    first
}

/// Why the save of `value` is refused.
fn refused<T: Owned>(target: &str, value: &T, what: &str) -> String {
    match save(target, value) {
        Ok(_) => panic!("{what} saved"),
        Err(e) => e,
    }
}

/// The root's raw layout as the save wrote it.
fn peek<T: Owned>(saved: &(Aligned, usize, usize)) -> Raw<T> {
    <Raw<T> as Plain>::read_from(&saved.0.bytes()[saved.2..]).expect("the root")
}

fn row(i: u32) -> Vec4 {
    Vec4 {
        x: i as f32,
        y: -(i as f32),
        z: 0.5,
        w: 1.0 / (i as f32),
    }
}

fn texture(name: &str) -> Option<Box<legacy::Texture>> {
    Some(Box::new(legacy::Texture {
        name: Hash32::of(name),
        width: 64,
        height: 32,
        depth: 1,
        ..Default::default()
    }))
}

fn material() -> legacy::Material {
    legacy::Material {
        entries: vec![
            GrcInstanceDataEntry {
                register: 0,
                name_hash: Hash32::of("diffuse"),
                data: EntryData::Texture(texture("brick")),
            },
            GrcInstanceDataEntry {
                register: 1,
                name_hash: Hash32::of("normal"),
                data: EntryData::Texture(None),
            },
            GrcInstanceDataEntry {
                register: 4,
                name_hash: Hash32::of("tint"),
                data: EntryData::Float(vec![row(1)]),
            },
            GrcInstanceDataEntry {
                register: 5,
                name_hash: Hash32::of("matrix"),
                data: EntryData::Float(vec![row(2), row(3), row(4)]),
            },
        ],
        shader: Hash32::of("default"),
        ..Default::default()
    }
}

#[test]
fn a_legacy_parameter_list_saves_its_blob_and_the_counts_it_implies() {
    let value = material();
    let saved = round_trip("gen8", &value);
    let raw = peek::<legacy::Material>(&saved);
    // Four entries, then four rows: one for `tint` and three for `matrix`.
    let (entry, row) = (size_of::<Raw<legacy::MaterialEntry>>(), size_of::<Vec4>());
    let hashes = 4 * entry + 4 * row;
    assert_eq!(
        (
            raw.entry_count,
            raw.texture_count,
            usize::from(raw.hash_offset)
        ),
        (4, 2, hashes)
    );
    assert_eq!(
        usize::from(raw.blob_size),
        (hashes + 4 * size_of::<u32>()).next_multiple_of(row) + 2 * row
    );
}

#[test]
fn an_edited_parameter_list_saves_at_its_new_length() {
    let mut value = material();
    value.entries.remove(1);
    if let EntryData::Float(rows) = &mut value.entries[2].data {
        rows.push(row(9));
    }
    let raw = peek::<legacy::Material>(&round_trip("gen8", &value));
    assert_eq!((raw.entry_count, raw.texture_count), (3, 1));

    value.entries.clear();
    let raw = peek::<legacy::Material>(&round_trip("gen8", &value));
    assert_eq!(
        (raw.entry_count, raw.blob_size, raw.entries.is_null()),
        (0, 0, true)
    );
}

#[test]
fn a_parameter_list_the_blob_cannot_state_is_refused_by_name() {
    let mut value = material();
    value.entries.swap(0, 3);
    let e = refused("gen8", &value, "a texture after a float entry");
    assert!(
        e.contains("Material.entries") && e.contains("textures lead"),
        "{e}"
    );

    let mut value = material();
    value.entries[2].data = EntryData::Float(Vec::new());
    let e = refused("gen8", &value, "an entry of no rows");
    assert!(e.contains("1 to 255"), "{e}");
}

/// A file whose list has a texture behind a constant, as a tool that appends a sampler writes it.
#[test]
fn a_texture_a_file_puts_after_a_constant_opens_ahead_of_it() {
    let value = material();
    let (bytes, virtual_size, root) = save("gen8", &value).unwrap();
    let raw = peek::<legacy::Material>(&(bytes.clone(), virtual_size, root));
    let entry = size_of::<Raw<legacy::MaterialEntry>>();
    let table = (raw.entries.raw() - FIXED_VIRTUAL_BASE) as usize;
    let hashes = table + usize::from(raw.hash_offset);

    // Entries 1 and 2, `normal` and `tint`, trade places, each with its hash.
    let mut edited = bytes.bytes().to_vec();
    for (at, len) in [(table + entry, entry), (hashes + 4, 4)] {
        let (a, b) = edited[at..at + 2 * len].split_at_mut(len);
        a.swap_with_slice(b);
    }
    let file = (Aligned::from_bytes(&edited), virtual_size, root);

    let opened: legacy::Material = open("gen8", &file).unwrap();
    assert_eq!(opened, value, "the textures lead once it is open");
    assert_eq!(
        save("gen8", &opened).unwrap().0.bytes(),
        bytes.bytes(),
        "and it saves as the one that does"
    );
}

fn bone(tag: u16) -> legacy::Bone {
    legacy::Bone {
        tag,
        parent: -1,
        ..Default::default()
    }
}

#[test]
fn a_bone_tag_table_is_derived_from_the_bones_on_every_save() {
    let mut value = legacy::Skeleton {
        bones: [3, 10, 17, 4].map(bone).to_vec(),
        ..Default::default()
    };
    let saved = round_trip("gen8", &value);
    let raw = peek::<legacy::Skeleton>(&saved);
    assert!(!raw.bone_tags.is_null() && raw.bone_tag_slots == 7);

    value.bones.push(bone(24));
    let reopened: legacy::Skeleton = open("gen8", &round_trip("gen8", &value)).unwrap();
    assert_eq!(reopened.bones.len(), 5);
}

#[test]
fn a_bone_tag_table_that_is_not_the_derivation_is_refused_at_open() {
    let value = legacy::Skeleton {
        bones: [3, 10].map(bone).to_vec(),
        ..Default::default()
    };
    let (mut bytes, virtual_size, root) = save("gen8", &value).unwrap();
    // Swap the two bones' tags behind the table's back.
    let raw = peek::<legacy::Skeleton>(&(bytes.clone(), virtual_size, root));
    let first = (raw.bones.raw() - FIXED_VIRTUAL_BASE) as usize;
    let mut edited = bytes.bytes().to_vec();
    for (i, tag) in [10u16, 3].into_iter().enumerate() {
        let at = first + i * size_of::<Raw<legacy::Bone>>() + offset_of!(Raw<legacy::Bone>, tag);
        edited[at..at + size_of::<u16>()].copy_from_slice(&tag.to_le_bytes());
    }
    bytes = Aligned::from_bytes(&edited);
    let e = open::<legacy::Skeleton>("gen8", &(bytes, virtual_size, root))
        .expect_err("a table another bone order implies");
    assert!(e.contains("not the one its bone run implies"), "{e}");
}

fn block() -> enhanced::Material {
    let texture = |name: &str| {
        Some(Box::new(enhanced::Texture {
            name: Hash32::of(name),
            width: 16,
            height: 16,
            ..Default::default()
        }))
    };
    enhanced::Material {
        shader: Hash32::of("default"),
        params: Params {
            buffers: vec![(0u8..32).collect(), (100u8..148).collect()],
            textures: vec![texture("brick"), None, texture("grime")],
            samplers: vec![1, 2],
            others: 2,
        },
        param_infos: Box::new(enhanced::ParamInfos {
            copies: 3,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The buffer, texture, other and sampler counts the saved infos state.
fn stated_counts(saved: &(Aligned, usize, usize)) -> [usize; 4] {
    let raw = peek::<enhanced::Material>(saved);
    let at = (raw.param_infos.raw() - FIXED_VIRTUAL_BASE) as usize;
    let infos =
        <Raw<enhanced::ParamInfos> as Plain>::read_from(&saved.0.bytes()[at..]).expect("the infos");
    [
        infos.buffer_count,
        infos.texture_count,
        infos.other_count,
        infos.sampler_count,
    ]
    .map(usize::from)
}

fn held(p: &Params<Option<Box<enhanced::Texture>>>) -> [usize; 4] {
    [
        p.buffers.len(),
        p.textures.len(),
        p.others,
        p.samplers.len(),
    ]
}

#[test]
fn an_enhanced_parameter_block_writes_every_copy_and_reads_back_as_one() {
    let value = block();
    let saved = round_trip("gen9", &value);
    let raw = peek::<enhanced::Material>(&saved);
    let p = &value.params;
    assert_eq!(stated_counts(&saved), held(p));
    let (copies, slot) = (usize::from(value.param_infos.copies), size_of::<u64>());
    let table = p.buffers.len() * copies * slot;
    let one_copy: usize = p.buffers.iter().map(Vec::len).sum();
    let textures = p.textures.len() * copies * slot;
    let others = p.others * copies * slot;
    assert_eq!(
        usize::from(raw.param_data_size),
        table + copies * one_copy + textures + others + p.samplers.len()
    );
    let params = (raw.params.raw() - FIXED_VIRTUAL_BASE) as usize;
    let slots = (raw.texture_slots.raw() - FIXED_VIRTUAL_BASE) as usize;
    assert_eq!(slots - params, table + copies * one_copy);
    let image = saved.0.bytes();
    let word = |at: usize| u64::from_le_bytes(image[at..at + slot].try_into().unwrap());
    let buffer = |copy: usize, k: usize| word(params + (copy * p.buffers.len() + k) * slot);
    assert_eq!(
        (buffer(1, 0) - buffer(0, 0)) as usize,
        one_copy,
        "each copy addresses its own buffers"
    );
    let texture_slot = |i: usize| word(slots + i * slot);
    assert!(texture_slot(0) != 0 && texture_slot(1) == 0 && texture_slot(2) != 0);
    assert!(
        (p.textures.len()..p.textures.len() * copies).all(|i| texture_slot(i) == 0),
        "later copies' textures are null"
    );
    let others_at = (raw.other_slots.raw() - FIXED_VIRTUAL_BASE) as usize;
    assert_eq!(
        others_at - slots,
        textures,
        "the other slots follow the textures"
    );
    assert!(
        (0..p.others * copies).all(|i| word(others_at + i * slot) == 0),
        "every copy's other slots are null"
    );
}

#[test]
fn an_edited_block_saves_the_counts_its_infos_state() {
    let mut value = block();
    value.params.samplers.push(3);
    value.params.textures.pop();
    value.params.buffers.push(vec![7; size_of::<Vec4>()]);
    let saved = round_trip("gen9", &value);
    assert_eq!(stated_counts(&saved), held(&value.params));

    value.params = Params::default();
    let saved = round_trip("gen9", &value);
    assert_eq!(stated_counts(&saved), [0; 4]);
    assert!(peek::<enhanced::Material>(&saved).params.is_null());

    let mut value = block();
    value.params.buffers[1].push(0);
    let e = refused("gen9", &value, "a buffer that is not whole registers");
    assert!(e.contains("-byte registers"), "{e}");
}
