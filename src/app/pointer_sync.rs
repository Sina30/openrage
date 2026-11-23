#[cfg(target_os = "windows")]
pub(super) fn reconcile_mouse_buttons(ctx: &egui::Context, state: &mut egui_winit::State) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
    };

    fn os_pressed(vk: i32) -> bool {
        // SAFETY: takes an integer key code and no pointer; any value is accepted.
        let raw = unsafe { GetAsyncKeyState(vk) };
        (raw as u16) & 0x8000 != 0
    }

    let (buttons, last_pos) = ctx.input(|i| {
        (
            [
                (
                    egui::PointerButton::Primary,
                    i.pointer.button_down(egui::PointerButton::Primary),
                ),
                (
                    egui::PointerButton::Secondary,
                    i.pointer.button_down(egui::PointerButton::Secondary),
                ),
                (
                    egui::PointerButton::Middle,
                    i.pointer.button_down(egui::PointerButton::Middle),
                ),
                (
                    egui::PointerButton::Extra1,
                    i.pointer.button_down(egui::PointerButton::Extra1),
                ),
                (
                    egui::PointerButton::Extra2,
                    i.pointer.button_down(egui::PointerButton::Extra2),
                ),
            ],
            i.pointer
                .interact_pos()
                .or_else(|| i.pointer.latest_pos())
                .unwrap_or(egui::Pos2::ZERO),
        )
    });

    let vk_for = |b: egui::PointerButton| -> i32 {
        match b {
            egui::PointerButton::Primary => VK_LBUTTON as i32,
            egui::PointerButton::Secondary => VK_RBUTTON as i32,
            egui::PointerButton::Middle => VK_MBUTTON as i32,
            egui::PointerButton::Extra1 => VK_XBUTTON1 as i32,
            egui::PointerButton::Extra2 => VK_XBUTTON2 as i32,
        }
    };

    let modifiers = state.egui_input().modifiers;
    let input = state.egui_input_mut();
    for (button, egui_thinks_down) in buttons {
        if egui_thinks_down && !os_pressed(vk_for(button)) {
            input.events.push(egui::Event::PointerButton {
                pos: last_pos,
                button,
                pressed: false,
                modifiers,
            });
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn reconcile_mouse_buttons(_ctx: &egui::Context, _state: &mut egui_winit::State) {}

/// Put egui's pointer where the cursor is while files are dragged over the
/// window or dropped on it, so the drop lands on the row under the cursor.
pub(super) fn aim_file_drop(
    ctx: &egui::Context,
    state: &mut egui_winit::State,
    window: &winit::window::Window,
) {
    let input = state.egui_input();
    if input.hovered_files.is_empty() && input.dropped_files.is_empty() {
        return;
    }
    let Some((x, y)) = crate::platform::drag_drop::cursor_in_client(window) else {
        return;
    };
    let ppp = f64::from(egui_winit::pixels_per_point(ctx, window));
    state
        .egui_input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(
            (x / ppp) as f32,
            (y / ppp) as f32,
        )));
}

pub(super) fn synthesize_pointer_release(state: &mut egui_winit::State) {
    let modifiers = state.egui_input().modifiers;
    let input = state.egui_input_mut();
    let last_pos = last_pointer_pos(input);
    for button in [
        egui::PointerButton::Primary,
        egui::PointerButton::Secondary,
        egui::PointerButton::Middle,
        egui::PointerButton::Extra1,
        egui::PointerButton::Extra2,
    ] {
        input.events.push(egui::Event::PointerButton {
            pos: last_pos,
            button,
            pressed: false,
            modifiers,
        });
    }
    input.events.push(egui::Event::PointerGone);
}

fn last_pointer_pos(input: &egui::RawInput) -> egui::Pos2 {
    for event in input.events.iter().rev() {
        match event {
            egui::Event::PointerMoved(p) => return *p,
            egui::Event::PointerButton { pos, .. } if *pos != egui::Pos2::ZERO => return *pos,
            _ => continue,
        }
    }
    egui::Pos2::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(x: f32, y: f32) -> egui::Event {
        egui::Event::PointerMoved(egui::pos2(x, y))
    }

    fn pressed_at(x: f32, y: f32) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn queue(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            events,
            ..Default::default()
        }
    }

    #[test]
    fn the_last_known_position_wins_over_the_origin() {
        let input = queue(vec![
            moved(10.0, 10.0),
            pressed_at(40.0, 50.0),
            moved(80.0, 90.0),
        ]);
        assert_eq!(last_pointer_pos(&input), egui::pos2(80.0, 90.0));
    }

    #[test]
    fn a_button_event_supplies_a_position_when_no_move_follows_it() {
        let input = queue(vec![moved(10.0, 10.0), pressed_at(40.0, 50.0)]);
        assert_eq!(last_pointer_pos(&input), egui::pos2(40.0, 50.0));
    }

    #[test]
    fn a_button_event_at_the_origin_does_not_mask_a_real_earlier_position() {
        let input = queue(vec![moved(33.0, 44.0), pressed_at(0.0, 0.0)]);
        assert_eq!(last_pointer_pos(&input), egui::pos2(33.0, 44.0));
    }

    #[test]
    fn an_empty_queue_falls_back_to_the_origin() {
        let input = egui::RawInput::default();
        assert_eq!(last_pointer_pos(&input), egui::Pos2::ZERO);
    }
}
