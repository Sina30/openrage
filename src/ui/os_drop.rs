//! Files crossing the window's edge: dropped in from Explorer, and archive
//! rows dragged out to it.

use std::path::PathBuf;

use crate::state::os_drop::{drop_asks_to_reopen, is_an_image, DropTarget};
use crate::state::{AppState, DocPayload, DocumentId};
use crate::ui::panels::hierarchy;
use crate::ui::theme;

/// Files staged for a drag out of the window, waiting for the event loop to
/// hand them to the shell once this frame has finished.
#[derive(Default)]
pub struct DragOutRequest {
    pub paths: Vec<PathBuf>,
}

impl crate::ui::panel_state::PanelState for DragOutRequest {
    const KEY: &'static str = "drag-out-request";
}

/// The Project workspace as drawn this frame, which a drop puts files into.
#[derive(Default)]
pub struct ProjectArea(Option<(egui::Rect, u64)>);

impl crate::ui::panel_state::PanelState for ProjectArea {
    const KEY: &'static str = "project-drop-area";
}

/// The dock drew the Project workspace in `rect`: a drop on it imports.
pub fn note_project_area(ctx: &egui::Context, state: &mut AppState, rect: egui::Rect) {
    state.panels.get::<ProjectArea>().0 = Some((rect, ctx.cumulative_pass_nr()));
}

/// The Project workspace's rectangle as drawn in this pass, none when it was not drawn.
fn project_area(ctx: &egui::Context, state: &AppState) -> Option<egui::Rect> {
    let (rect, pass) = state.panels.peek::<ProjectArea>()?.0?;
    (pass == ctx.cumulative_pass_nr()).then_some(rect)
}

/// The staged files, if a drag left the window this frame.
pub fn take_drag_out(state: &mut AppState) -> Vec<PathBuf> {
    state.panels.take::<DragOutRequest>().paths
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let (dropped, held, hovering, pointer) = ctx.input(|i| {
        (
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            (i.raw.hovered_files.iter())
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            !i.raw.hovered_files.is_empty(),
            i.pointer.hover_pos().or(i.pointer.latest_pos()),
        )
    });
    let target = pointer
        .map(|p| target_at(ctx, state, p))
        .unwrap_or(DropTarget::Nowhere);
    // ⚠ A texture tile takes an image and nothing else, so the hint and the drop agree on what any other file does there.
    let first = held.first().or(dropped.first());
    let target = match target {
        DropTarget::Texture if !first.is_some_and(|path| is_an_image(path)) => DropTarget::Nowhere,
        target => target,
    };
    if hovering {
        // ⚠ The hint says what the drop will do: a built pack asks to be reopened, whatever the area under it imports.
        // It is asked of the names held, every frame of the drag: the plan of the drop itself reads the folders.
        let reopens = held.iter().any(|path| drop_asks_to_reopen(path, target));
        paint_hint(ctx, state, target, pointer, reopens);
        ctx.request_repaint();
    }
    if !dropped.is_empty() {
        if crate::ui::panels::question_waits(ctx) {
            state.show_info_toast(tr!(
                "Answer the question on screen first; nothing was dropped"
            ));
        } else {
            state.handle_os_drop(dropped, target);
        }
    }
    drag_out(ctx, state);
}

