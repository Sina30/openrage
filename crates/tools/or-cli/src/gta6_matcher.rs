//! What GTA VI's animation reader states of a motion dictionary's matchers: every word it reads,
//! raw where no role is known.

use or_io::gta6::anim::{AnimResource, Matcher, MatcherClass};

/// One line for each matcher of `resource`, and one for its database.
pub(crate) fn print_matchers(resource: &AnimResource) {
    if resource.matchers.is_empty() {
        return;
    }
    println!("  matchers      {}", resource.matchers.len());
    for matcher in &resource.matchers {
        let listed = matcher
            .clips
            .iter()
            .filter(|c| c.dictionary.is_some())
            .count();
        println!(
            "    matcher {:?}  key {:#010X}, {} clip(s) ({listed} with their clip dictionary's key, the others by an alias word whose source is not known), {} entries, {} frame(s), {} key(s) in its map at 0x170",
            matcher.name,
            matcher.key,
            matcher.clips.len(),
            matcher.entries.len(),
            matcher.database.frames,
            matcher.keys_at_170.len()
        );
        let database = &matcher.database;
        let dims: u32 = database.features.iter().map(|f| u32::from(f.dims)).sum();
        let kinds: Vec<String> = (database.features.iter())
            .map(|f| format!("kind {} of {}", f.kind, f.dims))
            .collect();
        let class = match &database.class {
            MatcherClass::Vectors { vectors, .. } => {
                format!("class 1, a rotation and a vector of f16 halves for each of {} frame(s), whose meaning is not known", vectors.len())
            }
            MatcherClass::Levels {
                dimension_order,
                pairs,
                stored,
                levels,
            } => {
                let lanes: Vec<String> = (levels.iter())
                    .map(|l| {
                        format!(
                            "{} element(s) of lanes of {} frames",
                            l.minima.len(),
                            l.lane_frames
                        )
                    })
                    .collect();
                format!(
                    "class 3, {} frame(s) of {} stored dimension(s) in {pairs} pair(s), {} level(s) of bounds ({}); an order of {} dimensions, which by an inferred reading maps stored dimension k to feature dimension order[k]",
                    stored.len(),
                    dimension_order.len(),
                    levels.len(),
                    lanes.join(", "),
                    dimension_order.len()
                )
            }
        };
        println!(
            "      database  {} bone(s), {} feature(s) of {dims} dimension(s) ({}), {} hash(es) at 0x68, {} frame word(s), {class}",
            database.skeleton.bones.len(),
            database.features.len(),
            kinds.join(", "),
            database.hashes_at_68.len(),
            database.frame_words.len()
        );
        print_words(matcher);
    }
}

fn hex(bytes: &[u8]) -> String {
    let shown: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    shown.join(" ")
}

fn halves(halves: &[u16]) -> String {
    let shown: Vec<String> = (halves.iter())
        .map(|h| or_io::half::half_to_f32(*h).to_string())
        .collect();
    shown.join(" ")
}

