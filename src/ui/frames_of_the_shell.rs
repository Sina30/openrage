//! Frames of the whole shell with no window, for the tests that read what it paints.

use egui::epaint::ClippedShape;

use or_transport::TimeRange;

use crate::state::{AppState, AppearancePrefs, DocPayload, Document, Prefs, ThemeChoice};
use crate::ui::theme;
use crate::ui::timeline::{
    ChannelRow, CurveChannel, CurveDomain, CurveHandle, CurveSeries, KeyEvent, KeyShape, RowBadge,
    RowEvents, RowId, RowKind, SpanEvent, TimelineSource, TimelineState,
};

/// The editor's window and the Settings window, in points.
const EDITOR: (u32, u32) = crate::app::EDITOR_SIZE;
const SETTINGS: (u32, u32) = crate::app::SETTINGS_WINDOW_SIZE;

pub(super) fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A widget AccessKit described in a frame.
#[derive(Debug, Clone)]
pub(super) struct Described {
    pub(super) role: egui::accesskit::Role,
    /// What a screen reader would call it, when it is called anything.
    pub(super) name: Option<String>,
    pub(super) bounds: egui::Rect,
    pub(super) disabled: bool,
}

impl Described {
    fn of(node: &egui::accesskit::Node) -> Option<Self> {
        use egui::accesskit::Role;
        let name = match node.role() {
            // A text field is named by its label or its hint, never by what is typed in it.
            Role::TextInput => node.label().or_else(|| node.placeholder()),
            Role::Label => node.value().or_else(|| node.label()),
            _ => node.label(),
        };
        let b = node.bounds()?;
        Some(Self {
            role: node.role(),
            name: name.map(str::to_string),
            bounds: egui::Rect::from_min_max(
                egui::pos2(b.x0 as f32, b.y0 as f32),
                egui::pos2(b.x1 as f32, b.y1 as f32),
            ),
            disabled: node.is_disabled(),
        })
    }
}

/// The real app state, drawn through the context the editor draws with.
pub(super) struct Shell {
    pub(super) ctx: egui::Context,
    pub(super) state: AppState,
    time: f64,
    /// What AccessKit called the widget with the keyboard in the last frame, when it called it anything.
    focus_label: Option<String>,
    /// Every widget AccessKit described in the last frame.
    pub(super) described: Vec<Described>,
    /// Files the next frame receives as dropped from outside the window.
    dropping: Vec<std::path::PathBuf>,
    /// Files held over the window by a drag that has not let go, in every frame until `drop_files`.
    hovering: Vec<std::path::PathBuf>,
    /// The modifier keys held in every frame, as a person holds Shift through a click.
    pub(super) modifiers: egui::Modifiers,
    /// The text every frame so far asked the clipboard to hold, oldest first.
    pub(super) copied: Vec<String>,
    /// The toasts a frame has drawn, by the name of their area, so that the frame in which one first shows is known.
    drawn_toasts: std::collections::HashSet<(std::time::Instant, String)>,
    /// Where this test's preferences are written, so that no other test's are touched.
    _preferences: tempfile::TempDir,
}

impl Shell {
    /// With a text document open, whose markup the editor colours, and a screen reader attached: the window has been asked for its tree.
    pub(super) fn new(choice: ThemeChoice) -> Self {
        let shell = Self::unattended(choice);
        let asked = egui_winit::accesskit_winit::WindowEvent::InitialTreeRequested;
        crate::app::accessibility::on_event(&shell.ctx, asked);
        shell
    }

    /// As [`Shell::new`] with no screen reader, so that the window is asked for no tree.
    pub(super) fn unattended(choice: ThemeChoice) -> Self {
        let preferences = tempfile::tempdir().expect("a temporary folder");
        crate::state::prefs::keep_prefs_in(preferences.path());
        let appearance = AppearancePrefs {
            theme: choice,
            ..AppearancePrefs::default()
        };
        let ctx = crate::app::gpu::configured_egui_context(None, &appearance);
        let mut state = AppState::from_prefs(Prefs::default());
        let source = b"<?xml version=\"1.0\"?>\n<!-- a comment -->\n<Item name=\"adder\" price=\"1000\">\n  <Flags>Fast, Quiet</Flags>\n</Item>\n";
        let payload = DocPayload::Text(crate::state::TextDocument::decode(source));
        state.documents.clear();
        state
            .documents
            .push(Document::new(1, Some("probe.xml".into()), payload));
        state.set_active_document(1);
        Self {
            ctx,
            state,
            time: 0.0,
            focus_label: None,
            described: Vec::new(),
            dropping: Vec::new(),
            hovering: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            copied: Vec::new(),
            drawn_toasts: Default::default(),
            _preferences: preferences,
        }
    }

