//! A file changed and saved: text typed and undone, a texture replaced, a model converted and exported, and what a close asks of unsaved work.

use std::path::PathBuf;

use super::{corpus, run, Flow, Step};

const HANDLING: &str =
    "<?xml version=\"1.0\"?>\n<Item>\n  <fMass value=\"1500.0\" />\n  <handlingName>ADDER</handlingName>\n</Item>\n";

/// A metadata text file of the flow's own, open in front.
fn a_text_file(at: &mut Flow) -> Result<PathBuf, String> {
    let file = at.scratch.path().join("handling.meta");
    std::fs::write(&file, HANDLING).unwrap();
    at.open_file(&file)?;
    Ok(file)
}

fn dirty(at: &Flow) -> bool {
    (at.shell.state.active_document()).is_some_and(|d| d.dirty)
}

/// The text editor given the keyboard and `text` typed at the caret.
fn type_into_the_text(at: &mut Flow, text: &str) -> Step {
    // A metadata file opens on its fields: its text is the tab behind them.
    at.click("Text")?;
    // The editor is the one text field named Text: its tab is a button of the same name.
    let editor = (at.shell.described.iter())
        .find(|w| w.role == egui::accesskit::Role::TextInput && w.name.as_deref() == Some("Text"))
        .cloned()
        .ok_or_else(|| {
            format!(
                "the text editor is no text field a screen reader can find: {:?}",
                at.names()
            )
        })?;
    at.say("click the text editor");
    at.press(editor.bounds.center(), egui::PointerButton::Primary, 1);
    at.type_text(text);
    Ok(())
}

#[test]
fn f60_text_is_typed_into_a_file_and_saved() {
    run("f60_edit_text_and_save", |at| {
        let file = a_text_file(at)?;
        type_into_the_text(at, "<!-- tuned -->")?;
        if !dirty(at) {
            return Err("typing left the document clean".into());
        }
        at.menu("File", "Save Ctrl+S")?;
        at.wait("the file saved", |flow| !dirty(flow))?;
        let written = std::fs::read_to_string(&file).unwrap();
        if !written.contains("<!-- tuned -->") {
            return Err(format!("the file on disk lacks what was typed: {written}"));
        }
        Ok(())
    });
}

#[test]
fn f61_an_edit_is_undone_and_redone_from_the_edit_menu() {
    run("f61_undo_redo", |at| {
        a_text_file(at)?;
        type_into_the_text(at, "ZZZ")?;
        let typed = |flow: &Flow| {
            (flow.shell.state.active_document()).is_some_and(|d| match &d.payload {
                crate::state::DocPayload::Text(t) => t.text.to_string().contains("ZZZ"),
                crate::state::DocPayload::Meta(m) => m.text.text.to_string().contains("ZZZ"),
                _ => false,
            })
        };
        if !typed(at) {
            return Err("what was typed is not in the document".into());
        }
        at.menu("Edit", "Undo")?;
        if typed(at) {
            return Err("Edit ▸ Undo left the typed text in the document".into());
        }
        at.menu("Edit", "Redo")?;
        if !typed(at) {
            return Err("Edit ▸ Redo did not bring the typed text back".into());
        }
        Ok(())
    });
}

#[test]
fn f62_closing_a_changed_file_asks_and_dont_save_drops_the_change() {
    run("f62_close_dirty", |at| {
        let file = a_text_file(at)?;
        type_into_the_text(at, "ZZZ")?;
        at.click("Close")?;
        at.look("the close question");
        at.click("Don't save")?;
        at.wait("the file closed", |flow| {
            flow.shell.state.documents.is_empty()
        })?;
        if std::fs::read_to_string(&file).unwrap() != HANDLING {
            return Err("Don't save wrote the file".into());
        }
        Ok(())
    });
}

#[test]
fn f63_closing_a_changed_file_and_cancelling_keeps_it_open() {
    run("f63_close_dirty_cancel", |at| {
        a_text_file(at)?;
        type_into_the_text(at, "ZZZ")?;
        at.click("Close")?;
        at.click("Cancel")?;
        if at.shell.state.documents.is_empty() || !dirty(at) {
            return Err("Cancel closed the file or dropped its change".into());
        }
        Ok(())
    });
}

#[test]
fn f64_a_changed_file_is_saved_under_another_name() {
    run("f64_save_as_text", |at| {
        let file = a_text_file(at)?;
        type_into_the_text(at, "<!-- tuned -->")?;
        let copy = at.scratch.path().join("handling_tuned.meta");
        if !at.save_as(&copy)? {
            return Err("Save as… is greyed on a text file".into());
        }
        if std::fs::read_to_string(&file).unwrap() != HANDLING {
            return Err("Save as… wrote the first file too".into());
        }
        if at.active_name() != "handling_tuned.meta" {
            at.note(format!(
                "after Save as… the tab is still named {}",
                at.active_name()
            ));
        }
        Ok(())
    });
}

