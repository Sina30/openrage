//! The open documents' tabs above the dock: one per document, each with a close
//! button and one mark for unsaved work, and a list at the strip's end of the
//! tabs that do not fit.

use std::sync::Arc;

use egui_phosphor::regular as ph;

use crate::state::gltf::Model;
use crate::state::workbench::TabPlace;
use crate::state::{AppState, DocumentId, DocumentKind};
use crate::ui::commands::{self, Action, Command, Target};
use crate::ui::theme;

/// The widest a tab's name is drawn before it is cut short.
const TITLE_MAX: f32 = 280.0;
const PAD: f32 = 8.0;
const CLOSE_SIDE: f32 = 16.0;
const DOT_WIDTH: f32 = 16.0;
/// Room kept at the strip's end for the list of the tabs that do not fit.
const LIST_WIDTH: f32 = 48.0;

/// One tab, as the strip draws it.
struct Tab {
    id: DocumentId,
    title: String,
    glyph: &'static str,
    /// The colour of the kind, as the Files tree draws its icon.
    tint: egui::Color32,
    active: bool,
    unsaved: bool,
    preview: bool,
    install: bool,
    hover: String,
}

/// What a frame's clicks on the strip asked for, done once it is drawn.
#[derive(Default)]
struct Asked {
    activate: Option<DocumentId>,
    keep: Option<DocumentId>,
    close: Option<DocumentId>,
    /// The tab of a screen of the project that was clicked.
    place: Option<TabPlace>,
    /// A command the tab's menu offers.
    command: Option<Command>,
}

/// The room the row of tabs keeps at its right end for the commands of a screen of the project, until a frame has measured them.
const SCREEN_ROOM: f32 = 380.0;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    state.tick_tabs(ui.input(|i| i.modifiers.ctrl));
    let tabs: Vec<Tab> = state
        .tab_order()
        .into_iter()
        .filter_map(|id| describe(state, id))
        .collect();
    let screens = screens(state);
    if tabs.is_empty() && screens.is_empty() {
        return;
    }
    // A screen of the project that has commands of its own draws them at the right end of the row, where the header of its list had them: a second row would take its height from every panel under it.
    let ws = state.active_workspace;
    let of_the_screen = (ws.needs_a_project() && state.project_path.is_some())
        .then(|| crate::ui::task_screens::of_workspace(ws).and_then(|row| row.bar))
        .flatten();
    egui::Panel::top("document_tabs")
        .frame(
            egui::Frame::NONE
                .fill(theme::TAB_STRIP_BG.get())
                .inner_margin(egui::Margin::symmetric(6, 2)),
        )
        .min_size(theme::TAB_STRIP_HEIGHT - 4.0)
        .show_inside(ui, |ui| {
            let row = ui.max_rect();
            // ⚠ As wide as the commands were in the frame before: a language with longer words takes more of the row, and a width of its own put the last button past the window's edge.
            let width_of = egui::Id::new("commands_of_the_screen_in_the_row_of_tabs");
            let measured: Option<f32> = ui.data(|d| d.get_temp(width_of));
            let kept = match of_the_screen {
                Some(_) => measured.unwrap_or(SCREEN_ROOM).min(row.width() * 0.7),
                None => 0.0,
            };
            // ⚠ The tabs keep to their own part of the row, so the list of the ones that do not fit ends before the commands begin.
            // ⚠ One row tall: the panel is as tall as what is drawn in it, and its own rectangle reaches the bottom of the window until then.
            let bottom = row.top() + theme::TAB_STRIP_BUTTON_HEIGHT;
            let left = egui::Rect::from_min_max(row.min, egui::pos2(row.right() - kept, bottom));
            let tabs_in = egui::UiBuilder::new().max_rect(left);
            let asked = ui
                .scope_builder(tabs_in, |ui| strip(ui, state, &screens, &tabs))
                .inner;
            if let Some(commands_of) = of_the_screen {
                let at = egui::Rect::from_min_max(
                    egui::pos2(row.right() - kept, row.top()),
                    egui::pos2(row.right(), bottom),
                );
                let beside = egui::UiBuilder::new()
                    .max_rect(at)
                    .layout(egui::Layout::left_to_right(egui::Align::Center));
                let drawn = ui.scope_builder(beside, |ui| commands_of(ui, state));
                let used = drawn.response.rect.width() + 4.0;
                ui.data_mut(|d| d.insert_temp(width_of, used));
            }
            apply(state, ui.ctx(), asked);
        });
    // A file's bar says where the file is and offers what is done with its kind.
    if let Some(context) = context(state) {
        egui::Panel::top("context_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::TAB_STRIP_BG.get())
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show_inside(ui, |ui| {
                let mut asked = Asked::default();
                bar(ui, &context, &mut asked);
                apply(state, ui.ctx(), asked);
            });
    }
}

