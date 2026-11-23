//! A Legacy and an Enhanced `.ytd` authored as plain Rust values, with
//! invented content: saved whole, reopened, edited, and refused where the
//! pixels would be lost. Needs no game file.

use std::mem::offset_of;
use std::path::Path;

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::decl::{Aligned, Open, Raw, View};
use or_io::desc::Image;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::{
    GrcTextureG9, PixelFormat, ShaderResourceViewG9, TextureDimension, TileMode,
};
use or_io::gta5::texture_dict::gen8::PAYLOAD_SIZE_MASK;
use or_io::gta5::{formats, File, PgDictionary, Resource};
use or_io::{Asset, Generation};

const PIXELS: usize = 0x80;
/// Bits of the payload word outside [`PAYLOAD_SIZE_MASK`]: the template type.
const TEMPLATE: u32 = 1 << 28;

fn texture(name: &str, pixels: usize) -> GrcTexture {
    let mut t = GrcTexture {
        name: name.into(),
        width: 8,
        height: 8,
        depth: 1,
        mip_stride: 16,
        format: u32::from_le_bytes(*b"DXT1"),
        mip_count: 1,
        backing_store: (0..pixels).map(|i| i as u8).collect(),
        ..Default::default()
    };
    t.set_template_type(TEMPLATE);
    t
}

fn authored() -> PgDictionary<GrcTexture> {
    PgDictionary {
        codes: vec![or_io::hash::string_hash_str("stone")],
        entries: vec![Some(Box::new(texture("stone", PIXELS)))],
        ..Default::default()
    }
}

fn only(dict: &PgDictionary<GrcTexture>) -> &GrcTexture {
    dict.entries[0].as_deref().expect("one texture")
}

fn payload(file: &[u8]) -> Vec<u8> {
    let o = Rsc7::open(file).unwrap();
    let mut out = o.seal.header_bytes();
    out.extend_from_slice(&o.data);
    out
}

#[test]
fn an_authored_legacy_ytd_saves_reopens_and_reaches_a_fixed_point() {
    let first = authored().to_bytes().unwrap();
    let again = PgDictionary::<GrcTexture>::from_bytes(&first).unwrap();
    let t = only(&again);
    assert_eq!((t.width, t.height, t.mip_count), (8, 8, 1));
    assert_eq!(t.format.to_le_bytes(), *b"DXT1");
    assert_eq!(t.name, "stone");
    assert_eq!(t.payload_size(), PIXELS);
    assert_eq!(t.backing_store[5], 5);
    assert_eq!(again, authored());
    assert_eq!(
        stated(&first),
        TEMPLATE | PIXELS as u32,
        "an authored texture states its template type beside its count"
    );

    let peek = PgDictionary::<GrcTexture>::peek(&first).unwrap();
    let root = peek.root().unwrap();
    let t = root.entries().unwrap().follow(0).unwrap().unwrap();
    assert_eq!(t.name().unwrap().unwrap().to_str().unwrap(), "stone");
    assert_eq!(t.backing_store().unwrap().len(), PIXELS);

    let ytd = formats::format("ytd").unwrap();
    let version = Rsc7::open(&first).unwrap().version;
    assert_eq!(ytd.target_of_version(version).unwrap(), Generation::Gen8);
    assert_eq!(payload(&again.to_bytes().unwrap()), payload(&first));
}

#[test]
fn a_texture_is_found_and_inserted_by_its_name() {
    let mut dict = authored();
    assert_eq!(dict.get("stone").unwrap().name, "stone");
    assert_eq!(dict.get("STONE"), dict.get("stone"), "the hash folds case");
    assert!(dict.get("marble").is_none());
    dict.get_mut("stone").unwrap().width = 16;

    assert!(dict.insert(texture("marble", PIXELS)).is_none());
    assert!(dict.insert(texture("granite", PIXELS)).is_none());
    let replaced = dict.insert(texture("stone", PIXELS)).unwrap();
    assert_eq!(replaced.width, 16, "a name already held is replaced");

    let mut codes: Vec<u32> = ["stone", "marble", "granite"]
        .map(or_io::hash::string_hash_str)
        .to_vec();
    codes.sort_unstable();
    assert_eq!(dict.codes, codes, "codes stay ascending");
    for (code, entry) in dict.codes.iter().zip(&dict.entries) {
        let name = &entry.as_deref().unwrap().name;
        assert_eq!(
            *code,
            or_io::hash::string_hash_str(name),
            "{name} beside its code"
        );
    }
    let again = PgDictionary::<GrcTexture>::from_bytes(&dict.to_bytes().unwrap()).unwrap();
    assert_eq!(again.get("marble").unwrap().name, "marble");
}