/// Every word the reader reads of `matcher`, raw where no role is known: its stored bytes and
/// words, each entry, each frame's word, record and vector, and each level's bounds.
fn print_words(matcher: &Matcher) {
    let d = &matcher.database;
    let bones: Vec<&str> = d.skeleton.bones.iter().map(|b| b.name.as_str()).collect();
    println!("      bones     {}", bones.join(", "));
    println!("      guid      {}", hex(&matcher.guid));
    println!("      0x48      {}", hex(&matcher.object_at_48));
    println!("      0xB0      {}", hex(&matcher.clip_bytes));
    println!("      0x110     {:02X}", matcher.byte_at_110);
    println!("      0x170     {:08X?}", matcher.keys_at_170);
    println!("      0x190     {:08X?}", matcher.words_at_190);
    println!("      db 0x30   {}", hex(&d.object_at_30));
    println!("      db 0x68   {:08X?}", d.hashes_at_68);
    println!("      db 0x78   {:08X?}", d.words_at_78);
    println!("      db 0xC0   {}", d.word_at_c0);
    for (i, e) in matcher.entries.iter().enumerate() {
        println!(
            "      entry {i}   clip {}, start {} s, rate {}, frames {}..{}",
            e.clip, e.start, e.rate, e.frames.start, e.frames.end
        );
    }
    let (vectors, rotations) = match &d.class {
        MatcherClass::Vectors { vectors, rotations } => (vectors, Some(rotations)),
        MatcherClass::Levels { stored, .. } => (stored, None),
    };
    for (f, vector) in vectors.iter().enumerate() {
        let word = d.frame_words.get(f).copied().unwrap_or_default();
        let record = match (rotations.and_then(|r| r.get(f)), d.record_floats.get(f)) {
            (Some(q), Some(rest)) => format!(" rotation {q:?}, floats 4 to 11 {rest:?},"),
            _ => String::new(),
        };
        println!(
            "      frame {f}   word {word:#X},{record} vector {}",
            halves(vector)
        );
    }
    if let MatcherClass::Levels { pairs, levels, .. } = &d.class {
        for (l, level) in levels.iter().enumerate() {
            let pairs = usize::from(*pairs);
            for (e, (lo, hi)) in level.minima.iter().zip(&level.maxima).enumerate() {
                let (node, pair) = (
                    e.checked_div(pairs).unwrap_or(0),
                    e.checked_rem(pairs).unwrap_or(0),
                );
                println!(
                    "      level {l} node {node} pair {pair}   least {}   greatest {}",
                    halves(lo),
                    halves(hi)
                );
            }
        }
    }
}

/// Each matcher of `resource`, field by field, for `rpf scan-anim --json`.
pub(crate) fn report(resource: &AnimResource) -> serde_json::Value {
    use serde_json::json;
    let matcher = |m: &Matcher| {
        let d = &m.database;
        let class = match &d.class {
            MatcherClass::Vectors { vectors, rotations } => json!({
                "class": 1, "frames": vectors.len(), "vectors": vectors, "rotations": rotations,
                "record_floats": d.record_floats,
            }),
            MatcherClass::Levels {
                dimension_order,
                pairs,
                stored,
                levels,
            } => {
                let levels: Vec<_> = (levels.iter())
                    .map(|l| json!({ "lane_frames": l.lane_frames, "elements": l.minima.len(), "minima": l.minima, "maxima": l.maxima }))
                    .collect();
                json!({ "class": 3, "dimension_order": dimension_order, "pairs": pairs, "frames": stored.len(), "stored": stored, "levels": levels })
            }
        };
        json!({
            "name": m.name, "key": m.key,
            "clips": m.clips.iter().map(|c| json!({ "leaf": c.leaf, "alias": c.alias, "dictionary": c.dictionary })).collect::<Vec<_>>(),
            "entries": m.entries.iter().map(|e| json!({ "clip": e.clip, "start": e.start, "rate": e.rate, "frames": [e.frames.start, e.frames.end] })).collect::<Vec<_>>(),
            "keys_at_170": m.keys_at_170, "words_at_190": m.words_at_190, "guid": m.guid,
            "object_at_48": m.object_at_48, "clip_bytes": m.clip_bytes, "byte_at_110": m.byte_at_110,
            "database": {
                "bones": d.skeleton.bones.iter().map(|b| &b.name).collect::<Vec<_>>(),
                "features": d.features.iter().map(|f| json!({ "name": f.name, "offset": f.offset, "dims": f.dims, "kind": f.kind, "words": f.words })).collect::<Vec<_>>(),
                "frames": d.frames, "hashes_at_68": d.hashes_at_68, "frame_words": d.frame_words,
                "word_at_c0": d.word_at_c0, "weights": d.weights, "words_at_78": d.words_at_78,
                "object_at_30": d.object_at_30, "layout": class,
            },
        })
    };
    resource.matchers.iter().map(matcher).collect()
}
