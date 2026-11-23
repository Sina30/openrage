//! The Wardrobe panel: its header, the garments of the pack grouped by ped and slot, each row one click target, and the figures and the build under them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use or_wardrobe::edit::Edit;
use or_wardrobe::query::{Query, Subject};
use or_wardrobe::{search, Garment, Ped, Slot, MAX_POSITION};

use super::rows::rows;
use super::words::{distinct_parts, ped_words, slot_order, slot_words};
use super::{chips, header, keys, quick, recover, resolve, strip, Chip, WardrobeUi};
use crate::platform::file_dialogs;
use crate::state::file_index::{fuzzy_letters, fuzzy_score};
use crate::state::front_door::counted;
use crate::state::workbench::{Dressed, PackRead, Place, Severity};
use crate::state::AppState;
use crate::ui::commands::Action;
use crate::ui::theme;

pub(super) const ROW_HEIGHT: f32 = 22.0;

/// Pictures held as textures of the screen before the list lets them go; they are made again when a row asks.
const MAX_TILES: usize = 600;

/// The rows the list keeps where the column is short: the figures under it give way before it does.
const LIST_MIN_ROWS: f32 = 6.0;

/// What the figures of the strip and the build row under the list take, with their margins.
const BELOW_THE_LIST: f32 = 78.0;

/// What a slot's heading says, what resting on it says, and the ped and slot of the garments under it where it has them.
#[derive(Clone, PartialEq)]
pub(super) struct Heading {
    pub words: String,
    pub tip: String,
    pub seat: Option<(Ped, Slot)>,
}

/// A line of the list: a heading, or the garment at an index of the pack.
#[derive(Clone, PartialEq)]
pub(super) enum Line {
    Group(Heading),
    Garment(usize),
}

/// What the findings say of one garment: how many, the worst of them and the first.
pub(super) struct Trouble {
    pub count: usize,
    pub worst: Severity,
    pub first: String,
}

/// The list laid out for the pack, the filters and the findings it was made from, so it is laid out again only when they change.
pub(super) struct Cached {
    key: (String, u64, String, Option<Ped>, Chip, usize, &'static str),
    pub(super) lines: Vec<Line>,
    /// The ids of the garments in the order the list shows them: what the arrow keys walk and a span runs through.
    pub(super) order: Vec<String>,
    pub(super) summary: Summary,
    pub(super) problems: HashMap<String, Trouble>,
    /// What each garment is called in its row, by its index in the pack: what the others of its ped and kind do not say.
    pub(super) labels: Vec<String>,
}

#[derive(Default, Clone, Copy)]
pub(super) struct Summary {
    pub garments: usize,
    pub textures: usize,
    /// Garments whose names left a ped or a slot unsaid.
    pub unread: usize,
    /// Places held for a drawable with no model.
    pub held: usize,
    /// Garments with a ped or a slot unsaid, an error, or a duplicate.
    pub attention: usize,
    /// Garments that ship a model another garment ships.
    pub duplicates: usize,
    /// The garments the filters let through, and their textures: what the counts say while a filter is in force.
    pub listed: usize,
    pub listed_textures: usize,
}

/// What the list scrolls to, and which of its lines it drew last.
#[derive(Default)]
pub(super) struct View {
    /// The garment to bring into sight, when the keys moved to one that is out of it.
    pub scroll_to: Option<String>,
    pub shown: std::ops::Range<usize>,
}

/// How a click on a row changes what is picked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Click {
    /// The garment alone.
    Only,
    /// Ctrl: the garment in or out of the pick.
    Toggle,
    /// Shift: everything from the garment clicked first.
    Span,
}