fn describe(state: &AppState, id: DocumentId) -> Option<Tab> {
    let doc = state.document(id)?;
    let preview = state.is_preview(id);
    let unsaved = state.loses_work(doc);
    let path = doc.location();
    let hint = if preview {
        tr!("A preview: the next file clicked in Files takes its place. Double-click it, or edit it, to keep it.")
    } else if unsaved {
        tr!("Unsaved changes — closing it asks to save them first")
    } else {
        tr!("Middle-click or its close button closes it")
    };
    Some(Tab {
        id,
        title: state.tab_title(doc),
        glyph: doc.kind.info().glyph,
        tint: theme::graphic(doc.kind.info().color),
        // ⚠ A screen of the project in front draws no document, so no file's tab is the one on show.
        active: state.tab_on_show(TabPlace::Document(id)),
        unsaved,
        preview,
        install: doc.kind == DocumentKind::GameRoot,
        hover: format!("{path}\n\n{hint}"),
    })
}

/// The most commands the bar offers beside the primary one.
const MORE: usize = 3;

/// The one primary command of a kind of file: what a person most often opened that kind of file to do.
///
/// ⚠ Every kind has exactly one, so the bar never draws a tab with none: an editor's is Save, and a kind that is only looked through leads to the search of what it holds.
pub(crate) fn primary_of(kind: DocumentKind) -> Action {
    match kind {
        DocumentKind::Ydr | DocumentKind::Ydd | DocumentKind::Yft => Action::ExportGltf,
        DocumentKind::Ytd | DocumentKind::Dds => Action::ReplaceTextureFromImage,
        kind => match crate::ui::dock::Workspace::for_kind(kind) {
            crate::ui::dock::Workspace::Browse => Action::GoToFile,
            _ => Action::Save,
        },
    }
}

/// What the bar under the tabs says of the tab in front: where its thing is, its primary command and at most three more.
pub(crate) struct Context {
    pub place: String,
    pub primary: Option<Command>,
    pub more: Vec<Command>,
}

/// The bar of the tab in front, none with nothing in front.
///
/// ⚠ None over a screen of the project: it draws no document, the window's Build is the overview's primary action, and a content's screen draws the commands its row names.
pub(crate) fn context(state: &AppState) -> Option<Context> {
    let ws = state.active_workspace;
    if ws.needs_a_project() {
        return None;
    }
    let doc = state.active_document()?;
    let primary = primary_of(doc.kind);
    let target = Target::Document(doc.id);
    // ⚠ Only what can run: the bar is three shortcuts and not a menu, which is where a command says why it waits.
    let more = [Action::Save, Action::Convert, Action::RevealOnDisk]
        .into_iter()
        .filter(|action| *action != primary)
        .filter_map(|action| {
            commands::on(state, action, target.clone()).or_else(|| commands::of(state, action))
        })
        .filter(|command| command.enabled.is_ok())
        .take(MORE)
        .collect();
    Some(Context {
        place: doc.location(),
        primary: commands::of(state, primary),
        more,
    })
}