/// The folder under `p`, from the rows the hierarchy drew this frame.
///
/// ⚠ **This frame's rows only.** A hierarchy that is not on screen leaves
/// last frame's rectangles behind, and a drop aimed at the viewport must not
/// land in a folder that happened to be drawn there earlier.
fn target_at(ctx: &egui::Context, state: &mut AppState, p: egui::Pos2) -> DropTarget {
    // ⚠ The Wardrobe's panel as drawn this frame, and only while its workspace is the one on screen.
    if state.active_workspace == crate::ui::dock::Workspace::Wardrobe
        && crate::ui::panels::wardrobe::accepts_drop_at(ctx, state, p)
    {
        // A tile of a texture variation takes the image for its own texture; the rest of the Wardrobe adds clothes.
        if let Some(tile) = crate::ui::panels::wardrobe::variant_at(ctx, state, p) {
            state.workbench.wardrobe.dressed.drop_tile = Some(tile);
            return DropTarget::Variant;
        }
        return DropTarget::Wardrobe;
    }
    // ⚠ The same for the Weapons screen's list and its assembled weapon.
    if state.active_workspace == crate::ui::dock::Workspace::Weapons
        && crate::ui::panels::weapons::accepts_drop_at(ctx, state, p)
    {
        return DropTarget::Weapons;
    }
    // ⚠ And for the Map screen's list, plan and scene.
    if state.active_workspace == crate::ui::dock::Workspace::Map
        && crate::ui::panels::maps::accepts_drop_at(ctx, state, p)
    {
        return DropTarget::Maps;
    }
    // ⚠ And for the Effects panel of the Particle screen, which is also the editor of a file: only while the project holds particles.
    if state.active_workspace == crate::ui::dock::Workspace::Particle
        && state.has_particles()
        && crate::ui::panels::particles::accepts_drop_at(ctx, state, p)
    {
        return DropTarget::Particles;
    }
    // A texture tile of the gallery, as drawn this pass: an image dropped on it is that texture's.
    if crate::ui::panels::gallery::aim_at_tile(ctx, state, p) {
        return DropTarget::Texture;
    }
    let tree = hierarchy::ui_state(state);
    let hit = (tree.rows_pass == ctx.cumulative_pass_nr())
        .then(|| {
            (tree.row_rects.iter().rev())
                .find(|(_, _, rect)| rect.contains(p))
                .map(|(doc, node, _)| (*doc, *node))
        })
        .flatten();
    if let Some((doc, folder)) =
        hit.and_then(|(doc, node)| Some((doc, state.drop_folder_for(doc, node)?)))
    {
        return DropTarget::Folder { doc, folder };
    }
    // ⚠ Anywhere else on the Project workspace a drop is put in the project; on every other screen it opens.
    if state.active_workspace == crate::ui::dock::Workspace::Project
        && project_area(ctx, state).is_some_and(|area| area.contains(p))
    {
        return DropTarget::Project;
    }
    DropTarget::Nowhere
}

/// What the hint under a held drag says: what the drop does and, under it, what it changes.
struct Hint {
    title: String,
    effect: String,
    /// The drop will be refused, so the hint is drawn quiet and the target is not outlined.
    refused: bool,
}

impl Hint {
    fn new(title: &str, effect: &str) -> Self {
        Self {
            title: title.to_string(),
            effect: effect.to_string(),
            refused: false,
        }
    }
}

