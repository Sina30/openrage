//! A data file the model states that is not there, is a folder, leaves the module or is not text is
//! a problem of the plan, so the build is refused before it swaps anything in.
//!
//! The ped's files have names and bytes and nothing a reader opens: a plan at `Depth::Digest` hashes them.

mod common;

use std::path::{Path, PathBuf};

use common::{container, peds_meta, plan, write};
use or_addonped::{AddonPeds, Ped, PedFile};
use or_project::build;

/// A module folder `modules/peds` holding the ped `my_ped` and the data file the model states.
fn module(folder: &Path) -> PathBuf {
    let dir = folder.join("modules").join("peds");
    let mut ped = Ped::new("my_ped");
    for path in ["my_ped.yft", "my_ped.ytd"] {
        write(&dir.join(path), path.as_bytes());
        ped.files.push(PedFile {
            path: path.into(),
            at: None,
        });
    }
    write(&dir.join("peds.meta"), peds_meta(&["my_ped"]).as_bytes());
    let mut model = AddonPeds::new();
    model.meta = Some("peds.meta".into());
    model.peds.push(ped);
    model.write(&dir).unwrap();
    dir
}

fn problems(folder: &Path) -> Vec<String> {
    plan(&container(&["peds"]), folder).problems
}

#[test]
fn a_data_file_that_is_missing_a_folder_outside_the_module_or_not_text_refuses_the_build() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    assert_eq!(
        problems(dir.path()),
        Vec::<String>::new(),
        "the module as it is has no problem"
    );

    let data = module.join("peds.meta");
    let good = std::fs::read(&data).unwrap();
    let refused = |code: &str| {
        let plan = plan(&container(&["peds"]), dir.path());
        assert!(
            plan.problems
                .iter()
                .any(|p| p.starts_with(&format!("{code}: ")) && p.contains("peds.meta")),
            "{code}: {:?}",
            plan.problems
        );
        assert!(!plan.is_buildable());
        let root = dir.path().join("build");
        assert!(
            build(&plan, &root).is_err(),
            "{code}: a refused plan is not built"
        );
        assert!(
            !root.join("ped_pack").exists(),
            "{code}: nothing was swapped in"
        );
    };

    std::fs::remove_file(&data).unwrap();
    refused("file-missing");

    std::fs::create_dir(&data).unwrap();
    refused("file-missing");
    std::fs::remove_dir(&data).unwrap();

    std::fs::write(&data, [0xff, 0xfe, 0x00, 0x01]).unwrap();
    refused("file-unreadable");

    std::fs::write(&data, &good).unwrap();
    let mut model = AddonPeds::read(&module).unwrap();
    model.meta = Some("../peds.meta".into());
    model.write(&module).unwrap();
    refused("file-escapes");
}
