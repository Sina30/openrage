//! What a plugin adds to the editor, and how each part crosses the boundary: the
//! panels, commands and settings it registers, the widgets it describes each
//! frame, the points and lines it draws, and the events and picks that come back.
//!
//! ⚠ egui never crosses: a guest describes a widget in this vocabulary and the
//! host draws it with whatever it draws with, so the vocabulary is the contract
//! and stays as small as the plugins that exist need. Every encoding is
//! little-endian and unpadded, as [`crate::abi`]'s values are, and a decoder
//! grows what it builds from the bytes it has rather than reserving from a
//! count the bytes have not yet shown to be there.

use crate::abi::CodecError;

/// The longest id, title, label or shortcut a registration may carry, in bytes.
pub const MAX_NAME: usize = 256;
/// The longest text a guest or a user puts in one widget or setting, in bytes.
pub const MAX_TEXT: usize = 4096;
/// The most items one list holds.
pub const MAX_ITEMS: usize = 4096;
/// The most widgets one frame of a panel holds.
pub const MAX_WIDGETS: usize = 1024;
/// The most points and lines one overlay holds.
pub const MAX_PRIMITIVES: usize = 1 << 18;
/// The most panels, commands or settings one plugin registers, each.
pub const MAX_REGISTERED: usize = 64;

/// A panel a plugin registers: its id within the plugin, and the title its tab shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSpec {
    pub id: String,
    pub title: String,
}

/// A command a plugin registers, with the binding it would like, for example `Ctrl+Shift+K`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
}

/// What a setting holds, with the value it starts at.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    Bool { default: bool },
    Number { min: f64, max: f64, default: f64 },
    Text { default: String },
}

/// A setting a plugin declares; Settings shows it on the plugin's page and the preferences keep its value.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingSpec {
    pub id: String,
    pub label: String,
    pub kind: SettingKind,
}

/// A setting's value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl SettingKind {
    pub fn default_value(&self) -> SettingValue {
        match self {
            Self::Bool { default } => SettingValue::Bool(*default),
            Self::Number { default, .. } => SettingValue::Number(*default),
            Self::Text { default } => SettingValue::Text(default.clone()),
        }
    }

    /// `value` as this kind holds it: a number kept to its range, a text to [`MAX_TEXT`], and `None` for another kind's value.
    pub fn keep(&self, value: SettingValue) -> Option<SettingValue> {
        match (self, value) {
            (Self::Bool { .. }, SettingValue::Bool(v)) => Some(SettingValue::Bool(v)),
            (Self::Number { min, max, .. }, SettingValue::Number(v)) if v.is_finite() => {
                Some(SettingValue::Number(v.clamp(*min, *max)))
            }
            (Self::Text { .. }, SettingValue::Text(v)) => {
                Some(SettingValue::Text(cut(v, MAX_TEXT)))
            }
            _ => None,
        }
    }
}

/// `text` cut to at most `max` bytes at a character.
pub fn cut(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut at = max;
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        text.truncate(at);
    }
    text
}

/// One widget of a panel's frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    Label {
        text: String,
    },
    Button {
        id: String,
        text: String,
    },
    Slider {
        id: String,
        label: String,
        min: f32,
        max: f32,
        value: f32,
    },
    Checkbox {
        id: String,
        label: String,
        checked: bool,
    },
    TextField {
        id: String,
        label: String,
        text: String,
    },
    List {
        id: String,
        items: Vec<String>,
        selected: Option<u32>,
    },
}

impl Widget {
    /// The id its events carry; a label has none.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Label { .. } => None,
            Self::Button { id, .. }
            | Self::Slider { id, .. }
            | Self::Checkbox { id, .. }
            | Self::TextField { id, .. }
            | Self::List { id, .. } => Some(id),
        }
    }
}

/// What the user did to a widget, which the guest's next call for that widget answers with.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Clicked { id: String },
    Slid { id: String, value: f32 },
    Checked { id: String, checked: bool },
    Typed { id: String, text: String },
    Chose { id: String, index: u32 },
}