/// What a click in the list asked for, carried out once the list is drawn.
#[derive(Default)]
pub(super) struct Asked {
    pub choose: Option<(Click, String)>,
    /// A click on a heading: the garments under it.
    pub heading: Option<Vec<String>>,
    pub remove: Option<String>,
    /// A slot to fill with dummies or to sort so that they come last.
    pub places: Option<(Ped, or_wardrobe::Slot, PlaceAsk)>,
    pub unpin: Option<String>,
    /// The garment to copy to the other ped.
    pub duplicate: Option<(String, Ped)>,
    /// The garment to swap places with its neighbour, and which one.
    pub moved: Option<(String, or_wardrobe::edit::Toward)>,
    /// What the menu of garments picked together asked for.
    pub group: Option<(Vec<String>, GroupAsk)>,
    /// What to put on the clipboard, and how a sentence says what it is.
    pub copy: Option<(&'static str, String)>,
    /// The file to show in the file manager.
    pub reveal: Option<PathBuf>,
    /// The garment whose model to look at where it is.
    pub open_model: Option<String>,
    /// The textures the rows drawn want a picture of.
    pub thumbs: Vec<PathBuf>,
    pub pick: bool,
    /// The filters are let go: the text, the ped and the chip.
    pub clear: bool,
}

/// What the menu of a slot's heading asks of its places.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaceAsk {
    Fill,
    Sort,
}

/// What the menu of garments picked together asks of them.
pub(super) enum GroupAsk {
    Remove,
    Ped(Ped),
    Slot(Slot),
    /// Pick the folder their files are written into.
    Export,
}

/// What the rows are drawn from.
pub(super) struct Frame<'a> {
    pub read: &'a PackRead,
    pub folder: &'a Path,
    pub cached: &'a Cached,
    /// The garments picked, in the pack's order.
    pub picked: &'a [String],
    pub linked: bool,
    pub dressed: &'a Dressed,
    /// What the filters in force are, in words, for a list that comes out empty: none while no filter is on.
    pub filters: &'a str,
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    if !state.has_clothing() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    ui.ctx()
        .request_repaint_after(crate::state::workbench::RESCAN_AFTER);
    let mut shown = state.panels.take::<WardrobeUi>();
    // ⚠ Before a picture is painted: a texture let go in the middle of a frame would blank the pictures already placed in it.
    if shown.tiles.len() > MAX_TILES {
        shown.tiles.clear();
    }
    shown.drop = Some((ui.max_rect(), ui.ctx().cumulative_pass_nr()));
    let module = state.wardrobe_module_id().unwrap_or_default();
    // ⚠ Before the header: an Enter that ends a word typed in the filter must not also open a model.
    keys::handle(ui, state, &mut shown, &module);
    header::draw(ui, state, &mut shown, &module);
    if state.wardrobe_is_linked() {
        header::linked_banner(ui, state, &module);
    } else if state.wardrobe_locked_out() {
        recover::held_elsewhere(ui, state);
    }
    // ⚠ The figures give way before the list does: a column too short for six rows and the figures keeps the rows, and the build row stays. An empty pack has no figures: a memory of 0 B and a count of no problems say nothing.
    let bare = header::bare(state);
    let figures =
        !bare && ui.available_height() >= LIST_MIN_ROWS * ROW_HEIGHT + BELOW_THE_LIST + 60.0;
    egui::Panel::bottom(ui.id().with("wardrobe_strip"))
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 3)))
        .show_inside(ui, |ui| {
            header::progress(ui, state);
            if figures {
                strip::draw(ui, state, &module);
            }
            header::build_chooser(ui, state, &module);
        });
    if state.workbench.wardrobe.quick_look {
        egui::Panel::bottom(ui.id().with("wardrobe_quick_look"))
            .resizable(false)
            .show_inside(ui, |ui| quick::draw(ui, state, &mut shown));
    }
    body(ui, state, &mut shown, &module);
    resolve::draw(ui.ctx(), state, &mut shown);
    state.panels.put(shown);
}

