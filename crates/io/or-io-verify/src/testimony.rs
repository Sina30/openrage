//! The compiled readers' recorded testimony, and the check a reader runs
//! against it once they are gone.
//!
//! A record is line-oriented text, one block per resource file: every struct
//! instance a reader read, as `field=hex` at the offset it read it, and the
//! hash of what each writer produced. Nothing here knows how the record was
//! taken, so the check survives the compiled readers being deleted.
//!
//! ⚠ **A record is derived game content.** It belongs in `openrage-corpus`
//! beside the files it describes, never in this repository.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, BufRead, Write};

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::desc::{FieldRepr, Image, Pack, PackField, PackType, Plans, TypePlan, Value};
use sha1::{Digest, Sha1};

pub const MAGIC: &str = "#openrage-testimony";
pub const VERSION: u32 = 1;

/// Who located the bytes a recorded value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Witness {
    /// A compiled layout: independent of any pack.
    Compiled,
    /// A descriptor pack: a regression snapshot, which cannot catch a
    /// description that was already wrong the day it was recorded.
    Pack,
}

impl Witness {
    fn code(self) -> char {
        match self {
            Self::Compiled => 'c',
            Self::Pack => 'p',
        }
    }

    fn parse(code: &str) -> Option<Self> {
        match code {
            "c" => Some(Self::Compiled),
            "p" => Some(Self::Pack),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compiled => "compiled",
            Self::Pack => "pack snapshot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedInstance {
    pub type_name: String,
    /// Offset into the decompressed image: virtual pages, then physical.
    pub offset: usize,
    pub witness: Witness,
    pub fields: Vec<(String, Vec<u8>)>,
}

/// What a writer produced, fingerprinted twice: the file as written, and the
/// RSC7 header plus the decompressed payload, which does not depend on which
/// deflate implementation compressed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub file_sha1: String,
    pub payload_sha1: Option<String>,
    /// The decompressed pages alone. Differs from `payload_sha1` only in
    /// leaving out the header's page flags, which is what tells a writer that
    /// packed the same allocations into different pages from one that wrote
    /// different allocations.
    pub data_sha1: Option<String>,
    pub bytes: usize,
}

impl Fingerprint {
    pub fn of(bytes: &[u8]) -> Self {
        let opened = Rsc7::open(bytes).ok();
        let payload_sha1 = opened.as_ref().map(|opened| {
            let mut hasher = Sha1::new();
            hasher.update(opened.seal.header_bytes());
            hasher.update(&opened.data);
            hex(&hasher.finalize())
        });
        Self {
            file_sha1: sha1_hex(bytes),
            payload_sha1,
            data_sha1: opened.map(|o| sha1_hex(&o.data)),
            bytes: bytes.len(),
        }
    }

    /// Whether `other` is the same output. The payload decides when both
    /// carry one; the file bytes only when neither does.
    pub fn matches(&self, other: &Fingerprint) -> bool {
        match (&self.payload_sha1, &other.payload_sha1) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.file_sha1 == other.file_sha1,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteRecord {
    pub writer: String,
    pub outcome: Result<Fingerprint, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    /// `<origin>/<path>`, where the origin names an install or the corpus.
    pub key: String,
    pub ext: String,
    pub target: String,
    pub version: u32,
    pub bytes: usize,
    pub sha1: String,
    pub virtual_size: usize,
    pub read: Result<(), String>,
    pub instances: Vec<RecordedInstance>,
    pub writes: Vec<WriteRecord>,
}

impl FileRecord {
    pub fn field_count(&self) -> usize {
        self.instances.iter().map(|i| i.fields.len()).sum()
    }
}

/// A compiled layout as the recorder knew it, kept so a record says what the
/// reader believed as well as what it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedLayout {
    pub type_name: String,
    /// The compiled struct's Rust path, which two targets' layouts sharing one
    /// pack name do not share.
    pub rust_type: String,
    pub source: String,
    pub pack_only: bool,
    pub size: usize,
    pub fields: Vec<(String, usize, usize)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// `(key, value)` in the order written: revision, reader, packs, install.
    pub meta: Vec<(String, String)>,
    pub layouts: Vec<RecordedLayout>,
}

impl Header {
    pub fn meta<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> {
        self.meta
            .iter()
            .filter(move |(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    hex(&Sha1::digest(bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xF) as usize] as char);
    }
    out
}

/// ⚠ Decoded by nibble rather than `u8::from_str_radix`: a `.ynv` record is
/// 67 million fields, and this is read once a field on every pass over it.
fn unhex(text: &str) -> Option<Vec<u8>> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_chunks::<2>().0 {
        out.push(nibble(pair[0])? << 4 | nibble(pair[1])?);
    }
    Some(out)
}

// Keys and messages are single tokens or line tails; a space in a path or a
// newline in a message would split one.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    text.replace("%20", " ")
        .replace("%0A", "\n")
        .replace("%0D", "\r")
        .replace("%25", "%")
}

/// Streams a record out one file at a time.
pub struct RecordWriter<W: Write> {
    out: W,
}

impl<W: Write> RecordWriter<W> {
    pub fn new(mut out: W, header: &Header) -> io::Result<Self> {
        writeln!(out, "{MAGIC} {VERSION}")?;
        for (key, value) in &header.meta {
            writeln!(out, "meta {} {}", escape(key), escape(value))?;
        }
        for layout in &header.layouts {
            let fields: Vec<String> = layout
                .fields
                .iter()
                .map(|(name, offset, size)| format!("{name}:{offset:x}:{size:x}"))
                .collect();
            writeln!(
                out,
                "layout {} {} {} {} {:x} {}",
                layout.type_name,
                escape(&layout.rust_type),
                layout.source,
                u8::from(layout.pack_only),
                layout.size,
                fields.join(",")
            )?;
        }
        Ok(Self { out })
    }

    pub fn file(&mut self, record: &FileRecord) -> io::Result<()> {
        let out = &mut self.out;
        writeln!(out, "file {}", escape(&record.key))?;
        writeln!(
            out,
            "at {} {} {} {} {} {:x}",
            record.ext,
            record.target,
            record.version,
            record.bytes,
            record.sha1,
            record.virtual_size
        )?;
        match &record.read {
            Ok(()) => writeln!(out, "read ok")?,
            Err(why) => writeln!(out, "read err {}", escape(why))?,
        }
        for instance in &record.instances {
            write!(
                out,
                "i {} {:x} {}",
                instance.type_name,
                instance.offset,
                instance.witness.code()
            )?;
            for (name, bytes) in &instance.fields {
                write!(out, " {name}={}", hex(bytes))?;
            }
            writeln!(out)?;
        }
        for write in &record.writes {
            match &write.outcome {
                Ok(fp) => writeln!(
                    out,
                    "write {} ok {} {} {} {}",
                    write.writer,
                    fp.bytes,
                    fp.file_sha1,
                    fp.payload_sha1.as_deref().unwrap_or("-"),
                    fp.data_sha1.as_deref().unwrap_or("-")
                )?,
                Err(why) => writeln!(out, "write {} err {}", write.writer, escape(why))?,
            }
        }
        writeln!(out, "end")
    }

    pub fn into_inner(self) -> W {
        self.out
    }
}

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub why: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "testimony line {}: {}", self.line, self.why)
    }
}

impl std::error::Error for ParseError {}

/// Streams a record back one file at a time, so a check holds one file's
/// testimony at once however large the install was.
///
/// ⚠ One line buffer, reused: a `.ynv` record is 15 million lines a pass.
pub struct RecordReader<R: BufRead> {
    input: R,
    /// The current line, without its terminator.
    line: String,
    line_no: usize,
    /// `line` was read ahead and not yet taken.
    pending: bool,
    header: Header,
}

