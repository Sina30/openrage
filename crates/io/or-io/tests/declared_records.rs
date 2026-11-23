//! One declaration read typed and through the tree, saved to a fixed point,
//! edited, and held to `offset_of!` — on invented types, so hosted CI runs it.

mod synthetic;

use std::mem::{offset_of, size_of};
use std::sync::Arc;

use or_io::decl::{
    self, Aligned, Cx, Described, Doc, Gen8, Gen9, Hash32, Member, OpenEnum, Owned, Plain, Raw,
    Record, View, ViewError,
};
use or_io::desc::{self, FieldRepr, Pack, Value};
use synthetic::*;

fn pack(target: &str) -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = match target {
        "gen8" => &[decl::root::<Raw<Catalogue>>],
        _ => &[decl::root::<Raw<Frame<Gen9>>>],
    };
    Arc::new(
        decl::describe("synthetic", target, roots).expect("the synthetic declarations describe"),
    )
}

fn aligned(image: &synthetic::Image) -> (Aligned, usize) {
    (Aligned::from_bytes(&image.bytes), image.virtual_size)
}

/// Everything a typed read of a catalogue says, pointers followed and addresses left out.
fn facts(c: View<'_, Raw<Catalogue>>) -> Vec<String> {
    let mut out = vec![
        format!("header {:?}", *View::get(&c.header())),
        format!("shape {}", c.header.shape.get()),
        format!("name {:?}", c.name().unwrap()),
        format!("points {:?}", c.points().unwrap().as_slice()),
        format!(
            "flags {:#x} unknown {:#x} stamp {:#x}",
            c.flags, c.reserved, c.stamp
        ),
        format!("primary {:?}", c.primary().unwrap().map(|p| *View::get(&p))),
        format!("spare {:?}", c.spare().unwrap().map(|p| *View::get(&p))),
        format!("tags {:?}", c.tags().unwrap().as_slice()),
        format!("tail {:?}", c.tail().unwrap().as_slice()),
    ];
    let held = c.held().unwrap().expect("held");
    let slots = held.items().unwrap();
    for i in 0..slots.len() {
        out.push(format!(
            "held[{i}] {:?}",
            slots.follow(i).unwrap().map(|p| *View::get(&p))
        ));
    }
    let channels = c.held_channels().unwrap().expect("held channels");
    let channel = channels
        .items()
        .unwrap()
        .follow(0)
        .unwrap()
        .expect("a channel");
    out.push(format!(
        "channel {:?} {:?}",
        *View::get(&channel.first()),
        *View::get(&channel.second())
    ));
    out.push(format!(
        "selected {:?}",
        channel.selected().unwrap().map(|p| *View::get(&p))
    ));
    let order = channel.order().unwrap();
    out.push(format!("order len {}", order.len()));
    let frame = c.frame().unwrap().expect("frame");
    let swatch = frame.swatch().unwrap().expect("swatch");
    out.push(format!(
        "frame {} swatch {}x{} pixels {:?}",
        frame.index,
        swatch.width,
        swatch.height,
        swatch.pixels().unwrap().as_slice()
    ));
    out
}

