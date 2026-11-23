//! Every staged `.ydr` and `.ydd` of both targets states its shaders, distances, bounds and bone tags in
//! `drawable.json`, compiles back to the same bytes with them unedited, and takes one edited parameter into the file and nothing with it.

use std::path::{Path, PathBuf};

use or_interchange::extract::{effect_name, Extracted};
use or_interchange::import::Imported;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::{GtaDrawable, PgDictionary, RmcDrawable};
use or_source::asset::Source;
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8, TARGET_GEN9};
use serde_json::Value;

/// The keys of `drawable.json` that state a value of the base, which a source written before them lacks.
const VALUES: [&str; 7] = [
    "materials",
    "models",
    "lod_distances",
    "bucket_masks",
    "bounds_box",
    "bounds_sphere",
    "bones",
];

#[derive(Default)]
struct Tally {
    drawables: usize,
    materials: usize,
    edited: usize,
}

fn corpus(extension: &str, target: &str, what: &str) -> Option<Vec<PathBuf>> {
    Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(target)
        .find_or_skip(what)
}

fn how(file: &str) -> Extraction<'_> {
    Extraction {
        from_game: true,
        virtual_path: Some(file),
        textures: false,
        skeleton: None,
    }
}

/// Every `drawable.json` of a source folder.
fn rule_files(folder: &Path) -> Vec<PathBuf> {
    let own = folder.join("drawable.json");
    if own.is_file() {
        return vec![own];
    }
    let mut out: Vec<PathBuf> = std::fs::read_dir(folder.join("drawables"))
        .map(|dir| {
            dir.flatten()
                .map(|e| e.path().join("drawable.json"))
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn compile(folder: &Path, label: &str) -> Vec<u8> {
    models::compile(&Source::open(folder).unwrap())
        .unwrap_or_else(|problems| panic!("{label}: compile: {problems:#?}"))
        .bytes
}

/// A parameter of a constant, and a value for its first number that is not the one it has.
fn editable(rules: &Value) -> Option<(usize, String, Value)> {
    for (at, material) in rules["materials"].as_array()?.iter().enumerate() {
        for (key, value) in material["parameters"].as_object()? {
            let Some(first) = value
                .get(0)
                .and_then(|row| row.get(0))
                .and_then(Value::as_f64)
            else {
                continue;
            };
            let first = first as f32;
            let new = first + 0.5 * first.abs().max(1.0);
            let mut edited = value.clone();
            // Written as the extraction writes a number: the shortest text that reads back as the same `f32`.
            edited[0][0] = serde_json::json!(new.to_string().parse::<f64>().unwrap());
            return Some((at, key.clone(), edited));
        }
    }
    None
}

/// Checks one extracted source and, with the edit applied, the file compiled from it.
fn check(
    label: &str,
    folder: &Path,
    scratch: &Path,
    shaders: &dyn Fn(&Value) -> Result<(), String>,
    reextract: &dyn Fn(&[u8], &Path) -> PathBuf,
    tally: &mut Tally,
) {
    let files = rule_files(folder);
    let originals: Vec<Value> = files.iter().map(|p| read(p)).collect();
    for rules in &originals {
        shaders(rules).unwrap_or_else(|why| panic!("{label}: {why}"));
        tally.materials += rules["materials"].as_array().map_or(0, Vec::len);
    }

    // A source written before the values were stated compiles to the same bytes as one that states them.
    let with = compile(folder, label);
    for (path, rules) in files.iter().zip(&originals) {
        let mut bare = rules.clone();
        for key in VALUES {
            bare.as_object_mut().unwrap().remove(key);
        }
        write(path, &bare);
    }
    assert_eq!(
        compile(folder, label),
        with,
        "{label}: stating the base's own values changed a byte"
    );
    for (path, rules) in files.iter().zip(&originals) {
        write(path, rules);
    }

    // One parameter, edited: the file takes it and describes everything else as it did.
    let Some((index, (path, rules))) = files
        .iter()
        .zip(&originals)
        .enumerate()
        .find(|(_, (_, r))| editable(r).is_some())
    else {
        return;
    };
    let (material, key, value) = editable(rules).unwrap();
    let mut edited = rules.clone();
    edited["materials"][material]["parameters"][&key] = value;
    write(path, &edited);
    let compiled = compile(folder, label);
    assert_ne!(compiled, with, "{label}: the edit changed nothing");
    write(path, rules);

    let again = reextract(&compiled, scratch);
    let after = rule_files(&again);
    assert_eq!(after.len(), files.len(), "{label}: the entries changed");
    for (n, (found, original)) in after.iter().zip(&originals).enumerate() {
        let found = read(found);
        let mut expected = original.clone();
        if n == index {
            expected = edited.clone();
        }
        assert_eq!(
            found, expected,
            "{label}[{n}]: the compiled file does not describe the edit and nothing else"
        );
    }
    std::fs::remove_dir_all(&again).unwrap();
    tally.edited += 1;
}

fn every_ydr<T: Extracted + Imported>(target: &str, what: &str) -> Option<Tally>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let files = corpus("ydr", target, what)?;
    let scratch = tempfile::tempdir().unwrap();
    let mut tally = Tally::default();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydr").to_string();
        let bytes = std::fs::read(&path).unwrap();
        let base = GtaDrawable::<T>::from_bytes(&bytes).unwrap().base;
        let Ok(folder) = models::extract_drawable(&bytes, &name, scratch.path(), &how(&file))
        else {
            continue;
        };
        tally.drawables += 1;
        let shaders = shader_names(&base);
        let same_shaders = move |rules: &Value| names_match(rules, &shaders);
        check(
            &file,
            &folder,
            scratch.path(),
            &same_shaders,
            &|bytes, parent| {
                let again = parent.join("again");
                std::fs::create_dir_all(&again).unwrap();
                models::extract_drawable(bytes, &name, &again, &how("again"))
                    .unwrap_or_else(|p| panic!("{file}: extract the compiled file: {p:#?}"))
            },
            &mut tally,
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
    Some(tally)
}

fn every_ydd<T: Extracted + Imported>(target: &str, what: &str) -> Option<Tally>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
{
    let files = corpus("ydd", target, what)?;
    let scratch = tempfile::tempdir().unwrap();
    let mut tally = Tally::default();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = file.trim_end_matches(".ydd").to_string();
        let bytes = std::fs::read(&path).unwrap();
        let Ok(folder) = models::extract_drawables(&bytes, &name, scratch.path(), &how(&file))
        else {
            continue;
        };
        let dictionary = PgDictionary::<GtaDrawable<T>>::from_bytes(&bytes).unwrap();
        let held: Vec<Vec<String>> = dictionary
            .entries
            .iter()
            .flatten()
            .map(|e| shader_names(&e.base))
            .collect();
        tally.drawables += held.len();
        // The extraction keeps the entries that hold geometry, in file order; their shaders are checked as a set.
        let known: Vec<String> = held.into_iter().flatten().collect();
        let same_shaders = move |rules: &Value| {
            let shown: Vec<String> = rules["materials"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|m| m["shader"].as_str().unwrap_or("").to_string())
                .collect();
            match shown.iter().find(|s| !known.contains(s)) {
                Some(unknown) => Err(format!(
                    "a material draws with {unknown:?}, which no shader of the base is"
                )),
                None => Ok(()),
            }
        };
        check(
            &file,
            &folder,
            scratch.path(),
            &same_shaders,
            &|bytes, parent| {
                let again = parent.join("again");
                std::fs::create_dir_all(&again).unwrap();
                models::extract_drawables(bytes, &name, &again, &how("again"))
                    .unwrap_or_else(|p| panic!("{file}: extract the compiled file: {p:#?}"))
            },
            &mut tally,
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
    Some(tally)
}

fn shader_names<T: Extracted>(drawable: &RmcDrawable<T>) -> Vec<String> {
    drawable
        .shader_group
        .as_deref()
        .map(|g| {
            g.shaders
                .iter()
                .map(|s| effect_name(T::effect_hash(s)))
                .collect()
        })
        .unwrap_or_default()
}

fn names_match(rules: &Value, expected: &[String]) -> Result<(), String> {
    let shown: Vec<&str> = rules["materials"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| m["shader"].as_str().unwrap_or(""))
        .collect();
    if shown == expected {
        Ok(())
    } else {
        Err(format!(
            "drawable.json states the shaders {shown:?} and the base has {expected:?}"
        ))
    }
}

fn report(what: &str, tally: &Tally) {
    println!(
        "{what}: {} drawables, {} materials stated, {} sources took an edited parameter",
        tally.drawables, tally.materials, tally.edited
    );
    assert!(tally.materials > 0, "{what} compared nothing");
    assert!(tally.edited > 0, "{what} edited nothing");
}

#[test]
fn every_corpus_ydr_states_its_values_and_takes_an_edited_parameter() {
    let what = "every_corpus_ydr_states_its_values_and_takes_an_edited_parameter";
    if let Some(tally) = every_ydr::<Gen8>(TARGET_GEN8, what) {
        report(what, &tally);
    }
}

#[test]
fn every_corpus_ydd_states_its_values_and_takes_an_edited_parameter() {
    let what = "every_corpus_ydd_states_its_values_and_takes_an_edited_parameter";
    if let Some(tally) = every_ydd::<Gen8>(TARGET_GEN8, what) {
        report(what, &tally);
    }
}

#[test]
fn every_enhanced_ydr_states_its_values_and_takes_an_edited_parameter() {
    let what = "every_enhanced_ydr_states_its_values_and_takes_an_edited_parameter";
    if let Some(tally) = every_ydr::<Gen9>(TARGET_GEN9, what) {
        report(what, &tally);
    }
}

#[test]
fn every_enhanced_ydd_states_its_values_and_takes_an_edited_parameter() {
    let what = "every_enhanced_ydd_states_its_values_and_takes_an_edited_parameter";
    if let Some(tally) = every_ydd::<Gen9>(TARGET_GEN9, what) {
        report(what, &tally);
    }
}
