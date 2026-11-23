//! The New project dialog's model: the templates that exist, what a template writes, and the write.
//!
//! ⚠ **Nothing is written before [`NewProject::create`].** The tree the dialog shows is
//! [`NewProject::tree`], computed from the same [`Writes`] that `create` carries out, so what the
//! dialog shows is what the write makes.

use crate::state::front_door::KindWords as _;
use std::path::{Path, PathBuf};

use or_project::adopt::{Adoption, Written};
use or_project::container::is_name;
use or_project::template::{Template as Shared, EXTENSION, RESOURCE};
use or_project::{Container, Module, ModuleKind, Output, PROJECT_EXTENSION};
use or_source::git;
use serde_json::{Map, Value};

use super::adopt::recognisers;
pub use super::template::{GameChoice, Template};
use crate::state::project::{save_project, ProjectDocument, ProjectFile};
use crate::state::{DocumentKind, Prefs};
use crate::ui::dock::Workspace;

pub use or_project::template::resource_name_for;

/// The folder a project keeps its sources in, made beside its file.
const SOURCES_DIR: &str = "sources";

/// The folder of a clothing pack's content, beside the sources.
const CLOTHING_DIR: &str = "clothing";

/// The folder of a weapon pack's content, beside the sources.
const WEAPONS_DIR: &str = "weapons";
/// The folder of a particle pack's content, beside the sources.
const PARTICLES_DIR: &str = "particles";
/// The folder of a map pack's content, beside the sources.
const MAPS_DIR: &str = "maps";

/// The key of the project file that says which game the project is for.
pub const GAME_KEY: &str = "game";

/// How many template files the folder of saved templates is read for, and how big each may be.
const MAX_TEMPLATES: usize = 50;
const MAX_TEMPLATE_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Gallery,
    Details,
}

/// One line of the tree the dialog shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeLine {
    pub depth: usize,
    pub name: String,
    pub is_folder: bool,
    /// Made by Create; a folder that is already there is shown as it is.
    pub new: bool,
    /// A name in angle brackets that stands for one not chosen yet.
    pub stands_in: bool,
}

/// The field of the details a refusal is about, so the dialog can say it under that field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Place,
    Resource,
    /// Nothing the person typed: the template file chosen.
    Template,
}

/// Why Create cannot run, and which field to say it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub field: Field,
    pub why: String,
}

impl Refusal {
    fn at(field: Field, why: impl Into<String>) -> Option<Self> {
        Some(Self {
            field,
            why: why.into(),
        })
    }
}

/// The workspace a project opens on: `wanted`, except that a task screen belongs to one kind of content, so a project holding several opens on the neutral Project view.
pub fn workspace_for(container: &Container, wanted: Workspace) -> Workspace {
    let mut kinds: Vec<&ModuleKind> = Vec::new();
    for module in &container.modules {
        if !kinds.contains(&&module.kind) {
            kinds.push(&module.kind);
        }
    }
    if wanted.is_task_screen() && kinds.len() > 1 {
        Workspace::Project
    } else {
        wanted
    }
}

/// What Create writes, as the dialog's tree and the write both read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writes {
    /// The project's folder, which is made unless it is there.
    pub folder: PathBuf,
    pub folder_is_new: bool,
    /// Folders made inside it, with `/` between their parts.
    pub inside: Vec<String>,
    /// The folder made inside it that holds an empty clothing pack, with its `wardrobe.json`.
    pub clothing: Option<&'static str>,
    /// The folder made inside it that holds an empty weapon pack, with its `weapons.json`.
    pub weapons: Option<&'static str>,
    /// The folder made inside it that holds an empty particle pack, with its `effects.json`.
    pub particles: Option<&'static str>,
    /// The folder made inside it that holds an empty map pack, with its `maps.json`.
    pub maps: Option<&'static str>,
    pub file: PathBuf,
    /// The two Git files, which hold OpenRage's blocks from the start: written where the folder has neither, since a folder's own are not rewritten at Create.
    pub git: bool,
}

