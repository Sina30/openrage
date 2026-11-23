//! The guest half of the OpenRage plugin ABI, version 1 (`docs/reference/Plugin_ABI.md`):
//! each host function as a call returning `Result<_, Status>`, and the values
//! `field_value` answers and `patch_field` takes.
//!
//! ⚠ Only a function the guest calls is imported, and importing one its grants
//! do not pay for fails instantiation: a guest that never patches must never
//! call [`patch_field`], and one that never replaces the file must never call
//! [`write_file`]. A plugin that calls nothing newer than version 0 declares `"abi": 0` in its
//! `plugin.json` and still loads; [`write_files`], [`pack`] and [`report_with_fix`] are version 1's,
//! and a plugin that calls one declares `"abi": 1`.

mod surface;

#[cfg(target_arch = "wasm32")]
pub use surface::{
    declare_setting, overlay_picking, register_command, register_panel, set_overlay, setting_value,
    ui,
};
pub use surface::{
    encode_command, encode_overlay, encode_panel, encode_setting, Primitive, SettingKind,
    SettingValue, Vertex, Widget,
};

/// The newest ABI version this library speaks; a host of an older version refuses a plugin that declares it.
pub const ABI_VERSION: u32 = 1;

/// A host call's answer other than success; the codes are the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    BufferTooSmall = 1,
    NotGranted = 2,
    NotWritable = 3,
    NoSuchType = 4,
    NoSuchField = 5,
    Refused = 6,
    BadArgument = 7,
    TooLarge = 8,
    Truncated = 9,
    NullPointer = 10,
}

impl Status {
    /// `Ok(())` for 0; a code this version does not define is an ABI mismatch and panics.
    pub fn check(code: i32) -> Result<(), Status> {
        Err(match code {
            0 => return Ok(()),
            1 => Self::BufferTooSmall,
            2 => Self::NotGranted,
            3 => Self::NotWritable,
            4 => Self::NoSuchType,
            5 => Self::NoSuchField,
            6 => Self::Refused,
            7 => Self::BadArgument,
            8 => Self::TooLarge,
            9 => Self::Truncated,
            10 => Self::NullPointer,
            _ => panic!("status {code} is not in plugin ABI version {ABI_VERSION}"),
        })
    }
}

/// How serious a finding is; the codes are the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Note = 0,
    Warning = 1,
    /// Stops the build.
    Error = 2,
}

/// A value `field_value` answers: a scalar, an enum or a fixed run of scalars.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i128),
    F32(f32),
    F64(f64),
    Bool(bool),
    Enum {
        name: String,
        value: i64,
    },
    Array {
        elements: Vec<Value>,
        capacity: Option<u64>,
    },
}

const INT: u8 = 0x01;
const F32: u8 = 0x02;
const F64: u8 = 0x03;
const BOOL: u8 = 0x04;
const ENUM: u8 = 0x05;
const ARRAY: u8 = 0x07;

