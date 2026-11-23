//! One box for files and commands, as VS Code's Quick Open: Go to File finds a
//! file of the install and of the open folders by name, fuzzily, or by the hash
//! of its name, and a `>` turns the box into the command palette.

use crate::state::file_index::{fuzzy_letters, fuzzy_score, Offer, Scope};
use crate::state::navigation::Arrival;
use crate::state::AppState;
use crate::ui::commands::{self, Command};

const QUERY_KEY: &str = "command_palette_query";
const INPUT_ID: &str = "command_palette_query_input";
/// What a query starts with to name a command rather than a file.
const COMMANDS: char = '>';
/// How many files the box lists at once.
const SHOWN: usize = 100;
/// How many files opened from the box it remembers.
const REMEMBERED: usize = 50;
/// How often the files listed are found again while only the index grows: finding them reads
/// every file of a whole install, too long for every frame of a walk.
const REFIND: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    /// The row Enter runs, which the arrow keys move.
    highlighted: usize,
    /// The ids of the commands run from here, the latest first.
    recent: Vec<String>,
    /// The files opened or revealed from here, the latest first.
    recent_files: Vec<Offer>,
    scope: Scope,
    /// The query the box opens with, put in on the next frame it draws.
    opening: Option<&'static str>,
    /// The files listed, what they were found for, and when.
    offers: Option<(Asked, std::time::Instant, Vec<Offer>)>,
}

/// What a list of files was found for; a change to any of it finds them again.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    query: String,
    scope: Scope,
    files: usize,
    generations: u64,
}

impl crate::ui::panel_state::PanelState for CommandPalette {
    const KEY: &'static str = "command-palette";
}

/// The command palette: the box, opened on `>`.
pub fn open(state: &mut AppState) {
    open_with(state, ">");
}

/// Go to File: the box, opened empty.
pub fn go_to_file(state: &mut AppState) {
    open_with(state, "");
    // Each opening looks again for what changed on disk outside the tree.
    state.file_index.recheck = true;
}

fn open_with(state: &mut AppState, query: &'static str) {
    let palette = state.panels.get::<CommandPalette>();
    palette.open = true;
    palette.opening = Some(query);
    palette.highlighted = 0;
    palette.offers = None;
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let query_id = egui::Id::new(QUERY_KEY);
    if !state.panels.get::<CommandPalette>().open {
        ctx.data_mut(|d| d.remove::<String>(query_id));
        return;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.panels.get::<CommandPalette>().open = false;
        return;
    }

    if let Some(query) = state.panels.get::<CommandPalette>().opening.take() {
        ctx.data_mut(|d| d.insert_temp(query_id, query.to_string()));
        let input = egui::Id::new(INPUT_ID);
        let mut edit =
            egui::widgets::text_edit::TextEditState::load(ctx, input).unwrap_or_default();
        let end = egui::text::CCursor::new(query.chars().count());
        edit.cursor
            .set_char_range(Some(egui::text::CCursorRange::one(end)));
        edit.store(ctx, input);
    }
    let query: String = ctx.data(|d| d.get_temp(query_id)).unwrap_or_default();
    let title = if query.starts_with(COMMANDS) {
        tr!("Command palette")
    } else {
        tr!("Go to file")
    };

    let mut keep_open = true;
    egui::Window::new(title)
        .id(egui::Id::new("command_palette"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
        .default_width(560.0)
        .open(&mut keep_open)
        .show(ctx, |ui| draw_body(ui, state));
    if !keep_open {
        state.panels.get::<CommandPalette>().open = false;
    }
}

fn draw_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.set_min_width(540.0);

    let query_id = egui::Id::new(QUERY_KEY);
    let mut query: String = ui
        .data(|d| d.get_temp::<String>(query_id))
        .unwrap_or_default();

    let edit = egui::TextEdit::singleline(&mut query)
        .id(egui::Id::new(INPUT_ID))
        .hint_text(tr!("Type a file's name or a hash; > for commands"))
        .desired_width(f32::INFINITY);
    let response = ui.add(edit);
    if response.gained_focus() || ui.memory(|m| m.focused() != Some(response.id)) {
        response.request_focus();
    }
    ui.data_mut(|d| d.insert_temp(query_id, query.clone()));
    if response.changed() {
        state.panels.get::<CommandPalette>().highlighted = 0;
    }

    match query.strip_prefix(COMMANDS) {
        Some(command) => draw_commands(ui, state, command),
        None => {
            draw_files(ui, state, &query, true, ROWS_TALL);
        }
    }
}

/// The highlighted row once the arrow keys moved it, and whether Enter was pressed, with Shift or not.
fn highlight(
    ui: &egui::Ui,
    state: &mut AppState,
    rows: usize,
    keys: bool,
) -> (usize, bool, Option<bool>) {
    // ⚠ Only while the field the rows are under has the keys: Enter typed elsewhere in the window opens nothing.
    let (down, up, enter, shift) = ui.input(|i| {
        if !keys {
            return (false, false, false, false);
        }
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
            i.modifiers.shift,
        )
    });
    let palette = state.panels.get::<CommandPalette>();
    if down {
        palette.highlighted += 1;
    }
    if up {
        palette.highlighted = palette.highlighted.saturating_sub(1);
    }
    palette.highlighted = palette.highlighted.min(rows.saturating_sub(1));
    (palette.highlighted, down || up, enter.then_some(shift))
}

