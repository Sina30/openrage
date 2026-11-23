//! What a person most often does outside a project, each done start to finish through the names on screen: an install opened and browsed, a file of
//! each kind opened, edited and saved, the views of a file, and the settings. Every flow leaves a record of its clicks, its keys, the toasts it raised
//! and what the log warned of, in `openrage-flows` under the workspace's build folder, one file a flow.
//!
//! ⚠ No flow writes into an install: a save goes to a scratch folder through Save as…, and a flow that needs a writable game folder makes its own.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use super::frames_of_the_shell::{Described, Shell};
use crate::diagnostics::log_buffer::{BufferLayer, LogBuffer};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

mod editing;
mod formats;
mod installs;
mod settings;
mod views;

pub(super) type Step = Result<(), String>;

/// What a flow that cannot run here says first, so that it is recorded as skipped and not as failed.
const SKIP: &str = "skip: ";

fn skip(why: impl std::fmt::Display) -> String {
    format!("{SKIP}{why}")
}

/// File ▸ Save as its row is named, with the key this machine binds it to.
const SAVE: &str = if cfg!(target_os = "macos") {
    "Save Cmd+S"
} else {
    "Save Ctrl+S"
};

/// The frames after a click: more than egui's double-click time, so the next click on the same place is a click of its own.
const AFTER_A_CLICK: usize = 12;

fn log() -> &'static Arc<LogBuffer> {
    static LOG: OnceLock<Arc<LogBuffer>> = OnceLock::new();
    LOG.get_or_init(|| {
        use tracing_subscriber::layer::SubscriberExt;
        let buffer = Arc::new(LogBuffer::default());
        let subscriber = tracing_subscriber::registry().with(BufferLayer::new(Arc::clone(&buffer)));
        // Refused when another test of the process set one first: its flows then record no log.
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    })
}

/// Whether `c` is a glyph of the icon font, which a row's name starts with.
fn is_glyph(c: char) -> bool {
    (0xE000..=0xF8FF).contains(&(c as u32))
}

/// A person at the shell, and the record of what they did.
pub(super) struct Flow {
    pub(super) shell: Shell,
    name: &'static str,
    clicks: u32,
    keys: u32,
    typed: u32,
    lines: Vec<String>,
    notes: Vec<String>,
    toasts: Vec<String>,
    in_settings: bool,
    /// What the last look saw, so that the next one records only what is new.
    looked: Vec<String>,
    started: Instant,
    /// Scratch files of the flow, gone when it ends.
    pub(super) scratch: tempfile::TempDir,
}

impl Flow {
    /// A first launch: nothing open, the start page on show.
    fn first_launch(name: &'static str) -> Self {
        let mut shell = Shell::new(ThemeChoice::Dark);
        shell.state.documents.clear();
        shell.state.active_document = None;
        shell.state.active_workspace = Workspace::Browse;
        let mut flow = Self {
            shell,
            name,
            clicks: 0,
            keys: 0,
            typed: 0,
            lines: Vec::new(),
            notes: Vec::new(),
            toasts: Vec::new(),
            in_settings: false,
            looked: Vec::new(),
            started: Instant::now(),
            scratch: tempfile::tempdir().expect("a scratch folder"),
        };
        flow.settle();
        flow
    }

    /// One frame as the editor's redraw makes it: the workers' results taken, then the window in front drawn.
    fn frame(&mut self, events: Vec<egui::Event>) {
        let state = &mut self.shell.state;
        state.poll_load_result();
        state.poll_save_result();
        state.drive_close(crate::ui::panels::menu::save_dialog_for);
        state.poll_key_extraction();
        state.poll_material_palette_load();
        state.poll_clip_regions_load();
        state.poll_shader_definitions();
        state.poll_game_discovery();
        state.poll_expansions();
        state.tick_navigation();
        state.poll_extract_job();
        state.poll_jobs();
        state.update_toasts();
        if self.in_settings {
            self.shell.draw_settings(events);
        } else {
            self.shell.draw(events);
        }
        for toast in self.shell.state.toasts.iter() {
            let said = format!("{:?}: {}", toast.toast_type, toast.message);
            if !self.toasts.contains(&said) {
                self.lines.push(format!("  toast {said}"));
                self.toasts.push(said);
            }
        }
        self.in_settings = crate::ui::panels::settings::is_settings_open(&self.shell.state);
    }