#[test]
fn a_declaration_describes_itself_as_offset_of_does() {
    let pack = pack("gen8");
    let expect = |name: &str, size: usize, fields: &[(&str, usize)]| {
        let ty = pack
            .type_by_name(name)
            .unwrap_or_else(|| panic!("`{name}` is described"));
        assert_eq!(ty.size, size, "`{name}`'s size");
        let described: Vec<(&str, usize)> = ty
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.offset))
            .collect();
        assert_eq!(described, fields, "`{name}`'s fields");
    };
    expect(
        "Header",
        size_of::<Raw<Header>>(),
        &[
            ("magic", offset_of!(Raw<Header>, magic)),
            ("version", offset_of!(Raw<Header>, version)),
            ("shape", offset_of!(Raw<Header>, shape)),
            ("visible", offset_of!(Raw<Header>, visible)),
            ("id", offset_of!(Raw<Header>, id)),
            ("reserved", offset_of!(Raw<Header>, reserved)),
            ("centre", offset_of!(Raw<Header>, centre)),
        ],
    );
    expect(
        "Catalogue",
        size_of::<Raw<Catalogue>>(),
        &[
            ("header", offset_of!(Raw<Catalogue>, header)),
            ("name", offset_of!(Raw<Catalogue>, name)),
            ("points", offset_of!(Raw<Catalogue>, points)),
            ("point_count", offset_of!(Raw<Catalogue>, point_count)),
            ("flags", offset_of!(Raw<Catalogue>, flags)),
            ("reserved", offset_of!(Raw<Catalogue>, reserved)),
            ("primary", offset_of!(Raw<Catalogue>, primary)),
            ("spare", offset_of!(Raw<Catalogue>, spare)),
            ("tags", offset_of!(Raw<Catalogue>, tags)),
            ("held", offset_of!(Raw<Catalogue>, held)),
            ("held_channels", offset_of!(Raw<Catalogue>, held_channels)),
            ("channel", offset_of!(Raw<Catalogue>, channel)),
            ("frame", offset_of!(Raw<Catalogue>, frame)),
            ("tail_count", offset_of!(Raw<Catalogue>, tail_count)),
            ("stamp", offset_of!(Raw<Catalogue>, stamp)),
            ("tail", offset_of!(Raw<Catalogue>, tail)),
        ],
    );
    expect(
        "Holder<Point>",
        size_of::<Raw<Holder<Point>>>(),
        &[
            ("items", offset_of!(Raw<Holder<Point>>, items)),
            ("count", offset_of!(Raw<Holder<Point>>, count)),
        ],
    );
    expect(
        "Channel",
        size_of::<Raw<Channel>>(),
        &[
            ("first", offset_of!(Raw<Channel>, first)),
            ("second", offset_of!(Raw<Channel>, second)),
            ("selected", offset_of!(Raw<Channel>, selected)),
            ("order", offset_of!(Raw<Channel>, order)),
        ],
    );
    let frame = pack
        .type_by_name("Frame")
        .expect("a target parameter leaves no trace in the name");
    assert_eq!(frame.size, size_of::<Raw<Frame<Gen8>>>());
    assert_eq!(size_of::<Raw<Frame<Gen8>>>(), size_of::<Raw<Frame<Gen9>>>());

    let catalogue = pack.type_by_name("Catalogue").expect("described");
    let field = |n: &str| {
        catalogue
            .fields
            .iter()
            .find(|f| f.name == n)
            .unwrap()
            .repr
            .clone()
    };
    assert!(
        catalogue.fields.iter().all(|f| !f.name.starts_with("_pad")),
        "padding is left undescribed, so the writer writes it as zero"
    );
    assert!(matches!(field("tail"), FieldRepr::Trailing { .. }));
    assert!(matches!(field("tags"), FieldRepr::AtArray(_)));
    assert_eq!(
        catalogue
            .fields
            .iter()
            .find(|f| f.name == "stamp")
            .unwrap()
            .note
            .as_deref(),
        Some("a checksum nothing reads")
    );
}

#[test]
fn the_built_in_description_is_what_the_roots_reach() {
    let legacy = pack("gen8");
    let names: Vec<&str> = legacy.types.iter().map(|t| t.name.as_str()).collect();
    for name in [
        "Catalogue",
        "Header",
        "Point",
        "Holder<Point>",
        "Holder<Channel>",
        "Channel",
        "Frame",
        "Swatch",
    ] {
        assert!(names.contains(&name), "`{name}` is reached; got {names:?}");
    }
    assert!(
        !names.contains(&"Loose"),
        "nothing reaches `Loose`, so nothing describes it"
    );
    assert_eq!(
        legacy.type_by_name("Swatch").unwrap().size,
        size_of::<Raw<gen8::Swatch>>()
    );
    assert_eq!(
        legacy.enum_by_name("Shape").map(|e| e.variants.len()),
        Some(4)
    );

    let enhanced = pack("gen9");
    assert_eq!(
        enhanced.type_by_name("Swatch").unwrap().size,
        size_of::<Raw<gen9::Swatch>>()
    );
    assert!(enhanced
        .type_by_name("Swatch")
        .unwrap()
        .fields
        .iter()
        .any(|f| f.name == "layers"));

    let crossed = decl::describe("synthetic", "gen9", &[decl::root::<Raw<Frame<Gen8>>>])
        .expect_err("crossed");
    assert!(
        crossed
            .iter()
            .any(|p| p.contains("`Swatch` is declared for `gen8` only")),
        "{crossed:?}"
    );
}

