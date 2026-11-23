//! `openrage-cli navmesh disable|delete`: one polygon of a `.ynv`, the result written to a file of its own.

use std::process::ExitCode;

use or_io::decl::File;
use or_io::gta5::CNavMesh;

use super::usage_error;

pub fn run(args: &[String]) -> ExitCode {
    let Some((verb, rest)) = args.split_first() else {
        print_usage();
        return ExitCode::from(2);
    };
    match rest {
        [help] if matches!(help.as_str(), "-h" | "--help" | "help") => {
            print_usage();
            ExitCode::SUCCESS
        }
        [from, polygon, to] => {
            let Ok(polygon) = polygon.parse::<usize>() else {
                return usage_error(&format!("'{polygon}' is not a polygon number"));
            };
            if same_file(from, to) {
                return usage_error("the output is the input: name another file");
            }
            match edit_file(verb, from, polygon, to) {
                Ok(said) => {
                    println!("{said}");
                    ExitCode::SUCCESS
                }
                Err(why) => {
                    eprintln!("Error: {why}");
                    ExitCode::FAILURE
                }
            }
        }
        // GTA VI's form puts the polygon third, after an offset, where GTA V's puts its output.
        [_, _, polygon, _, ..] if polygon.parse::<usize>().is_ok() => {
            super::gta6_navmesh_edit::run(verb, rest)
        }
        _ => usage_error(&format!("{verb} needs <in.ynv> <polygon> <out.ynv>")),
    }
}

