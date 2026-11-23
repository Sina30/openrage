//! What a guest adds to the editor: panels, commands and settings it registers, the widgets it
//! describes each frame, and the points and lines it draws over the open document.
//!
//! The encodings are the host's (`or_plugin::surface`), and
//! `the_guest_library_encodes_what_the_host_encodes` holds the two to each other.

/// A widget of a panel's frame; the host draws it and answers with what the user did to it.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget<'a> {
    Label {
        text: &'a str,
    },
    Button {
        id: &'a str,
        text: &'a str,
    },
    Slider {
        id: &'a str,
        label: &'a str,
        min: f32,
        max: f32,
        value: f32,
    },
    Checkbox {
        id: &'a str,
        label: &'a str,
        checked: bool,
    },
    TextField {
        id: &'a str,
        label: &'a str,
        text: &'a str,
    },
    List {
        id: &'a str,
        items: &'a [&'a str],
        selected: Option<u32>,
    },
}

/// The `selected` a list carries for no selection.
const NO_SELECTION: u32 = u32::MAX;

fn put_str(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

impl Widget<'_> {
    /// The ABI's encoding of the widget.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Self::Label { text } => {
                out.push(0x01);
                put_str(&mut out, text);
            }
            Self::Button { id, text } => {
                out.push(0x02);
                put_str(&mut out, id);
                put_str(&mut out, text);
            }
            Self::Slider {
                id,
                label,
                min,
                max,
                value,
            } => {
                out.push(0x03);
                put_str(&mut out, id);
                put_str(&mut out, label);
                for v in [min, max, value] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            Self::Checkbox { id, label, checked } => {
                out.push(0x04);
                put_str(&mut out, id);
                put_str(&mut out, label);
                out.push(u8::from(*checked));
            }
            Self::TextField { id, label, text } => {
                out.push(0x05);
                put_str(&mut out, id);
                put_str(&mut out, label);
                put_str(&mut out, text);
            }
            Self::List {
                id,
                items,
                selected,
            } => {
                out.push(0x06);
                put_str(&mut out, id);
                out.extend_from_slice(&selected.unwrap_or(NO_SELECTION).to_le_bytes());
                out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                items.iter().for_each(|item| put_str(&mut out, item));
            }
        }
        out
    }
}

/// What a setting holds, with the value it starts at.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    Bool { default: bool },
    Number { min: f64, max: f64, default: f64 },
    Text { default: String },
}

/// A setting's value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl SettingValue {
    /// One whole value, or `None` for bytes that are not one of these shapes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (tag, rest) = bytes.split_first()?;
        match (tag, rest) {
            (0x01, [0]) => Some(Self::Bool(false)),
            (0x01, [1]) => Some(Self::Bool(true)),
            (0x02, rest) => Some(Self::Number(f64::from_le_bytes(rest.try_into().ok()?))),
            (0x03, rest) => {
                let (len, text) = rest.split_first_chunk::<4>()?;
                let text = text.get(..u32::from_le_bytes(*len) as usize)?;
                let whole = text.len() + 4 == rest.len();
                whole
                    .then(|| String::from_utf8(text.to_vec()).ok().map(Self::Text))
                    .flatten()
            }
            _ => None,
        }
    }
}

/// The ABI's encoding of a panel registration.
pub fn encode_panel(id: &str, title: &str) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, id);
    put_str(&mut out, title);
    out
}

/// The ABI's encoding of a command registration; no shortcut crosses as an empty one.
pub fn encode_command(id: &str, label: &str, shortcut: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, id);
    put_str(&mut out, label);
    put_str(&mut out, shortcut.unwrap_or(""));
    out
}

/// The ABI's encoding of a setting declaration.
pub fn encode_setting(id: &str, label: &str, kind: &SettingKind) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, id);
    put_str(&mut out, label);
    match kind {
        SettingKind::Bool { default } => out.extend_from_slice(&[0x01, u8::from(*default)]),
        SettingKind::Number { min, max, default } => {
            out.push(0x02);
            for v in [min, max, default] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        SettingKind::Text { default } => {
            out.push(0x03);
            put_str(&mut out, default);
        }
    }
    out
}

/// A point of an overlay's line, in the document's model space, with its own colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub at: [f32; 3],
    pub colour: [f32; 4],
}

/// One thing drawn over the open document, in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    /// A cross whose arms reach `size` from `at`, in model units.
    Point {
        at: [f32; 3],
        size: f32,
        colour: [f32; 4],
    },
    /// A segment whose colour runs from one vertex's to the other's.
    Line { from: Vertex, to: Vertex },
}

