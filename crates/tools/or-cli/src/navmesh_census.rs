//! `openrage-cli navmesh`: over an install, what a navmesh polygon's 21 flag
//! bits and an eight-byte adjacency record hold, and which cells reference
//! which.
//!
//! ⚠ An adjacency split is held to three checks that a wrong split fails:
//! its cell field indexes the cell's own lookup table, its polygon field is
//! under the target cell's polygon count, and a neighbour in the same cell
//! has both corners of the edge. Two control splits, the candidate shifted
//! or narrowed by one bit, run beside it, so the checks are seen failing.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

use or_io::decl::{Element, Owned, Param, Plain, Raw, Resource, View, ViewError};
use or_io::gta5::navmesh::{
    items, AiSplitArray, CNavMesh, CNavMeshCompressedVertex, TAdjPoly, TNavMeshPoly,
};
use or_title::MountedGame;

use super::search::resolve_game_root;

/// Bits `low .. low + width` of a word.
#[derive(Clone, Copy)]
struct Field {
    low: u32,
    width: u32,
}

impl Field {
    fn get(self, word: u32) -> u32 {
        (word >> self.low) & ((1u32 << self.width) - 1)
    }

    fn all_ones(self) -> u32 {
        (1u32 << self.width) - 1
    }
}

/// A reading of an adjacency record's first word as a lookup-table slot and a polygon.
#[derive(Clone, Copy)]
struct Split {
    name: &'static str,
    cell: Field,
    poly: Field,
}

/// The candidate first; the controls, it shifted or narrowed by one bit, must fail.
const SPLITS: [Split; 3] = [
    Split {
        name: "cell 0..5, polygon 5..20",
        cell: Field { low: 0, width: 5 },
        poly: Field { low: 5, width: 15 },
    },
    Split {
        name: "control: cell 1..6, polygon 6..21",
        cell: Field { low: 1, width: 5 },
        poly: Field { low: 6, width: 15 },
    },
    Split {
        name: "control: cell 0..4, polygon 4..19",
        cell: Field { low: 0, width: 4 },
        poly: Field { low: 4, width: 15 },
    },
];

/// Bit ranges above the candidate's polygon field whose values are tallied.
const UPPER: [(usize, Field); 4] = [
    (0, Field { low: 20, width: 2 }),
    (0, Field { low: 22, width: 5 }),
    (0, Field { low: 27, width: 5 }),
    (1, Field { low: 24, width: 8 }),
];

/// The flag bits at the bottom of a polygon's first word.
const FLAG_BITS: usize = 21;

#[derive(Default)]
struct SplitTally {
    edges: usize,
    /// The polygon field all ones: read as no neighbour.
    none: usize,
    /// The cell field names a slot the cell's lookup table fills.
    slot_filled: usize,
    /// The slot names the cell itself.
    to_self: usize,
    to_self_in_range: usize,
    /// A same-cell neighbour holding both corners of the edge from corner k to k + 1, by
    /// position; the rival reading is the edge from k - 1 to k.
    to_self_shares_edge: usize,
    to_self_shares_previous: usize,
    /// The slot names a cell the mount holds.
    to_other_known: usize,
    to_other_in_range: usize,
    /// Target cell → the source cell and polygon field of each edge naming it, checked once every cell is read.
    pending: HashMap<u32, Vec<(u32, u32)>>,
}

/// What the census keeps of one cell for the checks that need every cell.
struct Cell {
    flags: Vec<u32>,
    referenced: Vec<bool>,
    /// The cells its lookup table names, and those its edges reach under the candidate.
    table: Vec<u32>,
    reached: BTreeSet<u32>,
    /// The archive the mount read it from.
    container: String,
}

#[derive(Default)]
struct Census {
    /// Files the mount holds, and copies a later one of the same name replaces.
    seen: usize,
    shadowed: usize,
    unreadable: usize,
    refused: Vec<String>,
    /// Files whose cell index an earlier file took: read, but no edge in another file can reach them.
    sharing: Vec<Cell>,
    /// Edges of the candidate whose polygon is past the target cell's count: (from, to, polygon, count).
    out_of_range: Vec<(u32, u32, u32, usize)>,
    cells: BTreeMap<u32, Cell>,
    containers: BTreeMap<String, usize>,

