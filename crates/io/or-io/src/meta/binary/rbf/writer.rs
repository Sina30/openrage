use crate::meta::binary::rbf::layout::{RbfLayout, RbfStructShape};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::{MetaInstance, Value};

const RBF_IDENT: u32 = 0x30464252;

const MAX_DESCRIPTORS: usize = 0xFD;

const CLOSE_MARKER: u8 = 0xFF;
const BYTES_MARKER: u8 = 0xFD;

const TYPE_STRUCT: u8 = 0x00;
const TYPE_UINT32: u8 = 0x10;
const TYPE_BOOL_TRUE: u8 = 0x20;
const TYPE_BOOL_FALSE: u8 = 0x30;
const TYPE_FLOAT: u8 = 0x40;
const TYPE_FLOAT3: u8 = 0x50;
const TYPE_STRING: u8 = 0x60;

const CONTENT_FIELD: &str = "Content";

pub fn write(layout: Option<&RbfLayout>, instance: &MetaInstance) -> Result<Vec<u8>> {
    let default_layout = RbfLayout::default();
    let layout = layout.unwrap_or(&default_layout);

    let mut w = RbfWriter {
        out: Vec::new(),
        descriptors: Vec::new(),
        layout,
        seen: std::collections::BTreeMap::new(),
    };
    w.out.extend_from_slice(&RBF_IDENT.to_le_bytes());
    if layout.wide_values {
        w.out.extend_from_slice(&super::reader::WIDE_MARKER);
    }
    w.write_struct(instance)?;
    Ok(w.out)
}

struct RbfWriter<'a> {
    out: Vec<u8>,
    descriptors: Vec<String>,
    layout: &'a RbfLayout,
    /// How many structs of each name are written so far.
    seen: std::collections::BTreeMap<String, usize>,
}

