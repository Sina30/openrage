//! `.ytd` through its declarations, over the corpus and for both targets:
//! typed reads against the tree, the owned form against the tree's save, the
//! recorded testimony re-keyed by offset, the canonical fixed point, the
//! alignment census, and the observations that name fields.

use std::mem::{offset_of, size_of};
use std::path::PathBuf;

use or_io::decl::{Described, File, Raw, Record, View};
use or_io::desc::{Image, Pack};
use or_io::gta5::shared::PgDictionary;
use or_io::gta5::texture_dict::{gen8, gen9};
use or_io::Generation;
use or_io_verify::gates::{
    self, files, name, offset_of_run, FieldMoved, Filed, Gates, Opened, Relocated,
};

const EXT: &str = "ytd";
const TARGETS: [Generation; 2] = [Generation::Gen8, Generation::Gen9];

fn ytd_files(target: Generation, what: &str) -> Option<Vec<PathBuf>> {
    files(EXT, Some(target), what)
}

fn bytes_of<R: Record>(view: &View<'_, R>) -> Vec<u8> {
    or_io::decl::Plain::as_bytes(View::get(view)).to_vec()
}

/// What the typed API reads of a Legacy dictionary, in the shape of [`Filed`].
fn typed_legacy(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: or_io::decl::ViewError| e.to_string();
    let mut out = Filed::default();
    let dict = View::<Raw<PgDictionary<gen8::GrcTexture>>>::at_offset(image, 0).map_err(e)?;
    out.structs
        .insert((0, "PgDictionary<GrcTexture>".to_string()), bytes_of(&dict));
    let codes = dict.codes().map_err(e)?;
    if let Some(at) = offset_of_run(image, &codes) {
        out.runs
            .insert(at, codes.iter().flat_map(|c| c.to_le_bytes()).collect());
    }
    let entries = dict.entries().map_err(e)?;
    for i in 0..entries.len() {
        let Some(texture) = entries.follow(i).map_err(e)? else {
            continue;
        };
        out.structs.insert(
            (View::offset(&texture), "GrcTexture".to_string()),
            bytes_of(&texture),
        );
        if let Ok(run) = texture.backing_store() {
            if let Some(at) = offset_of_run(image, run.as_slice()) {
                out.runs.insert(at, run.as_slice().to_vec());
            }
        }
    }
    Ok(out)
}

/// What the typed API reads of an Enhanced dictionary, in the shape of [`Filed`].
fn typed_enhanced(image: Image<'_>) -> Result<Filed, String> {
    let e = |e: or_io::decl::ViewError| e.to_string();
    let mut out = Filed::default();
    let dict = View::<Raw<PgDictionary<gen9::GrcTextureG9>>>::at_offset(image, 0).map_err(e)?;
    out.structs.insert(
        (0, <Raw<PgDictionary<gen9::GrcTextureG9>>>::name()),
        bytes_of(&dict),
    );
    let codes = dict.codes().map_err(e)?;
    if let Some(at) = offset_of_run(image, &codes) {
        out.runs
            .insert(at, codes.iter().flat_map(|c| c.to_le_bytes()).collect());
    }
    let entries = dict.entries().map_err(e)?;
    for i in 0..entries.len() {
        let Some(texture) = entries.follow(i).map_err(e)? else {
            continue;
        };
        out.structs.insert(
            (View::offset(&texture), <Raw<gen9::GrcTextureG9>>::name()),
            bytes_of(&texture),
        );
        if let Ok(run) = texture.pixels() {
            if let Some(at) = offset_of_run(image, run.as_slice()) {
                out.runs.insert(at, run.as_slice().to_vec());
            }
        }
    }
    Ok(out)
}

const LEGACY: Gates = Gates {
    census_types: 4,
    relocated: RELOCATED,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen8, ("GrcTexture", "width"))
};

const ENHANCED: Gates = Gates {
    census_types: 4,
    relocated: RELOCATED,
    moved: MOVED,
    ..Gates::new(EXT, Generation::Gen9, ("GrcTextureG9", "width"))
};

