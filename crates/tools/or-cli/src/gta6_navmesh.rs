//! `openrage-cli navmesh scan`: over the navigation cells a scan of a GTA VI archive finds, how
//! often each polygon flag bit is set, what the adjacency records lead to and which neighbouring
//! cells join across their border, structure and counts only.

use std::collections::BTreeMap;
use std::fs::File;
use std::process::ExitCode;

use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9Role, Rpf9Scan, Rpf9ZlibRun};
use or_io::gta5::navmesh::NO_NEIGHBOUR;
use or_io::gta6::headerless::{join_edges, read_navmesh, Border, BorderEdge, NavMesh, NOWHERE};

/// The most a cell's body inflates to.
const BODY_LIMIT: u64 = 1 << 30;

/// What the census keeps of one cell for the joins, which need every cell: its border edges.
type Cell = Vec<BorderEdge>;

#[derive(Default)]
struct Census {
    bodies: usize,
    /// Why a body did not read, with how many and the first offset.
    refused: BTreeMap<String, (usize, u64)>,
    /// Cells of more polygons than GTA V's view names across an edge.
    over: usize,
    /// Every body of a cell, in archive order.
    cells: BTreeMap<u32, Vec<Cell>>,
    polygons: usize,
    sides: usize,
    flag_set: [usize; 32],
    edges: usize,
    in_cell: usize,
    nowhere: usize,
    border_named: usize,
    border_unnamed: usize,
}

impl Census {
    fn fold(&mut self, m: &NavMesh) {
        self.over += usize::from(m.polygons.len() > NO_NEIGHBOUR);
        let proper = m.polygons_proper.min(m.polygons.len());
        self.polygons += proper;
        self.sides += m.polygons.len() - proper;
        for p in &m.polygons[..proper] {
            for (bit, count) in self.flag_set.iter_mut().enumerate() {
                *count += usize::from(p.flags >> bit & 1 == 1);
            }
        }
        self.edges += m.edges.len();
        self.nowhere += m.edges.iter().filter(|e| e.mesh == NOWHERE).count();
        self.in_cell += m.edges.iter().filter(|e| e.mesh != NOWHERE).count();
        let edges = m.border_edges();
        let named = edges.iter().filter(|e| e.polygon.is_some()).count();
        self.border_named += named;
        self.border_unnamed += edges.len() - named;
        self.cells.entry(m.cell).or_default().push(edges);
    }

    fn refuse(&mut self, offset: u64, why: String) {
        self.refused.entry(why).or_insert((0, offset)).0 += 1;
    }

    /// The body a neighbour joins: the second of a repeated cell, in archive order.
    fn joined(&self, cell: u32) -> Option<&Cell> {
        let bodies = self.cells.get(&cell)?;
        bodies.get(1).or(bodies.first())
    }

    fn report(&self) -> String {
        use std::fmt::Write;
        let mut o = String::new();
        let read: usize = self.cells.values().map(Vec::len).sum();
        let repeated = self.cells.values().filter(|b| b.len() > 1).count();
        let _ = writeln!(o, "Navigation census of a scan");
        let _ = writeln!(
            o,
            "  bodies        {} of role navigation mesh; {read} read, {} refused; {} cells, {repeated} of them stated by more than one body, whose neighbours join the second in archive order",
            self.bodies,
            self.refused.values().map(|r| r.0).sum::<usize>(),
            self.cells.len(),
        );
        let _ = writeln!(
            o,
            "  over          {} cells of more than {NO_NEIGHBOUR} polygons, which GTA V's view cannot name across an edge, so the app's view leaves an edge past it leading nowhere",
            self.over
        );
        for (why, (n, at)) in &self.refused {
            let _ = writeln!(o, "  refused       {n}, the first at {at:#X}: {why}");
        }
        let pct = |n: usize, d: usize| 100.0 * n as f64 / d.max(1) as f64;
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "1. Polygon flag bits at +0x08, over {} polygons; {} side records after them",
            self.polygons, self.sides
        );
        let _ = writeln!(o, "   bit       set      %");
        for (bit, n) in self.flag_set.iter().enumerate().filter(|(_, n)| **n > 0) {
            let _ = writeln!(o, "   {bit:>3} {n:>9} {:>6.2}", pct(*n, self.polygons));
        }
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "2. Adjacency records, over {} edges: {} into the cell's own polygons, {} without neighbour",
            self.edges, self.in_cell, self.nowhere
        );
        let _ = writeln!(
            o,
            "   side records' chain edges: {} name the polygon on this side, {} name none",
            self.border_named, self.border_unnamed
        );
        let (mut pairs, mut joined, mut joins, mut mirrored) = (0, 0, 0, 0);
        let mut degree: BTreeMap<u32, usize> = BTreeMap::new();
        for &cell in self.cells.keys() {
            let Some(a) = self.joined(cell) else { continue };
            for border in [Border::East, Border::North] {
                let Some(other) = border.across(cell) else {
                    continue;
                };
                let Some(b) = self.joined(other) else {
                    continue;
                };
                pairs += 1;
                let there = join_edges((cell, a), (other, b));
                let back = join_edges((other, b), (cell, a));
                let mut ours: Vec<_> = there.iter().map(|j| (j.polygon, j.other)).collect();
                let mut theirs: Vec<_> = back.iter().map(|j| (j.other, j.polygon)).collect();
                ours.sort_unstable();
                theirs.sort_unstable();
                mirrored += usize::from(ours == theirs);
                if !there.is_empty() {
                    joined += 1;
                    *degree.entry(cell).or_default() += 1;
                    *degree.entry(other).or_default() += 1;
                }
                joins += there.len();
            }
        }
        let mut by_degree: BTreeMap<usize, usize> = BTreeMap::new();
        for &cell in self.cells.keys() {
            *by_degree
                .entry(degree.get(&cell).copied().unwrap_or(0))
                .or_default() += 1;
        }
        let by_degree: Vec<String> = by_degree.iter().map(|(d, n)| format!("{d}:{n}")).collect();
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "3. Cross-cell joins: {pairs} pairs of neighbouring cells both read, {joined} of them with a polygon that meets one across the border, {joins} polygon pairs in all; {mirrored} pairs give the same joins read from either side"
        );
        let _ = writeln!(
            o,
            "   cells by how many neighbours they join: {}",
            by_degree.join(" ")
        );
        o
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let [path] = args else {
        print_usage();
        return ExitCode::from(2);
    };
    match census(path) {
        Ok(census) => {
            print!("{}", census.report());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Every navigation cell a scan of the archive at `path` finds, read.
fn census(path: &str) -> Result<Census, String> {
    let file = File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let scan = Rpf9Scan::new(&file, 0..u64::MAX).map_err(|e| e.to_string())?;
    let mut census = Census::default();
    let mut cells = Vec::new();
    for find in scan {
        let find = find.map_err(|e| e.to_string())?;
        if let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            signature: Some(s),
            resource: Some(_),
            ..
        }) = find.kind
        {
            if s.role() == Some(Rpf9Role::NavMesh) {
                cells.push(find.offset);
            }
        }
    }
    census.bodies = cells.len();
    for offset in cells {
        let read = read_rpf9_body(&file, offset, BODY_LIMIT).map_err(|e| e.to_string());
        let mesh = read.and_then(|(find, bytes)| match find.kind {
            Rpf9BodyKind::Zlib(Rpf9ZlibRun {
                resource: Some(resource),
                ..
            }) => read_navmesh(resource.sections(&bytes).0).map_err(|e| e.to_string()),
            _ => Err("no headerless resource starts here".to_string()),
        });
        match mesh {
            Ok(m) => census.fold(&m),
            Err(why) => census.refuse(offset, why),
        }
    }
    Ok(census)
}

