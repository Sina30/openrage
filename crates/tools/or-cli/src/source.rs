//! `openrage-cli source`: what is done to one source folder, from the command line.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use or_source::asset::Source;
use or_source::models;

use crate::project::{parse, Failure};
use Failure::{Run, Usage};

pub fn run(args: &[String]) -> ExitCode {
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("import-model") => import_model(rest, &mut std::io::stdout().lock()),
        Some("compile") => crate::project_steps::compile(rest),
        Some("-h" | "--help" | "help") => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(Usage(format!("unknown source command: {other}"))),
        None => {
            print_usage();
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => code,
        Err(Usage(message)) => {
            print_usage();
            crate::usage_error(&message)
        }
        Err(Run(message)) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage:\n  \
         openrage-cli source import-model <name.drawable.src | name.drawables.src> <model.glb | .gltf | .fbx> [--entry NAME]\n  \
         openrage-cli source compile <source folder> --out <folder>\n\n\
         compile cooks the one source into the game file it stands for, as a build does, and puts the file in --out\n\
         (a project holds only sources, so it is never put beside one). It writes nothing when the source does not\n\
         compile, and exits 1 with the sentences that say why.\n\n\
         import-model replaces the geometry of a drawable or drawables source with the model in the\n\
         file: a glb is stored as it is and a glTF or an FBX is converted to one, and the command lists\n\
         what the drawable has no place for (\"not carried\"). A dictionary source holds many drawables:\n\
         --entry names the one the model goes to. Nothing is written unless the source still\n\
         compiles with the model in it, and the command exits 1 when it does not.\n"
    );
}

/// Replaces the geometry of a source and prints what the compile did and what the drawable had no place for.
fn import_model(args: &[String], out: &mut impl Write) -> Result<ExitCode, Failure> {
    let (positional, flags) = parse(args, &["--entry"])?;
    let [folder, file] = positional.as_slice() else {
        return Err(Usage(
            "expected a <source folder> and a <model file>".into(),
        ));
    };
    let source = Source::open(Path::new(folder)).map_err(|e| Run(e.to_string()))?;
    let entry = flags.get("--entry").map(String::as_str);
    let imported = models::import(&source, entry, Path::new(file)).map_err(|problems| {
        let why: Vec<String> = problems.iter().map(|p| format!("  {p}")).collect();
        Run(format!(
            "the import was refused and {folder} is unchanged:\n{}",
            why.join("\n")
        ))
    })?;
    let mut text = format!(
        "{folder} + {file} -> the geometry of {}{}\n",
        source.manifest.name,
        entry.map_or_else(String::new, |entry| format!(" ({entry})")),
    );
    for line in imported.lines() {
        text.push_str(&format!("  {line}\n"));
    }
    out.write_all(text.as_bytes())
        .map_err(|e| Run(format!("cannot print the report: {e}")))?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use or_interchange::gltf::{self, Container};
    use or_interchange::{drawable_scene, ExtractOptions, LodSelection};
    use or_io::decl::{File, Gen8};
    use or_io::gta5::GtaDrawable;
    use or_source::models::Extraction;
    use or_test_support::{Corpus, TARGET_GEN8};

    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(ToString::to_string).collect()
    }

    /// A drawable source made from the first staged `.ydr` that has geometry, and a glb of the same drawable lifted by 3 m.
    fn subject(dir: &Path) -> Option<(PathBuf, PathBuf)> {
        let files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
            .target(TARGET_GEN8)
            .find_or_skip("a drawable to replace the geometry of")?;
        let how = Extraction {
            from_game: false,
            virtual_path: None,
            textures: false,
            skeleton: None,
        };
        let (folder, bytes) = files.iter().find_map(|file| {
            let bytes = std::fs::read(file).ok()?;
            let folder = models::extract_drawable(&bytes, "subject", dir, &how).ok()?;
            Some((folder, bytes))
        })?;
        let document = GtaDrawable::<Gen8>::from_bytes(&bytes).expect("the drawable reads");
        let options = ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        };
        let mut scene = drawable_scene("subject", &document.base, &options);
        for position in scene
            .objects
            .iter_mut()
            .flat_map(|o| o.meshes.iter_mut())
            .flat_map(|m| m.primitives.iter_mut())
            .flat_map(|p| p.positions.iter_mut())
        {
            position[2] += 3.0;
        }
        let external = dir.join("edited.glb");
        std::fs::write(
            &external,
            gltf::write(&scene, "edited", Container::Glb).primary,
        )
        .unwrap();
        Some((folder, external))
    }

    fn said(args: &[&str]) -> (Result<ExitCode, String>, String) {
        let mut out = Vec::new();
        let done = import_model(&self::args(args), &mut out).map_err(|failure| match failure {
            Usage(why) | Run(why) => why,
        });
        (done, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_model_replaces_the_geometry_of_a_source_and_the_command_lists_what_had_no_place() {
        let work = tempfile::tempdir().unwrap();
        let Some((folder, external)) = subject(work.path()) else {
            return;
        };
        let before = std::fs::read(folder.join("model.glb")).unwrap();

        let (done, printed) = said(&[folder.to_str().unwrap(), external.to_str().unwrap()]);

        assert!(done.is_ok(), "{done:?}");
        assert_eq!(
            std::fs::read(folder.join("model.glb")).unwrap(),
            std::fs::read(&external).unwrap(),
            "the glb is stored as it came"
        );
        assert_ne!(std::fs::read(folder.join("model.glb")).unwrap(), before);
        assert!(
            printed.contains("geometries"),
            "the compile's report: {printed}"
        );
        assert!(
            printed
                .lines()
                .any(|l| l.trim_start().starts_with("not carried: ") && l.contains("materials")),
            "what the drawable has no place for: {printed}"
        );
        let source = Source::open(&folder).unwrap();
        assert!(
            models::compile(&source).is_ok(),
            "the source still compiles"
        );
    }

    #[test]
    fn a_model_the_source_cannot_take_is_refused_and_the_source_is_as_it_was() {
        let work = tempfile::tempdir().unwrap();
        let Some((folder, _)) = subject(work.path()) else {
            return;
        };
        let before = std::fs::read(folder.join("model.glb")).unwrap();
        let obj = work.path().join("mesh.obj");
        std::fs::write(&obj, "v 0 0 0").unwrap();

        let (done, _) = said(&[folder.to_str().unwrap(), obj.to_str().unwrap()]);
        let why = done.expect_err("an .obj is not imported");
        assert!(
            why.contains("mesh.obj") && why.contains("unchanged"),
            "{why}"
        );
        assert_eq!(std::fs::read(folder.join("model.glb")).unwrap(), before);

        let (done, _) = said(&[
            folder.to_str().unwrap(),
            obj.to_str().unwrap(),
            "--entry",
            "x",
        ]);
        assert!(done.is_err(), "a drawable source takes no entry");
    }

    #[test]
    fn the_command_asks_for_a_source_and_a_model() {
        let (done, _) = said(&[]);
        assert!(done.is_err());
        let (done, _) = said(&["only-a-folder"]);
        assert!(done.is_err());
        assert_eq!(run(&args(&["nothing-like-it"])), ExitCode::from(2));
        assert_eq!(run(&[]), ExitCode::from(2));
    }
}