/// The payload word each texture of `file` states, as a peek reads it.
fn stated(file: &[u8]) -> u32 {
    let peek = PgDictionary::<GrcTexture>::peek(file).unwrap();
    let root = peek.root().unwrap();
    let t = root.entries().unwrap().follow(0).unwrap().unwrap();
    t.physical_size_and_template_type
}

#[test]
fn a_resized_masked_run_reopens_at_its_new_length() {
    let mut dict = authored();
    let t = dict.entries[0].as_deref_mut().unwrap();
    t.name = "granite".into();
    t.backing_store.resize(2 * PIXELS, 0xEE);

    let saved = dict.to_bytes().unwrap();
    let again = PgDictionary::<GrcTexture>::from_bytes(&saved).unwrap();
    let t = only(&again);
    assert_eq!(t.name, "granite");
    assert_eq!(t.backing_store.len(), 2 * PIXELS);
    assert_eq!(
        stated(&saved) & PAYLOAD_SIZE_MASK,
        2 * PIXELS as u32,
        "the count is written from the Vec"
    );
    assert_eq!(
        stated(&saved) & !PAYLOAD_SIZE_MASK,
        TEMPLATE,
        "the template type is saved outside the mask"
    );
    assert_eq!(
        t.template_type(),
        TEMPLATE,
        "an opened texture shows the template type and not the count"
    );
    assert_eq!((t.backing_store[5], t.backing_store[PIXELS]), (5, 0xEE));
    assert_eq!(payload(&again.to_bytes().unwrap()), payload(&saved));
}

/// A dictionary built in code states every reference count as 1, as every shipped one does.
#[test]
fn an_authored_dictionary_states_a_reference_count_of_one() {
    let file = authored().to_bytes().unwrap();
    let peek = PgDictionary::<GrcTexture>::peek(&file).unwrap();
    let root = peek.root().unwrap();
    assert_eq!({ root.ref_count }, 1, "the dictionary");
    let t = root.entries().unwrap().follow(0).unwrap().unwrap();
    assert_eq!({ t.ref_count }, 1, "the texture");
}

#[test]
fn a_masked_run_its_mask_cannot_state_is_refused_by_name() {
    let mut dict = authored();
    dict.entries[0]
        .as_deref_mut()
        .unwrap()
        .backing_store
        .push(0);
    let why = dict.to_bytes().unwrap_err().to_string();
    assert!(why.contains("GrcTexture.backing_store"), "{why}");
}

#[test]
fn a_live_pixel_pointer_with_no_stated_or_implied_length_is_refused() {
    let ytd = formats::format("ytd").unwrap();
    let file = Rsc7::open(&authored().to_bytes().unwrap()).unwrap();
    let saved = Aligned::from_bytes(&file.data);
    let img = Image::new(saved.bytes(), file.virtual_size as usize);
    let dict = View::<Raw<PgDictionary<GrcTexture>>>::at_offset(img, 0).unwrap();
    let t = dict.entries().unwrap().follow(0).unwrap().unwrap();
    let at = View::offset(&t) + offset_of!(Raw<GrcTexture>, physical_size_and_template_type);
    let mut bytes = file.data.clone();
    bytes[at..at + 4].copy_from_slice(&TEMPLATE.to_le_bytes());
    // No level, so the mip chain implies nothing either.
    bytes[View::offset(&t) + offset_of!(Raw<GrcTexture>, mip_count)] = 0;

    let aligned = Aligned::from_bytes(&bytes);
    let tree = ytd
        .read(
            Generation::Gen8,
            Image::new(aligned.bytes(), file.virtual_size as usize),
        )
        .unwrap();
    let why = ytd.save(Generation::Gen8, &tree).unwrap_err().to_string();
    assert!(
        why.contains("zero-byte payload beside a live pointer"),
        "{why}"
    );
}