impl<R: BufRead> RecordReader<R> {
    pub fn new(input: R) -> Result<Self, ParseError> {
        let mut reader = Self {
            input,
            line: String::new(),
            line_no: 0,
            pending: false,
            header: Header::default(),
        };
        if !reader.advance()? {
            return Err(reader.error("empty record"));
        }
        let version = reader
            .line
            .strip_prefix(MAGIC)
            .map(str::trim)
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or_else(|| reader.error("not an OpenRage testimony record"))?;
        if version != VERSION {
            return Err(reader.error(&format!(
                "a version {version} record; this build reads version {VERSION}"
            )));
        }
        while reader.advance()? {
            if let Some(rest) = reader.line.strip_prefix("meta ") {
                let (key, value) = rest.split_once(' ').unwrap_or((rest, ""));
                let meta = (unescape(key), unescape(value));
                reader.header.meta.push(meta);
            } else if let Some(rest) = reader.line.strip_prefix("layout ") {
                let layout = parse_layout(rest).ok_or_else(|| reader.error("bad layout line"))?;
                reader.header.layouts.push(layout);
            } else {
                reader.pending = true;
                break;
            }
        }
        Ok(reader)
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    fn error(&self, why: &str) -> ParseError {
        ParseError {
            line: self.line_no,
            why: why.to_string(),
        }
    }

    /// Read the next line into `line`, terminator stripped as `BufRead::lines`
    /// strips it; `false` at the end of the record.
    fn advance(&mut self) -> Result<bool, ParseError> {
        if self.pending {
            self.pending = false;
            return Ok(true);
        }
        self.line.clear();
        match self.input.read_line(&mut self.line) {
            Ok(0) => Ok(false),
            Ok(_) => {
                if self.line.ends_with('\n') {
                    self.line.pop();
                    if self.line.ends_with('\r') {
                        self.line.pop();
                    }
                }
                self.line_no += 1;
                Ok(true)
            }
            Err(e) => Err(self.error(&e.to_string())),
        }
    }

    fn expect_line(&mut self) -> Result<(), ParseError> {
        if self.advance()? {
            Ok(())
        } else {
            Err(self.error("record ends inside a file block"))
        }
    }

    pub fn next_file(&mut self) -> Result<Option<FileRecord>, ParseError> {
        if !self.advance()? {
            return Ok(None);
        }
        let key = self
            .line
            .strip_prefix("file ")
            .map(unescape)
            .ok_or_else(|| self.error("expected `file`"))?;

        self.expect_line()?;
        let parts: Vec<&str> = self
            .line
            .strip_prefix("at ")
            .ok_or_else(|| self.error("expected `at`"))?
            .split(' ')
            .collect();
        let [ext, target, version, bytes, sha1, vsize] = parts[..] else {
            return Err(self.error("`at` needs six fields"));
        };
        let bad = |_| self.error("bad number in `at`");
        let mut record = FileRecord {
            key,
            ext: ext.to_string(),
            target: target.to_string(),
            version: version.parse().map_err(bad)?,
            bytes: bytes.parse().map_err(bad)?,
            sha1: sha1.to_string(),
            virtual_size: usize::from_str_radix(vsize, 16).map_err(bad)?,
            read: Ok(()),
            instances: Vec::new(),
            writes: Vec::new(),
        };

        loop {
            self.expect_line()?;
            let line = self.line.as_str();
            if line == "end" {
                return Ok(Some(record));
            } else if line == "read ok" {
                record.read = Ok(());
            } else if let Some(why) = line.strip_prefix("read err ") {
                record.read = Err(unescape(why));
            } else if let Some(rest) = line.strip_prefix("i ") {
                let instance =
                    parse_instance(rest).ok_or_else(|| self.error("bad instance line"))?;
                record.instances.push(instance);
            } else if let Some(rest) = line.strip_prefix("write ") {
                let write = parse_write(rest).ok_or_else(|| self.error("bad write line"))?;
                record.writes.push(write);
            } else {
                return Err(self.error(&format!("unexpected line `{line}`")));
            }
        }
    }
}

fn parse_layout(rest: &str) -> Option<RecordedLayout> {
    let mut parts = rest.splitn(6, ' ');
    let type_name = parts.next()?.to_string();
    let rust_type = unescape(parts.next()?);
    let source = parts.next()?.to_string();
    let pack_only = parts.next()? == "1";
    let size = usize::from_str_radix(parts.next()?, 16).ok()?;
    let fields = parts
        .next()
        .unwrap_or("")
        .split(',')
        .filter(|f| !f.is_empty())
        .map(|f| {
            let mut bits = f.split(':');
            Some((
                bits.next()?.to_string(),
                usize::from_str_radix(bits.next()?, 16).ok()?,
                usize::from_str_radix(bits.next()?, 16).ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RecordedLayout {
        type_name,
        rust_type,
        source,
        pack_only,
        size,
        fields,
    })
}

fn parse_instance(rest: &str) -> Option<RecordedInstance> {
    let mut parts = rest.split(' ');
    let type_name = parts.next()?.to_string();
    let offset = usize::from_str_radix(parts.next()?, 16).ok()?;
    let witness = Witness::parse(parts.next()?)?;
    let mut fields = Vec::with_capacity(rest.bytes().filter(|&b| b == b'=').count());
    for pair in parts {
        let (name, value) = pair.split_once('=')?;
        fields.push((name.to_string(), unhex(value)?));
    }
    Some(RecordedInstance {
        type_name,
        offset,
        witness,
        fields,
    })
}

fn parse_write(rest: &str) -> Option<WriteRecord> {
    let (writer, rest) = rest.split_once(' ')?;
    let outcome = if let Some(why) = rest.strip_prefix("err ") {
        Err(unescape(why))
    } else {
        let mut parts = rest.strip_prefix("ok ")?.split(' ');
        let bytes = parts.next()?.parse().ok()?;
        let file_sha1 = parts.next()?.to_string();
        let mut optional = || match parts.next()? {
            "-" => Some(None),
            hash => Some(Some(hash.to_string())),
        };
        let payload_sha1 = optional()?;
        let data_sha1 = optional()?;
        Ok(Fingerprint {
            file_sha1,
            payload_sha1,
            data_sha1,
            bytes,
        })
    };
    Some(WriteRecord {
        writer: writer.to_string(),
        outcome,
    })
}

/// What a reader under test answers when asked for one recorded field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Bytes(Vec<u8>),
    /// The reader does not know the type at all.
    NoType,
    /// It knows the type and not this field.
    NoField,
    /// It places the field somewhere the image does not reach.
    OutOfBounds,
    /// It places the field at a width other than the one recorded.
    Width(usize),
    /// The bytes at the offset a [`FieldMoved`] says the recording reader used.
    Corrected(Vec<u8>),
    /// The zero bytes between a record's end and its allocation's 16-byte alignment.
    Padding(Vec<u8>),
}

/// A reader the record is checked against: given where a struct instance
/// sits, where does it say one of its fields is, and what is there?
pub trait Reading {
    fn field(&self, type_name: &str, offset: usize, field: &str, len: usize) -> Answer;
}

/// The descriptor pack, read at its own offsets over a decompressed image.
pub struct PackReading<'a> {
    pack: &'a Pack,
    image: &'a [u8],
}

impl<'a> PackReading<'a> {
    pub fn new(pack: &'a Pack, image: &'a [u8]) -> Self {
        Self { pack, image }
    }
}

/// The field a pack means by a recorded name, by the rule `bind` resolves it
/// with: the exact name, then without its leading underscores.
pub fn pack_field<'p>(
    pack: &'p Pack,
    type_name: &str,
    field: &str,
) -> Result<&'p PackField, Answer> {
    let ty = pack.type_by_name(type_name).ok_or(Answer::NoType)?;
    ty.field_by_name(field)
        .or_else(|| ty.field_by_name(field.trim_start_matches('_')))
        .ok_or(Answer::NoField)
}

