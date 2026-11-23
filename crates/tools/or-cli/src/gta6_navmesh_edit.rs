//! `openrage-cli navmesh disable|delete` over a scanned or loose GTA VI cell: one polygon proper
//! cut off or deleted as the editor does it, the cell written as a loose resource that
//! `rpf scan-open` reads at offset 0.

use std::fs::File;
use std::io::Cursor;
use std::process::ExitCode;

use or_io::container::rpf::{
    read_rpf9_body, write_rpf9_resource, Rpf9BodyKind, Rpf9Role, Rpf9Sections,
};
use or_io::gta6::headerless::{
    cut_edges, delete_polygon, links_into, polygon_cut, read_navmesh, Border, NavMesh, NOWHERE,
};

const USAGE: &str = "navmesh disable|delete <archive.rpf|file> <offset> <polygon> <out> \
                     [--neighbour <offset>]... [--limit <bytes>]";

const BORDERS: [Border; 4] = [Border::East, Border::West, Border::North, Border::South];

pub(crate) fn run(verb: &str, args: &[String]) -> ExitCode {
    let (limit, neighbours, path, offset, polygon, out) = match parse(args) {
        Ok(parsed) => parsed,
        Err(why) => return super::usage_error(&why),
    };
    if super::navmesh_edit::same_file(&path, &out) {
        return super::usage_error(&format!(
            "the output is the input {path}: name another file"
        ));
    }
    match edit_body(verb, &path, offset, polygon, &neighbours, limit) {
        Ok((saved, lines)) => match std::fs::write(&out, &saved) {
            Ok(()) => {
                for line in lines {
                    println!("{line}");
                }
                println!("wrote {out} ({} bytes)", saved.len());
                println!("This checks the cell's bytes. It does not test that GTA VI loads them.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: write {out}: {e}");
                ExitCode::FAILURE
            }
        },
        Err(why) => {
            eprintln!("Error: {path} at {offset:#X}: {why}");
            ExitCode::FAILURE
        }
    }
}

type Parsed = (u64, Vec<u64>, String, u64, u64, String);

/// The `--limit`, every `--neighbour` offset, the path, the offset, the polygon and the output.
fn parse(args: &[String]) -> Result<Parsed, String> {
    let (limit, mut rest) = super::rpf::split_limit(args)?;
    let mut neighbours = Vec::new();
    while let (Some(word), next) = super::rpf::split_value(&rest, "--neighbour")? {
        neighbours.push(super::rpf::parse_offset(&word)?);
        rest = next;
    }
    let [path, offset, polygon, out] = rest.as_slice() else {
        return Err(USAGE.to_string());
    };
    let at = super::rpf::parse_offset(offset)?;
    let Ok(number) = polygon.parse::<u64>() else {
        return Err(format!("'{polygon}' is not a polygon number"));
    };
    Ok((limit, neighbours, path.clone(), at, number, out.clone()))
}

/// The cell body at `offset` of the file at `path` and its sections.
fn cell_at(file: &File, offset: u64, limit: u64) -> Result<(Rpf9Sections, NavMesh), String> {
    let (find, bytes) = read_rpf9_body(file, offset, limit).map_err(|e| e.to_string())?;
    let role = match find.kind {
        Rpf9BodyKind::Zlib(run) => run.signature.and_then(|s| s.role()),
        _ => None,
    };
    if role != Some(Rpf9Role::NavMesh) {
        return Err(format!(
            "the body at {offset:#X} is no GTA VI navigation cell"
        ));
    }
    let sections = Rpf9Sections::of(&find, &bytes)
        .ok_or_else(|| format!("the body at {offset:#X} is not a resource"))?;
    let mesh = read_navmesh(&sections.virtual_section).map_err(|e| e.to_string())?;
    Ok((sections, mesh))
}

/// Indices as a person reads a list of them.
fn listed(items: impl IntoIterator<Item = impl ToString>) -> String {
    let items: Vec<String> = items.into_iter().map(|i| i.to_string()).collect();
    items.join(", ")
}

/// The loose resource the edit makes of the cell at `offset`, read back, and what it did.
fn edit_body(
    verb: &str,
    path: &str,
    offset: u64,
    polygon: u64,
    neighbours: &[u64],
    limit: u64,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let file = File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (mut sections, cell) = cell_at(&file, offset, limit)?;
    let polygon = usize::try_from(polygon).map_err(|_| format!("no polygon {polygon}"))?;
    let mut near = Vec::new();
    for &at in neighbours {
        let (_, body) = cell_at(&file, at, limit).map_err(|e| format!("--neighbour: {e}"))?;
        let across = BORDERS
            .iter()
            .any(|b| b.across(cell.cell) == Some(body.cell));
        if !across {
            return Err(format!(
                "--neighbour {at:#X} is cell {}, which is not across a border of cell {}",
                body.cell, cell.cell
            ));
        }
        near.push(body);
    }
    let lines = match verb {
        "disable" => disable(&mut sections, &cell, polygon, &near)?,
        _ => delete(&mut sections, &cell, polygon, &near)?,
    };
    let saved = write_rpf9_resource(&sections).map_err(|e| e.to_string())?;
    // The file is written only once it reads back as the cell the edit made.
    let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    let back = Rpf9Sections::of(&find, &bytes).ok_or("the written resource does not read back")?;
    read_navmesh(&back.virtual_section)
        .map_err(|e| format!("the written cell does not read back: {e}"))?;
    Ok((saved, lines))
}