#[test]
fn f65_a_texture_is_selected_and_replaced_from_an_image() {
    run("f65_replace_texture", |at| {
        at.open_file(&corpus("ytd")?)?;
        let image = at.scratch.path().join("flat.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 40, 40, 255]))
            .save(&image)
            .map_err(|e| e.to_string())?;
        let first = (at.names().into_iter())
            .find(|n| n.starts_with("ap_") || n.starts_with("cs_"))
            .ok_or("no texture tile is named")?;
        at.click(&first)?;
        at.look("a texture selected");
        at.pick(&image);
        at.click("Replace from image…")?;
        at.settle();
        at.look("after Replace from image…");
        at.wait("the dictionary changed", dirty)?;
        let copy = at.scratch.path().join("replaced.ytd");
        if !at.save_as(&copy)? {
            return Err("Save as… is greyed on a changed texture dictionary".into());
        }
        at.close_document()?;
        at.open_file(&copy)?;
        at.expect("64×64")
    });
}

#[test]
fn f66_a_model_is_converted_to_enhanced_and_the_copy_opened() {
    run("f66_convert_to_enhanced", |at| {
        at.open_file(&corpus("ydr")?)?;
        let copy = at.scratch.path().join("enhanced.ydr");
        let out = copy.clone();
        at.click("Convert to Enhanced…")?;
        at.wait_for("Convert…")?;
        at.pick(&out);
        at.click("Convert…")?;
        at.settle();
        at.look("after Convert…");
        let written = copy.clone();
        at.wait("the Enhanced copy written", move |_| written.exists())?;
        at.pick(&copy);
        at.menu("File", "Open file…")?;
        at.wait_for("switch to GTA V (Enhanced)")?;
        at.say("a file of the other edition asks before it mounts that edition's install");
        at.click("switch to GTA V (Enhanced)")?;
        at.wait("the Enhanced copy open", move |flow| {
            (flow.shell.state.active_document()).is_some_and(|d| d.path.as_ref() == Some(&copy))
        })?;
        at.expect("Enhanced, version")
    });
}

#[test]
fn f67_a_model_is_exported_to_gltf() {
    run("f67_export_gltf", |at| {
        at.open_file(&corpus("ydr")?)?;
        let out = at.scratch.path().join("bridge.glb");
        at.click("Export to glTF…")?;
        at.wait("Export… ready", |flow| {
            flow.find("Export…").is_ok_and(|w| !w.disabled)
        })?;
        at.pick(&out);
        at.click("Export…")?;
        let written = out.clone();
        at.wait("the .glb written", move |_| written.exists())
    });
}

#[test]
fn f68_a_file_dropped_on_the_window_opens() {
    run("f68_drop_a_file", |at| {
        let file = corpus("ytd")?;
        at.say(format!("drop {}", file.display()));
        at.shell
            .drop_files(egui::pos2(800.0, 450.0), vec![file.clone()]);
        at.wait("the dropped file open", move |flow| {
            (flow.shell.state.active_document()).is_some_and(|d| d.path.as_ref() == Some(&file))
        })?;
        at.expect("Texture Dictionary")
    });
}

#[test]
fn f69_a_closed_tab_is_reopened_from_the_palette() {
    run("f69_reopen_closed_tab", |at| {
        a_text_file(at)?;
        at.close_document()?;
        at.wait("no document", |flow| flow.shell.state.documents.is_empty())?;
        at.menu("Edit", "Command palette…")?;
        at.type_text("reopen closed");
        at.key(egui::Key::Enter);
        at.wait("the tab back", |flow| flow.active_name() == "handling.meta")
    });
}

#[test]
fn f70_every_open_file_is_closed_at_once() {
    run("f70_close_all", |at| {
        a_text_file(at)?;
        at.open_file(&corpus("ytd")?)?;
        at.menu("File", "Close all")?;
        at.wait("no document", |flow| flow.shell.state.documents.is_empty())?;
        at.expect("Open an install")
    });
}

#[test]
fn f71_a_number_of_a_metadata_file_is_changed_in_its_fields() {
    run("f71_edit_a_meta_field", |at| {
        a_text_file(at)?;
        at.look_at_controls("the fields of the file");
        let number = (at.shell.described.iter())
            .find(|w| {
                w.role == egui::accesskit::Role::SpinButton && w.name.as_deref() == Some("fMass")
            })
            .cloned()
            .ok_or_else(|| format!("no number field is named fMass: {:?}", at.names()))?;
        at.say("double-click the number named fMass");
        at.press(number.bounds.center(), egui::PointerButton::Primary, 2);
        at.type_text("9");
        at.key(egui::Key::Enter);
        at.look("after typing into fMass");
        if !dirty(at) {
            return Err(
                "double-clicking a number of a metadata file and typing changes nothing".into(),
            );
        }
        Ok(())
    });
}