#[test]
fn a_typed_read_equals_the_tree_read() {
    let pack = pack("gen8");
    let image = catalogue();
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let typed = View::<Raw<Catalogue>>::at_offset(img, 0).expect("typed root");
    let tree = desc::value::read(&pack, desc::TypeName("Catalogue"), img, 0).expect("tree root");
    let root = tree.record().expect("a record");

    // Every byte the description says is data, at every depth, pointers aside.
    let agree = |ty: &str, typed: &[u8], tree: &[u8]| {
        for (start, end) in data_ranges(&pack, ty, 0) {
            assert_eq!(
                &typed[start..end],
                &tree[start..end],
                "`{ty}` bytes {start:#x}..{end:#x}"
            );
        }
    };
    agree("Catalogue", typed.as_bytes(), root.bytes());
    assert_eq!(
        typed.header.version as i128,
        root.int("header.version").unwrap()
    );
    assert_eq!(
        typed.header.shape.raw() as i128,
        root.int("header.shape").unwrap()
    );
    assert_eq!(
        typed.header.id.get() as i128,
        root.int("header.id").unwrap()
    );
    assert_eq!(typed.header.shape.get(), Shape::Long);
    assert!(typed.header.visible == true);

    let points = typed.points().unwrap();
    let run = target(root.child("points").unwrap())
        .records()
        .expect("a packed run");
    assert_eq!(points.len(), run.len());
    for i in 0..points.len() {
        agree("Point", points[i].as_bytes(), run.get(i).unwrap().bytes());
    }

    let primary = typed.primary().unwrap().unwrap();
    agree(
        "Point",
        primary.as_bytes(),
        target(root.child("primary").unwrap())
            .record()
            .unwrap()
            .bytes(),
    );
    assert!(typed.spare().unwrap().is_none());
    assert!(target_opt(root.child("spare").unwrap()).is_none());

    assert_eq!(typed.name().unwrap().unwrap().to_bytes(), b"stock");
    let tags: Vec<u32> = typed.tags().unwrap().iter().map(|h| h.get()).collect();
    assert_eq!(tags, ints(target(root.child("tags").unwrap()), 4));
    let tail: Vec<u32> = typed.tail().unwrap().to_vec();
    assert_eq!(tail, ints(root.child("tail").unwrap(), 4));

    let held = typed.held().unwrap().unwrap();
    let held_tree = target(root.child("held").unwrap()).record().unwrap();
    agree("Holder<Point>", held.as_bytes(), held_tree.bytes());
    let slots_tree = target(held_tree.child("items").unwrap())
        .elements()
        .unwrap();
    let slots = held.items().unwrap();
    assert_eq!(slots.len(), slots_tree.len());
    agree(
        "Point",
        slots.follow(0).unwrap().unwrap().as_bytes(),
        target(&slots_tree[0]).record().unwrap().bytes(),
    );
    assert!(slots.follow(1).unwrap().is_none());

    let frame = typed.frame().unwrap().unwrap();
    let frame_tree = target(root.child("frame").unwrap()).record().unwrap();
    agree("Frame", frame.as_bytes(), frame_tree.bytes());
    let swatch = frame.swatch().unwrap().unwrap();
    let swatch_tree = target(frame_tree.child("swatch").unwrap())
        .record()
        .unwrap();
    agree("Swatch", swatch.as_bytes(), swatch_tree.bytes());
    assert_eq!(
        swatch.pixels().unwrap().as_slice(),
        target(swatch_tree.child("pixels").unwrap())
            .as_bytes()
            .unwrap()
    );
}

