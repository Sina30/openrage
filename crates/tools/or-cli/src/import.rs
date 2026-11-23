//! `openrage-cli project import`: game files become sources in a project, and the command says what became of each.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use or_project::{project_folder, Project};
use or_source::game_import::{import, Facts, Input, Options, Report, Skeleton, Verdict};
use serde_json::{json, Value};

use crate::project::{parse, Failure};
use Failure::{Run, Usage};

pub fn run(args: &[String]) -> ExitCode {
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help" | "help"))
    {
        print_usage();
        return ExitCode::SUCCESS;
    }
    match import_to(args, &mut std::io::stdout().lock()) {
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
         openrage-cli project import <project.openrage-proj> <path>... [--from-game] [--skeleton <ped.yft>] [--json]\n\n\
         import turns the game files in each <path> (a file, or a folder walked recursively) into\n\
         sources under sources/ beside the project file: a .ytd, .ydr, .ydd, .yft, .ymt, .ymap or\n\
         .ytyp is extracted to a folder where it can be, and any other file is kept as it is under files/, except an archive found in a\n\
         folder, which is listed as left out. It only adds: a name taken by a\n\
         file with another origin gets a number, a file that is a source already is skipped and\n\
         said so, and no input is changed. --from-game says the files are the game's, so the sources\n\
         are kept out of Git and a build does not ship them unchanged. --skeleton names the .yft (or a .ydr) of\n\
         the ped a ped component is for, whose skeleton the component's skin is bound to: with it the glb of the\n\
         source has its skin, and a skeleton the component does not fit is said in the source and does not refuse\n\
         the file. --json prints the report as JSON. The command exits 1 when a file was refused.\n"
    );
}

