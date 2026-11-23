//! The floor under a ped that wears heels: a grid at the height the shoes raise the ped to, beside the ordinary grid at the ground.

use or_renderer::LineVertex;

use crate::state::AppState;

/// How far the floor reaches from the ped on each side, in metres.
const HALF: f32 = 1.0;

/// The cells of the floor, in metres: finer than the ordinary grid, which a metre apart would show one line either side.
const STEP: f32 = 0.25;

/// A warm line, so that it is not taken for the ordinary grid it floats over.
const COLOUR: [f32; 4] = [0.95, 0.65, 0.30, 0.9];

/// The lines of a floor `lift` metres over the ground; none for a ped that is not raised.
pub fn floor(lift: f32) -> Vec<LineVertex> {
    if lift <= 0.0 {
        return Vec::new();
    }
    let cells = (HALF / STEP).round() as i32;
    let mut lines = Vec::with_capacity(((2 * cells + 1) * 4) as usize);
    for i in -cells..=cells {
        let at = i as f32 * STEP;
        lines.push(LineVertex::new([at, -HALF, lift], COLOUR));
        lines.push(LineVertex::new([at, HALF, lift], COLOUR));
        lines.push(LineVertex::new([-HALF, at, lift], COLOUR));
        lines.push(LineVertex::new([HALF, at, lift], COLOUR));
    }
    lines
}

/// The floor under the dressed ped as it is dressed now.
pub fn of(state: &AppState) -> Vec<LineVertex> {
    floor(state.workbench.wardrobe.dressed.lift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_floor_lies_at_the_height_of_the_lift_and_a_ped_that_is_not_raised_has_none() {
        assert!(floor(0.0).is_empty() && floor(-0.1).is_empty());
        let lines = floor(0.15);
        assert!(!lines.is_empty() && lines.len().is_multiple_of(2));
        assert!(lines.iter().all(|v| v.position[2] == 0.15));
        let reach = lines
            .iter()
            .flat_map(|v| [v.position[0].abs(), v.position[1].abs()])
            .fold(0.0, f32::max);
        assert_eq!(reach, HALF, "it reaches a metre either way");
    }
    #[test]
    #[ignore = "needs a GPU adapter: acquisition can hang, not just fail"]
    fn the_viewport_draws_the_floor_in_the_wardrobe_only_and_only_while_the_ped_is_raised() {
        use crate::ui::dock::Workspace;
        const WHAT: &str = "the_viewport_draws_the_floor_in_the_wardrobe_only";
        let Some((device, queue)) = crate::render::test_device(WHAT) else {
            return;
        };
        let mut bridge = crate::render::RenderBridge::new(
            std::sync::Arc::new(device),
            std::sync::Arc::new(queue),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let mut state = AppState::from_prefs(crate::state::Prefs::default());
        state.workbench.wardrobe.dressed.lift = 0.15;
        let floor_lines = |bridge: &crate::render::RenderBridge| {
            (bridge.line_vertices().iter())
                .filter(|v| v.colour == COLOUR && v.position[2] == 0.15)
                .count()
        };
        state.active_workspace = Workspace::Model;
        bridge.refresh_lines(&state);
        assert_eq!(
            floor_lines(&bridge),
            0,
            "another workspace has no ped to raise"
        );
        state.active_workspace = Workspace::Wardrobe;
        bridge.refresh_lines(&state);
        assert_eq!(floor_lines(&bridge), floor(0.15).len());
        state.workbench.wardrobe.dressed.lift = 0.0;
        bridge.refresh_lines(&state);
        assert_eq!(floor_lines(&bridge), 0);
    }
}
