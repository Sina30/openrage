//! The power features of the Wardrobe as a person meets them: the filter reads a query and the pack keeps one as a group of the list, a bulk edit
//! says what it would change before it is made and is one step of Undo, two versions of the pack are reviewed read-only, a garment is credited
//! in the `asset.json` of its sources, and a whole pack leaves as one package and comes in as an equal pack.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are, as a person meets it.

use std::path::Path;

use or_wardrobe::edit::Edit;
use or_wardrobe::query::SavedQuery;

use super::frames_of_the_shell::Shell;
use super::the_wardrobe_list_ergonomics_on_screen::{
    id_at, idle, read, rows, run, type_into, with_jackets,
};
use super::the_wardrobe_operations_on_screen::{click_on, garments, names, shows, until};
use crate::platform::file_dialogs;
use crate::ui::commands::Action;
use crate::ui::panels::wardrobe::WardrobeUi;
use crate::ui::theme;

/// Put `text` in the filter of the Wardrobe, as typing it would leave it.
fn filter(shell: &mut Shell, text: &str) {
    let mut ui = shell.state.panels.take::<WardrobeUi>();
    ui.filter = text.to_string();
    shell.state.panels.put(ui);
    shell.settled();
}

/// The tags of the garments, in the pack's order.
fn tags(shell: &Shell) -> Vec<Vec<String>> {
    (read(shell).wardrobe.garments.iter())
        .map(|g| g.tags.clone())
        .collect()
}

/// Tag the garments at `places` with `tag`, one edit each, and wait for them to be taken in.
fn tag(shell: &mut Shell, places: &[u32], tag: &str) {
    for place in places {
        let id = id_at(shell, *place);
        shell
            .state
            .edit_garment(id.clone(), Edit::AddTag(tag.into()));
        until(shell, "the tag", |s| {
            idle(s)
                && read(s)
                    .wardrobe
                    .garment(&id)
                    .is_some_and(|g| g.tags.iter().any(|t| t == tag))
        });
    }
}

#[test]
fn the_filter_reads_a_query_says_the_term_it_cannot_read_and_keeps_a_saved_query_as_a_group_of_the_list(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 4);
    tag(&mut shell, &[0, 2], "wool");

    filter(&mut shell, "tag:wool");
    assert_eq!(rows(&shell), 2, "the garments the pack states are tagged");
    filter(&mut shell, "tag:wool slot:lowr");
    assert_eq!(rows(&shell), 0);
    filter(&mut shell, "-tag:wool");
    assert_eq!(rows(&shell), 2, "a term turned round");
    filter(&mut shell, "wool");
    assert_eq!(rows(&shell), 2, "a word finds the garments it names");
    filter(&mut shell, "-wool");
    assert_eq!(
        rows(&shell),
        2,
        "a minus in front of a word leaves out the garments it finds"
    );
    // The models are sources the drop decompiled, counted from their glb without a cook: each draws a few triangles.
    filter(&mut shell, "polys>1");
    assert_eq!(rows(&shell), 4, "every model is counted");
    filter(&mut shell, "polys>1000000");
    assert_eq!(rows(&shell), 0, "and none draws a million triangles");

    // A term that does not read is said, and the list does not empty as it is typed.
    filter(&mut shell, "colour:red");
    assert!(
        shows(&shell, "not a term of a query"),
        "{:?}",
        names(&shell)
    );
    assert_eq!(rows(&shell), 4);

    // Saved under a name, it is a chip of the list that follows the pack.
    filter(&mut shell, "tag:wool");
    click_on(&mut shell, "Save query…");
    type_into(&mut shell, "Name of the saved query", "Wool");
    click_on(&mut shell, "Save");
    until(&mut shell, "the saved query", |s| {
        idle(s) && !read(s).wardrobe.queries.is_empty()
    });
    assert_eq!(
        read(&shell).wardrobe.queries,
        [SavedQuery {
            name: "Wool".into(),
            query: "tag:wool".into()
        }]
    );
    let text = std::fs::read_to_string(
        shell
            .state
            .wardrobe_pack()
            .unwrap()
            .folder
            .join("wardrobe.json"),
    )
    .unwrap();
    assert!(
        text.contains("\"query\": \"tag:wool\""),
        "a plain line that diffs well"
    );

    filter(&mut shell, "");
    // The saved queries are one menu, which names what it holds.
    click_on(&mut shell, "Saved queries");
    assert!(shows(&shell, "Saved query: Wool"), "{:?}", names(&shell));
    assert_eq!(rows(&shell), 4);
    click_on(&mut shell, "Saved query: Wool");
    assert_eq!(rows(&shell), 2, "the chip lists the garments it selects");
    // A garment tagged later joins the group: the query is read again, not remembered.
    tag(&mut shell, &[3], "wool");
    shell.settled();
    assert_eq!(rows(&shell), 3);

    // Forgetting is a step too, and Undo brings the query back.
    let steps = shell.state.workbench.wardrobe.undo.len();
    shell
        .state
        .forget_query(shell.state.wardrobe_module_id().unwrap(), "Wool".into());
    until(&mut shell, "the forgetting", |s| {
        idle(s) && read(s).wardrobe.queries.is_empty()
    });
    assert_eq!(shell.state.workbench.wardrobe.undo.len(), steps + 1);
    run(&mut shell, Action::UndoClothes);
    until(&mut shell, "the undo", |s| {
        idle(s) && read(s).wardrobe.queries.len() == 1
    });
}