/// What a Create made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub file: PathBuf,
    /// Files an adopted folder gave the project.
    pub adopted: usize,
    /// Part of the folder was left out of the project: past a bound on its files or its depth.
    pub truncated: bool,
    /// What an adopted folder was recognised as, one line each.
    pub found: Vec<String>,
    /// How many files an adopted folder could not place.
    pub unplaced: usize,
}

/// A template file the dialog has read.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub template: Shared,
    /// Where it was read from, none for one that was made in memory.
    pub from: Option<PathBuf>,
    /// One sentence for each thing in the file that was left out.
    pub findings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewProject {
    pub step: Step,
    pub template: Template,
    /// The gallery's search.
    pub search: String,
    pub name: String,
    /// The name the dialog gave, which the next template chosen replaces while it stands.
    given: String,
    /// The folder the project's own folder is made in; for an adopted folder, the folder itself.
    pub place: String,
    /// Where a project is made when no folder was typed.
    default_place: String,
    pub game: GameChoice,
    pub resource: String,
    /// The resource name was typed, so the project's name no longer fills it in.
    pub resource_typed: bool,
    /// Enter was pressed in the name field, which Create answers once its button has been drawn.
    pub enter_confirms: bool,
    /// The name field takes the focus in the next frame.
    pub focus: bool,
    /// Why the last Create did not make the project.
    pub error: Option<String>,
    /// The template files the dialog has read, and the one a [`Template::Shared`] draft makes its project from.
    pub loaded: Vec<Loaded>,
    pub loaded_ix: usize,
    looked_at: Option<Looked>,
}

/// What the dialog last found out about its draft from the disk, so a frame reads none.
#[derive(Debug, Clone)]
struct Looked {
    key: (Template, usize, String, String, String),
    at: std::time::Instant,
    refusal: Option<Refusal>,
    tree: Vec<TreeLine>,
}

/// How long what the dialog found out about the disk is trusted.
const TRUSTED: std::time::Duration = std::time::Duration::from_secs(1);

impl NewProject {
    pub fn new(template: Template, place: PathBuf) -> Self {
        let default_place = place.display().to_string();
        Self {
            step: Step::Gallery,
            template,
            search: String::new(),
            name: String::new(),
            given: String::new(),
            // ⚠ A folder to adopt is never a guess: the default is where a project is made.
            place: if template == Template::Adopt {
                String::new()
            } else {
                default_place.clone()
            },
            default_place,
            game: GameChoice::Both,
            resource: String::new(),
            resource_typed: false,
            enter_confirms: false,
            focus: true,
            error: None,
            loaded: Vec::new(),
            loaded_ix: 0,
            looked_at: None,
        }
    }

    /// Why Create cannot run and the tree it would write, read from the disk once a second and when the draft changes, not every frame.
    pub fn looked_at(&mut self) -> (Option<Refusal>, Vec<TreeLine>) {
        let key = (
            self.template,
            self.loaded_ix,
            self.name.clone(),
            self.place.clone(),
            self.resource.clone(),
        );
        let fresh = self
            .looked_at
            .take()
            .filter(|looked| looked.key == key && looked.at.elapsed() < TRUSTED);
        let looked = fresh.unwrap_or_else(|| Looked {
            key,
            at: std::time::Instant::now(),
            refusal: self.refused(),
            tree: self.tree(),
        });
        let found = (looked.refusal.clone(), looked.tree.clone());
        self.looked_at = Some(looked);
        found
    }

    /// Whether Create can run by the look the messages were last drawn from, which looks at nothing: a look that came due between the messages and the button would move the button alone.
    pub fn clear_by_last_look(&self) -> bool {
        self.looked_at
            .as_ref()
            .is_some_and(|looked| looked.refusal.is_none())
    }

    /// Why Create waits, by the look the messages were last drawn from: the sentence its hover says.
    pub fn why_it_waits(&self) -> Option<&str> {
        let looked = self.looked_at.as_ref()?;
        looked.refusal.as_ref().map(|refusal| refusal.why.as_str())
    }