    pub(super) fn settle(&mut self) {
        for _ in 0..AFTER_A_CLICK {
            self.frame(Vec::new());
        }
    }

    /// Frames until `done`, for the three minutes a large archive can take on a busy machine.
    pub(super) fn wait(&mut self, what: &str, done: impl Fn(&Flow) -> bool) -> Step {
        self.wait_within(Duration::from_secs(180), what, done)
    }

    fn wait_within(&mut self, long: Duration, what: &str, done: impl Fn(&Flow) -> bool) -> Step {
        let began = Instant::now();
        let until = began + long;
        while !done(self) {
            if Instant::now() > until {
                return Err(format!("{what} never happened\n{}", self.screen()));
            }
            self.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(5));
        }
        self.settle();
        let waited = began.elapsed();
        if waited > Duration::from_secs(2) {
            self.lines
                .push(format!("  waited {:.1} s for {what}", waited.as_secs_f32()));
        }
        Ok(())
    }

    pub(super) fn wait_for(&mut self, text: &str) -> Step {
        self.wait(&format!("`{text}` on screen"), |flow| flow.shows(text))
    }

    /// Every name on screen, top to bottom.
    pub(super) fn names(&self) -> Vec<String> {
        let mut seen: Vec<&Described> = (self.shell.described.iter())
            .filter(|w| w.name.as_deref().is_some_and(|n| !n.trim().is_empty()))
            .collect();
        seen.sort_by(|a, b| {
            (a.bounds.top(), a.bounds.left())
                .partial_cmp(&(b.bounds.top(), b.bounds.left()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        seen.iter().map(|w| w.name.clone().unwrap()).collect()
    }

    fn screen(&self) -> String {
        format!("on screen: {:?}", self.names())
    }

    /// Write what is on screen and was not at the last look into the record.
    pub(super) fn look(&mut self, at: &str) {
        let names = self.names();
        let new: Vec<&String> = names.iter().filter(|n| !self.looked.contains(n)).collect();
        self.lines
            .push(format!("  look ({at}) new on screen: {new:?}"));
        self.looked = names;
    }

    /// Write the controls on screen that are neither a label nor a button, with what each is.
    pub(super) fn look_at_controls(&mut self, at: &str) {
        use egui::accesskit::Role;
        let controls: Vec<String> = (self.shell.described.iter())
            .filter(|w| {
                !matches!(
                    w.role,
                    Role::Label | Role::Button | Role::GenericContainer | Role::Unknown
                )
            })
            .map(|w| format!("{:?} {:?}", w.role, w.name.clone().unwrap_or_default()))
            .collect();
        self.lines.push(format!("  controls ({at}) {controls:?}"));
    }

    pub(super) fn shows(&self, text: &str) -> bool {
        (self.shell.described.iter()).any(|w| w.name.as_deref().is_some_and(|n| n.contains(text)))
    }

    pub(super) fn expect(&mut self, text: &str) -> Step {
        if self.shows(text) {
            return Ok(());
        }
        Err(format!("`{text}` is not on screen\n{}", self.screen()))
    }

    /// A step the flow takes that is not a click or a key.
    pub(super) fn say(&mut self, what: impl Into<String>) {
        self.lines.push(format!("  {}", what.into()));
    }

    /// Something the flow noticed that a person would trip on.
    pub(super) fn note(&mut self, what: impl Into<String>) {
        let what = what.into();
        self.lines.push(format!("  NOTE {what}"));
        self.notes.push(what);
    }

    /// The widget named `name`: exactly, else ending in it after a space or a glyph (a button's or a row's icon comes first), else holding it.
    pub(super) fn find(&self, name: &str) -> Result<Described, String> {
        let named = |test: &dyn Fn(&str) -> bool| {
            // A row scrolled out of view is described with no place: nothing there can be clicked.
            let mut hits: Vec<&Described> = (self.shell.described.iter())
                .filter(|w| w.bounds.is_finite() && w.bounds.is_positive())
                .filter(|w| w.name.as_deref().is_some_and(test))
                .collect();
            // An enabled one first: a menu and a toolbar may both name a command, one of them greyed. Then a control before the label beside it, which a field shares its name with.
            hits.sort_by_key(|w| (w.disabled, w.role == egui::accesskit::Role::Label));
            hits.first().map(|w| (*w).clone())
        };
        named(&|n| n == name)
            .or_else(|| named(&|n| n.ends_with(&format!(" {name}"))))
            .or_else(|| named(&|n| n.trim_start_matches(is_glyph) == name))
            .or_else(|| named(&|n| n.contains(name)))
            .ok_or_else(|| format!("nothing is named `{name}`\n{}", self.screen()))
    }

    /// The row of a list named `name` after its glyph, which a crumb or a heading of the same name is not.
    pub(super) fn find_row(&self, name: &str) -> Result<Described, String> {
        (self.shell.described.iter())
            .filter(|w| w.bounds.is_finite() && w.bounds.is_positive())
            .find(|w| {
                (w.name.as_deref()).is_some_and(|n| {
                    n.starts_with(is_glyph) && n.trim_start_matches(is_glyph) == name
                })
            })
            .cloned()
            .ok_or_else(|| format!("no row is named `{name}`\n{}", self.screen()))
    }

    pub(super) fn double_click_row(&mut self, name: &str) -> Step {
        self.act("double-click", self.find_row(name), name)
    }

    pub(super) fn right_click_row(&mut self, name: &str) -> Step {
        self.act("right-click", self.find_row(name), name)
    }

    /// The widget named `name` on the row of the one named `beside`, or the nearest under it: a card's buttons are under its title, never over it.
    pub(super) fn find_beside(&self, name: &str, beside: &str) -> Result<Described, String> {
        let anchor = self.find(beside)?.bounds;
        let anchor = anchor.center().y;
        (self.shell.described.iter())
            .filter(|w| w.bounds.is_finite() && w.name.as_deref() == Some(name))
            .filter(|w| w.bounds.bottom() >= anchor)
            .min_by(|a, b| {
                let far = |w: &Described| (w.bounds.center().y - anchor).abs();
                far(a).total_cmp(&far(b))
            })
            .cloned()
            .ok_or_else(|| format!("no `{name}` beside `{beside}`\n{}", self.screen()))
    }

    pub(super) fn press(&mut self, at: egui::Pos2, button: egui::PointerButton, times: usize) {
        let held = self.shell.modifiers;
        let event = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: held,
        };
        self.frame(vec![egui::Event::PointerMoved(at)]);
        for _ in 0..times {
            self.frame(vec![event(true)]);
            self.frame(vec![event(false)]);
        }
        self.clicks += 1;
        self.settle();
    }

    fn act(&mut self, verb: &str, widget: Result<Described, String>, name: &str) -> Step {
        let widget = widget?;
        if widget.disabled {
            return Err(format!("`{name}` is greyed\n{}", self.screen()));
        }
        self.lines.push(format!(
            "{verb} `{name}` ({:?} at {:?})",
            widget.role,
            widget.bounds.center()
        ));
        let (button, times) = match verb {
            "right-click" => (egui::PointerButton::Secondary, 1),
            "double-click" => (egui::PointerButton::Primary, 2),
            _ => (egui::PointerButton::Primary, 1),
        };
        self.press(widget.bounds.center(), button, times);
        Ok(())
    }

    pub(super) fn click(&mut self, name: &str) -> Step {
        self.act("click", self.find(name), name)
    }

    pub(super) fn click_beside(&mut self, name: &str, beside: &str) -> Step {
        self.act("click", self.find_beside(name, beside), name)
    }

    pub(super) fn double_click(&mut self, name: &str) -> Step {
        self.act("double-click", self.find(name), name)
    }

    pub(super) fn right_click(&mut self, name: &str) -> Step {
        self.act("right-click", self.find(name), name)
    }

    /// A command of a menu of the bar: the menu's title clicked, then the item.
    pub(super) fn menu(&mut self, title: &str, item: &str) -> Step {
        self.click(title)?;
        self.click(item)
    }

    pub(super) fn type_text(&mut self, text: &str) {
        self.lines.push(format!("type `{text}`"));
        self.typed += text.chars().count() as u32;
        self.frame(vec![egui::Event::Text(text.to_string())]);
        self.settle();
    }

    pub(super) fn key(&mut self, key: egui::Key) {
        self.lines.push(format!("press {key:?}"));
        self.keys += 1;
        self.frame(vec![super::frames_of_the_shell::key(key)]);
        self.settle();
    }

    /// Have the next file dialog pick `path`, as the person would in the system's own window.
    pub(super) fn pick(&mut self, path: &Path) {
        self.lines.push(format!("pick {}", path.display()));
        crate::platform::file_dialogs::answer_next(path.to_path_buf());
    }

    /// File ▸ Open file… with `path` picked, until its tab is in front.
    pub(super) fn open_file(&mut self, path: &Path) -> Step {
        self.pick(path);
        self.menu("File", "Open file…")?;
        let wanted = path.to_path_buf();
        self.wait(&format!("{} open", path.display()), move |flow| {
            let state = &flow.shell.state;
            state.show_error_dialog
                || (state.active_document()).is_some_and(|d| d.path.as_ref() == Some(&wanted))
        })?;
        if self.shell.state.show_error_dialog {
            let said = self.shell.state.error_dialog_message.clone();
            return Err(format!("{} did not open: {said}", path.display()));
        }
        Ok(())
    }

    /// The window's own close button of the Settings window.
    pub(super) fn close_settings(&mut self) {
        self.lines.push("close the Settings window".into());
        self.clicks += 1;
        crate::ui::panels::settings::set_settings_open(&mut self.shell.state, false);
        self.settle();
    }

    pub(super) fn active_name(&self) -> String {
        (self.shell.state.active_document()).map_or(String::new(), |d| d.file_name())
    }

    /// The search of this machine for installs over: a launch starts it and it ends within a few seconds.
    pub(super) fn wait_for_the_installs(&mut self) {
        let _ = self.wait_within(Duration::from_secs(20), "the search for installs", |flow| {
            !flow.shell.state.discovery.is_running()
        });
    }

    /// The card of `game` on the start page opened, until its rows are read.
    pub(super) fn open_install(&mut self, game: &str) -> Step {
        // ⚠ A game's card is drawn whether or not its install is found, so the card's name says nothing.
        self.wait_for_the_installs();
        let found = crate::state::Game::all().any(|g| {
            g.label() == game
                && crate::ui::panels::game_picker::found_install(&self.shell.state, g).is_some()
        });
        if !found {
            return Err(skip(format!("no {game} install is found on this machine")));
        }
        self.click_beside("Open", game)?;
        self.wait("the install's rows", |flow| {
            let state = &flow.shell.state;
            (state.active_document()).is_some_and(|doc| !state.expander.is_listing(doc.id))
        })
    }

    /// Wait as a person does not know to: an archive opened before the keys are read fails.
    pub(super) fn wait_for_the_keys(&mut self) -> Step {
        self.wait("the keys", |flow| flow.shell.state.keys.has_keys())
    }

    /// A path typed into the centre's path field, and Enter.
    pub(super) fn go_to(&mut self, path: &str) -> Step {
        self.click("Type a path")?;
        self.type_text(path);
        self.key(egui::Key::Enter);
        let last = path.rsplit('/').next().unwrap_or(path).to_string();
        self.wait_for(&last)
    }

    /// File ▸ Save as… with `to` picked; whether the command could be run at all.
    pub(super) fn save_as(&mut self, to: &Path) -> Result<bool, String> {
        self.click("File")?;
        if self.find("Save as…")?.disabled {
            self.key(egui::Key::Escape);
            return Ok(false);
        }
        self.pick(to);
        self.click("Save as…")?;
        let (file, before) = (to.to_path_buf(), self.toasts.len());
        self.wait(&format!("{} written", to.display()), move |flow| {
            std::fs::metadata(&file).is_ok_and(|m| m.len() > 0)
                || flow.shell.state.show_error_dialog
                || flow.toasts.len() > before
        })?;
        if !to.exists() {
            let said = self.toasts.last().cloned().unwrap_or_default();
            return Err(format!("Save as… wrote nothing: {said}\n{}", self.screen()));
        }
        Ok(true)
    }

    pub(super) fn close_document(&mut self) -> Step {
        self.menu("File", "Close document")
    }

    fn finish(mut self, result: Step) {
        let serious: Vec<String> = (log().snapshot().records.iter())
            .filter(|record| record.is_serious())
            .map(|record| record.line())
            .collect();
        let outcome = match &result {
            Ok(()) => "ok".to_string(),
            Err(why) if why.starts_with(SKIP) => why.clone(),
            Err(why) => format!("FAILED: {why}"),
        };
        let mut record = format!(
            "flow\t{}\noutcome\t{}\nclicks\t{}\nkeys\t{}\ntyped\t{}\nseconds\t{:.1}\ntoasts\t{}\nlog\t{}\nnotes\t{}\n\n",
            self.name,
            outcome.lines().next().unwrap_or_default(),
            self.clicks,
            self.keys,
            self.typed,
            self.started.elapsed().as_secs_f32(),
            self.toasts.len(),
            serious.len(),
            self.notes.len(),
        );
        for line in self.lines.drain(..) {
            record.push_str(&line);
            record.push('\n');
        }
        for line in &serious {
            record.push_str(&format!("LOG {line}\n"));
        }
        if let Err(why) = &result {
            record.push_str(&format!("\n{why}\n"));
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/openrage-flows");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{}.txt", self.name)), record);
        match result {
            Err(why) if why.starts_with(SKIP) => {
                or_test_support::skip_install(self.name, why.trim_start_matches(SKIP));
            }
            Err(why) => panic!("{}: {}", self.name, why.lines().next().unwrap_or_default()),
            Ok(()) => {}
        }
    }
}