/// Imports and prints the report to `out`; the exit code is 1 when any file was refused.
fn import_to(args: &[String], out: &mut impl Write) -> Result<ExitCode, Failure> {
    let json = args.iter().any(|a| a == "--json");
    let from_game = args.iter().any(|a| a == "--from-game");
    let rest: Vec<String> = args
        .iter()
        .filter(|a| !matches!(a.as_str(), "--json" | "--from-game"))
        .cloned()
        .collect();
    let (positional, flags) = parse(&rest, &["--skeleton"])?;
    let Some((project, paths)) = positional.split_first() else {
        return Err(Usage("expected a <project.openrage-proj>".into()));
    };
    if paths.is_empty() {
        return Err(Usage("expected at least one <path> to import".into()));
    }
    let project_path = Path::new(project);
    let loaded = Project::load(project_path).map_err(|e| Run(format!("{project}: {e}")))?;
    for finding in &loaded.findings {
        eprintln!("warning: {finding}");
    }

    // The skeleton of the ped a ped component is for, which lives in another file: a component imported with it has its skin in the glb.
    let skeleton = match flags.get("--skeleton") {
        Some(file) => {
            let bytes = std::fs::read(file).map_err(|e| Run(format!("{file}: {e}")))?;
            let extension = Path::new(file)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            Skeleton::of(
                or_source::models::reference_skeleton(&extension, &bytes)
                    .map_err(|p| Run(format!("{file}: {p}")))?,
            )
        }
        None => Skeleton::default(),
    };
    let facts = Facts {
        from_game,
        skeleton,
        ..Facts::default()
    };
    let inputs: Vec<Input<'_>> = paths
        .iter()
        .map(|path| Input::Path {
            path: Path::new(path),
            facts: facts.clone(),
        })
        .collect();
    let report = import(&project_folder(project_path), &inputs, &Options::default());
    crate::project_steps::keep_import_step(project_path, &report.created);

    let text = if json {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&report_json(&report)).unwrap_or_default()
        )
    } else {
        report_text(&report)
    };
    out.write_all(text.as_bytes())
        .map_err(|e| Run(format!("cannot print the report: {e}")))?;
    let refused = counts(&report)[3] > 0;
    Ok(if refused {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// How many files were made, already sources, kept and refused.
fn counts(report: &Report) -> [usize; 4] {
    let mut counts = [0; 4];
    for file in &report.files {
        counts[match file.verdict {
            Verdict::Made { .. } => 0,
            Verdict::AlreadyASource { .. } => 1,
            Verdict::Kept { .. } => 2,
            Verdict::Refused => 3,
        }] += 1;
    }
    counts
}

fn word(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Made { .. } => "made",
        Verdict::AlreadyASource { .. } => "already_a_source",
        Verdict::Kept { .. } => "kept",
        Verdict::Refused => "refused",
    }
}

fn report_json(report: &Report) -> Value {
    let files: Vec<Value> = report
        .files
        .iter()
        .map(|file| {
            let mut value = json!({
                "input": file.input,
                "verdict": word(&file.verdict),
                "reason": file.reason,
            });
            match &file.verdict {
                Verdict::Made { folder } | Verdict::AlreadyASource { folder } => {
                    value["folder"] = folder.display().to_string().into();
                }
                Verdict::Kept { copied_to } => {
                    value["copied_to"] = copied_to
                        .as_ref()
                        .map_or(Value::Null, |p| p.display().to_string().into());
                }
                Verdict::Refused => {}
            }
            value
        })
        .collect();
    let [made, already, kept, refused] = counts(report);
    json!({
        "files": files,
        "created": report.created.iter().map(|c| json!({
            "path": c.path.display().to_string(),
            "digest": c.digest,
        })).collect::<Vec<_>>(),
        "left_out": report.left_out.iter().map(|l| json!({ "path": l.path, "why": l.why })).collect::<Vec<_>>(),
        "notes": report.notes,
        "counts": { "made": made, "already_a_source": already, "kept": kept, "refused": refused },
    })
}

fn report_text(report: &Report) -> String {
    let mut text = String::new();
    for file in &report.files {
        text.push_str(&format!(
            "{:<17} {}: {}\n",
            word(&file.verdict),
            file.input,
            file.reason
        ));
    }
    for left in &report.left_out {
        text.push_str(&format!("{:<17} {}: {}\n", "left out", left.path, left.why));
    }
    for note in &report.notes {
        text.push_str(&format!("note: {note}\n"));
    }
    let [made, already, kept, refused] = counts(report);
    text.push_str(&format!(
        "{made} made, {already} already a source, {kept} kept as a compiled file, {refused} refused\n"
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::authoring::TextureBuildOptions;
    use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
    use or_io::decl::File;
    use or_io::gta5::gen8::GrcTexture;
    use or_io::gta5::texture_dict::gen8::PROCESSED;
    use or_io::gta5::PgDictionary;
    use or_project::{Container, Module, Output, OutputKind};

    /// A texture dictionary with one 8 by 8 texture.
    fn ytd(seed: u8) -> Vec<u8> {
        let mut image = RgbaImage::new(8, 8);
        for (i, byte) in image.data.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(seed);
        }
        let mut texture = GrcTexture {
            name: "bark".to_string(),
            resource_type_and_conversion_flags: PROCESSED,
            ..Default::default()
        };
        texture
            .set_pixels(
                GrcTextureFormat::DXT5,
                &image,
                &TextureBuildOptions::default(),
            )
            .unwrap();
        let mut dictionary = PgDictionary::<GrcTexture>::default();
        dictionary.insert(texture);
        dictionary.to_bytes().unwrap()
    }

    fn project(folder: &Path) -> String {
        let path = folder.join("harbor.openrage-proj");
        let container = Container {
            modules: vec![Module::files("files")],
            outputs: vec![Output::of(
                OutputKind::FivemResource,
                "fivem",
                "harbor",
                &["files"],
            )],
        };
        Project::create(&path, &container, &[]).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn printed(words: &[&str]) -> (Result<ExitCode, Failure>, String) {
        let mut out = Vec::new();
        let result = import_to(&args(words), &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn the_report_is_printed_as_json_and_the_sources_are_on_disk() {
        let work = tempfile::tempdir().unwrap();
        let project = project(work.path());
        let input = work.path().join("incoming");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::write(input.join("bark_set.ytd"), ytd(3)).unwrap();
        std::fs::write(input.join("hull.ybn"), b"bounds").unwrap();
        std::fs::write(input.join("broken.ytd"), b"not a texture dictionary").unwrap();
        let input = input.to_string_lossy().into_owned();

        let (code, text) = printed(&[&project, &input, "--from-game", "--json"]);
        assert!(
            matches!(code, Ok(c) if c == ExitCode::FAILURE),
            "one file is refused"
        );
        let report: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(
            report["counts"],
            json!({ "made": 1, "already_a_source": 0, "kept": 1, "refused": 1 })
        );
        let verdicts: Vec<&str> = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["verdict"].as_str().unwrap())
            .collect();
        assert_eq!(
            verdicts,
            ["made", "refused", "kept"],
            "in path order: bark_set, broken, hull: {text}"
        );
        assert!(report["files"][0]["folder"]
            .as_str()
            .unwrap()
            .ends_with("bark_set.textures.src"));
        assert!(report["files"][1]["reason"]
            .as_str()
            .unwrap()
            .contains("texture dictionary"));
        assert_eq!(report["created"].as_array().unwrap().len(), 2);
        assert!(work
            .path()
            .join("sources/bark_set.textures.src/asset.json")
            .is_file());
        assert!(work.path().join("files/hull.ybn").is_file());

        let (code, text) = printed(&[&project, &input]);
        assert!(matches!(code, Ok(c) if c == ExitCode::FAILURE));
        assert!(text.contains("already_a_source"), "{text}");
        assert!(
            text.contains("0 made, 1 already a source, 1 kept as a compiled file, 1 refused"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_project_or_path_is_a_usage_or_run_error_and_prints_nothing() {
        let work = tempfile::tempdir().unwrap();
        let missing = work
            .path()
            .join("none.openrage-proj")
            .to_string_lossy()
            .into_owned();
        assert!(matches!(printed(&[&missing, "x"]).0, Err(Run(_))));
        assert!(matches!(printed(&[]).0, Err(Usage(_))));
        let project = project(work.path());
        let (result, text) = printed(&[&project]);
        assert!(matches!(result, Err(Usage(_))) && text.is_empty());
    }

    #[test]
    fn the_folders_a_walk_leaves_alone_are_the_ones_a_project_names() {
        use or_source::game_import as g;
        assert_eq!(g::BUILD_DIR, or_project::BUILD_DIR);
        assert_eq!(g::SHADOW_DIR, or_project::SHADOW_DIR);
        assert_eq!(g::PROJECT_EXTENSION, or_project::PROJECT_EXTENSION);
    }
}