#[test]
fn a_record_saves_to_a_fixed_point() {
    let pack = pack("gen8");
    let image = catalogue();
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let before = facts(View::at_offset(img, 0).unwrap());

    let first = Doc::<Raw<Catalogue>>::read(pack.clone(), img, 0)
        .unwrap()
        .save()
        .expect("first save");
    let after = facts(
        first
            .view::<Raw<Catalogue>>()
            .expect("the save reads back typed"),
    );
    assert_eq!(before, after, "the first save is value-equivalent");

    let second = Doc::<Raw<Catalogue>>::read(pack.clone(), first.image(), first.root)
        .unwrap()
        .save()
        .unwrap();
    assert_eq!(
        first.bytes.bytes(),
        second.bytes.bytes(),
        "the second save is byte-identical"
    );
    assert_eq!(first.virtual_size, second.virtual_size);
    let third = Doc::<Raw<Catalogue>>::read(pack, second.image(), second.root)
        .unwrap()
        .save()
        .unwrap();
    assert_eq!(second.bytes.bytes(), third.bytes.bytes(), "and stays so");
}

#[test]
fn unknown_regions_are_carried_and_padding_is_written_as_zero() {
    let pack = pack("gen8");
    let image = catalogue();
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let saved = Doc::<Raw<Catalogue>>::read(pack, img, 0)
        .unwrap()
        .save()
        .unwrap();
    let c = saved.view::<Raw<Catalogue>>().unwrap();
    assert_eq!(
        c.reserved, 0x1234_5678,
        "an unknown field survives the save untouched"
    );
    assert_eq!(c.header.reserved, 0xDEAD_BEEF);
    assert_eq!(
        c.header.centre.w.to_bits(),
        0x7FC0_0001,
        "a NaN's payload is carried"
    );
    let root = saved.root;
    assert_eq!(
        &saved.bytes.bytes()[root + 0x7c..root + 0x80],
        &[0; 4],
        "declared padding is zero"
    );
    let held = c.held().unwrap().unwrap();
    let at = View::offset(&held);
    assert_eq!(&saved.bytes.bytes()[at + 0x0a..at + 0x10], &[0; 6]);
    assert_eq!(
        &image.bytes[0x16a..0x170],
        &[0xAA; 6],
        "the source padding was dirty"
    );
}

#[test]
fn an_edit_through_a_record_survives_the_save() {
    let pack = pack("gen8");
    let image = catalogue();
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let mut doc = Doc::<Raw<Catalogue>>::read(pack.clone(), img, 0).unwrap();
    {
        let mut root = doc.root_mut();
        root.header.version = 7;
        root.header.shape = or_io::decl::Open::new(Shape::Square);
        root.flags = 0x0042;
        {
            let mut points = root.points_mut().unwrap();
            assert_eq!(points.len(), 3);
            points.get_mut(1).unwrap().unwrap().x = -1.0;
            let mut extra = <Raw<Point> as Plain>::zeroed();
            extra.x = 100.0;
            extra.weight = 40;
            points.push(extra).unwrap();
            assert_eq!(points.len(), 4);
        }
        assert_eq!(
            root.point_count, 4,
            "the insert kept the count, and the handle sees it"
        );
        root.primary_mut().unwrap().unwrap().weight = 77;
    }
    let saved = doc.save().expect("the edited document saves");
    let c = saved.view::<Raw<Catalogue>>().unwrap();
    assert_eq!(c.header.version, 7);
    assert_eq!(c.header.shape.get(), Shape::Square);
    assert_eq!(c.flags, 0x42);
    assert_eq!(c.point_count, 4);
    let points = c.points().unwrap();
    assert_eq!(points[1].x, -1.0);
    assert_eq!((points[3].x, points[3].weight), (100.0, 40));
    assert_eq!(c.primary().unwrap().unwrap().weight, 77);
    assert_eq!(
        c.reserved, 0x1234_5678,
        "an edit elsewhere leaves the unknown alone"
    );

    let again = Doc::<Raw<Catalogue>>::read(pack, saved.image(), saved.root)
        .unwrap()
        .save()
        .unwrap();
    assert_eq!(
        saved.bytes.bytes(),
        again.bytes.bytes(),
        "the edited file is itself a fixed point"
    );
}