fn body(ui: &mut egui::Ui, state: &mut AppState, shown: &mut WardrobeUi, module: &str) {
    let mut asked = Asked::default();
    let linked = state.wardrobe_is_linked();
    // The garments picked and no longer listed, which are let go once the list is drawn.
    let mut unlisted: Option<Vec<String>> = None;
    {
        let Some(pack) = state.workbench.packs().iter().find(|p| p.module == module) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new(tr!("Reading the pack…")).color(theme::TEXT_SECONDARY),
                );
            });
            return;
        };
        let read = match &pack.read {
            Ok(read) => read,
            Err(why) => {
                let (why, good) = (why.clone(), pack.last_good.clone());
                recover::unreadable(ui, state, &why, good.as_ref());
                return;
            }
        };
        let findings = state.workbench.findings();
        refresh(
            shown,
            state.workbench.wardrobe.generation,
            module,
            read,
            findings,
        );
        // ⚠ Only what the list shows is picked: a filter that hides a garment picked earlier lets it go, so that no command acts on one the person cannot see.
        let mut picked = state.picked_garments();
        if picked.len() > 1 {
            if let Some(cached) = shown.cache.as_ref() {
                let listed: HashSet<&str> = cached.order.iter().map(String::as_str).collect();
                if picked.iter().any(|id| !listed.contains(id.as_str())) {
                    unlisted = Some(cached.order.clone());
                    picked.retain(|id| listed.contains(id.as_str()));
                }
            }
        }
        // ⚠ An empty pack is one drop area and nothing else: no counts of nothing, no filters of nobody.
        if read.wardrobe.garments.is_empty() && !linked {
            chips::empty_wardrobe(ui, &mut asked);
        } else {
            if linked {
                chips::linked_note(ui);
            }
            chips::draw(ui, shown, read, &picked, linked);
            let filters = filters_in_force(shown);
            let WardrobeUi {
                cache, tiles, view, ..
            } = &mut *shown;
            let Some(cached) = cache.as_ref() else {
                return;
            };
            let frame = Frame {
                read,
                folder: &pack.folder,
                cached,
                picked: &picked,
                linked,
                dressed: &state.workbench.wardrobe.dressed,
                filters: &filters,
            };
            rows(ui, &frame, tiles, view, &mut asked);
        }
    }
    if let Some(order) = unlisted {
        state.keep_picks_listed(module, &order);
    }
    if asked.clear {
        shown.filter.clear();
        (shown.ped, shown.chip) = (None, Chip::All);
    }
    if asked.pick {
        crate::ui::commands::run_action(state, ui.ctx(), Action::AddClothes);
    }
    if let Some(ids) = asked.heading {
        state.pick_garments(module, ids);
    }
    if let Some((click, id)) = asked.choose {
        match click {
            Click::Only => state.select_garment(module, Some(id)),
            Click::Toggle => state.toggle_garment(module, id),
            Click::Span => {
                let order = (shown.cache.as_ref()).map_or_else(Vec::new, |c| c.order.clone());
                state.span_garments(module, &order, id);
            }
        }
    }
    if let Some(id) = asked.unpin {
        state.edit_garment(id, Edit::Position(None));
    }
    if let Some((ids, ask)) = asked.group {
        match ask {
            GroupAsk::Remove => state.remove_garments(ids),
            GroupAsk::Ped(ped) => state.set_ped_of(ids, Some(ped)),
            GroupAsk::Slot(slot) => state.set_slot_of(ids, Some(slot)),
            GroupAsk::Export => {
                let picked = file_dialogs::pick_folder(tr!("Export Files Into Folder"));
                if let Some(folder) = picked {
                    state.export_garments(ids, folder);
                }
            }
        }
    }
    if let Some((id, ped)) = asked.duplicate {
        state.duplicate_garments(vec![id], ped);
    }
    if let Some((id, toward)) = asked.moved {
        state.move_garment(id, toward);
    }
    if let Some(id) = asked.remove {
        state.edit_garment(id, Edit::Remove);
    }
    match asked.places {
        Some((ped, slot, PlaceAsk::Fill)) => state.fill_slot(ped, slot),
        Some((ped, slot, PlaceAsk::Sort)) => state.sort_slot(ped, slot),
        None => {}
    }
    if let Some((what, text)) = asked.copy {
        ui.ctx().copy_text(text);
        state.show_info_toast(tr!("Copied {what}", what = what));
    }
    if let Some(file) = asked.reveal {
        crate::platform::shell::reveal(&file);
    }
    if let Some(id) = asked.open_model {
        state.open_garment_model(&id);
    }
    state
        .workbench
        .wardrobe
        .dressed
        .ask_row_thumbs(asked.thumbs);
}

