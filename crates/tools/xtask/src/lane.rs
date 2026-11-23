//! Agent lanes: a git worktree under `../openrage-lanes`, made and marked.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};

use crate::git;

const MARKER: &str = ".agent-worktree";

/// ⚠ The only supported way to make a lane: the janitor enumerates the lane
/// root, and a worktree anywhere else is found only by name. Leave
/// `CARGO_TARGET_DIR` unset in it.
pub fn new(name: &str) -> Result<ExitCode> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('-') {
        bail!("a lane name is one path segment, not `{name}`");
    }
    let root = git::here()?;
    if root.join(MARKER).exists() {
        bail!(
            "this is a lane; the lane root is only locatable from the checkout. \
             Run it from the checkout, not from {}.",
            root.display()
        );
    }
    let lanes: PathBuf = root.parent().map_or_else(
        || PathBuf::from("openrage-lanes"),
        |p| p.join("openrage-lanes"),
    );
    std::fs::create_dir_all(&lanes)?;
    let worktree = lanes.join(name);
    let branch = format!("agent/{name}");
    git::run(
        &root,
        &[
            "worktree",
            "add",
            &worktree.to_string_lossy(),
            "-b",
            &branch,
        ],
    )?;
    std::fs::write(
        worktree.join(MARKER),
        format!("agent lane, created {}\n", utc_now()),
    )?;
    println!(
        "lane ready at {} on {branch}. Build output stays in the worktree; leave CARGO_TARGET_DIR unset, and create nothing outside {}.",
        worktree.display(),
        lanes.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// The current UTC time as `2026-10-06T14:49:51Z`.
fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc(seconds)
}

/// `seconds` since the Unix epoch as an RFC 3339 UTC timestamp.
fn utc(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// Whether `path` sits under the lane root of the checkout beside it.
#[allow(dead_code)]
pub fn is_lane(path: &Path) -> bool {
    path.join(MARKER).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_is_the_calendar_date_it_names() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        // The merge of #576, recorded by GitHub as 2026-10-06T14:49:51Z.
        assert_eq!(utc(1_791_298_191), "2026-10-06T14:49:51Z");
    }
}
