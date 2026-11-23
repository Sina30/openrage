//! `openrage-cli project import --skeleton` binds a ped component's skin to the skeleton of its ped, which lives in another file, and a skeleton
//! that does not fit comes in as it did and says so in the source; `wardrobe mesh --materials` says why it cannot run without a shader database.
//!
//! ⚠ Reads a ped component and a ped staged in the corpus; a machine without them skips, and says so.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_project::{Container, Module, Output, OutputKind, Project};
use or_source::asset::Source;
use or_test_support::{Corpus, TARGET_GEN8};

fn cli(args: &[&str]) -> ExitCode {
    let all: Vec<String> = args.iter().map(ToString::to_string).collect();
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

fn staged(extension: &str, name: Option<&str>) -> Option<PathBuf> {
    let found = Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .find(|p| name.is_none_or(|n| p.file_name().is_some_and(|f| f == n)));
    if found.is_none() {
        or_test_support::skip_corpus(&format!("a staged .{extension}"), "not staged".to_string());
    }
    found
}

#[test]
fn a_skeleton_named_on_the_command_line_is_offered_to_the_component_and_a_bad_one_is_refused() {
    let (Some(component), Some(ped)) = (staged("ydd", Some("accs_000_u.ydd")), staged("yft", None))
    else {
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let file = work.path().join("harbor.openrage-proj");
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::of(
            OutputKind::FivemResource,
            "fivem",
            "harbor",
            &["files"],
        )],
    };
    Project::create(&file, &container, &[]).unwrap();

    assert_eq!(
        cli(&[
            "project",
            "import",
            text(&file),
            text(&component),
            "--skeleton",
            text(&work.path().join("nothing.yft"))
        ]),
        ExitCode::FAILURE,
        "a skeleton that is not there is refused before anything is imported"
    );
    assert!(!work
        .path()
        .join("sources")
        .join("accs_000_u.drawables.src")
        .exists());

    assert_eq!(
        cli(&[
            "project",
            "import",
            text(&file),
            text(&component),
            "--skeleton",
            text(&ped)
        ]),
        ExitCode::SUCCESS
    );
    let made = Source::open(&work.path().join("sources/accs_000_u.drawables.src")).unwrap();
    let said = made.manifest.origin.extra["reference_skeleton"]
        .as_str()
        .unwrap();
    assert!(
        said == "embedded" || said.starts_with("not embedded: "),
        "the source says what became of the skeleton: {said}"
    );
}
