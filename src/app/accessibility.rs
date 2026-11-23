//! A window's AccessKit tree, sent to the platform's screen reader.

use egui::accesskit::ActionRequest;
use egui_winit::accesskit_winit::{Event, WindowEvent};

/// What the event loop carries besides winit's own events.
#[derive(Debug)]
pub(super) enum UserEvent {
    AccessKit(Event),
}

impl From<Event> for UserEvent {
    fn from(event: Event) -> Self {
        Self::AccessKit(event)
    }
}

/// Starts or stops `ctx` building a tree as the platform asks, and returns the action a screen reader asked of the window, for its next frame's input.
///
/// ⚠ egui builds no tree until asked, so a window that never calls this tells a screen reader nothing.
pub(crate) fn on_event(ctx: &egui::Context, event: WindowEvent) -> Option<ActionRequest> {
    match event {
        WindowEvent::InitialTreeRequested => {
            ctx.enable_accesskit();
            ctx.request_repaint();
            None
        }
        WindowEvent::AccessibilityDeactivated => {
            ctx.disable_accesskit();
            None
        }
        WindowEvent::ActionRequested(request) => Some(request),
    }
}