/// A command's label behind its icon, where it has one.
fn with_glyph(command: &commands::Command) -> String {
    match commands::glyph(&command.id) {
        Some(glyph) => format!("{glyph} {}", command.label),
        None => command.label.clone(),
    }
}

/// The bar: the place at the left, cut short where it does not fit, and the commands at the right with the primary one filled and last.
fn bar(ui: &mut egui::Ui, context: &Context, asked: &mut Asked) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(command) = &context.primary {
            // ⚠ Filled only while it can run: the accent fill under the dimmed label of a disabled button is navy on navy.
            let button = match command.enabled {
                Ok(()) => egui::Button::new(
                    egui::RichText::new(with_glyph(command)).color(theme::ON_ACCENT.get()),
                )
                .fill(theme::ACCENT.get()),
                Err(_) => egui::Button::new(with_glyph(command)),
            };
            let response = ui.add_enabled(command.enabled.is_ok(), button.small());
            let response = crate::ui::accessible_name::named(response, &command.label);
            if commands::hints(response, command).clicked() {
                asked.command = Some(command.clone());
            }
        }
        for command in context.more.iter().rev() {
            let button = egui::Button::new(with_glyph(command)).small();
            let response = ui.add_enabled(command.enabled.is_ok(), button);
            let response = crate::ui::accessible_name::named(response, &command.label);
            if commands::hints(response, command).clicked() {
                asked.command = Some(command.clone());
            }
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let place = egui::RichText::new(&context.place)
                .small()
                .color(theme::TEXT_SECONDARY);
            ui.add(egui::Label::new(place).truncate());
        });
    });
}

fn strip(ui: &mut egui::Ui, state: &AppState, screens: &[ScreenTab], tabs: &[Tab]) -> Asked {
    let gap = theme::SPACING_TIGHT;
    let titles: Vec<Arc<egui::Galley>> = tabs.iter().map(|tab| title(ui, tab)).collect();
    let widths: Vec<f32> = titles.iter().map(|title| width(title)).collect();
    let active = tabs.iter().position(|tab| tab.active);
    let names: Vec<Arc<egui::Galley>> = (screens.iter())
        .map(|screen| screen_title(ui, screen))
        .collect();
    let taken: f32 = names.iter().map(|n| 2.0 * PAD + n.size().x + gap).sum();
    let shown = fitting(&widths, gap, ui.available_width() - taken, active);
    let hidden: Vec<usize> = (0..tabs.len()).filter(|i| !shown.contains(i)).collect();

    let mut asked = Asked::default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (nth, (screen, name)) in screens.iter().zip(names).enumerate() {
            draw_screen(ui, nth, screen, name, &mut asked);
        }
        for &i in &shown {
            draw_tab(
                ui,
                state,
                &tabs[i],
                titles[i].clone(),
                widths[i],
                &mut asked,
            );
        }
        if !hidden.is_empty() {
            list(ui, tabs, &hidden, &mut asked);
        }
    });
    asked
}

/// A screen of the project as the row draws its tab.
struct ScreenTab {
    place: TabPlace,
    label: String,
    on_show: bool,
    hover: &'static str,
}

/// The tabs of the project's screens: its overview, and the screen of each content it holds.
fn screens(state: &AppState) -> Vec<ScreenTab> {
    (state.screen_tabs().into_iter())
        .filter_map(|place| {
            let ws = match place {
                TabPlace::Screen(ws) | TabPlace::Content(ws, _) => ws,
                TabPlace::Document(_) => return None,
            };
            Some(ScreenTab {
                place,
                label: state.tab_place_name(place),
                on_show: state.tab_on_show(place),
                hover: ws.tooltip(),
            })
        })
        .collect()
}

fn screen_title(ui: &egui::Ui, screen: &ScreenTab) -> Arc<egui::Galley> {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = theme::document_chip_fg(screen.on_show);
    ui.painter()
        .layout_no_wrap(screen.label.clone(), font, color)
}

