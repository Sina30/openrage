//! Every value GTA VI's motion reader reads of a motion, beyond the counts `rpf scan-open` prints:
//! time warps, weight lists, parameters, and each group's curve and range object with its grids.

use or_io::gta6::anim::{known_name, ArrayHead, Grid, Motion, Typed};
use serde_json::{json, Value};

fn named(hash: u32) -> String {
    known_name(hash).map_or_else(|| format!("hash_{hash:08X}"), str::to_string)
}

fn head_line(h: &ArrayHead) -> String {
    format!(
        "{{{:#X}, {} of {}, {:#X}}}",
        h.pointer, h.count, h.capacity, h.word
    )
}

fn head_json(h: &ArrayHead) -> Value {
    json!({ "pointer": h.pointer, "count": h.count, "capacity": h.capacity, "word": h.word })
}

fn grid_line(grid: &Grid) -> String {
    format!(
        "words {:?}, cells {:?}, bounds {:?}, cell sizes {:?}, inverses {:?}",
        grid.words,
        grid.cells,
        &grid.floats[..6],
        &grid.floats[6..9],
        &grid.floats[9..]
    )
}

fn grid_json(grid: &Grid) -> Value {
    json!({ "words": grid.words, "cells": grid.cells, "floats": grid.floats })
}

/// The lines of group `g`'s typed object `typed`.
fn typed_lines(g: usize, typed: &Typed) -> Vec<String> {
    let mut out = Vec::new();
    match typed {
        Typed::Constants(values) => out.push(format!("group {g} constants {values:?}")),
        Typed::PhaseCurves(curves) => {
            for (i, c) in curves.iter().enumerate() {
                out.push(format!(
                    "group {g} phase curve {i}: points {:?}, x {} to {}, y1 {}, {} cycle(s)",
                    c.points, c.x0, c.x1, c.y1, c.cycles
                ));
            }
        }
        Typed::Range {
            lo,
            hi,
            hashes,
            bounds,
        } => {
            let hashes: Vec<String> = hashes.iter().map(|h| named(*h)).collect();
            let bounds: Vec<String> = (bounds.iter())
                .map(|(h, v)| format!("{} {v}", named(*h)))
                .collect();
            out.push(format!(
                "group {g} range {lo} to {hi}: hashes [{}], bounds [{}]",
                hashes.join(", "),
                bounds.join(", ")
            ));
        }
        Typed::Lists {
            head,
            lists,
            hashes,
        } => {
            out.push(format!(
                "group {g} lists {lists:08X?}, hashes {hashes:08X?}, array at 0x10 {}",
                head_line(head)
            ));
        }
        Typed::Table(db) => out.push(format!("group {g} grid 0: {}", grid_line(&db.grid))),
        Typed::Rows(db) => {
            for (i, grid) in db.grids.iter().enumerate() {
                out.push(format!("group {g} grid {i}: {}", grid_line(grid)));
            }
            out.push(format!(
                "group {g} database bounds {:?}, hash at 0x80 {}, {} weight list(s)",
                db.bounds,
                named(db.hash),
                db.lists
            ));
        }
        Typed::Simplices(db) => {
            for (i, grid) in db.grids.iter().enumerate() {
                out.push(format!("group {g} grid {i}: {}", grid_line(grid)));
            }
            out.push(format!("group {g} corner grid: {}", grid_line(&db.grid)));
            for (i, words) in db.simplices.iter().enumerate() {
                out.push(format!("group {g} simplex {i}: {words:?}"));
            }
            out.push(format!(
                "group {g} column ranges {:?}, {} weight list(s)",
                db.ranges, db.lists
            ));
        }
    }
    out
}

