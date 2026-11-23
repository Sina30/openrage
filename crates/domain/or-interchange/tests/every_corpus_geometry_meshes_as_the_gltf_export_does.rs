//! Every staged Legacy `.ydr` opened owned, and each geometry's `mesh()` held
//! to the glTF export's primitive for the same geometry. The export does not
//! take Enhanced files, so there is no Enhanced half.

use std::path::Path;

use or_interchange::{drawable_scene, ExtractOptions, LodSelection, LodSlot};
use or_io::Asset;
use or_test_support::{shared_corpus_root, Corpus};

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn every_corpus_geometry_meshes_as_the_gltf_export_does() {
    let Some(legacy) = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "ydr")
        .find_or_skip("every_corpus_geometry_meshes_as_the_gltf_export_does")
    else {
        return;
    };
    let options = ExtractOptions {
        lods: LodSelection::All,
        textures: false,
    };
    let mut compared = 0usize;
    for path in &legacy {
        let owned = match or_io::open(path) {
            Ok(Asset::Drawable(d)) => d,
            Ok(_) => panic!("{} did not open as a Legacy drawable", name(path)),
            Err(e) => panic!("{}: the owned drawable refused it: {e}", name(path)),
        };
        let scene = drawable_scene(&name(path), &owned, &options);
        let report = &scene.report;
        assert_eq!(report.indices_out_of_range, 0, "{}", name(path));
        assert_eq!(report.geometries_not_triangles, 0, "{}", name(path));
        for primitive in scene
            .objects
            .iter()
            .flat_map(|o| &o.meshes)
            .flat_map(|m| &m.primitives)
        {
            let LodSlot::Level(lod) = primitive.lod else {
                continue;
            };
            let at = format!(
                "{} lod {lod} model {} geometry {}",
                name(path),
                primitive.model,
                primitive.geometry
            );
            let mesh = owned.lod_group.lods[usize::from(lod)]
                .models
                .get(primitive.model)
                .and_then(|m| m.geometries.get(primitive.geometry))
                .unwrap_or_else(|| panic!("{at}: the owned drawable has no such geometry"))
                .mesh();
            assert_eq!(mesh.positions, primitive.positions, "{at}: positions");
            assert_eq!(mesh.indices, primitive.indices, "{at}: indices");
            compared += 1;
        }
    }
    println!("{compared} Legacy primitives from {} files", legacy.len());
    assert!(compared > 0, "no Legacy geometry was compared");
}
