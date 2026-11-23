//! A scan that read a whole GTA VI archive, kept in the configuration folder, a file an archive:
//! the archive is listed from it, and not read again, while its size and modification time stand.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use or_io::container::rpf::{seal_rpf9_scan, unseal_rpf9_scan, Rpf9ScanFind};

use super::rpf9_scan::{Ended, Found, Row, MAX_ROWS};

/// The version of what this file writes of a scan: a change to it takes another.
const VERSION: u64 = 1;
/// The version of the rules a scan finds and places bodies by: a change to what a scan lists of
/// an archive takes another, so every scan kept before it is scanned again.
pub(super) const RULES: u64 = 4;
/// The most a kept scan inflates to: the longest list, whose every row is under 256 bytes.
const MOST_BYTES: usize = MAX_ROWS * 256;

/// The folder the scans are kept in.
// ponytail: the scan of an archive since moved or deleted stays there; a sweep of the files whose
// archive is gone, if the folder ever grows large.
fn folder() -> Option<PathBuf> {
    super::prefs::config_root().map(|root| root.join("gta6_scans"))
}

/// Where the scan of one archive is kept, and the archive it is a scan of.
#[derive(Clone)]
pub(super) struct Kept {
    file: PathBuf,
    /// What a kept scan starts with: this file's version and the scan's rules, then the archive's size, modification time and path.
    of: Vec<u8>,
}

impl Kept {
    /// Where the scan of the archive at `path` is kept, the archive being as `meta` states it;
    /// `None` with no configuration folder, or no modification time to tell a changed archive by.
    pub fn of(path: &Path, meta: &std::fs::Metadata) -> Option<Self> {
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        let path = path.to_string_lossy();
        let mut of = Vec::new();
        // Nanoseconds, which a u64 holds for some centuries yet.
        let modified = modified.as_nanos() as u64;
        for word in [VERSION, RULES, meta.len(), modified, path.len() as u64] {
            of.extend(word.to_le_bytes());
        }
        of.extend(path.as_bytes());
        let name = or_io::gta6::hash::name_hash64(path.as_bytes());
        let file = folder()?.join(format!("{name:016x}.scan"));
        Some(Self { file, of })
    }

    /// Whether a scan of the archive's path is kept, of whatever size and time.
    pub fn is_kept(&self) -> bool {
        self.file.is_file()
    }

    /// The scan kept of the archive as it is now; `None` where none is, or it does not read.
    pub fn read(&self) -> Option<Found> {
        let sealed = std::fs::read(&self.file).ok()?;
        self.found(&sealed)
            .inspect_err(|e| tracing::info!("{}: {e}", self.file.display()))
            .ok()
    }

    fn found(&self, sealed: &[u8]) -> io::Result<Found> {
        let plain = unseal_rpf9_scan(sealed, MOST_BYTES)?;
        let other = |what| io::Error::new(io::ErrorKind::InvalidData, what);
        let bytes = &mut (plain.strip_prefix(self.of.as_slice()))
            .ok_or_else(|| other("a kept scan of another version, archive, size or time"))?;
        let made = UNIX_EPOCH.checked_add(Duration::from_secs(word(bytes)?));
        let mut found = Found {
            kept: Some(made.ok_or_else(|| other("a kept scan made at no time"))?),
            ended: Some(Ended::Whole),
            bodies: word(bytes)? as usize,
            reached: word(bytes)?,
            missing: word(bytes)?,
            unclaimed: word(bytes)?,
            ..Found::default()
        };
        for _ in 0..word(bytes)? {
            found.ends.insert(word(bytes)?, word(bytes)?);
        }
        while let Some((&kind, rest)) = bytes.split_first() {
            *bytes = rest;
            found.rows.push(match kind {
                0 => Row::Body(Box::new(Rpf9ScanFind::decode(bytes)?)),
                1 => Row::Missing {
                    from: word(bytes)?,
                    to: word(bytes)?,
                    bytes: word(bytes)?,
                },
                _ => return Err(other("a kept scan with a row of no kind")),
            });
        }
        Ok(found)
    }

    /// Keep `found`, which a scan that read the whole archive made, in place of any scan kept before.
    pub fn keep(&self, found: &Mutex<Found>) -> io::Result<()> {
        let mut plain = self.of.clone();
        let made = SystemTime::now().duration_since(UNIX_EPOCH);
        {
            // ponytail: the rows are written out under the list's lock, tens of milliseconds for
            // hundreds of thousands, once a scan; a slice at a time if a frame ever waits on it.
            let found = found.lock().unwrap_or_else(|p| p.into_inner());
            let ends = found.ends.iter().flat_map(|(at, end)| [*at, *end]);
            let head = [
                made.map_or(0, |since| since.as_secs()),
                found.bodies as u64,
                found.reached,
                found.missing,
                found.unclaimed,
                found.ends.len() as u64,
            ];
            for word in head.into_iter().chain(ends) {
                plain.extend(word.to_le_bytes());
            }
            for row in &found.rows {
                match row {
                    Row::Body(find) => {
                        plain.push(0);
                        find.encode(&mut plain);
                    }
                    &Row::Missing { from, to, bytes } => {
                        plain.push(1);
                        for word in [from, to, bytes] {
                            plain.extend(word.to_le_bytes());
                        }
                    }
                }
            }
        }
        if let Some(folder) = self.file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&self.file, seal_rpf9_scan(&plain)?)
    }
}

/// The next eight bytes of `bytes` as a number, which moves past them.
pub(super) fn word(bytes: &mut &[u8]) -> io::Result<u64> {
    let mut word = [0; 8];
    bytes.read_exact(&mut word)?;
    Ok(u64::from_le_bytes(word))
}
