//! Choosing what a content needs and does not own: the picker's state, the three answers it takes, and what each does.
//!
//! A project holds only sources, so a file a content names that is not a source of the project is a reference, and a
//! reference is one of three things: a source or file of this project, a file of another place that the person chooses to
//! import or only to reference, or an id typed for a file they do not have. This is every kind's way to name one, and the
//! Wardrobe's Link cloth, Replace model, Replace texture, Add texture and Locate are its first users.
//!
//! ⚠ **The rules are `or-wardrobe`'s and `or-project`'s.** This asks, lists the candidates the scan already read, and hands
//! the answer to the engine on the Wardrobe's worker, as every other edit of a pack does.

use std::path::{Path, PathBuf};

use or_project::reference::{asset_id, holder, Reference};
use or_project::{project_folder, relative_inside};
use or_wardrobe::edit::Linked;
use or_wardrobe::{replace, variations};

use super::wardrobe::{variation_letter, Change, Done};
use super::{AssetRow, Kept, Missing, Question};
use crate::state::AppState;

/// What a picker is choosing a file for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanting {
    /// The cloth of a garment.
    Cloth { module: String, garment: String },
    /// The model of a garment.
    Model { module: String, garment: String },
    /// Texture `variation` of a garment.
    Texture {
        module: String,
        garment: String,
        variation: usize,
    },
    /// A texture added to a garment as its next.
    NewTexture { module: String, garment: String },
    /// What a Problem says is not there, put right.
    Locate {
        module: String,
        garment: String,
        what: Missing,
    },
}

/// What kind of file a [`Wanting`] is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Cloth,
    Model,
    Texture,
}

impl Part {
    /// The word a sentence uses for it.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Cloth => "cloth",
            Self::Model => "model",
            Self::Texture => "texture",
        }
    }

    /// The kind of source it may be, none where it has no source form.
    pub fn source(self) -> Option<&'static str> {
        match self {
            Self::Cloth => None,
            Self::Model => Some(or_source::models::DRAWABLES),
            Self::Texture => Some(or_source::textures::KIND),
        }
    }

    /// The extensions of the files it may be, which a dialog filters by.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Cloth => &["yld"],
            Self::Model => &["ydd"],
            Self::Texture => &[
                "ytd", "png", "tga", "tif", "tiff", "bmp", "jpg", "jpeg", "dds",
            ],
        }
    }
}

impl Wanting {
    pub fn part(&self) -> Part {
        match self {
            Self::Cloth { .. } => Part::Cloth,
            Self::Model { .. } => Part::Model,
            Self::Texture { .. } | Self::NewTexture { .. } => Part::Texture,
            Self::Locate { what, .. } => match what {
                Missing::Texture(..) => Part::Texture,
                Missing::Cloth => Part::Cloth,
                Missing::Model => Part::Model,
            },
        }
    }

    /// What the file is wanted as, in a sentence's words: `the model`, `texture B`, `a new texture`.
    pub fn role(&self) -> String {
        match self {
            Self::NewTexture { .. } => tr!("a new texture").to_string(),
            Self::Texture { variation, .. }
            | Self::Locate {
                what: Missing::Texture(variation, _),
                ..
            } => format!(
                "texture {}",
                variation_letter(*variation).to_ascii_uppercase()
            ),
            _ => format!("the {}", self.part().noun()),
        }
    }

    /// The window's title.
    pub fn title(&self) -> String {
        match self {
            Self::Locate { .. } => tr!("Locate the {noun}", noun = self.part().noun()),
            _ => tr!("Choose the {noun}", noun = self.part().noun()),
        }
    }

    fn module(&self) -> &str {
        match self {
            Self::Cloth { module, .. }
            | Self::Model { module, .. }
            | Self::Texture { module, .. }
            | Self::NewTexture { module, .. }
            | Self::Locate { module, .. } => module,
        }
    }

    fn garment(&self) -> &str {
        match self {
            Self::Cloth { garment, .. }
            | Self::Model { garment, .. }
            | Self::Texture { garment, .. }
            | Self::NewTexture { garment, .. }
            | Self::Locate { garment, .. } => garment,
        }
    }
}