#[test]
fn a_live_pixel_pointer_with_no_stated_length_opens_at_its_mip_chain() {
    let mut dict = authored();
    // 16x16 DXT1 of one level: 128 bytes, a length the mask can state.
    let t = dict.entries[0].as_deref_mut().unwrap();
    (t.width, t.height) = (16, 16);
    let file = Rsc7::open(&dict.to_bytes().unwrap()).unwrap();
    let saved = Aligned::from_bytes(&file.data);
    let img = Image::new(saved.bytes(), file.virtual_size as usize);
    let view = View::<Raw<PgDictionary<GrcTexture>>>::at_offset(img, 0).unwrap();
    let t = view.entries().unwrap().follow(0).unwrap().unwrap();
    assert_eq!(
        t.physical_size_and_template_type, TEMPLATE,
        "a length the mip chain implies is not stated"
    );
    let again = PgDictionary::<GrcTexture>::from_bytes(&dict.to_bytes().unwrap()).unwrap();
    assert_eq!(again, dict);
}

/// `file` with the payload word of its first texture set to `word`, resealed under the same header.
fn with_stated(file: &[u8], word: u32) -> Vec<u8> {
    use or_io::container::resource::Compression;
    let opened = Rsc7::open(file).unwrap();
    let saved = Aligned::from_bytes(&opened.data);
    let img = Image::new(saved.bytes(), opened.virtual_size as usize);
    let view = View::<Raw<PgDictionary<GrcTexture>>>::at_offset(img, 0).unwrap();
    let t = view.entries().unwrap().follow(0).unwrap().unwrap();
    let at = View::offset(&t) + offset_of!(Raw<GrcTexture>, physical_size_and_template_type);
    let mut bytes = opened.data.clone();
    bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
    Rsc7::reseal(&bytes, &opened.seal, Compression::fast()).unwrap()
}

/// A mod tool that states more pixel bytes than its resource holds, beside dimensions whose mip chain fits.
#[test]
fn a_stated_length_past_the_resource_opens_at_the_mip_chain() {
    let mut dict = authored();
    let t = dict.entries[0].as_deref_mut().unwrap();
    (t.width, t.height) = (16, 16);
    let file = dict.to_bytes().unwrap();

    let overstated = with_stated(&file, TEMPLATE | (PAYLOAD_SIZE_MASK & 0x0100_0000));
    let again = PgDictionary::<GrcTexture>::from_bytes(&overstated).unwrap();
    assert_eq!(only(&again).backing_store, only(&dict).backing_store);
    assert_eq!(
        (only(&again).width, only(&again).height),
        (16, 16),
        "the dimensions the chain was read from"
    );
}

/// A chain the mask cannot state, beside a word that overstates it: opened at the chain, a save pads it.
#[test]
fn a_mip_chain_that_is_not_a_whole_number_of_units_is_padded_at_save() {
    let unit = (PAYLOAD_SIZE_MASK & PAYLOAD_SIZE_MASK.wrapping_neg()) as usize;
    // 8x8 DXT1 with four levels is 32 + 8 + 8 + 8 bytes.
    let chain: usize = 56;
    assert!(!chain.is_multiple_of(unit));
    let mut dict = authored();
    let t = dict.entries[0].as_deref_mut().unwrap();
    t.mip_count = 4;
    t.backing_store.truncate(chain);
    let file = dict.to_bytes().unwrap();
    assert_eq!(
        stated(&file),
        TEMPLATE,
        "a chain the dimensions imply is not stated"
    );

    let overstated = with_stated(&file, TEMPLATE | (PAYLOAD_SIZE_MASK & 0x0100_0000));
    let opened = PgDictionary::<GrcTexture>::from_bytes(&overstated).unwrap();
    assert_eq!(
        only(&opened).backing_store.len(),
        chain,
        "opened at the chain"
    );

    let saved = opened
        .to_bytes()
        .unwrap_or_else(|e| panic!("a save refused the chain: {e}"));
    let again = PgDictionary::<GrcTexture>::from_bytes(&saved).unwrap();
    let t = only(&again);
    assert_eq!(t.backing_store.len(), chain.next_multiple_of(unit));
    assert_eq!(t.backing_store[..chain], only(&dict).backing_store[..]);
    assert!(t.backing_store[chain..].iter().all(|b| *b == 0));
    assert_eq!(
        stated(&saved),
        TEMPLATE | chain.next_multiple_of(unit) as u32,
        "the word states the padded run and keeps the template type"
    );
    assert_eq!(
        payload(&again.to_bytes().unwrap()),
        payload(&saved),
        "a fixed point"
    );
}