    /// A frame of the window, and a second at once when a toast shows in it for the first time.
    fn pass(
        &mut self,
        size: (u32, u32),
        events: Vec<egui::Event>,
        mut draw: impl FnMut(&egui::Context, &mut AppState),
    ) -> Vec<ClippedShape> {
        let shapes = self.pass_once(size, events, &mut draw);
        // ⚠ A click lands on what the pass before it registered, and a toast's first pass is a sizing pass whose area senses clicks over a 600 by 400 box. The window draws again as soon as that pass asks for it; a test that drew once left the box under the next click.
        let mut first = false;
        for toast in self.state.toasts.iter() {
            first |= (self.drawn_toasts).insert((toast.created_at, toast.message.clone()));
        }
        if first {
            return self.pass_once(size, Vec::new(), &mut draw);
        }
        shapes
    }

    fn pass_once(
        &mut self,
        size: (u32, u32),
        events: Vec<egui::Event>,
        draw: &mut impl FnMut(&egui::Context, &mut AppState),
    ) -> Vec<ClippedShape> {
        self.time += 1.0 / 30.0;
        self.state.toasts.keep_fresh();
        self.ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.0 as f32, size.1 as f32),
            )),
            time: Some(self.time),
            modifiers: self.modifiers,
            events,
            hovered_files: (self.hovering.iter())
                .map(|path| egui::HoveredFile {
                    path: Some(path.clone()),
                    mime: String::new(),
                })
                .collect(),
            dropped_files: std::mem::take(&mut self.dropping)
                .into_iter()
                .map(|path| egui::DroppedFile {
                    path: Some(path),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        draw(&self.ctx, &mut self.state);
        let output = self.ctx.end_pass();
        for command in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                self.copied.push(text.clone());
            }
        }
        let update = output.platform_output.accesskit_update;
        self.described = (update.iter())
            .flat_map(|update| update.nodes.iter())
            .filter_map(|(_, node)| Described::of(node))
            .collect();
        self.focus_label = update.and_then(|update| {
            let (_, node) = update.nodes.iter().find(|(id, _)| *id == update.focus)?;
            Described::of(node)?.name
        });
        output.shapes
    }

    /// The widget with the keyboard after the last frame: the part of it that shows, and what it is called.
    pub(super) fn focused(&self) -> Option<(egui::Rect, String)> {
        let id = self.ctx.memory(|m| m.focused())?;
        let rect = self.ctx.read_response(id)?.interact_rect;
        Some((rect, self.focus_label.clone().unwrap_or_default()))
    }

    /// A frame in which `paths` are dropped from outside the window with the pointer at `at`, after one that moved it there.
    pub(super) fn drop_files(&mut self, at: egui::Pos2, paths: Vec<std::path::PathBuf>) {
        self.draw(vec![egui::Event::PointerMoved(at)]);
        self.hovering.clear();
        self.dropping = paths;
        self.draw(Vec::new());
    }

    /// A frame in which `paths` are held over the window by a drag with the pointer at `at`, and its shapes; `drop_files` or `hover_end` ends the hold.
    pub(super) fn hover_files(
        &mut self,
        at: egui::Pos2,
        paths: Vec<std::path::PathBuf>,
    ) -> Vec<ClippedShape> {
        self.hovering = paths;
        self.draw(vec![egui::Event::PointerMoved(at)])
    }

    /// As [`Shell::hover_files`] in a window of `size` points.
    pub(super) fn hover_files_sized(
        &mut self,
        size: (u32, u32),
        at: egui::Pos2,
        paths: Vec<std::path::PathBuf>,
    ) -> Vec<ClippedShape> {
        self.hovering = paths;
        self.draw_sized(size, vec![egui::Event::PointerMoved(at)])
    }

    pub(super) fn hover_end(&mut self) {
        self.hovering.clear();
    }

    /// One frame of the editor's window: the shell, and what the window draws over it.
    pub(super) fn draw(&mut self, events: Vec<egui::Event>) -> Vec<ClippedShape> {
        self.draw_sized(EDITOR, events)
    }

    /// As [`Shell::draw`] in a window of `size` points.
    pub(super) fn draw_sized(
        &mut self,
        size: (u32, u32),
        events: Vec<egui::Event>,
    ) -> Vec<ClippedShape> {
        self.pass(size, events, |ctx, state| {
            crate::ui::draw(ctx, state, None);
            crate::app::modals::draw_editor_overlays(ctx, state);
        })
    }

    /// Frames of the editor's window until what it lays out has settled, and the last one's shapes.
    pub(super) fn settled(&mut self) -> Vec<ClippedShape> {
        self.settled_sized(EDITOR)
    }

    /// As [`Shell::settled`] in a window of `size` points.
    pub(super) fn settled_sized(&mut self, size: (u32, u32)) -> Vec<ClippedShape> {
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = self.draw_sized(size, Vec::new());
        }
        shapes
    }

    /// Frames of `add` alone in the editor's window, on the panel colour, and the last one's shapes.
    pub(super) fn scratch(&mut self, add: impl Fn(&mut egui::Ui)) -> Vec<ClippedShape> {
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = self.pass(EDITOR, Vec::new(), |ctx, _| {
                crate::ui::root::with_root_ui(ctx, |ui| add(ui));
            });
        }
        shapes
    }

    /// Frames of a clip's timeline filling the editor's window, and the last one's shapes; the Animation workspace draws rows only once a clip is open.
    pub(super) fn a_clips_timeline(&mut self) -> Vec<ClippedShape> {
        self.timeline(false)
    }

    /// As [`Shell::a_clips_timeline`] for a clip that takes edits, whose curve draws its handles.
    pub(super) fn an_editable_clips_timeline(&mut self) -> Vec<ClippedShape> {
        self.timeline(true)
    }

    fn timeline(&mut self, editable: bool) -> Vec<ClippedShape> {
        let (clip, mut timeline) = clip_timeline(editable);
        let mut transport = or_transport::Transport::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = self.pass(EDITOR, Vec::new(), |ctx, _| {
                crate::ui::root::with_root_ui(ctx, |ui| {
                    crate::ui::timeline::draw(ui, &clip, &mut transport, &mut timeline);
                });
            });
        }
        shapes
    }

    /// Frames of the Settings window on `category`'s page, and the last one's shapes.
    pub(super) fn settings(
        &mut self,
        category: crate::ui::panels::settings::SettingsCategory,
    ) -> Vec<ClippedShape> {
        crate::ui::panels::settings::reset_settings_transient(&self.ctx);
        self.state
            .panels
            .get::<crate::ui::panels::settings::SettingsUiState>()
            .category = category;
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = self.pass(SETTINGS, Vec::new(), |ctx, state| {
                crate::ui::panels::settings::draw_settings_window_body(ctx, state)
            });
        }
        shapes
    }

    /// One frame of the Settings window as it stands, with `events`.
    pub(super) fn draw_settings(&mut self, events: Vec<egui::Event>) -> Vec<ClippedShape> {
        self.pass(SETTINGS, events, |ctx, state| {
            crate::ui::panels::settings::draw_settings_window_body(ctx, state)
        })
    }

    /// A click at `at`, as the press and the release.
    pub(super) fn click(&mut self, at: egui::Pos2) {
        let held = self.modifiers;
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: held,
        };
        self.draw(vec![egui::Event::PointerMoved(at), button(true)]);
        self.draw(vec![button(false)]);
    }

    /// A folder of a few files open in Browse, its rows read; the folder goes when the guard does.
    pub(super) fn open_a_folder(&mut self) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary folder");
        for (name, bytes) in [
            ("adder.xml", &b"<Item/>"[..]),
            ("notes.txt", &b"hello"[..]),
            ("blob.bin", &[0u8; 64][..]),
        ] {
            std::fs::write(dir.path().join(name), bytes).expect("a file in the folder");
        }
        std::fs::create_dir(dir.path().join("folder")).expect("a folder in the folder");
        self.state.active_workspace = crate::ui::dock::Workspace::Browse;
        let ctx = self.ctx.clone();
        let mut time = 100.0;
        crate::self_check::shots::open_and_wait(&mut self.state, dir.path(), |state| {
            time += 1.0 / 30.0;
            state.poll_load_result();
            state.poll_expansions();
            state.poll_jobs();
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(EDITOR.0 as f32, EDITOR.1 as f32),
                )),
                time: Some(time),
                ..Default::default()
            });
            crate::ui::draw(&ctx, state, None);
            let _ = ctx.end_pass();
            Ok(())
        })
        .expect("the folder opens");
        dir
    }
}

