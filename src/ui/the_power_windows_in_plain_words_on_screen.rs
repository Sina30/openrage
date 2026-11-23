//! The power windows of the Wardrobe say what they act on and what they do in plain words: the Fix menu groups its commands by scope, the optimiser asks which
//! garments and calls nothing by an engine word, the credit is offered where the garment is and its fields and the review's two choices start at one edge, and
//! the sharing check can show the folder it talks about.
//!
//! ⚠ Read through AccessKit from whole frames of the shell and clicked where the names are, as a person meets it.

use or_wardrobe::edit::Edit;

use super::frames_of_the_shell::{key, Shell};
use super::the_wardrobe_list_ergonomics_on_screen::{id_at, idle, read, run, with_jackets};
use super::the_wardrobe_operations_on_screen::{click_on, names, shows, until};
use crate::state::workbench::{Concern, Question};
use crate::ui::commands::Action;
use crate::ui::theme;

/// A shell on `n` jackets with the first one selected.
fn with_the_first_selected(dir: &std::path::Path, n: u32) -> Shell {
    let mut shell = with_jackets(dir, n);
    let module = shell.state.wardrobe_module_id().unwrap();
    let first = id_at(&shell, 0);
    shell.state.show_garment(&module, first);
    shell.settled();
    shell
}

/// Where the left edge of the control `name` stands.
fn left_of(shell: &Shell, name: &str) -> f32 {
    (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
        .bounds
        .left()
}

#[test]
fn the_fix_menu_groups_by_scope_and_the_optimiser_asks_which_garments_in_plain_words() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_the_first_selected(dir.path(), 2);

    click_on(&mut shell, "Fix");
    for scope in [
        "The whole pack",
        "The garments picked, or all",
        "The garment selected",
    ] {
        assert!(shows(&shell, scope), "{scope}: {:?}", names(&shell));
    }
    // Each fix says what it does under its name, and the option of Fix all safe stands under that sentence, drawn as a control.
    let top_of = |shell: &Shell, text: &str| {
        (shell.described.iter())
            .find(|w| w.name.as_deref().is_some_and(|n| n.contains(text)))
            .unwrap_or_else(|| panic!("nothing says `{text}`: {:?}", names(shell)))
            .bounds
            .top()
    };
    let (fix, said, option) = (
        top_of(&shell, "Fix all safe"),
        top_of(&shell, "Mends what is safe to mend"),
        top_of(&shell, "Also add missing levels of detail"),
    );
    assert!(fix < said && said < option, "{fix} {said} {option}");
    for fix in [
        "Makes the textures use less memory",
        "Adds a normal map and a specular map",
        "Fits the textures in the memory you pick",
        "Fills in the shader values a material lacks",
    ] {
        assert!(shows(&shell, fix), "`{fix}`: {:?}", names(&shell));
    }
    let shapes = shell.settled();
    let tick = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Also add missing levels of detail"))
        .expect("the option")
        .bounds;
    assert!(
        shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(r)
            if r.stroke.width > 0.0 && r.stroke.color == theme::outline() && tick.contains_rect(r.rect))),
        "the box of the option is drawn with no outline, so it reads as greyed out"
    );
    shell.draw(vec![key(egui::Key::Escape)]);

    shell.state.ask_to_optimise();
    shell.settled();
    for text in [
        "The garment picked",
        "The whole pack (2 garments)",
        "Store each picture in the smallest format that keeps it",
        "Add the smaller copies the game draws at a distance",
        "Round odd sizes to 256, 512, 1024, 2048…",
    ] {
        assert!(shows(&shell, text), "`{text}`: {:?}", names(&shell));
    }
    for jargon in ["BC1", "BC3", "mips", "power of two"] {
        assert!(
            !names(&shell).iter().any(|n| n.contains(jargon)),
            "`{jargon}` belongs in a tooltip, not on the window: {:?}",
            names(&shell)
        );
    }

    click_on(&mut shell, "The whole pack (2 garments)");
    let Some(Question::Optimise { garments, .. }) = &shell.state.workbench.asking else {
        panic!("the window is still asking");
    };
    assert!(
        garments.is_none(),
        "the scope was widened to the whole pack"
    );
}

#[test]
fn the_credit_is_offered_where_the_garment_is_and_its_fields_and_the_reviews_choices_start_at_one_edge(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_the_first_selected(dir.path(), 2);

    assert!(shows(&shell, "Credit garment…"), "{:?}", names(&shell));
    // The button stands at the foot of a long panel, so the window is asked for as the palette asks.
    run(&mut shell, Action::CreditGarment);
    let fields = [
        "Author of the sources",
        "Link to the sources",
        "Licence of the sources",
    ]
    .map(|name| left_of(&shell, name));
    assert!(
        fields.iter().all(|x| (x - fields[0]).abs() < 0.5),
        "the three fields start at {fields:?}"
    );
    shell.draw(vec![key(egui::Key::Escape)]);
    assert!(shell.state.workbench.asking.is_none());

    // A change makes a second version to review.
    let id = id_at(&shell, 0);
    shell
        .state
        .edit_garment(id.clone(), Edit::AddTag("wool".into()));
    until(&mut shell, "the tag", |s| {
        idle(s)
            && read(s)
                .wardrobe
                .garment(&id)
                .is_some_and(|g| g.tags.iter().any(|t| t == "wool"))
    });
    run(&mut shell, Action::ReviewVersions);
    let (from, to) = (
        left_of(&shell, "From version"),
        left_of(&shell, "To version"),
    );
    assert!((from - to).abs() < 0.5, "From starts at {from}, To at {to}");
    assert!(
        shows(&shell, "saved query added") || shows(&shell, "tags: none → wool"),
        "a line says what changed in words and an arrow: {:?}",
        names(&shell)
    );
}

#[test]
fn the_sharing_check_can_show_the_folder_it_talks_about() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_the_first_selected(dir.path(), 1);
    shell.state.workbench.asking = Some(Question::Sharing {
        concerns: vec![Concern {
            kind: or_content::sharing::Kind::PrivateFolder,
            text: "clothing/.openrage/ keeps the earlier versions of this pack. Leave it out when you zip the folder; Git already ignores it.".into(),
        }],
    });
    shell.settled();
    assert!(shows(&shell, "Show the folder"), "{:?}", names(&shell));
    click_on(&mut shell, "Show the folder");
    let project = shell.state.project_path.clone().expect("a project is open");
    assert!(crate::platform::shell::fake::was_revealed(&project));
}

#[test]
fn the_filter_says_what_it_understands_and_a_term_clicked_goes_in_the_field_with_its_value_to_type_over(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = with_the_first_selected(dir.path(), 2);

    click_on(&mut shell, "Filter terms");
    for term in [
        "tag:wool",
        "slot:jbib",
        "polys>5000",
        "is:duplicate",
        "-tag:wool",
    ] {
        let name = format!("Add {term} to the filter");
        assert!(shows(&shell, &name), "`{name}`: {:?}", names(&shell));
    }
    click_on(&mut shell, "Add tag:wool to the filter");
    let filter = |shell: &mut Shell| {
        shell
            .state
            .panels
            .get::<crate::ui::panels::wardrobe::WardrobeUi>()
            .filter
            .clone()
    };
    assert_eq!(filter(&mut shell), "tag:wool");
    // The keyboard is in the field with the value chosen, so the next letters replace it.
    let (_, focused) = shell.focused().expect("the keyboard is in a field");
    assert_eq!(focused, "Filter garments");
    shell.draw(vec![egui::Event::Text("silk".into())]);
    assert_eq!(filter(&mut shell), "tag:silk");
}