impl Reading for PackReading<'_> {
    /// Through the pack's type plan: the field by its hashed name, and the
    /// instance's bytes checked once against the type's size.
    fn field(&self, type_name: &str, offset: usize, field: &str, len: usize) -> Answer {
        let plans = self.pack.plans();
        let Ok(plan) = plans.get(type_name) else {
            return Answer::NoType;
        };
        let instance = plan.record(self.image, offset).map(|rec| rec.bytes());
        answer(plans, plan, instance, self.image, offset, field, len)
    }
}

/// One recorded field of an instance whose bytes are `instance`, where one was found.
fn answer(
    plans: &Plans,
    plan: &TypePlan,
    instance: Option<&[u8]>,
    image: &[u8],
    offset: usize,
    field: &str,
    len: usize,
) -> Answer {
    let Some(described) = plan
        .field(field)
        .or_else(|| plan.field(field.trim_start_matches('_')))
        .filter(|f| f.depth == 0)
    else {
        return Answer::NoField;
    };
    let width = match &described.repr {
        FieldRepr::Inline { type_name } => plans.get(type_name).ok().map(|t| t.size()),
        FieldRepr::InlineArray { type_name, count } => {
            plans.get(type_name).ok().map(|t| t.size() * count)
        }
        FieldRepr::PtrSlots { count, .. } => Some(or_io::desc::repr::POINTER_SIZE * count),
        other => other.inline_size(),
    };
    if let Some(width) = width {
        if width != len && !described.bit_packed {
            return Answer::Width(width);
        }
    }
    let within = described.offset..described.offset.saturating_add(len);
    let start = offset.saturating_add(described.offset);
    match instance
        .and_then(|bytes| bytes.get(within))
        .or_else(|| image.get(start..start.saturating_add(len)))
    {
        Some(bytes) => Answer::Bytes(bytes.to_vec()),
        None => Answer::OutOfBounds,
    }
}

pub const KEY_MAGIC: &str = "#openrage-testimony-key";
pub const KEY_VERSION: u32 = 1;
/// The key's file name, beside the testimony's stream directories.
pub const KEY_FILE: &str = "pack-witnessed.key";

/// Where the private description placed a `pack_only` layout's field, and how wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyRow {
    pub offset: usize,
    pub width: usize,
}

/// The offsets `pack_only` layouts were read at, frozen from the private
/// description, by `(target, recorded type, recorded field)`.
///
/// ⚠ **Taken once, from the version 12 pack, and never retaken:** the record
/// holds the bytes those offsets read, so a key taken from any later
/// description would hold the record to offsets it was not read at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Key {
    pub meta: Vec<(String, String)>,
    pub rows: BTreeMap<(String, String, String), KeyRow>,
}

impl Key {
    pub fn row(&self, target: &str, type_name: &str, field: &str) -> Option<KeyRow> {
        self.rows
            .get(&(target.to_string(), type_name.to_string(), field.to_string()))
            .copied()
    }

    pub fn rows_of(&self, target: &str) -> usize {
        self.rows.keys().filter(|(t, ..)| t == target).count()
    }

    /// Add a row for every field of every `pack_only` layout in `header` that
    /// `pack` describes, resolved as [`pack_field`] resolves a recorded name;
    /// returns each described field `pack` gives no width, which gets no row.
    pub fn take(&mut self, header: &Header, pack: &Pack) -> Vec<String> {
        let mut widthless = Vec::new();
        for layout in header.layouts.iter().filter(|l| l.pack_only) {
            for (field, ..) in &layout.fields {
                let Ok(described) = pack_field(pack, &layout.type_name, field) else {
                    continue;
                };
                let width = match &described.repr {
                    FieldRepr::Inline { type_name } => pack.type_by_name(type_name).map(|t| t.size),
                    FieldRepr::InlineArray { type_name, count } => {
                        pack.type_by_name(type_name).map(|t| t.size * count)
                    }
                    other => other.inline_size(),
                };
                let Some(width) = width else {
                    widthless.push(format!("{}.{field}", layout.type_name));
                    continue;
                };
                self.rows.insert(
                    (pack.target.clone(), layout.type_name.clone(), field.clone()),
                    KeyRow {
                        offset: described.offset,
                        width,
                    },
                );
            }
        }
        widthless
    }

    pub fn write<W: Write>(&self, mut out: W) -> io::Result<()> {
        writeln!(out, "{KEY_MAGIC} {KEY_VERSION}")?;
        for (key, value) in &self.meta {
            writeln!(out, "meta {} {}", escape(key), escape(value))?;
        }
        for ((target, type_name, field), row) in &self.rows {
            writeln!(
                out,
                "row {target} {type_name} {field} {:x} {:x}",
                row.offset, row.width
            )?;
        }
        Ok(())
    }

    pub fn read<R: BufRead>(input: R) -> Result<Self, ParseError> {
        let mut key = Self::default();
        let error = |line: usize, why: &str| ParseError {
            line,
            why: why.to_string(),
        };
        let mut lines = input.lines().enumerate();
        let first = match lines.next() {
            Some((_, Ok(line))) => line,
            Some((_, Err(e))) => return Err(error(1, &e.to_string())),
            None => return Err(error(0, "empty key")),
        };
        match first.strip_prefix(KEY_MAGIC).map(str::trim) {
            Some(v) if v == KEY_VERSION.to_string() => {}
            Some(v) => {
                return Err(error(
                    1,
                    &format!("a version {v} key; this build reads version {KEY_VERSION}"),
                ))
            }
            None => return Err(error(1, "not an OpenRage testimony key")),
        }
        for (i, line) in lines {
            let line = line.map_err(|e| error(i + 1, &e.to_string()))?;
            if let Some(rest) = line.strip_prefix("meta ") {
                let (k, v) = rest.split_once(' ').unwrap_or((rest, ""));
                key.meta.push((unescape(k), unescape(v)));
                continue;
            }
            let parts: Vec<&str> = line
                .strip_prefix("row ")
                .ok_or_else(|| error(i + 1, "expected `meta` or `row`"))?
                .split(' ')
                .collect();
            let [target, type_name, field, offset, width] = parts[..] else {
                return Err(error(i + 1, "`row` needs five fields"));
            };
            let number =
                |text| usize::from_str_radix(text, 16).map_err(|_| error(i + 1, "bad number"));
            key.rows.insert(
                (target.to_string(), type_name.to_string(), field.to_string()),
                KeyRow {
                    offset: number(offset)?,
                    width: number(width)?,
                },
            );
        }
        Ok(key)
    }
}

/// A field a private description moved after a record was taken through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMoved {
    /// The target whose description moved it.
    pub target: &'static str,
    /// The declared type the field's instance is filed as.
    pub inside: &'static str,
    pub now: usize,
    pub was: usize,
    /// The field's width, so no other field at `now` matches.
    pub len: usize,
}

/// A type's spans, `(start, end, scalar)`, sorted.
type Spans = std::rc::Rc<[(usize, usize, bool)]>;

