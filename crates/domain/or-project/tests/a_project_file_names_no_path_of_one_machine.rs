//! What Check before sharing reads from a project file: a path that is a drive, a share or a root, and a path that climbs out of the project folder, each with where in the file it is.

use or_project::check::{machine_paths, MachinePath, Why};

fn found(text: &str) -> Vec<MachinePath> {
    machine_paths(text.as_bytes()).expect("a project file")
}

#[test]
fn a_file_of_relative_paths_inside_its_folder_names_nothing_of_a_machine() {
    let text = r#"{
        "schema": 4,
        "template": "Clothing pack",
        "documents": [{ "path": "stream/a.ydd" }, { "path": "client/main.lua", "kind": "Text" }],
        "modules": [{ "id": "files", "kind": "files" }, { "id": "clothing", "kind": "clothing", "path": "clothing" }],
        "outputs": [{ "id": "fivem", "name": "police_pack", "take": ["files", "clothing"] }]
    }"#;
    assert_eq!(found(text), []);
}

#[test]
fn a_drive_a_share_a_root_and_a_climb_out_of_the_folder_are_each_found_where_they_stand() {
    let text = r#"{
        "documents": [{ "path": "stream/a.ydd" }, { "path": "D:\\Mods\\b.ytd" }],
        "modules": [
            { "id": "files", "kind": "files" },
            { "id": "other", "kind": "files", "link": true, "path": "D:/shared/pack" },
            { "id": "net", "kind": "files", "link": true, "path": "\\\\server\\share\\pack" },
            { "id": "home", "kind": "files", "link": true, "path": "/home/me/pack" },
            { "id": "up", "kind": "files", "link": true, "path": "../pack" }
        ]
    }"#;
    let at_and_why: Vec<(String, String, Why)> = found(text)
        .into_iter()
        .map(|m| (m.at, m.path, m.why))
        .collect();
    assert_eq!(
        at_and_why,
        [
            (
                "documents[1].path".to_string(),
                r"D:\Mods\b.ytd".to_string(),
                Why::Absolute
            ),
            (
                "modules[1].path".to_string(),
                "D:/shared/pack".to_string(),
                Why::Absolute
            ),
            (
                "modules[2].path".to_string(),
                r"\\server\share\pack".to_string(),
                Why::Absolute
            ),
            (
                "modules[3].path".to_string(),
                "/home/me/pack".to_string(),
                Why::Absolute
            ),
            (
                "modules[4].path".to_string(),
                "../pack".to_string(),
                Why::LeavesTheFolder
            ),
        ]
    );
}

#[test]
fn bytes_that_are_no_project_file_are_refused_with_a_sentence() {
    assert!(machine_paths(b"not json").is_err());
    assert!(
        machine_paths(b"[1, 2]").is_err(),
        "a project file is an object"
    );
}