fn draw_commands(ui: &mut egui::Ui, state: &mut AppState, query: &str) {
    ui.separator();

    // The first call reads the plugins directory.
    state.installed_plugins();
    let recent = state.panels.get::<CommandPalette>().recent.clone();
    let rows = ranked(commands::all(state), query, &recent);
    if rows.is_empty() {
        ui.label(
            egui::RichText::new(tr!("No commands match “{query}”", query = query.trim()))
                .color(crate::ui::theme::TEXT_SECONDARY),
        );
        return;
    }
    let (highlighted, moved, enter) = highlight(ui, state, rows.len(), true);

    let mut to_run = None;
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for (i, command) in rows.iter().enumerate() {
                let row = draw_row(ui, command, i == highlighted);
                if i == highlighted && moved {
                    row.scroll_to_me(None);
                }
                if row.clicked() {
                    to_run = Some(i);
                }
            }
        });
    let chosen = &rows[highlighted];
    match chosen.enabled {
        Ok(()) if enter.is_some() => to_run = Some(highlighted),
        Err(why) => {
            ui.label(egui::RichText::new(why).color(crate::ui::theme::TEXT_SECONDARY));
        }
        Ok(()) => {}
    }

    if let Some(command) = to_run.map(|i| &rows[i]) {
        let palette = state.panels.get::<CommandPalette>();
        palette.open = false;
        palette.recent.retain(|id| *id != command.id);
        palette.recent.insert(0, command.id.clone());
        commands::run(state, ui.ctx(), command);
    }
}

/// The most room the rows of a list take before they scroll.
const ROWS_TALL: f32 = 360.0;

