//! The templates a new project can start from and the game it is for: what each says of itself, and the screen it opens on.

use or_project::Target;

use crate::ui::dock::Workspace;
use crate::ui::task_screens;

/// A starting set of content and builds, and the screen the project opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Empty,
    Files,
    Clothing,
    Weapons,
    Particles,
    Maps,
    Adopt,
    /// A template another project made, as a file: the dialog holds the ones it has read.
    Shared,
}

impl Template {
    /// Every template that is always offered; the shared ones are the dialog's own.
    pub const ALL: [Template; 7] = [
        Template::Empty,
        Template::Files,
        Template::Clothing,
        Template::Weapons,
        Template::Particles,
        Template::Maps,
        Template::Adopt,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => tr!("Empty project"),
            Self::Files => tr!("Files project"),
            Self::Clothing => tr!("Clothing pack"),
            Self::Weapons => tr!("Weapon pack"),
            Self::Particles => tr!("Particle pack"),
            Self::Maps => tr!("Map pack"),
            Self::Adopt => tr!("Project from a folder"),
            Self::Shared => tr!("Template file"),
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Empty => {
                tr!("A project with nothing in it yet. Add content and builds when you need them.")
            }
            Self::Files => {
                tr!("Loose files shipped as they are: scripts, interface and configuration, copied into a FiveM resource.")
            }
            Self::Clothing => {
                tr!("Clothes for the male and female freemode peds: drop models and textures, and OpenRage names, numbers and checks them and makes a FiveM resource.")
            }
            Self::Weapons => {
                tr!("Weapons for the game: drop weapon models, textures and weapon files, and OpenRage reads what they define, lets you change it, checks the pack and makes a FiveM resource.")
            }
            Self::Particles => {
                tr!("Particle effects for the game: drop .ypt files, and OpenRage keeps each as a source of rules and textures you can read and change, checks the pack and makes a FiveM resource.")
            }
            Self::Maps => {
                tr!("A map for the game: add .ymap and .ytyp files, and OpenRage shows what they place, lets you move, turn and scale each thing with a manipulator, checks the pack and makes a FiveM resource.")
            }
            Self::Adopt => {
                tr!("Make a folder you already have a project. Nothing in it is moved or rewritten; a project file is added beside what is there, with a .gitignore and a .gitattributes where the folder has neither, and a clothing pack in it becomes clothing content.")
            }
            Self::Shared => {
                tr!("The content, builds and layout another project made and shared as a file. It holds no file and no code.")
            }
        }
    }

    pub fn content(self) -> &'static str {
        match self {
            Self::Empty => tr!("None yet"),
            Self::Files => tr!("Files"),
            Self::Clothing => {
                tr!("Clothing: an empty pack, ready for the models and textures you drop on it")
            }
            Self::Weapons => {
                tr!("Weapons: an empty pack, ready for the models, textures and weapon files you drop on it")
            }
            Self::Particles => {
                tr!("Particles: an empty pack, ready for the .ypt files you drop on it")
            }
            Self::Maps => {
                tr!("Maps: an empty pack, ready for the .ymap and .ytyp files you add to it")
            }
            Self::Adopt => tr!("What the folder holds, grouped by what is recognised in it"),
            Self::Shared => tr!("As the template states"),
        }
    }

    pub fn builds(self) -> &'static str {
        match self {
            Self::Empty => tr!("None yet"),
            Self::Files
            | Self::Clothing
            | Self::Weapons
            | Self::Particles
            | Self::Maps
            | Self::Adopt => {
                tr!("A FiveM resource, named after the project")
            }
            Self::Shared => tr!("As the template states"),
        }
    }

    /// The screen a project made from the template opens on: the task screen the table says it starts, else the Project workspace.
    pub fn opens_on(self) -> Workspace {
        task_screens::of_template(self).map_or(Workspace::Project, |screen| screen.workspace)
    }

    /// The workspace buttons the template pins; none shows every one.
    pub fn pinned(self) -> Option<Vec<Workspace>> {
        let starts = task_screens::of_template(self)?.starts.as_ref()?;
        Some(starts.pins.to_vec())
    }

    pub(super) fn makes_a_folder(self) -> bool {
        self != Self::Adopt
    }

    /// Whether the gallery's search finds the template.
    pub fn matches(self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.label().to_lowercase().contains(&query)
            || self.blurb().to_lowercase().contains(&query)
    }
}

/// Which game a project is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameChoice {
    Legacy,
    Enhanced,
    Both,
}

impl GameChoice {
    pub const ALL: [GameChoice; 3] = [Self::Legacy, Self::Enhanced, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::Legacy => tr!("Legacy"),
            Self::Enhanced => tr!("Enhanced"),
            Self::Both => tr!("Legacy and Enhanced"),
        }
    }

    /// As the project file spells it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Enhanced => "enhanced",
            Self::Both => "both",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|game| game.word() == word)
    }

    /// The edition a new build ships for: the game's own, and as authored for both.
    pub fn default_target(self) -> Option<Target> {
        match self {
            Self::Legacy => Some(Target::Legacy),
            Self::Enhanced => Some(Target::Enhanced),
            Self::Both => None,
        }
    }
}