/// A screen of the project as a tab before the files': it has no close button, being a part of the project and not a thing opened.
fn draw_screen(
    ui: &mut egui::Ui,
    nth: usize,
    screen: &ScreenTab,
    name: Arc<egui::Galley>,
    asked: &mut Asked,
) {
    let on_show = screen.on_show;
    let size = egui::vec2(2.0 * PAD + name.size().x, theme::TAB_STRIP_BUTTON_HEIGHT);
    // ⚠ An id of its place: two contents may be called alike, and the row lays them out one after another.
    let (rect, body) = ui
        .push_id(("screen_tab", nth), |ui| {
            ui.allocate_exact_size(size, egui::Sense::click())
        })
        .inner;
    let border = theme::BORDER.get();
    let corner = egui::CornerRadius::same(theme::RADIUS_SMALL);
    let painter = ui.painter();
    painter.rect_filled(rect, corner, theme::document_chip_fill(on_show));
    painter.rect_stroke(
        rect,
        corner,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
    if on_show {
        theme::paint_active_tab_line(painter, rect);
    }
    let at = egui::pos2(rect.left() + PAD, rect.center().y - name.size().y / 2.0);
    painter.galley(at, name, theme::document_chip_fg(on_show));
    body.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            on_show,
            &screen.label,
        )
    });
    if body.on_hover_text(screen.hover).clicked() {
        asked.place = Some(screen.place);
    }
}

/// The tabs drawn in `room`: all when they fit, else as many as fit beside the list, the active one always among them.
fn fitting(widths: &[f32], gap: f32, room: f32, active: Option<usize>) -> Vec<usize> {
    let all = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if all <= room {
        return (0..widths.len()).collect();
    }
    let room = room - LIST_WIDTH - gap;
    let span = |shown: &[usize]| shown.iter().map(|&i| widths[i] + gap).sum::<f32>();
    let mut shown = Vec::new();
    for (i, width) in widths.iter().enumerate() {
        if span(&shown) + width > room {
            break;
        }
        shown.push(i);
    }
    if let Some(active) = active.filter(|a| !shown.contains(a)) {
        while !shown.is_empty() && span(&shown) + widths[active] > room {
            shown.pop();
        }
        shown.push(active);
    }
    shown
}

fn title(ui: &egui::Ui, tab: &Tab) -> Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: egui::TextStyle::Button.resolve(ui.style()),
        color: theme::document_chip_fg(tab.active),
        italics: tab.preview,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let tint = egui::TextFormat {
        color: tab.tint,
        italics: false,
        ..format.clone()
    };
    job.append(tab.glyph, 0.0, tint);
    job.append(&format!(" {}", tab.title), 0.0, format);
    job.wrap = egui::text::TextWrapping {
        max_width: TITLE_MAX,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// ⚠ Room for the unsaved mark whether or not it shows, so the first edit moves no tab.
fn width(title: &egui::Galley) -> f32 {
    PAD + title.size().x + DOT_WIDTH + CLOSE_SIDE + PAD
}

fn draw_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    tab: &Tab,
    title: Arc<egui::Galley>,
    width: f32,
    asked: &mut Asked,
) {
    ui.push_id(("doc_tab", tab.id), |ui| {
        let size = egui::vec2(width, theme::TAB_STRIP_BUTTON_HEIGHT);
        let (rect, body) = ui.allocate_exact_size(size, egui::Sense::click());
        let close_at = egui::pos2(rect.right() - PAD / 2.0 - CLOSE_SIDE / 2.0, rect.center().y);
        let close_rect = egui::Rect::from_center_size(close_at, egui::Vec2::splat(CLOSE_SIDE));
        // ⚠ After the tab's own response, so a click on the button is the button's.
        let close = ui.interact(close_rect, body.id.with("close"), egui::Sense::click());

        let fg = theme::document_chip_fg(tab.active);
        let border = theme::BORDER.get();
        let corner = egui::CornerRadius::same(theme::RADIUS_SMALL);
        let font = egui::TextStyle::Button.resolve(ui.style());
        let painter = ui.painter();
        painter.rect_filled(rect, corner, theme::document_chip_fill(tab.active));
        painter.rect_stroke(
            rect,
            corner,
            egui::Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );
        if tab.active {
            theme::paint_active_tab_line(painter, rect);
        }
        let title_end = rect.left() + PAD + title.size().x;
        let title_at = egui::pos2(rect.left() + PAD, rect.center().y - title.size().y / 2.0);
        painter.galley(title_at, title, fg);
        if tab.unsaved {
            let dot_at = egui::pos2(title_end + DOT_WIDTH / 2.0, rect.center().y);
            painter.text(
                dot_at,
                egui::Align2::CENTER_CENTER,
                "●",
                font.clone(),
                theme::WARNING.get(),
            );
        }
        if close.hovered() {
            painter.rect_filled(
                close_rect,
                corner,
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
        }
        painter.text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            ph::X,
            font,
            fg,
        );

        let label = if tab.unsaved {
            format!("{} ●", tab.title)
        } else {
            tab.title.clone()
        };
        body.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, tab.active, &label)
        });
        close.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Close"))
        });
        let close = close.on_hover_text(tr!("Close"));
        let body = body.on_hover_text(&tab.hover);
        if close.clicked() || body.middle_clicked() {
            asked.close = Some(tab.id);
        } else if body.clicked() {
            asked.activate = Some(tab.id);
            if body.double_clicked() {
                asked.keep = Some(tab.id);
            }
        }
        body.context_menu(|ui| menu(ui, state, tab, asked));
    });
}

