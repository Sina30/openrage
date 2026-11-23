//! The compiled readers' recorded testimony as a whole, read through the
//! built-in descriptions and the frozen key.
//!
//! `openrage-cli testimony record` ran every compiled reader over the corpus
//! and wrote down every field of every struct instance it read, at the offset
//! it read it. Each declared format's suite holds its own streams field by
//! field (`declared_gates!`); this holds what no one format's suite sees: that
//! every recorded file is still the file it was recorded from and read by a
//! declared description, that every described scalar fits its declared kind,
//! and that a reader misreading one field is named in exactly that file.
//!
//! ⚠ **The record is derived game content and lives in `openrage-corpus`**,
//! beside the corpus it describes — `testimony/corpus/`, or wherever
//! `OPENRAGE_TESTIMONY` points. Without it every test here records a skip.

use std::collections::BTreeMap;
use std::path::PathBuf;

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::desc::Pack;
use or_io::Generation;
use or_io_verify::gates::{corpus_root, format, frozen_key, open_stream, Opened};
use or_io_verify::testimony::{
    check_file, sha1_hex, Answer, Census, DeclaredReading, Key, Reading, Witness,
};
use or_test_support::ledger::{self, SkipReason};

const TESTIMONY_ENV: &str = "OPENRAGE_TESTIMONY";
const SUFFIX: &str = ".testimony.gz";

struct Stream {
    label: String,
    ext: String,
    target: Generation,
    path: PathBuf,
}

struct Staged {
    corpus: PathBuf,
    streams: Vec<Stream>,
    key: Key,
}

impl Staged {
    fn bytes(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.corpus.join(key.strip_prefix("corpus/")?)).ok()
    }
}

/// Every stream of every corpus record: `testimony/corpus` from the recorder
/// at the tip, and any `testimony/corpus-<revision>` an older revision took.
fn staged(what: &str) -> Option<Staged> {
    let Some(corpus) = corpus_root() else {
        ledger::record(SkipReason::Corpus, what, "no corpus is staged");
        return None;
    };
    let root = corpus.parent()?.join("testimony");
    let records: Vec<PathBuf> = match std::env::var_os(TESTIMONY_ENV).filter(|v| !v.is_empty()) {
        Some(dir) => vec![PathBuf::from(dir)],
        None => std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("corpus"))
            })
            .collect(),
    };
    let mut streams = Vec::new();
    for dir in &records {
        let record = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((ext, target)) = name.strip_suffix(SUFFIX).and_then(|s| s.split_once('.'))
            else {
                continue;
            };
            let target = Generation::from_slug(target)
                .unwrap_or_else(|| panic!("{}: names no target", entry.path().display()));
            streams.push(Stream {
                label: format!("{record}/{ext}.{}", target.slug()),
                ext: ext.to_string(),
                target,
                path: entry.path(),
            });
        }
    }
    streams.sort_by(|a, b| a.label.cmp(&b.label));
    if streams.is_empty() {
        ledger::record(
            SkipReason::Corpus,
            what,
            format_args!(
                "no recorded testimony under {}; set {TESTIMONY_ENV} to a record",
                root.display()
            ),
        );
        return None;
    }
    Some(Staged {
        corpus,
        streams,
        key: frozen_key().unwrap_or_else(|| {
            panic!("no frozen key beside the testimony, and it cannot be retaken without the version 12 pack")
        }),
    })
}

/// The built-in description a stream is read through.
fn described(stream: &Stream) -> Option<std::sync::Arc<Pack>> {
    or_io::gta5::formats::format(&stream.ext)?
        .description(stream.target)
        .ok()
}