    polys: usize,
    flag_set: [usize; FLAG_BITS],
    /// No edge of the polygon names a neighbour, under the candidate.
    isolated: usize,
    flag_set_isolated: [usize; FLAG_BITS],
    /// Where the cell's own index sits in its lookup table.
    self_slot: BTreeMap<String, usize>,

    edges: usize,
    word_bits: [[usize; 32]; 2],
    /// The second word's low twenty bits equal the first's.
    low20_twin: usize,
    upper: [BTreeMap<u32, usize>; UPPER.len()],
    splits: [SplitTally; SPLITS.len()],
}

/// A split array's items, or none when the mesh holds no array.
fn list<'a, T: Owned>(
    view: Result<Option<View<'a, Raw<AiSplitArray<T>>>>, ViewError>,
    name: &'static str,
) -> Result<Vec<&'a Raw<T>>, String>
where
    Raw<T>: Plain + Element + Param,
{
    match view.map_err(|e| e.to_string())? {
        Some(l) => items::<T>(&l, name).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

fn bit_set(word: u32, bit: usize) -> usize {
    usize::from(word >> bit & 1 == 1)
}

impl Census {
    fn fold(&mut self, container: &str, bytes: &[u8]) -> Result<(), String> {
        let file = CNavMesh::peek(bytes).map_err(|e| e.to_string())?;
        let root = file.root().map_err(|e| e.to_string())?;
        let vertices = list::<CNavMeshCompressedVertex>(root.compressed_vertex_array(), "vertex")?;
        let indices = list::<u16>(root.vertex_index_array(), "index")?;
        let edges = list::<TAdjPoly>(root.adjacent_polys_array(), "edge")?;
        let polys = list::<TNavMeshPoly>(root.polys_array(), "polygon")?;
        if edges.len() != indices.len() {
            return Err(format!(
                "{} edges against {} indices",
                edges.len(),
                indices.len()
            ));
        }

        let me = root.index_of_mesh;
        let lookups = &root.adjacent_meshes;
        let table: Vec<u32> =
            lookups.navmesh_indices[..(lookups.num_indices as usize).min(32)].to_vec();
        *self.containers.entry(container.to_string()).or_default() += 1;
        *self
            .self_slot
            .entry(
                table
                    .iter()
                    .position(|c| *c == me)
                    .map_or("absent".into(), |i| i.to_string()),
            )
            .or_default() += 1;

        let position = |i: usize| {
            indices
                .get(i)
                .and_then(|v| vertices.get(usize::from(**v)))
                .map(|v| v.data)
        };
        let mut referenced = vec![false; polys.len()];
        let mut reached = BTreeSet::new();
        let mut first = 0usize;
        for p in &polys {
            let n = ((p.as_integer1 >> 21) & 0xf) as usize;
            let flags = p.as_integer1 & 0x1f_ffff;
            self.polys += 1;
            let mut neighbours = 0usize;
            for k in 0..n {
                let at = first + k;
                let Some(edge) = edges.get(at) else {
                    return Err(format!("polygon corner {at} has no edge"));
                };
                let words = [edge.as_integer1, edge.as_integer2];
                for (w, word) in words.iter().enumerate() {
                    for (bit, count) in self.word_bits[w].iter_mut().enumerate().take(32) {
                        *count += bit_set(*word, bit);
                    }
                }
                self.edges += 1;
                self.low20_twin += usize::from(words[0] & 0xf_ffff == words[1] & 0xf_ffff);
                for (u, (w, f)) in UPPER.iter().enumerate() {
                    *self.upper[u].entry(f.get(words[*w])).or_default() += 1;
                }
                let previous = position(first + (k + n - 1) % n);
                let corners = (position(at), position(first + (k + 1) % n));
                for (s, split) in SPLITS.iter().enumerate() {
                    let t = &mut self.splits[s];
                    t.edges += 1;
                    let poly = split.poly.get(words[0]);
                    if poly == split.poly.all_ones() {
                        t.none += 1;
                        continue;
                    }
                    if s == 0 {
                        neighbours += 1;
                    }
                    let Some(target) = table.get(split.cell.get(words[0]) as usize) else {
                        continue;
                    };
                    t.slot_filled += 1;
                    if *target != me {
                        t.pending.entry(*target).or_default().push((me, poly));
                        if s == 0 {
                            reached.insert(*target);
                        }
                        continue;
                    }
                    t.to_self += 1;
                    let Some(q) = polys.get(poly as usize) else {
                        continue;
                    };
                    t.to_self_in_range += 1;
                    if s == 0 {
                        referenced[poly as usize] = true;
                    }
                    let q_first = (q.as_integer2 & 0xffff) as usize;
                    let q_n = ((q.as_integer1 >> 21) & 0xf) as usize;
                    let holds = |c: Option<[u16; 3]>| {
                        c.is_some() && (q_first..q_first + q_n).any(|i| position(i) == c)
                    };
                    t.to_self_shares_edge += usize::from(holds(corners.0) && holds(corners.1));
                    t.to_self_shares_previous += usize::from(holds(previous) && holds(corners.0));
                }
            }
            for (bit, count) in self.flag_set.iter_mut().enumerate().take(FLAG_BITS) {
                *count += bit_set(flags, bit);
            }
            if neighbours == 0 {
                self.isolated += 1;
                for (bit, count) in self
                    .flag_set_isolated
                    .iter_mut()
                    .enumerate()
                    .take(FLAG_BITS)
                {
                    *count += bit_set(flags, bit);
                }
            }
            first += n;
        }
        let cell = Cell {
            flags: polys.iter().map(|p| p.as_integer1 & 0x1f_ffff).collect(),
            referenced,
            table,
            reached,
            container: container.to_string(),
        };
        match self.cells.entry(me) {
            std::collections::btree_map::Entry::Occupied(_) => self.sharing.push(cell),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(cell);
            }
        }
        Ok(())
    }

    /// Checks the edges that leave their cell, now that every cell is read.
    fn settle(&mut self) {
        for (s, t) in self.splits.iter_mut().enumerate() {
            for (target, refs) in std::mem::take(&mut t.pending) {
                let Some(cell) = self.cells.get_mut(&target) else {
                    continue;
                };
                t.to_other_known += refs.len();
                for (from, p) in refs {
                    match cell.referenced.get_mut(p as usize) {
                        Some(r) => {
                            t.to_other_in_range += 1;
                            *r |= s == 0;
                        }
                        None if s == 0 => {
                            self.out_of_range
                                .push((from, target, p, cell.referenced.len()));
                        }
                        None => {}
                    }
                }
            }
        }
    }

    fn report(&self, index: bool) -> String {
        use std::fmt::Write;
        let mut o = String::new();
        let pct = |n: usize, d: usize| {
            if d == 0 {
                0.0
            } else {
                100.0 * n as f64 / d as f64
            }
        };
        let _ = writeln!(o, "Navmesh census");
        let _ = writeln!(
            o,
            "  .ynv in the mount {}, of which a later copy of the same name replaces {}; \
             unreadable {}, refused {}; {} cells read, and {} more files whose cell index an \
             earlier file took",
            self.seen,
            self.shadowed,
            self.unreadable,
            self.refused.len(),
            self.cells.len(),
            self.sharing.len()
        );
        for (c, n) in &self.containers {
            let _ = writeln!(o, "    {n:>6}  {c}");
        }
        for r in self.refused.iter().take(10) {
            let _ = writeln!(o, "  refused: {r}");
        }

        let unreferenced: Vec<[usize; FLAG_BITS + 1]> = self
            .cells
            .values()
            .chain(&self.sharing)
            .map(|c| {
                let mut row = [0usize; FLAG_BITS + 1];
                for (f, r) in c.flags.iter().zip(&c.referenced) {
                    if !r {
                        row[FLAG_BITS] += 1;
                        for (bit, slot) in row.iter_mut().take(FLAG_BITS).enumerate() {
                            *slot += bit_set(*f, bit);
                        }
                    }
                }
                row
            })
            .collect();
        let unref = |bit: usize| unreferenced.iter().map(|r| r[bit]).sum::<usize>();
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "1. Polygon flag bits, over {} polygons; {} have no neighbour on any edge, {} are \
             named by no edge in any cell",
            self.polys,
            self.isolated,
            unref(FLAG_BITS)
        );
        let _ = writeln!(
            o,
            "   bit       set      %   set+isolated   set+unreferenced"
        );
        for bit in 0..FLAG_BITS {
            let _ = writeln!(
                o,
                "   {bit:>3} {:>9} {:>6.2} {:>14} {:>18}",
                self.flag_set[bit],
                pct(self.flag_set[bit], self.polys),
                self.flag_set_isolated[bit],
                unref(bit)
            );
        }

        let _ = writeln!(o);
        let _ = writeln!(o, "2. Adjacency records, over {} edges", self.edges);
        let _ = writeln!(o, "   bit   word 1 set   word 2 set");
        for bit in 0..32 {
            let _ = writeln!(
                o,
                "   {bit:>3} {:>12} {:>12}",
                self.word_bits[0][bit], self.word_bits[1][bit]
            );
        }
        let _ = writeln!(
            o,
            "   word 2's bits 0..20 equal word 1's: {}/{}",
            self.low20_twin, self.edges
        );
        for ((w, f), values) in UPPER.iter().zip(&self.upper) {
            let shown: Vec<String> = values
                .iter()
                .take(12)
                .map(|(v, n)| format!("{v}:{n}"))
                .collect();
            let _ = writeln!(
                o,
                "   word {} bits {}..{}: {} values; {}{}",
                w + 1,
                f.low,
                f.low + f.width,
                values.len(),
                shown.join(" "),
                if values.len() > 12 { " ..." } else { "" }
            );
        }
        let slots: Vec<String> = self
            .self_slot
            .iter()
            .map(|(k, n)| format!("{k}:{n}"))
            .collect();
        let _ = writeln!(
            o,
            "   a cell's own index in its lookup table, by slot: {}",
            slots.join(" ")
        );
        for (split, t) in SPLITS.iter().zip(&self.splits) {
            let named = t.edges - t.none;
            let _ = writeln!(o, "   split {}", split.name);
            let _ = writeln!(
                o,
                "     no neighbour (polygon all ones) {}/{}; slot filled {}/{}",
                t.none, t.edges, t.slot_filled, named
            );
            let _ = writeln!(
                o,
                "     same cell {}: polygon in range {}, shares corners k and k + 1 {}, k - 1 and k {}",
                t.to_self,
                t.to_self_in_range,
                t.to_self_shares_edge,
                t.to_self_shares_previous
            );
            let _ = writeln!(
                o,
                "     other cell {} (read {}): polygon in range {}",
                t.slot_filled - t.to_self,
                t.to_other_known,
                t.to_other_in_range
            );
            let out = named - t.to_self_in_range - t.to_other_in_range;
            let _ = writeln!(
                o,
                "     out of range or unresolved: {out}/{named} ({:.3}%)",
                pct(out, named)
            );
        }
        let mut failing: BTreeMap<(u32, u32), (usize, u32, usize)> = BTreeMap::new();
        for (from, to, poly, count) in &self.out_of_range {
            failing.entry((*from, *to)).or_insert((0, *poly, *count)).0 += 1;
        }
        let _ = writeln!(
            o,
            "   split {}: {} edges past their target cell's polygon count, over {} cell pairs",
            SPLITS[0].name,
            self.out_of_range.len(),
            failing.len()
        );
        let from_of = |c: &u32| self.cells.get(c).map_or("?", |c| c.container.as_str());
        let crossing = failing
            .keys()
            .filter(|(a, b)| from_of(a) != from_of(b))
            .count();
        let _ = writeln!(
            o,
            "     {crossing} of those pairs join cells the mount read from different archives"
        );
        for ((from, to), (n, poly, count)) in failing.iter().take(12) {
            let _ = writeln!(
                o,
                "     {from} -> {to}: {n} edges, such as polygon {poly} of {count}; {} -> {}",
                from_of(from),
                from_of(to)
            );
        }

        let pairs: usize = self.cells.values().map(|c| c.reached.len()).sum();
        let symmetric = self
            .cells
            .iter()
            .flat_map(|(a, c)| c.reached.iter().map(move |b| (*a, *b)))
            .filter(|(a, b)| self.cells.get(b).is_some_and(|c| c.reached.contains(a)))
            .count();
        let table_entries: usize = self
            .cells
            .iter()
            .map(|(me, c)| c.table.iter().filter(|t| *t != me).count())
            .sum();
        let table_reached: usize = self
            .cells
            .values()
            .map(|c| c.reached.iter().filter(|r| c.table.contains(r)).count())
            .sum();
        let mut degree: BTreeMap<usize, usize> = BTreeMap::new();
        for c in self.cells.values() {
            *degree.entry(c.reached.len()).or_default() += 1;
        }
        let degree: Vec<String> = degree.iter().map(|(d, n)| format!("{d}:{n}")).collect();
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "3. Cross-cell references, under split {}: {} cells, {} ordered pairs (a cell's \
             edges reach another), {} of them returned",
            SPLITS[0].name,
            self.cells.len(),
            pairs,
            symmetric
        );
        let _ = writeln!(
            o,
            "   lookup table entries naming another cell {table_entries}, of which an edge \
             reaches {table_reached}"
        );
        let _ = writeln!(
            o,
            "   cells by how many others they reach: {}",
            degree.join(" ")
        );
        if index {
            for (cell, c) in &self.cells {
                let reached: Vec<String> = c.reached.iter().map(u32::to_string).collect();
                let _ = writeln!(o, "   {cell} -> {}", reached.join(" "));
            }
        }
        o
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let mut game: Option<PathBuf> = None;
    let mut keys: Option<PathBuf> = None;
    let mut index = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            "--index" => index = true,
            key @ ("--game" | "--keys") => {
                i += 1;
                let Some(v) = args.get(i) else {
                    return super::usage_error(&format!("{key} needs a value"));
                };
                if key == "--game" {
                    game = Some(PathBuf::from(v));
                } else {
                    keys = Some(PathBuf::from(v));
                }
            }
            other => return super::usage_error(&format!("unknown option '{other}'")),
        }
        i += 1;
    }
    let game_root = match resolve_game_root(game) {
        Ok(root) => root,
        Err(message) => return super::usage_error(&message),
    };
    let game = match super::keys::mount_all(&game_root, keys.as_deref()) {
        Ok(game) => game,
        Err(message) => {
            eprintln!("Error: {message}");
            return ExitCode::FAILURE;
        }
    };
    for finding in game.diagnostics.messages() {
        eprintln!("warning: {finding}");
    }
    let mut census = survey(&game);
    census.settle();
    print!("{}", census.report(index));
    ExitCode::SUCCESS
}

