//! The empty textures of a garment: adding one, and putting the empty ones after the textures that have a file.
//!
//! ⚠ Thin by design. An empty texture is a texture of the garment with no file (`or_wardrobe::variations::reserve`), filled by the Fill button
//! of its row; what each does to the pack, and the Undo step each is, are the state's.
//!
//! One name on every screen: an empty texture. A place is a number a garment of a slot holds, in the Rules window.

use or_wardrobe::{variations, Garment};

use crate::ui::accessible_name::named;
use crate::ui::theme;

/// What the buttons of the places asked for.
pub(super) enum Ask {
    /// Add one more empty texture after the textures.
    Hold,
    /// Put the empty textures after the textures that have a file.
    Last,
}

/// The buttons for the empty textures among the textures of `g`; what one asked for goes in `asked`.
///
/// ⚠ A button that cannot run is greyed with the reason in its tip: no empty texture that stands before one that has a file leaves nothing to move.
pub(super) fn draw(ui: &mut egui::Ui, g: &Garment, asked: &mut Option<(String, Ask)>) {
    let held = g.variations.iter().filter(|v| v.texture.is_none()).count();
    // An empty texture that stands before a texture that has a file.
    let scattered = (g.variations.iter())
        .skip_while(|v| v.texture.is_some())
        .any(|v| v.texture.is_some());
    ui.horizontal_wrapped(|ui| {
        let hold = ui.add_enabled(
            g.variations.len() < variations::MOST,
            egui::Button::new(tr!("Add an empty texture")),
        );
        let hold = named(hold, tr!("Add an empty texture"))
            .on_hover_text(
                tr!("Add the next texture letter with no file, as an empty place holds a drawable number: the game counts it, the textures after it keep their letters, and a texture you add later can take it"),
            )
            .on_disabled_hover_text(tr!("A garment has at most {most} textures", most = variations::MOST));
        if hold.clicked() {
            *asked = Some((g.id.clone(), Ask::Hold));
        }
        let last = ui.add_enabled(scattered, egui::Button::new(tr!("Move the empty textures to the end")));
        let last = named(last, tr!("Move the empty textures to the end"))
            .on_hover_text(
                tr!("The textures that have a file take the lowest letters and the empty ones follow, each group in the order it has. A texture that moves changes its letter"),
            )
            .on_disabled_hover_text(tr!("No empty texture stands before a texture with a file"));
        if last.clicked() {
            *asked = Some((g.id.clone(), Ask::Last));
        }
    });
    ui.label(
        egui::RichText::new(match held {
            0 => tr!("An empty texture keeps its letter for a texture you add later").to_string(),
            1 => tr!("1 empty texture: its letter is kept for a texture you add later").to_string(),
            n => tr!(
                "{n} empty textures: their letters are kept for textures you add later",
                n = n
            ),
        })
        .small()
        .color(theme::TEXT_SECONDARY),
    );
}