/// Polygon proper `polygon` cut off as the editor cuts it: every edge into or out of it that names a
/// polygon of this cell written as leading nowhere.
fn disable(
    sections: &mut Rpf9Sections,
    cell: &NavMesh,
    polygon: usize,
    near: &[NavMesh],
) -> Result<Vec<String>, String> {
    let cut = polygon_cut(cell, polygon).map_err(|e| e.to_string())?;
    let edges: Vec<usize> = (cut.edges.iter().copied())
        .filter(|&j| cell.edges.get(j).is_some_and(|e| e.mesh == cell.cell))
        .collect();
    cut_edges(&mut sections.virtual_section, &edges).map_err(|e| e.to_string())?;
    let mut lines = vec![match edges.len() {
        0 => format!("polygon {polygon} has no edge into or out of it left to cut"),
        n => format!("polygon {polygon} disabled: {n} edges now lead nowhere"),
    }];
    if !cut.side_records.is_empty() {
        lines.push(format!(
            "side records {} still name it at their corner 1, which every shipped side record leads somewhere from, so it still joins the next cell across that border",
            listed(&cut.side_records)
        ));
    }
    if !cut.links.is_empty() {
        lines.push(format!(
            "special links {} of this cell still end in it",
            listed(&cut.links)
        ));
    }
    for body in near {
        let links = links_into(body, cell.cell, polygon);
        if !links.is_empty() {
            lines.push(format!(
                "special links {} of the neighbouring cell {} still end in it",
                listed(&links),
                body.cell
            ));
        }
    }
    let read: Vec<u32> = near.iter().map(|b| b.cell).collect();
    let unread = across(cell).filter(|c| !read.contains(c));
    let unread: Vec<u32> = unread.collect();
    if !unread.is_empty() {
        lines.push(format!(
            "the bodies of the neighbouring cells {} were not given with --neighbour, so whether their special links end in it is not known",
            listed(&unread)
        ));
    }
    Ok(lines)
}

/// The cells across `cell`'s borders.
fn across(cell: &NavMesh) -> impl Iterator<Item = u32> + '_ {
    BORDERS.into_iter().filter_map(|b| b.across(cell.cell))
}