fn paint_hint(
    ctx: &egui::Context,
    state: &mut AppState,
    target: DropTarget,
    pointer: Option<egui::Pos2>,
    reopens: bool,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("os_drop_hint"),
    ));
    let outline = |rect: egui::Rect, rounding: f32, kind: egui::StrokeKind| {
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(2.0_f32, theme::ACCENT.get()),
            kind,
        );
    };
    let hint = match target {
        DropTarget::Folder { doc, folder } => {
            let (path, on_disk) = match state.document(doc).map(|d| &d.payload) {
                Some(DocPayload::Rpf(rpf)) => (
                    rpf.session.path_of(folder).unwrap_or_default(),
                    rpf.session.node(folder).is_some_and(|node| {
                        matches!(
                            node.backing,
                            crate::state::RpfNodeBacking::DiskFolder { .. }
                        )
                    }),
                ),
                _ => (String::new(), false),
            };
            let refused = state.a_drop_into_is_refused(doc, folder);
            if !refused {
                if let Some((_, _, rect)) = hierarchy::ui_state(state)
                    .row_rects
                    .iter()
                    .find(|(d, n, _)| *d == doc && *n == folder)
                {
                    outline(rect.expand(1.0), 3.0, egui::StrokeKind::Outside);
                }
            }
            // ⚠ A folder on disk takes the copy at once; an archive's takes it when it is saved.
            let (into, effect) = match on_disk {
                true => ("folder", tr!("Copied at once")),
                false => (
                    "archive",
                    tr!("Added now, and written when the archive is saved"),
                ),
            };
            let place = path
                .trim_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default();
            Hint {
                title: tr!("Copy into {into} {place}", into = into, place = place)
                    .trim_end()
                    .to_string(),
                effect: match refused {
                    true => tr!("Read only: this drop will be refused").to_string(),
                    false => effect.to_string(),
                },
                refused,
            }
        }
        DropTarget::Wardrobe => {
            if let Some(rect) = crate::ui::panels::wardrobe::drop_rect(state) {
                outline(rect.shrink(2.0), 4.0, egui::StrokeKind::Inside);
            }
            Hint::new(
                tr!("Add as clothes"),
                tr!("Each file becomes a garment or a texture. Undo takes it back"),
            )
        }
        DropTarget::Weapons => Hint::new(
            tr!("Add as weapons"),
            tr!("Each file becomes a source of the pack, and the weapons a weapon file defines join it"),
        ),
        DropTarget::Maps => Hint::new(
            tr!("Add to the maps"),
            tr!("Each file becomes a source of the pack and joins the map set shown"),
        ),
        DropTarget::Particles => {
            if let Some(rect) = crate::ui::panels::particles::drop_rect(state) {
                outline(rect.shrink(2.0), 4.0, egui::StrokeKind::Inside);
            }
            Hint::new(
                tr!("Add as particle effects"),
                tr!("Each .ypt file becomes a source of the pack, its effects as rules and its textures as DDS files"),
            )
        }
        DropTarget::Project => {
            if let Some(area) = project_area(ctx, state) {
                outline(area.shrink(2.0), 4.0, egui::StrokeKind::Inside);
            }
            Hint::new(
                tr!("Import into the project"),
                tr!("Decompiled into sources. The originals are not touched"),
            )
        }
        DropTarget::Variant => {
            if let Some((garment, variation)) = state.workbench.wardrobe.dressed.drop_tile.clone() {
                if let Some(rect) =
                    crate::ui::panels::wardrobe::variant_rect(state, &garment, variation)
                {
                    outline(rect.shrink(1.0), 4.0, egui::StrokeKind::Inside);
                }
            }
            Hint::new(
                tr!("Make this texture from the image"),
                tr!("Replaces this texture. Undo takes it back"),
            )
        }
        DropTarget::Texture => {
            if let Some(rect) = crate::ui::panels::gallery::aimed_tile_rect(state) {
                outline(rect.shrink(1.0), 4.0, egui::StrokeKind::Inside);
            }
            Hint::new(
                tr!("Replace this texture from the image"),
                tr!("The texture keeps its name and its format. Undo takes it back"),
            )
        }
        DropTarget::Nowhere => Hint::new(
            tr!("Open in OpenRage"),
            tr!("Opens the file where it is. Nothing is changed"),
        ),
    };
    let hint = if reopens {
        Hint::new(
            tr!("Import as a project"),
            tr!("Asks first. The pack is never changed"),
        )
    } else {
        hint
    };
    if let Some(at) = pointer {
        plate(ctx, &painter, at, &hint);
    }
}

/// The hint on a plate beside the pointer, kept inside the window, so it reads over whatever is under it.
fn plate(ctx: &egui::Context, painter: &egui::Painter, at: egui::Pos2, hint: &Hint) {
    const PAD: egui::Vec2 = egui::vec2(8.0, 6.0);
    let text = if hint.refused {
        theme::TEXT_SECONDARY
    } else {
        theme::TEXT_PRIMARY
    };
    let title = painter.layout_no_wrap(
        hint.title.clone(),
        egui::FontId::proportional(13.0),
        text.get(),
    );
    let effect = painter.layout(
        hint.effect.clone(),
        egui::FontId::proportional(11.0),
        theme::TEXT_SECONDARY.get(),
        280.0,
    );
    let inner = egui::vec2(
        title.size().x.max(effect.size().x),
        title.size().y + 2.0 + effect.size().y,
    );
    let size = inner + PAD * 2.0;
    let room = ctx.content_rect().shrink(4.0);
    let min = (at + egui::vec2(16.0, 16.0))
        .min(room.max - size)
        .max(room.min);
    let rect = egui::Rect::from_min_size(min, size);
    let edge = if hint.refused {
        theme::BORDER
    } else {
        theme::ACCENT
    };
    painter.rect(
        rect,
        6.0,
        theme::BG_L3.get(),
        egui::Stroke::new(1.0_f32, edge.get()),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + PAD, title.clone(), text.get());
    painter.galley(
        rect.min + PAD + egui::vec2(0.0, title.size().y + 2.0),
        effect,
        theme::TEXT_SECONDARY.get(),
    );
}

fn drag_key(doc: DocumentId) -> egui::Id {
    egui::Id::new(("rpf_drag", doc))
}