or_io_verify::declared_gates! {
    gates: LEGACY,
    typed: typed_legacy,
    reads_typed: every_legacy_ytd_reads_typed_as_the_tree_reads,
    owned: PgDictionary<gen8::GrcTexture> => every_legacy_ytd_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_legacy_ytd_saves_to_a_fixed_point,
    census: every_legacy_ytd_instance_is_aligned_for_its_view,
    testimony: the_legacy_ytd_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_legacy_field_one_byte_late_is_named_by_the_testimony,
}

or_io_verify::declared_gates! {
    gates: ENHANCED,
    typed: typed_enhanced,
    reads_typed: every_enhanced_ytd_reads_typed_as_the_tree_reads,
    owned: PgDictionary<gen9::GrcTextureG9> => every_enhanced_ytd_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_enhanced_ytd_saves_to_a_fixed_point,
    census: every_enhanced_ytd_instance_is_aligned_for_its_view,
    testimony: the_enhanced_ytd_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_enhanced_field_one_byte_late_is_named_by_the_testimony,
}

/// Each texture's view pointer, as a peek reads it: how many textures, and how many land on their own view.
fn own_views(image: Image<'_>) -> (usize, usize) {
    let dict = View::<Raw<PgDictionary<gen9::GrcTextureG9>>>::at_offset(image, 0).expect("root");
    let entries = dict.entries().expect("entries");
    let (mut textures, mut own) = (0, 0);
    for t in (0..entries.len()).filter_map(|i| entries.follow(i).expect("reads")) {
        textures += 1;
        let view = t.view_ptr().expect("a view pointer lands on a member");
        own += usize::from(view.is_some_and(|v| {
            View::offset(&v) == View::offset(&t) + offset_of!(Raw<gen9::GrcTextureG9>, view)
        }));
    }
    (textures, own)
}

#[test]
fn every_enhanced_view_pointer_lands_on_its_own_view_before_and_after_a_save() {
    let Some(paths) = ytd_files(Generation::Gen9, "an Enhanced texture's view") else {
        return;
    };
    let (mut textures, mut shipped, mut saved) = (0usize, 0usize, 0usize);
    for path in &paths {
        let file = std::fs::read(path).expect("a corpus file reads");
        let (n, own) = own_views(Opened::of(&file).image());
        let owned = PgDictionary::<gen9::GrcTextureG9>::from_bytes(&file).expect("opens owned");
        let (again, own_again) = own_views(Opened::of(&owned.to_bytes().expect("saves")).image());
        assert_eq!(again, n, "{}: the save kept every texture", name(path));
        textures += n;
        shipped += own;
        saved += own_again;
    }
    println!(
        "{textures} Enhanced textures: {shipped} shipped and {saved} saved owned point at their \
         own view"
    );
    assert!(textures > 0);
    assert_eq!(shipped, textures);
    assert_eq!(saved, textures);
}