/// The scopes, how far the index has come, and the files `query` names with the highlighted one first to open, in a list at most `tall`; `keys` says whether the arrows and Enter are this list's. Returns whether a file was chosen, and whether a scope was.
fn draw_files(
    ui: &mut egui::Ui,
    state: &mut AppState,
    query: &str,
    keys: bool,
    tall: f32,
) -> (bool, bool) {
    let mut scoped = false;
    ui.horizontal(|ui| {
        let palette = state.panels.get::<CommandPalette>();
        for scope in Scope::ALL {
            let chosen = ui
                .selectable_label(palette.scope == scope, scope.label())
                .on_hover_text(scope.hover());
            if chosen.clicked() {
                palette.scope = scope;
                palette.highlighted = 0;
                scoped = true;
            }
        }
    });
    ui.separator();

    state.tick_file_index();
    draw_index_status(ui, state);
    let offers = offers(ui.ctx(), state, query);
    if offers.is_empty() {
        let hint = if query.trim().is_empty() {
            let example = or_io::hash::string_hash(b"adder");
            tr!(
                "Type a file's name, such as adder, or the hash of one, such as {example}",
                example = format!("0x{example:08X}")
            )
        } else if state.file_index.is_walking() {
            tr!("Nothing matches yet").to_string()
        } else {
            tr!("No file matches").to_string()
        };
        ui.label(egui::RichText::new(hint).color(crate::ui::theme::TEXT_SECONDARY));
        // ⚠ Enter leaves a field of one line: with nothing to open yet the keys go back to it, so Enter opens once the index has caught up.
        let entered = keys && ui.input(|i| i.key_pressed(egui::Key::Enter));
        return (false, scoped || entered);
    }
    let (highlighted, moved, enter) = highlight(ui, state, offers.len(), keys);

    let mut chosen = enter.map(|reveal| (highlighted, reveal));
    egui::ScrollArea::vertical()
        // ⚠ An id of its own: under the field of Files the tree's scroll area is drawn in the same panel, and two of one id share their scroll.
        .id_salt("go_to_file_rows")
        .max_height(tall)
        .show(ui, |ui| {
            for (i, offer) in offers.iter().enumerate() {
                let row = file_row(ui, offer, i == highlighted);
                if i == highlighted && moved {
                    row.scroll_to_me(None);
                }
                if row.clicked() {
                    chosen = Some((i, ui.input(|input| input.modifiers.shift)));
                }
            }
        });
    ui.label(
        egui::RichText::new(tr!("Enter opens · Shift+Enter reveals in Files"))
            .small()
            .color(crate::ui::theme::TEXT_SECONDARY),
    );

    let chose = chosen.is_some();
    if let Some((row, reveal)) = chosen {
        go(state, offers[row].clone(), reveal);
    }
    (chose, scoped)
}

/// How far the index has come, where its results are listed.
fn draw_index_status(ui: &mut egui::Ui, state: &AppState) {
    let index = &state.file_index;
    let files = index.files();
    let secondary = |text: String| {
        egui::RichText::new(text)
            .small()
            .color(crate::ui::theme::TEXT_SECONDARY)
    };
    if index.roots.is_empty() {
        ui.label(secondary(
            tr!("Nothing to look through: choose a game install (Settings ▸ Games) or open a folder")
                .to_string(),
        ));
    } else if index.is_walking() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(secondary(tr!(
                "Reading the archives: {files} files so far",
                files = files
            )));
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    } else {
        let unread: Vec<&String> = index.warnings().collect();
        ui.horizontal(|ui| {
            ui.label(secondary(tr!("{files} files", files = files)));
            if !unread.is_empty() {
                let why = tr!("{count} could not be read", count = unread.len());
                let lines: Vec<&str> = unread.iter().take(20).map(|w| w.as_str()).collect();
                ui.label(
                    egui::RichText::new(why)
                        .small()
                        .color(crate::ui::theme::WARNING),
                )
                .on_hover_text(lines.join("\n"));
            }
        });
    }
}

/// The files `query` finds in the scope, found again when the query or the
/// scope changed, and at most every [`REFIND`] while only the index grew.
fn offers(ctx: &egui::Context, state: &mut AppState, query: &str) -> Vec<Offer> {
    let index = &state.file_index;
    let palette = state.panels.get::<CommandPalette>();
    let asked = Asked {
        query: query.to_string(),
        scope: palette.scope,
        files: index.files(),
        generations: index.generations(),
    };
    if let Some((before, at, offers)) = &palette.offers {
        let same_question = before.query == asked.query && before.scope == asked.scope;
        if *before == asked || (same_question && at.elapsed() < REFIND) {
            if *before != asked {
                ctx.request_repaint_after(REFIND);
            }
            return offers.clone();
        }
    }
    let install = state.prefs.active_game_root().map(|root| root.as_path());
    let found = index.find(query, palette.scope, install, &palette.recent_files, SHOWN);
    palette.offers = Some((asked, std::time::Instant::now(), found.clone()));
    found
}

