//! `openrage-cli search --verify`: parse every file in an install and report
//! what refused.
//!
//! ⚠ This is a measurement of **our readers**, not of the game. A refusal is
//! either a reader defect or a file shape nobody has described yet, and the
//! sweep cannot tell which — it reports the message and the path so somebody
//! can.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use or_io::awc::AwcKey;
use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_title::install::crawl::{self, ControlFlow};

#[derive(Default)]
pub struct Sweep {
    pub seen: usize,
    pub attempted: usize,
    pub refused: usize,
    /// Per extension: (attempted, refused).
    pub by_extension: BTreeMap<String, (usize, usize)>,
    /// One example refusal per distinct message, so a defect affecting
    /// 40,000 files is one line rather than 40,000.
    pub reasons: BTreeMap<String, (usize, String)>,
    pub stopped: bool,
}

impl Sweep {
    fn record(&mut self, ext: &str, where_: &str, outcome: Result<(), String>) {
        self.attempted += 1;
        let entry = self.by_extension.entry(ext.to_string()).or_default();
        entry.0 += 1;
        if let Err(message) = outcome {
            self.refused += 1;
            entry.1 += 1;
            let reason = self
                .reasons
                .entry(shape_of(&message))
                .or_insert_with(|| (0, format!("{where_}: {message}")));
            reason.0 += 1;
        }
    }
}