/// Bytes a mip chain of `levels` occupies at `(w, h, d)`, for a format of
/// `block` bytes per `dim`×`dim` block. The one quantity a stated length does
/// not take part in, which is what makes it evidence about the length.
fn mip_chain(w: u16, h: u16, d: u16, levels: u8, block: usize, dim: usize) -> usize {
    let (mut w, mut h) = (usize::from(w).max(1), usize::from(h).max(1));
    let mut total = 0;
    for _ in 0..levels.max(1) {
        total += w.div_ceil(dim) * h.div_ceil(dim) * block;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    total * usize::from(d).max(1)
}

/// Block bytes and size for a Legacy D3D9 format word: FourCCs and the few plain formats shipped.
fn legacy_block(format: u32) -> Option<(usize, usize)> {
    match &format.to_le_bytes() {
        b"DXT1" | b"ATI1" => Some((8, 4)),
        b"DXT3" | b"DXT5" | b"ATI2" | b"BC7 " => Some((16, 4)),
        _ => match format {
            21 | 22 => Some((4, 1)),
            23 | 25 | 26 => Some((2, 1)),
            28 | 50 => Some((1, 1)),
            _ => None,
        },
    }
}

/// Block bytes and size for an Enhanced DXGI format.
fn enhanced_block(format: gen9::PixelFormat) -> Option<(usize, usize)> {
    use gen9::PixelFormat::*;
    Some(match format {
        Bc1Unorm | Bc1UnormSrgb | Bc4Unorm => (8, 4),
        Bc2Unorm | Bc2UnormSrgb | Bc3Unorm | Bc3UnormSrgb | Bc5Unorm | Bc6hUf16 | Bc6hSf16
        | Bc7Unorm | Bc7UnormSrgb => (16, 4),
        R8G8B8A8Unorm | R8G8B8A8UnormSrgb | B8G8R8A8Unorm | B8G8R8A8UnormSrgb => (4, 1),
        R16Unorm | B5G6R5Unorm | B5G5R5A1Unorm => (2, 1),
        R8Unorm | A8Unorm => (1, 1),
        _ => return None,
    })
}

/// ⚠ **Measured against the mip chain, never against the pixels read**: a
/// reader that sizes the payload by the word reproduces the word by
/// construction, which proves nothing about it.
#[test]
fn the_physical_size_is_carried_because_it_cannot_be_derived() {
    let Some(paths) = ytd_files(Generation::Gen8, "the Legacy payload length census") else {
        return;
    };
    let (mut textures, mut rounded, mut zero, mut other, mut unknown) = (0, 0, 0, 0, 0);
    let (mut holds, mut short) = (0usize, 0usize);
    for path in &paths {
        let opened = Opened::read(path);
        let dict = View::<Raw<PgDictionary<gen8::GrcTexture>>>::at_offset(opened.image(), 0)
            .expect("root");
        let entries = dict.entries().expect("entries");
        for t in (0..entries.len()).filter_map(|i| entries.follow(i).expect("reads")) {
            textures += 1;
            let Some((block, dim)) = legacy_block(t.format) else {
                unknown += 1;
                continue;
            };
            let chain = mip_chain(t.width, t.height, t.depth, t.mip_count, block, dim);
            let stated = (t.physical_size_and_template_type & gen8::PAYLOAD_SIZE_MASK) as usize;
            if stated != 0 {
                if stated >= chain {
                    holds += 1;
                } else {
                    short += 1;
                }
            }
            match stated {
                0 => zero += 1,
                n if n == chain.next_multiple_of(128) => rounded += 1,
                _ => other += 1,
            }
        }
    }
    println!(
        "{textures} Legacy textures: {holds} state a length that holds their mip chain and \
         {short} one that does not; {rounded} are exactly the chain rounded up to 128 bytes and \
         {other} are padded otherwise, so the word is carried rather than derived; {zero} state \
         zero, {unknown} in a format this table does not size"
    );
    assert!(textures > 0);
    assert_eq!(zero, 1, "the one known zero is server_assets.ytd's");
    assert_eq!(short, 0, "a stated length shorter than its mip chain");
    assert_eq!(holds + zero + unknown, textures);
}

#[test]
fn whether_an_enhanced_payload_length_is_stated_or_computed() {
    let Some(paths) = ytd_files(Generation::Gen9, "the Enhanced payload length census") else {
        return;
    };
    let (mut textures, mut product, mut chain_equal, mut unknown) = (0, 0, 0, 0);
    for path in &paths {
        let opened = Opened::read(path);
        let image = opened.image();
        let dict =
            View::<Raw<PgDictionary<gen9::GrcTextureG9>>>::at_offset(image, 0).expect("root");
        let entries = dict.entries().expect("entries");
        for t in (0..entries.len()).filter_map(|i| entries.follow(i).expect("reads")) {
            textures += 1;
            let stated = t.block_count as usize * t.block_stride as usize;
            // The pixels run until the next payload or the end of the graphics pages.
            let pixels = t.pixels().expect("the payload is where it says");
            product += usize::from(pixels.len() == stated);
            match enhanced_block(t.format.get()) {
                Some((block, dim)) => {
                    let chain = mip_chain(t.width, t.height, t.depth, t.mip_levels, block, dim);
                    chain_equal += usize::from(chain == stated);
                }
                None => unknown += 1,
            }
        }
    }
    println!(
        "{textures} Enhanced textures: block_count × block_stride is the payload read for \
         {product}, and equals the mip chain from format and size for {chain_equal} \
         ({unknown} in a format this table does not size)"
    );
    assert!(textures > 0);
    assert_eq!(product, textures);
}

#[inline(never)]
fn typed_legacy_sum(image: Image<'_>) -> u64 {
    let dict = View::<Raw<PgDictionary<gen8::GrcTexture>>>::at_offset(image, 0).expect("root");
    let entries = dict.entries().expect("entries");
    (0..entries.len())
        .filter_map(|i| entries.follow(i).ok().flatten())
        .map(|t| {
            u64::from(t.width)
                + u64::from(t.height)
                + u64::from(t.mip_count)
                + u64::from(t.format)
                + t.backing_store().map_or(0, |p| p.len() as u64)
        })
        .sum()
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_ytd_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(paths) = ytd_files(Generation::Gen8, "the .ytd read cost") else {
        return;
    };
    let opened: Vec<Opened> = paths.iter().map(|p| Opened::read(p)).collect();
    let textures: usize = opened
        .iter()
        .map(|o| {
            View::<Raw<PgDictionary<gen8::GrcTexture>>>::at_offset(o.image(), 0)
                .and_then(|d| d.entries().map(|e| e.len()))
                .unwrap_or(0)
        })
        .sum();
    let best = |rounds: usize, f: &mut dyn FnMut() -> u64| {
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let mut sink = 0u64;
            for _ in 0..rounds {
                sink = sink.wrapping_add(f());
            }
            black_box(sink);
            best = best.min(start.elapsed());
        }
        best.as_secs_f64() * 1e9 / (rounds * textures) as f64
    };
    let typed_ns = best(2_000, &mut || {
        opened
            .iter()
            .map(|o| typed_legacy_sum(black_box(o.image())))
            .sum()
    });
    let tree_ns = best(3, &mut || {
        opened
            .iter()
            .map(|o| {
                let tree = gates::format(EXT)
                    .read(Generation::Gen8, o.image())
                    .expect("reads");
                u64::from(tree.record().is_some())
            })
            .sum()
    });
    println!(
        "{} Legacy files, {textures} textures: typed from the root {typed_ns:.1} ns a texture \
         (five fields and the pixel run); whole tree read {tree_ns:.0} ns a texture ({:.0}x), \
         which copies every payload",
        paths.len(),
        tree_ns / typed_ns
    );
}