impl Event {
    pub fn id(&self) -> &str {
        match self {
            Self::Clicked { id }
            | Self::Slid { id, .. }
            | Self::Checked { id, .. }
            | Self::Typed { id, .. }
            | Self::Chose { id, .. } => id,
        }
    }
}

/// A point of an overlay's line, in the document's model space, with its own colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub at: [f32; 3],
    pub colour: [f32; 4],
}

/// One thing a plugin draws over the open document, in model space.
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

/// What a click in the viewport landed on: the geometry of the level drawn, the triangle under the cursor and its corner nearest the hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub lod: u32,
    pub model: u32,
    pub geometry: u32,
    /// The index of the triangle among the geometry's, in index buffer order.
    pub polygon: u32,
    /// The index, in the geometry's vertex buffer, of the triangle's corner nearest the hit.
    pub vertex: u32,
}

/// What a guest registered while it started.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registrations {
    pub panels: Vec<PanelSpec>,
    pub commands: Vec<CommandSpec>,
    pub settings: Vec<SettingSpec>,
}

mod tag {
    pub const LABEL: u8 = 0x01;
    pub const BUTTON: u8 = 0x02;
    pub const SLIDER: u8 = 0x03;
    pub const CHECKBOX: u8 = 0x04;
    pub const TEXT_FIELD: u8 = 0x05;
    pub const LIST: u8 = 0x06;

    pub const BOOL: u8 = 0x01;
    pub const NUMBER: u8 = 0x02;
    pub const TEXT: u8 = 0x03;

    pub const POINT: u8 = 0x01;
    pub const LINE: u8 = 0x02;
}

/// The `selected` a list carries for no selection.
pub const NO_SELECTION: u32 = u32::MAX;

#[derive(Default)]
pub(crate) struct Out(pub(crate) Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn str(&mut self, text: &str) {
        self.u32(text.len() as u32);
        self.0.extend_from_slice(text.as_bytes());
    }

    fn vertex(&mut self, vertex: &Vertex) {
        vertex.at.iter().for_each(|v| self.f32(*v));
        vertex.colour.iter().for_each(|v| self.f32(*v));
    }
}

pub(crate) struct In<'a>(pub(crate) &'a [u8]);

impl<'a> In<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        if self.0.len() < n {
            return Err(CodecError::Truncated {
                wanted: n,
                had: self.0.len(),
            });
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        Ok(self.take(N)?.try_into().expect("take returned N bytes"))
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, CodecError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn f64(&mut self) -> Result<f64, CodecError> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    fn bool(&mut self) -> Result<bool, CodecError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CodecError::NotABool(other)),
        }
    }

    /// A string of at most `max` bytes.
    pub(crate) fn str(&mut self, max: usize) -> Result<String, CodecError> {
        let len = self.u32()? as usize;
        if len > max {
            return Err(CodecError::TooLarge);
        }
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| CodecError::NotUtf8)
    }

    fn vertex(&mut self) -> Result<Vertex, CodecError> {
        Ok(Vertex {
            at: [self.f32()?, self.f32()?, self.f32()?],
            colour: [self.f32()?, self.f32()?, self.f32()?, self.f32()?],
        })
    }

    pub(crate) fn finish(self) -> Result<(), CodecError> {
        match self.0.len() {
            0 => Ok(()),
            left => Err(CodecError::TrailingBytes(left)),
        }
    }
}

impl PanelSpec {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        out.str(&self.id);
        out.str(&self.title);
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut input = In(bytes);
        let spec = Self {
            id: input.str(MAX_NAME)?,
            title: input.str(MAX_NAME)?,
        };
        input.finish()?;
        Ok(spec)
    }
}