/// A line for each value `motion`'s reader reads that `rpf scan-open` counts.
pub(crate) fn lines(motion: &Motion) -> Vec<String> {
    let mut out = Vec::new();
    for (i, w) in motion.time_warps.iter().enumerate() {
        out.push(format!(
            "time warp {i}: a {}, b {}, points {:?}",
            w.a, w.b, w.points
        ));
    }
    for (i, list) in motion.weight_lists.iter().enumerate() {
        let weights: Vec<String> = (list.iter())
            .map(|w| format!("clip {} ({}, {})", w.clip, w.values[0], w.values[1]))
            .collect();
        out.push(format!("weight list {i}: {}", weights.join(", ")));
    }
    for p in &motion.parameters {
        out.push(format!(
            "parameter {}: words {:?}, word {:#010X}, {} to {}",
            named(p.hash),
            p.words,
            p.word,
            p.min,
            p.max
        ));
    }
    for (c, items) in motion.parameter_items.iter().enumerate() {
        for (p, item) in items.iter().enumerate() {
            out.push(format!(
                "clip {c} parameter {p}: points {:?}, floats {:?}",
                item.points, item.floats
            ));
        }
    }
    for (i, o) in motion.parameter_objects.iter().enumerate() {
        let mut line = format!(
            "parameter object {i} of type {}: arrays at 0x10 {} and 0x20 {}",
            o.kind,
            head_line(&o.heads[0]),
            head_line(&o.heads[1])
        );
        if let Some((lists, hashes)) = &o.lists {
            line.push_str(&format!(", lists {lists:08X?}, hashes {hashes:08X?}"));
        }
        out.push(line);
    }
    for (g, group) in motion.groups.iter().enumerate() {
        out.extend(typed_lines(g, &group.curve));
        out.extend(typed_lines(g, &group.range));
    }
    out
}

/// Prints [`lines`] under a motion's line.
pub(crate) fn print(motion: &Motion) {
    for line in lines(motion) {
        println!("      {line}");
    }
}

fn typed_json(typed: &Typed) -> Value {
    match typed {
        Typed::Constants(values) => json!({ "type": 1, "values": values }),
        Typed::PhaseCurves(curves) => json!({
            "type": 2,
            "curves": curves.iter().map(|c| json!({
                "points": c.points, "x0": c.x0, "x1": c.x1, "y1": c.y1, "cycles": c.cycles,
            })).collect::<Vec<_>>(),
        }),
        Typed::Range {
            lo,
            hi,
            hashes,
            bounds,
        } => json!({ "type": 3, "lo": lo, "hi": hi, "hashes": hashes, "bounds": bounds }),
        Typed::Lists {
            head,
            lists,
            hashes,
        } => json!({ "type": 4, "head": head_json(head), "lists": lists, "hashes": hashes }),
        Typed::Table(db) => json!({ "type": 5, "grid": grid_json(&db.grid) }),
        Typed::Rows(db) => json!({
            "type": 6, "grids": db.grids.iter().map(grid_json).collect::<Vec<_>>(),
            "bounds": db.bounds, "hash": db.hash, "lists": db.lists,
        }),
        Typed::Simplices(db) => json!({
            "type": 8, "grids": db.grids.iter().map(grid_json).collect::<Vec<_>>(),
            "grid": grid_json(&db.grid), "simplices": db.simplices, "ranges": db.ranges,
            "lists": db.lists,
        }),
    }
}

