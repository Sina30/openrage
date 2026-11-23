//! The Particle screen's panel for a project: the effects of every particle file of the pack.
//!
//! ⚠ **Thin by design.** Every rule, every read and every write is `or-particles`'s and `or-source`'s and runs through
//! `state.workbench.particles`; the panel reads what the last scan left and asks for an effect to be shown. The effects themselves are
//! edited in the Particle workspace's own panels, on the document of the file.

mod effects;

pub use effects::draw as draw_effects;

use crate::state::AppState;
use crate::ui::panel_state::PanelState;

/// What the panel keeps between frames.
#[derive(Default)]
pub struct ParticlesUi {
    /// The panel's rectangle in the pass it was last drawn, which is where a drop is the Particle screen's.
    drop: Option<(egui::Rect, u64)>,
}

impl PanelState for ParticlesUi {
    const KEY: &'static str = "particles";
}

/// The panel's rectangle as drawn this frame, none when it was not drawn.
pub fn drop_rect(state: &AppState) -> Option<egui::Rect> {
    state
        .panels
        .peek::<ParticlesUi>()?
        .drop
        .map(|(rect, _)| rect)
}

/// Whether a drop at `at` lands on the Effects panel, as drawn in this pass.
pub fn accepts_drop_at(ctx: &egui::Context, state: &AppState, at: egui::Pos2) -> bool {
    let pass = ctx.cumulative_pass_nr();
    state
        .panels
        .peek::<ParticlesUi>()
        .and_then(|ui| ui.drop)
        .is_some_and(|(rect, drawn)| drawn == pass && rect.contains(at))
}
