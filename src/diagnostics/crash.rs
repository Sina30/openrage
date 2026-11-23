//! What happens when the process panics: a report on disk, and a window that
//! says where it is.
//!
//! ⚠ **Nothing in the hook may panic, block, or log.** A panic inside a panic
//! hook aborts the process with neither report nor message. Blocking on the
//! log buffer deadlocks when the panic was raised inside it. And a `tracing`
//! call re-enters that same buffer. So the hook writes stderr directly, takes
//! every lock with `try_lock`, and treats every failure as a line in the report
//! rather than a reason to stop.
//!
//! ⚠ **Local and user-initiated.** The report is a text file under the
//! configuration root and a copy button; nothing is sent anywhere.

use std::io::Write as _;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, TryLockError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::Level;

use super::log_buffer::{LogBuffer, LogRecord};

/// Log lines copied into a report.
pub const RECENT_LINES: usize = 200;

const UNREAD_MARKER: &str = "unread.txt";

/// A report the user has not dismissed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashNotice {
    /// `None` when the report could not be written and exists only here.
    pub path: Option<PathBuf>,
    pub report: String,
    /// The process that wrote it is gone; this one found it on startup.
    pub from_last_session: bool,
}

static PENDING: Mutex<Option<CrashNotice>> = Mutex::new(None);
static EDITION: Mutex<Option<&'static str>> = Mutex::new(None);

/// Where reports go: `crash-reports` under the configuration root.
pub fn reports_dir() -> Option<PathBuf> {
    crate::state::prefs::config_root().map(|root| root.join("crash-reports"))
}

/// The edition a report names, as the user knows it.
pub fn note_edition(label: &'static str) {
    *EDITION.lock().unwrap_or_else(PoisonError::into_inner) = Some(label);
}

/// A report waiting to be shown, if any.
pub fn pending() -> Option<CrashNotice> {
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Show a report. The hook does not come through here: it cannot block.
pub fn publish(notice: CrashNotice) {
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(notice);
}

/// The user has read it: stop showing it, here and on the next start.
pub fn dismiss() {
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(dir) = reports_dir() {
        let _ = std::fs::remove_file(dir.join(UNREAD_MARKER));
    }
}

/// The report a previous process wrote and nobody dismissed.
pub fn load_unread(dir: &Path) -> Option<CrashNotice> {
    let name = std::fs::read_to_string(dir.join(UNREAD_MARKER)).ok()?;
    let path = dir.join(name.trim());
    let report = std::fs::read_to_string(&path).ok()?;
    Some(CrashNotice {
        path: Some(path),
        report,
        from_last_session: true,
    })
}

/// Surface an unread report from a previous run, if there is one.
pub fn restore_unread() {
    let Some(notice) = reports_dir().as_deref().and_then(load_unread) else {
        return;
    };
    let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
    if pending.is_none() {
        *pending = Some(notice);
    }
}

/// What the hook writes to, captured once when it is installed.
pub struct Reporter {
    pub dir: Option<PathBuf>,
    pub buffer: Arc<LogBuffer>,
}

/// Install the hook, keeping whatever was there before it.
///
/// ⚠ The previous hook still runs, after the report is written, so a
/// terminal still gets the standard panic message.
pub fn install(reporter: Reporter) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reporter.on_panic(info);
        previous(info);
    }));
}

/// The parts of a panic a report states, gathered without the hook's
/// `PanicHookInfo` so the composition can be tested on its own.
pub struct PanicFacts {
    pub message: String,
    pub location: String,
    pub thread: String,
    pub backtrace: String,
    pub unix_time: Duration,
}

impl PanicFacts {
    fn gather(info: &PanicHookInfo<'_>) -> Self {
        let message = info
            .payload_as_str()
            .unwrap_or("(a panic payload that is not text)")
            .to_string();
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "an unknown location".to_string());
        let thread = std::thread::current()
            .name()
            .unwrap_or("an unnamed thread")
            .to_string();
        Self {
            message,
            location,
            thread,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
            unix_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default(),
        }
    }
}

impl Reporter {
    fn on_panic(&self, info: &PanicHookInfo<'_>) {
        let facts = PanicFacts::gather(info);
        let recent = self.buffer.try_recent(RECENT_LINES);
        let edition = match EDITION.try_lock() {
            Ok(guard) => *guard,
            Err(TryLockError::Poisoned(p)) => *p.into_inner(),
            Err(TryLockError::WouldBlock) => None,
        };
        let report = compose(&facts, recent.as_deref(), edition);
        let written = self.write(&report, facts.unix_time);

        let where_ = match &written {
            Ok(path) => format!("a crash report was written to {}", path.display()),
            Err(why) => format!("no crash report could be written: {why}"),
        };
        let _ = writeln!(std::io::stderr(), "OpenRage panicked; {where_}");
        self.buffer.try_push(
            Level::ERROR,
            "openrage::panic",
            format!(
                "panicked at {}: {}; {where_}",
                facts.location, facts.message
            ),
        );

        let notice = CrashNotice {
            path: written.ok(),
            report,
            from_last_session: false,
        };
        match PENDING.try_lock() {
            Ok(mut slot) => *slot = Some(notice),
            Err(TryLockError::Poisoned(p)) => *p.into_inner() = Some(notice),
            Err(TryLockError::WouldBlock) => {}
        }
    }

    fn write(&self, report: &str, unix_time: Duration) -> Result<PathBuf, String> {
        static SEQUENCE: AtomicU32 = AtomicU32::new(0);
        let dir = self
            .dir
            .as_ref()
            .ok_or("this platform has no configuration directory")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = format!(
            "crash-{}-{}-{}.txt",
            file_stamp(unix_time),
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let path = dir.join(&name);
        std::fs::write(&path, report).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = std::fs::write(dir.join(UNREAD_MARKER), &name);
        Ok(path)
    }
}

/// The report as the user will paste it.
pub fn compose(
    facts: &PanicFacts,
    recent: Option<&[Arc<LogRecord>]>,
    edition: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("OpenRage crash report\n=====================\n\n");
    out.push_str(&format!("Version:  {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("When:     {} UTC\n", utc_stamp(facts.unix_time)));
    out.push_str(&format!(
        "System:   {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str(&format!(
        "Edition:  {}\n",
        edition.unwrap_or("none chosen yet")
    ));
    out.push_str(&format!("Thread:   {}\n", facts.thread));
    out.push_str(&format!("Panic:    {}\n", facts.message));
    out.push_str(&format!("At:       {}\n", facts.location));

    out.push_str("\nRecent log\n----------\n");
    match recent {
        Some([]) => out.push_str("(nothing was logged before the panic)\n"),
        Some(lines) => {
            for record in lines {
                out.push_str(&record.line());
                out.push('\n');
            }
        }
        None => out.push_str(
            "(unavailable: the panic was raised while the log was being written, \
             so reading it here would have deadlocked)\n",
        ),
    }

    out.push_str("\nBacktrace\n---------\n");
    out.push_str(&facts.backtrace);
    if !facts.backtrace.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// `YYYY-MM-DD HH:MM:SS`, from seconds since the Unix epoch.
pub fn utc_stamp(since_epoch: Duration) -> String {
    let (y, mo, d, h, mi, s) = civil(since_epoch.as_secs());
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

fn file_stamp(since_epoch: Duration) -> String {
    let (y, mo, d, h, mi, s) = civil(since_epoch.as_secs());
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// Howard Hinnant's `civil_from_days`, plus the time of day.
fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3_600) as u32,
        (rem % 3_600 / 60) as u32,
        (rem % 60) as u32,
    )
}