#[test]
fn a_pointer_that_does_not_land_is_refused_by_name() {
    let mut image = catalogue();
    // `primary` outside the image
    image.bytes[0x38..0x40].copy_from_slice(&(VIRTUAL_BASE + 0x0FFF_0000).to_le_bytes());
    // `spare` at an address in neither half
    image.bytes[0x40..0x48].copy_from_slice(&0x7000_0010u64.to_le_bytes());
    // `points` says 400 elements
    image.bytes[0x30..0x32].copy_from_slice(&400u16.to_le_bytes());
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let c = View::<Raw<Catalogue>>::at_offset(img, 0).unwrap();

    assert!(
        matches!(
            c.primary(),
            Err(ViewError::OutOfBounds { .. })
                | Err(ViewError::BadPointer {
                    field: "primary",
                    ..
                })
        ),
        "{:?}",
        c.primary()
    );
    assert!(
        matches!(c.spare(), Err(ViewError::BadPointer { field: "spare", .. })),
        "{:?}",
        c.spare()
    );
    assert!(
        matches!(c.points(), Err(ViewError::OutOfBounds { .. })),
        "{:?}",
        c.points()
    );
    let named = c.spare().unwrap_err().to_string();
    assert!(named.contains("Catalogue.spare"), "{named}");
}

#[test]
fn a_misaligned_instance_is_refused_and_a_member_pointer_must_land_on_a_member() {
    let mut image = catalogue();
    image.bytes[0x38..0x40].copy_from_slice(&(VIRTUAL_BASE + 0x202).to_le_bytes());
    // `selected` at the channel's own start plus four: inside, but not a member
    image.bytes[0x1c0..0x1c8].copy_from_slice(&(VIRTUAL_BASE + 0x1a4).to_le_bytes());
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let c = View::<Raw<Catalogue>>::at_offset(img, 0).unwrap();
    assert!(
        matches!(c.primary(), Err(ViewError::Misaligned { align: 4, .. })),
        "{:?}",
        c.primary()
    );
    let channel = c.channel().unwrap().unwrap();
    assert!(
        matches!(channel.selected(), Err(ViewError::NotAMember { .. })),
        "{:?}",
        channel.selected()
    );

    let good = catalogue();
    let (bytes, virtual_size) = aligned(&good);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let channel = View::<Raw<Catalogue>>::at_offset(img, 0)
        .unwrap()
        .channel()
        .unwrap()
        .unwrap();
    let second = channel.selected().unwrap().unwrap();
    assert_eq!(
        View::offset(&second),
        View::offset(&channel) + offset_of!(Raw<Channel>, second)
    );
}

#[test]
fn a_record_carries_its_target_and_the_other_target_refuses_it() {
    let legacy = pack("gen8");
    let enhanced = pack("gen9");
    let image = catalogue();
    let (bytes, virtual_size) = aligned(&image);
    let img = desc::Image::new(bytes.bytes(), virtual_size);
    let swatch_at = 0x230;
    assert!(Doc::<Raw<gen8::Swatch>>::read(legacy.clone(), img, swatch_at).is_ok());
    let refused = Doc::<Raw<gen8::Swatch>>::read(enhanced, img, swatch_at)
        .err()
        .expect("refused");
    assert!(
        refused.to_string().contains("`gen8` declaration"),
        "{refused}"
    );
    assert_eq!(<Raw<gen8::Swatch> as Described>::target(), Some("gen8"));
    assert_eq!(<Raw<Frame<Gen8>> as Described>::target(), None);
}