/// What the filters in force are, in words: the text typed, the ped chosen and the chip pressed; nothing while none is on.
fn filters_in_force(shown: &WardrobeUi) -> String {
    let mut on: Vec<String> = Vec::new();
    if !shown.filter.trim().is_empty() {
        on.push(format!("\"{}\"", shown.filter.trim()));
    }
    if let Some(ped) = shown.ped {
        on.push(tr!(
            "the {ped} ped",
            ped = ped_words(Some(ped)).to_lowercase()
        ));
    }
    match shown.chip {
        Chip::All => {}
        Chip::Attention => on.push(tr!("the garments that need attention").into()),
        Chip::Duplicates => on.push(tr!("the duplicates").into()),
    }
    on.join(", ")
}

/// The garments an error or a duplicate is about, by id: the engine's findings that no waiver sets aside.
fn marks(read: &PackRead) -> (HashSet<&str>, HashSet<&str>) {
    let (mut errors, mut copies) = (HashSet::new(), HashSet::new());
    for finding in read.findings.iter().filter(|f| f.waived.is_none()) {
        let Some(id) = finding.garment.as_deref() else {
            continue;
        };
        if finding.code == "duplicate-content" {
            copies.insert(id);
        }
        if finding.severity == or_wardrobe::Severity::Error {
            errors.insert(id);
        }
    }
    (errors, copies)
}

/// Lay the list out again when the pack, the filters or the findings are not the ones it was laid out for.
fn refresh(
    shown: &mut WardrobeUi,
    generation: u64,
    module: &str,
    read: &PackRead,
    findings: &[crate::state::workbench::Finding],
) {
    let key = |chip: Chip, shown: &WardrobeUi| {
        (
            module.to_string(),
            generation,
            shown.filter.clone(),
            shown.ped,
            chip,
            findings.len(),
            // The headings and labels are said in the language in use.
            crate::i18n::active().code.as_str(),
        )
    };
    if shown
        .cache
        .as_ref()
        .is_some_and(|c| c.key == key(shown.chip, shown))
    {
        return;
    }
    let mut made = lay_out(read, &shown.filter, shown.ped, shown.chip);
    // The chip that narrows the list is drawn only while it has garments to pick, so it goes with the last of them.
    let empty = match shown.chip {
        Chip::All => false,
        Chip::Attention => made.summary.attention == 0,
        Chip::Duplicates => made.summary.duplicates == 0,
    };
    if empty {
        shown.chip = Chip::All;
        made = lay_out(read, &shown.filter, shown.ped, Chip::All);
    }
    let mut problems: HashMap<String, Trouble> = HashMap::new();
    for finding in findings {
        if let Place::Garment {
            content,
            id: Some(id),
            ..
        } = &finding.place
        {
            if content == module {
                let entry = problems.entry(id.clone()).or_insert_with(|| Trouble {
                    count: 0,
                    worst: finding.severity,
                    first: finding.text.clone(),
                });
                entry.count += 1;
                // The sentence shown is the worst one's, the first of them where two are as bad.
                if finding.severity > entry.worst {
                    entry.worst = finding.severity;
                    entry.first = finding.text.clone();
                }
            }
        }
    }
    let order = (made.lines.iter())
        .filter_map(|line| match line {
            Line::Garment(i) => Some(read.wardrobe.garments[*i].id.clone()),
            Line::Group(..) => None,
        })
        .collect();
    shown.cache = Some(Cached {
        key: key(shown.chip, shown),
        lines: made.lines,
        order,
        summary: made.summary,
        problems,
        labels: made.labels,
    });
}

