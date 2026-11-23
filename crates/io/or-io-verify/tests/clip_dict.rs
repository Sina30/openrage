//! `.ycd` through its declarations, over the corpus: typed reads against the
//! tree, the owned form against the tree, the hash maps' chains, the recorded
//! testimony re-keyed by offset, the canonical fixed point, the alignment
//! census, and what an animation's blocks state about its frames.

use or_io::decl::{File, Raw, Record, View};
use or_io::desc::{FieldRepr, Image, Value};
use or_io::gta5::clip_dict::{entries, validate};
use or_io::gta5::*;
use or_io::Generation;
use or_io_verify::gates::{self, files, name, offset_of_run, Filed, Gates, Opened};

const EXT: &str = "ycd";
const T: Generation = Generation::Gen8;

type E = or_io::decl::ViewError;

fn bytes_of<R: Record>(view: &View<'_, R>) -> Vec<u8> {
    or_io::decl::Plain::as_bytes(View::get(view)).to_vec()
}

fn put<R: Record>(out: &mut Filed, view: &View<'_, R>, ty: &str) {
    out.structs
        .insert((View::offset(view), ty.to_string()), bytes_of(view));
}

fn property(p: &View<'_, Raw<CrProperty>>, out: &mut Filed) -> Result<(), E> {
    let attributes = p.attributes()?;
    for i in 0..attributes.len() {
        let Some(a) = attributes.follow(i)? else {
            continue;
        };
        if let Some(v) = View::variant::<Raw<CrPropertyAttributeFloat>>(&a)? {
            put(out, &v, "CrPropertyAttributeFloat");
        } else if let Some(v) = View::variant::<Raw<CrPropertyAttributeInt>>(&a)? {
            put(out, &v, "CrPropertyAttributeInt");
        } else if let Some(v) = View::variant::<Raw<CrPropertyAttributeBool>>(&a)? {
            put(out, &v, "CrPropertyAttributeBool");
        } else if let Some(v) = View::variant::<Raw<CrPropertyAttributeVector3>>(&a)? {
            put(out, &v, "CrPropertyAttributeVector3");
        } else if let Some(v) = View::variant::<Raw<CrPropertyAttributeQuaternion>>(&a)? {
            put(out, &v, "CrPropertyAttributeQuaternion");
        } else if let Some(v) = View::variant::<Raw<CrPropertyAttributeHashString>>(&a)? {
            put(out, &v, "CrPropertyAttributeHashString");
        }
    }
    Ok(())
}

fn animation(a: &View<'_, Raw<CrAnimation>>, image: Image<'_>, out: &mut Filed) -> Result<(), E> {
    put(out, a, "CrAnimation");
    for d in a.dofs()?.views() {
        put(out, &d, "Dof");
    }
    let blocks = a.blocks()?;
    for i in 0..blocks.len() {
        if let Some(b) = blocks.follow(i)? {
            put(out, &b, "CrBlockStream");
            let data = b.data()?;
            if let Some(at) = offset_of_run(image, &data) {
                out.runs.insert(at, data.to_vec());
            }
        }
    }
    Ok(())
}

/// What the typed API reads of a file, in the same shape as [`Filed`].
fn typed(image: Image<'_>) -> Result<Filed, String> {
    typed_walk(image).map_err(|e| e.to_string())
}