impl CommandSpec {
    /// An absent shortcut crosses as an empty string.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        out.str(&self.id);
        out.str(&self.label);
        out.str(self.shortcut.as_deref().unwrap_or(""));
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut input = In(bytes);
        let id = input.str(MAX_NAME)?;
        let label = input.str(MAX_NAME)?;
        let shortcut = Some(input.str(MAX_NAME)?).filter(|s| !s.is_empty());
        input.finish()?;
        Ok(Self {
            id,
            label,
            shortcut,
        })
    }
}

impl SettingSpec {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        out.str(&self.id);
        out.str(&self.label);
        match &self.kind {
            SettingKind::Bool { default } => {
                out.u8(tag::BOOL);
                out.u8(u8::from(*default));
            }
            SettingKind::Number { min, max, default } => {
                out.u8(tag::NUMBER);
                out.f64(*min);
                out.f64(*max);
                out.f64(*default);
            }
            SettingKind::Text { default } => {
                out.u8(tag::TEXT);
                out.str(default);
            }
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut input = In(bytes);
        let id = input.str(MAX_NAME)?;
        let label = input.str(MAX_NAME)?;
        let kind = match input.u8()? {
            tag::BOOL => SettingKind::Bool {
                default: input.bool()?,
            },
            tag::NUMBER => SettingKind::Number {
                min: input.f64()?,
                max: input.f64()?,
                default: input.f64()?,
            },
            tag::TEXT => SettingKind::Text {
                default: input.str(MAX_TEXT)?,
            },
            other => return Err(CodecError::UnknownTag(other)),
        };
        input.finish()?;
        Ok(Self { id, label, kind })
    }
}

impl SettingValue {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            Self::Bool(v) => {
                out.u8(tag::BOOL);
                out.u8(u8::from(*v));
            }
            Self::Number(v) => {
                out.u8(tag::NUMBER);
                out.f64(*v);
            }
            Self::Text(v) => {
                out.u8(tag::TEXT);
                out.str(v);
            }
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut input = In(bytes);
        let value = match input.u8()? {
            tag::BOOL => Self::Bool(input.bool()?),
            tag::NUMBER => Self::Number(input.f64()?),
            tag::TEXT => Self::Text(input.str(MAX_TEXT)?),
            other => return Err(CodecError::UnknownTag(other)),
        };
        input.finish()?;
        Ok(value)
    }
}

impl Widget {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            Self::Label { text } => {
                out.u8(tag::LABEL);
                out.str(text);
            }
            Self::Button { id, text } => {
                out.u8(tag::BUTTON);
                out.str(id);
                out.str(text);
            }
            Self::Slider {
                id,
                label,
                min,
                max,
                value,
            } => {
                out.u8(tag::SLIDER);
                out.str(id);
                out.str(label);
                out.f32(*min);
                out.f32(*max);
                out.f32(*value);
            }
            Self::Checkbox { id, label, checked } => {
                out.u8(tag::CHECKBOX);
                out.str(id);
                out.str(label);
                out.u8(u8::from(*checked));
            }
            Self::TextField { id, label, text } => {
                out.u8(tag::TEXT_FIELD);
                out.str(id);
                out.str(label);
                out.str(text);
            }
            Self::List {
                id,
                items,
                selected,
            } => {
                out.u8(tag::LIST);
                out.str(id);
                out.u32(selected.unwrap_or(NO_SELECTION));
                out.u32(items.len() as u32);
                items.iter().for_each(|item| out.str(item));
            }
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut input = In(bytes);
        let widget = match input.u8()? {
            tag::LABEL => Self::Label {
                text: input.str(MAX_TEXT)?,
            },
            tag::BUTTON => Self::Button {
                id: input.str(MAX_NAME)?,
                text: input.str(MAX_NAME)?,
            },
            tag::SLIDER => Self::Slider {
                id: input.str(MAX_NAME)?,
                label: input.str(MAX_NAME)?,
                min: input.f32()?,
                max: input.f32()?,
                value: input.f32()?,
            },
            tag::CHECKBOX => Self::Checkbox {
                id: input.str(MAX_NAME)?,
                label: input.str(MAX_NAME)?,
                checked: input.bool()?,
            },
            tag::TEXT_FIELD => Self::TextField {
                id: input.str(MAX_NAME)?,
                label: input.str(MAX_NAME)?,
                text: input.str(MAX_TEXT)?,
            },
            tag::LIST => {
                let id = input.str(MAX_NAME)?;
                let selected = Some(input.u32()?).filter(|s| *s != NO_SELECTION);
                let count = input.u32()? as usize;
                if count > MAX_ITEMS {
                    return Err(CodecError::TooLarge);
                }
                // ⚠ Grown per item: `count` is not yet shown to be there.
                let mut items = Vec::new();
                for _ in 0..count {
                    items.push(input.str(MAX_NAME)?);
                }
                Self::List {
                    id,
                    items,
                    selected,
                }
            }
            other => return Err(CodecError::UnknownTag(other)),
        };
        input.finish()?;
        Ok(widget)
    }
}

