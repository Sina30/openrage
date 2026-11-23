//! Documentation links on tooltips: a control the guide explains says which section does in its tooltip, and Shift+F1 with the pointer on it opens the guide there.
//!
//! The ask is left in the egui context and taken by [`take`] once a frame, so a control needs no access to the state to make one.

const ASKED: &str = "workbench_guide_asked";

/// The headings of [`docs/guides/Workbench_Guide.md`](../../docs/guides/Workbench_Guide.md) the controls of the Wardrobe lead to.
pub const ADD_YOUR_CLOTHES: &str = "Add your clothes";
pub const FIX_IT: &str = "See what is wrong and fix it";
pub const CHECK_ON_THE_PED: &str = "Check it on the ped";
pub const BUILD: &str = "Build";

/// The guide section that explains the command whose id is `id`, none where the guide says nothing of it.
pub fn section_of(id: &str) -> Option<&'static str> {
    match id {
        "add_clothing" | "add_clothes" | "add_clothes_folder" => Some(ADD_YOUR_CLOTHES),
        "fix_all_safe" | "optimise_textures" | "make_maps" | "budget_textures"
        | "complete_materials" | "repair_model" | "undo_clothes" => Some(FIX_IT),
        "build_project" | "build_all" => Some(BUILD),
        _ => None,
    }
}

/// The last line of a tooltip for a control the guide explains.
pub fn line(section: &str) -> String {
    tr!(
        "Shift+F1 opens the guide at \"{section}\"",
        section = section
    )
}

/// `hover` with the line that names the guide section.
pub fn hover(hover: &str, section: &str) -> String {
    format!("{hover}\n\n{}", line(section))
}

/// Ask for the guide at `section` when Shift+F1 is pressed with the pointer or the keyboard on `response`.
pub fn ask_on_shift_f1(response: &egui::Response, section: &str) {
    // The modifiers the key event carries, which are the ones held when it was pressed.
    let pressed = response.ctx.input(|i| {
        (i.events.iter()).any(|e| {
            matches!(e, egui::Event::Key { key: egui::Key::F1, pressed: true, modifiers, .. } if modifiers.shift)
        })
    });
    // The keyboard is where the control is for a person who tabbed to it, as the pointer is for one who points.
    if pressed && (response.contains_pointer() || response.has_focus()) {
        let ask = section.to_string();
        response
            .ctx
            .data_mut(|d| d.insert_temp(egui::Id::new(ASKED), ask));
    }
}

/// `response` with `text` and the guide line as its tooltip, and Shift+F1 on it asking for the guide at `section`.
pub fn tip(response: egui::Response, text: &str, section: &str) -> egui::Response {
    let response = response.on_hover_text(hover(text, section));
    ask_on_shift_f1(&response, section);
    response
}

/// The section a control asked for in this frame, taken so that it is asked once.
pub fn take(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new(ASKED)))
}
