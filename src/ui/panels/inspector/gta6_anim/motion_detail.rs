//! Every value GTA VI's motion reader reads of a motion dictionary's motions, a line each: time
//! warps, weight lists, parameters, and each group's curve and range object with its grids.

use or_io::gta6::anim::{Grid, Motion, Typed};

use super::*;

/// One scrolled list of every motion's values, made once for the resource.
pub(super) fn draw(ui: &mut egui::Ui, doc_id: DocumentId, resource: &std::sync::Arc<AnimResource>) {
    draw_lines(
        ui,
        ("gta6_anim_motion_lines", doc_id),
        resource,
        |resource| resource.motions.iter().flat_map(lines).collect(),
    );
}

fn points(points: &[[f32; 2]]) -> String {
    let shown: Vec<String> = points.iter().map(|[x, y]| format!("({x}, {y})")).collect();
    shown.join(" ")
}

fn hashes(words: &[u32]) -> String {
    let shown: Vec<String> = words.iter().map(|h| named(*h)).collect();
    format!("[{}]", shown.join(", "))
}

/// Grid `index` of group `group`: its two words, its cell ranges, its bounds, cell sizes and inverses.
fn grid(group: usize, index: usize, grid: &Grid) -> String {
    tr!(
        "Group {group}, grid {index}: words {words}, cells {cells}, bounds {bounds}, cell sizes {sizes}, inverses {inverses}",
        group = group,
        index = index,
        words = format!("{:?}", grid.words),
        cells = format!("{:?}", grid.cells),
        bounds = format!("{:?}", &grid.floats[..6]),
        sizes = format!("{:?}", &grid.floats[6..9]),
        inverses = format!("{:?}", &grid.floats[9..]),
    )
}

/// The lines of group `group`'s typed object `typed`.
fn typed_lines(group: usize, typed: &Typed, out: &mut Vec<String>) {
    match typed {
        Typed::Constants(values) => out.push(tr!(
            "Group {group}, constants: {values}",
            group = group,
            values = format!("{values:?}")
        )),
        Typed::PhaseCurves(curves) => {
            for (i, c) in curves.iter().enumerate() {
                out.push(tr!(
                    "Group {group}, phase curve {index}: points {points}, x from {x0} to {x1}, y1 {y1}, {cycles} cycles",
                    group = group,
                    index = i,
                    points = points(&c.points),
                    x0 = c.x0,
                    x1 = c.x1,
                    y1 = c.y1,
                    cycles = c.cycles,
                ));
            }
        }
        Typed::Range {
            lo,
            hi,
            hashes: words,
            bounds,
        } => {
            let bounds: Vec<String> = (bounds.iter())
                .map(|(h, v)| format!("{} {v}", named(*h)))
                .collect();
            out.push(tr!(
                "Group {group}, range {lo} to {hi}: hashes {hashes}, bounds {bounds}",
                group = group,
                lo = lo,
                hi = hi,
                hashes = hashes(words),
                bounds = bounds.join(", "),
            ));
        }
        Typed::Lists {
            lists,
            hashes: words,
        } => out.push(tr!(
            "Group {group}, lists {lists}, hashes {hashes}",
            group = group,
            lists = format!("{lists:08X?}"),
            hashes = hashes(words),
        )),
        Typed::Table(database) => out.push(grid(group, 0, &database.grid)),
        Typed::Rows(database) => {
            out.extend((database.grids.iter().enumerate()).map(|(i, g)| grid(group, i, g)));
            out.push(tr!(
                "Group {group}, database bounds {bounds}, hash at 0x80 {hash}, weight lists {lists}",
                group = group,
                bounds = format!("{:?}", database.bounds),
                hash = named(database.hash),
                lists = database.lists,
            ));
        }
        Typed::Simplices(database) => {
            // The grid the corners lie over is listed after the tables' grids.
            let grids = database.grids.iter().chain([&database.grid]);
            out.extend(grids.enumerate().map(|(i, g)| grid(group, i, g)));
            for (i, words) in database.simplices.iter().enumerate() {
                out.push(tr!(
                    "Group {group}, simplex {index}: {words}",
                    group = group,
                    index = i,
                    words = format!("{words:?}"),
                ));
            }
            out.push(tr!(
                "Group {group}, column ranges {ranges}, weight lists {lists}",
                group = group,
                ranges = format!("{:?}", database.ranges),
                lists = database.lists,
            ));
        }
    }
}