    /// A project made of `folder` as it is: its own name and a build named after it.
    pub fn adopting(folder: &Path) -> Self {
        let mut draft = Self::new(Template::Adopt, folder.to_path_buf());
        draft.step = Step::Details;
        draft.pick_place(folder);
        draft
    }

    /// Choose another built-in template; the folder follows only while it is the default one or none.
    pub fn choose(&mut self, template: Template) {
        if template == Template::Adopt && self.place == self.default_place {
            self.place.clear();
        } else if self.template == Template::Adopt && self.place.is_empty() {
            self.place.clone_from(&self.default_place);
        }
        self.template = template;
        self.name_after_the_template();
    }

    /// Name the project after its template, numbered until no folder of that name is in the way, so that Create does not wait for a name; a name that was typed stays, and a folder to adopt names the project itself.
    pub fn name_after_the_template(&mut self) {
        if self.name != self.given {
            return;
        }
        if self.template == Template::Adopt {
            self.set_name(String::new());
            self.given.clear();
            return;
        }
        // A template read from a file names the project as the file does.
        let base = (self.shared())
            .map_or(self.template.label(), |loaded| {
                loaded.template.name.as_str()
            })
            .to_string();
        // ponytail: a hundred tries, then the last name stands and Create says why it waits.
        for n in 1..=100 {
            self.set_name(if n == 1 {
                base.clone()
            } else {
                format!("{base} {n}")
            });
            if !(self.refused()).is_some_and(|refusal| refusal.field == Field::Name) {
                break;
            }
        }
        self.given.clone_from(&self.name);
    }

    /// Choose the template file the dialog has read at `ix`; the game follows what it states.
    pub fn choose_loaded(&mut self, ix: usize) {
        let Some(loaded) = self.loaded.get(ix) else {
            return;
        };
        let game = loaded
            .template
            .game
            .as_deref()
            .and_then(GameChoice::from_word);
        // ⚠ Before the template is chosen, which names the project after the file read at this place.
        self.loaded_ix = ix;
        self.choose(Template::Shared);
        if let Some(game) = game {
            self.game = game;
        }
    }

    /// Add a template the dialog has read and choose it.
    pub fn offer(&mut self, loaded: Loaded) {
        let same = |other: &Loaded| other.from.is_some() && other.from == loaded.from;
        let ix = match self.loaded.iter().position(same) {
            Some(ix) => {
                self.loaded[ix] = loaded;
                ix
            }
            None => {
                self.loaded.push(loaded);
                self.loaded.len() - 1
            }
        };
        self.choose_loaded(ix);
    }

    /// Read the template file at `path` into the dialog and choose it.
    pub fn load_template(&mut self, path: &Path) -> Result<(), String> {
        let loaded = read_template(path)?;
        self.offer(loaded);
        Ok(())
    }

    /// The template file a draft of [`Template::Shared`] is made from.
    pub fn shared(&self) -> Option<&Loaded> {
        (self.template == Template::Shared)
            .then(|| self.loaded.get(self.loaded_ix))
            .flatten()
    }

    /// What the gallery says the chosen template is.
    pub fn blurb(&self) -> String {
        match self.shared() {
            Some(loaded) if !loaded.template.description.is_empty() => {
                loaded.template.description.clone()
            }
            _ => self.template.blurb().to_string(),
        }
    }

    /// The content the chosen template starts the project with, in words.
    pub fn content(&self) -> String {
        let Some(loaded) = self.shared() else {
            return self.template.content().to_string();
        };
        let words: Vec<String> = loaded
            .template
            .container
            .modules
            .iter()
            .map(|m| format!("{} ({})", m.kind.words(), m.id))
            .collect();
        if words.is_empty() {
            tr!("None yet").to_string()
        } else {
            words.join(", ")
        }
    }

