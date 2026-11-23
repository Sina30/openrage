//! What the suites about addon peds share: staged ped files copied under a ped's name, and a project that holds them.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use or_addonped::{AddonPeds, Ped, PedFile, FILE};
use or_project::{Container, Module, Output, Plan};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

/// One staged file of `extension` with a name that contains `prefer` where one does, or the first.
pub fn staged(extension: &str, prefer: &str) -> Option<PathBuf> {
    let files = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), extension)
        .target(TARGET_GEN8)
        .find_or_skip(&format!("a staged .{extension} as a file of an addon ped"))?;
    files
        .iter()
        .find(|f| f.to_string_lossy().contains(prefer))
        .or(files.first())
        .cloned()
}

/// The staged files an addon ped is made of: a model, a texture dictionary, a drawable dictionary and an expression dictionary.
pub struct Staged {
    pub model: PathBuf,
    pub textures: PathBuf,
    pub drawables: PathBuf,
    pub expressions: PathBuf,
}

pub fn staged_files() -> Option<Staged> {
    Some(Staged {
        model: staged("yft", "hipster")?,
        textures: staged("ytd", "jbib_diff")?,
        drawables: staged("ydd", "downtown")?,
        expressions: staged("yed", "money")?,
    })
}

pub fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A data file with an entry for each of `names`.
pub fn peds_meta(names: &[&str]) -> String {
    let items: String = names
        .iter()
        .map(|n| format!("    <Item>\n      <Name>{n}</Name>\n    </Item>\n"))
        .collect();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CPedModelInfo__InitDataList>\n  <InitDatas>\n{items}  </InitDatas>\n</CPedModelInfo__InitDataList>\n")
}

/// A module folder `folder/modules/peds` holding the ped `name` made of `staged`, with its model written, and the folder's path in the project.
pub fn ped_module(folder: &Path, name: &str, staged: &Staged) -> PathBuf {
    let dir = folder.join("modules").join("peds");
    let copy = |from: &Path, to: String| {
        write(&dir.join(&to), &std::fs::read(from).unwrap());
        to
    };
    let mut ped = Ped::new(name);
    for (from, to) in [
        (&staged.model, format!("{name}.yft")),
        (&staged.textures, format!("{name}.ytd")),
        (&staged.drawables, "parts/head_000_r.ydd".to_string()),
        (&staged.expressions, format!("{name}.yed")),
    ] {
        let path = copy(from, to);
        ped.files.push(PedFile { path, at: None });
    }
    write(&dir.join("peds.meta"), peds_meta(&[name]).as_bytes());
    let mut model = AddonPeds::new();
    model.meta = Some("peds.meta".into());
    model.peds.push(ped);
    model.write(&dir).unwrap();
    assert!(dir.join(FILE).is_file());
    dir
}

pub fn container(take: &[&str]) -> Container {
    Container {
        modules: vec![Module::addon_ped("peds", "modules/peds")],
        outputs: vec![Output::fivem_resource("fivem", "ped_pack", take)],
    }
}

/// The plan of the one output, with the contribution of every addon ped module.
pub fn plan(container: &Container, folder: &Path) -> Plan {
    let contributions = or_addonped::contributions(container, folder);
    or_project::plan_with(container, "fivem", folder, &contributions).expect("the output exists")
}