fn builtin(target: Generation) -> Option<Pack> {
    Some((*gates::pack(EXT, target)).clone())
}

/// ⚠ Both records read the Enhanced view as a field at 0x50 — the older
/// through the compiled layout of the day, the newer through a description
/// that has since moved it — and the view is where `offset_of!` puts it.
/// Answered from 0x50, so the recorded bytes are still held to the file where
/// they were read.
const MOVED: &[FieldMoved] = &[FieldMoved {
    target: "gen9",
    inside: "GrcTextureG9",
    now: offset_of!(Raw<gen9::GrcTextureG9>, view),
    was: 0x50,
    len: size_of::<Raw<gen9::ShaderResourceViewG9>>(),
}];

/// ⚠ The older record read an Enhanced texture's view where the description
/// of the day put it, eight bytes early. Only that record, only there.
const RELOCATED: &[Relocated] = &[Relocated {
    record: "corpus-cda05819",
    inside: "GrcTextureG9",
    delta: 0x50,
    because: "the view is embedded at 0x58: every `view_ptr` is its texture + 0x58, and \
              CodeWalker writes it there. The recorder took 0x50 from the description of the day",
}];

/// A frozen key row one byte late is named, for each target's pack-witnessed textures.
#[test]
fn a_frozen_key_row_one_byte_late_is_named_by_the_testimony() {
    let Some(mut key) = gates::frozen_key() else {
        or_test_support::skip_corpus("a moved frozen key row", "no frozen key");
        return;
    };
    for target in TARGETS {
        let row = (
            target.slug().to_string(),
            "GrcTexture".to_string(),
            "width".to_string(),
        );
        key.rows.get_mut(&row).expect("a frozen row").offset += 1;
    }
    let Some(rows) = gates::check_against_keyed(
        EXT,
        &builtin,
        &key,
        RELOCATED,
        &[],
        MOVED,
        "the .ytd testimony through a moved frozen key",
    ) else {
        return;
    };
    for (label, t) in rows
        .iter()
        .filter(|(label, _)| label.starts_with("corpus/"))
    {
        let caught = t.differ.len() + t.unanswered;
        println!("{label}: {caught} caught with the frozen `GrcTexture.width` one byte late");
        assert!(caught > 0, "{label} did not notice");
    }
}