fn typed_walk(image: Image<'_>) -> Result<Filed, E> {
    let mut out = Filed::default();
    let dict = View::<Raw<CrClipDictionary>>::at_offset(image, 0)?;
    put(&mut out, &dict, "CrClipDictionary");
    if let Some(anims) = dict.anim_dictionary()? {
        put(&mut out, &anims, "CrAnimDictionary");
        for (_, entry) in entries(&anims.animations())? {
            put(&mut out, &entry, "AtMapEntry<CrAnimation>");
            if let Some(a) = entry.data()? {
                animation(&a, image, &mut out)?;
            }
        }
    }
    for (_, entry) in entries(&dict.clips())? {
        put(&mut out, &entry, "AtMapEntry<CrClip>");
        let Some(clip) = entry.data()? else {
            continue;
        };
        if let Some(v) = View::variant::<Raw<CrClipAnimation>>(&clip)? {
            put(&mut out, &v, "CrClipAnimation");
        } else if let Some(v) = View::variant::<Raw<CrClipAnimations>>(&clip)? {
            put(&mut out, &v, "CrClipAnimations");
            for a in v.animations()?.views() {
                put(&mut out, &a, "Animation");
            }
        }
        if let Some(tags) = clip.tags()? {
            put(&mut out, &tags, "CrTags");
            let run = tags.tags()?;
            for i in 0..run.len() {
                if let Some(tag) = run.follow(i)? {
                    put(&mut out, &tag, "CrTag");
                    property(&tag.base(), &mut out)?;
                }
            }
        }
        if let Some(properties) = clip.properties()? {
            put(&mut out, &properties, "CrProperties");
            for (_, entry) in entries(&properties.properties())? {
                put(&mut out, &entry, "AtMapEntry<CrProperty>");
                if let Some(p) = entry.data()? {
                    put(&mut out, &p, "CrProperty");
                    property(&p, &mut out)?;
                }
            }
        }
    }
    Ok(out)
}

const GATES: Gates = Gates {
    census_types: 30,
    ..Gates::new(EXT, T, ("Dof", "track"))
};

or_io_verify::declared_gates! {
    gates: GATES,
    typed: typed,
    reads_typed: every_ycd_reads_typed_as_the_tree_reads,
    owned: CrClipDictionary => every_ycd_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_ycd_saves_to_a_fixed_point,
    census: every_ycd_instance_is_aligned_for_its_view,
    testimony: the_ycd_testimony_holds_through_its_declarations,
    one_byte_late: a_declared_field_one_byte_late_is_named_by_the_testimony,
}

fn ycd_files(what: &str) -> Option<Vec<std::path::PathBuf>> {
    files(EXT, Some(T), what)
}

fn opened_owned(what: &str) -> Option<Vec<(String, CrClipDictionary)>> {
    let paths = ycd_files(what)?;
    Some(
        paths
            .iter()
            .map(|p| {
                let bytes = std::fs::read(p).expect("reads");
                let dict = CrClipDictionary::from_bytes(&bytes)
                    .unwrap_or_else(|e| panic!("{}: {e}", name(p)));
                (name(p), dict)
            })
            .collect(),
    )
}