    /// The builds the chosen template starts the project with, in words.
    pub fn builds(&self) -> String {
        let Some(loaded) = self.shared() else {
            return self.template.builds().to_string();
        };
        let words: Vec<String> = loaded
            .template
            .container
            .outputs
            .iter()
            .map(|o| {
                let resource = if self.resource.is_empty() {
                    tr!("the resource name")
                } else {
                    &self.resource
                };
                tr!(
                    "{kind} named {name}",
                    kind = o.kind.words(),
                    name = o.name.replace(RESOURCE, resource)
                )
            })
            .collect();
        if words.is_empty() {
            tr!("None yet").to_string()
        } else {
            words.join("; ")
        }
    }

    /// Whether the project this draft makes declares a build, so the dialog asks for a resource name.
    pub fn has_a_build(&self) -> bool {
        match self.template {
            Template::Empty => false,
            Template::Files
            | Template::Clothing
            | Template::Weapons
            | Template::Particles
            | Template::Maps
            | Template::Adopt => true,
            Template::Shared => self
                .shared()
                .is_some_and(|loaded| !loaded.template.container.outputs.is_empty()),
        }
    }

    /// Type the project's name; the resource's follows it until the resource is typed itself.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
        if !self.resource_typed {
            self.resource = resource_name_for(&self.name);
        }
    }

    pub fn set_resource(&mut self, resource: String) {
        self.resource_typed = true;
        self.resource = resource;
    }

    /// Choose a folder in the dialog's picker: the place itself, and for an adopted folder also its name.
    pub fn pick_place(&mut self, folder: &Path) {
        self.place = folder.display().to_string();
        if self.template == Template::Adopt && self.name.is_empty() {
            let name = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .map(|name| clean_name(&name))
                .unwrap_or_else(|| "project".to_string());
            self.set_name(name);
        }
    }

    /// The project's folder: the place for an adopted folder, else a folder of its name in the place.
    pub fn folder(&self) -> PathBuf {
        let place = std::path::absolute(&self.place).unwrap_or_else(|_| PathBuf::from(&self.place));
        if self.template.makes_a_folder() {
            place.join(&self.name)
        } else {
            place
        }
    }

    pub fn file(&self) -> PathBuf {
        self.folder()
            .join(format!("{}.{PROJECT_EXTENSION}", self.name))
    }

    /// Why Create cannot run, in a sentence; none when it can.
    pub fn refusal(&self) -> Option<String> {
        self.refused().map(|refusal| refusal.why)
    }

    /// What a clothing pack is called to the person who makes one, who never meets the words project, content or build.
    fn thing(&self) -> &'static str {
        match self.template {
            Template::Clothing => tr!("clothing pack"),
            Template::Weapons => tr!("weapon pack"),
            Template::Particles => tr!("particle pack"),
            Template::Maps => tr!("map pack"),
            _ => tr!("project"),
        }
    }

    /// Why Create cannot run and the field to say it under; none when it can.
    pub fn refused(&self) -> Option<Refusal> {
        let thing = self.thing();
        if self.name.is_empty() {
            return Refusal::at(
                Field::Name,
                match self.template {
                    Template::Adopt => {
                        tr!("Give the project a name; its project file is called after it.")
                            .to_string()
                    }
                    _ => tr!(
                        "Give the {thing} a name; it becomes the folder.",
                        thing = thing
                    ),
                },
            );
        }
        if !is_name(&self.name) {
            return Refusal::at(
                Field::Name,
                tr!("A {thing}'s name is a folder name: no slash, colon or other reserved character, no leading dot and no trailing dot or space", thing = thing),
            );
        }
        if self.template == Template::Shared && self.shared().is_none() {
            return Refusal::at(Field::Template, tr!("Choose a template file"));
        }
        let place = Path::new(&self.place);
        if self.place.trim().is_empty() || !place.is_dir() {
            return Refusal::at(
                Field::Place,
                match self.template {
                    Template::Adopt => {
                        tr!("Choose the folder to make a project of; it must exist").to_string()
                    }
                    _ => tr!(
                        "Choose the folder to make the {thing} in; it must exist",
                        thing = thing
                    ),
                },
            );
        }
        let folder = self.folder();
        if self.template.makes_a_folder() {
            let taken = std::fs::read_dir(&folder)
                .ok()
                .is_some_and(|mut entries| entries.next().is_some());
            if taken {
                let advice = if matches!(
                    self.template,
                    Template::Clothing | Template::Weapons | Template::Particles | Template::Maps
                ) {
                    tr!("choose another name or another folder")
                } else {
                    tr!("Project from a folder makes a project of one")
                };
                return Refusal::at(
                    Field::Name,
                    tr!(
                        "{folder} already exists and is not empty; {advice}",
                        advice = advice,
                        folder = folder.display()
                    ),
                );
            }
        } else if let Some(held) = project_file_in(&folder) {
            return Refusal::at(
                Field::Place,
                tr!(
                    "{folder} already holds the project {file_name}; open it instead",
                    folder = folder.display(),
                    file_name = held
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                ),
            );
        }
        if self.has_a_build() && !is_name(&self.resource) {
            return Refusal::at(
                Field::Resource,
                tr!("A resource name is a folder name: no slash, colon or other reserved character, no leading dot and no trailing dot or space"),
            );
        }
        if let Err(why) = self.instance() {
            return Refusal::at(Field::Template, why);
        }
        None
    }

    /// The template applied to this draft's resource name, when it is made from a template file.
    fn instance(&self) -> Result<Option<or_project::template::Instance>, String> {
        match self.shared() {
            Some(loaded) => loaded.template.instantiate(&self.resource).map(Some),
            None => Ok(None),
        }
    }

    /// What Create writes.
    pub fn plan(&self) -> Writes {
        let folder = self.folder();
        let makes = self.template.makes_a_folder();
        let clothing = (self.template == Template::Clothing).then_some(CLOTHING_DIR);
        let weapons = (self.template == Template::Weapons).then_some(WEAPONS_DIR);
        let particles = (self.template == Template::Particles).then_some(PARTICLES_DIR);
        let maps = (self.template == Template::Maps).then_some(MAPS_DIR);
        let mut inside = Vec::new();
        if makes {
            inside.push(SOURCES_DIR.to_string());
            inside.extend(clothing.map(str::to_string));
            inside.extend(weapons.map(str::to_string));
            inside.extend(particles.map(str::to_string));
            inside.extend(maps.map(str::to_string));
            if let Ok(Some(instance)) = self.instance() {
                inside.extend(
                    instance
                        .folders
                        .into_iter()
                        .map(|f| f.path)
                        .filter(|path| path != "."),
                );
            }
        }
        Writes {
            file: self.file(),
            inside,
            clothing,
            weapons,
            particles,
            maps,
            folder_is_new: makes && !folder.is_dir(),
            git: !folder.join(git::IGNORE).exists() && !folder.join(git::ATTRIBUTES).exists(),
            folder,
        }
    }

    /// The tree Create makes, as lines to draw; a name or a folder not chosen yet stands in as `<name>` or `<folder to adopt>`.
    pub fn tree(&self) -> Vec<TreeLine> {
        let writes = self.plan();
        let line = |depth, name: String, is_folder, new| TreeLine {
            depth,
            name,
            is_folder,
            new,
            stands_in: false,
        };
        let stand_in = |depth, name: &str, is_folder| TreeLine {
            stands_in: true,
            ..line(depth, name.to_string(), is_folder, true)
        };
        let nameless = self.name.is_empty();
        let mut lines = Vec::new();
        if self.template == Template::Adopt && self.place.trim().is_empty() {
            lines.push(stand_in(0, tr!("<folder to adopt>"), true));
        } else if nameless && self.template.makes_a_folder() {
            lines.push(stand_in(0, tr!("<name>"), true));
        } else {
            let root = writes
                .folder
                .file_name()
                .map_or_else(|| self.name.clone(), |n| n.to_string_lossy().into_owned());
            lines.push(line(0, root, true, writes.folder_is_new));
        }
        for inside in &writes.inside {
            lines.push(line(1, inside.clone(), true, true));
            if writes.clothing == Some(inside.as_str()) {
                lines.push(line(2, or_wardrobe::FILE.to_string(), false, true));
            }
            if writes.weapons == Some(inside.as_str()) {
                lines.push(line(2, or_weapons::FILE.to_string(), false, true));
            }
            if writes.particles == Some(inside.as_str()) {
                lines.push(line(2, or_particles::FILE.to_string(), false, true));
            }
            if writes.maps == Some(inside.as_str()) {
                lines.push(line(2, or_maps::FILE.to_string(), false, true));
            }
        }
        if nameless {
            lines.push(stand_in(
                1,
                &format!("{}.{PROJECT_EXTENSION}", tr!("<name>")),
                false,
            ));
        } else {
            let file = writes
                .file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            lines.push(line(1, file, false, true));
        }
        if writes.git {
            lines.push(line(1, git::IGNORE.to_string(), false, true));
            lines.push(line(1, git::ATTRIBUTES.to_string(), false, true));
        }
        lines
    }

    /// The screen the project opens on, which the gallery says and the project file states: the template's own, or the neutral one when the content is of several kinds.
    pub fn opens_on(&self) -> Workspace {
        let wanted = self
            .shared()
            .and_then(|loaded| loaded.template.opens_on.as_deref())
            .and_then(|word| serde_json::from_value(Value::String(word.into())).ok())
            .unwrap_or(self.template.opens_on());
        workspace_for(&self.container(), wanted)
    }

    /// The container the template starts the project with; a build ships for the game's own edition unless it states one.
    pub fn container(&self) -> Container {
        let container = match self.template {
            Template::Empty => Container {
                modules: Vec::new(),
                outputs: Vec::new(),
            },
            Template::Files | Template::Adopt => Container {
                modules: vec![Module::files("files")],
                outputs: vec![Output::fivem_resource("fivem", &self.resource, &["files"])],
            },
            Template::Clothing => Container {
                modules: vec![Module::clothing("clothing", CLOTHING_DIR)],
                outputs: vec![Output::fivem_resource(
                    "fivem",
                    &self.resource,
                    &["clothing"],
                )],
            },
            Template::Weapons => Container {
                modules: vec![Module::weapons("weapons", WEAPONS_DIR)],
                outputs: vec![Output::fivem_resource(
                    "fivem",
                    &self.resource,
                    &["weapons"],
                )],
            },
            Template::Particles => Container {
                modules: vec![Module::particles("particles", PARTICLES_DIR)],
                outputs: vec![Output::fivem_resource(
                    "fivem",
                    &self.resource,
                    &["particles"],
                )],
            },
            Template::Maps => Container {
                modules: vec![Module::maps("maps", MAPS_DIR)],
                outputs: vec![Output::fivem_resource("fivem", &self.resource, &["maps"])],
            },
            Template::Shared => self
                .instance()
                .ok()
                .flatten()
                .map(|instance| instance.container)
                .unwrap_or_default(),
        };
        with_default_edition(container, self.game)
    }

    /// Write the project: its folder, the folders inside it and its file, and nothing else.
    pub fn create(&self) -> Result<Created, String> {
        if let Some(why) = self.refusal() {
            return Err(why);
        }
        let writes = self.plan();
        if writes.file.exists() {
            return Err(tr!("{file} already exists", file = writes.file.display()));
        }
        let adoption = (self.template == Template::Adopt)
            .then(|| or_project::adopt::analyse(&writes.folder, &recognisers()));
        let documents: Vec<ProjectDocument> = adoption
            .iter()
            .flat_map(|a| &a.files)
            .map(|path| ProjectDocument::new(path.clone(), DocumentKind::from_path(path)))
            .collect();
        let container = match &adoption {
            Some(adoption) => with_default_edition(adoption.container(&self.resource), self.game),
            None => self.container(),
        };
        let mut extra = Map::new();
        extra.insert(GAME_KEY.into(), Value::String(self.game.word().into()));
        // A template file records its own name below; a built-in one records its label.
        if self.template != Template::Shared {
            extra.insert(
                or_project::template::TEMPLATE_KEY.into(),
                Value::String(self.template.label().into()),
            );
        }
        if let Some(record) = adoption.as_ref().and_then(Adoption::record) {
            extra.insert(or_project::adopt::ADOPTED_KEY.into(), record);
        }
        let mut project = ProjectFile {
            workspace: self.opens_on(),
            shown: self.template.pinned(),
            documents,
            container,
            extra,
            ..ProjectFile::default()
        };
        if let Some(loaded) = self.shared() {
            let keys = loaded.template.project_keys();
            if let Some(name) = keys.get(or_project::template::TEMPLATE_KEY) {
                project
                    .extra
                    .insert(or_project::template::TEMPLATE_KEY.into(), name.clone());
            }
            project.shown = loaded.template.shown.as_ref().map(|names| {
                names
                    .iter()
                    .filter_map(|name| serde_json::from_value(Value::String(name.clone())).ok())
                    .collect()
            });
            project.shown = project
                .shown
                .filter(|shown: &Vec<Workspace>| !shown.is_empty());
        }

        let made = self.make_folders(&writes)?;
        if let Err(why) = self.start_content(&writes.folder, &project.container) {
            self.take_back(&writes, &made);
            return Err(why);
        }
        let written: Option<Written> = match &adoption {
            Some(adoption) => match adoption.write() {
                Ok(written) => Some(written),
                Err(e) => return Err(format!("{}: {e}", writes.folder.display())),
            },
            None => None,
        };
        if let Err(e) = save_project(&writes.file, &project) {
            if let Some(written) = written {
                written.undo();
            }
            self.take_back(&writes, &made);
            return Err(format!("{}: {e}", writes.file.display()));
        }
        // ⚠ After the project file, so a project that could not be written leaves no Git file behind; a project is made
        // all the same when these cannot be, since the blocks are written again by the first import.
        if writes.git {
            if let Err(why) = git::update(&writes.folder) {
                tracing::warn!(%why, "the Git blocks of a new project were not written");
            }
        }
        Ok(Created {
            file: writes.file,
            adopted: adoption.as_ref().map_or(0, |a| a.files.len()),
            truncated: adoption.as_ref().is_some_and(|a| a.truncated),
            found: adoption
                .iter()
                .flat_map(|a| &a.found)
                .map(|f| format!("{}: {}", f.module.kind.words(), f.summary))
                .collect(),
            unplaced: adoption.as_ref().map_or(0, |a| a.unplaced.len()),
        })
    }

    /// The folder of each clothing content a template declares, with an empty pack in it.
    fn start_content(&self, folder: &Path, container: &Container) -> Result<(), String> {
        if self.template == Template::Clothing {
            let pack = or_wardrobe::names::identifier_for(&self.resource);
            for module in &container.modules {
                or_content::start_module(folder, module, &pack)?;
            }
            return Ok(());
        }
        if matches!(
            self.template,
            Template::Weapons | Template::Particles | Template::Maps
        ) {
            for module in &container.modules {
                or_content::start_module(folder, module, &self.resource)?;
            }
            return Ok(());
        }
        let Some(instance) = self.instance()? else {
            return Ok(());
        };
        let pack = resource_name_for(&self.resource);
        for needed in &instance.folders {
            if let Some(module) = container.module(&needed.module) {
                or_content::start_module(folder, module, &pack)?;
            }
        }
        Ok(())
    }

    /// Take back what Create made: the folder when it made it, else the folders it added, each only while it is empty.
    fn take_back(&self, writes: &Writes, made: &[PathBuf]) {
        if writes.folder_is_new {
            // Only what this made: the folder was not there before the write.
            let _ = std::fs::remove_dir_all(&writes.folder);
        } else {
            if let Some(dir) = writes.clothing {
                let _ = std::fs::remove_file(writes.folder.join(dir).join(or_wardrobe::FILE));
            }
            if let Some(dir) = writes.weapons {
                let _ = std::fs::remove_file(writes.folder.join(dir).join(or_weapons::FILE));
            }
            if let Some(dir) = writes.particles {
                let _ = std::fs::remove_file(writes.folder.join(dir).join(or_particles::FILE));
            }
            if let Some(dir) = writes.maps {
                let _ = std::fs::remove_file(writes.folder.join(dir).join(or_maps::FILE));
            }
            for folder in made.iter().rev() {
                let _ = std::fs::remove_dir(folder);
            }
        }
    }

    /// Make the folders of `writes` that are not there, and say which.
    fn make_folders(&self, writes: &Writes) -> Result<Vec<PathBuf>, String> {
        let mut wanted = Vec::new();
        if writes.folder_is_new {
            wanted.push(writes.folder.clone());
        }
        wanted.extend(
            writes
                .inside
                .iter()
                .map(|inside| writes.folder.join(inside)),
        );
        let mut made = Vec::new();
        for folder in wanted {
            if folder.is_dir() {
                continue;
            }
            if let Err(e) = std::fs::create_dir_all(&folder) {
                for undone in made.iter().rev() {
                    let _ = std::fs::remove_dir(undone);
                }
                return Err(format!("{}: {e}", folder.display()));
            }
            made.push(folder);
        }
        Ok(made)
    }
}

