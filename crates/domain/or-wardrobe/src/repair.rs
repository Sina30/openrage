//! Repairing a model whose counts contradict the runs they describe, losslessly and on request.
//!
//! The policy is the owner's of 2026-10-03: a count is rewritten from its run only when somebody asked, and only where that
//! loses nothing. A pack holds no compiled file that it makes, so a model that needed a repair is made a drawables source from the
//! repaired file, the garment names the source, and the file it was is untouched. The sentences of the repair are kept in the new source's
//! `asset.json` (`repaired`) as well as in the report.

use std::path::Path;

use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::decl::{File, Gen8, Gen9, Repair};
use or_io::gta5::{GtaDrawable, PgDictionary};

use crate::edit::Copying;
use crate::facts::{is_source, stays_inside};
use crate::import::protected;
use crate::intake::{self, Scratch};

/// What repairing one model came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Repaired {
    /// Nothing to repair: the file is a source, a reference, not there, or states no count against its run.
    Nothing,
    /// The new source, as a path inside the pack, and each count rewritten.
    Source(String, Vec<String>),
}

/// A file with its counts rewritten from their runs, and each rewrite.
type Rewritten = (Vec<u8>, Vec<String>);

/// The file `bytes` with its counts rewritten from their runs; none when it states none against its run.
fn repaired_bytes(bytes: &[u8]) -> Result<Option<Rewritten>, String> {
    let seal = Rsc7::peek(bytes).map_err(|e| format!("it is not a resource: {e}"))?;
    let format = or_io::gta5::formats::format("ydd").ok_or("no .ydd format is declared")?;
    let target = format
        .target_of_version(seal.version())
        .map_err(|e| e.to_string())?;
    let opened = if target.is_gen9() {
        PgDictionary::<GtaDrawable<Gen9>>::from_bytes(bytes)
            .and_then(|d| d.to_bytes_repairing(Repair::Lossless))
    } else {
        PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes)
            .and_then(|d| d.to_bytes_repairing(Repair::Lossless))
    };
    let (repaired, repairs) = opened.map_err(|e| e.to_string())?;
    Ok((!repairs.is_empty()).then_some((repaired, repairs)))
}

/// Repair the model `rel` of the pack at `copying`'s folder: a source for the repaired file when it needed one.
///
/// Refused, with the reason, a file that opens with `FXAP` (a protected asset), one that does not open, and one whose counts could only be rewritten at a loss.
pub(crate) fn model(
    copying: &mut Copying,
    scratch: &mut Scratch,
    rel: &str,
) -> Result<Repaired, String> {
    if !stays_inside(rel)
        || is_source(rel)
        || or_project::reference::is_reference(rel)
        || !rel.to_ascii_lowercase().ends_with(".ydd")
    {
        return Ok(Repaired::Nothing);
    }
    let at = copying
        .folder
        .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !at.is_file() {
        return Ok(Repaired::Nothing);
    }
    if let Some(why) = protected(&at) {
        return Err(why);
    }
    let bytes = std::fs::read(&at).map_err(|e| format!("cannot be read: {e}"))?;
    let Some((repaired, lines)) = repaired_bytes(&bytes)? else {
        return Ok(Repaired::Nothing);
    };
    let name = at
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let held = scratch.next().map_err(|e| e.to_string())?.join(&name);
    std::fs::write(&held, &repaired).map_err(|e| format!("cannot write a scratch file: {e}"))?;
    let mark = copying.mark();
    let made = intake::model(copying, scratch, &held).map_err(|e| e.to_string());
    let into =
        made.and_then(|into| keep_the_report(&copying.folder.join(&into), &lines).map(|()| into));
    match into {
        Ok(into) => Ok(Repaired::Source(into, lines)),
        Err(why) => {
            copying.rollback_to(mark);
            Err(why)
        }
    }
}

/// Note the sentences of the repair in the manifest of the source it made.
fn keep_the_report(source: &Path, lines: &[String]) -> Result<(), String> {
    let refused = |p: or_source::Problem| p.to_string();
    let mut opened = or_source::asset::Source::open(source).map_err(refused)?;
    opened
        .manifest
        .origin
        .extra
        .insert("repaired".into(), serde_json::json!(lines));
    opened.write_manifest().map_err(refused)
}

/// The sentences of the repair of a model, for a report: how many counts, and the first few.
pub(crate) fn sentence(file: &str, lines: &[String]) -> String {
    const SHOWN: usize = 3;
    let more = lines.len().saturating_sub(SHOWN);
    let mut said = format!(
        "{file}: {} count(s) rewritten from their runs: {}",
        lines.len(),
        lines
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    );
    if more > 0 {
        said.push_str(&format!(" and {more} more"));
    }
    said
}
