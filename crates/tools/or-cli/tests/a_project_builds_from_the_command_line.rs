//! `openrage-cli project` creates a project of loose files, prints its plan, builds it and
//! verifies the build, and says no when it cannot.

use std::path::Path;
use std::process::ExitCode;

fn project(args: &[&str]) -> ExitCode {
    let mut all = vec!["project".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a scratch path is UTF-8")
}

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn a_project_of_loose_files_is_created_planned_built_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("pack");
    let script = folder.join("client.lua");
    let data = folder.join("stream").join("a.bin");
    write(&script, b"print('hello')\n");
    write(&data, &[0, 1, 2, 255]);
    let file = folder.join("pack.openrage-proj");

    let created = project(&["create", text(&folder), text(&script), text(&data)]);
    assert_eq!(created, ExitCode::SUCCESS);
    assert!(file.is_file(), "no project file at {}", file.display());
    assert_eq!(
        project(&["create", text(&folder)]),
        ExitCode::FAILURE,
        "a second create replaced the project"
    );

    assert_eq!(project(&["plan", text(&file)]), ExitCode::SUCCESS);
    assert_eq!(project(&["build", text(&file)]), ExitCode::SUCCESS);
    let built = folder.join("build").join("pack");
    assert_eq!(
        std::fs::read(built.join("client.lua")).unwrap(),
        b"print('hello')\n"
    );
    assert_eq!(
        std::fs::read(built.join("stream").join("a.bin")).unwrap(),
        [0, 1, 2, 255]
    );
    assert!(built.join("fxmanifest.lua").is_file());
    assert_eq!(project(&["verify", text(&file)]), ExitCode::SUCCESS);

    write(&script, b"changed after the build");
    assert_eq!(
        project(&["verify", text(&file)]),
        ExitCode::FAILURE,
        "a build the project has moved past verified clean"
    );
}

#[test]
fn a_project_that_cannot_be_planned_exits_with_a_failure_and_changes_no_build() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("pack");
    let script = folder.join("client.lua");
    write(&script, b"a");
    let file = folder.join("pack.openrage-proj");
    assert_eq!(
        project(&["create", text(&folder), text(&script)]),
        ExitCode::SUCCESS
    );
    assert_eq!(project(&["build", text(&file)]), ExitCode::SUCCESS);
    let built = folder.join("build").join("pack").join("client.lua");

    std::fs::remove_file(&script).unwrap();

    assert_eq!(project(&["plan", text(&file)]), ExitCode::FAILURE);
    assert_eq!(project(&["build", text(&file)]), ExitCode::FAILURE);
    assert_eq!(
        std::fs::read(built).unwrap(),
        b"a",
        "a refused build touched the last one"
    );
}

#[test]
fn a_project_asked_for_what_it_does_not_declare_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("pack");
    let script = folder.join("client.lua");
    write(&script, b"a");
    let file = folder.join("pack.openrage-proj");
    let _ = project(&["create", text(&folder), text(&script)]);

    assert_eq!(
        project(&["plan", text(&file), "--output", "nowhere"]),
        ExitCode::FAILURE
    );
}

#[test]
fn a_misspelt_command_is_a_usage_error() {
    let usage = ExitCode::from(2);
    assert_eq!(project(&["frobnicate"]), usage);
    assert_eq!(project(&["create"]), usage);
    assert_eq!(project(&["plan"]), usage);
    assert_eq!(project(&["plan", "p.openrage-proj", "--nope", "x"]), usage);
    assert_eq!(project(&["build", "p.openrage-proj", "--out"]), usage);
}

#[test]
fn a_build_asked_to_land_on_its_own_project_is_refused_and_no_source_is_lost() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("pack");
    let script = folder.join("client.lua");
    let notes = folder.join("notes.txt");
    let psd = folder.join("raw").join("source.psd");
    write(&script, b"a");
    write(&notes, b"not a member");
    write(&psd, b"the artist's file");
    let file = folder.join("pack.openrage-proj");
    assert_eq!(
        project(&["create", text(&folder), text(&script)]),
        ExitCode::SUCCESS
    );

    // The resource is named for the project folder, so `--out` the folder above it is that folder.
    for _ in 0..2 {
        assert_eq!(
            project(&["build", text(&file), "--out", text(dir.path())]),
            ExitCode::FAILURE
        );
    }

    for kept in [&file, &script, &notes, &psd] {
        assert!(kept.is_file(), "{} is gone", kept.display());
    }
}