fn menu(ui: &mut egui::Ui, state: &AppState, tab: &Tab, asked: &mut Asked) {
    crate::ui::popup_of_a_document(ui.ctx(), tab.id);
    if ui.button(tr!("Activate")).clicked() {
        asked.activate = Some(tab.id);
        ui.close_kind(egui::UiKind::Menu);
    }
    let target = Target::Document(tab.id);
    let command = &mut asked.command;
    commands::offer(ui, state, Action::CloseDocument, &target, command);
    if !tab.install {
        commands::offer(ui, state, Action::CloseOtherTabs, &target, command);
    }
    ui.separator();
    commands::offer(ui, state, Action::CopyPath, &target, command);
    commands::offer(ui, state, Action::RevealOnDisk, &target, command);
    let (converts, gltf) = (
        state.document_conversion_to(tab.id).is_ok(),
        state.offers_gltf(Model::Document(tab.id)),
    );
    if converts || gltf {
        ui.separator();
    }
    if converts {
        commands::offer(ui, state, Action::Convert, &target, command);
    }
    if gltf {
        commands::offer(ui, state, Action::ExportGltf, &target, command);
        commands::offer(ui, state, Action::ImportGltf, &target, command);
    }
}

/// The list at the strip's end of the tabs `hidden` names, which do not fit.
fn list(ui: &mut egui::Ui, tabs: &[Tab], hidden: &[usize], asked: &mut Asked) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let opener = format!("{} {}", ph::CARET_DOWN, hidden.len());
        ui.menu_button(opener, |ui| {
            crate::ui::popup_of_the_shell(ui.ctx());
            for &i in hidden {
                let tab = &tabs[i];
                let unsaved = if tab.unsaved { " ●" } else { "" };
                let mut text = egui::RichText::new(format!("{} {}{unsaved}", tab.glyph, tab.title));
                if tab.preview {
                    text = text.italics();
                }
                let row = ui.add(egui::Button::selectable(tab.active, text));
                if row.on_hover_text(&tab.hover).clicked() {
                    asked.activate = Some(tab.id);
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        })
        .response
        .on_hover_text(tr!("{count} tab(s) that do not fit", count = hidden.len()));
    });
}