/// The keys of the stand-in clip's dense row: more than a pixel's worth each, so the row is drawn as a density graph.
pub(super) const DENSE_KEYS: usize = 5000;

/// The handles of the stand-in clip's curve, which an editable clip draws.
pub(super) const HANDLES: usize = 3;

/// A clip's rows as the Animation workspace lists them, with a row picked, hidden, muted and locked, a badge of every channel format and spans with labels, a row of more keys than pixels, and the curve view under them with its handles when the clip is editable.
struct Clip {
    rows: Vec<ChannelRow>,
    curves: Vec<CurveSeries>,
    editable: bool,
}

fn clip_timeline(editable: bool) -> (Clip, TimelineState) {
    use crate::ui::panels::timeline_ycd::format_colour;
    use or_io::codec::anim::ChannelFormat as Format;

    let mut rows = vec![ChannelRow::group(RowId::new(1, 0, 0), "SKEL_Root")
        .with_colour(theme::TEXT_PRIMARY.get())
        .expandable(true, true)];
    for (index, format) in [
        Format::Constant,
        Format::ConstantVec,
        Format::ConstantQuat,
        Format::Raw,
        Format::Quantize,
        Format::Indirect,
        Format::Linear,
        Format::Reconstruct,
        Format::Normalize,
    ]
    .into_iter()
    .enumerate()
    {
        rows.push(
            ChannelRow::group(RowId::new(2, 0, index as u16), format!("Track {index}"))
                .with_depth(1)
                .with_kind(RowKind::Track)
                .with_badge(Some(RowBadge {
                    text: format.engine_name().trim_start_matches('k').to_string(),
                    colour: format_colour(format),
                    tooltip: String::new(),
                })),
        );
    }
    rows.push(
        ChannelRow::group(RowId::new(1, 1, 0), "SKEL_Spine")
            .with_colour(theme::TEXT_PRIMARY.get())
            .expandable(true, false),
    );
    rows.push(
        ChannelRow::group(RowId::new(1, 2, 0), "SKEL_Dense").with_colour(theme::TEXT_PRIMARY.get()),
    );

    let mut state = TimelineState::default();
    state.show_curves = true;
    state.selected_rows.insert(rows[2].id);
    state.rows.toggle_expanded(rows[0].id);
    state.rows.toggle_hidden(rows[3].id);
    state.rows.toggle_muted(rows[4].id);
    state.rows.toggle_locked(rows[5].id);
    let curves = vec![CurveSeries {
        label: "Translation".into(),
        channels: vec![CurveChannel {
            colour: crate::ui::panels::timeline_ycd::TRANSLATION,
            samples: vec![(0.0, -1.0), (0.5, 0.25), (1.0, 1.0)],
        }],
        handles: [(0.0, -1.0), (0.5, 0.25), (1.0, 1.0)]
            .into_iter()
            .enumerate()
            .map(|(index, (time, value))| CurveHandle {
                channel: 0,
                index,
                time,
                value,
            })
            .collect(),
        domain: CurveDomain::Normalized,
        domain_label: "0 to 1".into(),
    }];
    (
        Clip {
            rows,
            curves,
            editable,
        },
        state,
    )
}