/// The ranks of a candidate in the list: the garment's own file, the files of garments of its ped, the rest.
const CURRENT: u8 = 0;
const SAME_PED: u8 = 1;
const OTHERS: u8 = 2;

/// A source or file of this project that a picker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    /// Where it is in the project, and what it is.
    pub detail: String,
    pub reference: Reference,
    /// Where it stands in the list: the garment's own file, the files of garments of its ped, then the rest.
    pub rank: u8,
}

/// The picker: what it is for, what is typed in it, and what it offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub wanting: Wanting,
    pub query: String,
    /// The id typed for a file nobody has.
    pub id: String,
    /// The row the person chose by a click or the arrow keys; none until they do, so Enter never answers for them.
    pub selected: Option<usize>,
    /// The name of the garment it is asked for, which its title says.
    pub garment: String,
    /// This project's own contents, first.
    pub mine: Vec<Candidate>,
    /// Files opened lately that could be the one.
    pub recent: Vec<PathBuf>,
    /// The search field takes the keyboard when the window opens.
    pub focus: bool,
}

/// What a picker's answer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// A source or file of this project.
    Mine(Reference),
    /// A file somewhere else, which is to be imported or only referenced.
    Outside(PathBuf),
    /// An id typed by hand.
    Id(String),
}

impl Picker {
    /// The candidates and the recent files the search words keep, in the order the window lists them: the project's first.
    pub fn shown(&self) -> (Vec<&Candidate>, Vec<&PathBuf>) {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let has = |text: &str| {
            let text = text.to_lowercase();
            words.iter().all(|w| text.contains(w))
        };
        (
            (self.mine.iter())
                .filter(|c| has(&c.label) || has(&c.detail))
                .collect(),
            self.recent
                .iter()
                .filter(|p| has(&p.to_string_lossy()))
                .collect(),
        )
    }

    /// How many rows the window lists.
    pub fn rows(&self) -> usize {
        let (mine, recent) = self.shown();
        mine.len() + recent.len()
    }

    /// The answer the row `selected` stands for, the id typed when the search is empty and nothing is listed, none when there is nothing to answer.
    pub fn answer(&self) -> Option<Choice> {
        let (mine, recent) = self.shown();
        match self.selected? {
            n if n < mine.len() => Some(Choice::Mine(mine[n].reference.clone())),
            n => recent
                .get(n - mine.len())
                .map(|p| Choice::Outside((*p).clone())),
        }
    }

    /// Move the selection by `by` rows, staying on the list.
    pub fn step(&mut self, by: isize) {
        let rows = self.rows();
        self.selected = match (rows, self.selected) {
            (0, _) => None,
            // The first arrow key lands on the first row, or the last when it goes up.
            (n, None) => Some(if by > 0 { 0 } else { n - 1 }),
            (n, Some(at)) => Some((at as isize + by).clamp(0, n as isize - 1) as usize),
        };
    }
}

/// Whether `row` can be what `part` wants: a source of its kind, or a loose file with one of its extensions.
fn fits(row: &AssetRow, part: Part) -> bool {
    match (row.kept, part.source()) {
        (Kept::Source, Some(kind)) => row.source_kind.as_deref() == Some(kind),
        (Kept::File, _) => has_extension(&row.path, part.extensions()),
        _ => false,
    }
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|w| e.eq_ignore_ascii_case(w)))
}

impl AppState {
    /// The name of the garment `wanting` is for, as the pack's screens give it.
    pub fn garment_wanted(&self, wanting: &Wanting) -> String {
        self.garment_name(wanting.module(), wanting.garment())
    }