impl RbfWriter<'_> {
    fn emit_descriptor(&mut self, name: &str, data_type: u8) -> Result<()> {
        if let Some(idx) = self.descriptors.iter().position(|d| d == name) {
            self.out.push(u8::try_from(idx).map_err(|_| {
                MetaError::Other(format!("rbf: descriptor index {idx} exceeds one byte"))
            })?);
            self.out.push(data_type);
            return Ok(());
        }

        let idx = self.descriptors.len();
        if idx >= MAX_DESCRIPTORS {
            return Err(MetaError::Other(format!(
                "rbf: {} distinct tag names exceeds the {MAX_DESCRIPTORS} the one-byte \
                 descriptor index can address (0xFD and 0xFF are control markers)",
                idx + 1
            )));
        }
        self.out.push(idx as u8);
        self.out.push(data_type);

        let name_bytes = name.as_bytes();
        let len = i16::try_from(name_bytes.len()).map_err(|_| {
            MetaError::Other(format!("rbf: tag name '{name}' is longer than 32767 bytes"))
        })?;
        self.out.extend_from_slice(&len.to_le_bytes());
        self.out.extend_from_slice(name_bytes);
        self.descriptors.push(name.to_string());
        Ok(())
    }

    fn write_struct(&mut self, inst: &MetaInstance) -> Result<()> {
        self.emit_descriptor(&inst.type_name, TYPE_STRUCT)?;

        let shape = self.resolve_shape(inst);
        self.out.extend_from_slice(&shape.x1.to_le_bytes());
        self.out.extend_from_slice(&shape.x2.to_le_bytes());
        self.out
            .extend_from_slice(&shape.attribute_count.to_le_bytes());

        for (name, value) in &inst.fields {
            self.write_field(name, value)?;
        }

        self.out.push(CLOSE_MARKER);
        self.out.push(CLOSE_MARKER);
        Ok(())
    }

    fn resolve_shape(&mut self, inst: &MetaInstance) -> RbfStructShape {
        let mut shape = self.layout.shape_for(&inst.type_name);
        // A body whose structs of one name state different shapes writes each struct the shape
        // read at its place among that name's, a struct past them the tag's; one tag's shape for
        // all would lose the others. No shipped GTA V file states two shapes for a tag
        // (`rbf_shapes_are_consistent_per_tag`), so GTA V saves write as before.
        if self.layout.wide_values || !self.layout.conflicts.is_empty() {
            let k = self.seen.entry(inst.type_name.clone()).or_default();
            if let Some(own) =
                (self.layout.occurrences.get(&inst.type_name)).and_then(|s| s.get(*k))
            {
                shape = *own;
            }
            *k += 1;
        }
        let leaf_count = i16::try_from(inst.fields.len()).unwrap_or(i16::MAX);
        if shape.attribute_count > leaf_count {
            shape.attribute_count = leaf_count;
        }
        shape
    }

    fn write_field(&mut self, name: &str, value: &Value) -> Result<()> {
        match value {
            Value::Struct(inner) => self.write_struct(inner),
            Value::Pointer(Some(inner)) => self.write_struct(inner),
            Value::Pointer(None) | Value::Null => Ok(()),
            Value::Bool(b) => {
                let ty = if *b { TYPE_BOOL_TRUE } else { TYPE_BOOL_FALSE };
                self.emit_descriptor(name, ty)
            }
            Value::UInt(v) if self.layout.wide_values => {
                self.emit_descriptor(name, TYPE_UINT32)?;
                self.out.extend_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Value::Int(v) if self.layout.wide_values => {
                self.emit_descriptor(name, TYPE_UINT32)?;
                self.out.extend_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Value::Float(v) if self.layout.wide_values => {
                self.emit_descriptor(name, TYPE_FLOAT)?;
                self.out.extend_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Value::UInt(v) => {
                self.emit_descriptor(name, TYPE_UINT32)?;
                self.out
                    .extend_from_slice(&(u32::try_from(*v).unwrap_or(u32::MAX)).to_le_bytes());
                Ok(())
            }
            Value::Int(v) => {
                self.emit_descriptor(name, TYPE_UINT32)?;
                self.out.extend_from_slice(&(*v as u32).to_le_bytes());
                Ok(())
            }
            Value::Float(v) => {
                self.emit_descriptor(name, TYPE_FLOAT)?;
                self.out.extend_from_slice(&(*v as f32).to_le_bytes());
                Ok(())
            }
            Value::Vec3([x, y, z]) => {
                self.emit_descriptor(name, TYPE_FLOAT3)?;
                self.out.extend_from_slice(&x.to_le_bytes());
                self.out.extend_from_slice(&y.to_le_bytes());
                self.out.extend_from_slice(&z.to_le_bytes());
                Ok(())
            }
            Value::String(s) if name == CONTENT_FIELD => {
                self.write_content_bytes(s.as_bytes(), true)
            }
            Value::String(s) => {
                self.emit_descriptor(name, TYPE_STRING)?;
                let bytes = s.as_bytes();
                let len = i16::try_from(bytes.len()).map_err(|_| {
                    MetaError::Other(format!("rbf: string field '{name}' exceeds 32767 bytes"))
                })?;
                self.out.extend_from_slice(&len.to_le_bytes());
                self.out.extend_from_slice(bytes);
                Ok(())
            }
            Value::ByteArray(bytes) => self.write_content_bytes(bytes, false),
            Value::Array(items) if name == CONTENT_FIELD => {
                let mut payload = Vec::with_capacity(items.len() * 2);
                for item in items {
                    let v = match item {
                        Value::UInt(v) => *v as u16,
                        Value::Int(v) => *v as u16,
                        other => {
                            return Err(MetaError::Other(format!(
                                "rbf: short_array content holds {other:?}, expected an integer"
                            )))
                        }
                    };
                    payload.extend_from_slice(&v.to_le_bytes());
                }
                self.write_content_bytes(&payload, false)
            }
            Value::Array(items) => {
                for item in items {
                    self.write_field(name, item)?;
                }
                Ok(())
            }
            Value::Hash(h) => {
                self.emit_descriptor(name, TYPE_STRING)?;
                let text = h.to_xml_text();
                let bytes = text.as_bytes();
                let len = i16::try_from(bytes.len()).map_err(|_| {
                    MetaError::Other(format!("rbf: hash field '{name}' exceeds 32767 bytes"))
                })?;
                self.out.extend_from_slice(&len.to_le_bytes());
                self.out.extend_from_slice(bytes);
                Ok(())
            }
            other => Err(MetaError::Other(format!(
                "rbf: field '{name}' holds {} which RBF cannot represent",
                value_kind(other)
            ))),
        }
    }

    fn write_content_bytes(&mut self, bytes: &[u8], nul_terminate: bool) -> Result<()> {
        self.out.push(BYTES_MARKER);
        self.out.push(CLOSE_MARKER);
        let total = bytes.len() + usize::from(nul_terminate);
        let len = i32::try_from(total)
            .map_err(|_| MetaError::Other("rbf: content payload exceeds 2 GiB".into()))?;
        self.out.extend_from_slice(&len.to_le_bytes());
        self.out.extend_from_slice(bytes);
        if nul_terminate {
            self.out.push(0);
        }
        Ok(())
    }
}

fn value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Int(_) => "an int",
        Value::UInt(_) => "a uint",
        Value::Float(_) => "a float",
        Value::String(_) => "a string",
        Value::Hash(_) => "a hash",
        Value::Vec2(_) => "a Vec2",
        Value::Vec3(_) => "a Vec3",
        Value::Vec4(_) => "a Vec4",
        Value::Mat34(_) => "a Mat34",
        Value::Color(_) => "a colour",
        Value::Enum { .. } => "an enum",
        Value::Bitset { .. } => "a bitset",
        Value::Struct(_) => "a struct",
        Value::Pointer(_) => "a pointer",
        Value::Array(_) => "an array",
        Value::Map(_) => "a map",
        Value::ByteArray(_) => "a byte array",
        Value::Raw(_) => "an undecoded placeholder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::binary::rbf::reader;
    use crate::meta::default_library;
    use crate::meta::diagnostics::Diagnostics;

    fn roundtrip(inst: &MetaInstance, layout: &RbfLayout) -> MetaInstance {
        let bytes = write(Some(layout), inst).expect("write");
        let mut diags = Diagnostics::lenient();
        let (back, _) = reader::parse_full(default_library(), &bytes, &mut diags).expect("re-read");
        back
    }

    fn shape(attrs: i16) -> RbfStructShape {
        RbfStructShape {
            x1: 0,
            x2: 0,
            attribute_count: attrs,
        }
    }

    #[test]
    fn scalars_round_trip() {
        let mut root = MetaInstance::new("CRoot");
        root.set("flag", Value::Bool(true));
        root.set("off", Value::Bool(false));
        root.set("count", Value::UInt(42));
        root.set("ratio", Value::Float(0.5));
        root.set("where", Value::Vec3([1.0, 2.0, 3.0]));
        root.set("label", Value::String("hello".into()));

        let mut layout = RbfLayout::default();
        layout.record_shape("CRoot", shape(0));

        let back = roundtrip(&root, &layout);
        assert_eq!(back.type_name, "CRoot");
        assert_eq!(back.get("flag"), Some(&Value::Bool(true)));
        assert_eq!(back.get("off"), Some(&Value::Bool(false)));
        assert_eq!(back.get("count"), Some(&Value::UInt(42)));
        assert_eq!(back.get("ratio"), Some(&Value::Float(0.5)));
        assert_eq!(back.get("where"), Some(&Value::Vec3([1.0, 2.0, 3.0])));
        assert_eq!(back.get("label"), Some(&Value::String("hello".into())));
    }

    #[test]
    fn nested_structs_round_trip() {
        let mut child = MetaInstance::new("CChild");
        child.set("v", Value::UInt(7));
        let mut root = MetaInstance::new("CRoot");
        root.set("CChild", Value::Struct(Box::new(child)));

        let mut layout = RbfLayout::default();
        layout.record_shape("CRoot", shape(0));
        layout.record_shape("CChild", shape(0));

        let back = roundtrip(&root, &layout);
        let Some(Value::Struct(inner)) = back.get("CChild") else {
            panic!("child struct missing: {back:?}");
        };
        assert_eq!(inner.get("v"), Some(&Value::UInt(7)));
    }

    #[test]
    fn descriptor_table_is_reused_not_redefined() {
        let mut root = MetaInstance::new("CRoot");
        root.set("a", Value::UInt(1));
        let mut inst = MetaInstance::new("CRoot");
        inst.fields.push(("a".into(), Value::UInt(1)));
        inst.fields.push(("a".into(), Value::UInt(2)));
        inst.fields.push(("a".into(), Value::UInt(3)));

        let mut layout = RbfLayout::default();
        layout.record_shape("CRoot", shape(0));

        let one = write(Some(&layout), &root).expect("write one");
        let three = write(Some(&layout), &inst).expect("write three");
        assert_eq!(three.len(), one.len() + 12);
    }

    #[test]
    fn too_many_distinct_tags_is_an_error_not_a_corrupt_file() {
        let mut inst = MetaInstance::new("CRoot");
        for i in 0..MAX_DESCRIPTORS + 4 {
            inst.fields.push((format!("f{i}"), Value::UInt(i as u64)));
        }
        let err = write(None, &inst).expect_err("should refuse");
        assert!(
            err.to_string().contains("descriptor index"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unrepresentable_value_is_rejected() {
        let mut inst = MetaInstance::new("CRoot");
        inst.set("m", Value::Mat34([[0.0; 4]; 3]));
        let err = write(None, &inst).expect_err("should refuse");
        assert!(err.to_string().contains("cannot represent"), "{err}");
    }

    /// A body with GTA VI's marker: a struct of an 8-byte 0x10 of each sign, an 8-byte 0x40 and a
    /// 0x50 of three f32.
    fn wide_body(marker: [u8; 4]) -> Vec<u8> {
        let mut b = b"RBF0".to_vec();
        b.extend(marker);
        let name = |b: &mut Vec<u8>, index: u8, ty: u8, text: &str| {
            b.extend([index, ty]);
            b.extend((text.len() as i16).to_le_bytes());
            b.extend(text.as_bytes());
        };
        name(&mut b, 0, TYPE_STRUCT, "CRoot");
        b.extend([1, 0, 2, 0, 1, 0]);
        name(&mut b, 1, TYPE_UINT32, "below");
        b.extend((-5i64).to_le_bytes());
        name(&mut b, 2, TYPE_FLOAT, "ratio");
        b.extend(0.1f64.to_le_bytes());
        name(&mut b, 3, TYPE_UINT32, "count");
        b.extend(0xFFFF_FFF0u64.to_le_bytes());
        name(&mut b, 4, TYPE_FLOAT3, "where");
        [1.0f32, -2.0, 3.5]
            .iter()
            .for_each(|f| b.extend(f.to_le_bytes()));
        // Two structs of one name whose shapes differ.
        name(&mut b, 5, TYPE_STRUCT, "CItem");
        b.extend([0, 0, 0, 0, 0, 0, CLOSE_MARKER, CLOSE_MARKER]);
        b.extend([5, TYPE_STRUCT, 3, 0, 1, 0, 0, 0, CLOSE_MARKER, CLOSE_MARKER]);
        b.extend([CLOSE_MARKER, CLOSE_MARKER]);
        b
    }

    #[test]
    fn a_body_with_gta_vis_marker_reads_8_byte_values_and_writes_back_byte_identical() {
        let bytes = wide_body(super::super::reader::WIDE_MARKER);
        let mut diags = Diagnostics::lenient();
        let read = |b: &[u8], d: &mut Diagnostics| reader::parse_full(default_library(), b, d);
        let (root, layout) = read(&bytes, &mut diags).expect("reads");
        assert!(layout.wide_values);
        assert_eq!(root.get("below"), Some(&Value::Int(-5)));
        assert_eq!(root.get("ratio"), Some(&Value::Float(0.1)));
        assert_eq!(root.get("count"), Some(&Value::UInt(0xFFFF_FFF0)));
        assert_eq!(root.get("where"), Some(&Value::Vec3([1.0, -2.0, 3.5])));
        let written = write(Some(&layout), &root).expect("writes");
        assert_eq!(written, bytes);
        let (again, layout) = read(&written, &mut diags).expect("reads");
        assert_eq!(write(Some(&layout), &again).expect("writes"), written);

        let err = read(&wide_body([0xFB, 0xFF, 2, 0]), &mut diags)
            .expect_err("another version")
            .to_string();
        assert!(
            err.contains("where every measured one is FB FF 01 00"),
            "{err}"
        );
    }

    #[test]
    fn a_wide_payload_that_is_no_text_and_one_nul_writes_back_as_stored() {
        let mut bytes = wide_body(super::super::reader::WIDE_MARKER);
        // A payload with no NUL, and one with bytes after its NUL, in the root after CItem.
        let close = bytes.len() - 2;
        let mut payloads = Vec::new();
        for payload in [&b"no nul"[..], b"text\0pad\0"] {
            payloads.extend([BYTES_MARKER, CLOSE_MARKER]);
            payloads.extend((payload.len() as i32).to_le_bytes());
            payloads.extend(payload);
        }
        bytes.splice(close..close, payloads);
        let mut diags = Diagnostics::lenient();
        let (root, layout) =
            reader::parse_full(default_library(), &bytes, &mut diags).expect("reads");
        assert_eq!(write(Some(&layout), &root).expect("writes"), bytes);
    }

    #[test]
    fn a_body_without_the_marker_whose_structs_of_one_name_differ_writes_back_byte_identical() {
        let mut b = b"RBF0".to_vec();
        let name = |b: &mut Vec<u8>, index: u8, ty: u8, text: &str| {
            b.extend([index, ty]);
            b.extend((text.len() as i16).to_le_bytes());
            b.extend(text.as_bytes());
        };
        name(&mut b, 0, TYPE_STRUCT, "CRoot");
        b.extend([0, 0, 0, 0, 2, 0]);
        name(&mut b, 1, TYPE_STRUCT, "CItem");
        b.extend([0, 0, 0, 0, 1, 0]);
        name(&mut b, 2, TYPE_UINT32, "count");
        b.extend(7u32.to_le_bytes());
        b.extend([CLOSE_MARKER, CLOSE_MARKER]);
        // The second states two attributes where the first states one.
        b.extend([1, TYPE_STRUCT, 0, 0, 0, 0, 2, 0]);
        for value in [8u32, 9] {
            b.extend([2, TYPE_UINT32]);
            b.extend(value.to_le_bytes());
        }
        b.extend([CLOSE_MARKER, CLOSE_MARKER, CLOSE_MARKER, CLOSE_MARKER]);
        let mut diags = Diagnostics::lenient();
        let (root, layout) = reader::parse_full(default_library(), &b, &mut diags).expect("reads");
        assert!(!layout.wide_values);
        assert_eq!(layout.conflicts, vec!["CItem".to_string()]);
        let written = write(Some(&layout), &root).expect("writes");
        assert_eq!(written, b);
    }

    #[test]
    fn stale_attribute_count_cannot_outlive_its_fields() {
        let mut inst = MetaInstance::new("CRoot");
        inst.set("a", Value::UInt(1));
        inst.set("b", Value::UInt(2));
        let mut layout = RbfLayout::default();
        layout.record_shape("CRoot", shape(5));

        let back = roundtrip(&inst, &layout);
        assert_eq!(back.get("a"), Some(&Value::UInt(1)));
        assert_eq!(back.get("b"), Some(&Value::UInt(2)));
    }
}