/// `motion`'s name, then a line for each value its reader reads that the rows above count.
fn lines(motion: &Motion) -> Vec<String> {
    let mut out = vec![motion.name.clone()];
    for (i, w) in motion.time_warps.iter().enumerate() {
        out.push(tr!(
            "Time warp {index}: a {a}, b {b}, points {points}",
            index = i,
            a = w.a,
            b = w.b,
            points = points(&w.points),
        ));
    }
    for (i, list) in motion.weight_lists.iter().enumerate() {
        let weights: Vec<String> = (list.iter())
            .map(|w| format!("#{} ({}, {})", w.clip, w.values[0], w.values[1]))
            .collect();
        out.push(tr!(
            "Weight list {index}: {weights}",
            index = i,
            weights = weights.join(" "),
        ));
    }
    for p in &motion.parameters {
        out.push(tr!(
            "Parameter {name}: words {words}, word {word}, from {min} to {max}",
            name = named(p.hash),
            words = format!("{:?}", p.words),
            word = format!("{:#010X}", p.word),
            min = p.min,
            max = p.max,
        ));
    }
    if !motion.parameter_objects.is_empty() {
        out.push(tr!(
            "Parameter objects, by type: {types}",
            types = format!("{:?}", motion.parameter_objects)
        ));
    }
    for (g, group) in motion.groups.iter().enumerate() {
        typed_lines(g, &group.curve, &mut out);
        typed_lines(g, &group.range, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use or_io::gta6::anim::{Parameter, PhaseCurve, TimeWarp, Weight};

    use super::*;

    #[test]
    fn a_motion_s_values_are_each_a_line_and_its_groups_name_their_objects() {
        let _serial = crate::ui::theme::serial();
        let grid = Grid {
            words: [2, 3],
            cells: [[0, 1], [-1, 1], [0, 0]],
            floats: [0.5; 12],
        };
        let motion = Motion {
            name: "pack:/walk.motion".into(),
            key: 1,
            kind: 1,
            clips: vec![7, 8],
            time_warps: vec![TimeWarp {
                a: 2.0,
                b: 2.0,
                points: vec![[0.0, 0.0], [2.0, 2.0]],
            }],
            weight_lists: vec![vec![Weight {
                clip: 1,
                values: [0.25, 0.75],
            }]],
            parameters: vec![Parameter {
                hash: 0xABCD,
                words: [3, 4],
                word: 9,
                min: -1.0,
                max: 1.0,
            }],
            groups: vec![or_io::gta6::anim::MotionGroup {
                first_clip: 0,
                clips: 2,
                first_weight_list: 0,
                weight_lists: 1,
                curve: Typed::PhaseCurves(vec![PhaseCurve {
                    points: vec![[0.0, 0.0]],
                    x0: 0.0,
                    x1: 1.0,
                    y1: 1.0,
                    cycles: 2,
                }]),
                range: Typed::Table(or_io::gta6::anim::TableDatabase {
                    columns: Vec::new(),
                    tetrahedra: Vec::new(),
                    rows: Vec::new(),
                    grid,
                }),
                tags: Vec::new(),
            }],
            parameter_objects: vec![1, 5],
            properties: Vec::new(),
        };
        let lines = lines(&motion);
        assert_eq!(lines.len(), 7, "{lines:#?}");
        assert_eq!(lines[0], "pack:/walk.motion");
        assert_eq!(lines[1], "Time warp 0: a 2, b 2, points (0, 0) (2, 2)");
        assert_eq!(lines[2], "Weight list 0: #1 (0.25, 0.75)");
        assert!(lines[3].contains("words [3, 4], word 0x00000009, from -1 to 1"));
        assert_eq!(lines[4], "Parameter objects, by type: [1, 5]");
        assert!(
            lines[5].ends_with("x from 0 to 1, y1 1, 2 cycles"),
            "{}",
            lines[5]
        );
        assert!(
            lines[6].starts_with("Group 0, grid 0: words [2, 3], cells [[0, 1], [-1, 1], [0, 0]]")
        );
    }
}
