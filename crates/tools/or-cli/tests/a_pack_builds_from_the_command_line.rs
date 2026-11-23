//! `openrage-cli wardrobe` imports the files of a pack, validates them, builds the resource, says
//! what moved since the last build and writes the redirect map, and says no when it must.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use or_test_support::{Corpus, TARGET_GEN8};

const WHAT: &str = "a_pack_builds_from_the_command_line";

fn wardrobe(args: &[&str]) -> ExitCode {
    let mut all = vec!["wardrobe".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

/// The smallest staged file of `extension` that opens.
fn staged(extension: &str, count: usize) -> Option<Vec<PathBuf>> {
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(TARGET_GEN8)
        .find_or_skip(WHAT)?;
    files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    let format = or_io::gta5::formats::format(extension).expect("a declared format");
    let opens: Vec<PathBuf> = files
        .into_iter()
        .take(80)
        .filter(|p| std::fs::read(p).is_ok_and(|b| format.open_declared(&b).is_ok()))
        .take(count)
        .collect();
    (opens.len() == count).then_some(opens)
}

fn put(folder: &Path, rel: &str, from: &Path) {
    let to = folder.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap();
}

/// A folder of a pack named as the staged packs are: two drawables of the jacket slot and one of the trousers.
fn pack(dir: &Path) -> Option<PathBuf> {
    let ydds = staged("ydd", 3)?;
    let ytds = staged("ytd", 2)?;
    let folder = dir.join("pack");
    let c = "mp_m_freemode_01_mp_m_police";
    put(&folder, &format!("stream/{c}^jbib_000_u.ydd"), &ydds[0]);
    put(
        &folder,
        &format!("stream/{c}^jbib_diff_000_a_uni.ytd"),
        &ytds[0],
    );
    put(&folder, &format!("stream/{c}^jbib_001_u.ydd"), &ydds[1]);
    put(
        &folder,
        &format!("stream/{c}^jbib_diff_001_a_uni.ytd"),
        &ytds[1],
    );
    put(&folder, &format!("stream/{c}^lowr_004_r.ydd"), &ydds[2]);
    put(
        &folder,
        &format!("stream/{c}^lowr_diff_004_a_whi.ytd"),
        &ytds[0],
    );
    Some(folder)
}

#[test]
fn a_pack_is_imported_validated_built_and_its_renumbering_written_out() {
    let dir = tempfile::tempdir().unwrap();
    let Some(folder) = pack(dir.path()) else {
        return;
    };
    let file = folder.join("police.openrage-proj");

    assert_eq!(wardrobe(&["import", text(&folder)]), ExitCode::SUCCESS);
    assert!(folder.join("wardrobe.json").is_file());
    assert!(file.is_file(), "the pack is a project");
    assert_eq!(
        wardrobe(&["import", text(&folder)]),
        ExitCode::FAILURE,
        "a second import replaced the wardrobe"
    );

    assert_eq!(wardrobe(&["validate", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(
        wardrobe(&["validate", text(&file), "--json"]),
        ExitCode::SUCCESS
    );
    assert_eq!(wardrobe(&["plan", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(wardrobe(&["build", text(&file)]), ExitCode::SUCCESS);

    let built = folder.join("build").join("police");
    let c = "mp_m_freemode_01_mp_m_police";
    for rel in [
        "fxmanifest.lua".to_string(),
        format!("data/{c}.meta"),
        format!("stream/police/{c}.ymt"),
        format!("stream/police/{c}/jbib/{c}^jbib_001_u.ydd"),
        format!("stream/police/{c}/lowr/{c}^lowr_004_r.ydd"),
    ] {
        assert!(built.join(&rel).is_file(), "{rel} is not in the build");
    }
    // The project's own verb checks the same build, since clothing content is content.
    assert_eq!(
        or_cli::run(&["project".into(), "verify".into(), text(&file).into()]),
        ExitCode::SUCCESS
    );

    // The first garment leaves the pack: its number is pinned by the import, so nothing moves.
    let mut w = or_wardrobe::Wardrobe::read(&folder).unwrap();
    assert!(w.garments.iter().all(|g| g.position.is_some()));
    w.garments.retain(|g| g.position != Some(0));
    for g in &mut w.garments {
        if g.position == Some(1) {
            g.position = None;
        }
    }
    w.write(&folder).unwrap();
    let redirects = dir.path().join("redirects.json");
    assert_eq!(
        wardrobe(&["diff", text(&file), "--redirects", text(&redirects)]),
        ExitCode::SUCCESS
    );
    let map: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&redirects).unwrap()).unwrap();
    assert_eq!(map["format"], "openrage-redirects");
    let moved = map["redirects"].as_array().unwrap();
    assert_eq!(
        moved.len(),
        1,
        "the garment that lost its pin takes the first place: {moved:?}"
    );
    assert!(moved[0]["from"].as_str().unwrap().ends_with("jbib_001"));
    assert!(moved[0]["to"].as_str().unwrap().ends_with("jbib_000"));
}

#[test]
fn a_pack_that_cannot_be_planned_exits_with_a_failure_and_changes_no_build() {
    let dir = tempfile::tempdir().unwrap();
    let Some(folder) = pack(dir.path()) else {
        return;
    };
    let file = folder.join("police.openrage-proj");
    assert_eq!(wardrobe(&["import", text(&folder)]), ExitCode::SUCCESS);
    assert_eq!(wardrobe(&["build", text(&file)]), ExitCode::SUCCESS);
    let c = "mp_m_freemode_01_mp_m_police";
    let built = folder
        .join("build")
        .join("police")
        .join(format!("stream/police/{c}/jbib/{c}^jbib_000_u.ydd"));
    let before = std::fs::read(&built).unwrap();

    std::fs::remove_file(folder.join(format!("stream/{c}^jbib_000_u.ydd"))).unwrap();

    assert_eq!(wardrobe(&["validate", text(&file)]), ExitCode::FAILURE);
    assert_eq!(wardrobe(&["plan", text(&file)]), ExitCode::FAILURE);
    assert_eq!(wardrobe(&["build", text(&file)]), ExitCode::FAILURE);
    assert_eq!(
        std::fs::read(&built).unwrap(),
        before,
        "a refused build left the last one alone"
    );
}

#[test]
fn a_command_that_is_not_one_and_a_folder_with_no_pack_are_refused() {
    assert_eq!(wardrobe(&["nonsense"]), ExitCode::from(2));
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(wardrobe(&["import", text(dir.path())]), ExitCode::FAILURE);
    assert_eq!(
        wardrobe(&["validate", text(&dir.path().join("none.openrage-proj"))]),
        ExitCode::FAILURE
    );
}

/// A project whose one clothing content is `dir`, holding `wardrobe.json` with the text given.
fn hand_edited(dir: &Path, text: &str) -> PathBuf {
    std::fs::write(dir.join("wardrobe.json"), text).unwrap();
    let container = or_project::Container {
        modules: vec![or_project::Module::clothing("wardrobe", ".")],
        outputs: vec![or_project::Output::fivem_resource(
            "fivem",
            "pack",
            &["wardrobe"],
        )],
    };
    let file = dir.join("pack.openrage-proj");
    or_project::Project::create(&file, &container, &[]).unwrap();
    file
}

fn with_id(id: &str) -> String {
    format!(
        r#"{{"format":"openrage-wardrobe","schema":1,"pack":"pack","garments":[{{"id":"{id}","ped":"mp_m_freemode_01","slot":"jbib"}}]}}"#
    )
}

#[test]
fn a_hand_edited_wardrobe_is_read_or_refused_and_never_a_crash() {
    let dir = tempfile::tempdir().unwrap();

    // An id of wide characters is a file name, and a garment with no label or model is named by its first eight.
    let file = hand_edited(dir.path(), &with_id("aéééé"));
    for command in ["validate", "plan", "build"] {
        assert_eq!(wardrobe(&[command, text(&file)]), ExitCode::SUCCESS);
    }

    // An id that is no file name is refused by every command that reads the file.
    std::fs::write(dir.path().join("wardrobe.json"), with_id("a/b")).unwrap();
    for command in ["validate", "plan", "build"] {
        assert_eq!(wardrobe(&[command, text(&file)]), ExitCode::FAILURE);
    }
}

#[test]
fn a_pack_with_cloth_is_imported_built_and_its_cloth_read_back_from_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let Some(folder) = pack(dir.path()) else {
        return;
    };
    let Some(cloth) = staged("yld", 1) else {
        return;
    };
    let c = "mp_m_freemode_01_mp_m_police";
    put(&folder, &format!("stream/{c}^jbib_000_u.yld"), &cloth[0]);
    let file = folder.join("police.openrage-proj");

    assert_eq!(wardrobe(&["import", text(&folder)]), ExitCode::SUCCESS);
    let w = or_wardrobe::Wardrobe::read(&folder).unwrap();
    let jacket = w
        .garments
        .iter()
        .find(|g| g.position == Some(0))
        .expect("the jacket");
    assert_eq!(
        jacket.cloth.as_deref(),
        Some(format!("stream/{c}^jbib_000_u.yld").as_str()),
        "the import keeps the cloth beside its model"
    );
    assert!(!jacket.missing_cloth);

    assert_eq!(wardrobe(&["validate", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(wardrobe(&["build", text(&file)]), ExitCode::SUCCESS);
    let built = folder.join("build").join("police");
    assert!(
        built
            .join(format!("stream/police/{c}/jbib/{c}^jbib_000_u.yld"))
            .is_file(),
        "the build ships the cloth under the model's name"
    );

    // A cloth the pack names that is no longer there is an error validation exits for.
    std::fs::remove_file(folder.join(format!("stream/{c}^jbib_000_u.yld"))).unwrap();
    assert_eq!(wardrobe(&["validate", text(&file)]), ExitCode::FAILURE);
}
