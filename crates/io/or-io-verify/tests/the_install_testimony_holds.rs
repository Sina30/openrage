//! The retail installs' recorded testimony, read through the declarations and
//! the frozen key as the corpus's is, each file taken again from the install
//! its record names.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, and each is
//! checked against the records whose headers list its executables. A record
//! whose file changed, moved or no longer opens fails; one the compiled reader
//! refused with nothing recorded has nothing to ask.

use std::collections::BTreeSet;
use std::mem::{offset_of, size_of};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::RpfArchive;
use or_io::decl::{Aligned, Raw};
use or_io::desc::{Image, TypeName};
use or_io::gta5::texture_dict::gen9;
use or_io::Generation;
use or_io_verify::gates::{self, Stream, Tally};
use or_io_verify::testimony::{check_file, sha1_hex, DeclaredReading, FieldMoved, Key};
use or_test_support::ledger::{self, SkipReason};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

const SUFFIX: &str = ".testimony.gz";

/// An Enhanced texture's view, as the format suites answer it.
const MOVED: &[FieldMoved] = &[FieldMoved {
    target: "gen9",
    inside: "GrcTextureG9",
    now: offset_of!(Raw<gen9::GrcTextureG9>, view),
    was: 0x50,
    len: size_of::<Raw<gen9::ShaderResourceViewG9>>(),
}];

/// Recorded files the declared read must refuse, each with why; their fields
/// are not asked, any other refusal fails, and a listed file that reads fails.
const REFUSED: &[(&str, &str)] = &[];

/// Every install stream under `dir`, at any depth, labelled by its path; the
/// corpus's own directories are the format suites'.
fn install_streams(dir: &Path, label: &str, out: &mut Vec<Stream>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let label = if label.is_empty() {
            name.clone()
        } else {
            format!("{label}/{name}")
        };
        if path.is_dir() {
            if !label.starts_with("corpus") {
                install_streams(&path, &label, out);
            }
            continue;
        }
        let target = name
            .strip_suffix(SUFFIX)
            .and_then(|s| s.split_once('.'))
            .and_then(|(_, t)| Generation::from_slug(t));
        if let (Some(target), true) = (target, label.contains('/')) {
            out.push(Stream {
                label,
                target,
                path,
            });
        }
    }
}

fn ext_of(stream: &Stream) -> &str {
    let file = stream.label.rsplit('/').next().unwrap_or_default();
    file.split('.').next().unwrap_or_default()
}

/// The executables a stream's header lists, by name.
fn recorded_executables(stream: &Stream) -> BTreeSet<String> {
    gates::open_stream(&stream.path)
        .header()
        .meta("executable")
        .map(|line| line.split(" bytes=").next().unwrap_or(line).to_string())
        .collect()
}

fn executables(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".exe"))
        .collect()
}

/// The archives the last record was read through, outermost first.
#[derive(Default)]
struct Chain(Vec<(String, Arc<RpfArchive>)>);

impl Chain {
    /// The file a record's key names below its origin: loose under `root`, or
    /// inside the archives each `.rpf` segment opens.
    fn read(&mut self, root: &Path, keys: &Arc<Gta5Keys>, rest: &str) -> Result<Vec<u8>, String> {
        let segments: Vec<&str> = rest.split('/').collect();
        let (mut containers, mut start) = (Vec::new(), 0);
        for (i, segment) in segments[..segments.len() - 1].iter().enumerate() {
            if segment.to_ascii_lowercase().ends_with(".rpf") {
                containers.push(segments[start..=i].join("/"));
                start = i + 1;
            }
        }
        let path = segments[start..].join("/");
        if containers.is_empty() {
            return std::fs::read(root.join(&path)).map_err(|e| format!("{path}: {e}"));
        }
        let keep = self
            .0
            .iter()
            .zip(&containers)
            .take_while(|(held, want)| &held.0 == *want)
            .count();
        self.0.truncate(keep);
        for want in &containers[keep..] {
            let archive = match self.0.last() {
                None => RpfArchive::open_with(
                    root.join(want),
                    Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(keys))),
                ),
                Some((_, parent)) => {
                    let i = parent
                        .find(want)
                        .ok_or_else(|| format!("no archive {want}"))?;
                    parent.open_nested(i)
                }
            }
            .map_err(|e| format!("{want}: {e}"))?;
            self.0.push((want.clone(), Arc::new(archive)));
        }
        let (_, archive) = self.0.last().expect("an archive was opened");
        archive.read(&path).map_err(|e| format!("{path}: {e}"))
    }
}

