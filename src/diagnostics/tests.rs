use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tracing::Level;

use super::crash::{self, CrashNotice, PanicFacts};
use super::log_buffer::{LogBuffer, MAX_MESSAGE_BYTES, ROUTINE_CAPACITY, SERIOUS_CAPACITY};
use crate::ui::panels::log::{LogView, Threshold};

/// The panic hook and the pending report are process-wide.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn view_of(buffer: &LogBuffer) -> LogView {
    let mut view = LogView::default();
    view.threshold = Threshold::Everything;
    view.refresh(buffer);
    view
}

fn lines(view: &LogView) -> Vec<String> {
    view.visible().iter().map(|r| r.line()).collect()
}

/// Runs `body` with the application's own subscriber as the thread default.
fn logged(body: impl FnOnce()) -> Arc<LogBuffer> {
    super::captured(body)
}

/// ⚠ Falsified by taking the buffer layer out of `super::subscriber`: every
/// assertion below then sees an empty panel.
#[test]
fn what_tracing_emits_reaches_the_log_panel() {
    let buffer = logged(|| {
        tracing::warn!(target: "or_title::dlc", "mount failed for {}", "update.rpf");
        tracing::info!(target: "or_io::gta5", "read 3 models");
        tracing::info!(target: "openrage::state", path = "a.ydr", "opened");
        tracing::debug!(target: "naga::front", "a validator's chatter");
    });
    let shown = lines(&view_of(&buffer));

    assert!(
        shown
            .iter()
            .any(|l| l.contains("WARN") && l.contains("mount failed for update.rpf")),
        "a warning from a workspace crate is missing: {shown:#?}"
    );
    assert!(
        shown.iter().any(|l| l.contains("or_io::gta5: read 3 models")),
        "the terminal filter admits only errors from `or_io`; the panel must take its info: {shown:#?}"
    );
    assert!(
        shown.iter().any(|l| l.contains("opened path=a.ydr")),
        "a structured field was dropped: {shown:#?}"
    );
    assert!(
        !shown.iter().any(|l| l.contains("chatter")),
        "debug from a third-party crate should stay out: {shown:#?}"
    );
    assert_eq!(shown.len(), 3);
}

/// ⚠ Falsified by counting every target: on a clean start the Vulkan loader's
/// own errors, logged through `wgpu_hal`, read as OpenRage's.
#[test]
fn another_crates_error_is_shown_but_not_counted() {
    let buffer = logged(|| {
        tracing::error!(target: "wgpu_hal::vulkan::instance", "GENERAL [Loader Message (0x0)]");
        tracing::warn!(target: "naga::valid", "a validator's warning");
        // What the `log` crate's bridge sends, as wgpu-hal's Vulkan loader lines arrive.
        tracing::error!(target: "log", "GENERAL [Loader Message (0x0)]");
    });
    assert_eq!(
        buffer.counts(),
        (0, 0),
        "the status bar counts OpenRage's own lines"
    );
    let shown = lines(&view_of(&buffer));
    assert!(
        shown
            .iter()
            .any(|l| l.contains("ERROR") && l.contains("wgpu_hal::vulkan::instance")),
        "the Log panel still shows it: {shown:#?}"
    );

    let buffer = logged(|| {
        tracing::error!(target: "openrage::state", "the save failed");
        tracing::warn!(target: "or_title::dlc", "mount failed");
    });
    assert_eq!(
        buffer.counts(),
        (1, 1),
        "(warnings, errors) of the app and an or_ crate"
    );
}

#[test]
fn a_toast_that_has_faded_is_still_in_the_log() {
    let buffer = logged(|| {
        let mut toasts = crate::state::Toasts::default();
        toasts.error("Could not save weapons.ytd: access denied");
        toasts.warning("2 textures could not be resolved");
        toasts.clear();
    });
    let view = view_of(&buffer);
    let shown = lines(&view);
    assert!(
        shown
            .iter()
            .any(|l| l.contains("ERROR")
                && l.contains("openrage::toast: Could not save weapons.ytd")),
        "{shown:#?}"
    );
    assert!(
        shown
            .iter()
            .any(|l| l.contains("WARN") && l.contains("2 textures")),
        "{shown:#?}"
    );
}