/// `container` with the game's own edition on each build that states none.
pub fn with_default_edition(mut container: Container, game: GameChoice) -> Container {
    for output in &mut container.outputs {
        output.target = output.target.or(game.default_target());
    }
    container
}

/// The template file at `path`, read.
pub fn read_template(path: &Path) -> Result<Loaded, String> {
    let shown = path.display();
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{shown}: {e}"))?
        .len();
    if size > MAX_TEMPLATE_BYTES {
        return Err(tr!(
            "{shown} is too big to be a template ({size} bytes)",
            shown = shown,
            size = size
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    let (template, findings) =
        Shared::from_json(&bytes).map_err(|why| format!("{shown}: {why}"))?;
    Ok(Loaded {
        template,
        from: Some(path.to_path_buf()),
        findings,
    })
}

/// The folder the templates a person saved are kept in, beside their preferences.
pub fn templates_folder() -> Option<PathBuf> {
    crate::state::prefs::config_root().map(|root| root.join("templates"))
}

/// The templates saved in [`templates_folder`], by name; a file that is no template is left out.
pub fn saved_templates() -> Vec<Loaded> {
    let Some(folder) = templates_folder() else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
        })
        .collect();
    files.sort();
    files.truncate(MAX_TEMPLATES);
    files
        .iter()
        .filter_map(|path| read_template(path).ok())
        .collect()
}

/// `name` with every character a folder cannot hold made `_`, for a folder adopted under the name it has.
fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_string();
    if is_name(&cleaned) {
        cleaned
    } else {
        "project".to_string()
    }
}

/// The folder the dialog offers first: where the last project's folder was made, else the user's documents.
pub fn default_place(prefs: &Prefs) -> PathBuf {
    prefs
        .recent_projects
        .first()
        .and_then(|project| project.parent())
        .and_then(Path::parent)
        .filter(|parent| parent.is_dir())
        .map(Path::to_path_buf)
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_default()
}

#[cfg(test)]
thread_local! {
    /// How many folders this thread has read looking for a project file in them: a test counts what a frame reads.
    pub static FOLDER_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A project file directly inside `folder`, the first by name.
pub fn project_file_in(folder: &Path) -> Option<PathBuf> {
    #[cfg(test)]
    FOLDER_READS.with(|reads| reads.set(reads.get() + 1));
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && super::is_project_file(path))
        .collect();
    found.sort();
    found.into_iter().next()
}
