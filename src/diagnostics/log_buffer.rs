//! Everything `tracing` emits, kept in memory so the Log panel can show it.
//!
//! ⚠ **Bounded twice, not once.** Warnings and errors go in one ring and
//! everything quieter in another, each with its own capacity. A single ring
//! would let a burst of debug lines push out the warning a user opened the
//! panel to read, which is the one entry the panel exists for.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// Warnings and errors kept.
pub const SERIOUS_CAPACITY: usize = 1_000;
/// Info, debug and trace entries kept.
pub const ROUTINE_CAPACITY: usize = 4_000;
/// Longest message kept, in bytes; the rest is cut and marked.
pub const MAX_MESSAGE_BYTES: usize = 2_048;

#[derive(Debug, Clone)]
pub struct LogRecord {
    /// Order of arrival across both rings, starting at 1.
    pub seq: u64,
    pub uptime: Duration,
    pub level: Level,
    pub target: Cow<'static, str>,
    pub message: String,
}

impl LogRecord {
    pub fn is_serious(&self) -> bool {
        self.level <= Level::WARN
    }

    /// One line, the same in the panel's copy and in a crash report.
    pub fn line(&self) -> String {
        format!(
            "+{:>9.3}s {:<5} {}: {}",
            self.uptime.as_secs_f64(),
            self.level,
            self.target,
            self.message
        )
    }
}

#[derive(Default)]
struct Rings {
    serious: VecDeque<Arc<LogRecord>>,
    routine: VecDeque<Arc<LogRecord>>,
    next_seq: u64,
    dropped_serious: u64,
    dropped_routine: u64,
    warnings: u64,
    errors: u64,
}

impl Rings {
    fn push(&mut self, started: Instant, level: Level, target: Cow<'static, str>, message: String) {
        self.next_seq += 1;
        let record = Arc::new(LogRecord {
            seq: self.next_seq,
            uptime: started.elapsed(),
            level,
            target,
            message: bounded(message),
        });
        // ⚠ OpenRage's own lines only: on a clean start the Vulkan loader logs
        // errors through `wgpu_hal` that are not OpenRage's to report.
        match level {
            Level::ERROR if is_own(&record.target) => self.errors += 1,
            Level::WARN if is_own(&record.target) => self.warnings += 1,
            _ => {}
        }
        let (ring, capacity, dropped) = if record.is_serious() {
            (
                &mut self.serious,
                SERIOUS_CAPACITY,
                &mut self.dropped_serious,
            )
        } else {
            (
                &mut self.routine,
                ROUTINE_CAPACITY,
                &mut self.dropped_routine,
            )
        };
        if ring.len() == capacity {
            ring.pop_front();
            *dropped += 1;
        }
        ring.push_back(record);
    }

    fn merged(&self) -> Vec<Arc<LogRecord>> {
        let mut out = Vec::with_capacity(self.serious.len() + self.routine.len());
        let (mut a, mut b) = (
            self.serious.iter().peekable(),
            self.routine.iter().peekable(),
        );
        loop {
            let take_a = match (a.peek(), b.peek()) {
                (Some(x), Some(y)) => x.seq < y.seq,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            let next = if take_a { a.next() } else { b.next() };
            if let Some(record) = next {
                out.push(Arc::clone(record));
            }
        }
        out
    }
}

/// Whether `target` is the app's or a workspace crate's.
fn is_own(target: &str) -> bool {
    let krate = target.split_once("::").map_or(target, |(krate, _)| krate);
    krate == "openrage" || krate.starts_with("or_")
}

fn bounded(mut message: String) -> String {
    if message.len() > MAX_MESSAGE_BYTES {
        let mut cut = MAX_MESSAGE_BYTES;
        while !message.is_char_boundary(cut) {
            cut -= 1;
        }
        message.truncate(cut);
        message.push_str(" …[cut]");
    }
    message
}

/// What the buffer holds at one moment, oldest first.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub records: Vec<Arc<LogRecord>>,
    pub dropped_serious: u64,
    pub dropped_routine: u64,
    pub warnings: u64,
    pub errors: u64,
}

pub struct LogBuffer {
    rings: Mutex<Rings>,
    started: Instant,
    generation: AtomicU64,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self {
            rings: Mutex::new(Rings::default()),
            started: Instant::now(),
            generation: AtomicU64::new(0),
        }
    }
}

