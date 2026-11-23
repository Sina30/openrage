use winit::{event_loop::ActiveEventLoop, window::Window};

#[derive(Debug, Clone, Copy)]
pub(super) struct PhysicalRect {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) w: i32,
    pub(super) h: i32,
}

pub(super) fn centered_position(
    event_loop: &ActiveEventLoop,
    logical_size: (u32, u32),
) -> Option<winit::dpi::PhysicalPosition<i32>> {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())?;
    let scale = monitor.scale_factor();
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();
    let physical_w = (logical_size.0 as f64 * scale) as i32;
    let physical_h = (logical_size.1 as f64 * scale) as i32;
    let x = monitor_pos.x + ((monitor_size.width as i32 - physical_w) / 2).max(0);
    let y = monitor_pos.y + ((monitor_size.height as i32 - physical_h) / 2).max(0);
    Some(winit::dpi::PhysicalPosition::new(x, y))
}

pub(super) fn is_sane_position(pos: (i32, i32)) -> bool {
    const MAX_REASONABLE: i32 = 20_000;
    pos.0.abs() < MAX_REASONABLE && pos.1.abs() < MAX_REASONABLE
}

pub(super) fn center_inside_bounds(
    bounds: Option<PhysicalRect>,
    logical_size: (u32, u32),
    event_loop: &ActiveEventLoop,
) -> Option<winit::dpi::PhysicalPosition<i32>> {
    let bounds = match bounds {
        Some(b) => b,
        None => return centered_position(event_loop, logical_size),
    };
    let scale = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let physical_w = (logical_size.0 as f64 * scale).round() as i32;
    let physical_h = (logical_size.1 as f64 * scale).round() as i32;
    let x = bounds.x + (bounds.w - physical_w) / 2;
    let y = bounds.y + (bounds.h - physical_h) / 2;
    Some(winit::dpi::PhysicalPosition::new(x, y))
}

pub(super) fn host_window_bounds(window: &Window) -> Option<PhysicalRect> {
    let pos = window.outer_position().ok()?;
    if !is_sane_position((pos.x, pos.y)) {
        return None;
    }
    let size = window.inner_size();
    Some(PhysicalRect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minimised_window_sentinel_is_rejected_and_real_positions_are_not() {
        assert!(!is_sane_position((-32000, -32000)));
        assert!(!is_sane_position((0, -32000)));
        assert!(!is_sane_position((-32000, 0)));

        assert!(is_sane_position((0, 0)));
        assert!(
            is_sane_position((-1920, 0)),
            "a monitor left of the primary"
        );
        assert!(is_sane_position((3840, 2160)));
        assert!(is_sane_position((19_999, -19_999)), "just inside the limit");
        assert!(!is_sane_position((20_000, 0)), "just outside it");
    }

    #[test]
    fn a_window_centres_on_the_middle_of_the_bounds_it_is_given() {
        let bounds = PhysicalRect {
            x: 100,
            y: 200,
            w: 1000,
            h: 800,
        };
        let (w, h) = (400i32, 300i32);
        let x = bounds.x + (bounds.w - w) / 2;
        let y = bounds.y + (bounds.h - h) / 2;
        assert_eq!((x, y), (400, 450));
        assert_eq!(x + w / 2, bounds.x + bounds.w / 2);
        assert_eq!(y + h / 2, bounds.y + bounds.h / 2);
    }
}
