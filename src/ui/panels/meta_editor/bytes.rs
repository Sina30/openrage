//! A byte array, or a bitset too wide for a value's 128 bits, edited in the form as hex with its
//! length kept, so the writer puts it back in the place it was read from.

use or_io::meta::Value;

use crate::ui::theme;

/// The widget tag the derived field list gives a byte array.
pub(super) const TAG: &str = "(byte array)";

// ponytail: a longer array stays in the Text tab, so no frame formats kilobytes of hex.
const MOST: usize = 1024;

/// Draws `value`, a byte array, as hex; text that parses to as many bytes replaces it.
pub(super) fn draw(ui: &mut egui::Ui, value: &mut Value, dirty: &mut bool) {
    let (shown, count) = match value {
        Value::ByteArray(bytes) if bytes.len() <= MOST => (hex(bytes), bytes.len()),
        _ => {
            ui.label(
                egui::RichText::new(tr!("(raw bytes: edit them in the Text tab)"))
                    .small()
                    .color(theme::TEXT_DIM),
            );
            return;
        }
    };
    let id = ui.id().with("meta_bytes_buf");
    let mut buf: String = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown.clone());
    let response = ui
        .add(
            egui::TextEdit::singleline(&mut buf)
                .desired_width(320.0)
                .font(egui::TextStyle::Monospace),
        )
        .on_hover_text(tr_n!(
            "{count} byte, as two hex digits; the length cannot change",
            "{count} bytes, two hex digits each; the length cannot change",
            count
        ));
    let focused = response.has_focus() || response.gained_focus();
    if !focused && buf != shown {
        buf = shown;
    }
    ui.data_mut(|d| d.insert_temp::<String>(id, buf.clone()));
    match parsed(&buf, count) {
        Some(bytes) if response.changed() => {
            if !matches!(value, Value::ByteArray(held) if *held == bytes) {
                *value = Value::ByteArray(bytes);
                *dirty = true;
            }
        }
        Some(_) => {}
        None => {
            ui.label(
                egui::RichText::new(tr!("Not that many bytes of hex: the value is unchanged"))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let pairs: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    pairs.join(" ")
}

/// `count` bytes from `text`, two hex digits each with any spacing; `None` for any other text.
fn parsed(text: &str, count: usize) -> Option<Vec<u8>> {
    let mut digits: Vec<u8> = text.bytes().collect();
    digits.retain(|b| !b.is_ascii_whitespace());
    if digits.len() != 2 * count || !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    (digits.chunks(2))
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_of_the_same_length_parses_back_and_any_other_text_is_refused() {
        let bytes = vec![0x00, 0x7F, 0xA5, 0xFF];
        assert_eq!(hex(&bytes), "00 7F A5 FF");
        assert_eq!(parsed(&hex(&bytes), 4), Some(bytes.clone()));
        assert_eq!(parsed("007fa5FF", 4), Some(bytes));
        for wrong in ["00 7F A5", "00 7F A5 FF 01", "0 7F A5 FF", "00 7F A5 FG"] {
            assert_eq!(parsed(wrong, 4), None, "{wrong}");
        }
        assert_eq!(parsed("+0 7F A5 FF", 4), None, "a sign is no digit");
        assert_eq!(parsed("", 0), Some(Vec::new()));
    }

    /// Text typed into the field replaces the bytes, keeps their length and records one edit.
    #[test]
    fn typing_hex_into_the_field_writes_the_bytes_it_names() {
        let ctx = super::super::ctx::EditorCtx::for_test();
        let mut inst = or_io::meta::MetaInstance::new("T");
        inst.set("wide".to_string(), Value::ByteArray(vec![0x11; 3]));
        let fields = super::super::derived::fields_of(&inst);
        let egui_ctx = egui::Context::default();
        egui_ctx.enable_accesskit();
        let mut dirty = false;
        let frame = |inst: &mut or_io::meta::MetaInstance, dirty: &mut bool, events| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let output = egui_ctx.run_ui(input, |ui| {
                super::super::widgets::draw_struct(ui, &ctx, &fields, inst, dirty);
            });
            let update = output.platform_output.accesskit_update;
            (update.into_iter().flat_map(|u| u.nodes))
                .filter(|(_, n)| n.value() == Some("11 11 11"))
                .find_map(|(_, n)| n.bounds())
                .map(|b| egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
        };
        let typed = |text: &str| vec![egui::Event::Text(text.into())];
        frame(&mut inst, &mut dirty, Vec::new());
        let at = frame(&mut inst, &mut dirty, Vec::new());
        let at = at.expect("the field holding the bytes");
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut inst,
            &mut dirty,
            vec![egui::Event::PointerMoved(at), press(true)],
        );
        frame(&mut inst, &mut dirty, vec![press(false)]);
        let select_all = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        frame(&mut inst, &mut dirty, vec![select_all]);
        // Too short: the bytes are left as they are.
        frame(&mut inst, &mut dirty, typed("0A 1B"));
        assert_eq!(inst.get("wide"), Some(&Value::ByteArray(vec![0x11; 3])));
        assert!(!dirty);
        frame(&mut inst, &mut dirty, typed(" 2C"));
        assert_eq!(
            inst.get("wide"),
            Some(&Value::ByteArray(vec![0x0A, 0x1B, 0x2C]))
        );
        assert!(dirty);
        assert_eq!(ctx.edits.borrow().len(), 1, "one step to undo");
    }
}