#[test]
fn the_view_filters_by_level_and_text_and_copies_what_it_shows() {
    let buffer = LogBuffer::default();
    buffer.push(Level::ERROR, "openrage::a", "the save failed".into());
    buffer.push(
        Level::WARN,
        "or_title::b",
        "an overlay entry is shadowed".into(),
    );
    buffer.push(Level::INFO, "openrage::c", "opened a file".into());

    let mut view = view_of(&buffer);
    view.threshold = Threshold::Warnings;
    assert_eq!(view.visible().len(), 2);

    view.text = "OVERLAY".into();
    let copied = view.copy_text();
    assert_eq!(copied.lines().count(), 1, "{copied}");
    assert!(copied.contains("or_title::b: an overlay entry is shadowed"));

    view.text = "or_title".into();
    assert_eq!(view.visible().len(), 1, "a filter matches the target too");
}

/// ⚠ Clear hides; the buffer a crash report reads keeps the entries.
#[test]
fn clearing_the_panel_does_not_empty_what_a_crash_report_copies() {
    let buffer = LogBuffer::default();
    buffer.push(Level::WARN, "openrage", "before the clear".into());
    let mut view = view_of(&buffer);
    view.clear();
    assert!(view.visible().is_empty());

    buffer.push(Level::WARN, "openrage", "after the clear".into());
    view.refresh(&buffer);
    assert_eq!(lines(&view).len(), 1);

    let recent = buffer.try_recent(10).expect("nobody holds the buffer");
    assert_eq!(recent.len(), 2);
}

#[test]
fn a_flood_of_routine_lines_cannot_push_out_a_warning() {
    let buffer = LogBuffer::default();
    buffer.push(Level::WARN, "openrage", "the one that matters".into());
    let flood = 3 * ROUTINE_CAPACITY;
    for i in 0..flood {
        buffer.push(Level::DEBUG, "openrage", format!("frame {i}"));
    }

    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.records.len(), 1 + ROUTINE_CAPACITY);
    assert_eq!(snapshot.dropped_routine, (flood - ROUTINE_CAPACITY) as u64);
    assert_eq!(snapshot.records[0].message, "the one that matters");
    assert!(
        snapshot.records.windows(2).all(|w| w[0].seq < w[1].seq),
        "the two rings merge back into arrival order"
    );

    let note = view_of(&buffer).bound_note();
    assert!(note.contains("1,000") && note.contains("4,000"), "{note}");
    assert!(note.contains("8,000 older entries were dropped"), "{note}");
}

#[test]
fn warnings_are_bounded_as_well() {
    let buffer = LogBuffer::default();
    for i in 0..SERIOUS_CAPACITY + 25 {
        buffer.push(Level::WARN, "openrage", format!("warning {i}"));
    }
    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.records.len(), SERIOUS_CAPACITY);
    assert_eq!(snapshot.dropped_serious, 25);
    assert_eq!(snapshot.warnings, (SERIOUS_CAPACITY + 25) as u64);
}

#[test]
fn one_enormous_message_is_cut_rather_than_kept() {
    let buffer = LogBuffer::default();
    buffer.push(Level::ERROR, "openrage", "é".repeat(MAX_MESSAGE_BYTES));
    let kept = &buffer.snapshot().records[0].message;
    assert!(kept.len() <= MAX_MESSAGE_BYTES + " …[cut]".len());
    assert!(kept.ends_with("[cut]"));
}

fn facts() -> PanicFacts {
    PanicFacts {
        message: "boom".into(),
        location: "src/x.rs:1:1".into(),
        thread: "main".into(),
        backtrace: "disabled".into(),
        unix_time: Duration::ZERO,
    }
}