#[test]
fn an_enum_names_its_values_and_keeps_the_ones_it_does_not() {
    assert_eq!(Shape::Long.to_string(), "Long (4)");
    assert_eq!(Shape::from_raw(7), Shape::Other(7));
    assert_eq!(Shape::Other(7).to_string(), "7");
    assert_eq!(Shape::from_raw(1).to_raw(), 1);
    let e = Shape::describe();
    let notes: Vec<(&str, i64, Option<&str>)> = e
        .variants
        .iter()
        .map(|v| (v.name.as_str(), v.value, v.note.as_deref()))
        .collect();
    assert_eq!(
        notes,
        [
            ("Round", 0, Some("Round")),
            ("Square", 1, Some("Square")),
            ("Long", 4, Some("Long")),
            ("Flat", 6, Some("Flat"))
        ],
        "a variant's note is the first line of its doc comment that says something"
    );
}

/// ⚠ A variant's first doc line is pack data: it is what renders a value as `Geometry (4)`.
#[test]
fn every_declared_enum_variant_carries_its_note() {
    let mut silent = Vec::new();
    for format in or_io::gta5::formats::FORMATS {
        for target in format.targets() {
            let pack = format.description(target).expect("describes");
            for e in &pack.enums {
                for v in e.variants.iter().filter(|v| v.note.is_none()) {
                    silent.push(format!("{}::{} ({})", e.name, v.name, format.ext));
                }
            }
        }
    }
    silent.sort();
    silent.dedup();
    assert!(silent.is_empty(), "no note: {silent:#?}");
}

#[test]
fn a_bit_range_is_described_on_its_word_and_a_patch_refuses_the_word() {
    use or_io::desc::value::ScalarValue;
    use or_io::desc::ImageMut;
    use or_io::write::{patch, Edit, PatchError};

    let pack = decl::describe("synthetic", "gen8", &[decl::root::<Raw<Slot>>]).unwrap();
    let slot = pack.type_by_name("Slot").expect("Slot");
    let word = slot.field_by_name("word").expect("word");
    assert!(word.is_bit_packed());
    let place = word.bits.iter().find(|b| b.name == "place").expect("place");
    assert_eq!(
        (place.low, place.width, place.mask()),
        (10, 21, ((1u64 << 21) - 1) << 10)
    );
    assert_eq!(place.note.as_deref(), Some("in blocks of 256 bytes"));
    assert!(!slot.field_by_name("body").unwrap().is_bit_packed());

    let mut bytes = vec![0u8; size_of::<Raw<Slot>>()];
    let untouched = bytes.clone();
    let mut image = ImageMut::new(&mut bytes, untouched.len());
    let refused = patch(
        &pack,
        "Slot",
        &mut image,
        0,
        &[Edit::new("word", ScalarValue::Int(1))],
    )
    .expect_err("a word of bit ranges is not patched whole");
    assert!(
        matches!(refused, PatchError::BitPacked { .. }),
        "{refused:?}"
    );
    assert_eq!(bytes, untouched);
}

#[test]
fn a_generic_record_is_named_with_its_parameter() {
    assert_eq!(<Raw<Holder<Point>> as Described>::name(), "Holder<Point>");
    assert_eq!(
        <Raw<Holder<Channel>> as Described>::name(),
        "Holder<Channel>"
    );
    assert_eq!(Hash32::of("catalogue"), Hash32::of("catalogue"));
}

fn owned_pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[
        decl::root::<Raw<Tally>>,
        decl::root::<Raw<Links>>,
        decl::root::<Raw<Palette>>,
        decl::root::<Raw<Shelves>>,
    ];
    Arc::new(
        decl::describe("synthetic", "gen8", roots)
            .expect("the owned synthetic declarations describe"),
    )
}

/// `value` saved through the canonical writer and opened again, with the bytes the save made.
fn saved<T: Owned>(value: &T) -> Result<(T, Vec<u8>), String>
where
    Raw<T>: Record,
{
    let pack = owned_pack();
    let tree = value
        .to_element(&Cx::new(pack.plans().clone()))
        .map_err(|e| e.to_string())?;
    let built = or_io::write::build(&pack, &tree).map_err(|e| e.to_string())?;
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    let root = (built.root - or_io::constants::FIXED_VIRTUAL_BASE) as usize;
    let aligned = Aligned::from_bytes(&bytes);
    let doc = Doc::<Raw<T>>::read(pack, desc::Image::new(aligned.bytes(), virtual_size), root)
        .map_err(|e| e.to_string())?;
    let again = T::from_element(doc.value()).map_err(|e| e.to_string())?;
    Ok((again, bytes))
}