#[test]
fn a_bulk_edit_on_a_query_says_what_it_would_change_first_and_one_undo_takes_all_of_it_back() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 4);
    tag(&mut shell, &[0, 2], "wool");
    let before = tags(&shell);
    let steps = shell.state.workbench.wardrobe.undo.len();

    filter(&mut shell, "tag:wool");
    click_on(&mut shell, "Edit the 2 listed…");
    assert!(shows(&shell, "Change to make"), "{:?}", names(&shell));
    assert!(
        shows(&shell, "Type the word, and this says what would change"),
        "nothing is said until a word is typed"
    );
    type_into(&mut shell, "Tag or group to change", "winter");
    assert!(
        shows(&shell, "Would change all 2 garments"),
        "{:?}",
        names(&shell)
    );
    assert_eq!(tags(&shell), before, "a dry run writes nothing");

    click_on(&mut shell, "Apply");
    until(&mut shell, "the edit", |s| idle(s) && tags(s) != before);
    let after = tags(&shell);
    assert_eq!(
        after
            .iter()
            .filter(|t| t.contains(&"winter".to_string()))
            .count(),
        2
    );
    assert_eq!(
        shell.state.workbench.wardrobe.undo.len(),
        steps + 1,
        "two garments edited are one step"
    );

    run(&mut shell, Action::UndoClothes);
    until(&mut shell, "the undo", |s| idle(s) && tags(s) == before);
    assert_eq!(garments(&shell), 4);
}

#[test]
fn the_review_of_two_versions_names_what_was_edited_between_them_and_changes_nothing() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    tag(&mut shell, &[0], "wool");
    tag(&mut shell, &[1], "silk");
    let held = std::fs::read(
        shell
            .state
            .wardrobe_pack()
            .unwrap()
            .folder
            .join("wardrobe.json"),
    )
    .unwrap();

    run(&mut shell, Action::ReviewVersions);
    assert!(
        shows(&shell, "Read-only: nothing here changes the pack."),
        "{:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "garments: 1 changed"),
        "the newest earlier version against the pack as it is: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "tags: none → silk"), "{:?}", names(&shell));
    assert!(
        !shows(&shell, "tags: none → wool"),
        "the older edit is in both versions"
    );
    assert!(shows(&shell, "Pictures of both sides…"));
    assert_eq!(
        std::fs::read(
            shell
                .state
                .wardrobe_pack()
                .unwrap()
                .folder
                .join("wardrobe.json")
        )
        .unwrap(),
        held,
        "a review writes nothing"
    );
    click_on(&mut shell, "Close");
    assert!(!shows(&shell, "Read-only: nothing here changes the pack."));
}

/// The model sources of the garments, as the pack names them.
fn model_sources(shell: &Shell) -> Vec<String> {
    (read(shell).wardrobe.garments.iter())
        .filter_map(|g| g.model.clone())
        .collect()
}

