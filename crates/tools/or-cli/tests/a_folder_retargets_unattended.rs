//! `openrage-cli convert retarget` over a staged folder of every format it
//! converts, in both directions: each output reopens owned as the other
//! target, and the batch's one report names each value `or_io`'s retarget
//! dropped for that file, or why the file was refused.

use std::path::{Path, PathBuf};

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::formats::format;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::GrcTextureG9;
use or_io::gta5::retarget::{self, LossReport};
use or_io::gta5::shaders::fxc::GrcEffect;
use or_io::gta5::shaders::shader_defs::{ParamRemap, ShaderDefinitions};
use or_io::gta5::{GtaDrawable, GtaFragType, PgDictionary};
use or_io::Generation;
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};
use serde_json::Value;

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

/// The target `bytes` holds, by its RSC7 version.
fn target(ext: &str, bytes: &[u8]) -> Result<Generation, String> {
    let version = Rsc7::peek(bytes).map_err(|e| e.to_string())?.version();
    (format(ext).expect("declared").target_of_version(version)).map_err(|e| e.to_string())
}

/// What `or_io` drops retargeting `bytes`, as `at: field` lines, or why it refuses.
fn losses(ext: &str, bytes: &[u8], remap: &ParamRemap) -> Result<Vec<String>, String> {
    fn lines(report: LossReport) -> Vec<String> {
        report.0.iter().map(ToString::to_string).collect()
    }
    let e = |e: or_io::Error| e.to_string();
    let gen9 = target(ext, bytes)?.is_gen9();
    Ok(match (ext, gen9) {
        ("ydr", false) => lines(
            retarget::to_gen9(&GtaDrawable::<Gen8>::from_bytes(bytes).map_err(e)?, remap)
                .map_err(e)?
                .1,
        ),
        ("ydr", true) => lines(
            retarget::to_gen8(&GtaDrawable::<Gen9>::from_bytes(bytes).map_err(e)?, remap)
                .map_err(e)?
                .1,
        ),
        ("ytd", false) => lines(
            retarget::texture_dictionary_to_gen9(
                &PgDictionary::<GrcTexture>::from_bytes(bytes).map_err(e)?,
            )
            .map_err(e)?
            .1,
        ),
        ("ytd", true) => lines(
            retarget::texture_dictionary_to_gen8(
                &PgDictionary::<GrcTextureG9>::from_bytes(bytes).map_err(e)?,
            )
            .map_err(e)?
            .1,
        ),
        ("ydd", false) => lines(
            retarget::drawable_dictionary_to_gen9(
                &PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).map_err(e)?,
                remap,
            )
            .map_err(e)?
            .1,
        ),
        ("ydd", true) => lines(
            retarget::drawable_dictionary_to_gen8(
                &PgDictionary::<GtaDrawable<Gen9>>::from_bytes(bytes).map_err(e)?,
                remap,
            )
            .map_err(e)?
            .1,
        ),
        ("yft", false) => lines(
            retarget::fragment_to_gen9(&GtaFragType::<Gen8>::from_bytes(bytes).map_err(e)?, remap)
                .map_err(e)?
                .1,
        ),
        ("yft", true) => lines(
            retarget::fragment_to_gen8(&GtaFragType::<Gen9>::from_bytes(bytes).map_err(e)?, remap)
                .map_err(e)?
                .1,
        ),
        _ => unreachable!("{ext}"),
    })
}

/// `bytes` opens owned as `to`'s type.
fn reopens(ext: &str, bytes: &[u8], to: Generation) -> Result<(), or_io::Error> {
    match (ext, to.is_gen9()) {
        ("ydr", false) => GtaDrawable::<Gen8>::from_bytes(bytes).map(drop),
        ("ydr", true) => GtaDrawable::<Gen9>::from_bytes(bytes).map(drop),
        ("ytd", false) => PgDictionary::<GrcTexture>::from_bytes(bytes).map(drop),
        ("ytd", true) => PgDictionary::<GrcTextureG9>::from_bytes(bytes).map(drop),
        ("ydd", false) => PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).map(drop),
        ("ydd", true) => PgDictionary::<GtaDrawable<Gen9>>::from_bytes(bytes).map(drop),
        ("yft", false) => GtaFragType::<Gen8>::from_bytes(bytes).map(drop),
        ("yft", true) => GtaFragType::<Gen9>::from_bytes(bytes).map(drop),
        _ => unreachable!("{ext}"),
    }
}