fn mark(i: u32) -> Mark {
    Mark {
        x: i as f32,
        y: 0.5,
        weight: i,
        id: Hash32::of(&format!("mark{i}")),
        ..Default::default()
    }
}

fn tally() -> Tally {
    Tally {
        entries: vec![1, 2, 3],
        entry_count: 3,
        slots: vec![7, 8],
        slot_count: 2,
        slot_capacity: 2,
        blob: vec![0xB0; 0x20],
        packed: 0x0100_0000 | 0x20,
        grid: vec![1; 6],
        row_count: 2,
        row_width: 3,
        parts: vec![10, 11, 12],
        head_count: 1,
        tail_count: 2,
        corners: vec![0.0, 1.0, 2.0, 3.0],
        payload: vec![0xAB; 5],
        byte_count: 5,
        boxes: vec![mark(0), mark(1), mark(2)],
        box_count: 2,
        ..Default::default()
    }
}

#[test]
fn an_owned_run_of_every_len_kind_saves_to_a_fixed_point() {
    let (again, first) = saved(&tally()).unwrap();
    assert_eq!(again, tally());
    let (_, second) = saved(&again).unwrap();
    assert_eq!(first, second, "the second save is byte-identical");
}

#[test]
fn a_resized_owned_run_reopens_at_its_new_length_where_its_count_can_say_it() {
    let mut t = tally();
    t.entries.push(4);
    t.slots.push(9);
    t.blob.resize(0x30, 0xB1);
    t.grid.resize(9, 2);
    t.payload.push(0xAC);
    t.boxes.push(mark(3));
    let (again, _) = saved(&t).unwrap();
    assert_eq!((again.entries.len(), again.entry_count), (4, 4));
    assert_eq!(
        (again.slots.len(), again.slot_count, again.slot_capacity),
        (3, 3, 3)
    );
    assert_eq!(again.blob.len(), 0x30);
    assert_eq!(
        again.packed,
        0x0100_0000 | 0x30,
        "the bits outside the mask are carried"
    );
    assert_eq!(
        (again.grid.len(), again.row_count, again.row_width),
        (9, 3, 3)
    );
    assert_eq!((again.payload.len(), again.byte_count), (6, 6));
    assert_eq!((again.boxes.len(), again.box_count), (4, 3));
    assert!(again.boxes[3].id == "mark3");

    t.boxes.truncate(1);
    t.parts.push(13);
    t.tail_count = 3;
    let (again, _) = saved(&t).unwrap();
    assert_eq!((again.boxes.len(), again.box_count), (1, 1));
    assert_eq!(
        again.parts,
        [10, 11, 12, 13],
        "a sum's parts edited with it"
    );
}

#[test]
fn a_resized_owned_run_its_count_cannot_state_is_refused_by_name() {
    type Resize = fn(&mut Tally);
    let cases: [(&str, Resize); 5] = [
        ("Tally.blob", |t| t.blob.push(0)),
        ("Tally.grid", |t| t.grid.push(0)),
        ("Tally.parts", |t| t.parts.push(0)),
        ("Tally.corners", |t| t.corners.push(0.0)),
        ("Tally.boxes", |t| t.boxes.truncate(2)),
    ];
    for (field, resize) in cases {
        let mut t = tally();
        resize(&mut t);
        let why = saved(&t).unwrap_err();
        assert!(why.starts_with(field), "{field}: {why}");
    }
}

