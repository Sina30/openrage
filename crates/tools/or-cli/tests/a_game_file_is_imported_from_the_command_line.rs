//! `openrage-cli project import` reaches the import engine through the command line's own dispatch.

use std::path::Path;
use std::process::ExitCode;

use or_project::{Container, Module, Output, OutputKind, Project};

fn cli(args: &[&str]) -> ExitCode {
    let mut all = vec!["project".to_string(), "import".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

#[test]
fn a_file_with_no_source_form_is_kept_in_the_project_and_a_second_import_adds_nothing() {
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
    let hull = work.path().join("incoming").join("hull.ybn");
    std::fs::create_dir_all(hull.parent().unwrap()).unwrap();
    std::fs::write(&hull, b"bounds").unwrap();

    assert_eq!(
        cli(&[text(&file), text(&hull), "--json"]),
        ExitCode::SUCCESS
    );
    let kept = work.path().join("files").join("hull.ybn");
    assert_eq!(std::fs::read(&kept).unwrap(), b"bounds");
    assert_eq!(
        std::fs::read(&hull).unwrap(),
        b"bounds",
        "the input is left as it was"
    );

    assert_eq!(cli(&[text(&file), text(&hull)]), ExitCode::SUCCESS);
    assert!(!work.path().join("files").join("hull_2.ybn").exists());

    let missing = work.path().join("nothing_here.ybn");
    assert_eq!(
        cli(&[text(&file), text(&missing)]),
        ExitCode::FAILURE,
        "a path that is not there is refused"
    );
    assert_eq!(cli(&[text(&file)]), ExitCode::from(2), "a path is required");
}