/// Every map's entry count against its chains, and every key in the bucket
/// its hash names: what `validate` refuses a save for.
#[test]
fn every_map_holds_each_key_in_its_own_bucket() {
    let Some(paths) = ycd_files("the .ycd map chains") else {
        return;
    };
    let (mut clips, mut animations, mut properties, mut failures) = (0, 0, 0, Vec::new());
    for path in &paths {
        let opened = Opened::read(path);
        let image = opened.image();
        let dict = View::<Raw<CrClipDictionary>>::at_offset(image, 0).unwrap();
        clips += entries(&dict.clips()).unwrap().len();
        if let Some(a) = dict.anim_dictionary().unwrap() {
            animations += entries(&a.animations()).unwrap().len();
        }
        for (_, entry) in entries(&dict.clips()).unwrap() {
            if let Some(p) = entry.data().unwrap().and_then(|c| c.properties().unwrap()) {
                properties += entries(&p.properties()).unwrap().len();
            }
        }
        if let Err(e) = validate(image, 0) {
            failures.push(format!("{}: {e}", name(path)));
        }
    }
    // ⚠ Counted per map: a total cannot tell both maps read from one of them.
    println!(
        "{} .ycd: {clips} clip, {animations} animation and {properties} property entries, each \
         in its own bucket",
        paths.len()
    );
    assert!(clips > 0 && animations > 0 && properties > 0);
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A clip map stating one entry more than its chains hold.
#[test]
fn a_map_whose_count_disagrees_with_its_chains_is_refused() {
    let Some(paths) = ycd_files("a broken .ycd refused") else {
        return;
    };
    let ycd = gates::format(EXT);
    let opened = Opened::read(&paths[0]);
    let dict = View::<Raw<CrClipDictionary>>::at_offset(opened.image(), 0).unwrap();
    let states = dict.clips().used;
    let at = std::mem::offset_of!(Raw<CrClipDictionary>, clips)
        + std::mem::offset_of!(Raw<AtMap<CrClipAny>>, used);
    let mut bytes = opened.bytes.bytes().to_vec();
    bytes[at..at + 2].copy_from_slice(&(states + 1).to_le_bytes());
    let aligned = or_io::decl::Aligned::from_bytes(&bytes);
    let tree = ycd
        .read(T, Image::new(aligned.bytes(), opened.virtual_size))
        .expect("reads");
    let why = ycd
        .save(T, &tree)
        .expect_err("one entry too many")
        .to_string();
    println!("{}: {why}", name(&paths[0]));
    assert!(
        why.contains(&format!("states {} entries", states + 1)),
        "{why}"
    );
}

/// The owned value holds every clip and every animation its maps state.
#[test]
fn every_ycd_opened_owned_holds_every_clip_and_animation_its_maps_state() {
    let Some(paths) = ycd_files("the owned .ycd's clips and animations") else {
        return;
    };
    let (mut clips, mut animations, mut failures) = (0usize, 0usize, Vec::new());
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let opened = Opened::of(&bytes);
        let raw = View::<Raw<CrClipDictionary>>::at_offset(opened.image(), 0).unwrap();
        let states = (
            usize::from(raw.clips().used),
            raw.anim_dictionary()
                .unwrap()
                .map_or(0, |a| usize::from(a.animations().used)),
        );
        let dict = CrClipDictionary::from_bytes(&bytes).unwrap();
        let holds = (
            dict.clips.entries().filter(|e| e.data.is_some()).count(),
            dict.anim_dictionary.as_ref().map_or(0, |a| {
                a.animations.entries().filter(|e| e.data.is_some()).count()
            }),
        );
        if holds != states {
            failures.push(format!(
                "{}: states {states:?}, holds {holds:?}",
                name(path)
            ));
        }
        clips += holds.0;
        animations += holds.1;
    }
    println!(
        "{} .ycd opened owned: {clips} clips and {animations} animations",
        paths.len()
    );
    assert!(clips > 0 && animations > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every clip's animation reference names an animation the dictionary holds.
#[test]
fn every_clip_names_an_animation_its_dictionary_holds() {
    let Some(dicts) = opened_owned("the .ycd clip references") else {
        return;
    };
    let (mut named, mut failures) = (0usize, Vec::new());
    for (file, dict) in &dicts {
        let held: std::collections::BTreeSet<u32> = dict
            .anim_dictionary
            .as_ref()
            .map(|a| a.animations.entries().map(|e| e.key).collect())
            .unwrap_or_default();
        for entry in dict.clips.entries() {
            let keys: Vec<Option<u32>> = match entry.data.as_deref() {
                Some(CrClipAny::CrClipAnimation(c)) => vec![c.animation.key],
                Some(CrClipAny::CrClipAnimations(c)) => {
                    c.animations.iter().map(|a| a.animation.key).collect()
                }
                _ => Vec::new(),
            };
            for key in keys.into_iter().flatten() {
                named += 1;
                if !held.contains(&key) {
                    failures.push(format!(
                        "{file}: clip {:#010x} names {key:#010x}",
                        entry.key
                    ));
                }
            }
        }
    }
    println!("{named} clip references, each to an animation its dictionary holds");
    assert!(named > 0);
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The block count `num_frames` and `frames_per_chunk` imply, as the engine computes it.
fn calc_num_blocks(a: &CrAnimation) -> usize {
    if a.num_frames <= 1 || a.frames_per_chunk == 0 {
        return 1;
    }
    usize::from(a.num_frames - 2) / usize::from(a.frames_per_chunk) + 1
}

/// The frames block `i` covers: a chunk and its shared end frame, the last one the remainder.
fn frames_in_block(a: &CrAnimation, i: usize) -> usize {
    let chunk = usize::from(a.frames_per_chunk);
    if chunk == 0 {
        return 0;
    }
    if i + 1 < calc_num_blocks(a) {
        return chunk + 1;
    }
    match usize::from(a.num_frames.saturating_sub(1)) % chunk {
        0 => chunk + 1,
        remainder => remainder + 1,
    }
}

/// Where a block's channel directory starts: past its constant section and its frames.
///
/// ⚠ Derived, not stored: `channel_offset` is the root-motion table.
fn channel_directory_offset(b: &CrBlockStream) -> usize {
    (b.constant_size as usize)
        .saturating_add(usize::from(b.frame_size).saturating_mul(usize::from(b.num_frames)))
}

/// What an animation's blocks contradict of its own frame claims, one line each.
fn contradictions(where_: &str, a: &CrAnimation) -> Vec<String> {
    let blocks: Vec<&CrBlockStream> = a.blocks.iter().flatten().map(|b| b.as_ref()).collect();
    let mut out = Vec::new();
    if blocks.is_empty() {
        return out;
    }
    let expected = calc_num_blocks(a);
    if a.blocks.len() != expected {
        out.push(format!(
            "{where_}: {} frames at {} per chunk needs {expected} block(s), and it has {}",
            a.num_frames,
            a.frames_per_chunk,
            a.blocks.len()
        ));
    }
    if a.frames_per_chunk != 0 {
        for (i, b) in blocks.iter().enumerate() {
            let expected = frames_in_block(a, i);
            if usize::from(b.num_frames) != expected {
                out.push(format!(
                    "{where_}: block {i} says {} frame(s) where the chunking needs {expected}",
                    b.num_frames
                ));
            }
        }
    }
    for (i, b) in blocks.iter().enumerate() {
        let start = channel_directory_offset(b);
        if b.compact_size == 0 && !b.data.is_empty() && start > b.data.len() {
            out.push(format!(
                "{where_}: block {i}'s directory starts at {start} in a {}-byte payload",
                b.data.len()
            ));
        }
    }
    let largest = blocks.iter().map(|b| b.data.len()).max().unwrap_or(0);
    if largest > a.max_block_size as usize {
        out.push(format!(
            "{where_}: max_block_size says {} and the largest block holds {largest}",
            a.max_block_size
        ));
    }
    out
}

/// Each animation's block count, each block's frames, each block's channel
/// directory and its largest block against what the animation states: pairs
/// of places a single wrong offset moves independently.
#[test]
fn every_animations_blocks_agree_with_its_frames() {
    let Some(dicts) = opened_owned("the .ycd block invariants") else {
        return;
    };
    let (mut animations, mut blocks, mut failures) = (0usize, 0usize, Vec::new());
    for (file, dict) in &dicts {
        let Some(anims) = dict.anim_dictionary.as_ref() else {
            continue;
        };
        for entry in anims.animations.entries() {
            let Some(a) = entry.data.as_deref() else {
                continue;
            };
            animations += usize::from(!a.blocks.is_empty());
            blocks += a.blocks.len();
            failures.extend(contradictions(&format!("{file} {:#010x}", entry.key), a));
        }
    }
    println!("{animations} animations with blocks, {blocks} blocks, each agreeing with its frames");
    assert!(animations > 0 && blocks > 0);
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn each_block_invariant_can_fail() {
    let block = |frames: u16, bytes: usize| {
        Some(Box::new(CrBlockStream {
            num_frames: frames,
            constant_size: 64,
            frame_size: 12,
            data: vec![0; bytes],
            ..Default::default()
        }))
    };
    let control = CrAnimation {
        num_frames: 260,
        frames_per_chunk: 127,
        max_block_size: 4096,
        blocks: vec![block(128, 4096), block(128, 1696), block(6, 200)],
        ..Default::default()
    };
    assert_eq!(contradictions("control", &control), Vec::<String>::new());
    let broken = |edit: &dyn Fn(&mut CrAnimation)| {
        let mut a = control.clone();
        edit(&mut a);
        contradictions("perturbed", &a).len()
    };
    assert_eq!(
        broken(&|a| a.num_frames = 127),
        4,
        "the block count, and each of three blocks against one 127-frame chunk"
    );
    assert_eq!(
        broken(&|a| a.blocks.truncate(1)),
        1,
        "one block for 260 frames"
    );
    assert_eq!(
        broken(&|a| a.blocks[1].as_mut().unwrap().num_frames = 127),
        1,
        "a middle block one frame short"
    );
    assert_eq!(
        broken(&|a| a.blocks[1].as_mut().unwrap().frame_size = 120),
        1,
        "a directory past the payload"
    );
    assert_eq!(
        broken(&|a| a.max_block_size = 2),
        1,
        "a claim below the largest block"
    );
}

/// The bucket check refuses an entry chained under the wrong bucket, and a
/// count the chains do not hold.
#[test]
fn the_map_checks_can_fail() {
    let Some(paths) = ycd_files("a .ycd's maps perturbed") else {
        return;
    };
    let bytes = std::fs::read(&paths[0]).unwrap();
    let mut dict = CrClipDictionary::from_bytes(&bytes).unwrap();
    assert!(
        dict.clips.hash.len() > 1,
        "a map of one bucket moves nothing"
    );
    dict.to_bytes().expect("the file as shipped saves");
    let head = dict
        .clips
        .hash
        .iter()
        .position(Option::is_some)
        .expect("an entry");
    let entry = dict.clips.hash[head].as_mut().unwrap();
    entry.key = entry.key.wrapping_add(1);
    let why = dict.to_bytes().unwrap_err().to_string();
    println!("{why}");
    assert!(why.contains("a lookup walks bucket"), "{why}");
}

/// The description carries a block's payload as bytes trailing its header,
/// with no element a decoder could misread as a layout.
#[test]
fn a_blocks_payload_trails_its_header_as_bytes() {
    let pack = gates::pack(EXT, T);
    let block = pack.type_by_name("CrBlockStream").expect("described");
    let run = block.trailing().expect("the payload trails the header");
    assert_eq!(run.name, "data");
    assert!(
        run.repr.referenced_type().is_none(),
        "the payload is bit- and Rice-packed channels, not records: {:?}",
        run.repr
    );
}

/// Each map names its own element, so a clip map is never read as a map of animations.
#[test]
fn each_map_names_its_own_element_type() {
    let pack = gates::pack(EXT, T);
    let inline = |owner: &str, field: &str| match pack
        .type_by_name(owner)
        .and_then(|t| t.field_by_name(field))
        .map(|f| f.repr.clone())
    {
        Some(FieldRepr::Inline { type_name }) => type_name,
        other => panic!("{owner}.{field} is {other:?}"),
    };
    let clips = inline("CrClipDictionary", "clips");
    let animations = inline("CrAnimDictionary", "animations");
    assert_ne!(clips, animations);
    let data = |map: &str| {
        let bucket = pack
            .type_by_name(map)
            .and_then(|t| t.field_by_name("hash"))
            .and_then(|f| f.repr.referenced_type().map(str::to_string))
            .unwrap_or_else(|| panic!("{map}.hash"));
        pack.type_by_name(&bucket)
            .and_then(|t| t.field_by_name("data"))
            .and_then(|f| f.repr.referenced_type().map(str::to_string))
            .unwrap_or_else(|| panic!("{bucket}.data"))
    };
    assert_eq!(data(&clips), "CrClip");
    assert_eq!(data(&animations), "CrAnimation");
}

/// The clip map read as a map of animations does not reproduce any file.
#[test]
fn naming_the_wrong_element_for_the_clip_map_does_not_read_the_file() {
    let Some(paths) = ycd_files("the clip map read as a map of animations") else {
        return;
    };
    let honest = gates::pack(EXT, T);
    let animations = match honest
        .type_by_name("CrAnimDictionary")
        .and_then(|t| t.field_by_name("animations"))
        .map(|f| f.repr.clone())
    {
        Some(r @ FieldRepr::Inline { .. }) => r,
        other => panic!("{other:?}"),
    };
    let mut wrong = (*honest).clone();
    let root = gates::format(EXT).root_name(T).unwrap();
    wrong
        .types
        .iter_mut()
        .find(|t| t.name == root)
        .and_then(|t| t.fields.iter_mut().find(|f| f.name == "clips"))
        .expect("a clip dictionary embeds its clip map")
        .repr = animations;
    let root = or_io::decl::intern(&root);
    let mut reproduced = 0usize;
    for path in &paths {
        let opened = Opened::read(path);
        let honest_tree = gates::format(EXT).read(T, opened.image()).unwrap();
        if let Ok(skewed) =
            or_io::desc::value::read(&wrong, or_io::desc::TypeName(root), opened.image(), 0)
        {
            reproduced += usize::from(skewed == honest_tree);
        }
    }
    println!(
        "{reproduced} of {} files read the same with the wrong element",
        paths.len()
    );
    assert_eq!(reproduced, 0);
}

/// ⚠ A v43 `.ycd` inflates but is not the v46 layout: read as v46 it yields
/// a property claiming tens of thousands of attributes, so it is refused.
#[test]
fn a_v43_ycd_is_refused_rather_than_read_as_v46() {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let format = gates::format(EXT);
    assert!(format.target_of_version(43).is_err());
    let staged: Vec<_> = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), EXT)
        .all_targets()
        .find()
        .into_iter()
        .filter(|p| {
            std::fs::read(p)
                .ok()
                .and_then(|b| Rsc7::peek(&b).ok())
                .is_some_and(|h| h.version() == 43)
        })
        .collect();
    for path in &staged {
        let bytes = std::fs::read(path).expect("reads");
        let o = Rsc7::open(&bytes).unwrap_or_else(|e| panic!("{}: {e}", name(path)));
        assert_eq!(
            o.data.len(),
            o.seal.total_size() as usize,
            "{}: the payload past its eight-byte prefix inflates to the stated size",
            name(path)
        );
        assert!(
            or_io::open(path).is_err(),
            "{}: a v43 clip dictionary opened",
            name(path)
        );
    }
    println!("{} staged v43 .ycd, each refused", staged.len());
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_a_ycd_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(paths) = ycd_files("the .ycd read cost") else {
        return;
    };
    let opened: Vec<Opened> = paths.iter().map(|p| Opened::read(p)).collect();
    let best = |rounds: usize, f: &mut dyn FnMut() -> usize| {
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let mut sink = 0usize;
            for _ in 0..rounds {
                sink = sink.wrapping_add(f());
            }
            black_box(sink);
            best = best.min(start.elapsed());
        }
        best.as_secs_f64() * 1e3 / rounds as f64
    };
    // Every animation's frame count and every dof's bone tag, typed from the root.
    #[inline(never)]
    fn typed_sum(image: Image<'_>) -> usize {
        let dict = View::<Raw<CrClipDictionary>>::at_offset(image, 0).unwrap();
        let Some(animations) = dict.anim_dictionary().unwrap() else {
            return 0;
        };
        let mut total = 0usize;
        for (_, entry) in entries(&animations.animations()).unwrap() {
            if let Some(a) = entry.data().unwrap() {
                total = total.wrapping_add(usize::from(a.num_frames));
                total = a
                    .dofs()
                    .unwrap()
                    .iter()
                    .fold(total, |t, d| t.wrapping_add(usize::from(d.id)));
            }
        }
        total
    }
    let typed_ms = best(200, &mut || {
        opened.iter().fold(0usize, |t, o| {
            t.wrapping_add(typed_sum(black_box(o.image())))
        })
    });
    let tree_ms = best(3, &mut || {
        opened
            .iter()
            .map(|o| {
                let tree = gates::format(EXT).read(T, o.image()).unwrap();
                usize::from(matches!(tree, Value::Struct(_)))
            })
            .sum()
    });
    println!(
        "{} files: every animation's frames and dofs typed from the root {typed_ms:.4} ms; the \
         whole tree read {tree_ms:.2} ms ({:.0}x)",
        paths.len(),
        tree_ms / typed_ms
    );
}