pub(crate) fn same_file(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn edit_file(verb: &str, from: &str, polygon: usize, to: &str) -> Result<String, String> {
    let bytes = std::fs::read(from).map_err(|e| format!("read {from}: {e}"))?;
    let (saved, said) = edit(verb, &bytes, polygon).map_err(|e| format!("{from}: {e}"))?;
    std::fs::write(to, saved).map_err(|e| format!("write {to}: {e}"))?;
    Ok(format!("{said}; wrote {to}"))
}

/// The mesh `bytes` hold with `polygon` disabled or deleted, saved, and what was done; a refusal writes nothing.
fn edit(verb: &str, bytes: &[u8], polygon: usize) -> Result<(Vec<u8>, String), String> {
    let mut mesh = CNavMesh::from_bytes(bytes).map_err(|e| e.to_string())?;
    let said = if verb == "disable" {
        let cut = mesh.disable_polygon(polygon).map_err(|e| e.to_string())?;
        let left = mesh.references(polygon).map_err(|e| e.to_string())?;
        let mut said = match cut {
            0 => format!("polygon {polygon} already has no edge into or out of it"),
            n => format!("polygon {polygon} disabled: {n} edges now lead nowhere"),
        };
        if !left.is_empty() {
            said.push_str(&format!(". Still reaching it: {left}"));
        }
        said
    } else {
        mesh.delete_polygon(polygon).map_err(|e| e.to_string())?;
        let left = mesh.polys_array.as_deref().map_or(0, |l| l.len());
        format!("polygon {polygon} deleted: {left} polygons remain")
    };
    Ok((mesh.to_bytes().map_err(|e| e.to_string())?, said))
}

fn print_usage() {
    eprintln!("Usage: openrage-cli navmesh disable <in.ynv> <polygon> <out.ynv>");
    eprintln!("       openrage-cli navmesh delete  <in.ynv> <polygon> <out.ynv>");
    eprintln!();
    eprintln!("disable cuts a polygon off: every edge out of it, and every edge of this cell");
    eprintln!("into it, leads nowhere. Cells that list this one may still have edges into it,");
    eprintln!("and special links and sectors still name it; the command lists what it knows.");
    eprintln!("How the game treats a disabled polygon is untested.");
    eprintln!();
    eprintln!("delete removes a polygon and renumbers every reference to a later one, or");
    eprintln!("refuses, naming the edges, special links and cells that would break, and");
    eprintln!("writes nothing. A cell that lists another is always refused: that cell's own");
    eprintln!("edges name this one's polygons by number. Disable instead.");
    eprintln!();
    eprintln!("The result goes to <out.ynv>, never into the input.");
    eprintln!();
    eprintln!(
        "       openrage-cli navmesh disable|delete <archive.rpf|file> <offset> <polygon> <out>"
    );
    eprintln!("                    [--neighbour <offset>]... [--limit <bytes>]");
    eprintln!("A scanned or loose GTA VI cell, as the editor edits it, written as a loose");
    eprintln!("resource that `rpf scan-open <out> 0` reads. A deletion needs the body of every");
    eprintln!("cell across its borders from the same archive, each named by --neighbour.");
}

#[cfg(test)]
mod tests {
    use or_io::decl::{Element, Owned, Param, Plain, Raw};
    use or_io::gta5::navmesh::NO_NEIGHBOUR;
    use or_io::gta5::{AiSplitArray, CNavMeshCompressedVertex, SubArray, TAdjPoly, TNavMeshPoly};

    use super::*;

    fn split<T: Owned>(elements: Vec<T>) -> Option<Box<AiSplitArray<T>>>
    where
        Raw<T>: Plain + Element + Param,
    {
        let sub_arrays = vec![SubArray {
            elements,
            ..Default::default()
        }];
        Some(Box::new(AiSplitArray {
            sub_arrays,
            ..Default::default()
        }))
    }

    /// Four triangles in a strip, in cell 7: corner 0 of polygon i leads to i + 1 and corner 1 to i - 1;
    /// when `next`, the table lists cell 8 and polygon 0's corner 2 leads into it.
    fn strip(next: bool) -> Vec<u8> {
        let mut triangle = TNavMeshPoly::default();
        triangle.set_num_vertices(3).unwrap();
        let to = |slot, polygon| TAdjPoly::leading_to(slot, polygon).unwrap();
        let nowhere = to(1, NO_NEIGHBOUR);
        let mut edges = Vec::new();
        for i in 0..4usize {
            edges.push(if i < 3 { to(0, i + 1) } else { nowhere.clone() });
            edges.push(if i > 0 { to(0, i - 1) } else { nowhere.clone() });
            edges.push(if i == 0 && next {
                to(1, 5)
            } else {
                nowhere.clone()
            });
        }
        let mut mesh = CNavMesh {
            index_of_mesh: 7,
            compressed_vertex_array: split(vec![CNavMeshCompressedVertex::default(); 3]),
            vertex_index_array: split((0..12).map(|i| i % 3).collect()),
            adjacent_polys_array: split(edges),
            polys_array: split(vec![triangle; 4]),
            ..Default::default()
        };
        mesh.adjacent_meshes.navmesh_indices[0] = 7;
        mesh.adjacent_meshes.navmesh_indices[1] = if next { 8 } else { 0 };
        mesh.to_bytes().expect("the mesh saves")
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn disable_writes_a_mesh_whose_polygon_is_cut_off() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("in.ynv"), dir.path().join("out.ynv"));
        std::fs::write(&from, strip(false)).unwrap();
        let words = ["disable", from.to_str().unwrap(), "0", to.to_str().unwrap()];
        assert_eq!(run(&args(&words)), ExitCode::SUCCESS);
        let mesh = CNavMesh::from_bytes(&std::fs::read(&to).unwrap()).unwrap();
        assert_eq!(mesh.disabled_polygons(), [true, false, false, false]);
        let was = CNavMesh::from_bytes(&std::fs::read(&from).unwrap()).unwrap();
        assert_eq!(
            was.disabled_polygons(),
            [false; 4],
            "the input is untouched"
        );
    }

    #[test]
    fn a_disable_says_how_many_edges_it_cut_and_what_still_reaches_the_polygon() {
        // Polygon 0's edge to 1, its edge into the next cell, and 1's edge back to it.
        let (_, said) = edit("disable", &strip(true), 0).unwrap();
        assert!(
            said.starts_with("polygon 0 disabled: 3 edges now lead nowhere")
                && said.contains("Still reaching it")
                && said.contains("cell 8"),
            "{said}"
        );
    }

    #[test]
    fn a_deletion_is_refused_naming_what_leads_in_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("in.ynv"), dir.path().join("out.ynv"));
        std::fs::write(&from, strip(false)).unwrap();
        let words = ["delete", from.to_str().unwrap(), "1", to.to_str().unwrap()];
        assert_eq!(run(&args(&words)), ExitCode::FAILURE);
        assert!(!to.exists(), "a refusal wrote a file");
        let why = edit("delete", &strip(false), 1).unwrap_err();
        assert!(why.contains("polygon 0 corner 0"), "{why}");
        let why = edit("delete", &strip(true), 1).unwrap_err();
        assert!(why.contains("cell 8"), "{why}");
    }

    #[test]
    fn a_disabled_polygon_is_then_deleted() {
        let (disabled, _) = edit("disable", &strip(false), 1).unwrap();
        let (deleted, said) = edit("delete", &disabled, 1).unwrap();
        assert_eq!(said, "polygon 1 deleted: 3 polygons remain");
        let mesh = CNavMesh::from_bytes(&deleted).unwrap();
        assert_eq!(mesh.disabled_polygons(), [true, false, false]);
    }

    #[test]
    fn a_polygon_past_the_end_and_an_output_that_is_the_input_are_refused() {
        let why = edit("disable", &strip(false), 9).unwrap_err();
        assert!(why.contains("no polygon 9"), "{why}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("in.ynv");
        std::fs::write(&path, strip(false)).unwrap();
        let p = path.to_str().unwrap();
        assert_eq!(run(&args(&["disable", p, "0", p])), ExitCode::from(2));
        assert_eq!(
            run(&args(&["delete", p, "x", "out.ynv"])),
            ExitCode::from(2)
        );
    }
}
