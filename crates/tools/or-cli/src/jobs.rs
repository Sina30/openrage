//! The terminal's half of a long job: an interrupt handler and one line a second.
//!
//! The counting, the cancellation flag and the per-item tolerance are
//! [`or_job`]'s, shared with the editor. What is here is what a console has and
//! a frame does not: a Ctrl-C hook, and a rate limiter over stderr.
//!
//! ⚠ Progress prints at most one line a second and one at the end: a line per
//! item makes a 300,000-entry run produce 300,000 lines and spend real time in
//! the terminal.

use std::io::Write;
use std::sync::atomic::AtomicBool as Flag;
#[cfg(any(windows, test))]
use std::sync::atomic::AtomicBool;
#[cfg(any(windows, unix, test))]
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_job::{Cancel, Progress, Report};

/// A [`Cancel`] that Ctrl-C sets.
///
/// ⚠ **Sets a flag rather than exiting**: a job that dies on the signal leaves
/// a half-written output directory and reports nothing, where one that unwinds
/// says how far it got. The second Ctrl-C is left to the runtime's default so
/// a wedged job is still killable.
pub fn on_interrupt() -> Cancel {
    let flag = Cancel::new();
    let handler = flag.clone();
    // A failure here is not fatal: it means something already owns the handler,
    // and a job that cannot be cancelled is worse than one that cannot be
    // cancelled *twice*.
    if let Err(e) = install_stop_handler(move || handler.set()) {
        tracing::debug!(error = %e, "no interrupt handler; the job will not be cancellable");
    }
    flag
}

/// Run `on_signal` on the first Ctrl-C and leave the second to the default handler.
///
/// ⚠ `std` has no signal API and a crate for one callback is not worth its weight, so each family
/// of platform makes its own small hook: the console handler on Windows and `signal(2)` on a Unix.
#[cfg(windows)]
pub(crate) fn install_stop_handler(on_signal: impl Fn() + Send + 'static) -> Result<(), String> {
    use std::sync::Mutex;
    static HANDLER: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);
    static STOPPED: AtomicBool = AtomicBool::new(false);

    let mut slot = HANDLER.lock().map_err(|_| "handler lock poisoned")?;
    if slot.is_some() {
        return Err("an interrupt handler is already installed".to_string());
    }
    *slot = Some(Box::new(on_signal));
    drop(slot);

    unsafe extern "system" fn trampoline(_kind: u32) -> i32 {
        match HANDLER.lock() {
            Ok(slot) => console_event(slot.as_deref(), &STOPPED),
            Err(_) => 0,
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    // SAFETY: `trampoline` is a static `extern "system"` function of the
    // signature the API takes, so it outlives the registration.
    let ok = unsafe { SetConsoleCtrlHandler(Some(trampoline), 1) };
    if ok == 0 {
        return Err("SetConsoleCtrlHandler failed".to_string());
    }
    Ok(())
}

/// Whether a console event is handled: the first runs `on_signal`, and once
/// `stopped` is set every later one is left to the default handler, which
/// ends the process.
#[cfg(any(windows, test))]
fn console_event(on_signal: Option<&(dyn Fn() + Send)>, stopped: &AtomicBool) -> i32 {
    let Some(f) = on_signal else {
        return 0;
    };
    if stopped.swap(true, Ordering::SeqCst) {
        return 0;
    }
    f();
    1
}

/// ⚠ The signal handler only sets a flag, which is all a handler may safely do, and restores the
/// default so a second Ctrl-C ends a wedged job; a thread does the work of `on_signal`, which may
/// allocate or take a lock.
#[cfg(unix)]
pub(crate) fn install_stop_handler(on_signal: impl Fn() + Send + 'static) -> Result<(), String> {
    use std::sync::atomic::AtomicBool;

    static INSTALLED: AtomicBool = AtomicBool::new(false);
    static SIGNALLED: AtomicBool = AtomicBool::new(false);

    const SIGINT: i32 = 2;
    const SIG_DFL: usize = 0;
    const SIG_ERR: usize = usize::MAX;

    extern "C" {
        fn signal(signal: i32, handler: usize) -> usize;
    }

    extern "C" fn on_sigint(_: i32) {
        SIGNALLED.store(true, Ordering::SeqCst);
        // SAFETY: `signal` is async-signal-safe; the default handler is what a second Ctrl-C should reach.
        unsafe { signal(SIGINT, SIG_DFL) };
    }

    if INSTALLED.swap(true, Ordering::SeqCst) {
        return Err("an interrupt handler is already installed".to_string());
    }
    // SAFETY: `on_sigint` has the signature a handler has and touches only an atomic and `signal`.
    let previous = unsafe { signal(SIGINT, on_sigint as extern "C" fn(i32) as usize) };
    if previous == SIG_ERR {
        INSTALLED.store(false, Ordering::SeqCst);
        return Err("the system refused the interrupt handler".to_string());
    }
    std::thread::Builder::new()
        .name("interrupt".into())
        .spawn(move || {
            while !SIGNALLED.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(25));
            }
            on_signal();
        })
        .map(|_| ())
        .map_err(|e| format!("no thread for the interrupt handler: {e}"))
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn install_stop_handler(_on_signal: impl Fn() + Send + 'static) -> Result<(), String> {
    Err("no signal hook on this platform".to_string())
}