/// One file: its name, and the folders and archives it is in.
fn file_row(ui: &mut egui::Ui, offer: &Offer, highlighted: bool) -> egui::Response {
    use egui::AtomExt as _;

    let mut atoms = egui::Atoms::new(offer.leaf());
    if !offer.place().is_empty() {
        atoms.push_right(egui::Atom::grow());
        // ⚠ The place is the one that gives way, cut short: with none marked it is the first
        // text, the name, that shrinks, and it wraps a letter to a line.
        atoms.push_right(egui::RichText::new(offer.place()).weak().atom_shrink(true));
    }
    let button = egui::Button::selectable(highlighted, atoms)
        .truncate()
        .min_size(egui::vec2(ui.available_width(), 0.0));
    ui.add(button)
        .on_hover_text(offer.root.join(&offer.path).display().to_string())
}

/// Open `offer` as a double click on its row in Files does, or reveal it there.
fn go(state: &mut AppState, offer: Offer, reveal: bool) {
    let palette = state.panels.get::<CommandPalette>();
    palette.open = false;
    palette.offers = None;
    palette.recent_files.retain(|known| *known != offer);
    palette.recent_files.insert(0, offer.clone());
    palette.recent_files.truncate(REMEMBERED);
    let then = if reveal {
        Arrival::Reveal
    } else {
        Arrival::Open
    };
    state.walk_to(&offer.root, vec![offer.path.clone()], then);
    if reveal {
        crate::ui::panels::search::show_tree(state);
    }
}

/// What came of the list under the field of Files in a frame.
#[derive(Default)]
pub(crate) struct Found {
    /// The finding is over: a file was chosen, or Escape left it.
    pub ended: bool,
    /// A scope was chosen with the pointer, which took the keys from the field: they go back to it.
    pub refocus: bool,
}

/// Go to File starts over under the field of Files: nothing highlighted but the first row, and the files looked for again.
pub(crate) fn begin_finding(state: &mut AppState) {
    let palette = state.panels.get::<CommandPalette>();
    palette.highlighted = 0;
    palette.offers = None;
}

/// The first row is the highlighted one again, which a change of the words asks for.
pub(crate) fn first_row(state: &mut AppState) {
    state.panels.get::<CommandPalette>().highlighted = 0;
}

/// The files `query` names anywhere Go to File looks, under the field of the Files panel.
///
/// While `finding`, which Go to File starts, it is the list of the palette's box in place: the scopes, the files opened lately while nothing is typed, the arrows and Enter while the field has the `keys`, and Escape to leave. Otherwise the words are a filter of the tree, and the list is a count that opens.
///
/// ⚠ Folded while the words are a filter: a row here has the name of the row of the tree under it, and two rows of one name in one panel are what a person, and a test, clicks by mistake.
pub(crate) fn draw_matches_of(
    ui: &mut egui::Ui,
    state: &mut AppState,
    query: &str,
    finding: bool,
    keys: bool,
) -> Found {
    if finding {
        if keys && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Found {
                ended: true,
                refocus: false,
            };
        }
        // ⚠ No taller than half of what the panel has left: the tree under the list keeps the rest.
        let tall = (ui.available_height() * 0.5).clamp(80.0, ROWS_TALL);
        let (chosen, scoped) = draw_files(ui, state, query, keys, tall);
        return Found {
            ended: chosen,
            refocus: scoped,
        };
    }
    if query.trim().is_empty() {
        return Found::default();
    }
    state.tick_file_index();
    let offers = offers(ui.ctx(), state, query);
    let scope = state.panels.get::<CommandPalette>().scope;
    // ⚠ A scope that finds nothing keeps its list, or the chip that widens it again would go with the rows.
    if offers.is_empty() && scope == Scope::All {
        return Found::default();
    }
    let (mut chosen, mut picked) = (None, None);
    let title = tr!("{count} found by name or hash", count = offers.len());
    egui::CollapsingHeader::new(egui::RichText::new(title).small())
        .id_salt("files_matches_of_the_index")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for each in Scope::ALL {
                    let chip = ui
                        .selectable_label(scope == each, each.label())
                        .on_hover_text(each.hover());
                    if chip.clicked() {
                        picked = Some(each);
                    }
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("files_matches_rows")
                .max_height(240.0)
                .show(ui, |ui| {
                    for offer in &offers {
                        let row = file_row(ui, offer, false)
                            .on_hover_text(tr!("Click opens · Shift+click reveals in Files"));
                        if row.clicked() {
                            chosen = Some((offer.clone(), ui.input(|input| input.modifiers.shift)));
                        }
                    }
                });
        });
    if let Some(scope) = picked {
        state.panels.get::<CommandPalette>().scope = scope;
    }
    if let Some((offer, reveal)) = chosen {
        go(state, offer, reveal);
    }
    Found::default()
}

