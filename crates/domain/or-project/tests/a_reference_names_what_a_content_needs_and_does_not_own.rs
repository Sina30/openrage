//! A reference: the one place a content may name something that is not a source of its project. It is a small JSON file in
//! the content's folder with three forms (a source or file of this project, a source or file of another project, an id
//! typed by hand), and it is resolved when a plan is made.

use std::fs;
use std::path::Path;

use or_project::reference::{self, Reference, Resolved, SUFFIX};

fn source(root: &Path, rel: &str, id: &str) {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("asset.json"), format!("{{\"id\": \"{id}\"}}")).unwrap();
}

fn project(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("p.openrage-proj"), b"{}").unwrap();
}

#[test]
fn a_reference_is_a_small_json_file_that_reads_back_in_each_form() {
    let dir = tempfile::tempdir().unwrap();
    let forms = [
        Reference::here("sources/jacket.drawables.src", Some("a1b2")),
        Reference::here("files/jacket.yld", None),
        Reference::other("../brand", "sources/logo.textures.src", Some("c3d4")),
        Reference::by_id("mp_m_freemode_01_x^jbib_001_u"),
    ];
    for (n, form) in forms.iter().enumerate() {
        let file = dir.path().join(format!("f{n}{SUFFIX}"));
        form.write(&file).unwrap();
        assert_eq!(&Reference::read(&file).unwrap(), form);
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.ends_with("}\n") && !text.contains('\r'), "{text:?}");
        assert!(
            text.contains("\"format\": \"openrage-reference\""),
            "{text}"
        );
    }
    assert_eq!(
        forms[3].describe(),
        "the id mp_m_freemode_01_x^jbib_001_u, typed by hand"
    );
    assert!(reference::is_reference("models/jacket.ref.json"));
    assert!(reference::is_reference("MODELS/JACKET.REF.JSON"));
    assert!(!reference::is_reference("models/jacket.ydd"));
}

#[test]
fn a_file_of_another_format_or_a_newer_schema_or_a_path_that_leaves_the_project_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for (text, why) in [
        (
            r#"{"format":"x","schema":1,"form":"by_id","id":"a"}"#,
            "format",
        ),
        (
            r#"{"format":"openrage-reference","schema":9,"form":"by_id","id":"a"}"#,
            "newer",
        ),
        (
            r#"{"format":"openrage-reference","schema":1,"form":"here","path":"../x"}"#,
            "inside",
        ),
        (
            r#"{"format":"openrage-reference","schema":1,"form":"by_id","id":""}"#,
            "id",
        ),
        ("not json", "reference"),
    ] {
        let file = dir.path().join("bad.ref.json");
        fs::write(&file, text).unwrap();
        let refused = Reference::read(&file).unwrap_err();
        assert!(refused.contains(why), "{text}: {refused}");
    }
}

#[test]
fn a_reference_to_a_source_of_the_project_follows_a_rename_and_one_to_a_file_follows_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root);
    source(root, "sources/jacket.drawables.src", "id-jacket");
    fs::create_dir_all(root.join("files")).unwrap();
    fs::write(root.join("files/jacket.yld"), b"cloth").unwrap();
    let to_source = Reference::here("sources/jacket.drawables.src", Some("id-jacket"));
    let to_file = Reference::here("files/jacket.yld", None);

    assert_eq!(
        reference::resolve(&to_source, root),
        Resolved::Folder {
            path: root.join("sources/jacket.drawables.src")
        }
    );
    assert_eq!(
        reference::resolve(&to_file, root),
        Resolved::File {
            path: root.join("files/jacket.yld"),
            bytes: 5
        }
    );

    // The person renames the folder, and moves it.
    fs::create_dir_all(root.join("garments")).unwrap();
    fs::rename(
        root.join("sources/jacket.drawables.src"),
        root.join("garments/blazer.drawables.src"),
    )
    .unwrap();
    assert_eq!(
        reference::resolve(&to_source, root),
        Resolved::Folder {
            path: root.join("garments/blazer.drawables.src")
        },
        "the id is what it follows"
    );
    // A file has no id, so a rename is a file that is not there.
    fs::rename(root.join("files/jacket.yld"), root.join("files/blazer.yld")).unwrap();
    match reference::resolve(&to_file, root) {
        Resolved::Unresolved { wanted, why } => {
            assert_eq!(wanted, "files/jacket.yld");
            assert!(why.contains("not there"), "{why}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_source_whose_id_is_gone_is_unresolved_and_says_which_id() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let gone = Reference::here("sources/jacket.drawables.src", Some("id-gone"));

    match reference::resolve(&gone, dir.path()) {
        Resolved::Unresolved { why, .. } => assert!(why.contains("id-gone"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reference_to_another_project_is_stored_by_path_as_a_link_is_and_resolved_there() {
    let dir = tempfile::tempdir().unwrap();
    let (hub, brand) = (dir.path().join("hub"), dir.path().join("brand"));
    project(&hub);
    project(&brand);
    source(&brand, "art/logo.textures.src", "id-logo");
    let stored = Reference::to_other(&brand, &hub, "art/logo.textures.src", Some("id-logo"));

    let text = serde_json::to_string(&stored).unwrap();
    assert!(
        text.contains("\"../brand\""),
        "relative to the project that holds the reference: {text}"
    );
    assert_eq!(
        reference::resolve(&stored, &hub),
        Resolved::Folder {
            path: brand.join("art/logo.textures.src")
        }
    );
    // Their rename is followed by id, as ours is.
    fs::rename(
        brand.join("art/logo.textures.src"),
        brand.join("art/mark.textures.src"),
    )
    .unwrap();
    assert_eq!(
        reference::resolve(&stored, &hub),
        Resolved::Folder {
            path: brand.join("art/mark.textures.src")
        }
    );
    // The other project is moved away.
    fs::rename(&brand, dir.path().join("elsewhere")).unwrap();
    match reference::resolve(&stored, &hub) {
        Resolved::Unresolved { why, .. } => {
            assert!(
                why.contains("../brand") && why.contains("not there"),
                "{why}"
            )
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_id_reference_resolves_to_the_id_and_the_plan_says_in_a_sentence_that_the_file_is_not_in_the_project(
) {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let typed = Reference::by_id("cloth_jacket_001");

    let resolved = reference::resolve(&typed, dir.path());

    assert_eq!(
        resolved,
        Resolved::Named {
            id: "cloth_jacket_001".into()
        }
    );
    let sentence = resolved.sentence();
    assert!(
        sentence.contains("cloth_jacket_001") && sentence.contains("not in the project"),
        "{sentence}"
    );
    assert!(Resolved::Unresolved {
        wanted: "x".into(),
        why: "y".into()
    }
    .sentence()
    .contains('y'));
}

#[test]
fn the_project_that_holds_a_content_folder_is_the_nearest_one_above_it() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let folder = dir.path().join("clothing/inner");
    fs::create_dir_all(&folder).unwrap();

    assert_eq!(reference::holder(&folder), Some(dir.path().to_path_buf()));
    assert_eq!(
        reference::holder(dir.path()),
        Some(dir.path().to_path_buf())
    );
    let lone = tempfile::tempdir().unwrap();
    assert_eq!(reference::holder(lone.path()), None);
}