/// A row dragged past the window's edge becomes files Explorer can take.
///
/// ⚠ **A released drag is forgotten here whatever it was over.** The tree
/// only clears it when the release lands on a folder, so a drag let go over
/// a file would be remembered and the next click on any folder would move it.
fn drag_out(ctx: &egui::Context, state: &mut AppState) {
    let docs: Vec<DocumentId> = state.documents.iter().map(|d| d.id).collect();
    let Some((doc, node)) = docs.iter().find_map(|&doc| {
        ctx.data(|d| d.get_temp::<u64>(drag_key(doc)))
            .map(|n| (doc, n))
    }) else {
        return;
    };
    let (held, pos) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
    if !held {
        ctx.data_mut(|d| d.remove::<u64>(drag_key(doc)));
        return;
    }
    // A drop with Ctrl held copies: the pointer says so while the drag is on, as Explorer's does
    if ctx.input(|i| i.modifiers.ctrl) {
        ctx.set_cursor_icon(egui::CursorIcon::Copy);
    }
    let outside = pos.is_some_and(|p| !ctx.content_rect().contains(p));
    if !outside {
        return;
    }
    ctx.data_mut(|d| d.remove::<u64>(drag_key(doc)));

    let nodes = state.selection.dragged(doc, node);
    let staging = staging_dir("openrage-drag");
    match state.stage_drag_out(doc, &nodes, &staging) {
        Ok(paths) => state.panels.get::<DragOutRequest>().paths = paths,
        Err(e) => state.show_error_toast(e),
    }
}