#[test]
fn a_garment_is_credited_in_the_asset_json_of_its_sources() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 2);
    let module = shell.state.wardrobe_module_id().unwrap();
    let id = id_at(&shell, 1);
    shell.state.select_garment(&module, Some(id));
    shell.settled();

    run(&mut shell, Action::CreditGarment);
    assert!(
        shows(&shell, "Licence of the sources"),
        "{:?}",
        names(&shell)
    );
    type_into(&mut shell, "Author of the sources", "Jane Doe");
    type_into(&mut shell, "Licence of the sources", "CC-BY-4.0");
    // Nothing is stated yet, so there is nothing to remove; and what Undo does not cover is said above the buttons.
    let button = |shell: &super::frames_of_the_shell::Shell, name: &str| {
        (shell.described.iter())
            .find(|w| w.name.as_deref() == Some(name))
            .cloned()
            .unwrap_or_else(|| panic!("no `{name}`: {:?}", names(shell)))
    };
    assert!(button(&shell, "Remove credit").disabled);
    assert!(shows(&shell, "Undo does not take it back"));
    click_on(&mut shell, "Save credit");
    until(&mut shell, "the credit", idle);

    let folder = shell.state.wardrobe_pack().unwrap().folder.clone();
    let credited = |shell: &super::frames_of_the_shell::Shell| -> Vec<(String, serde_json::Value)> {
        model_sources(shell)
            .into_iter()
            .filter(|m| m.ends_with(".drawables.src"))
            .filter_map(|m| {
                let text =
                    std::fs::read_to_string(Path::new(&folder).join(&m).join("asset.json")).ok()?;
                let json: serde_json::Value = serde_json::from_str(&text).ok()?;
                json.get("credit")
                    .filter(|c| !c.is_null())
                    .cloned()
                    .map(|c| (m, c))
            })
            .collect()
    };
    let stated = credited(&shell);
    assert_eq!(
        stated.len(),
        1,
        "only the garment selected is credited: {stated:?}"
    );
    assert_eq!(stated[0].1["author"], "Jane Doe");
    assert_eq!(stated[0].1["licence"], "CC-BY-4.0");

    // The credit is taken away by a button of its own, which only a credit that is stated enables.
    run(&mut shell, Action::CreditGarment);
    assert!(!button(&shell, "Remove credit").disabled);
    click_on(&mut shell, "Remove credit");
    until(&mut shell, "the credit gone", idle);
    assert!(credited(&shell).is_empty(), "{:?}", credited(&shell));
}

#[test]
fn a_whole_pack_leaves_as_one_package_and_comes_in_as_an_equal_pack() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    tag(&mut shell, &[1], "wool");
    let package = dir.path().join("out").join("demo.openrage-pack");
    std::fs::create_dir_all(package.parent().unwrap()).unwrap();

    file_dialogs::answer_next(package.clone());
    run(&mut shell, Action::ExportPackPackage);
    until(&mut shell, "the package", |s| idle(s) && package.is_file());
    assert!(std::fs::metadata(&package).unwrap().len() > 0);

    file_dialogs::answer_next(package);
    run(&mut shell, Action::ImportPackPackage);
    until(&mut shell, "the second pack", |s| {
        s.state.clothing_modules().len() == 2
            && s.state.workbench.packs().len() == 2
            && s.state.workbench.packs().iter().all(|p| p.read.is_ok())
    });
    let packs: Vec<_> = shell
        .state
        .workbench
        .packs()
        .iter()
        .filter_map(|p| p.read.as_ref().ok())
        .collect();
    let ids = |read: &crate::state::workbench::PackRead| -> Vec<String> {
        read.wardrobe
            .garments
            .iter()
            .map(|g| g.id.clone())
            .collect()
    };
    assert_eq!(packs.len(), 2);
    // Two packs of one name claim the same collections, which no build goes through with: the one that came in is given a free name, and the toast says so.
    assert_ne!(
        packs[0].wardrobe.pack, packs[1].wardrobe.pack,
        "a build refuses two packs of one name"
    );
    let said = (shell.state.toasts.iter())
        .find(|t| t.message.starts_with("Unpacked the pack"))
        .expect("the unpacking says what it did")
        .clone();
    assert!(
        said.message
            .contains("because another pack of this project is already called"),
        "{}",
        said.message
    );
    assert_eq!(
        said.action,
        Some(crate::state::toasts::ToastAction::RenamePack),
        "the person can choose a name of their own from the toast"
    );
    assert_eq!(ids(packs[0]), ids(packs[1]), "the same garments, by id");
    assert_eq!(packs[0].wardrobe.queries, packs[1].wardrobe.queries);
    assert_eq!(
        packs[0]
            .wardrobe
            .garments
            .iter()
            .map(|g| &g.tags)
            .collect::<Vec<_>>(),
        packs[1]
            .wardrobe
            .garments
            .iter()
            .map(|g| &g.tags)
            .collect::<Vec<_>>()
    );

    // The button of the toast puts the keyboard in the name of the pack that came in.
    shell
        .state
        .run_toast_action(crate::state::toasts::ToastAction::RenamePack);
    shell.settled();
    let (_, focused) = shell.focused().expect("the keyboard is in a field");
    assert_eq!(focused, "Pack name");
}