struct Made {
    lines: Vec<Line>,
    summary: Summary,
    labels: Vec<String>,
}

/// Whether each word of the query matches, as letters in order, one of the words a garment is found by.
///
/// ⚠ Against one word at a time: letters in order across the whole line match nearly anything.
fn found(words: &[Vec<u8>], terms: &[String]) -> bool {
    (words.iter()).all(|letters| (terms.iter()).any(|term| fuzzy_score(letters, term).is_some()))
}

/// The words garment `index` is found by: the engine's, and its ped and slot as the headings say them.
fn terms_of(read: &PackRead, index: usize) -> Vec<String> {
    let mut terms = search::terms(&read.wardrobe, &read.allocation, index);
    let g = &read.wardrobe.garments[index];
    terms.extend(g.slot.map(|slot| slot_words(slot).to_string()));
    terms.extend(g.ped.map(|ped| ped_words(Some(ped)).to_string()));
    terms
}

/// The lines of the list: the garments that pass the filters, under a heading for each ped and slot, Tops first.
fn lay_out(read: &PackRead, filter: &str, ped: Option<Ped>, chip: Chip) -> Made {
    let garments = &read.wardrobe.garments;
    let (errors, copies) = marks(read);
    let mut summary = Summary {
        garments: garments.len(),
        textures: garments.iter().map(|g| g.variations.len()).sum(),
        ..Summary::default()
    };
    // ⚠ A line that does not read as a query selects everything: the bar under the field says which term, and the list does not empty as a term is typed.
    let query = Query::parse(filter).ok();
    let words: Vec<Vec<u8>> = (query.iter())
        .flat_map(Query::words)
        .map(|w| fuzzy_letters(w))
        .collect();
    let left_out: Vec<Vec<u8>> = (query.iter())
        .flat_map(Query::not_words)
        .map(|w| fuzzy_letters(w))
        .collect();
    let subject = Subject::new(
        &read.wardrobe,
        &read.allocation,
        &read.findings,
        &read.counts,
    );
    let mut kept: Vec<usize> = Vec::new();
    for (i, g) in garments.iter().enumerate() {
        let unread = g.ped.is_none() || g.slot.is_none();
        let copy = copies.contains(g.id.as_str());
        let attention = unread || copy || errors.contains(g.id.as_str());
        summary.unread += usize::from(unread);
        summary.held += usize::from(g.is_reserved());
        summary.attention += usize::from(attention);
        summary.duplicates += usize::from(copy);
        if ped.is_some() && g.ped != ped && !unread {
            continue;
        }
        let wanted = match chip {
            Chip::All => true,
            Chip::Attention => attention,
            Chip::Duplicates => copy,
        };
        if !wanted {
            continue;
        }
        if !query.as_ref().is_none_or(|q| q.terms_hold(&subject, i)) {
            continue;
        }
        if !(words.is_empty() && left_out.is_empty()) {
            let terms = terms_of(read, i);
            if !found(&words, &terms)
                || (left_out.iter()).any(|w| found(std::slice::from_ref(w), &terms))
            {
                continue;
            }
        }
        kept.push(i);
    }
    summary.listed = kept.len();
    summary.listed_textures = kept.iter().map(|&i| garments[i].variations.len()).sum();
    let position = |i: usize| read.allocation.of(i).map_or(u32::MAX, |p| p.place);
    kept.sort_by_key(|&i| {
        let g = &garments[i];
        let unread = g.ped.is_none() || g.slot.is_none();
        (!unread, g.ped, slot_order(g.slot), position(i), name_of(g))
    });
    let mut lines = Vec::new();
    type Seat = (Option<Ped>, Option<or_wardrobe::Slot>);
    let mut groups: Vec<(Seat, Vec<usize>)> = Vec::new();
    for i in kept {
        let key = (garments[i].ped, garments[i].slot);
        match groups.last_mut() {
            Some((last, members)) if *last == key => members.push(i),
            _ => groups.push((key, vec![i])),
        }
    }
    for ((ped, slot), members) in groups {
        let garments = counted(members.len(), "garment");
        let heading = match (ped, slot) {
            (Some(ped), Some(slot)) => {
                // A collection holds as many as the cap of the pack, or the game's 255 where the files replace the game's own.
                let cap = match read.wardrobe.replaces(ped) {
                    Some(_) => MAX_POSITION + 1,
                    None => read.wardrobe.rules.slot_cap(),
                } as usize;
                let mut words = tr!(
                    "{ped} · {slot} · {garments}",
                    ped = ped_words(Some(ped)),
                    slot = slot_words(slot),
                    garments = garments
                );
                // ⚠ The cap is said only where it matters: a slot that is nearly full, or already written as several collections.
                match members.len().div_ceil(cap) {
                    0 | 1 if members.len() * 10 >= cap * 9 => {
                        words.push_str(&tr!(" · fills {count} of {cap} places", cap = cap, count = members.len()));
                    }
                    0 | 1 => {}
                    n => words.push_str(&tr!(" · written as {n} collections", n = n)),
                }
                Heading {
                    tip: tr!("Click to select the {garments} under this heading. Right-click, or the button at its end, for what can be done to all of them. They fill {count} of the {cap} places of this slot", garments = garments, cap = cap, count = members.len()),
                    words,
                    seat: Some((ped, slot)),
                }
            }
            _ => Heading {
                words: tr!("? Needs a ped or a slot · {garments}", garments = garments),
                tip: tr!("The names of these {garments} did not say a ped or a slot. Click to select them; Say what they are… sets both", garments = garments),
                seat: None,
            },
        };
        lines.push(Line::Group(heading));
        lines.extend(members.into_iter().map(Line::Garment));
    }
    Made {
        lines,
        summary,
        labels: row_labels(read),
    }
}