/// Every `.ynv` the game would load: of the copies of one name, the last the mount enumerates.
fn survey(game: &MountedGame) -> Census {
    let mut census = Census::default();
    let mut chosen: BTreeMap<String, (String, String)> = BTreeMap::new();
    game.for_each_file(|logical, container, leaf| {
        let leaf = leaf.to_ascii_lowercase();
        if !leaf.ends_with(".ynv") {
            return;
        }
        census.seen += 1;
        let copy = (logical.to_string(), container.to_string());
        census.shadowed += usize::from(chosen.insert(leaf, copy).is_some());
    });
    for (logical, container) in chosen.values() {
        let Some(bytes) = game.vfs.open(logical) else {
            census.unreadable += 1;
            continue;
        };
        if let Err(e) = census.fold(container, &bytes) {
            census.refused.push(format!("{logical}: {e}"));
        }
    }
    census
}

fn print_usage() {
    eprintln!("Usage: openrage-cli navmesh [--game DIR] [--keys FILE] [--index]");
    eprintln!();
    eprintln!("Reads every .ynv the mounted game loads and prints three tables: how often");
    eprintln!("each polygon flag bit is set, how an adjacency record's bits split into a");
    eprintln!("cell and a polygon (beside shifted controls that must fail), and which cells");
    eprintln!("reference which. --index also prints every cell's references.");
    eprintln!();
    eprintln!("openrage-cli navmesh disable|delete <in.ynv> <polygon> <out.ynv> edits one");
    eprintln!("polygon of a cell; `openrage-cli navmesh disable --help` says how.");
    eprintln!();
    eprintln!("openrage-cli navmesh scan <archive.rpf> prints the same tables over the navigation");
    eprintln!("cells a scan of a GTA VI archive finds.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(polygons: usize) -> Cell {
        Cell {
            flags: vec![0; polygons],
            referenced: vec![false; polygons],
            table: Vec::new(),
            reached: BTreeSet::new(),
            container: String::new(),
        }
    }

    /// An edge naming a polygon past its target cell's count is reported, and one inside it marks the polygon.
    #[test]
    fn an_edge_past_the_target_cells_polygons_is_out_of_range() {
        let mut census = Census::default();
        census.cells.insert(1, cell(2));
        census.cells.insert(2, cell(3));
        census.splits[0].pending.insert(2, vec![(1, 2), (1, 3)]);
        census.settle();
        assert_eq!(census.splits[0].to_other_known, 2);
        assert_eq!(census.splits[0].to_other_in_range, 1);
        assert_eq!(census.out_of_range, vec![(1, 2, 3, 3)]);
        assert_eq!(census.cells[&2].referenced, vec![false, false, true]);
    }
}