/// Run `flow` from a first launch and keep its record.
pub(super) fn run(name: &'static str, flow: impl FnOnce(&mut Flow) -> Step) {
    let _serial = crate::ui::theme::serial();
    log();
    let mut at = Flow::first_launch(name);
    let result = flow(&mut at);
    at.finish(result);
}

/// The first file of the shared corpus with `extension`, or the skip that says there is none.
pub(super) fn corpus(extension: &str) -> Result<PathBuf, String> {
    use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    (Corpus::with_root(&root, extension)
        .target(TARGET_GEN8)
        .find()
        .into_iter())
    .next()
    .ok_or_else(|| {
        skip(format!(
            "no .{extension} in the corpus at {}",
            root.display()
        ))
    })
}

/// The first file with `extension` among the metadata corpus, which is staged beside the shared one.
pub(super) fn meta_corpus(extension: &str) -> Result<PathBuf, String> {
    use or_test_support::{shared_corpus_root, Corpus};
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let meta = root.parent().map(|p| p.join("or-meta")).unwrap_or_default();
    (Corpus::with_root(&meta, extension)
        .all_targets()
        .find()
        .into_iter())
    .next()
    .ok_or_else(|| {
        skip(format!(
            "no .{extension} in the corpus at {}",
            meta.display()
        ))
    })
}
