//! A drawables source inside a clothing pack that a garment names is cooked by the garment, for the name its place gives
//! the model. The Files content, which holds every other `.src` folder of the project, leaves it out, so one source is
//! not cooked twice (which would also make two cooks into one file); the sources it holds are cooked as before.

use std::collections::BTreeMap;
use std::path::Path;

use or_content::plans_from;
use or_project::{find_sources, Container, Cooks, Module, Origin, Output, Source, SourceFolder};
use or_source::asset::{AssetManifest, How, Origin as AssetOrigin, Source as AssetSource};
use or_wardrobe::{Component, Garment, Ped, Slot, Wardrobe};

fn drawables_source(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let origin = AssetOrigin {
        how: How::Authored,
        from_game: false,
        virtual_path: None,
        game_build: None,
        sha256: None,
        cooked_sha256: None,
        extra: BTreeMap::new(),
    };
    let manifest = AssetManifest::new("drawables", name, "gen8", origin).unwrap();
    AssetSource {
        dir: dir.to_path_buf(),
        manifest,
    }
    .write_manifest()
    .unwrap();
    std::fs::write(dir.join("dictionary.json"), b"{}").unwrap();
}

/// Every `.src` folder of the project, each said to cook to `<name>.ydd` as `describe` says of a drawables source.
fn folders(project: &Path) -> Vec<SourceFolder> {
    let (found, _) = find_sources(project, &[]);
    found
        .into_iter()
        .map(|folder| {
            let name = folder
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_end_matches(".drawables.src")
                .to_string();
            SourceFolder {
                folder,
                cooks: Ok(Cooks {
                    kind: "drawables".into(),
                    name,
                    extension: "ydd".into(),
                    bytes: 1,
                    game: Vec::new(),
                }),
            }
        })
        .collect()
}

#[test]
fn the_files_content_cooks_its_own_sources_and_leaves_the_one_a_garment_names_to_the_garment() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    drawables_source(&project.join("sources/scarf.drawables.src"), "scarf");
    drawables_source(
        &project.join("clothing/models/jacket.drawables.src"),
        "jacket",
    );
    std::fs::create_dir_all(project.join("clothing/textures")).unwrap();
    std::fs::write(project.join("clothing/textures/a.ytd"), b"texture").unwrap();
    let mut garment = Garment::new();
    garment.ped = Some(Ped::Male);
    garment.slot = Some(Slot::Component(Component::Jbib));
    garment.position = Some(12);
    garment.model = Some("models/jacket.drawables.src".into());
    garment.variations = vec![or_wardrobe::Variation {
        texture: Some("textures/a.ytd".into()),
        ..Default::default()
    }];
    let mut pack = Wardrobe::new("demo");
    pack.garments.push(garment);
    pack.write(&project.join("clothing")).unwrap();

    let container = Container {
        modules: vec![
            Module::files("files"),
            Module::clothing("clothing", "clothing"),
        ],
        outputs: vec![Output::fivem_resource(
            "fivem",
            "demo_pack",
            &["files", "clothing"],
        )],
    };
    let held: Vec<Source> = Vec::new();
    let plans = plans_from(&container, "fivem", project, &held, &folders(project)).unwrap();

    let cooked: Vec<(String, String)> = plans
        .iter()
        .flat_map(|p| &p.files)
        .filter_map(|f| match &f.origin {
            Origin::Cook { source, .. } => Some((
                f.path.clone(),
                source.file_name().unwrap().to_string_lossy().into_owned(),
            )),
            _ => None,
        })
        .collect();
    let mut expected = vec![
        (
            "stream/scarf.ydd".to_string(),
            "scarf.drawables.src".to_string(),
        ),
        (
            "stream/demo/mp_m_freemode_01_mp_m_demo/jbib/mp_m_freemode_01_mp_m_demo^jbib_012_u.ydd"
                .to_string(),
            "jacket.drawables.src".to_string(),
        ),
    ];
    expected.sort();
    let mut cooked_sorted = cooked.clone();
    cooked_sorted.sort();
    assert_eq!(
        cooked_sorted, expected,
        "the jacket is cooked once, as the garment, and not again as stream/jacket.ydd"
    );
    assert!(
        plans.iter().all(|p| p.problems.is_empty()),
        "{:?}",
        plans.iter().map(|p| &p.problems).collect::<Vec<_>>()
    );
}

#[test]
fn a_source_in_a_content_folder_that_no_garment_names_is_that_contents_to_keep_and_is_not_shipped()
{
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    drawables_source(&project.join("sources/scarf.drawables.src"), "scarf");
    // A source the pack was given and replaced since: no garment names it, and it is not the Files content's to ship.
    drawables_source(
        &project.join("clothing/models/old_jacket.drawables.src"),
        "old_jacket",
    );
    Wardrobe::new("demo")
        .write(&project.join("clothing"))
        .unwrap();
    let container = Container {
        modules: vec![
            Module::files("files"),
            Module::clothing("clothing", "clothing"),
        ],
        outputs: vec![Output::fivem_resource(
            "fivem",
            "demo_pack",
            &["files", "clothing"],
        )],
    };

    let plans = plans_from(&container, "fivem", project, &[], &folders(project)).unwrap();

    let shipped: Vec<String> = plans
        .iter()
        .flat_map(|p| &p.files)
        .filter(|f| matches!(f.origin, Origin::Cook { .. }))
        .map(|f| f.path.clone())
        .collect();
    assert_eq!(
        shipped,
        ["stream/scarf.ydd"],
        "the old jacket is not shipped"
    );
}