#[test]
fn an_output_over_its_ceiling_is_refused_even_when_it_may_split_and_nothing_is_built() {
    use or_project::{Container, Module, Output, Project};

    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("pack");
    let script = folder.join("client.lua");
    write(&script, &[b'a'; 64]);
    let file = folder.join("pack.openrage-proj");
    // One Files content cannot be cut, so a ceiling it exceeds by itself stays a refusal.
    let mut output = Output::fivem_resource("fivem", "pack", &["files"]);
    output.max_bytes = Some(10);
    output.split = true;
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![output],
    };
    Project::create(&file, &container, &[script]).unwrap();

    assert_eq!(project(&["plan", text(&file)]), ExitCode::FAILURE);
    assert_eq!(project(&["build", text(&file)]), ExitCode::FAILURE);
    assert!(!folder.join("build").exists(), "a refused build wrote");
}

#[test]
fn a_dlc_and_a_package_are_created_by_kind_built_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    for (kind, built_file) in [("dlc", "dlc.rpf"), ("oiv", "pack.oiv")] {
        let folder = dir.path().join(kind);
        let props = folder.join("data").join("props.ityp");
        write(&props, &[1, 2, 3]);
        let file = folder.join("pack.openrage-proj");

        let created = project(&[
            "create",
            text(&folder),
            "--name",
            "pack",
            "--kind",
            kind,
            text(&props),
        ]);
        assert_eq!(created, ExitCode::SUCCESS, "{kind}");
        assert_eq!(project(&["plan", text(&file)]), ExitCode::SUCCESS, "{kind}");
        assert_eq!(
            project(&["build", text(&file)]),
            ExitCode::SUCCESS,
            "{kind}"
        );

        let built = folder.join("build").join("pack");
        assert!(
            built.join(built_file).is_file(),
            "{kind} wrote no {built_file}"
        );
        assert_eq!(
            project(&["verify", text(&file)]),
            ExitCode::SUCCESS,
            "{kind}"
        );
        std::fs::write(built.join(built_file), b"not what was built").unwrap();
        assert_eq!(
            project(&["verify", text(&file)]),
            ExitCode::FAILURE,
            "{kind}: a build that was replaced verified clean"
        );
    }
}

#[test]
fn a_target_moves_a_texture_dictionary_and_a_kind_or_target_that_is_not_one_is_a_usage_error() {
    use or_io::decl::File;
    use or_io::gta5::gen8::GrcTexture;
    use or_io::gta5::PgDictionary;

    let format = or_io::codec::texture::GrcTextureFormat::DXT1;
    let texture = GrcTexture {
        name: "road".into(),
        width: 16,
        height: 16,
        depth: 1,
        mip_count: 1,
        mip_stride: u16::try_from(format.legacy_mip_stride(16)).unwrap(),
        format: format.to_raw_format(),
        backing_store: vec![0; format.total_size(16, 16, 1) as usize],
        ..Default::default()
    };
    let dictionary = PgDictionary {
        codes: vec![or_io::string_hash(b"road")],
        entries: vec![Some(Box::new(texture))],
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("pack");
    let ytd = folder.join("stream").join("road.ytd");
    write(&ytd, &dictionary.to_bytes().unwrap());
    let file = folder.join("pack.openrage-proj");

    let usage = ExitCode::from(2);
    assert_eq!(project(&["create", text(&folder), "--kind", "zip"]), usage);
    assert_eq!(
        project(&["create", text(&folder), "--target", "gen9"]),
        usage
    );
    assert_eq!(
        project(&[
            "create",
            text(&folder),
            "--name",
            "pack",
            "--target",
            "enhanced",
            text(&ytd)
        ]),
        ExitCode::SUCCESS
    );
    assert_eq!(project(&["build", text(&file)]), ExitCode::SUCCESS);

    let written = std::fs::read(
        folder
            .join("build")
            .join("pack")
            .join("stream")
            .join("road.ytd"),
    )
    .unwrap();
    assert_ne!(
        written,
        std::fs::read(&ytd).unwrap(),
        "the dictionary was copied as it was"
    );
    assert_eq!(project(&["verify", text(&file)]), ExitCode::SUCCESS);
}