    /// The sources and files of this project that `wanting` could be, named as a reference to them, the garment's own first.
    fn candidates_for(&self, wanting: &Wanting) -> Vec<Candidate> {
        let part = wanting.part();
        let Some(project) = self.project_path.as_deref().map(project_folder) else {
            return Vec::new();
        };
        // What the garment wanted for has now, and which ped it dresses: its own file comes first and the files of its ped's garments after it.
        let wanted = (self.workbench.packs().iter())
            .filter(|p| p.module == wanting.module())
            .find_map(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(wanting.garment()));
        let own = wanted.and_then(|g| match wanting {
            Wanting::Cloth { .. } => g.cloth.clone(),
            Wanting::Model { .. } => g.model.clone(),
            Wanting::Texture { variation, .. } => g.variations.get(*variation)?.texture.clone(),
            Wanting::NewTexture { .. } | Wanting::Locate { .. } => None,
        });
        let ped = wanted.and_then(|g| g.ped);
        let mut found: Vec<Candidate> = Vec::new();
        // ⚠ Only what is there: a model the garment names that is not a file is the very thing the picker is asked to put right.
        let mut push = |label: String, path: &Path, id: Option<&str>, lead: &str, rank: u8| {
            if !path.exists() {
                return;
            }
            if let Some(inside) = relative_inside(path, &project) {
                let folder = inside.rsplit_once('/').map_or("", |(folder, _)| folder);
                let detail = [lead, folder]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("  ·  ");
                found.push(Candidate {
                    label,
                    detail,
                    reference: Reference::here(&inside, id),
                    rank,
                });
            }
        };
        for row in self.workbench.rows().iter().filter(|r| fits(r, part)) {
            push(
                row.name.clone(),
                &row.path,
                row.id.as_deref(),
                &row.kind,
                OTHERS,
            );
        }
        // What a garment of a pack names is a file or a source of the project, though no row of the Assets table lists it: the garment's row stands for it, so it is listed by the garment's name and the file's own name is the small line.
        for pack in self.workbench.packs() {
            let Ok(read) = &pack.read else { continue };
            for garment in &read.wardrobe.garments {
                let names: Vec<(&str, String)> = match part {
                    Part::Cloth => (garment.cloth.iter())
                        .map(|name| (name.as_str(), String::new()))
                        .collect(),
                    Part::Model => (garment.model.iter().map(|name| (name, String::new())))
                        .chain(
                            (garment.alternates.iter().enumerate())
                                .map(|(n, name)| (name, tr!(", alternate {n}", n = n + 1))),
                        )
                        .map(|(name, said)| (name.as_str(), said))
                        .collect(),
                    Part::Texture => (garment.variations.iter().enumerate())
                        .filter_map(|(n, v)| {
                            let letter = variation_letter(n).to_ascii_uppercase();
                            Some((
                                v.texture.as_deref()?,
                                tr!(", texture {letter}", letter = letter),
                            ))
                        })
                        .collect(),
                };
                let of = or_wardrobe::rules::name_of(garment);
                for (name, said) in names {
                    if or_project::reference::is_reference(name) {
                        continue;
                    }
                    let at = pack
                        .folder
                        .join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
                    let leaf = name.rsplit('/').next().unwrap_or(name);
                    let stem = (leaf.strip_suffix(".src"))
                        .and_then(|folder| folder.rsplit_once('.'))
                        .map_or(leaf, |(stem, _)| stem);
                    // ⚠ A garment with no name of its own is called by its file, which is then said once.
                    let base = if of == leaf || of == stem {
                        stem
                    } else {
                        of.as_str()
                    };
                    let theirs = pack.module == wanting.module() && Some(name) == own.as_deref();
                    let (rank, label) = match (theirs, ped.is_some() && garment.ped == ped) {
                        (true, _) => (CURRENT, format!("{base}{said} (current)")),
                        (false, true) => (SAME_PED, format!("{base}{said}")),
                        (false, false) => (OTHERS, format!("{base}{said}")),
                    };
                    let lead = if stem == base { "" } else { stem };
                    push(label, &at, asset_id(&at).as_deref(), lead, rank);
                }
            }
        }
        found.sort_by_key(|c| (c.rank, c.label.to_lowercase()));
        found
    }

    /// The files opened lately that `part` could be.
    fn recent_for(&self, part: Part) -> Vec<PathBuf> {
        (self.prefs.recent_files.iter())
            .filter(|r| r.inner.is_empty())
            .map(|r| PathBuf::from(&r.source))
            .filter(|p| has_extension(p, part.extensions()) && p.exists())
            .take(12)
            .collect()
    }