/// Collapse a message to the thing that varies least, so counts group by
/// defect rather than by file.
fn shape_of(message: &str) -> String {
    message
        .split_whitespace()
        .map(|word| {
            if word.chars().any(|c| c.is_ascii_digit()) {
                "N"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn sweep(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    awc_key: AwcKey,
    extensions: &[String],
    cancel: &AtomicBool,
    warnings: &mut Vec<String>,
    on_progress: &mut dyn FnMut(&Sweep),
) -> Sweep {
    let mut sweep = Sweep::default();
    let outcome = crawl::crawl(game_root, provider, warnings, &mut |found| {
        if cancel.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        sweep.seen += 1;
        let Some(ext) = found
            .leaf
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
        else {
            return ControlFlow::Continue(());
        };
        if !extensions.is_empty() && !extensions.contains(&ext) {
            return ControlFlow::Continue(());
        }
        if !is_known(found.leaf, &ext) {
            return ControlFlow::Continue(());
        }
        let bytes = match found.read() {
            Ok(bytes) => bytes,
            Err(e) => {
                sweep.record(&ext, found.path, Err(format!("extract: {e}")));
                return ControlFlow::Continue(());
            }
        };
        let where_ = format!("{}/{}", found.container, found.path);
        sweep.record(&ext, &where_, parse(found.leaf, &ext, &bytes, awc_key));
        if sweep.attempted % 2_000 == 0 {
            on_progress(&sweep);
        }
        ControlFlow::Continue(())
    });
    sweep.stopped = outcome == ControlFlow::Break(());
    sweep
}

/// Whether the sweep parses a file named `leaf`: every reader the census
/// names except `.srl`'s, which reads metadata and is `meta`'s to sweep.
pub(crate) fn is_known(leaf: &str, ext: &str) -> bool {
    ext != "srl" && crate::formats::read(leaf, ext, &[], AwcKey::default()).is_some()
}

/// What a file named `leaf` reads as: a declared format through `declared`,
/// anything else through the census's reader.
///
/// ⚠ `declared` reads `.ydr`, `.ydd`, `.yft`, `.ypt` and `.ycd` into the
/// record tree rather than as their owned roots, and the refusals
/// `Oracle_Testimony.md` quotes are that read's.
fn parse(leaf: &str, ext: &str, data: &[u8], awc_key: AwcKey) -> Result<(), String> {
    if or_io::gta5::formats::format(ext).is_some() {
        return declared(ext, data).map_err(|e| e.to_string());
    }
    crate::formats::read(leaf, ext, data, awc_key).map_or(Ok(()), |(_, outcome)| outcome)
}

/// A resource read through its declarations: as its owned root where one is
/// declared, otherwise whole into the record tree.
fn declared(ext: &str, data: &[u8]) -> Result<(), or_io::Error> {
    match or_io::gta5::formats::format(ext) {
        Some(format) => format.open_declared(data).map(drop),
        None => Ok(()),
    }
}

pub fn report(sweep: &Sweep, top: usize) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} file(s) seen, {} parsed, {} refused{}\n\n",
        sweep.seen,
        sweep.attempted,
        sweep.refused,
        if sweep.stopped { " (stopped)" } else { "" }
    ));
    out.push_str("ext    attempted   refused\n");
    for (ext, (attempted, refused)) in &sweep.by_extension {
        out.push_str(&format!("{ext:<6} {attempted:>9}  {refused:>8}\n"));
    }
    if sweep.reasons.is_empty() {
        return out;
    }
    let mut reasons: Vec<(&String, &(usize, String))> = sweep.reasons.iter().collect();
    reasons.sort_by_key(|(_, (count, _))| std::cmp::Reverse(*count));
    out.push_str("\nrefusals, most common first:\n");
    for (_, (count, example)) in reasons.into_iter().take(top) {
        out.push_str(&format!("  {count:>7}x  {example}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::container::rpf::crypto::NoOpCryptoProvider;

    #[test]
    fn a_message_groups_by_its_shape_rather_than_its_numbers() {
        assert_eq!(
            shape_of("pointer 0x1234 out of bounds at offset 88"),
            shape_of("pointer 0xBEEF out of bounds at offset 4")
        );
        assert_ne!(
            shape_of("pointer out of bounds"),
            shape_of("unsupported version")
        );
    }

    /// ⚠ A sweep that reports zero refusals because it parsed nothing is the
    /// failure mode. Deliberately corrupt input must be counted.
    #[test]
    fn a_corrupt_resource_is_counted_as_a_refusal_rather_than_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("broken.ydr"), b"RSC7 but not really").unwrap();
        std::fs::write(dir.path().join("also.ybn"), vec![0u8; 512]).unwrap();
        std::fs::write(dir.path().join("ignored.txt"), b"hello").unwrap();

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let sweep = sweep(
            dir.path(),
            provider,
            AwcKey::default(),
            &[],
            &AtomicBool::new(false),
            &mut warnings,
            &mut |_| {},
        );

        assert_eq!(sweep.seen, 3);
        assert_eq!(sweep.attempted, 2, "only the known extensions are parsed");
        assert_eq!(sweep.refused, 2);
        assert_eq!(sweep.by_extension["ydr"], (1, 1));
        assert_eq!(sweep.by_extension["ybn"], (1, 1));
        assert!(!sweep.reasons.is_empty());

        let rendered = report(&sweep, 10);
        assert!(rendered.contains("2 refused"), "{rendered}");
        assert!(rendered.contains("broken.ydr") || rendered.contains("also.ybn"));
    }

    #[test]
    fn an_extension_filter_narrows_what_is_parsed() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.ydr"), b"nope").unwrap();
        std::fs::write(dir.path().join("b.ybn"), b"nope").unwrap();

        let provider: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let mut warnings = Vec::new();
        let sweep = sweep(
            dir.path(),
            provider,
            AwcKey::default(),
            &["ydr".to_string()],
            &AtomicBool::new(false),
            &mut warnings,
            &mut |_| {},
        );
        assert_eq!(sweep.attempted, 1);
        assert!(sweep.by_extension.contains_key("ydr"));
        assert!(!sweep.by_extension.contains_key("ybn"));
    }

    #[test]
    fn a_move_network_is_swept_and_a_corrupt_one_refused() {
        let empty = or_io::mrf::MvNetworkDef {
            version: or_io::mrf::VERSION,
            xrefs: Vec::new(),
            requests: Vec::new(),
            flags: Vec::new(),
            nodes: Vec::new(),
            string_table: Vec::new(),
        }
        .save()
        .expect("an empty network saves");
        let key = AwcKey::default();
        assert!(is_known("a.mrf", "mrf"));
        assert_eq!(parse("a.mrf", "mrf", &empty, key), Ok(()));
        assert!(parse("a.mrf", "mrf", b"nope", key).is_err());
    }
}