impl TimelineSource for Clip {
    fn bounds(&self) -> TimeRange {
        TimeRange::new(0.0, 4.0)
    }

    fn rows(&self) -> &[ChannelRow] {
        &self.rows
    }

    fn row_events(&self, row: usize, _view: TimeRange, out: &mut RowEvents) {
        use crate::ui::panels::timeline_ycd::{ROTATION, SCALE, TAG_SPAN, TRANSLATION};
        out.clear();
        let colour = [TRANSLATION, ROTATION, SCALE, TAG_SPAN][row % 4];
        out.spans.push(SpanEvent {
            range: TimeRange::new(0.5, 3.5),
            colour,
            label: Some(format!("Span {row}")),
        });
        if row + 1 == self.rows.len() {
            out.keys = (0..DENSE_KEYS)
                .map(|key| KeyEvent {
                    time: 4.0 * key as f32 / DENSE_KEYS as f32,
                    shape: KeyShape::Diamond,
                    colour,
                    selected: false,
                })
                .collect();
            out.total_keys = DENSE_KEYS;
        }
    }

    fn curves(&self) -> &[CurveSeries] {
        &self.curves
    }

    fn editable(&self) -> bool {
        self.editable
    }

    fn read_only_reason(&self) -> Option<&str> {
        (!self.editable).then_some("a compact block")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::toasts::{Toast, ToastType};
    use std::time::{Duration, Instant};

    /// A frame of the shell reads no clock: a toast the machine was too slow to paint in time is not painted fading, or gone.
    #[test]
    fn a_toast_is_painted_whole_however_long_the_machine_took_to_get_to_it() {
        let mut shell = Shell::unattended(ThemeChoice::Light);
        shell.state.toasts.push(Toast {
            message: "late".into(),
            created_at: Instant::now() - Duration::from_millis(2990),
            duration: Duration::from_secs(3),
            toast_type: ToastType::Success,
            action: None,
            also: None,
        });
        shell.settled();
        let toast = shell
            .state
            .toasts
            .iter()
            .next()
            .expect("the toast stays up");
        // Made new, not left at the 0.003 it was given: the frames after it take wall time too, so a tighter bound fails on a slow machine
        assert!(
            toast.remaining_fraction() > 0.5,
            "{}",
            toast.remaining_fraction()
        );
    }
}