impl Value {
    /// The ABI's encoding: a tag byte and a little-endian, unpadded body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Int(v) => {
                out.push(INT);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Self::F32(v) => {
                out.push(F32);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Self::F64(v) => {
                out.push(F64);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Self::Bool(v) => out.extend_from_slice(&[BOOL, u8::from(*v)]),
            Self::Enum { name, value } => {
                out.push(ENUM);
                out.extend_from_slice(&(name.len() as u32).to_le_bytes());
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Array { elements, capacity } => {
                out.push(ARRAY);
                out.extend_from_slice(&(elements.len() as u32).to_le_bytes());
                match capacity {
                    Some(capacity) => {
                        out.push(1);
                        out.extend_from_slice(&capacity.to_le_bytes());
                    }
                    None => out.push(0),
                }
                for element in elements {
                    element.encode_into(out);
                }
            }
        }
    }

    /// One whole value, or `None` for bytes that are not one of these shapes.
    pub fn decode(bytes: &[u8]) -> Option<Value> {
        let mut rest = bytes;
        let value = Self::decode_from(&mut rest)?;
        rest.is_empty().then_some(value)
    }

    fn decode_from(rest: &mut &[u8]) -> Option<Value> {
        let [tag] = take::<1>(rest)?;
        Some(match tag {
            INT => Self::Int(i128::from_le_bytes(take(rest)?)),
            F32 => Self::F32(f32::from_le_bytes(take(rest)?)),
            F64 => Self::F64(f64::from_le_bytes(take(rest)?)),
            BOOL => match take::<1>(rest)? {
                [0] => Self::Bool(false),
                [1] => Self::Bool(true),
                _ => return None,
            },
            ENUM => {
                let len = u32::from_le_bytes(take(rest)?) as usize;
                let name = rest.get(..len)?;
                let name = std::str::from_utf8(name).ok()?.to_owned();
                *rest = &rest[len..];
                Self::Enum {
                    name,
                    value: i64::from_le_bytes(take(rest)?),
                }
            }
            ARRAY => {
                let count = u32::from_le_bytes(take(rest)?);
                let capacity = match take::<1>(rest)? {
                    [0] => None,
                    [1] => Some(u64::from_le_bytes(take(rest)?)),
                    _ => return None,
                };
                // ⚠ Grown per element rather than reserved from `count`, which
                // the bytes have not yet shown to be there.
                let mut elements = Vec::new();
                for _ in 0..count {
                    elements.push(Self::decode_from(rest)?);
                }
                Self::Array { elements, capacity }
            }
            _ => return None,
        })
    }
}

fn take<const N: usize>(rest: &mut &[u8]) -> Option<[u8; N]> {
    let (head, tail) = rest.split_first_chunk::<N>()?;
    *rest = tail;
    Some(*head)
}

/// The encoding of files written together: `u32 n`, then for each its path (`u32 len`, UTF-8) and its bytes (`u32 len`, bytes).
pub fn encode_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = (files.len() as u32).to_le_bytes().to_vec();
    for (path, bytes) in files {
        out.extend_from_slice(&(path.len() as u32).to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

#[cfg(target_arch = "wasm32")]
mod sys {
    #[link(wasm_import_module = "openrage")]
    extern "C" {
        pub fn root_type(out: *mut u8, cap: usize, len: *mut u32) -> i32;
        pub fn field_count(ty: *const u8, ty_len: usize, out: *mut u32) -> i32;
        pub fn field_name(
            ty: *const u8,
            ty_len: usize,
            index: u32,
            out: *mut u8,
            cap: usize,
            len: *mut u32,
        ) -> i32;
        pub fn type_size(ty: *const u8, ty_len: usize, out: *mut u32) -> i32;
        pub fn field_value(
            ty: *const u8,
            ty_len: usize,
            at: u32,
            name: *const u8,
            name_len: usize,
            out: *mut u8,
            cap: usize,
            len: *mut u32,
        ) -> i32;
        pub fn follow(
            ty: *const u8,
            ty_len: usize,
            at: u32,
            name: *const u8,
            name_len: usize,
            out: *mut u32,
        ) -> i32;
        pub fn element_count(
            ty: *const u8,
            ty_len: usize,
            at: u32,
            name: *const u8,
            name_len: usize,
            out: *mut u32,
        ) -> i32;
        pub fn patch_field(
            ty: *const u8,
            ty_len: usize,
            at: u32,
            name: *const u8,
            name_len: usize,
            value: *const u8,
            value_len: usize,
        ) -> i32;
        pub fn file_size(out: *mut u32) -> i32;
        pub fn read_file(offset: u32, out: *mut u8, len: usize) -> i32;
        pub fn write_file(
            bytes: *const u8,
            len: usize,
            why: *mut u8,
            why_cap: usize,
            why_len: *mut u32,
        ) -> i32;
        pub fn plan_size(out: *mut u32) -> i32;
        pub fn read_plan(offset: u32, out: *mut u8, len: usize) -> i32;
        pub fn report(
            code: *const u8,
            code_len: usize,
            level: i32,
            subject: *const u8,
            subject_len: usize,
            message: *const u8,
            message_len: usize,
        ) -> i32;
        pub fn add_file(path: *const u8, path_len: usize, text: *const u8, text_len: usize) -> i32;
        pub fn write_files(
            set: *const u8,
            len: usize,
            why: *mut u8,
            why_cap: usize,
            why_len: *mut u32,
        ) -> i32;
        pub fn pack_size(out: *mut u32) -> i32;
        pub fn read_pack(offset: u32, out: *mut u8, len: usize) -> i32;
        pub fn report_with_fix(
            code: *const u8,
            code_len: usize,
            level: i32,
            subject: *const u8,
            subject_len: usize,
            message: *const u8,
            message_len: usize,
            label: *const u8,
            label_len: usize,
            files: *const u8,
            files_len: usize,
        ) -> i32;
    }
}

/// A `write_file` the host refused: its status and the sentence it would show the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub status: Status,
    pub why: String,
}

#[cfg(target_arch = "wasm32")]
pub use calls::*;

#[cfg(target_arch = "wasm32")]
mod calls {
    use super::{sys, Refusal, Status, Value};

    /// Calls `call(out, cap, len)` and, on [`Status::BufferTooSmall`], once more
    /// with the length the host reported.
    pub(crate) fn answer(
        mut call: impl FnMut(*mut u8, usize, *mut u32) -> i32,
    ) -> Result<Vec<u8>, Status> {
        let mut buffer = vec![0u8; 64];
        let mut len = 0u32;
        let code = call(buffer.as_mut_ptr(), buffer.len(), &mut len);
        if code == Status::BufferTooSmall as i32 {
            buffer.resize(len as usize, 0);
            Status::check(call(buffer.as_mut_ptr(), buffer.len(), &mut len))?;
        } else {
            Status::check(code)?;
        }
        buffer.truncate(len as usize);
        Ok(buffer)
    }

    fn text(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("the host answers names in UTF-8")
    }

    fn number(call: impl FnOnce(*mut u32) -> i32) -> Result<u32, Status> {
        let mut out = 0u32;
        Status::check(call(&mut out))?;
        Ok(out)
    }

    /// The name of the document's root type, whose instance is at offset 0.
    pub fn root_type() -> Result<String, Status> {
        answer(|out, cap, len| unsafe { sys::root_type(out, cap, len) }).map(text)
    }

    /// How many fields `ty` declares.
    pub fn field_count(ty: &str) -> Result<u32, Status> {
        number(|out| unsafe { sys::field_count(ty.as_ptr(), ty.len(), out) })
    }

    /// The name of `ty`'s field at `index`, in declaration order.
    pub fn field_name(ty: &str, index: u32) -> Result<String, Status> {
        answer(|out, cap, len| unsafe {
            sys::field_name(ty.as_ptr(), ty.len(), index, out, cap, len)
        })
        .map(text)
    }

    /// The size of `ty`, the stride of a run of them.
    pub fn type_size(ty: &str) -> Result<u32, Status> {
        number(|out| unsafe { sys::type_size(ty.as_ptr(), ty.len(), out) })
    }

    /// The value of `field` of the `ty` at offset `at`.
    pub fn field_value(ty: &str, at: u32, field: &str) -> Result<Value, Status> {
        let bytes = answer(|out, cap, len| unsafe {
            sys::field_value(
                ty.as_ptr(),
                ty.len(),
                at,
                field.as_ptr(),
                field.len(),
                out,
                cap,
                len,
            )
        })?;
        Ok(Value::decode(&bytes).expect("the host answers a value this ABI version encodes"))
    }

    /// The offset the pointer `field` of the `ty` at `at` lands at.
    pub fn follow(ty: &str, at: u32, field: &str) -> Result<u32, Status> {
        number(|out| unsafe {
            sys::follow(ty.as_ptr(), ty.len(), at, field.as_ptr(), field.len(), out)
        })
    }

    /// How many elements `field` of the `ty` at `at` holds.
    pub fn element_count(ty: &str, at: u32, field: &str) -> Result<u32, Status> {
        number(|out| unsafe {
            sys::element_count(ty.as_ptr(), ty.len(), at, field.as_ptr(), field.len(), out)
        })
    }

    /// Sets the scalar or enum `field` of the `ty` at `at`; costs the edit grant.
    pub fn patch_field(ty: &str, at: u32, field: &str, value: &Value) -> Result<(), Status> {
        let value = value.encode();
        Status::check(unsafe {
            sys::patch_field(
                ty.as_ptr(),
                ty.len(),
                at,
                field.as_ptr(),
                field.len(),
                value.as_ptr(),
                value.len(),
            )
        })
    }

    /// The length of the open file's bytes; costs the file-bytes read grant.
    pub fn file_size() -> Result<u32, Status> {
        number(|out| unsafe { sys::file_size(out) })
    }

    /// `len` bytes of the open file from `offset`, copied into this guest's memory.
    pub fn read_file(offset: u32, len: u32) -> Result<Vec<u8>, Status> {
        let mut bytes = vec![0u8; len as usize];
        Status::check(unsafe { sys::read_file(offset, bytes.as_mut_ptr(), bytes.len()) })?;
        Ok(bytes)
    }

    /// The whole open file.
    pub fn file_bytes() -> Result<Vec<u8>, Status> {
        read_file(0, file_size()?)
    }

    /// Replaces the open file's bytes; costs the file-bytes write grant, and a
    /// refusal carries the host's sentence, cut to 1 KiB.
    pub fn write_file(bytes: &[u8]) -> Result<(), Refusal> {
        let mut why = vec![0u8; 1024];
        let mut len = 0u32;
        let code = unsafe {
            sys::write_file(
                bytes.as_ptr(),
                bytes.len(),
                why.as_mut_ptr(),
                why.len(),
                &mut len,
            )
        };
        Status::check(code).map_err(|status| {
            why.truncate((len as usize).min(why.len()));
            Refusal {
                status,
                why: String::from_utf8_lossy(&why).into_owned(),
            }
        })
    }

    /// The plan of the build as the JSON document `openrage-cli wardrobe plan` prints; costs the plan-read grant.
    pub fn plan() -> Result<Vec<u8>, Status> {
        let len = number(|out| unsafe { sys::plan_size(out) })?;
        let mut bytes = vec![0u8; len as usize];
        Status::check(unsafe { sys::read_plan(0, bytes.as_mut_ptr(), bytes.len()) })?;
        Ok(bytes)
    }

    /// Reports a finding about the plan: `code` is letters, digits, `_`, `-` and `.`, `subject` what it
    /// is about; costs the findings grant. A refusal is the host's to show, and says nothing to the guest but its status.
    pub fn report(
        code: &str,
        level: super::Level,
        subject: Option<&str>,
        message: &str,
    ) -> Result<(), Status> {
        let subject = subject.unwrap_or("");
        Status::check(unsafe {
            sys::report(
                code.as_ptr(),
                code.len(),
                level as i32,
                subject.as_ptr(),
                subject.len(),
                message.as_ptr(),
                message.len(),
            )
        })
    }

    /// Adds a text file at `path`, inside the resource, to the plan; costs the add-files grant.
    pub fn add_file(path: &str, text: &str) -> Result<(), Status> {
        Status::check(unsafe {
            sys::add_file(path.as_ptr(), path.len(), text.as_ptr(), text.len())
        })
    }

    /// The clothing packs the build takes as the JSON document the host describes in `Plugin_ABI.md`; costs the pack-read grant. ABI 1.
    pub fn pack() -> Result<Vec<u8>, Status> {
        let len = number(|out| unsafe { sys::pack_size(out) })?;
        let mut bytes = vec![0u8; len as usize];
        Status::check(unsafe { sys::read_pack(0, bytes.as_mut_ptr(), bytes.len()) })?;
        Ok(bytes)
    }

    /// Writes `files`, each by its path in the project, together: all of them or none, as one step of Undo;
    /// costs the write-files grant, and a refusal carries the host's sentence. ABI 1.
    pub fn write_files(files: &[(&str, &[u8])]) -> Result<(), Refusal> {
        let set = super::encode_files(files);
        let mut why = vec![0u8; 1024];
        let mut len = 0u32;
        let code = unsafe {
            sys::write_files(
                set.as_ptr(),
                set.len(),
                why.as_mut_ptr(),
                why.len(),
                &mut len,
            )
        };
        Status::check(code).map_err(|status| {
            why.truncate((len as usize).min(why.len()));
            Refusal {
                status,
                why: String::from_utf8_lossy(&why).into_owned(),
            }
        })
    }

    /// Reports a finding with a suggested fix: `files` are written together if the person accepts it, and `label` says what that does;
    /// costs the suggest-fixes grant. ABI 1.
    pub fn report_with_fix(
        code: &str,
        level: super::Level,
        subject: Option<&str>,
        message: &str,
        label: &str,
        files: &[(&str, &[u8])],
    ) -> Result<(), Status> {
        let subject = subject.unwrap_or("");
        let set = super::encode_files(files);
        Status::check(unsafe {
            sys::report_with_fix(
                code.as_ptr(),
                code.len(),
                level as i32,
                subject.as_ptr(),
                subject.len(),
                message.as_ptr(),
                message.len(),
                label.as_ptr(),
                label.len(),
                set.as_ptr(),
                set.len(),
            )
        })
    }
}