impl LogBuffer {
    fn rings(&self) -> MutexGuard<'_, Rings> {
        self.rings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn push(&self, level: Level, target: impl Into<Cow<'static, str>>, message: String) {
        self.rings()
            .push(self.started, level, target.into(), message);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Changes whenever something is pushed, so a reader can skip a copy.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> Snapshot {
        let rings = self.rings();
        Snapshot {
            records: rings.merged(),
            dropped_serious: rings.dropped_serious,
            dropped_routine: rings.dropped_routine,
            warnings: rings.warnings,
            errors: rings.errors,
        }
    }

    /// `(warnings, errors)` OpenRage logged since the process started, dropped or not.
    pub fn counts(&self) -> (u64, u64) {
        let rings = self.rings();
        (rings.warnings, rings.errors)
    }

    /// The latest warning or error of OpenRage's own still held, which [`LogBuffer::counts`] counts among the others.
    pub fn latest_own_serious(&self) -> Option<Arc<LogRecord>> {
        let rings = self.rings();
        (rings.serious.iter().rev())
            .find(|record| is_own(&record.target))
            .cloned()
    }

    /// The last `n` records, or `None` when the buffer is held.
    ///
    /// ⚠ **The panic hook's only way in.** A panic raised while this thread
    /// holds the lock — inside [`LogBuffer::push`] — would deadlock or abort on
    /// a blocking `lock`, so the hook asks and accepts no for an answer. A
    /// poisoned lock is still read: the rings are consistent between pushes.
    pub fn try_recent(&self, n: usize) -> Option<Vec<Arc<LogRecord>>> {
        let rings = match self.rings.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let merged = rings.merged();
        let skip = merged.len().saturating_sub(n);
        Some(merged.into_iter().skip(skip).collect())
    }

    /// [`LogBuffer::push`] that gives up rather than wait, for the same reason.
    pub fn try_push(
        &self,
        level: Level,
        target: impl Into<Cow<'static, str>>,
        message: String,
    ) -> bool {
        let mut rings = match self.rings.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        rings.push(self.started, level, target.into(), message);
        drop(rings);
        self.generation.fetch_add(1, Ordering::Release);
        true
    }
}

/// The buffer every window's Log panel reads.
pub fn global() -> &'static Arc<LogBuffer> {
    static GLOBAL: OnceLock<Arc<LogBuffer>> = OnceLock::new();
    GLOBAL.get_or_init(|| Arc::new(LogBuffer::default()))
}

/// The `tracing` layer that fills a [`LogBuffer`].
pub struct BufferLayer {
    buffer: Arc<LogBuffer>,
}

impl BufferLayer {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // ⚠ Formatted before the lock is taken, so nothing that can panic
        // runs while the buffer is held.
        let mut text = Message::default();
        event.record(&mut text);
        let meta = event.metadata();
        let target = text
            .target
            .take()
            .map_or(Cow::Borrowed(meta.target()), Cow::Owned);
        self.buffer.push(*meta.level(), target, text.finish());
    }
}

/// ⚠ The `log` crate's bridge files every line it forwards under the target
/// `log` and states the real one, with where the line was written, in fields
/// named for it.
const BRIDGE_FIELDS: [&str; 3] = ["log.module_path", "log.file", "log.line"];

#[derive(Default)]
struct Message {
    message: String,
    fields: String,
    /// The target a line written through the `log` crate names.
    target: Option<String>,
}

impl Message {
    fn finish(self) -> String {
        match (self.message.is_empty(), self.fields.is_empty()) {
            (_, true) => self.message,
            (true, false) => self.fields.trim_start().to_string(),
            (false, false) => format!("{}{}", self.message, self.fields),
        }
    }
}

impl Visit for Message {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message.push_str(value),
            "log.target" => self.target = Some(value.to_string()),
            name if BRIDGE_FIELDS.contains(&name) => {}
            name => {
                let _ = write!(self.fields, " {name}={value}");
            }
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => {
                let _ = write!(self.message, "{value:?}");
            }
            "log.target" => self.target = Some(format!("{value:?}")),
            name if BRIDGE_FIELDS.contains(&name) => {}
            name => {
                let _ = write!(self.fields, " {name}={value:?}");
            }
        }
    }
}