/// A declared description's record tree, asked for each recorded field by
/// where it sits rather than by what the recorder called it.
///
/// The record names its types and fields in the compiled readers' words, which
/// the declarations need not share. So a recorded field is located by the
/// instance's offset and the field's offset and width from the record's own
/// layout table, and answered from whichever declared record the tree filed at
/// that offset describes exactly those bytes. No name crosses: the recorded
/// names are keys into the record's header and nothing else.
///
/// ⚠ **An instance no declared record starts at is unreached, never answered
/// from the image**, so a declaration that misreads self-consistently shows
/// as a count rather than as agreement.
pub struct DeclaredReading<'a> {
    pack: &'a Pack,
    /// Where the private description placed a `pack_only` layout's fields.
    key: &'a Key,
    moved: &'a [FieldMoved],
    image: &'a [u8],
    /// The recorded layout each instance was read with, by `(offset, type)`.
    layouts: foldhash::HashMap<(usize, &'a str), &'a RecordedLayout>,
    at: foldhash::HashMap<usize, Vec<(&'a str, &'a [u8])>>,
    /// Each type's [`spans`](Self::spans), sorted, by the length it was filed
    /// at, built on its first field.
    span_cache: std::cell::RefCell<foldhash::HashMap<(&'a str, usize), Spans>>,
    reached: Cell<usize>,
    unreached: Cell<usize>,
    unreached_types: std::cell::RefCell<BTreeMap<String, usize>>,
    unreached_at: std::cell::RefCell<Vec<(usize, String)>>,
}

impl<'a> DeclaredReading<'a> {
    /// `root` is the tree a declared read of `image` made, starting `root_at`
    /// bytes in, and `record` is what the recorder read of the same file.
    ///
    /// ⚠ A record's header can hold two layouts under one name — one per
    /// target — so each instance is matched to the layout holding every field
    /// it recorded at the width it recorded.
    ///
    /// ⚠ **A `pack_only` layout's offsets are not the file's.** Its values were
    /// read through a private description, and the layout table records a Rust
    /// struct that no longer mirrored the bytes, so its fields are located
    /// through `key`, frozen from that description, by the recorded names.
    pub fn new(
        header: &'a Header,
        record: &'a FileRecord,
        pack: &'a Pack,
        key: &'a Key,
        image: Image<'a>,
        root: &'a Value,
        root_at: usize,
    ) -> Self {
        let mut by_name: foldhash::HashMap<&str, Vec<&RecordedLayout>> = Default::default();
        for layout in &header.layouts {
            by_name
                .entry(layout.type_name.as_str())
                .or_default()
                .push(layout);
        }
        let mut layouts = foldhash::HashMap::default();
        for instance in &record.instances {
            // A pack-snapshot layout's widths are its Rust struct's, so it fits by name.
            let fits = |l: &&&RecordedLayout| {
                instance.fields.iter().all(|(name, bytes)| {
                    l.fields
                        .iter()
                        .any(|(f, _, width)| f == name && (l.pack_only || *width == bytes.len()))
                })
            };
            // ⚠ Two layouts under one name can both hold every recorded field,
            // when one target's is a superset of the other's; the one with no
            // field the instance did not record is the one it was read with.
            if let Some(layout) = by_name
                .get(instance.type_name.as_str())
                .and_then(|candidates| {
                    candidates
                        .iter()
                        .filter(fits)
                        .min_by_key(|l| l.fields.len())
                })
            {
                layouts.insert((instance.offset, instance.type_name.as_str()), *layout);
            }
        }
        let mut reading = Self {
            pack,
            key,
            moved: &[],
            image: image.bytes(),
            layouts,
            at: Default::default(),
            span_cache: Default::default(),
            reached: Cell::new(0),
            unreached: Cell::new(0),
            unreached_types: Default::default(),
            unreached_at: Default::default(),
        };
        reading.walk(image, root, Some(root_at));
        reading
    }

    /// Pack-witnessed fields the private description has since moved: inside a
    /// declared `inside`, a field it now places at `now` was read at `was`.
    ///
    /// ⚠ Answered as [`Answer::Corrected`] from the bytes at `was`, so a wrong
    /// correction is still a discrepancy rather than a pass.
    pub fn with_moved(mut self, moved: &'a [FieldMoved]) -> Self {
        self.moved = moved;
        self
    }

    /// Every `(offset, recorded type)` asked for where no declared record starts.
    pub fn unreached_at(&self) -> Vec<(usize, String)> {
        let mut out = self.unreached_at.borrow().clone();
        out.sort();
        out.dedup();
        out
    }

    /// Recorded fields a declared record answered for.
    pub fn reached(&self) -> usize {
        self.reached.get()
    }

    /// Recorded fields no declared record starts at the instance of.
    pub fn unreached(&self) -> usize {
        self.unreached.get()
    }

    /// [`unreached`](Self::unreached), by the recorded type.
    pub fn unreached_types(&self) -> BTreeMap<String, usize> {
        self.unreached_types.borrow().clone()
    }

    /// Every `(offset, declared type)` the tree filed, embedded records included.
    pub fn filed(&self) -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> = self
            .at
            .iter()
            .flat_map(|(at, filed)| filed.iter().map(move |(name, _)| (*at, name.to_string())))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn walk(&mut self, image: Image<'a>, value: &'a Value, at: Option<usize>) {
        match value {
            Value::Ptr {
                source,
                target: Some(target),
            } => self.walk(image, target, image.resolve(*source).ok()),
            Value::Struct(record) => {
                if let Some(at) = at {
                    // A trailing scalar run's elements are the record's own
                    // fields, so it is filed with the record.
                    let run_end = self
                        .pack
                        .type_by_name(record.type_name())
                        .and_then(|ty| ty.trailing())
                        .filter(|f| f.repr.trailing_element_size().is_some())
                        .and_then(|f| match record.child_at(f.offset) {
                            Some(Value::Bytes(run)) => Some(f.offset + run.len()),
                            _ => None,
                        });
                    let bytes = run_end
                        .and_then(|end| image.slice_at(at, end).ok())
                        .unwrap_or(record.bytes());
                    self.file(at, record.type_name(), bytes);
                }
                for (offset, child) in record.children() {
                    self.walk(image, child, at.map(|at| at + *offset as usize));
                }
            }
            Value::Records(run) => {
                if let Some(at) = at {
                    let name = run.type_name();
                    for (i, element) in run.iter().enumerate() {
                        self.file(at + i * run.stride(), name, element.bytes());
                    }
                }
            }
            Value::Array { elements, .. } => {
                let mut next = at;
                for element in elements {
                    self.walk(image, element, next);
                    next = next
                        .zip(element.record())
                        .map(|(at, r)| at + r.bytes().len());
                }
            }
            Value::Blob { parts, .. } => {
                for (_, part) in parts {
                    self.walk(image, part, None);
                }
            }
            _ => {}
        }
    }

    /// File a record and, at any depth, every record it embeds.
    fn file(&mut self, at: usize, type_name: &str, bytes: &'a [u8]) {
        let pack: &'a Pack = self.pack;
        let Some(ty) = pack.type_by_name(type_name) else {
            return;
        };
        self.at
            .entry(at)
            .or_default()
            .push((ty.name.as_str(), bytes));
        for field in &ty.fields {
            let (inner, count) = match &field.repr {
                FieldRepr::Inline { type_name } => (type_name, 1),
                FieldRepr::InlineArray { type_name, count } => (type_name, *count),
                _ => continue,
            };
            let Some(size) = pack.type_by_name(inner).map(|t| t.size) else {
                continue;
            };
            for i in 0..count {
                let start = field.offset + i * size;
                if let Some(inner_bytes) = bytes.get(start..start + size) {
                    self.file(at + start, inner, inner_bytes);
                }
            }
        }
    }

    /// The image's `[offset + within, + len)` when it is padding after what
    /// `filed` holds at `offset`: all zero, from the longest filed record's
    /// end to the next 16 bytes, a paged allocation's alignment, and inside
    /// no record filed anywhere.
    fn padding(
        &self,
        offset: usize,
        within: usize,
        len: usize,
        filed: &[(&str, &[u8])],
    ) -> Option<&'a [u8]> {
        let end = offset + filed.iter().map(|(_, bytes)| bytes.len()).max()?;
        let (start, stop) = (offset + within, offset + within + len);
        if start < end || stop > end.next_multiple_of(16) {
            return None;
        }
        let claimed = self
            .at
            .iter()
            .any(|(at, filed)| *at < stop && filed.iter().any(|(_, b)| at + b.len() > start));
        let bytes = self.image.get(start..stop)?;
        (!claimed && bytes.iter().all(|&b| b == 0)).then_some(bytes)
    }

    /// Whether `ty`'s own fields, with a trailing run's elements up to the
    /// `filed` length, tile `[offset, offset + len)` exactly: one field, or a
    /// run of fields the recorder read as one.
    ///
    /// ⚠ **Exactly, not enclosed, unless the encloser is a scalar.** An
    /// embedded struct holding a misplaced member reads the same bytes
    /// whatever the member's offset, so enclosure by one would agree with any
    /// layout; a word the recorder read as its four bytes is still that word.
    fn covers(&self, ty: &'a PackType, offset: usize, len: usize, filed: usize) -> bool {
        let end = offset + len;
        let spans = self.sorted_spans(ty, filed);
        if spans
            .iter()
            .any(|&(s, e, scalar)| scalar && s <= offset && end <= e)
        {
            return true;
        }
        // Tiled by any sequence of spans: an embedded struct whole, or its
        // own fields, at every depth.
        let mut reached = vec![false; len + 1];
        reached[0] = true;
        for at in offset..end {
            if !reached[at - offset] {
                continue;
            }
            let first = spans.partition_point(|&(s, ..)| s < at);
            for &(_, e, _) in spans[first..].iter().take_while(|&&(s, ..)| s == at) {
                if e <= end {
                    reached[e - offset] = true;
                }
            }
        }
        reached[len]
    }

    /// `ty`'s [`spans`](Self::spans) up to the `filed` length, sorted: asked
    /// once a recorded field, and the same for every field of a type filed at
    /// one length.
    ///
    /// ⚠ Keyed by the filed length as well as the type, because a trailing
    /// run's elements end there. A record with no trailing scalar run is
    /// filed at its size, so it takes one entry.
    fn sorted_spans(&self, ty: &'a PackType, filed: usize) -> Spans {
        let key = (ty.name.as_str(), filed);
        if let Some(spans) = self.span_cache.borrow().get(&key) {
            return spans.clone();
        }
        let mut spans = Vec::new();
        self.spans(ty, 0, 0, filed, &mut spans);
        spans.sort();
        let spans: Spans = spans.into();
        self.span_cache.borrow_mut().insert(key, spans.clone());
        spans
    }

    /// Every span `ty`'s fields occupy from `base`, embedded structs both
    /// whole and field by field; `true` marks a scalar.
    ///
    /// Declared padding and a byte run have no boundaries inside them, so
    /// they tile byte by byte; the writer writes padding as zero. Each element
    /// of a scalar array, and of a trailing scalar run up to `run_end`, is a
    /// scalar of its own.
    fn spans(
        &self,
        ty: &PackType,
        base: usize,
        depth: usize,
        run_end: usize,
        out: &mut Vec<(usize, usize, bool)>,
    ) {
        if depth > 16 {
            return;
        }
        for f in &ty.fields {
            let at = base + f.offset;
            match &f.repr {
                FieldRepr::Inline { type_name } => {
                    if let Some(inner) = self.pack.type_by_name(type_name) {
                        out.push((at, at + inner.size, false));
                        self.spans(inner, at, depth + 1, 0, out);
                    }
                }
                FieldRepr::InlineArray { type_name, count } => {
                    if let Some(inner) = self.pack.type_by_name(type_name) {
                        out.push((at, at + inner.size * count, false));
                        for i in 0..*count {
                            let el = at + i * inner.size;
                            out.push((el, el + inner.size, false));
                            self.spans(inner, el, depth + 1, 0, out);
                        }
                    }
                }
                FieldRepr::PtrSlots { count, .. } => {
                    out.push((at, at + or_io::desc::repr::POINTER_SIZE * count, false))
                }
                // A counted array's header is a pointer, a count, a capacity and padding.
                FieldRepr::AtArray(_) => out.extend([
                    (at, at + 16, false),
                    (at, at + 8, false),
                    (at + 8, at + 10, true),
                    (at + 10, at + 12, true),
                    (at + 12, at + 16, false),
                ]),
                FieldRepr::Padding { size }
                | FieldRepr::ScalarArray {
                    element: or_io::desc::Scalar::Int(or_io::desc::IntWidth::U8),
                    count: size,
                } => out.extend((at..at + size).map(|b| (b, b + 1, false))),
                FieldRepr::ScalarArray { element, count } => {
                    let w = element.size();
                    out.push((at, at + w * count, false));
                    out.extend((0..*count).map(|i| (at + i * w, at + (i + 1) * w, true)));
                }
                FieldRepr::Trailing { .. } => {
                    if let Some(w) = f.repr.trailing_element_size().filter(|w| *w > 0) {
                        out.extend(
                            (at..run_end)
                                .step_by(w)
                                .filter(|e| e + w <= run_end)
                                .map(|e| (e, e + w, true)),
                        );
                    }
                }
                repr @ (FieldRepr::Scalar(_) | FieldRepr::Enum { .. }) => {
                    if let Some(w) = repr.inline_size() {
                        out.push((at, at + w, true));
                    }
                }
                other => {
                    if let Some(w) = other.inline_size() {
                        out.push((at, at + w, false));
                    }
                }
            }
        }
    }
}