#[test]
fn every_recorded_file_is_still_read_through_a_declared_description() {
    let Some(staged) = staged("the recorded testimony's files and descriptions") else {
        return;
    };
    let mut failures = Vec::new();
    let mut files = 0usize;
    for stream in &staged.streams {
        if described(stream).is_none() {
            failures.push(format!(
                "{}: no declared format describes .{} for {}",
                stream.label,
                stream.ext,
                stream.target.slug()
            ));
            continue;
        }
        let mut reader = open_stream(&stream.path);
        while let Some(record) = reader.next_file().expect("the stream parses") {
            files += 1;
            let why = match staged.bytes(&record.key) {
                None => "is no longer in the corpus",
                Some(b) if sha1_hex(&b) != record.sha1 => "changed since it was recorded",
                Some(b) if Rsc7::open(&b).is_err() => "is no longer an RSC7 resource",
                Some(_) => continue,
            };
            failures.push(format!(
                "{}: {} {why}; a record cannot be retaken once the compiled readers are gone",
                stream.label, record.key
            ));
        }
    }
    println!(
        "{files} recorded file(s) in {} stream(s)",
        staged.streams.len()
    );
    assert!(files > 0, "the testimony recorded no file at all");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Sites whose recorded values break their declared kind for a reason that
/// is not a misreading, each with the reason. A misfit anywhere else fails.
const RULED_MISFITS: &[(&str, &str)] = &[
    (
        "GrcTexture.format",
        "a Legacy texture's format is a D3D FourCC (DXT1, DXT5, ATI2, ...), which the enum table does not list",
    ),
    (
        "PhPolygon.area",
        "a polygon slot is a union, and for a sphere, capsule, box or cylinder primitive the word is not an area",
    ),
    (
        "PtxEffectRule._culling_offset_w",
        "the w lane of a vector the engine leaves as a signalling NaN",
    ),
    (
        "PtxEffectRule._rand_offset_w",
        "the w lane of a vector the engine leaves as a signalling NaN",
    ),
    ("PtxKeyframeEntry.time_y", "+inf marks a keyframe's open end"),
    (
        "Vec3V._w",
        "the padding lane of a three-vector, a NaN sentinel where it is not zero",
    ),
    (
        "Vec4V.w",
        "a vector slot holding a NaN sentinel or packed integers rather than a float",
    ),
    (
        "Vec4V.x",
        "a vector slot holding a NaN sentinel or packed integers rather than a float",
    ),
    (
        "Vec4V.y",
        "a vector slot holding a NaN sentinel or packed integers rather than a float",
    ),
    (
        "Vec4V.z",
        "a vector slot holding a NaN sentinel or packed integers rather than a float",
    ),
];

#[test]
fn every_described_scalar_fits_its_declared_kind() {
    let Some(staged) = staged("the census of recorded scalars against their declared kinds") else {
        return;
    };
    let mut total = Census::default();
    for stream in &staged.streams {
        let Some(pack) = described(stream) else {
            continue;
        };
        let mut census = Census::default();
        let mut reader = open_stream(&stream.path);
        while let Some(record) = reader.next_file().expect("the stream parses") {
            census.take(&record, &pack);
        }
        println!(
            "{}: {} value(s) asked, {} misfit(s)",
            stream.label,
            census.values(),
            census.misfits()
        );
        total.absorb(census);
    }
    let mut unruled = Vec::new();
    for ((site, kind), s) in total.sites.iter().filter(|(_, s)| s.misfits > 0) {
        let (key, at, bytes) = s.example.as_ref().expect("a misfit keeps its example");
        let line = format!(
            "{site}: {} of {} {} — first {key} at {at:#x}: {}",
            s.misfits,
            s.values,
            kind.label(),
            or_io_verify::testimony::hex(bytes)
        );
        match RULED_MISFITS.iter().find(|(ruled, _)| ruled == site) {
            Some((_, why)) => println!("  ruled   {line}\n          because {why}"),
            None => unruled.push(line),
        }
    }
    println!(
        "{} site(s), {} value(s) asked, {} misfit(s)",
        total.sites.len(),
        total.values(),
        total.misfits()
    );
    assert!(
        total.values() > 0,
        "no recorded value had a declared kind to fit"
    );
    assert!(
        unruled.is_empty(),
        "recorded values break their declared kind at a site nothing rules on — a field \
         read at the wrong offset usually lands here first:\n{}",
        unruled.join("\n")
    );
}

/// A reader that is right about everything but one field of one instance.
struct MisreadsOne<'a> {
    inner: &'a dyn Reading,
    type_name: &'a str,
    field: &'a str,
    offset: usize,
}

impl Reading for MisreadsOne<'_> {
    fn field(&self, type_name: &str, offset: usize, field: &str, len: usize) -> Answer {
        let answer = self.inner.field(type_name, offset, field, len);
        match answer {
            Answer::Bytes(mut b)
                if type_name == self.type_name && field == self.field && offset == self.offset =>
            {
                b[0] ^= 0x01;
                Answer::Bytes(b)
            }
            other => other,
        }
    }
}

#[test]
fn a_reader_that_misreads_one_field_is_named_in_that_file() {
    let Some(staged) = staged("a misreading reader against the testimony") else {
        return;
    };
    for stream in &staged.streams {
        let Some(pack) = described(stream) else {
            continue;
        };
        let root = or_io::decl::intern(
            &format(&stream.ext)
                .root_name(stream.target)
                .expect("declared"),
        );
        let mut reader = open_stream(&stream.path);
        let header = reader.header().clone();
        while let Some(record) = reader.next_file().expect("the stream parses") {
            let Some(bytes) = staged
                .bytes(&record.key)
                .filter(|b| sha1_hex(b) == record.sha1)
            else {
                continue;
            };
            let opened = Opened::of(&bytes);
            let image = opened.image();
            let tree = or_io::desc::value::read(&pack, or_io::desc::TypeName(root), image, 0)
                .unwrap_or_else(|e| panic!("{}: {e}", record.key));
            let honest =
                DeclaredReading::new(&header, &record, &pack, &staged.key, image, &tree, 0);
            if !check_file(&record, &honest).discrepancies.is_empty() {
                continue;
            }
            let chosen = record.instances.iter().find_map(|i| {
                (i.witness == Witness::Compiled)
                    .then(|| {
                        i.fields.iter().find(|(f, b)| {
                            !b.is_empty()
                                && matches!(
                                    honest.field(&i.type_name, i.offset, f, b.len()),
                                    Answer::Bytes(ref got) if got == b
                                )
                        })
                    })
                    .flatten()
                    .map(|(f, _)| (i.type_name.clone(), f.clone(), i.offset))
            });
            let Some((type_name, field, offset)) = chosen else {
                continue;
            };
            let wrong = MisreadsOne {
                inner: &honest,
                type_name: &type_name,
                field: &field,
                offset,
            };
            let got = check_file(&record, &wrong).discrepancies;
            let named: BTreeMap<_, _> = got
                .iter()
                .map(|d| {
                    (
                        (d.key.as_str(), d.type_name.as_str(), d.field.as_str()),
                        d.offset,
                    )
                })
                .collect();
            assert_eq!(
                named.into_iter().collect::<Vec<_>>(),
                vec![(
                    (record.key.as_str(), type_name.as_str(), field.as_str()),
                    offset
                )],
                "exactly the misread field, in exactly this file"
            );
            println!("{}", got[0]);
            return;
        }
    }
    panic!("no stream held a compiled field a declaration answers, so nothing was misread");
}
