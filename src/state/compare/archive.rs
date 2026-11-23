//! Two archives compared entry by entry: each file one holds alone, and each
//! both hold whose bytes differ, a nested archive read as the files in it and
//! a resource that differs read field by field.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use or_io::container::rpf::{BodyForm, RpfEntry};
use or_io::container::{RpfArchive, RpfCryptoProvider};
use or_title::vfs::{is_archive_segment, ArchiveDevice};

use super::found::{By, Change, FieldChange};
use super::{compared, Comparison, Ended, Found};
use crate::state::extract_job::archive_file;
use crate::state::rpf_archive_open::open_archive_from_disk;
use crate::state::DocumentKind;

/// How deep archives nest inside archives before one is read as a file.
const DEPTH: u32 = 8;

/// An archive as one side of a comparison.
pub(super) enum Archive {
    /// One a tree holds open, as it was last saved.
    Open(Arc<RpfArchive>),
    /// A file on disk, opened on the worker with `keys`.
    Disk(PathBuf, Arc<dyn RpfCryptoProvider>),
}

impl Archive {
    pub(super) fn open(archive: Arc<RpfArchive>) -> Self {
        Self::Open(archive)
    }

    pub(super) fn on_disk(file: PathBuf, keys: Arc<dyn RpfCryptoProvider>) -> Self {
        Self::Disk(file, keys)
    }

    /// The file on disk it is read out of.
    pub(super) fn file(&self) -> Option<PathBuf> {
        match self {
            Self::Open(archive) => archive_file(archive).map(PathBuf::from),
            Self::Disk(file, _) => Some(file.clone()),
        }
    }

    /// Opened for reading; `None` where it is a file that is not there.
    fn device(&self) -> Result<Option<ArchiveDevice>, String> {
        let archive = match self {
            Self::Open(archive) => Arc::clone(archive),
            Self::Disk(file, _) if !file.is_file() => return Ok(None),
            Self::Disk(file, keys) => open_archive_from_disk(file, Arc::clone(keys))?,
        };
        Ok(Some(ArchiveDevice::new(archive)))
    }
}

/// One file of an archive, or of an archive nested in it.
struct Entry {
    /// Where it is, through the archives it is nested in, spelled as they spell it.
    path: String,
    device: ArchiveDevice,
    index: u32,
}

impl Entry {
    /// What the archive's table says its size is, which reads nothing.
    fn stored(&self) -> u64 {
        match self.device.archive().entries.get(self.index as usize) {
            Some(RpfEntry::Binary(binary)) => binary.uncompressed_size as u64,
            Some(RpfEntry::Resource(resource)) => resource.consumed_size as u64,
            _ => 0,
        }
    }

    fn read(&self) -> Option<Vec<u8>> {
        let read = self.device.read_entry(self.index, BodyForm::Decoded);
        read.inspect_err(|e| tracing::warn!(path = %self.path, error = %e, "an entry could not be read to compare"))
            .ok()
    }
}

/// Every file of `device`, and of each archive nested in it, which is not itself one.
fn listed(device: &ArchiveDevice, prefix: &str, depth: u32, out: &mut Vec<Entry>) {
    for node in &device.archive().nodes {
        if node.is_dir() {
            continue;
        }
        let name = node.path().replace('\\', "/").trim_matches('/').to_string();
        let path = match prefix.is_empty() {
            true => name,
            false => format!("{prefix}/{name}"),
        };
        if is_archive_segment(node.name()) && depth < DEPTH {
            if let Ok(nested) = device.open_nested(node.entry_index()) {
                let inside = ArchiveDevice::new(nested);
                listed(&inside, &path, depth + 1, out);
                continue;
            }
        }
        out.push(Entry {
            path,
            device: device.clone(),
            index: node.entry_index(),
        });
    }
}

fn change(path: &str, field: &str, change: Change, left: String, right: String) -> FieldChange {
    FieldChange {
        record: path.to_string(),
        named: String::new(),
        field: field.to_string(),
        change,
        left,
        right,
    }
}

/// Both bodies' sizes, and where they first differ where the sizes do not.
pub(super) fn weighed(left: &[u8], right: &[u8]) -> [String; 2] {
    let size = |body: &[u8]| or_job::human_bytes(body.len() as u64);
    if left.len() != right.len() {
        return [size(left), size(right)];
    }
    let at = left.iter().zip(right).position(|(a, b)| a != b);
    [
        size(left),
        format!("{}, differing from byte {}", size(right), at.unwrap_or(0)),
    ]
}

/// How an entry differs: its rows, and whether reading it field by field left a part unread.
struct Differs {
    rows: Vec<FieldChange>,
    unread: bool,
}

/// Entry `x` of the left and `y` of the right, which stand at one path: how they differ, if they do.
///
/// ⚠ A resource, binary metadata or a texture dictionary reads field by field,
/// each change under its entry as it is where the two compare on their own;
/// any other entry, or one whose fields tell nothing apart, by its sizes and
/// where its bytes first differ.
fn differ(x: &Entry, y: &Entry) -> Differs {
    let (a, b) = (x.read(), y.read());
    let sizes = match (&a, &b) {
        (Some(a), Some(b)) if a == b => {
            return Differs {
                rows: Vec::new(),
                unread: false,
            }
        }
        (Some(a), Some(b)) => {
            if let Some(inside) = fields_of(&y.path, a, b) {
                return inside;
            }
            weighed(a, b)
        }
        (a, b) => {
            let size = |body: &Option<Vec<u8>>| {
                body.as_ref().map_or_else(
                    || "could not be read".to_string(),
                    |body| or_job::human_bytes(body.len() as u64),
                )
            };
            [size(a), size(b)]
        }
    };
    let [left, right] = sizes;
    Differs {
        rows: vec![change(&y.path, "Content", Change::Changed, left, right)],
        unread: false,
    }
}