/// A fresh directory under the temporary folder's `root` for one drag's or one copy's files.
///
/// ⚠ **Not removed when the drag ends**: Explorer may still be copying out of
/// it after the drop returns, or paste from it hours after a copy. Earlier
/// directories under `root` older than a day are swept on the next instead.
/// The temporary folder's `root` is the editor's, sweep included, so a
/// self-check stages under its own configuration folder instead.
pub(crate) fn staging_dir(root: &str) -> PathBuf {
    let base = match crate::state::prefs::config_root() {
        Some(own) if crate::platform::standing_in() => own,
        _ => std::env::temp_dir(),
    };
    let root = base.join(root);
    if let Ok(entries) = std::fs::read_dir(&root) {
        let day = std::time::Duration::from_secs(24 * 3600);
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > day);
            if old {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    root.join(staged_name(stamp))
}

/// ⚠ The clock ticks in 100 ns on Windows, so two asks can read one `stamp`, and the count tells them apart.
fn staged_name(stamp: u128) -> String {
    static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let asked = ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{stamp}-{asked}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DocumentKind;

    fn frame(ctx: &egui::Context, state: &mut AppState, n: usize, input: egui::RawInput) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(n as f64 * 0.05),
            ..input
        };
        let _ = ctx.run_ui(input, |ui| {
            hierarchy::draw(ui, state);
            draw(ui.ctx(), state);
        });
    }

    fn archive() -> (AppState, DocumentId, u64, u64) {
        let mut state = AppState::default();
        let doc = state.new_empty_document(DocumentKind::Rpf);
        let DocPayload::Rpf(rpf) = &mut state.document_mut(doc).unwrap().payload else {
            panic!("an empty RPF document holds an RPF payload");
        };
        let root = rpf.session.root_id();
        let props = rpf.session.add_folder(root, "props").unwrap();
        let file = rpf
            .session
            .add_file_copy(root, "a.ytd", vec![5; 9])
            .unwrap();
        state.set_active_document(doc);
        state.prefs.explorer = Default::default();
        (state, doc, props, file)
    }

    fn row(state: &mut AppState, doc: DocumentId, node: u64) -> egui::Rect {
        hierarchy::ui_state(state)
            .row_rects
            .iter()
            .find(|(d, n, _)| *d == doc && *n == node)
            .map(|(_, _, r)| *r)
            .expect("the row was drawn")
    }

    /// The cursor of a drag in progress, over the window, after the frames of `input`.
    fn cursor_of_a_drag(ctrl: bool) -> egui::CursorIcon {
        let (mut state, doc, _, file) = archive();
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(drag_key(doc), file));
        let at = egui::pos2(300.0, 300.0);
        let modifiers = egui::Modifiers {
            ctrl,
            ..Default::default()
        };
        let press = egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        };
        let mut icon = egui::CursorIcon::Default;
        for (n, events) in [vec![egui::Event::PointerMoved(at), press], Vec::new()]
            .into_iter()
            .enumerate()
        {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                time: Some(n as f64 * 0.05),
                modifiers,
                events,
                ..Default::default()
            };
            icon = ctx
                .run_ui(input, |ui| drag_out(ui.ctx(), &mut state))
                .platform_output
                .cursor_icon;
        }
        icon
    }

    #[test]
    fn a_drag_with_ctrl_held_shows_the_copy_cursor_and_one_without_does_not() {
        assert_eq!(cursor_of_a_drag(true), egui::CursorIcon::Copy);
        assert_ne!(cursor_of_a_drag(false), egui::CursorIcon::Copy);
    }

    fn names_in(state: &AppState, doc: DocumentId, folder: u64) -> Vec<String> {
        let DocPayload::Rpf(rpf) = &state.document(doc).unwrap().payload else {
            unreachable!()
        };
        rpf.session
            .children_of(folder)
            .into_iter()
            .map(|id| rpf.session.node(id).unwrap().name.clone())
            .collect()
    }

    /// ⚠ **The drop arrives the way egui-winit delivers it** — a dropped path
    /// in the frame's raw input and the pointer where the cursor is — and the
    /// test reads the archive, not the paint.
    #[test]
    fn a_file_dropped_on_a_folder_row_is_imported_into_that_folder() {
        let (mut state, doc, props, _) = archive();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0, Default::default());
        frame(&ctx, &mut state, 1, Default::default());
        let at = row(&mut state, doc, props).center();

        let disk = tempfile::tempdir().unwrap();
        let dropped = disk.path().join("lamp.ydr");
        std::fs::write(&dropped, b"bytes").unwrap();
        frame(
            &ctx,
            &mut state,
            2,
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(at)],
                dropped_files: vec![egui::DroppedFile {
                    path: Some(dropped),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        assert_eq!(names_in(&state, doc, props), ["lamp.ydr"]);
        assert!(
            state.documents.iter().all(|d| d.path.is_none()),
            "a .ydr aimed at a folder is imported, not opened"
        );
    }

    /// ⚠ **Drives a real row drag past the window's edge** and reads what was
    /// staged; nothing here reaches the shell, which only the event loop calls.
    #[test]
    fn a_row_dragged_out_of_the_window_is_staged_for_explorer_and_not_moved() {
        let (mut state, doc, props, file) = archive();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0, Default::default());
        frame(&ctx, &mut state, 1, Default::default());
        let at = row(&mut state, doc, file).center();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let moves = [
            vec![egui::Event::PointerMoved(at), button(at, true)],
            vec![egui::Event::PointerMoved(at + egui::vec2(12.0, 0.0))],
            vec![egui::Event::PointerMoved(at + egui::vec2(40.0, 0.0))],
            vec![egui::Event::PointerMoved(egui::pos2(-40.0, at.y))],
        ];
        for (n, events) in moves.into_iter().enumerate() {
            frame(
                &ctx,
                &mut state,
                2 + n,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
            );
        }
        let staged = take_drag_out(&mut state);
        assert_eq!(staged.len(), 1, "one row staged: {staged:?}");
        assert!(staged[0].ends_with("a.ytd"));
        assert_eq!(std::fs::read(&staged[0]).unwrap(), vec![5; 9]);
        let _ = std::fs::remove_dir_all(staged[0].parent().unwrap());

        let over_folder = row(&mut state, doc, props).center();
        frame(
            &ctx,
            &mut state,
            9,
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(over_folder),
                    button(over_folder, false),
                ],
                ..Default::default()
            },
        );
        assert!(
            names_in(&state, doc, props).is_empty(),
            "the drag left the window, so releasing over a folder moves nothing"
        );
    }

    #[test]
    fn a_row_dragged_onto_a_folder_row_moves_into_it() {
        let (mut state, doc, props, file) = archive();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0, Default::default());
        frame(&ctx, &mut state, 1, Default::default());
        let at = row(&mut state, doc, file).center();
        let folder = row(&mut state, doc, props).center();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let steps = [
            vec![egui::Event::PointerMoved(at), button(at, true)],
            vec![egui::Event::PointerMoved(at + egui::vec2(0.0, -6.0))],
            vec![egui::Event::PointerMoved(folder)],
            vec![button(folder, false)],
        ];
        for (n, events) in steps.into_iter().enumerate() {
            frame(
                &ctx,
                &mut state,
                2 + n,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
            );
        }
        assert_eq!(names_in(&state, doc, props), ["a.ytd"]);
    }

    #[test]
    fn a_drag_let_go_over_a_file_is_forgotten() {
        let (mut state, doc, props, file) = archive();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0, Default::default());
        frame(&ctx, &mut state, 1, Default::default());
        let at = row(&mut state, doc, file).center();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let steps = [
            vec![egui::Event::PointerMoved(at), button(at, true)],
            vec![egui::Event::PointerMoved(at + egui::vec2(30.0, 2.0))],
            vec![button(at + egui::vec2(30.0, 2.0), false)],
        ];
        for (n, events) in steps.into_iter().enumerate() {
            frame(
                &ctx,
                &mut state,
                2 + n,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
            );
        }
        let folder = row(&mut state, doc, props).center();
        for (n, events) in [
            vec![egui::Event::PointerMoved(folder), button(folder, true)],
            vec![button(folder, false)],
        ]
        .into_iter()
        .enumerate()
        {
            frame(
                &ctx,
                &mut state,
                10 + n,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
            );
        }
        assert!(
            names_in(&state, doc, props).is_empty(),
            "a click on a folder after an abandoned drag moves nothing into it"
        );
    }

    /// Every text the frame painted.
    fn painted(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    #[test]
    fn the_hint_over_a_folder_of_an_archive_names_the_folder_and_says_the_copy_waits_for_a_save() {
        let (mut state, doc, props, _) = archive();
        let ctx = egui::Context::default();
        frame(&ctx, &mut state, 0, Default::default());
        frame(&ctx, &mut state, 1, Default::default());
        let at = row(&mut state, doc, props).center();
        let disk = tempfile::tempdir().unwrap();
        let held = disk.path().join("lamp.ydr");
        std::fs::write(&held, b"bytes").unwrap();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(0.5),
            events: vec![egui::Event::PointerMoved(at)],
            hovered_files: vec![egui::HoveredFile {
                path: Some(held),
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| {
            hierarchy::draw(ui, &mut state);
            draw(ui.ctx(), &mut state);
        });
        let said = painted(&out.shapes);
        assert!(
            said.iter().any(|t| t == "Copy into archive props"),
            "the hint names the folder as the tree does, with no path: {said:?}"
        );
        assert!(
            said.iter()
                .any(|t| t.contains("written when the archive is saved")),
            "and says the copy waits for a save: {said:?}"
        );
    }

    /// ⚠ **A frame of a drag held over the window reads no folder**: the hint says what the drop will do from the held
    /// names alone, and the plan that carries the drop out looks inside each folder, which a held drag redid every frame.
    #[test]
    fn a_drag_held_over_the_window_reads_no_folder_to_say_what_the_drop_will_do() {
        use crate::state::front_door::new_project::FOLDER_READS;

        let disk = tempfile::tempdir().unwrap();
        let held = disk.path().join("mods");
        std::fs::create_dir_all(&held).unwrap();
        for n in 0..50 {
            std::fs::write(held.join(format!("road{n}.ytd")), b"x").unwrap();
        }
        let pack = disk.path().join("dlc.rpf");
        std::fs::write(&pack, b"an archive").unwrap();
        let mut state = AppState::default();
        state.active_workspace = crate::ui::dock::Workspace::Project;
        let ctx = egui::Context::default();
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let mut hover = |n: usize, paths: Vec<PathBuf>| {
            let input = egui::RawInput {
                screen_rect: Some(area),
                time: Some(n as f64 * 0.05),
                events: vec![egui::Event::PointerMoved(area.center())],
                hovered_files: (paths.into_iter())
                    .map(|path| egui::HoveredFile {
                        path: Some(path),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| {
                note_project_area(ui.ctx(), &mut state, area);
                draw(ui.ctx(), &mut state);
            });
            painted(&out.shapes)
        };
        hover(0, vec![held.clone()]);

        let before = FOLDER_READS.with(|reads| reads.get());
        let over_a_folder = hover(1, vec![held]);
        let over_a_pack = hover(2, vec![pack]);
        assert_eq!(
            FOLDER_READS.with(|reads| reads.get()),
            before,
            "a held drag read a folder to paint its hint"
        );
        assert!(
            over_a_folder.iter().any(|t| t == "Import into the project"),
            "{over_a_folder:?}"
        );
        assert!(
            over_a_pack.iter().any(|t| t == "Import as a project"),
            "{over_a_pack:?}"
        );
    }

    #[test]
    fn two_copies_in_one_tick_of_the_clock_stage_in_folders_of_their_own() {
        assert_ne!(
            staged_name(7),
            staged_name(7),
            "two asks that read one stamp were given one folder to stage in"
        );
    }
}