/// Polygon proper `polygon` deleted as the editor deletes it, once every cell across a border is
/// given, since their special links number this cell's polygons.
fn delete(
    sections: &mut Rpf9Sections,
    cell: &NavMesh,
    polygon: usize,
    near: &[NavMesh],
) -> Result<Vec<String>, String> {
    let given: Vec<u32> = near.iter().map(|b| b.cell).collect();
    let missing: Vec<u32> = across(cell).filter(|c| !given.contains(c)).collect();
    if !missing.is_empty() {
        return Err(format!(
            "polygon {polygon} is not deleted while the bodies of the neighbouring cells {} are not given with --neighbour <offset>: their special links may number this cell's polygons",
            listed(&missing)
        ));
    }
    let all: Vec<u32> = across(cell).collect();
    for (i, l) in cell.special_links.iter().enumerate() {
        for mesh in [l.from_mesh, l.to_mesh] {
            if mesh != cell.cell && mesh != NOWHERE && !all.contains(&mesh) {
                return Err(format!(
                    "polygon {polygon} is not deleted: special link {i} ends in cell {mesh}, which is not across a border of this one"
                ));
            }
        }
    }
    let others: Vec<&NavMesh> = near.iter().collect();
    let after = delete_polygon(sections, polygon, &others).map_err(|e| e.to_string())?;
    Ok(vec![format!(
        "polygon {polygon} deleted: {} polygons remain",
        after.polygons.len()
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::Rpf9Form;
    use or_io::gta6::headerless::NavEdge;

    /// The cell's grid place, which the built cell states at 0xC8.
    const CELL: u32 = 256 + 512 * 256;

    fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
        v[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn pointer(v: &mut [u8], at: usize, to: usize) {
        put(v, at, &(0x5000_0000 + to as u64).to_le_bytes());
    }

    /// The virtual section of a cell of one water triangle over 128 m, at grid place `cell`, whose
    /// first edge leads back into the triangle.
    fn cell_section(cell: u32) -> Vec<u8> {
        let mut v = vec![0u8; 0x360];
        // A split array at `at` of one chunk: header, chunk row, start, then the items.
        let list = |v: &mut Vec<u8>, at: usize, items: &[u8], stride: usize| {
            let count = (items.len() / stride) as u64;
            put(v, at + 8, &count.to_le_bytes());
            pointer(v, at + 0x10, at + 0x30);
            pointer(v, at + 0x18, at + 0x40);
            put(v, at + 0x20, &1u64.to_le_bytes());
            pointer(v, at + 0x30, at + 0x50);
            put(v, at + 0x38, &count.to_le_bytes());
            put(v, at + 0x50, items);
        };
        pointer(&mut v, 0x08, 0xF0);
        v[0xF8] = 1;
        put(&mut v, 0x10, &9u32.to_le_bytes());
        put(&mut v, 0x14, &0x0001_0015u32.to_le_bytes());
        for at in [0x20, 0x34, 0x48] {
            put(&mut v, at, &1.0f32.to_le_bytes());
        }
        let vertices: Vec<u8> = [0u16, 0, 0, 65535, 0, 0, 0, 65535, 0]
            .iter()
            .flat_map(|q| q.to_le_bytes())
            .collect();
        list(&mut v, 0x100, &vertices, 6);
        list(&mut v, 0x170, &[0, 0, 1, 0, 2, 0], 2);
        let edges = [[0xFF; 4], [0; 4], [0; 4]].repeat(3).concat();
        list(&mut v, 0x1D0, &edges, 12);
        let mut polygon = [0u8; 44];
        polygon[4] = 3;
        polygon[8] = 0x80;
        list(&mut v, 0x250, &polygon, 44);
        for (slot, at) in [
            (0x70, 0x100),
            (0x80, 0x170),
            (0x88, 0x1D0),
            (0x98, 0x250),
            (0xA0, 0x2D0),
        ] {
            pointer(&mut v, slot, at);
        }
        put(&mut v, 0x90, &3u32.to_le_bytes());
        for (at, x) in [(0x2E0, 128.0f32), (0x2E4, 128.0), (0x2E8, 4.0)] {
            put(&mut v, at, &x.to_le_bytes());
        }
        pointer(&mut v, 0x2D0 + 0x2C, 0x330);
        pointer(&mut v, 0x330 + 0x08, 0x350);
        v[0x330 + 0x18] = 1;
        for (at, word) in [
            (0xC0, 3),
            (0xC4, 1),
            (0xC8, cell),
            (0xCC, 6 * 3 + 14 * 3 + 44),
        ] {
            put(&mut v, at, &word.to_le_bytes());
        }
        // The adjacency list's items start 0x50 into its header at 0x1D0.
        put(&mut v, 0x220, &cell.to_le_bytes());
        put(&mut v, 0x224, &0u16.to_le_bytes());
        put(&mut v, 0x226, &0x1207u16.to_le_bytes());
        v
    }

    /// The cell at `cell` as a loose resource in `dir`.
    fn loose(dir: &std::path::Path, cell: u32) -> String {
        let sections = Rpf9Sections {
            form: Rpf9Form::BlockMapped,
            virtual_section: cell_section(cell),
            physical: Vec::new(),
        };
        let path = dir.join(format!("cell{cell}.rsc"));
        std::fs::write(&path, write_rpf9_resource(&sections).expect("written")).expect("saved");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_disabled_scanned_cell_saves_its_edge_into_the_polygon_cut_and_its_flags_kept() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = loose(dir.path(), CELL);
        let (saved, lines) = edit_body("disable", &path, 0, 0, &[], u64::MAX).expect("disabled");
        assert_eq!(lines[0], "polygon 0 disabled: 1 edges now lead nowhere");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("not given with --neighbour")),
            "{lines:?}"
        );
        let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX).expect("it reads");
        let back = Rpf9Sections::of(&find, &bytes).expect("a resource");
        let cell = read_navmesh(&back.virtual_section).expect("the cell reads");
        assert_eq!(
            cell.edges[0],
            NavEdge {
                mesh: NOWHERE,
                polygon: 0xFFFF,
                word_06: 0x1200,
                flags: 0,
            }
        );
        assert_eq!(cell.polygons[0].flags, 0x80, "the water bit as read");
        let (_, again) = edit_body("disable", &path, 0, 0, &[], u64::MAX).expect("said");
        assert!(
            again[0].contains("polygon 0 disabled"),
            "the input is untouched"
        );
    }

    #[test]
    fn a_deletion_without_every_neighbouring_cell_is_refused_naming_them() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = loose(dir.path(), CELL);
        let why = edit_body("delete", &path, 0, 0, &[], u64::MAX).expect_err("refused");
        let cells = [CELL + 1, CELL - 1, CELL + 512, CELL - 512].map(|c| c.to_string());
        assert!(why.contains(&cells.join(", ")), "{why}");
    }

    #[test]
    fn a_neighbour_not_across_a_border_a_side_record_and_an_output_that_is_the_input_are_refused() {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = loose(dir.path(), CELL);
        let why = edit_body("disable", &path, 0, 0, &[0], u64::MAX).expect_err("refused");
        assert!(why.contains("not across a border"), "{why}");
        let why = edit_body("disable", &path, 0, 1, &[], u64::MAX).expect_err("refused");
        assert!(why.contains("side record"), "{why}");
        let words = ["disable", path.as_str(), "0", "0", path.as_str()].map(String::from);
        assert_eq!(super::super::navmesh_edit::run(&words), ExitCode::from(2));
        let words = [path.as_str(), "0", "x", "out.rsc"].map(String::from);
        assert!(parse(&words)
            .expect_err("refused")
            .contains("polygon number"));
    }
}