    /// Open the picker for `wanting`: this project's contents first, then recent files, a way to look elsewhere and a field for an id.
    pub fn ask_for_a_file(&mut self, wanting: Wanting) {
        if self.project_path.is_none() {
            self.show_info_toast(tr!("Open or create a project first"));
            return;
        }
        let part = wanting.part();
        let garment = self.garment_name(wanting.module(), wanting.garment());
        let picker = Picker {
            mine: self.candidates_for(&wanting),
            recent: self.recent_for(part),
            wanting,
            query: String::new(),
            id: String::new(),
            selected: None,
            garment,
            focus: true,
        };
        self.workbench.asking = Some(Question::Pick(Box::new(picker)));
    }

    /// Close the picker without an answer.
    pub fn dismiss_picker(&mut self) {
        if matches!(self.workbench.asking, Some(Question::Pick(_))) {
            self.workbench.asking = None;
        }
    }

    /// Answer the picker: close it and do what the answer means for what it was choosing.
    pub fn choose(&mut self, wanting: Wanting, choice: Choice) {
        self.workbench.asking = None;
        match choice {
            Choice::Mine(reference) => self.refer(wanting, reference),
            Choice::Id(id) => match Reference::by_id(&id).check() {
                Ok(()) => self.refer(wanting, Reference::by_id(&id)),
                Err(why) => self.show_warning_toast(why),
            },
            Choice::Outside(path) => self.choose_outside(wanting, path),
        }
    }

    /// A file chosen elsewhere: one inside this project is this project's, any other asks whether to import it or only to reference it.
    fn choose_outside(&mut self, wanting: Wanting, path: PathBuf) {
        let Some(project) = self.project_path.as_deref().map(project_folder) else {
            return;
        };
        if let Some(inside) = relative_inside(&path, &project) {
            let id = asset_id(&path);
            return self.refer(wanting, Reference::here(&inside, id.as_deref()));
        }
        let whose = holder(path.parent().unwrap_or(&path)).map(|folder| {
            folder
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        });
        self.workbench.asking = Some(Question::ImportOrReference {
            wanting,
            path,
            whose,
        });
    }

    /// Answer the question a file of another place asked: import it into the project, or only reference it.
    pub fn answer_import_or_reference(&mut self, import: bool) {
        let Some(Question::ImportOrReference { wanting, path, .. }) = self.workbench.asking.take()
        else {
            return;
        };
        match import {
            true => self.import_for(wanting, path),
            false => {
                let Some(project) = self.project_path.as_deref().map(project_folder) else {
                    return;
                };
                let reference = reference_to(&path, &project);
                self.refer(wanting, reference);
            }
        }
    }

    /// Close the question without doing either.
    pub fn dismiss_import_or_reference(&mut self) {
        if matches!(
            self.workbench.asking,
            Some(Question::ImportOrReference { .. })
        ) {
            self.workbench.asking = None;
        }
    }

    /// Bring the file `path` into the project for what `wanting` is for: a game file is decompiled, and a cloth, which has no source form, is kept as it is.
    fn import_for(&mut self, wanting: Wanting, path: PathBuf) {
        match wanting {
            Wanting::Cloth { module, garment } => self.link_cloth_in(module, garment, path),
            Wanting::Model { garment, .. } => self.replace_model(garment, path),
            Wanting::Texture {
                garment, variation, ..
            } => self.replace_texture(garment, variation, path),
            Wanting::NewTexture { garment, .. } => self.add_variation(garment, path),
            Wanting::Locate {
                module,
                garment,
                what,
            } => self.locate_file(module, garment, what, path),
        }
    }