/// Every Enhanced texture the corpus holds, wherever its format files one: `(file, offset)`.
fn every_enhanced_texture(what: &str) -> Option<Vec<(Opened, Vec<usize>)>> {
    let names = [<Raw<gen9::GrcTextureG9>>::name()];
    let mut out = Vec::new();
    for ext in ["ytd", "ydr", "ydd", "yft", "ypt"] {
        let Some(paths) = files(ext, Some(Generation::Gen9), what) else {
            continue;
        };
        for path in &paths {
            let opened = Opened::read(path);
            let tree = gates::format(ext)
                .read(Generation::Gen9, opened.image())
                .expect("reads");
            let mut filed = Filed::default();
            gates::file_tree(opened.image(), &tree, Some(0), &mut filed);
            let at = filed
                .structs
                .keys()
                .filter(|(_, n)| names.contains(n))
                .map(|(at, _)| *at)
                .collect();
            out.push((opened, at));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The values each carried word's note states, held to every Enhanced texture.
#[test]
fn the_carried_enhanced_texture_words_hold_what_their_notes_state() {
    let Some(opened) = every_enhanced_texture("the carried Enhanced texture words") else {
        return;
    };
    let mut textures = 0;
    let mut after_mip_levels = std::collections::BTreeMap::<u8, usize>::new();
    let mut alike = std::collections::BTreeMap::<_, std::collections::BTreeSet<u8>>::new();
    for (file, at) in &opened {
        for &at in at {
            let t = View::<Raw<gen9::GrcTextureG9>>::at_offset(file.image(), at).expect("reads");
            let v = &t.view;
            textures += 1;
            let block = enhanced_block(t.format.get()).map(|(b, _)| b as u32);
            assert_eq!(Some(t.block_stride), block, "block_stride at {at:#x}");
            assert_eq!({ t.after_flags }, 0);
            assert_eq!({ t.second_after_mip_levels }, 0);
            assert_eq!({ t.third_after_mip_levels }, 0);
            assert_eq!({ t.ref_count }, 1);
            assert_eq!({ t.after_usage_data }, 2);
            assert_eq!({ t.second_after_usage_data }, 0);
            assert_eq!({ t.third_after_usage_data }, 0);
            assert_eq!({ t.after_view }, 0);
            assert!(t.dimension == gen9::TextureDimension::Texture2D);
            assert_eq!({ v.after_vft }, 0);
            assert_eq!({ v.view_dimension }, 0x41);
            assert_eq!({ v.after_view_dimension }, 0xFFFF);
            assert_eq!({ v.second_after_view_dimension }, u32::MAX);
            assert_eq!({ v.third_after_view_dimension }, 0);
            let byte = { t.after_mip_levels };
            assert!(matches!(byte, 0 | 0x28), "after_mip_levels {byte:#x}");
            *after_mip_levels.entry(byte).or_default() += 1;
            let key = (
                t.format.raw(),
                ({ t.width }, { t.height }, { t.depth }),
                ({ t.mip_levels }, { t.flags }, { t.usage_data }),
            );
            alike.entry(key).or_default().insert(byte);
        }
    }
    let split = alike.values().filter(|bytes| bytes.len() > 1).count();
    println!(
        "{textures} Enhanced textures in {} files: after_mip_levels is {}; {split} sets alike \
         in format, size, mip levels, flags and usage_data hold both values",
        opened.len(),
        after_mip_levels
            .iter()
            .map(|(byte, n)| format!("{byte:#x} in {n}"))
            .collect::<Vec<_>>()
            .join(" and ")
    );
    assert!(textures > 0);
    assert!(
        split > 0,
        "after_mip_levels follows format, size, mip levels, flags and usage_data"
    );
}

/// ⚠ Measured on the stated word of every shipped Legacy texture, never on what an import writes.
#[test]
fn every_shipped_compressed_texture_states_its_stride_per_row_of_texels() {
    use or_io::codec::texture::GrcTextureFormat;

    let Some(paths) = ytd_files(Generation::Gen8, "the Legacy mip_stride census") else {
        return;
    };
    let (mut compressed, mut texel_rows, mut block_rows) = (0usize, 0usize, 0usize);
    let (mut plain, mut plain_rows) = (0usize, 0usize);
    let mut neither = Vec::new();
    for path in &paths {
        let opened = Opened::read(path);
        let dict = View::<Raw<PgDictionary<gen8::GrcTexture>>>::at_offset(opened.image(), 0)
            .expect("root");
        let entries = dict.entries().expect("entries");
        for t in (0..entries.len()).filter_map(|i| entries.follow(i).expect("reads")) {
            let format = GrcTextureFormat::from(t.format);
            if format.bytes_per_block() == 0 {
                continue;
            }
            let row = format.row_pitch(u32::from(t.width));
            let stated = u32::from(t.mip_stride);
            if format.is_compressed() {
                compressed += 1;
                if stated == row / 4 {
                    texel_rows += 1;
                } else if stated == row {
                    block_rows += 1;
                } else {
                    neither.push(format!("{format:?} {}x{}: {stated}", t.width, t.height));
                }
            } else {
                plain += 1;
                if stated == row {
                    plain_rows += 1;
                } else {
                    neither.push(format!("{format:?} {}x{}: {stated}", t.width, t.height));
                }
            }
        }
    }
    println!(
        "{compressed} compressed Legacy textures: {texel_rows} state a quarter of their row of \
         blocks (a row of texels) and {block_rows} state the row of blocks; {plain} other formats: \
         {plain_rows} state their row; {} state neither: {neither:?}",
        neither.len()
    );
    assert!(compressed > 0);
    assert_eq!(block_rows, 0, "a texture states a row of blocks");
    assert!(neither.is_empty(), "{neither:?}");
    assert_eq!(texel_rows, compressed);
    assert_eq!(plain_rows, plain);
}

/// How the row slope of red and the column slope of green move together over the interior of a
/// normal map, as a correlation. A normal map made from a height field has a gradient with no curl,
/// so the two slopes are the same height's mixed second derivative: they move together when green
/// points down the image (the DirectX convention) and against each other when it points up (OpenGL).
fn red_green_curl(rgba: &[u8], width: usize, height: usize) -> f64 {
    let at = |channel: usize, x: usize, y: usize| f64::from(rgba[(y * width + x) * 4 + channel]);
    let (mut red_down, mut green_across) = (Vec::new(), Vec::new());
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            red_down.push(at(0, x, y + 1) - at(0, x, y - 1));
            green_across.push(at(1, x + 1, y) - at(1, x - 1, y));
        }
    }
    let n = red_down.len() as f64;
    let (mean_r, mean_g) = (
        red_down.iter().sum::<f64>() / n,
        green_across.iter().sum::<f64>() / n,
    );
    let (mut rg, mut rr, mut gg) = (0.0, 0.0, 0.0);
    for (r, g) in red_down.iter().zip(&green_across) {
        rg += (r - mean_r) * (g - mean_g);
        rr += (r - mean_r) * (r - mean_r);
        gg += (g - mean_g) * (g - mean_g);
    }
    if rr == 0.0 || gg == 0.0 {
        return 0.0;
    }
    rg / (rr.sqrt() * gg.sqrt())
}

/// ⚠ The sign says which way green points only where the map is a height field's: the textures
/// whose slopes barely correlate (below 0.2 either way) say nothing and are left out of the count.
#[test]
fn the_green_channel_of_shipped_normal_maps_points_down_the_image() {
    use or_io::codec::texture::decode_level;
    use or_io::gta5::gen8::GrcTexture;

    let Some(paths) = ytd_files(Generation::Gen8, "the Legacy normal map convention") else {
        return;
    };
    let (mut named, mut down, mut up, mut silent) = (0usize, 0usize, 0usize, 0usize);
    let (mut blue_flat, mut up_names) = (0usize, Vec::new());
    for path in &paths {
        let Ok(dictionary) =
            PgDictionary::<GrcTexture>::from_bytes(&std::fs::read(path).expect("reads"))
        else {
            continue;
        };
        for texture in dictionary.entries.iter().flatten() {
            let lower = texture.name.to_ascii_lowercase();
            if !(lower.ends_with("_n") || lower.ends_with("_normal")) {
                continue;
            }
            let Ok(image) = decode_level(&**texture, 0) else {
                continue;
            };
            let (width, height) = (image.width as usize, image.height as usize);
            if width < 8 || height < 8 {
                continue;
            }
            named += 1;
            blue_flat += usize::from(image.data.as_chunks::<4>().0.iter().all(|p| p[2] >= 240));
            let curl = red_green_curl(&image.data, width, height);
            if curl >= 0.2 {
                down += 1;
            } else if curl <= -0.2 {
                up += 1;
                up_names.push(format!("{}:{}", name(path), texture.name));
            } else {
                silent += 1;
            }
        }
    }
    println!(
        "{named} Legacy normal textures (named _n or _normal): green points down in {down}, up in \
         {up}, and {silent} are too noisy to say; blue is at least 240 everywhere in {blue_flat}; \
         up: {up_names:?}"
    );
    assert!(named > 0);
    assert!(down > 4 * up, "{down} point down and {up} up: {up_names:?}");
}

/// The install testimony once set this user's mod aside because its texture states 0x2b400 payload
/// bytes and its pages ship 0x20000; the declared read takes it, as the install suite now asks.
const SHORT_PAGES_MOD: &str = "mp_eye_colour.ytd";

#[test]
fn a_mod_texture_that_states_more_payload_than_its_pages_ship_reads_through_the_declarations() {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    use or_io::decl::Aligned;
    use or_io::desc::TypeName;

    let what = format!("the staged mod {SHORT_PAGES_MOD}");
    let mods = or_test_support::mods_corpus_origin(env!("CARGO_MANIFEST_DIR"));
    let staged = (or_test_support::Corpus::with_root(mods.path(), EXT).all_targets())
        .find()
        .into_iter()
        .find(|p| name(p) == SHORT_PAGES_MOD);
    let Some(path) = staged else {
        or_test_support::skip_corpus(&what, format!("not staged in {}", mods.describe()));
        return;
    };
    let bytes = std::fs::read(&path).expect("a corpus file reads");
    let opened = Rsc7::open(&bytes).expect("an RSC7 file");
    let pages = Aligned::from_bytes(&opened.data);
    let image = Image::new(pages.bytes(), opened.virtual_size as usize);
    let root = TypeName(or_io::decl::intern(
        &gates::format(EXT)
            .root_name(Generation::Gen8)
            .expect("declared"),
    ));
    let pack = gates::pack(EXT, Generation::Gen8);
    or_io::desc::value::read(&pack, root, image, 0)
        .unwrap_or_else(|e| panic!("{SHORT_PAGES_MOD} is refused by the declared read: {e}"));
}