/// The ABI's encoding of an overlay.
pub fn encode_overlay(primitives: &[Primitive]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(primitives.len() as u32).to_le_bytes());
    let floats = |out: &mut Vec<u8>, values: &[f32]| {
        values
            .iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    };
    for primitive in primitives {
        match primitive {
            Primitive::Point { at, size, colour } => {
                out.push(0x01);
                floats(&mut out, at);
                floats(&mut out, &[*size]);
                floats(&mut out, colour);
            }
            Primitive::Line { from, to } => {
                out.push(0x02);
                for vertex in [from, to] {
                    floats(&mut out, &vertex.at);
                    floats(&mut out, &vertex.colour);
                }
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
mod sys {
    #[link(wasm_import_module = "openrage")]
    extern "C" {
        pub fn register_panel(spec: *const u8, len: usize) -> i32;
        pub fn register_command(spec: *const u8, len: usize) -> i32;
        pub fn declare_setting(spec: *const u8, len: usize) -> i32;
        pub fn setting_value(
            id: *const u8,
            id_len: usize,
            out: *mut u8,
            cap: usize,
            len: *mut u32,
        ) -> i32;
        pub fn draw_widget(
            widget: *const u8,
            len: usize,
            out: *mut u8,
            cap: usize,
            out_len: *mut u32,
        ) -> i32;
        pub fn set_overlay(primitives: *const u8, len: usize) -> i32;
        pub fn overlay_picking(on: i32) -> i32;
    }
}

#[cfg(target_arch = "wasm32")]
pub use calls::*;

#[cfg(target_arch = "wasm32")]
mod calls {
    use super::{
        encode_command, encode_overlay, encode_panel, encode_setting, sys, Primitive, SettingKind,
        SettingValue, Widget,
    };
    use crate::calls::answer;
    use crate::Status;

    /// Registers a panel, from `register` only; costs the panels grant.
    pub fn register_panel(id: &str, title: &str) -> Result<(), Status> {
        let spec = encode_panel(id, title);
        Status::check(unsafe { sys::register_panel(spec.as_ptr(), spec.len()) })
    }

    /// Registers a command, from `register` only; costs the commands grant. `shortcut` is a
    /// binding such as `Ctrl+Shift+K`, which the host drops if another command already holds it.
    pub fn register_command(id: &str, label: &str, shortcut: Option<&str>) -> Result<(), Status> {
        let spec = encode_command(id, label, shortcut);
        Status::check(unsafe { sys::register_command(spec.as_ptr(), spec.len()) })
    }

    /// Declares a setting, from `register` only; costs the settings grant.
    pub fn declare_setting(id: &str, label: &str, kind: &SettingKind) -> Result<(), Status> {
        let spec = encode_setting(id, label, kind);
        Status::check(unsafe { sys::declare_setting(spec.as_ptr(), spec.len()) })
    }

    /// The value of a setting this guest declared: its default until the user changes it.
    pub fn setting_value(id: &str) -> Result<SettingValue, Status> {
        let bytes = answer(|out, cap, len| unsafe {
            sys::setting_value(id.as_ptr(), id.len(), out, cap, len)
        })?;
        Ok(
            SettingValue::decode(&bytes)
                .expect("the host answers a value this ABI version encodes"),
        )
    }

    /// Replaces what is drawn over the open document; costs the overlay grant.
    pub fn set_overlay(primitives: &[Primitive]) -> Result<(), Status> {
        let encoded = encode_overlay(primitives);
        Status::check(unsafe { sys::set_overlay(encoded.as_ptr(), encoded.len()) })
    }

    /// Whether the viewport's clicks go to this guest's `picked` export, and select nothing.
    pub fn overlay_picking(on: bool) -> Result<(), Status> {
        Status::check(unsafe { sys::overlay_picking(i32::from(on)) })
    }

    /// Describes `widget` and returns the host's answer, from `draw_panel` only.
    fn describe(widget: &Widget<'_>) -> Result<Vec<u8>, Status> {
        let encoded = widget.encode();
        answer(|out, cap, len| unsafe {
            sys::draw_widget(encoded.as_ptr(), encoded.len(), out, cap, len)
        })
    }

    /// Each widget returns the value the user left it at since the last frame, or the one it was
    /// given where nothing changed it; a host that refuses the widget answers the given value.
    pub mod ui {
        use super::{describe, Widget};

        pub fn label(text: &str) {
            let _ = describe(&Widget::Label { text });
        }

        /// Whether the button was clicked since the last frame.
        pub fn button(id: &str, text: &str) -> bool {
            matches!(describe(&Widget::Button { id, text }).as_deref(), Ok([1]))
        }

        pub fn slider(id: &str, label: &str, min: f32, max: f32, value: f32) -> f32 {
            let widget = Widget::Slider {
                id,
                label,
                min,
                max,
                value,
            };
            match describe(&widget).as_deref() {
                Ok(&[a, b, c, d]) => f32::from_le_bytes([a, b, c, d]),
                _ => value,
            }
        }

        pub fn checkbox(id: &str, label: &str, checked: bool) -> bool {
            match describe(&Widget::Checkbox { id, label, checked }).as_deref() {
                Ok([flag]) => *flag == 1,
                _ => checked,
            }
        }

        pub fn text_field(id: &str, label: &str, text: &str) -> String {
            match describe(&Widget::TextField { id, label, text }) {
                Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|_| text.to_string()),
                Err(_) => text.to_string(),
            }
        }

        /// The selected item's index, `None` for no selection.
        pub fn list(id: &str, items: &[&str], selected: Option<u32>) -> Option<u32> {
            let widget = Widget::List {
                id,
                items,
                selected,
            };
            match describe(&widget).as_deref() {
                Ok(&[a, b, c, d]) => {
                    Some(u32::from_le_bytes([a, b, c, d])).filter(|s| *s != u32::MAX)
                }
                _ => selected,
            }
        }
    }
}