/// One command: a check while it is on, where it sits, its label and its binding.
fn draw_row(ui: &mut egui::Ui, command: &Command, highlighted: bool) -> egui::Response {
    let check = ui.id().with(("palette_check", &command.id));
    let enabled = command.enabled.is_ok();
    let mut atoms = commands::atoms(command);
    atoms.push_left(egui::RichText::new(format!("{}:", command.category)).weak());
    atoms.push_left(egui::Atom::custom(check, egui::vec2(12.0, 0.0)));
    let button = egui::Button::selectable(highlighted, atoms)
        .min_size(egui::vec2(ui.available_width(), 0.0));
    let drawn = ui
        .scope(|ui| {
            if !enabled && highlighted {
                // ⚠ Faded at egui's 0.5 the highlighted row's text falls under 4.5:1, and only the primary
                // colour has the room to stay over it at the opacity that holds: the weak and selected colours do not.
                // Any other disabled row keeps the fade that shows.
                let primary = crate::ui::theme::TEXT_PRIMARY.get();
                let visuals = ui.visuals_mut();
                visuals.disabled_alpha =
                    crate::ui::theme::disabled_alpha_under_highlight(visuals.dark_mode);
                visuals.weak_text_color = Some(primary);
                visuals.selection.stroke.color = primary;
            }
            ui.add_enabled_ui(enabled, |ui| button.atom_ui(ui)).inner
        })
        .inner;
    let mark = match command.checked {
        Some(true) => Some((egui_phosphor::regular::CHECK, ui.visuals().text_color())),
        _ => commands::glyph(&command.id).map(|glyph| (glyph, ui.visuals().weak_text_color())),
    };
    if let (Some((glyph, colour)), Some(rect)) = (mark, drawn.rect(check)) {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(12.0),
            colour,
        );
    }
    commands::hints(drawn.response, command)
}

/// How far apart, in characters of a command's name, the letters of a word may stand for the word to be the command's: a few more than the letters.
const CLOSE: usize = 4;

/// Whether the command named `text` answers `typed`: every word typed is part of its name, or its letters stand close together in it (`svas` for Save As).
///
/// ⚠ Not letters scattered over the whole name: `waive` is there in order in View: Reset active workspace layout, and Enter would run it.
fn answers(typed: &str, letters: &[u8], text: &str) -> bool {
    let name = text.to_lowercase();
    if typed.split_whitespace().all(|word| name.contains(word)) {
        return true;
    }
    let name = name.as_bytes();
    let Some(&first) = letters.first() else {
        return true;
    };
    (0..name.len())
        .filter(|&start| name[start] == first)
        .any(|start| {
            let mut at = start;
            for &letter in letters {
                match (at..name.len()).find(|&i| name[i] == letter) {
                    Some(found) => at = found + 1,
                    None => return false,
                }
            }
            at - start <= letters.len() + CLOSE
        })
}