/// A shader database assembled from the staged `.fxc` effects, which may be none.
fn database() -> ShaderDefinitions {
    let effects: Vec<(String, GrcEffect)> = (Corpus::new(MANIFEST, "fxc").find().iter())
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let effect = stem.rsplit("__").next()?.to_string();
            Some((effect, GrcEffect::parse(&std::fs::read(path).ok()?).ok()?))
        })
        .collect();
    ShaderDefinitions::assemble("gta5", "gen8", &effects, &[]).0
}

fn run(args: &[&Path]) -> std::process::ExitCode {
    let mut words = vec!["convert".to_string(), "retarget".to_string()];
    words.extend(args.iter().map(|a| a.to_string_lossy().into_owned()));
    or_cli::run(&words)
}

#[test]
fn a_folder_of_every_format_converts_to_the_other_target_with_one_report() {
    let mut staged: Vec<PathBuf> = Vec::new();
    for ext in ["ydr", "ytd", "ydd", "yft"] {
        for target in [TARGET_GEN8, TARGET_GEN9] {
            staged.extend(Corpus::new(MANIFEST, ext).target(target).limit(2).find());
        }
    }
    if staged.is_empty() {
        or_test_support::skip_corpus(
            "a folder retargeted",
            "no .ydr, .ytd, .ydd or .yft is staged",
        );
        return;
    }

    let scratch = tempfile::tempdir().unwrap();
    let (input, output) = (scratch.path().join("in"), scratch.path().join("out"));
    for (i, path) in staged.iter().enumerate() {
        let dir = input.join(format!("group{}", i % 3));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(path, dir.join(path.file_name().unwrap())).unwrap();
    }
    std::fs::write(input.join("an_archive.rpf"), b"RPF7").unwrap();

    let defs = database();
    let defs_path = scratch.path().join("shader_defs.json");
    std::fs::write(&defs_path, defs.to_json().unwrap()).unwrap();
    let remap = ParamRemap::of(&defs);

    let _ = run(&[
        &input,
        Path::new("--out"),
        &output,
        Path::new("--shader-defs"),
        &defs_path,
    ]);

    let report: Value = serde_json::from_slice(
        &std::fs::read(output.join("retarget_report.json")).expect("the batch writes one report"),
    )
    .unwrap();
    assert_eq!(report["cancelled"], false);
    let files = report["files"].as_array().expect("a report per file");
    assert_eq!(
        files.len(),
        staged.len() + 1,
        "every file under the folder is reported"
    );

    let entry = |rel: &str| {
        (files.iter())
            .find(|f| f["file"].as_str().map(|s| s.replace('\\', "/")).as_deref() == Some(rel))
            .unwrap_or_else(|| panic!("{rel} is not in the report"))
    };
    let archive = entry("an_archive.rpf");
    assert_eq!(
        archive["status"], "refused",
        "an archive is refused: {archive}"
    );

    let mut converted = 0;
    for (i, path) in staged.iter().enumerate() {
        let leaf = path.file_name().unwrap().to_string_lossy().into_owned();
        let rel = format!("group{}/{leaf}", i % 3);
        let ext = path
            .extension()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        let reported = entry(&rel);
        let bytes = std::fs::read(path).unwrap();
        match losses(&ext, &bytes, &remap) {
            Ok(dropped) => {
                assert_eq!(reported["status"], "converted", "{rel}: {reported}");
                let named: Vec<String> = (reported["dropped"].as_array().expect("dropped").iter())
                    .map(|d| {
                        format!(
                            "{}: {}",
                            d["at"].as_str().unwrap(),
                            d["field"].as_str().unwrap()
                        )
                    })
                    .collect();
                assert_eq!(named, dropped, "{rel}: the report names each drop");

                let to = if target(&ext, &bytes).unwrap().is_gen9() {
                    Generation::Gen8
                } else {
                    Generation::Gen9
                };
                let out = std::fs::read(output.join(&rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
                assert_eq!(
                    target(&ext, &out),
                    Ok(to),
                    "{rel}: written as the other target"
                );
                reopens(&ext, &out, to).unwrap_or_else(|e| panic!("{rel}: reopens owned: {e}"));
                converted += 1;
            }
            Err(why) => {
                assert_eq!(reported["status"], "refused", "{rel}: {why}: {reported}");
                assert!(
                    !reported["why"].as_str().unwrap_or_default().is_empty(),
                    "{rel}: why"
                );
                assert!(
                    !output.join(&rel).exists(),
                    "{rel}: a refused file writes nothing"
                );
            }
        }
    }
    assert!(converted > 0, "nothing staged converted");
    println!("{converted} of {} staged files converted", staged.len());
}