#[test]
fn a_report_says_why_it_has_no_log_rather_than_showing_none() {
    let empty = crash::compose(&facts(), Some(&[]), None);
    assert!(empty.contains("(nothing was logged before the panic)"));
    assert!(empty.contains("Edition:  none chosen yet"));

    let held = crash::compose(&facts(), None, Some("Enhanced"));
    assert!(held.contains("(unavailable:"));
    assert!(held.contains("Edition:  Enhanced"));
}

/// ⚠ The report names an edition the way a user does.
#[test]
fn a_report_never_says_gen8_or_gen9() {
    for game in or_title::Game::all() {
        let report = crash::compose(&facts(), Some(&[]), Some(game.generation().edition_label()));
        assert!(
            !report.contains("Gen8") && !report.contains("Gen9"),
            "{report}"
        );
    }
}

#[test]
fn utc_stamps_follow_the_calendar() {
    const DAY: u64 = 86_400;
    assert_eq!(crash::utc_stamp(Duration::ZERO), "1970-01-01 00:00:00");
    assert_eq!(
        crash::utc_stamp(Duration::from_secs(365 * DAY + 3_661)),
        "1971-01-01 01:01:01"
    );
    let leap_years_1970_to_1999 = (1970..2000)
        .filter(|y| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0))
        .count() as u64;
    let y2000 = (30 * 365 + leap_years_1970_to_1999) * DAY;
    assert_eq!(
        crash::utc_stamp(Duration::from_secs(y2000 + (31 + 28) * DAY)),
        "2000-02-29 00:00:00"
    );
    assert_eq!(
        crash::utc_stamp(Duration::from_secs(y2000 + (31 + 29) * DAY)),
        "2000-03-01 00:00:00"
    );
}

/// ⚠ Behaviour, not drawing: the window's Close is `dismiss`, and a report
/// dismissed must not come back on the next start.
#[test]
fn a_report_from_last_session_shows_once_and_close_retires_it() {
    let _serial = serial();
    let dir = crash::reports_dir().expect("the suite's scratch root");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let name = "crash-19700101-000000-1-0.txt";
    std::fs::write(dir.join(name), "OpenRage crash report\nPanic: earlier\n").expect("write");
    std::fs::write(dir.join("unread.txt"), name).expect("marker");

    crash::dismiss();
    std::fs::write(dir.join("unread.txt"), name).expect("marker again");
    crash::restore_unread();
    let shown: Option<CrashNotice> = crash::pending();
    let shown = shown.expect("an unread report surfaces on start");
    assert!(shown.from_last_session);
    assert!(shown.report.contains("Panic: earlier"));

    crash::dismiss();
    assert!(crash::pending().is_none());
    assert!(
        crash::load_unread(&dir).is_none(),
        "the next start would show it again"
    );
    assert!(
        dir.join(name).is_file(),
        "closing the window must not delete the report"
    );
}

/// ⚠ Falsified by filing a line under its event's own target: the `log`
/// crate's bridge names every line it forwards `log`, and keeps the real one
/// in a field.
#[test]
fn a_line_written_through_the_log_crate_is_filed_under_its_own_target() {
    let buffer = logged(|| {
        // What the bridge sends for a line wgpu-hal's Vulkan loader writes through `log`.
        tracing::event!(
            target: "log",
            Level::WARN,
            {
                "log.target" = "wgpu_hal::vulkan::instance",
                "log.module_path" = "wgpu_hal::vulkan::instance",
                "log.file" = "src/vulkan/instance.rs",
                "log.line" = 42u32
            },
            "GENERAL [Loader Message (0x0)]"
        );
    });
    let shown = lines(&view_of(&buffer));
    assert!(
        shown
            .iter()
            .any(|l| l.ends_with("wgpu_hal::vulkan::instance: GENERAL [Loader Message (0x0)]")),
        "{shown:#?}"
    );
    assert_eq!(
        buffer.counts(),
        (0, 0),
        "another crate's line is not counted"
    );
}