    /// Name `reference` for what `wanting` is for: the garment is changed to lead to it, as a step Undo takes back.
    pub fn refer(&mut self, wanting: Wanting, reference: Reference) {
        let (module, id) = (wanting.module().to_string(), wanting.garment().to_string());
        if wanting.part() == Part::Cloth {
            return self.link_cloth_to_in(module, id, reference);
        }
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let shown = reference.describe();
        let (job, said, step): (&str, String, (&'static str, String)) = match &wanting {
            Wanting::Cloth { .. }
            | Wanting::Locate {
                what: Missing::Cloth,
                ..
            } => unreachable!("a cloth returned above"),
            Wanting::Model { .. }
            | Wanting::Locate {
                what: Missing::Model,
                ..
            } => (
                tr!("Pointing a model at a reference"),
                tr!(
                    "The model of {name} is now {shown}",
                    name = name,
                    shown = shown
                ),
                (
                    "model",
                    tr!("Pointed the model of {name} at a reference", name = name),
                ),
            ),
            Wanting::Texture { variation, .. } => {
                let letter = variation_letter(*variation);
                (
                    tr!("Pointing a texture at a reference"),
                    tr!(
                        "Texture {letter} of {name} is now {shown}",
                        letter = letter,
                        name = name,
                        shown = shown
                    ),
                    (
                        "texture",
                        tr!(
                            "Pointed texture {letter} of {name} at a reference",
                            letter = letter,
                            name = name
                        ),
                    ),
                )
            }
            Wanting::NewTexture { .. } => (
                tr!("Adding a texture"),
                tr!(
                    "Added {shown} to {name} as a texture",
                    shown = shown,
                    name = name
                ),
                (
                    tr!("texture added"),
                    tr!("Added a texture to {name}", name = name),
                ),
            ),
            Wanting::Locate {
                what: Missing::Texture(variation, _),
                ..
            } => {
                let letter = variation_letter(*variation);
                (
                    tr!("Locating a texture"),
                    tr!(
                        "Texture {letter} of {name} is now {shown}",
                        letter = letter,
                        name = name,
                        shown = shown
                    ),
                    (
                        "locate",
                        tr!(
                            "Located texture {letter} of {name}",
                            letter = letter,
                            name = name
                        ),
                    ),
                )
            }
        };
        self.wardrobe_job(
            job,
            &module,
            Box::new(move || {
                let revision = match &wanting {
                    Wanting::Model { .. }
                    | Wanting::Locate {
                        what: Missing::Model,
                        ..
                    } => replace::model_by_reference(&folder, &id, &reference),
                    Wanting::Texture { variation, .. } => {
                        replace::texture_by_reference(&folder, &id, *variation, &reference)
                    }
                    Wanting::NewTexture { .. } => {
                        variations::add_by_reference(&folder, &id, &reference)
                    }
                    Wanting::Locate {
                        what: Missing::Texture(variation, named),
                        ..
                    } => or_wardrobe::edit::locate_texture_to(
                        &folder, &id, *variation, named, &reference,
                    ),
                    _ => return Done::said(Err(tr!("There is nothing to point at that").into())),
                };
                match revision {
                    Ok(revision) => Done::revised(said, step, Some(revision)),
                    Err(why) => Done::said(Err(why.to_string())),
                }
            }),
        );
    }

    /// Link the cloth `reference` leads to, which stays where it is.
    fn link_cloth_to_in(&mut self, module: String, id: String, reference: Reference) {
        let Some(folder) = self.folder_to_edit(&module) else {
            return;
        };
        let name = self.garment_name(&module, &id);
        let shown = reference.describe();
        self.wardrobe_job(
            tr!("Linking a cloth"),
            &module,
            Box::new(
                move || match or_wardrobe::edit::link_cloth_to(&folder, &id, &reference) {
                    Ok(linked) => linked_said(
                        tr!(
                            "Linked the cloth {shown} to {name}",
                            shown = shown,
                            name = name
                        ),
                        linked,
                    ),
                    Err(why) => Done::said(Err(why.to_string())),
                },
            ),
        );
    }
}

fn linked_said(said: String, linked: Linked) -> Done {
    Done::with(said, Change::Cloth(Box::new(linked)))
}

/// The reference to `path`, a file or a source folder of another place, from the project whose folder is `project`.
///
/// The place is the project that holds it when one does, else the folder it is in, and the id is a source's own.
fn reference_to(path: &Path, project: &Path) -> Reference {
    let beside = path.parent().unwrap_or(path);
    let place = holder(beside).unwrap_or_else(|| beside.to_path_buf());
    let inside = relative_inside(path, &place).unwrap_or_else(|| {
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    });
    Reference::to_other(&place, project, &inside, asset_id(path).as_deref())
}