/// Neither the stated length nor the chain fits: still refused, naming the field.
#[test]
fn a_stated_length_past_the_resource_beside_a_chain_that_does_not_fit_is_refused() {
    let mut dict = authored();
    let t = dict.entries[0].as_deref_mut().unwrap();
    (t.width, t.height) = (16, 16);
    let mut file = with_stated(&dict.to_bytes().unwrap(), TEMPLATE | 0x0100_0000);
    // A 4096x4096 chain does not fit either.
    let opened = Rsc7::open(&file).unwrap();
    let saved = Aligned::from_bytes(&opened.data);
    let img = Image::new(saved.bytes(), opened.virtual_size as usize);
    let view = View::<Raw<PgDictionary<GrcTexture>>>::at_offset(img, 0).unwrap();
    let t = view.entries().unwrap().follow(0).unwrap().unwrap();
    let at = View::offset(&t) + offset_of!(Raw<GrcTexture>, width);
    let mut bytes = opened.data.clone();
    bytes[at..at + 4].copy_from_slice(&[0x00, 0x10, 0x00, 0x10]);
    file = Rsc7::reseal(
        &bytes,
        &opened.seal,
        or_io::container::resource::Compression::fast(),
    )
    .unwrap();
    let why = PgDictionary::<GrcTexture>::from_bytes(&file)
        .unwrap_err()
        .to_string();
    assert!(why.contains("backing_store"), "{why}");
}

const BC1_BLOCK: u32 = 8;

fn enhanced_texture(name: &str) -> GrcTextureG9 {
    let mut t = GrcTextureG9 {
        name: name.into(),
        width: 8,
        height: 8,
        depth: 1,
        dimension: Open::new(TextureDimension::Texture2D),
        format: Open::new(PixelFormat::Bc1Unorm),
        tile_mode: Open::new(TileMode::Auto),
        mip_levels: 1,
        pixels: (0..PIXELS).map(|i| i as u8).collect(),
        view: ShaderResourceViewG9 {
            view_dimension: 0x41,
            ..Default::default()
        },
        ..Default::default()
    };
    t.set_block_stride(BC1_BLOCK);
    t
}

fn enhanced() -> PgDictionary<GrcTextureG9> {
    PgDictionary {
        codes: vec![or_io::hash::string_hash_str("stone")],
        entries: vec![Some(Box::new(enhanced_texture("stone")))],
        ..Default::default()
    }
}

#[test]
fn an_authored_enhanced_ytd_points_at_its_own_view_and_reaches_a_fixed_point() {
    let first = enhanced().to_bytes().unwrap();
    let ytd = formats::format("ytd").unwrap();
    let version = Rsc7::open(&first).unwrap().version;
    assert_eq!(ytd.target_of_version(version).unwrap(), Generation::Gen9);

    let peek = PgDictionary::<GrcTextureG9>::peek(&first).unwrap();
    let root = peek.root().unwrap();
    let t = root.entries().unwrap().follow(0).unwrap().unwrap();
    let view = t.view_ptr().unwrap().expect("a view pointer");
    assert_eq!(
        View::offset(&view),
        View::offset(&t) + offset_of!(Raw<GrcTextureG9>, view),
        "a texture built in code points at its own view"
    );
    assert_eq!(
        { t.block_count },
        PIXELS as u32 / BC1_BLOCK,
        "the count is written from the Vec"
    );
    assert_eq!({ t.ref_count }, 1);

    let again = PgDictionary::<GrcTextureG9>::from_bytes(&first).unwrap();
    assert_eq!(again, enhanced());
    let t = again.entries[0].as_deref().expect("one texture");
    assert_eq!(t.block_stride(), BC1_BLOCK);
    assert_eq!(t.view.view_dimension, 0x41);
    assert_eq!(payload(&again.to_bytes().unwrap()), payload(&first));
}

