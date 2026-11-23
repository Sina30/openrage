//! A fragment's extra drawable has no shader group of its own and indexes the
//! one of the common drawable, so exporting the fragment gives each of its
//! primitives a material, and none is left with `None`.

use or_cli::export_scene;
use or_interchange::extract::{ExtractOptions, LodSelection};
use or_io::decl::{File, Gen8};
use or_io::gta5::GtaFragType;
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const TEST: &str = "a_fragments_extra_drawables_read_the_materials_of_its_common_one";

#[test]
fn a_primitive_of_an_extra_drawable_has_the_material_its_shader_index_names() {
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "yft").target(TARGET_GEN8).find();
    if files.is_empty() {
        or_test_support::skip(TEST, &root, "yft");
        return;
    }
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: false,
    };
    let mut checked = 0;
    for path in &files {
        let bytes = std::fs::read(path).expect("a corpus file reads");
        let fragment = GtaFragType::<Gen8>::from_bytes(&bytes).expect("a corpus fragment opens");
        let has_extra_without_group = fragment.common_drawable.is_some()
            && fragment
                .extra_drawables
                .iter()
                .any(|d| d.base.shader_group.is_none());
        if !has_extra_without_group {
            continue;
        }
        let name = path.display().to_string();
        let exported = export_scene(&name, &bytes, "fragment", None, &options)
            .unwrap_or_else(|why| panic!("{name}: {why}"));
        let without = exported
            .scene
            .objects
            .iter()
            .flat_map(|o| &o.meshes)
            .flat_map(|m| &m.primitives)
            .filter(|p| p.material.is_none())
            .count();
        assert_eq!(without, 0, "{name}: primitives with no material");
        checked += 1;
    }
    assert!(
        checked > 0,
        "no corpus fragment has an extra drawable without a shader group"
    );
}