impl<'a> Reading for DeclaredReading<'a> {
    fn field(&self, type_name: &str, offset: usize, field: &str, len: usize) -> Answer {
        let Some(layout) = self.layouts.get(&(offset, type_name)) else {
            return Answer::NoType;
        };
        let within = if layout.pack_only {
            match self.key.row(&self.pack.target, type_name, field) {
                Some(row) if row.width != len => return Answer::Width(row.width),
                row => row.map(|row| row.offset),
            }
        } else {
            layout
                .fields
                .iter()
                .find(|(name, ..)| name == field || name.trim_start_matches('_') == field)
                .map(|&(_, within, _)| within)
        };
        let Some(within) = within else {
            return Answer::NoField;
        };
        let Some(filed) = self.at.get(&offset) else {
            self.unreached.set(self.unreached.get() + 1);
            *self
                .unreached_types
                .borrow_mut()
                .entry(type_name.to_string())
                .or_default() += 1;
            self.unreached_at
                .borrow_mut()
                .push((offset, type_name.to_string()));
            return Answer::NoType;
        };
        self.reached.set(self.reached.get() + 1);
        // A pack-snapshot record locates the field where the pack now puts it;
        // a compiled record where the compiled layout of the day did.
        {
            let moved = self.moved.iter().find(|m| {
                m.target == self.pack.target
                    && within == if layout.pack_only { m.now } else { m.was }
                    && m.len == len
                    && filed.iter().any(|(name, _)| *name == m.inside)
            });
            if let Some(m) = moved {
                let bytes = filed
                    .iter()
                    .find(|(name, _)| *name == m.inside)
                    .and_then(|(_, bytes)| bytes.get(m.was..m.was + len));
                return match bytes {
                    Some(bytes) => Answer::Corrected(bytes.to_vec()),
                    None => Answer::OutOfBounds,
                };
            }
        }
        let answer = filed.iter().find_map(|(name, bytes)| {
            let ty = self.pack.type_by_name(name)?;
            (within + len <= bytes.len() && self.covers(ty, within, len, bytes.len()))
                .then(|| bytes.get(within..within + len))
                .flatten()
        });
        match answer {
            Some(bytes) => Answer::Bytes(bytes.to_vec()),
            None => self
                .padding(offset, within, len, filed)
                .map_or(Answer::NoField, |bytes| Answer::Padding(bytes.to_vec())),
        }
    }
}