/// The commands `query` matches, recent ones first and then the best matches,
/// in the table's order among equals; every command for an empty query.
fn ranked(all: Vec<Command>, query: &str, recent: &[String]) -> Vec<Command> {
    let letters = fuzzy_letters(query);
    let typed = query.trim().to_lowercase();
    // The recency, the score, how closely the label is the typed word, the table's order and the command.
    type Scored = (usize, i32, (u8, bool), usize, Command);
    let mut scored: Vec<Scored> = all
        .into_iter()
        .enumerate()
        .filter_map(|(order, command)| {
            let text = format!("{} {} {}", command.category, command.label, command.was);
            let score = fuzzy_score(&letters, &text)?;
            if !answers(&typed, &letters, &text) {
                return None;
            }
            let recency = recent
                .iter()
                .position(|id| *id == command.id)
                .unwrap_or(usize::MAX);
            // ⚠ Among equal scores the command that is named by the typed word comes first, then one that starts with it, and a command that runs before the toggle of a panel: the row Enter runs must be the one the word names.
            let label = command.label.to_lowercase();
            let named = match (typed.is_empty(), label == typed, label.starts_with(&typed)) {
                (false, true, _) => 0,
                (false, _, true) => 1,
                _ => 2,
            };
            Some((
                recency,
                score,
                (named, command.checked.is_some()),
                order,
                command,
            ))
        })
        .collect();
    scored.sort_by(|a, b| {
        (a.0.cmp(&b.0))
            .then(b.1.cmp(&a.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    scored.into_iter().map(|(.., command)| command).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palettes_exit_asks_the_window_to_close() {
        let mut state = AppState::default();
        state.documents.clear();
        let exit = ranked(commands::all(&state), "exit", &[])
            .into_iter()
            .find(|command| command.label == "Exit")
            .expect("the palette lists Exit");
        commands::run(&mut state, &egui::Context::default(), &exit);
        assert!(state.should_exit);
    }

    #[test]
    fn the_command_named_by_the_typed_word_comes_before_the_ones_that_only_hold_it() {
        let mut state = AppState::default();
        state.project_path = Some(std::path::PathBuf::from("C:/mods/police.openrage-proj"));
        let rows = ranked(commands::all(&state), "build", &[]);
        let first = rows.first().expect("a row");
        assert_eq!(
            (first.label.as_str(), first.checked),
            ("Build", None),
            "Enter on the exact word runs `{}`, a row of `{}`",
            first.label,
            first.category
        );
        let build = rows
            .iter()
            .position(|c| c.label == "Build" && c.checked.is_none());
        let add = rows.iter().position(|c| c.label == "Add build…");
        assert!(build < add, "Build is below Add build…");
    }

    /// The rows each text a frame painted was laid out in.
    fn rows_of(output: &egui::FullOutput) -> Vec<(String, usize)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, usize)>) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push((text.galley.text().to_string(), text.galley.rows.len()))
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn a_files_name_stays_whole_on_one_line_however_long_the_folders_it_is_in() {
        let offer = Offer {
            root: std::path::PathBuf::from("C:/GTAV"),
            path: format!(
                "x64e.rpf/levels/gta5/{}/prop_vehicle_generic_doorshut_n.ydr",
                ["a_very_long_folder_inside_an_archive"; 6].join("/")
            ),
        };
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let room = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(540.0, 100.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
                file_row(ui, &offer, false);
            });
        });
        let rows = rows_of(&output);
        let name = rows
            .iter()
            .find(|(text, _)| text.contains("prop_vehicle_generic_doorshut_n.ydr"))
            .unwrap_or_else(|| panic!("the row does not show the file's name: {rows:?}"));
        assert_eq!(
            name,
            &("prop_vehicle_generic_doorshut_n.ydr".to_string(), 1),
            "the name was cut or broken across lines: {rows:?}"
        );
        assert!(
            rows.iter().all(|(_, lines)| *lines == 1),
            "a text of the row runs over several lines: {rows:?}"
        );
    }
}
