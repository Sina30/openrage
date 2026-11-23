//! What GTA VI's animation reader states of a motion dictionary's matchers: structure and counts only.

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
    }
}

/// Each matcher of `resource`, field by field, for `rpf scan-anim --json`.
pub(crate) fn report(resource: &AnimResource) -> serde_json::Value {
    use serde_json::json;
    let matcher = |m: &Matcher| {
        let d = &m.database;
        let class = match &d.class {
            MatcherClass::Vectors { vectors, .. } => json!({ "class": 1, "frames": vectors.len() }),
            MatcherClass::Levels {
                dimension_order,
                pairs,
                stored,
                levels,
            } => {
                let levels: Vec<_> = (levels.iter())
                    .map(|l| json!({ "lane_frames": l.lane_frames, "elements": l.minima.len() }))
                    .collect();
                json!({ "class": 3, "dimension_order": dimension_order, "pairs": pairs, "frames": stored.len(), "levels": levels })
            }
        };
        json!({
            "name": m.name, "key": m.key,
            "clips": m.clips.iter().map(|c| json!({ "leaf": c.leaf, "alias": c.alias, "dictionary": c.dictionary })).collect::<Vec<_>>(),
            "entries": m.entries.iter().map(|e| json!({ "clip": e.clip, "start": e.start, "rate": e.rate, "frames": [e.frames.start, e.frames.end] })).collect::<Vec<_>>(),
            "keys_at_170": m.keys_at_170, "words_at_190": m.words_at_190,
            "database": {
                "bones": d.skeleton.bones.iter().map(|b| &b.name).collect::<Vec<_>>(),
                "features": d.features.iter().map(|f| json!({ "name": f.name, "offset": f.offset, "dims": f.dims, "kind": f.kind, "words": f.words })).collect::<Vec<_>>(),
                "frames": d.frames, "hashes_at_68": d.hashes_at_68, "frame_words": d.frame_words.len(),
                "word_at_c0": d.word_at_c0, "weights": d.weights, "layout": class,
            },
        })
    };
    resource.matchers.iter().map(matcher).collect()
}