/// One recorded field a reader does not reproduce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discrepancy {
    pub key: String,
    pub type_name: String,
    pub offset: usize,
    pub field: String,
    pub witness: Witness,
    pub recorded: Vec<u8>,
    pub answer: Answer,
}

impl fmt::Display for Discrepancy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let answer = match &self.answer {
            Answer::Bytes(b) => format!("reads {}", hex(b)),
            Answer::NoType => "does not describe the type".to_string(),
            Answer::NoField => "does not describe the field".to_string(),
            Answer::OutOfBounds => "places it past the end of the image".to_string(),
            Answer::Width(w) => format!("gives it {w} bytes"),
            Answer::Corrected(b) => format!("reads {} where the correction says it was", hex(b)),
            Answer::Padding(b) => format!("reads {} in the padding after its record", hex(b)),
        };
        write!(
            f,
            "{}: {}.{} at {:#x} — {} recorded {}, the reader {answer}",
            self.key,
            self.type_name,
            self.field,
            self.offset,
            self.witness.label(),
            hex(&self.recorded),
        )
    }
}

/// What checking one file found.
///
/// ⚠ **Two kinds of field go unchecked, and neither is a pass.** A type the
/// reader does not describe, and a field of a described type it has no name
/// for — usually the pack spelling a field differently from the struct. Both
/// are counted per site, so a gap is a named list rather than a smaller
/// denominator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    pub fields: usize,
    pub agreed: usize,
    pub undescribed: usize,
    pub unanswered: usize,
    pub snapshot_fields: usize,
    /// Recorded fields a [`FieldMoved`] answered, each still equal to the file.
    pub corrected: usize,
    /// Recorded fields in the zero padding after a record, each still equal to the file.
    pub padding: usize,
    /// `Type` → recorded fields of it the reader does not describe.
    pub undescribed_types: BTreeMap<String, usize>,
    /// `Type.field` → recorded values of it the reader has no field for.
    pub unanswered_sites: BTreeMap<String, usize>,
    pub discrepancies: Vec<Discrepancy>,
}

impl Verdict {
    pub fn absorb(&mut self, other: Verdict) {
        self.fields += other.fields;
        self.agreed += other.agreed;
        self.undescribed += other.undescribed;
        self.unanswered += other.unanswered;
        self.snapshot_fields += other.snapshot_fields;
        self.corrected += other.corrected;
        self.padding += other.padding;
        for (k, n) in other.undescribed_types {
            *self.undescribed_types.entry(k).or_default() += n;
        }
        for (k, n) in other.unanswered_sites {
            *self.unanswered_sites.entry(k).or_default() += n;
        }
        self.discrepancies.extend(other.discrepancies);
    }
}

/// Check every recorded field of one file against a reader.
///
/// ⚠ A field the reader places at another width or past the image is a
/// discrepancy, not a gap: the record says the compiled reader found
/// something there.
pub fn check_file(record: &FileRecord, reading: &dyn Reading) -> Verdict {
    let mut verdict = Verdict::default();
    for instance in &record.instances {
        for (field, recorded) in &instance.fields {
            verdict.fields += 1;
            if instance.witness == Witness::Pack {
                verdict.snapshot_fields += 1;
            }
            match reading.field(&instance.type_name, instance.offset, field, recorded.len()) {
                Answer::Bytes(bytes) if &bytes == recorded => verdict.agreed += 1,
                Answer::Corrected(bytes) if &bytes == recorded => verdict.corrected += 1,
                Answer::Padding(bytes) if &bytes == recorded => verdict.padding += 1,
                Answer::NoType => {
                    verdict.undescribed += 1;
                    *verdict
                        .undescribed_types
                        .entry(instance.type_name.clone())
                        .or_default() += 1;
                }
                Answer::NoField => {
                    verdict.unanswered += 1;
                    *verdict
                        .unanswered_sites
                        .entry(format!("{}.{field}", instance.type_name))
                        .or_default() += 1;
                }
                answer => verdict.discrepancies.push(Discrepancy {
                    key: record.key.clone(),
                    type_name: instance.type_name.clone(),
                    offset: instance.offset,
                    field: field.clone(),
                    witness: instance.witness,
                    recorded: recorded.clone(),
                    answer,
                }),
            }
        }
    }
    verdict
}

/// Whether a value fits the kind the pack declares for it.
///
/// Needs no second reader: a float read where an enum sits, or an enum read
/// where a float sits, is loud on its own. Integers and runs carry no kind a
/// value can break, so they are not asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Bool,
    Enum,
    Float,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bool => "bool outside {0, 1}",
            Self::Enum => "enum outside its variants",
            Self::Float => "float not finite",
        }
    }
}

/// One site's census: how many recorded values the kind was asked of, and
/// how many broke it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiteCensus {
    pub values: usize,
    pub misfits: usize,
    pub example: Option<(String, usize, Vec<u8>)>,
}

/// Every described scalar's recorded values against its declared kind,
/// keyed by `(Type.field, kind)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    pub sites: BTreeMap<(String, Kind), SiteCensus>,
}

impl Census {
    pub fn values(&self) -> usize {
        self.sites.values().map(|s| s.values).sum()
    }

    pub fn misfits(&self) -> usize {
        self.sites.values().map(|s| s.misfits).sum()
    }

    pub fn absorb(&mut self, other: Census) {
        for (site, s) in other.sites {
            let into = self.sites.entry(site).or_default();
            into.values += s.values;
            into.misfits += s.misfits;
            if into.example.is_none() {
                into.example = s.example;
            }
        }
    }

    /// Take every recorded value of one file whose field the pack gives a
    /// kind.
    pub fn take(&mut self, record: &FileRecord, pack: &Pack) {
        for instance in &record.instances {
            for (field, bytes) in &instance.fields {
                let Ok(described) = pack_field(pack, &instance.type_name, field) else {
                    continue;
                };
                let Some((kind, fits)) = fits(pack, &described.repr, bytes) else {
                    continue;
                };
                let site = self
                    .sites
                    .entry((format!("{}.{field}", instance.type_name), kind))
                    .or_default();
                site.values += 1;
                if !fits {
                    site.misfits += 1;
                    site.example.get_or_insert_with(|| {
                        (record.key.clone(), instance.offset, bytes.clone())
                    });
                }
            }
        }
    }
}

fn little_endian(bytes: &[u8]) -> Option<u64> {
    (bytes.len() <= 8).then(|| {
        bytes
            .iter()
            .rev()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
    })
}