/// Every recorded field of one install stream, through its format's declarations.
fn check_stream(root: &Path, keys: &Arc<Gta5Keys>, key: &Key, stream: &Stream) -> Tally {
    let ext = ext_of(stream);
    let pack = gates::pack(ext, stream.target);
    let root_type = TypeName(or_io::decl::intern(
        &gates::format(ext)
            .root_name(stream.target)
            .expect("declared"),
    ));
    let mut reader = gates::open_stream(&stream.path);
    let header = reader.header().clone();
    let absent = Key::default();
    let key = if header.layouts.iter().any(|l| l.pack_only) {
        key
    } else {
        &absent
    };
    let mut chain = Chain::default();
    let mut tally = Tally::default();
    while let Some(record) = reader.next_file().expect("the stream parses") {
        let failed = |why: String| format!("{}: {why}", record.key);
        let rest = record.key.split_once('/').map_or("", |(_, rest)| rest);
        let bytes = match chain.read(root, keys, rest) {
            Ok(bytes) => bytes,
            Err(why) => {
                tally.differ.push(failed(why));
                continue;
            }
        };
        if sha1_hex(&bytes) != record.sha1 {
            tally
                .differ
                .push(failed("not the bytes the record was taken from".into()));
            continue;
        }
        tally.files += 1;
        if record.instances.is_empty() {
            continue;
        }
        let opened = match Rsc7::open(&bytes) {
            Ok(opened) => opened,
            Err(e) => {
                tally.differ.push(failed(e.to_string()));
                continue;
            }
        };
        let pages = Aligned::from_bytes(&opened.data);
        let image = Image::new(pages.bytes(), opened.virtual_size as usize);
        let refused = REFUSED.iter().find(|(k, _)| *k == record.key);
        let tree = match (
            or_io::desc::value::read(&pack, root_type, image, 0),
            refused,
        ) {
            (Ok(tree), None) => tree,
            (Ok(_), Some(_)) => {
                tally
                    .differ
                    .push(failed("REFUSED lists it, and it reads".into()));
                continue;
            }
            (Err(e), Some((_, why))) => {
                println!("set aside {}: {e}; {why}", record.key);
                continue;
            }
            (Err(e), None) => {
                tally.differ.push(failed(e.to_string()));
                continue;
            }
        };
        let reading =
            DeclaredReading::new(&header, &record, &pack, key, image, &tree, 0).with_moved(MOVED);
        let verdict = check_file(&record, &reading);
        tally.fields += verdict.fields;
        tally.agreed += verdict.agreed;
        tally.unreached += reading.unreached();
        tally.undescribed += verdict.undescribed;
        tally.corrected += verdict.corrected;
        tally.padding += verdict.padding;
        tally.unanswered += verdict.unanswered;
        for (site, n) in &verdict.unanswered_sites {
            *tally.unanswered_sites.entry(site.clone()).or_default() += n;
        }
        tally.differ.extend(
            verdict
                .discrepancies
                .iter()
                .take(5)
                .map(ToString::to_string),
        );
    }
    tally
}

fn testimony_root() -> Option<PathBuf> {
    Some(gates::corpus_root()?.parent()?.join("testimony"))
}

#[test]
fn every_install_record_holds_through_the_declarations() {
    let what = "the installs' testimony through the declarations";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    let Some(testimony) = testimony_root().filter(|t| t.is_dir()) else {
        ledger::record(
            SkipReason::Corpus,
            what,
            "no testimony beside a staged corpus",
        );
        return;
    };
    let mut streams = Vec::new();
    install_streams(&testimony, "", &mut streams);
    streams.sort_by(|a, b| a.label.cmp(&b.label));
    let key = gates::frozen_key().expect("the frozen key, which cannot be retaken");
    for root in roots {
        let installed = executables(&root);
        let (mine, left_out): (Vec<&Stream>, Vec<&Stream>) = streams
            .iter()
            .filter(|s| recorded_executables(s) == installed)
            .partition(|s| or_io::gta5::formats::format(ext_of(s)).is_some());
        assert!(
            !mine.is_empty(),
            "{}: no install record lists its executables {installed:?}",
            root.display()
        );
        for stream in &left_out {
            println!("left out {}: no declared format reads it", stream.label);
        }
        let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
        let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the executable"));
        let rows: Vec<(String, Tally)> = std::thread::scope(|scope| {
            let running: Vec<_> = mine
                .iter()
                .map(|stream| {
                    let (root, keys, key) = (&root, &keys, &key);
                    let label = stream.label.clone();
                    (
                        label,
                        scope.spawn(move || check_stream(root, keys, key, stream)),
                    )
                })
                .collect();
            running
                .into_iter()
                .map(|(label, handle)| (label, handle.join().expect("a stream's check")))
                .collect()
        });
        println!("\n{}", root.display());
        let (total, mut failures) = gates::judge(&rows);
        let sum = |f: fn(&Tally) -> usize| rows.iter().map(|(_, t)| f(t)).sum::<usize>();
        println!(
            "total: {} files, {total} fields, {} agreed, {} unreached, {} undescribed, \
             {} unanswered, {} corrected, {} padding, {} differ",
            sum(|t| t.files),
            sum(|t| t.agreed),
            sum(|t| t.unreached),
            sum(|t| t.undescribed),
            sum(|t| t.unanswered),
            sum(|t| t.corrected),
            sum(|t| t.padding),
            sum(|t| t.differ.len()),
        );
        assert!(total > 0, "{}: no recorded field was asked", root.display());
        failures.truncate(30);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