#[test]
fn an_enhanced_payload_that_is_not_whole_blocks_is_refused_by_name() {
    let mut dict = enhanced();
    dict.entries[0].as_deref_mut().unwrap().pixels.push(0);
    let why = dict.to_bytes().unwrap_err().to_string();
    assert!(why.contains("GrcTextureG9.pixels"), "{why}");
}

#[test]
fn or_io_opens_an_enhanced_ytd_as_its_gen9_variant() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("owned_texture_dict");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("stone.ytd");
    enhanced().save_as(&path).unwrap();
    let Asset::TextureDictGen9(mut dict) = or_io::open(&path).unwrap() else {
        panic!("not an Enhanced texture dictionary");
    };
    assert_eq!(dict, enhanced());
    dict.entries[0].as_deref_mut().unwrap().width = 16;
    dict.save().unwrap();
    let again = PgDictionary::<GrcTextureG9>::open(&path).unwrap();
    assert_eq!(again.entries[0].as_deref().unwrap().width, 16);
    assert!(matches!(
        or_io::from_bytes(&std::fs::read(&path).unwrap()).unwrap(),
        Asset::TextureDictGen9(_)
    ));
}

#[test]
fn a_version_nothing_declares_is_named() {
    let ytd = formats::format("ytd").unwrap();
    let why = ytd.target_of_version(2).unwrap_err().to_string();
    assert!(why.contains("version 2"), "{why}");
    assert_eq!(ytd.target_of_version(13).unwrap(), Generation::Gen8);
    assert_eq!(ytd.target_of_version(5).unwrap(), Generation::Gen9);
}

/// What a Legacy texture states as its `mip_stride`, as every shipped one does: a block-compressed
/// format's bytes per row of texels, a quarter of its row of blocks, and any other format's row.
fn shipped_stride(format: or_io::codec::texture::GrcTextureFormat, width: u32) -> u32 {
    let row = format.row_pitch(width);
    if format.is_compressed() {
        row / 4
    } else {
        row
    }
}

#[test]
fn an_import_states_the_stride_a_shipped_file_does() {
    use or_io::authoring::TextureBuildOptions;
    use or_io::codec::texture::{save_png, GrcTextureFormat as F, RgbaImage};

    // Wider than tall, so a width read as a height shows.
    let mut image = RgbaImage::new(64, 32);
    for (i, byte) in image.data.iter_mut().enumerate() {
        *byte = (i * 7 + i / 255) as u8;
    }
    let mut formats = vec![
        F::DXT1,
        F::DXT3,
        F::DXT5,
        F::DXT5A,
        F::DXN,
        F::A8R8G8B8,
        F::L8,
        F::A8,
    ];
    // BC7 has an encoder only where this crate was built with `bc7-encode`.
    if cfg!(feature = "bc7-encode") {
        formats.push(F::BC7);
    }
    for format in formats {
        let mut texture = GrcTexture::default();
        texture
            .set_pixels(format, &image, &TextureBuildOptions::default())
            .unwrap();
        assert_eq!(
            u32::from(texture.mip_stride),
            shipped_stride(format, 64),
            "set_pixels, {format:?}"
        );
    }

    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("stone.png");
    save_png(&path, &image).unwrap();
    let texture = GrcTexture::from_png(&path).unwrap();
    assert_eq!(
        u32::from(texture.mip_stride),
        shipped_stride(F::from(texture.format), 64),
        "from_png"
    );
}