/// A set of points and lines as a guest hands it over.
pub fn encode_overlay(primitives: &[Primitive]) -> Vec<u8> {
    let mut out = Out::default();
    out.u32(primitives.len() as u32);
    for primitive in primitives {
        match primitive {
            Primitive::Point { at, size, colour } => {
                out.u8(tag::POINT);
                at.iter().for_each(|v| out.f32(*v));
                out.f32(*size);
                colour.iter().for_each(|v| out.f32(*v));
            }
            Primitive::Line { from, to } => {
                out.u8(tag::LINE);
                out.vertex(from);
                out.vertex(to);
            }
        }
    }
    out.0
}

/// The overlay a guest handed over, of at most [`MAX_PRIMITIVES`].
pub fn decode_overlay(bytes: &[u8]) -> Result<Vec<Primitive>, CodecError> {
    let mut input = In(bytes);
    let count = input.u32()? as usize;
    if count > MAX_PRIMITIVES {
        return Err(CodecError::TooLarge);
    }
    // ⚠ Grown per primitive: `count` is not yet shown to be there.
    let mut primitives = Vec::new();
    for _ in 0..count {
        primitives.push(match input.u8()? {
            tag::POINT => Primitive::Point {
                at: [input.f32()?, input.f32()?, input.f32()?],
                size: input.f32()?,
                colour: [input.f32()?, input.f32()?, input.f32()?, input.f32()?],
            },
            tag::LINE => Primitive::Line {
                from: input.vertex()?,
                to: input.vertex()?,
            },
            other => return Err(CodecError::UnknownTag(other)),
        });
    }
    input.finish()?;
    Ok(primitives)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_widget() -> Vec<Widget> {
        vec![
            Widget::Label { text: "hi".into() },
            Widget::Button {
                id: "go".into(),
                text: "Go".into(),
            },
            Widget::Slider {
                id: "w".into(),
                label: "Weight".into(),
                min: 0.0,
                max: 1.0,
                value: 0.25,
            },
            Widget::Checkbox {
                id: "c".into(),
                label: "Pin".into(),
                checked: true,
            },
            Widget::TextField {
                id: "t".into(),
                label: "Name".into(),
                text: "é".into(),
            },
            Widget::List {
                id: "l".into(),
                items: vec!["a".into(), "b".into()],
                selected: Some(1),
            },
            Widget::List {
                id: "l".into(),
                items: Vec::new(),
                selected: None,
            },
        ]
    }

    #[test]
    fn every_widget_survives_the_round_trip() {
        for widget in every_widget() {
            assert_eq!(Widget::decode(&widget.encode()).as_ref(), Ok(&widget));
        }
    }

    #[test]
    fn the_other_shapes_survive_the_round_trip() {
        let command = CommandSpec {
            id: "a".into(),
            label: "A".into(),
            shortcut: None,
        };
        assert_eq!(CommandSpec::decode(&command.encode()), Ok(command));
        let command = CommandSpec {
            shortcut: Some("Ctrl+K".into()),
            ..CommandSpec {
                id: "a".into(),
                label: "A".into(),
                shortcut: None,
            }
        };
        assert_eq!(CommandSpec::decode(&command.encode()), Ok(command));

        let panel = PanelSpec {
            id: "p".into(),
            title: "P".into(),
        };
        assert_eq!(PanelSpec::decode(&panel.encode()), Ok(panel));

        for kind in [
            SettingKind::Bool { default: true },
            SettingKind::Number {
                min: -1.0,
                max: 2.0,
                default: 0.5,
            },
            SettingKind::Text {
                default: "x".into(),
            },
        ] {
            let spec = SettingSpec {
                id: "s".into(),
                label: "S".into(),
                kind,
            };
            assert_eq!(SettingSpec::decode(&spec.encode()), Ok(spec));
        }
        for value in [
            SettingValue::Bool(false),
            SettingValue::Number(3.5),
            SettingValue::Text("t".into()),
        ] {
            assert_eq!(SettingValue::decode(&value.encode()), Ok(value));
        }

        let overlay = vec![
            Primitive::Point {
                at: [1.0, 2.0, 3.0],
                size: 0.5,
                colour: [1.0, 0.0, 0.0, 1.0],
            },
            Primitive::Line {
                from: Vertex {
                    at: [0.0; 3],
                    colour: [0.0, 0.0, 1.0, 1.0],
                },
                to: Vertex {
                    at: [1.0; 3],
                    colour: [1.0, 0.0, 0.0, 1.0],
                },
            },
        ];
        assert_eq!(decode_overlay(&encode_overlay(&overlay)), Ok(overlay));
    }

    /// Four billion primitives in four bytes is one call and an allocation for the side that trusted it.
    #[test]
    fn a_declared_count_allocates_nothing_before_the_bytes_are_there() {
        assert!(matches!(
            decode_overlay(&u32::MAX.to_le_bytes()),
            Err(CodecError::TooLarge)
        ));
        let mut bytes = (MAX_PRIMITIVES as u32).to_le_bytes().to_vec();
        bytes.push(tag::POINT);
        assert!(matches!(
            decode_overlay(&bytes),
            Err(CodecError::Truncated { .. })
        ));
        let mut list = vec![tag::LIST, 1, 0, 0, 0, b'l'];
        list.extend_from_slice(&NO_SELECTION.to_le_bytes());
        list.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Widget::decode(&list), Err(CodecError::TooLarge)));
    }

    #[test]
    fn bytes_that_are_not_this_encoding_are_refused() {
        assert!(matches!(
            Widget::decode(&[0x7F]),
            Err(CodecError::UnknownTag(0x7F))
        ));
        assert!(matches!(
            Widget::decode(&[tag::LABEL, 2, 0, 0, 0, 0xFF, 0xFE]),
            Err(CodecError::NotUtf8)
        ));
        let mut label = Widget::Label { text: "x".into() }.encode();
        label.push(0);
        assert_eq!(Widget::decode(&label), Err(CodecError::TrailingBytes(1)));
        let mut checkbox = Widget::Checkbox {
            id: "c".into(),
            label: "C".into(),
            checked: true,
        }
        .encode();
        *checkbox.last_mut().expect("a flag") = 2;
        assert_eq!(Widget::decode(&checkbox), Err(CodecError::NotABool(2)));
    }

    #[test]
    fn a_number_is_kept_to_its_range_and_another_kinds_value_is_not_kept() {
        let kind = SettingKind::Number {
            min: 0.0,
            max: 10.0,
            default: 1.0,
        };
        assert_eq!(
            kind.keep(SettingValue::Number(99.0)),
            Some(SettingValue::Number(10.0))
        );
        assert_eq!(kind.keep(SettingValue::Number(f64::NAN)), None);
        assert_eq!(kind.keep(SettingValue::Bool(true)), None);
        assert_eq!(kind.default_value(), SettingValue::Number(1.0));
    }
}
