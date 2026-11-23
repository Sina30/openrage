//! Rebuilding an archive without its slack, which `openrage-cli rpf defrag` and the editor both run.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use super::archive::RpfArchive;
use super::builder::RpfBuilder;
use super::entry::RpfEntry;
use super::version::RpfVersion;
use crate::error::{IoError, Result};

/// An archive's files re-added to a builder, and the size its save will write.
pub struct Defrag {
    pub builder: RpfBuilder,
    pub files: usize,
    pub payload: u64,
    pub planned: u64,
}

impl Defrag {
    /// Re-add every file of `archive`, calling `on_file` with each file's ordinal and path before reading it.
    ///
    /// ⚠ **Each entry's compression decision is carried, not re-made.**
    /// Rebuilding with a fresh compression choice turned a 12,288-byte fixture
    /// into 81,920, which is re-encoding rather than slack. See
    /// `crates/io/or-io/tests/what_a_write_costs_before_you_make_it.rs`.
    pub fn plan(archive: &RpfArchive, on_file: impl FnMut(usize, &str)) -> Result<Self> {
        let builder = archive.edit()?;
        let planned = builder
            .planned_size_with(on_file)
            .map_err(|e| invalid("rebuild", e))?;
        // What the rebuild reads: RPF7's files decoded, every other version's as they are stored.
        let payload = archive
            .entries
            .iter()
            .map(|entry| match entry {
                RpfEntry::Binary(b) if archive.version == RpfVersion::Rpf7 => {
                    u64::from(b.uncompressed_size)
                }
                RpfEntry::Binary(b) => u64::from(b.read_size()),
                RpfEntry::Resource(r) => u64::from(r.consumed_size),
                RpfEntry::Directory(_) => 0,
            })
            .sum();
        Ok(Self {
            builder,
            files: archive.file_count(),
            payload,
            planned,
        })
    }

    /// Replace `path` with the rebuild; true when this call took the `.bak` beside it.
    pub fn write_over(&self, path: &Path) -> Result<bool> {
        // ⚠ The `.bak` is taken only when none exists, so it keeps the file from
        // before the first write. Refreshed every time, it would hold the previous
        // rebuild and the shipped file would be gone.
        let backup = path.with_extension("rpf.bak");
        let took_backup = !backup.exists();
        if took_backup {
            fs::copy(path, &backup)?;
        }

        let temp = path.with_extension("rpf.tmp");
        let written = fs::File::create(&temp)
            .map_err(IoError::from)
            .and_then(|file| {
                let mut out = BufWriter::new(file);
                self.builder.write_to(&mut out)?;
                Ok(out.flush()?)
            })
            .and_then(|()| Ok(fs::metadata(&temp)?.len()));
        let actual = match written {
            Ok(actual) => actual,
            Err(e) => {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
        };
        if actual != self.planned {
            let _ = fs::remove_file(&temp);
            return Err(IoError::Invalid {
                what: format!("defragmenting {}", path.display()),
                why: format!(
                    "the rebuild wrote {actual} bytes where the preview said {}, so \
                     nothing was replaced. The preview runs the save, so the two \
                     disagree about something that is not layout.",
                    self.planned
                ),
            });
        }
        fs::rename(&temp, path)?;
        Ok(took_backup)
    }
}

fn invalid(what: &str, e: IoError) -> IoError {
    IoError::Invalid {
        what: what.to_owned(),
        why: e.to_string(),
    }
}