fn apply(state: &mut AppState, ctx: &egui::Context, asked: Asked) {
    if let Some(id) = asked.keep {
        state.keep_tab(id);
    }
    if let Some(place) = asked.place {
        state.go_to_place(place);
    }
    if let Some(id) = asked.activate {
        state.set_active_document(id);
    }
    if let Some(id) = asked.close {
        state.close_document(id);
    }
    if let Some(command) = asked.command {
        commands::run(state, ctx, &command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::compare::{Comparison, Found};
    use crate::state::{DocPayload, Document};

    #[test]
    fn every_kind_of_file_has_one_primary_command_and_the_bar_names_it_for_the_tab_in_front() {
        // The table is total: a kind added to the editor has a primary command without a line here.
        for info in crate::state::kinds::all() {
            let action = primary_of(info.kind);
            assert!(
                commands::spec(action).is_some(),
                "{:?}: {action:?} is no command of the table",
                info.kind
            );
        }

        let mut state = AppState::default();
        for (kind, wanted) in [
            (DocumentKind::Text, "save"),
            (DocumentKind::Rpf, "go_to_file"),
            (DocumentKind::Ypt, "save"),
        ] {
            let doc = state.new_empty_document(kind);
            state.set_active_document(doc);
            let bar = context(&state).unwrap_or_else(|| panic!("{kind:?}: no bar"));
            let primary = bar.primary.map(|command| command.id);
            assert_eq!(primary.as_deref(), Some(wanted), "{kind:?}");
            assert!(bar.more.len() <= MORE, "{kind:?}: {} more", bar.more.len());
            assert!(
                bar.more.iter().all(|command| command.id != wanted),
                "{kind:?}: the primary command is offered twice"
            );
        }

        // A screen of the project draws no document: the window's Build is the overview's primary action, and a content's screen has its own header.
        state.project_path = Some(std::path::PathBuf::from("C:/mods/pack.openrage-proj"));
        state.active_workspace = crate::ui::dock::Workspace::Project;
        assert!(
            context(&state).is_none(),
            "a screen of the project has no bar"
        );
    }

    #[test]
    fn the_overview_of_a_project_is_a_tab_and_while_it_is_on_show_no_files_tab_is() {
        use crate::ui::dock::Workspace;

        let mut state = AppState::default();
        let doc = state.new_empty_document(DocumentKind::Text);
        state.set_active_document(doc);
        assert!(screens(&state).is_empty(), "no project, no screen of one");
        assert!(describe(&state, doc).expect("a tab").active);

        state.project_path = Some(std::path::PathBuf::from("C:/mods/pack.openrage-proj"));
        state.active_workspace = Workspace::Project;
        let tabs = screens(&state);
        assert_eq!(tabs.len(), 1, "a project with no content has its overview");
        assert!(tabs[0].on_show);
        assert!(
            !describe(&state, doc).expect("a tab").active,
            "the file's tab reads as on show under the overview"
        );

        // The file's tab brings its editor back and the overview is a tab still.
        state.set_active_document(doc);
        assert_eq!(state.active_workspace, Workspace::Data);
        assert!(!screens(&state)[0].on_show);
        assert!(describe(&state, doc).expect("a tab").active);
    }

    #[test]
    fn a_comparisons_tab_says_what_it_compares_where_a_path_would_go() {
        let mut state = AppState::default();
        state.documents.clear();
        let comparison = Comparison {
            headings: ["a.ymt".into(), "b.ymt".into()],
            places: [r"C:\mods\a.ymt".into(), r"C:\mods\b.ymt".into()],
            found: Found::Lines(Vec::new()),
        };
        let mut doc = Document::new(7, None, DocPayload::Compare(comparison));
        doc.name = "a.ymt ↔ b.ymt".into();
        state.documents.push(doc);

        let hover = describe(&state, 7).expect("a tab").hover;
        assert!(
            !hover.contains("(unsaved)"),
            "a comparison is never saved, so it has no path to lack: {hover}"
        );
        assert!(
            hover.contains(r"C:\mods\a.ymt") && hover.contains(r"C:\mods\b.ymt"),
            "the hover does not name both files: {hover}"
        );
    }
}