fn fits(pack: &Pack, repr: &FieldRepr, bytes: &[u8]) -> Option<(Kind, bool)> {
    use or_io::desc::repr::Scalar;
    match repr {
        FieldRepr::Scalar(Scalar::Bool) if bytes.len() == 1 => Some((Kind::Bool, bytes[0] <= 1)),
        FieldRepr::Scalar(Scalar::F32) if bytes.len() == 4 => {
            let v = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Some((Kind::Float, v.is_finite()))
        }
        FieldRepr::Scalar(Scalar::F64) if bytes.len() == 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(bytes);
            Some((Kind::Float, f64::from_le_bytes(b).is_finite()))
        }
        FieldRepr::Enum { type_name, width } if bytes.len() == width.size() => {
            let variants = &pack.enum_by_name(type_name)?.variants;
            let raw = little_endian(bytes)?;
            let bits = (width.size() * 8) as u32;
            let signed = if bits < 64 && raw >> (bits - 1) == 1 {
                (raw as i64) - (1i64 << bits)
            } else {
                raw as i64
            };
            Some((
                Kind::Enum,
                variants
                    .iter()
                    .any(|v| v.value == signed || v.value == raw as i64),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::desc::repr::{IntWidth, Scalar};
    use or_io::desc::PackType;

    fn scalar(name: &str, offset: usize, width: IntWidth) -> PackField {
        PackField {
            name: name.to_string(),
            offset,
            repr: FieldRepr::Scalar(Scalar::Int(width)),
            space: None,
            bits: Vec::new(),
            note: None,
        }
    }

    fn pack(b_offset: usize) -> Pack {
        let mut pack = Pack::new("synthetic", "gen8");
        pack.types.push(PackType {
            name: "Invented".to_string(),
            size: 8,
            fields: vec![
                scalar("a", 0, IntWidth::U32),
                scalar("b", b_offset, IntWidth::U16),
            ],
            selector: None,
        });
        pack
    }

    fn image() -> Vec<u8> {
        (0u8..64).collect()
    }

    fn record() -> FileRecord {
        FileRecord {
            key: "corpus/with space.ydr".to_string(),
            ext: "ydr".to_string(),
            target: "gen8".to_string(),
            version: 165,
            bytes: 64,
            sha1: sha1_hex(&image()),
            virtual_size: 64,
            read: Ok(()),
            instances: vec![
                RecordedInstance {
                    type_name: "Invented".to_string(),
                    offset: 0x10,
                    witness: Witness::Compiled,
                    fields: vec![
                        ("a".to_string(), vec![0x10, 0x11, 0x12, 0x13]),
                        ("b".to_string(), vec![0x14, 0x15]),
                    ],
                },
                RecordedInstance {
                    type_name: "Unpacked".to_string(),
                    offset: 0,
                    witness: Witness::Pack,
                    fields: vec![("z".to_string(), vec![0])],
                },
            ],
            writes: vec![
                WriteRecord {
                    writer: "compiled".to_string(),
                    outcome: Ok(Fingerprint::of(b"not a resource")),
                },
                WriteRecord {
                    writer: "generic".to_string(),
                    outcome: Err("refused: no pack\nsecond line".to_string()),
                },
            ],
        }
    }

    fn header() -> Header {
        Header {
            meta: vec![
                ("revision".to_string(), "abc123 clean".to_string()),
                ("install".to_string(), "D:/Games/Some Title".to_string()),
            ],
            layouts: vec![RecordedLayout {
                type_name: "Invented".to_string(),
                rust_type: "a::Invented__Raw".to_string(),
                source: "compiled".to_string(),
                pack_only: false,
                size: 8,
                fields: vec![("a".to_string(), 0, 4), ("b".to_string(), 4, 2)],
            }],
        }
    }

    fn round_trip(records: &[FileRecord]) -> (Header, Vec<FileRecord>) {
        let mut writer = RecordWriter::new(Vec::new(), &header()).unwrap();
        for r in records {
            writer.file(r).unwrap();
        }
        let bytes = writer.into_inner();
        let mut reader = RecordReader::new(io::Cursor::new(bytes)).unwrap();
        let mut back = Vec::new();
        while let Some(r) = reader.next_file().unwrap() {
            back.push(r);
        }
        (reader.header().clone(), back)
    }

    #[test]
    fn a_record_reads_back_exactly_as_written() {
        let (h, back) = round_trip(&[record(), record()]);
        assert_eq!(h, header());
        assert_eq!(back, vec![record(), record()]);
    }

    #[test]
    fn a_pack_that_agrees_reproduces_every_described_field() {
        let image = image();
        let pack = pack(4);
        let verdict = check_file(&record(), &PackReading::new(&pack, &image));
        assert_eq!(verdict.fields, 3);
        assert_eq!(verdict.agreed, 2);
        assert_eq!(verdict.undescribed, 1, "`Unpacked` is not in the pack");
        assert_eq!(verdict.snapshot_fields, 1);
        assert!(
            verdict.discrepancies.is_empty(),
            "{:?}",
            verdict.discrepancies
        );
    }

    #[test]
    fn a_moved_offset_is_named_with_its_file_type_and_field() {
        let image = image();
        let pack = pack(6);
        let verdict = check_file(&record(), &PackReading::new(&pack, &image));
        assert_eq!(verdict.discrepancies.len(), 1);
        let d = &verdict.discrepancies[0];
        assert_eq!(
            (d.key.as_str(), d.type_name.as_str(), d.field.as_str()),
            ("corpus/with space.ydr", "Invented", "b")
        );
        assert_eq!(d.answer, Answer::Bytes(vec![0x16, 0x17]));
        let said = d.to_string();
        assert!(said.contains("Invented.b at 0x10"), "{said}");
        assert!(said.contains("compiled recorded 1415"), "{said}");
    }

    #[test]
    fn a_field_the_pack_cannot_name_is_a_named_gap_and_a_resized_one_a_discrepancy() {
        let image = image();
        let mut forgot = pack(4);
        forgot.types[0].fields.pop();
        let v = check_file(&record(), &PackReading::new(&forgot, &image));
        assert!(v.discrepancies.is_empty());
        assert_eq!(v.unanswered, 1);
        assert_eq!(v.unanswered_sites.get("Invented.b"), Some(&1));
        assert_eq!(v.undescribed_types.get("Unpacked"), Some(&1));

        let mut resized = pack(4);
        resized.types[0].fields[1].repr = FieldRepr::Scalar(Scalar::Int(IntWidth::U32));
        let v = check_file(&record(), &PackReading::new(&resized, &image));
        assert_eq!(v.discrepancies[0].answer, Answer::Width(4));
    }

    #[test]
    fn a_leading_underscore_resolves_the_way_bind_resolves_it() {
        let pack = pack(4);
        assert_eq!(pack_field(&pack, "Invented", "__b").unwrap().name, "b");
        assert_eq!(pack_field(&pack, "Nope", "b").unwrap_err(), Answer::NoType);
    }

    #[test]
    fn a_value_that_breaks_its_declared_kind_is_counted_at_its_site() {
        use or_io::desc::{PackEnum, PackVariant};
        let mut pack = pack(4);
        pack.enums.push(PackEnum {
            name: "Mode".to_string(),
            width: IntWidth::U16,
            variants: vec![PackVariant {
                name: "Only".to_string(),
                value: 0x1514,
                note: None,
            }],
        });
        pack.types[0].fields[0].repr = FieldRepr::Scalar(Scalar::F32);
        pack.types[0].fields[1].repr = FieldRepr::Enum {
            type_name: "Mode".to_string(),
            width: IntWidth::U16,
        };
        let mut census = Census::default();
        census.take(&record(), &pack);
        assert_eq!(census.values(), 2);
        assert_eq!(census.misfits(), 0, "{:?}", census.sites);

        let mut wrong = record();
        wrong.instances[0].fields[0].1 = f32::NAN.to_le_bytes().to_vec();
        wrong.instances[0].fields[1].1 = vec![0x15, 0x14];
        let mut census = Census::default();
        census.take(&wrong, &pack);
        assert_eq!(census.misfits(), 2);
        assert_eq!(
            census.sites[&("Invented.a".to_string(), Kind::Float)].misfits,
            1
        );
        assert_eq!(
            census.sites[&("Invented.b".to_string(), Kind::Enum)].misfits,
            1
        );
    }

    #[test]
    fn a_scalar_array_element_and_a_trailing_run_field_are_each_answered() {
        use or_io::desc::repr::CountSource;
        let field = |name: &str, offset: usize, repr: FieldRepr| PackField {
            name: name.to_string(),
            offset,
            repr,
            space: None,
            bits: Vec::new(),
            note: None,
        };
        let mut pack = Pack::new("synthetic", "gen8");
        pack.types.push(PackType {
            name: "Vertex".to_string(),
            size: 6,
            fields: vec![field(
                "data",
                0,
                FieldRepr::ScalarArray {
                    element: Scalar::Int(IntWidth::U16),
                    count: 3,
                },
            )],
            selector: None,
        });
        pack.types.push(PackType {
            name: "PageMap".to_string(),
            size: 0x10,
            fields: vec![
                scalar("count", 8, IntWidth::U8),
                field(
                    "page_info",
                    0x10,
                    FieldRepr::Trailing {
                        type_name: None,
                        count: CountSource::Field("count".to_string()),
                        element_size: Some(8),
                    },
                ),
            ],
            selector: None,
        });
        let mut bytes = image();
        bytes[8] = 2;
        let image = Image::new(&bytes, bytes.len());
        let layout = |name: &str, size: usize, fields: &[(&str, usize, usize)]| RecordedLayout {
            type_name: name.to_string(),
            rust_type: format!("a::{name}"),
            source: "compiled".to_string(),
            pack_only: false,
            size,
            fields: fields
                .iter()
                .map(|&(f, at, width)| (f.to_string(), at, width))
                .collect(),
        };
        let header = Header {
            meta: Vec::new(),
            layouts: vec![
                layout("Vertex", 6, &[("x", 0, 2), ("y", 2, 2), ("z", 4, 2)]),
                layout(
                    "PageMap",
                    0x28,
                    &[
                        ("page_0", 0x10, 8),
                        ("page_1", 0x18, 8),
                        ("beyond", 0x20, 8),
                    ],
                ),
            ],
        };
        let instance =
            |name: &str, offset: usize, fields: &[(&str, usize, usize)]| RecordedInstance {
                type_name: name.to_string(),
                offset,
                witness: Witness::Compiled,
                fields: fields
                    .iter()
                    .map(|&(f, at, width)| {
                        (
                            f.to_string(),
                            bytes[offset + at..offset + at + width].to_vec(),
                        )
                    })
                    .collect(),
            };
        let mut record = record();
        record.instances = vec![
            instance("Vertex", 0x30, &[("x", 0, 2), ("y", 2, 2), ("z", 4, 2)]),
            instance(
                "PageMap",
                0,
                &[
                    ("page_0", 0x10, 8),
                    ("page_1", 0x18, 8),
                    ("beyond", 0x20, 8),
                ],
            ),
        ];
        let key = Key::default();

        let tree =
            or_io::desc::value::read(&pack, or_io::desc::TypeName("Vertex"), image, 0x30).unwrap();
        let vertex = DeclaredReading::new(&header, &record, &pack, &key, image, &tree, 0x30);
        assert_eq!(
            vertex.field("Vertex", 0x30, "y", 2),
            Answer::Bytes(bytes[0x32..0x34].to_vec())
        );

        let tree =
            or_io::desc::value::read(&pack, or_io::desc::TypeName("PageMap"), image, 0).unwrap();
        let pages = DeclaredReading::new(&header, &record, &pack, &key, image, &tree, 0);
        assert_eq!(
            pages.field("PageMap", 0, "page_1", 8),
            Answer::Bytes(bytes[0x18..0x20].to_vec())
        );
        assert_eq!(
            pages.field("PageMap", 0, "beyond", 8),
            Answer::NoField,
            "the run ends where its count says"
        );
        let page_map = pack.type_by_name("PageMap").unwrap();
        assert!(
            pages.covers(page_map, 0x20, 8, 0x28),
            "a longer run's spans are not the shorter one's, cached"
        );
    }

    #[test]
    fn zero_bytes_past_a_record_up_to_its_sixteen_byte_alignment_are_padding() {
        use or_io::desc::repr::CountSource;
        let mut pack = Pack::new("synthetic", "gen8");
        pack.types.push(PackType {
            name: "PageMap".to_string(),
            size: 0x10,
            fields: vec![
                scalar("count", 8, IntWidth::U8),
                PackField {
                    name: "page_info".to_string(),
                    offset: 0x10,
                    repr: FieldRepr::Trailing {
                        type_name: None,
                        count: CountSource::Field("count".to_string()),
                        element_size: Some(8),
                    },
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
            ],
            selector: None,
        });
        let header = Header {
            meta: Vec::new(),
            layouts: vec![RecordedLayout {
                type_name: "FlagsBlock".to_string(),
                rust_type: "a::FlagsBlock".to_string(),
                source: "compiled".to_string(),
                pack_only: false,
                size: 0x28,
                fields: vec![("pad".to_string(), 0x18, 8), ("over".to_string(), 0x1c, 8)],
            }],
        };
        let key = Key::default();
        let check = |bytes: &[u8], claimed: bool| {
            let image = Image::new(bytes, bytes.len());
            let tree = or_io::desc::value::read(&pack, or_io::desc::TypeName("PageMap"), image, 0)
                .unwrap();
            let mut record = record();
            record.instances = vec![RecordedInstance {
                type_name: "FlagsBlock".to_string(),
                offset: 0,
                witness: Witness::Compiled,
                fields: vec![
                    ("pad".to_string(), bytes[0x18..0x20].to_vec()),
                    ("over".to_string(), bytes[0x1c..0x24].to_vec()),
                ],
            }];
            let mut reading = DeclaredReading::new(&header, &record, &pack, &key, image, &tree, 0);
            if claimed {
                reading.file(0x18, "PageMap", &bytes[0x18..0x28]);
            }
            let v = check_file(&record, &reading);
            (v.padding, v.unanswered)
        };
        let mut bytes = vec![0u8; 0x30];
        bytes[8] = 1;
        assert_eq!(
            check(&bytes, false),
            (1, 1),
            "one page slot files the map at 0x18; `over` reaches past 0x20"
        );
        assert_eq!(
            check(&bytes, true),
            (0, 2),
            "a record filed there owns them"
        );
        bytes[0x1a] = 1;
        assert_eq!(check(&bytes, false), (0, 2), "padding is zero");
    }

    #[test]
    fn a_record_from_another_version_is_refused_rather_than_misread() {
        let err = RecordReader::new(io::Cursor::new(b"#openrage-testimony 99\n".to_vec()))
            .err()
            .expect("refused");
        assert!(err.to_string().contains("version 99"), "{err}");
    }

    #[test]
    fn a_key_holds_the_pack_offsets_of_its_pack_only_layouts_and_reads_back() {
        let mut header = header();
        let mut key = Key::default();
        key.take(&header, &pack(4));
        assert!(key.rows.is_empty(), "a compiled layout gets no row");

        header.layouts[0].pack_only = true;
        key.meta
            .push(("pack-gen8".to_string(), "a build; with spaces".to_string()));
        assert!(key.take(&header, &pack(6)).is_empty());
        assert_eq!(
            key.row("gen8", "Invented", "b"),
            Some(KeyRow {
                offset: 6,
                width: 2
            })
        );
        assert_eq!(key.rows_of("gen8"), 2);
        assert_eq!(key.rows_of("gen9"), 0);

        let mut text = Vec::new();
        key.write(&mut text).unwrap();
        assert_eq!(Key::read(io::Cursor::new(text)).unwrap(), key);
        let err = Key::read(io::Cursor::new(b"#openrage-testimony-key 7\n".to_vec())).unwrap_err();
        assert!(err.to_string().contains("version 7"), "{err}");
    }
}
