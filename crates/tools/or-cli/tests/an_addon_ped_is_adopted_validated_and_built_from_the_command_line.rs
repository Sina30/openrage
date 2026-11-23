//! `openrage-cli addonped` validates the addon peds of a project, and `openrage-cli project` adopts a
//! folder of ped files, plans it, builds it and reads the build back, exactly as it does any content.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_test_support::{Corpus, TARGET_GEN8};

const WHAT: &str = "an_addon_ped_is_adopted_validated_and_built_from_the_command_line";

fn cli(group: &str, args: &[&str]) -> ExitCode {
    let mut all = vec![group.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// The smallest staged file of `extension` that opens.
fn staged(extension: &str) -> Option<PathBuf> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(TARGET_GEN8)
        .find_or_skip(WHAT)?;
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let format = or_io::gta5::formats::format(extension).expect("a declared format");
    files
        .into_iter()
        .take(80)
        .find(|p| std::fs::read(p).is_ok_and(|b| format.open_declared(&b).is_ok()))
}

fn put(folder: &Path, rel: &str, from: &Path) {
    let to = folder.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap();
}

const META: &str = "<CPedModelInfo__InitDataList>\n  <InitDatas>\n    <Item>\n      <Name>my_ped</Name>\n    </Item>\n  </InitDatas>\n</CPedModelInfo__InitDataList>\n";

#[test]
fn a_folder_of_ped_files_is_adopted_validated_planned_built_and_read_back() {
    let (Some(yft), Some(ytd), Some(ydd)) = (staged("yft"), staged("ytd"), staged("ydd")) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("peds");
    put(&folder, "stream/my_ped.yft", &yft);
    put(&folder, "stream/my_ped.ytd", &ytd);
    put(&folder, "stream/my_ped/head_000_r.ydd", &ydd);
    std::fs::write(folder.join("peds.meta"), META).unwrap();
    std::fs::write(folder.join("server.lua"), "print('hi')\n").unwrap();

    assert_eq!(
        cli("project", &["adopt", text(&folder), "--name", "peds"]),
        ExitCode::SUCCESS
    );
    let file = folder.join("peds.openrage-proj");
    assert!(file.is_file());
    assert!(
        folder.join("addonped.json").is_file(),
        "the peds are a model beside their files"
    );

    assert_eq!(
        cli("addonped", &["validate", text(&file)]),
        ExitCode::SUCCESS
    );
    assert_eq!(
        cli("addonped", &["validate", text(&file), "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(cli("project", &["plan", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("project", &["verify", text(&file)]), ExitCode::SUCCESS);

    let built = folder.join("build").join("peds");
    for written in [
        "stream/my_ped.yft",
        "stream/my_ped.ytd",
        "stream/my_ped/head_000_r.ydd",
        "peds.meta",
        "server.lua",
    ] {
        assert!(built.join(written).is_file(), "`{written}` is in the build");
    }
    let manifest = std::fs::read_to_string(built.join("fxmanifest.lua")).unwrap();
    assert!(
        manifest.contains("data_file 'PED_METADATA_FILE' 'peds.meta'"),
        "{manifest}"
    );
}

#[test]
fn a_data_file_that_does_not_define_a_ped_fails_the_validate_and_the_build_is_refused() {
    let (Some(yft), Some(ytd)) = (staged("yft"), staged("ytd")) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("peds");
    put(&folder, "stream/my_ped.yft", &yft);
    put(&folder, "stream/my_ped.ytd", &ytd);
    std::fs::write(folder.join("peds.meta"), META).unwrap();
    assert_eq!(
        cli("project", &["adopt", text(&folder), "--name", "peds"]),
        ExitCode::SUCCESS
    );
    let file = folder.join("peds.openrage-proj");

    // A data file defines the peds a folder is adopted for; edited afterwards, it no longer defines this one.
    std::fs::write(
        folder.join("peds.meta"),
        META.replace("my_ped", "someone_else"),
    )
    .unwrap();
    assert_eq!(
        cli("addonped", &["validate", text(&file)]),
        ExitCode::FAILURE
    );
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::FAILURE);
    assert!(!folder.join("build").join("peds").exists());
}

#[test]
fn a_project_with_no_addon_ped_content_is_told_so_and_a_word_that_is_no_command_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        cli("project", &["create", text(dir.path()), "--name", "plain"]),
        ExitCode::SUCCESS
    );
    let file = dir.path().join("plain.openrage-proj");
    assert_eq!(
        cli("addonped", &["validate", text(&file)]),
        ExitCode::FAILURE
    );
    assert_ne!(cli("addonped", &["dance"]), ExitCode::SUCCESS);
    assert_eq!(cli("addonped", &[]), ExitCode::from(2));
}

#[test]
fn a_build_of_peds_is_read_back_against_the_peds_and_a_tampered_one_fails() {
    let (Some(yft), Some(ytd)) = (staged("yft"), staged("ytd")) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("peds");
    put(&folder, "stream/my_ped.yft", &yft);
    put(&folder, "stream/my_ped.ytd", &ytd);
    std::fs::write(folder.join("peds.meta"), META).unwrap();
    assert_eq!(
        cli("project", &["adopt", text(&folder), "--name", "peds"]),
        ExitCode::SUCCESS
    );
    let file = folder.join("peds.openrage-proj");

    assert_eq!(
        cli("addonped", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "nothing is built yet, so there is nothing to read"
    );
    assert_eq!(cli("project", &["build", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(cli("addonped", &["verify", text(&file)]), ExitCode::SUCCESS);

    std::fs::write(
        folder.join("build").join("peds").join("peds.meta"),
        META.replace("my_ped", "someone_else"),
    )
    .unwrap();
    assert_eq!(
        cli("addonped", &["verify", text(&file)]),
        ExitCode::FAILURE,
        "the data file no longer defines the ped"
    );
}