fn print_usage() {
    eprintln!("Usage: openrage-cli navmesh scan <archive.rpf>");
    eprintln!();
    eprintln!("Reads every navigation cell a scan of a GTA VI archive finds and prints how often");
    eprintln!("each polygon flag bit is set, what the adjacency records lead to, and which");
    eprintln!("neighbouring cells join across their border; the cells refused and those of more");
    eprintln!("polygons than GTA V's view names.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::headerless::{NavEdge, NavNode, NavPolygon, SIDE_RECORD};

    /// A cell at grid (x, 7) of one triangle with a corner on its east or west border, and one side
    /// record on that border at height `z`, both its chain edges naming the triangle.
    fn cell(x: u32, east: bool, z: u16, flags: u32) -> NavMesh {
        let cell = x + 512 * 7;
        let line = if east { 65535 } else { 0 };
        let polygon = |first, corners, side_flags| NavPolygon {
            first,
            corners,
            flags,
            min_max: [0; 6],
            centroid: [0; 2],
            side_flags,
            steepness: 0,
            word_20: 0,
        };
        let edge = |mesh, polygon| NavEdge {
            mesh,
            polygon,
            word_06: 0,
            flags: 0,
        };
        NavMesh {
            flags: 1,
            cell,
            build_id: 0,
            tree: NavNode {
                box_min: [0.0; 3],
                box_max: [128.0, 128.0, 8.0],
                polygons: None,
                children: Default::default(),
            },
            vertices: vec![
                [line, 0, z],
                [line, 30000, z],
                [32768, 0, z],
                [line, 0, z],
                [line, 30000, z],
                [line, 15000, z],
            ],
            indices: vec![0, 1, 2, 3, 4, 5],
            edges: vec![
                edge(cell, 0),
                edge(NOWHERE, 0),
                edge(NOWHERE, 0),
                edge(NOWHERE, 0),
                edge(cell, 0),
                edge(cell, 0),
            ],
            polygons: vec![polygon(0, 3, 0), polygon(3, 3, SIDE_RECORD)],
            polygons_proper: 1,
            nodes: 1,
            depth: 0,
            leaves: 1,
            special_links: Vec::new(),
            special_link_indices: Vec::new(),
            footprints: Vec::new(),
        }
    }

    #[test]
    fn the_census_tallies_flags_and_joins_a_repeated_cell_s_second_body() {
        let mut census = Census {
            bodies: 4,
            ..Census::default()
        };
        census.fold(&cell(3, true, 1000, 0x81));
        // The first body of cell 4 lies at another height, so only its second body joins.
        census.fold(&cell(4, false, 9000, 0x01));
        census.fold(&cell(4, false, 1000, 0x01));
        census.refuse(0x400, "a made-up refusal".to_string());
        let text = census.report();
        for said in [
            "4 of role navigation mesh; 3 read, 1 refused; 2 cells, 1 of them stated by more than one body",
            "refused       1, the first at 0x400: a made-up refusal",
            "     0         3 100.00",
            "     7         1  33.33",
            "over 18 edges: 9 into the cell's own polygons, 9 without neighbour",
            "6 name the polygon on this side, 0 name none",
            "1 pairs of neighbouring cells both read, 1 of them with a polygon that meets one across the border, 1 polygon pairs in all; 1 pairs give the same joins",
            "cells by how many neighbours they join: 1:2",
        ] {
            assert!(text.contains(said), "{said}\n{text}");
        }
        let mut lone = Census::default();
        lone.fold(&cell(3, true, 1000, 0));
        lone.fold(&cell(4, false, 9000, 0));
        assert!(lone
            .report()
            .contains("1 pairs of neighbouring cells both read, 0 of them"));
    }
}
