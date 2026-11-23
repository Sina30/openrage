//! What a project does about a member that is gone: a `.src` folder that moved is pointed at with a
//! folder picker where any other member takes a file. Taking a member out and putting it back is the
//! project's History (`the_project_history`).

#[test]
fn a_source_folder_that_moved_is_asked_for_as_a_folder_and_any_other_member_as_a_file() {
    use std::path::Path;
    assert!(super::workbench::asks_for_a_folder(Path::new(
        "a/logo.textures.src"
    )));
    assert!(super::workbench::asks_for_a_folder(Path::new(
        "lamp.drawable.src"
    )));
    assert!(!super::workbench::asks_for_a_folder(Path::new(
        "a/client.lua"
    )));
    assert!(!super::workbench::asks_for_a_folder(Path::new("a/x.src")));
    assert!(!super::workbench::asks_for_a_folder(Path::new(
        "a/Logo.Textures.src"
    )));
}