/// What each garment is called in its row: its name without what the other garments of its ped and kind say too.
///
/// ⚠ Judged over the whole pack and not the garments the filters keep, so a name does not change as one types in the filter. Props are judged apart from components: a hat's file is named for its collection with a `p_` in front, and one hat among the tops would leave the collection's name on every top.
fn row_labels(read: &PackRead) -> Vec<String> {
    let garments = &read.wardrobe.garments;
    let mut labels: Vec<String> = garments.iter().map(name_of).collect();
    let kind = |g: &Garment| (g.ped, g.slot.is_some_and(Slot::is_prop));
    let mut kinds: Vec<_> = garments.iter().map(kind).collect();
    kinds.sort();
    kinds.dedup();
    for of_kind in kinds {
        let members: Vec<usize> = (0..garments.len())
            .filter(|&i| kind(&garments[i]) == of_kind)
            .collect();
        let names: Vec<&str> = members.iter().map(|&i| name_of_ref(&garments[i])).collect();
        for (i, label) in members.iter().zip(distinct_parts(&names)) {
            labels[*i] = label;
        }
    }
    labels
}

/// What a garment is called in the list: its label, else the file of its model.
fn name_of(g: &Garment) -> String {
    name_of_ref(g).to_string()
}

fn name_of_ref(g: &Garment) -> &str {
    if let Some(label) = &g.label {
        return label;
    }
    match g.model.as_deref().and_then(|m| m.rsplit('/').next()) {
        Some(file) => file,
        None => tr!("(a place held)"),
    }
}