#[test]
fn the_size_map_ranks_the_garments_of_the_build_that_states_a_tag_rule_and_counts_what_the_build_wrote(
) {
    use or_project::ModuleKind;
    use or_wardrobe::options::Options;
    use or_wardrobe::Depth;

    use crate::state::front_door::counted;
    use crate::state::workbench::LastBuild;
    use crate::ui::dock::Workspace;
    use crate::ui::panels::registry::ids;

    let human = or_job::human_bytes;
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_jackets(dir.path(), 3);
    tag(&mut shell, &[0], "singleplayer");
    let output = shell.state.project_carried.container.outputs[0].id.clone();
    let whole = shell.state.workbench.resources_of(&output)[0]
        .plan
        .totals
        .bytes;
    shell
        .state
        .edit_build(
            &output,
            "The build ships the singleplayer garments".into(),
            |o| {
                let (mut options, _) = Options::of(o);
                options.set("include-tags", "singleplayer").unwrap();
                options.store(o);
            },
        )
        .unwrap();
    until(&mut shell, "the plan of the build with its rule", |s| {
        s.state.workbench.is_scanned()
            && s.state.workbench.resources_of(&output)[0].plan.totals.bytes < whole
    });
    shell.state.active_workspace = Workspace::Project;
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::BUILD);
    shell.settled();

    // What the engine says the build's own plan weighs, and what the pack weighs planned without the rule.
    let folder = or_project::project_folder(shell.state.project_path.as_ref().unwrap());
    let module = (shell.state.project_carried.container.modules.iter())
        .find(|m| m.kind == ModuleKind::Clothing)
        .cloned()
        .unwrap();
    let build = shell
        .state
        .project_carried
        .container
        .output(&output)
        .cloned()
        .unwrap();
    let (pack, planned) =
        or_wardrobe::planned_for(&folder, &module, Depth::Measure, &build).unwrap();
    let own = or_wardrobe::sizes::sizes(&pack, &planned, &[]).bytes;
    let (pack, planned) =
        or_wardrobe::planned_with_model(&folder, &module, Depth::Measure).unwrap();
    let standard = or_wardrobe::sizes::sizes(&pack, &planned, &[]).bytes;
    assert_ne!(human(own), human(standard), "the rule leaves garments out");

    // Before a build: the garments are ranked by the plan of the build that states the rule, and the screen says the figures are planned.
    click_tall(&mut shell, "By garment");
    assert!(
        shows(&shell, &format!("{} in all", human(own))),
        "{:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "As planned"),
        "a source a build cooks counts its payload until it is cooked: {:?}",
        names(&shell)
    );

    // After a build: the files it wrote, at the sizes the cooks came out at.
    shell.state.build_selected();
    until(&mut shell, "the build", |s| {
        !s.state.workbench.builds().is_running()
    });
    let Some(LastBuild::Built(done)) = shell.state.workbench.builds().last(&output).cloned() else {
        panic!("the build did not end well");
    };
    let part = &done.parts[0];
    shell.state.workbench.want_scan();
    until(&mut shell, "the plan to know the build", |s| {
        s.state.workbench.resources_of(&output)[0]
            .since
            .as_ref()
            .is_some_and(or_project::Diff::is_empty)
    });
    let planned_bytes = shell.state.workbench.resources_of(&output)[0]
        .plan
        .totals
        .bytes;
    assert_ne!(
        human(part.bytes),
        human(planned_bytes),
        "the cooked files are not the payloads the plan counted"
    );
    // A build that went through folds the size map away, as it does the plan: open it again.
    click_tall(&mut shell, "Size map");
    click_tall(&mut shell, "By content");
    let total = format!(
        "Total: {}, {}",
        counted(part.files as usize, "file"),
        human(part.bytes)
    );
    assert!(shows(&shell, &total), "{total}: {:?}", names(&shell));
    assert!(shows(&shell, "As the last build wrote them"));
    click_tall(&mut shell, "By garment");
    let manifest = std::fs::metadata(part.folder.join("fxmanifest.lua"))
        .unwrap()
        .len();
    let all = format!("{} in all", human(part.bytes - manifest));
    assert!(shows(&shell, &all), "{all}: {:?}", names(&shell));
}

/// Click the widget named `name` in a window tall enough to hold the whole Build tab, where the section it sits in is not scrolled away.
fn click_tall(shell: &mut Shell, name: &str) {
    const TALL: (u32, u32) = (1600, 2600);
    shell.settled_sized(TALL);
    let at = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
        .bounds
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(TALL, vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw_sized(TALL, vec![button(false)]);
    shell.settled_sized(TALL);
}