/// The fields `a` and `b`, entries called `path`, differ in, each under `path`; `None` where they are not compared by field or no field differs.
fn fields_of(path: &str, a: &[u8], b: &[u8]) -> Option<Differs> {
    // A text line by line would be the whole of both, which no row here shows.
    if DocumentKind::from_path(Path::new(path)).is_none_or(|kind| kind == DocumentKind::Text) {
        return None;
    }
    let Found::Fields {
        changes, unread, ..
    } = compared((path, a), (path, b))
    else {
        return None;
    };
    let rows: Vec<FieldChange> = (changes.into_iter())
        .map(|inside| FieldChange {
            record: match inside.record.is_empty() {
                true => path.to_string(),
                false => format!("{path} > {}", inside.record),
            },
            ..inside
        })
        .collect();
    (!rows.is_empty()).then_some(Differs {
        rows,
        unread: unread.is_some(),
    })
}

/// The two archives compared, entry by entry, at the paths they hold them.
///
/// ⚠ Reads each entry both hold, so a big archive takes as long as reading
/// it; `stop` ends it between entries.
pub(super) fn compare(
    headings: [String; 2],
    places: [String; 2],
    names: [String; 2],
    archives: [Archive; 2],
    stop: &AtomicBool,
) -> Ended {
    let [left, right] = archives;
    let right_device = match right.device() {
        Ok(Some(device)) => device,
        Ok(None) => return Ended::Failed(format!("{} could not be read", names[1])),
        Err(why) => return Ended::Failed(why),
    };
    let left_device = match left.device() {
        Ok(Some(device)) => device,
        Ok(None) => {
            return Ended::NoOriginal(format!(
                "The game has no {} of its own: the mods overlay adds it",
                names[1]
            ))
        }
        Err(why) => return Ended::Failed(why),
    };
    let (mut l, mut r) = (Vec::new(), Vec::new());
    listed(&left_device, "", 0, &mut l);
    listed(&right_device, "", 0, &mut r);
    let key = |entry: &Entry| entry.path.to_ascii_lowercase();
    let in_right: HashMap<String, &Entry> = r.iter().map(|y| (key(y), y)).collect();
    let in_left: HashMap<String, &Entry> = l.iter().map(|x| (key(x), x)).collect();
    let size = |entry: &Entry| or_job::human_bytes(entry.stored());

    // Each entry's rows, so that a sort keeps the rows of one entry in the order they were read.
    let mut entries: Vec<(String, Vec<FieldChange>)> = Vec::new();
    let mut unread = 0;
    for x in &l {
        if stop.load(Ordering::Relaxed) {
            return Ended::Failed("The comparison was stopped".to_string());
        }
        let rows = match in_right.get(&key(x)) {
            Some(y) => {
                let inside = differ(x, y);
                unread += usize::from(inside.unread);
                inside.rows
            }
            None => vec![change(&x.path, "", Change::Removed, size(x), String::new())],
        };
        entries.push((key(x), rows));
    }
    for y in r.iter().filter(|y| !in_left.contains_key(&key(y))) {
        let added = change(&y.path, "", Change::Added, String::new(), size(y));
        entries.push((key(y), vec![added]));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ended::Compared(Comparison {
        headings,
        places,
        found: Found::Fields {
            by: By::Entry,
            changes: entries.into_iter().flat_map(|(_, rows)| rows).collect(),
            unread: (unread > 0).then(|| unread_words(unread)),
        },
    })
}

/// What a comparison says of `entries` it did not read whole, a limit of the outline having left a part out.
fn unread_words(entries: usize) -> String {
    let which = match entries {
        1 => "1 changed entry".to_string(),
        n => format!("{n} changed entries"),
    };
    format!("Not read whole, in {which}: compare one on its own to see what was left out.")
}

#[cfg(test)]
mod tests {
    use or_io::container::NoOpCryptoProvider;

    use super::*;
    use crate::state::nested_archives_survive_a_save::archive as archive_bytes;

    /// The two archives `a.rpf` and `b.rpf`, which differ in one entry, compared with `stop` as given.
    fn compared(stop: bool) -> Ended {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let crypto: Arc<dyn RpfCryptoProvider> = Arc::new(NoOpCryptoProvider::new());
        let on_disk = |name: &str, body: &[u8]| {
            let file = dir.path().join(name);
            let entries: [(&str, &[u8]); 2] = [("kept.txt", b"same"), ("edited.txt", body)];
            std::fs::write(&file, archive_bytes(&entries)).expect("the archive");
            Archive::on_disk(file, Arc::clone(&crypto))
        };
        let both = |text: &str| [text.to_string(), text.to_string()];
        let archives = [on_disk("a.rpf", b"before"), on_disk("b.rpf", b"after!")];
        compare(
            both("side"),
            both("place"),
            both("name.rpf"),
            archives,
            &AtomicBool::new(stop),
        )
    }

    #[test]
    fn a_stop_ends_an_archive_comparison_between_its_entries_and_without_one_it_finds_the_change() {
        match compared(true) {
            Ended::Failed(why) => assert_eq!(why, "The comparison was stopped"),
            Ended::Compared(found) => panic!("a stopped comparison went on: {found:?}"),
            Ended::NoOriginal(why) => panic!("no original: {why}"),
        }
        match compared(false) {
            Ended::Compared(found) => assert_eq!(found.summary(), "by entry, 1 difference"),
            Ended::Failed(why) | Ended::NoOriginal(why) => panic!("not compared: {why}"),
        }
    }
}
