//! The Enhanced half of the shader definition database: built from the
//! install's shader container, and checked against the corpus's Gen9 drawables.

use std::path::PathBuf;

use or_io::gta5::shaders::shader_defs::materials::materials_in;
use or_io::gta5::shaders::shader_defs::{Coverage, ShaderDefinitions};
use or_test_support::{game_roots, skip_install, Corpus, GAME_ROOT_ENV};

fn enhanced_roots() -> Vec<PathBuf> {
    game_roots()
        .into_iter()
        .filter(|root| root.join("GTA5_Enhanced.exe").is_file())
        .collect()
}

#[test]
fn an_enhanced_install_builds_a_database_that_names_its_shaders() {
    let roots = enhanced_roots();
    if roots.is_empty() {
        skip_install(
            "the Enhanced shader database",
            format_args!("no Enhanced install; set {GAME_ROOT_ENV}"),
        );
        return;
    }
    for root in roots {
        let built = or_shader_tool::defs::build_from_install(&root, "gta5", "gen9")
            .unwrap_or_else(|e| panic!("{}: {e}", root.display()));
        let defs = &built.definitions;
        println!(
            "{}: {} shaders, {} parameters, {} presets unresolved",
            root.display(),
            defs.shaders.len(),
            defs.parameter_count(),
            built.unresolved.len()
        );
        assert_eq!(defs.target, "gen9");
        let default = defs
            .shader_named("default")
            .expect("every install ships the `default` effect");
        assert!(
            default
                .parameter(or_io::string_hash(b"DiffuseTex"))
                .is_some(),
            "`default` does not declare the texture every Enhanced material of it binds"
        );
        assert!(built.unreadable.is_empty(), "{:?}", built.unreadable);
    }
}

#[test]
fn defs_check_reads_every_enhanced_drawable_in_the_corpus() {
    let files: Vec<PathBuf> = ["ydr", "ydd", "yft"]
        .iter()
        .flat_map(|ext| {
            Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
                .target("gen9")
                .find()
        })
        .collect();
    if files.is_empty() {
        or_test_support::skip(
            "Enhanced drawables for `defs check`",
            &Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
                .target("gen9")
                .search_dir(),
            "ydr",
        );
        return;
    }
    let cov = Coverage::of(
        &ShaderDefinitions::new("gta5", "gen9"),
        &files,
        materials_in,
    );
    assert!(
        cov.files_refused.is_empty(),
        "refused: {:?}",
        cov.files_refused
    );
    assert_eq!(cov.files_read, files.len());
    assert!(cov.materials > 0 && cov.entries > 0, "{cov:?}");
}