#[test]
fn an_empty_run_saves_as_a_null_pointer_beside_the_count_it_was_read_with() {
    let mut t = tally();
    t.blob.clear();
    t.grid.clear();
    t.parts.clear();
    t.corners.clear();
    t.boxes.clear();
    let (again, first) = saved(&t).unwrap();
    assert_eq!(
        again, t,
        "a masked, product, sum, fixed or prefixed count beside a null pointer is carried"
    );
    assert_eq!(saved(&again).unwrap().1, first);
}

#[test]
fn two_runs_one_count_states_save_when_they_agree_and_the_later_is_refused_when_not() {
    let shelves = |boxes: u32, weights: u16| Shelves {
        boxes: (0..boxes).map(mark).collect(),
        weights: (0..weights).collect(),
        ..Default::default()
    };
    let (again, _) = saved(&shelves(4, 3)).unwrap();
    assert_eq!(
        (again.boxes.len(), again.weights.len(), again.shelf_count),
        (4, 3, 3)
    );
    let why = saved(&shelves(4, 2)).unwrap_err();
    assert!(
        why.starts_with("Shelves.weights") && why.contains("also counts another run"),
        "{why}"
    );
}

#[test]
fn every_pointer_an_owned_record_holds_saves_and_reopens() {
    let disc = |rim| {
        FigureAny::Disc(Disc {
            base: Figure {
                kind: 1,
                ..Default::default()
            },
            radius: 2.5,
            rim,
            ..Default::default()
        })
    };
    let mut links = Links {
        first: mark(1),
        second: mark(2),
        selected: Member::to("second"),
        primary: Box::new(mark(3)),
        spare: None,
        figure: Some(Box::new(disc(7))),
        figures: vec![
            Some(Box::new(FigureAny::Base(Figure {
                kind: 9,
                ..Default::default()
            }))),
            None,
        ],
        ..Default::default()
    };
    let (again, first) = saved(&links).unwrap();
    assert_eq!(again, links);
    assert!(again.second.id == "mark2");
    assert!(again.second.id != "mark1");
    assert_eq!(saved(&again).unwrap().1, first);

    links.selected = Member::to("first");
    links.spare = Some(Box::new(mark(4)));
    links.figures.push(Some(Box::new(disc(8))));
    let (again, _) = saved(&links).unwrap();
    assert_eq!(again, links);
    assert!(matches!(
        again.figures[2].as_deref(),
        Some(FigureAny::Disc(d)) if d.rim == 8
    ));
}

#[test]
fn an_owned_array_longer_than_thirty_two_has_a_default() {
    let mut palette = Palette::default();
    assert_eq!(palette.colours, [0; 52]);
    palette.colours[51] = 0x00FF_00FF;
    assert_eq!(saved(&palette).unwrap().0, palette);
}

/// Byte ranges of `ty` that hold data rather than an address or padding, at any depth.
fn data_ranges(pack: &Pack, ty: &str, base: usize) -> Vec<(usize, usize)> {
    let ty = pack
        .type_by_name(ty)
        .unwrap_or_else(|| panic!("`{ty}` is described"));
    let mut out = Vec::new();
    for f in &ty.fields {
        let at = base + f.offset;
        match &f.repr {
            FieldRepr::Inline { type_name } => out.extend(data_ranges(pack, type_name, at)),
            FieldRepr::Ptr(_)
            | FieldRepr::AtArray(_)
            | FieldRepr::PtrSlots { .. }
            | FieldRepr::Padding { .. } => {}
            FieldRepr::Trailing { .. } => {}
            repr => out.push((at, at + repr.inline_size().expect("a scalar has a width"))),
        }
    }
    out
}

fn target(v: &Value) -> &Value {
    target_opt(v).expect("a pointer with a target")
}

fn target_opt(v: &Value) -> Option<&Value> {
    match v {
        Value::Ptr { target, .. } => target.as_deref(),
        other => Some(other),
    }
}

fn ints(v: &Value, width: usize) -> Vec<u32> {
    match v {
        Value::Bytes(bytes) => bytes
            .chunks(width)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect(),
        Value::Array { elements, .. } => elements
            .iter()
            .map(|e| e.as_int().unwrap() as u32)
            .collect(),
        other => panic!("not a run of integers: {other:?}"),
    }
}