/// What a build is watched by in a terminal: Ctrl-C stops the build being written, and a line a second says how far it has got.
///
/// ⚠ A build stopped removes the folder it was writing and leaves the last good build, so the interrupt is safe to take.
pub struct BuildWatch {
    pub watch: or_project::Watch,
    finished: Arc<Flag>,
    printer: Option<std::thread::JoinHandle<()>>,
}

impl BuildWatch {
    pub fn start() -> Self {
        let watch = or_project::Watch {
            progress: Progress::new(),
            cancel: on_interrupt(),
        };
        let finished = Arc::new(Flag::new(false));
        let (seen, over) = (watch.progress.clone(), finished.clone());
        let printer = std::thread::Builder::new()
            .name("build progress".into())
            .spawn(move || {
                let began = Instant::now();
                let mut printed = false;
                while !over.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(200));
                    // ⚠ Nothing before the first second, so a small build prints nothing.
                    if began.elapsed() < Duration::from_secs(1) || over.load(Ordering::SeqCst) {
                        continue;
                    }
                    let now = seen.snapshot();
                    let mut err = std::io::stderr().lock();
                    let _ = write!(
                        err,
                        "\r  {:.0}% of the plan  {}          ",
                        now.fraction() * 100.0,
                        now.item
                    );
                    let _ = err.flush();
                    printed = true;
                }
                if printed {
                    let _ = writeln!(std::io::stderr());
                }
            })
            .ok();
        Self {
            watch,
            finished,
            printer,
        }
    }

    /// The build is over: the line is ended and the thread joined.
    pub fn finish(mut self) {
        self.finished.store(true, Ordering::SeqCst);
        if let Some(printer) = self.printer.take() {
            let _ = printer.join();
        }
    }
}

/// One line a second, rendered from whatever the job has counted so far.
pub struct Ticker {
    progress: Progress,
    last: Instant,
    printed: bool,
}

impl Ticker {
    pub fn new(progress: &Progress) -> Self {
        Self {
            progress: progress.clone(),
            last: Instant::now(),
            printed: false,
        }
    }

    /// Call once per item, from inside the job's own closure.
    pub fn tick(&mut self) {
        if self.last.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last = Instant::now();
        self.printed = true;
        let snap = self.progress.snapshot();
        let percent = snap.fraction() * 100.0;
        let mut err = std::io::stderr().lock();
        let _ = write!(
            err,
            "\r  {}/{} ({percent:.0}%)          ",
            snap.attempted, snap.total
        );
        let _ = err.flush();
    }

    pub fn finish(&mut self, report: &Report) {
        if !self.printed {
            return;
        }
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "\r  {} item(s) in {:.1}s          ",
            report.counts.attempted,
            report.elapsed.as_secs_f64()
        );
        let _ = err.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ A job wedged past its stop flag is killable only if the second
    /// Ctrl-C reaches the default handler.
    #[test]
    fn a_second_interrupt_is_left_to_the_default_handler() {
        let runs = std::sync::atomic::AtomicUsize::new(0);
        let count = || {
            runs.fetch_add(1, Ordering::SeqCst);
        };
        let stopped = AtomicBool::new(false);

        assert_eq!(
            console_event(Some(&count), &stopped),
            1,
            "the first is handled"
        );
        assert_eq!(
            console_event(Some(&count), &stopped),
            0,
            "the second was reported handled, so nothing can end a wedged job"
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1, "the stop runs once");
        assert_eq!(console_event(None, &AtomicBool::new(false)), 0);
    }

    /// ⚠ Progress must not print per item.
    #[test]
    fn the_ticker_is_quiet_until_a_second_has_passed() {
        let progress = Progress::new();
        let cancel = Cancel::new();
        let mut ticker = Ticker::new(&progress);

        let report = or_job::run(0..1000u32, &cancel, &progress, |_| {
            ticker.tick();
            or_job::Step::Done(1)
        });

        assert_eq!(report.counts.attempted, 1000, "every item is counted");
        assert!(
            !ticker.printed,
            "a thousand ticks inside one second printed something; the rate \
             limiter is not limiting"
        );
    }

    /// A real SIGINT, to this process, reaching the callback; the process is still here to say so.
    #[cfg(unix)]
    #[test]
    fn the_first_interrupt_runs_the_callback_and_a_second_installer_is_refused() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        extern "C" {
            fn raise(signal: i32) -> i32;
        }
        let ran = Arc::new(AtomicBool::new(false));
        let seen = ran.clone();
        install_stop_handler(move || seen.store(true, Ordering::SeqCst))
            .expect("nothing owns the handler yet");
        // SAFETY: `raise` takes a signal number and no pointer; SIGINT is 2 on every Unix.
        assert_eq!(unsafe { raise(2) }, 0);

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ran.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ran.load(Ordering::SeqCst),
            "the interrupt never reached the callback"
        );
        assert!(
            install_stop_handler(|| {}).is_err(),
            "a second handler took the first's place"
        );
    }
}