/// What [`lines`] prints, for `rpf scan-anim --json`.
pub(crate) fn report(motion: &Motion) -> Value {
    json!({
        "time_warps": motion.time_warps.iter().map(|w| json!({ "a": w.a, "b": w.b, "points": w.points })).collect::<Vec<_>>(),
        "weight_lists": motion.weight_lists.iter().map(|l| l.iter().map(|w| json!({ "clip": w.clip, "values": w.values })).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "parameter_words": motion.parameters.iter().map(|p| json!({ "hash": p.hash, "words": p.words, "word": p.word, "min": p.min, "max": p.max })).collect::<Vec<_>>(),
        "parameter_items": motion.parameter_items.iter().map(|items| items.iter().map(|i| json!({ "points": i.points, "floats": i.floats })).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "parameter_objects": motion.parameter_objects.iter().map(|o| json!({ "type": o.kind, "heads": o.heads.iter().map(head_json).collect::<Vec<_>>(), "lists": o.lists.as_ref().map(|(l, _)| l), "hashes": o.lists.as_ref().map(|(_, h)| h) })).collect::<Vec<_>>(),
        "group_objects": motion.groups.iter().map(|g| json!({ "curve": typed_json(&g.curve), "range": typed_json(&g.range) })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use or_io::gta6::anim::{Parameter, ParameterItem, ParameterObject, TimeWarp, Weight};

    use super::*;

    #[test]
    fn every_value_of_a_motion_is_a_line_and_a_field() {
        let motion = Motion {
            name: "pack:/walk.motion".into(),
            key: 1,
            kind: 2,
            clips: vec![7],
            time_warps: vec![TimeWarp {
                a: 2.0,
                b: 2.0,
                points: vec![[0.0, 0.0], [2.0, 2.0]],
            }],
            weight_lists: vec![vec![Weight {
                clip: 0,
                values: [0.5, 1.5],
            }]],
            parameters: vec![Parameter {
                hash: 0x0000_00AB,
                words: [1, 2],
                word: 3,
                min: 0.0,
                max: 4.0,
            }],
            parameter_items: vec![vec![ParameterItem {
                points: vec![[0.0, 1.0]],
                floats: [1.0, 2.0, 3.0, 4.0],
            }]],
            groups: vec![or_io::gta6::anim::MotionGroup {
                first_clip: 0,
                clips: 1,
                first_weight_list: 0,
                weight_lists: 1,
                curve: Typed::Constants(vec![0.25]),
                range: Typed::Lists {
                    head: ArrayHead {
                        pointer: 0,
                        count: 0,
                        capacity: 0,
                        word: 0,
                    },
                    lists: vec![vec![1, 2]],
                    hashes: vec![0xFF],
                },
                tags: Vec::new(),
            }],
            parameter_objects: vec![ParameterObject {
                kind: 2,
                heads: [
                    ArrayHead {
                        pointer: 0x5000_0040,
                        count: 3,
                        capacity: 4,
                        word: 0,
                    },
                    ArrayHead {
                        pointer: 0,
                        count: 0,
                        capacity: 0,
                        word: 1,
                    },
                ],
                lists: None,
            }],
            properties: Vec::new(),
        };
        let lines = lines(&motion);
        assert_eq!(
            lines,
            [
                "time warp 0: a 2, b 2, points [[0.0, 0.0], [2.0, 2.0]]",
                "weight list 0: clip 0 (0.5, 1.5)",
                "parameter hash_000000AB: words [1, 2], word 0x00000003, 0 to 4",
                "clip 0 parameter 0: points [[0.0, 1.0]], floats [1.0, 2.0, 3.0, 4.0]",
                "parameter object 0 of type 2: arrays at 0x10 {0x50000040, 3 of 4, 0x0} and 0x20 {0x0, 0 of 0, 0x1}",
                "group 0 constants [0.25]",
                "group 0 lists [[00000001, 00000002]], hashes [000000FF], array at 0x10 {0x0, 0 of 0, 0x0}",
            ]
        );
        let report = report(&motion);
        assert_eq!(report["weight_lists"][0][0]["values"], json!([0.5, 1.5]));
        assert_eq!(report["group_objects"][0]["range"]["type"], 4);
        assert_eq!(report["parameter_words"][0]["words"], json!([1, 2]));
        assert_eq!(
            report["parameter_items"][0][0]["floats"],
            json!([1.0, 2.0, 3.0, 4.0])
        );
        assert_eq!(report["parameter_objects"][0]["heads"][0]["count"], 3);
        assert_eq!(report["parameter_objects"][0]["lists"], Value::Null);
    }
}
