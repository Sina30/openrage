//! How the project says how many and what a thing is called: one counting rule and one set of plurals for the start page's card, the Content panel, Properties and Problems, one count of errors and warnings, and one name for a content and for a build.
//!
//! ⚠ **An asset is a row of the Assets table**: a file, a source folder or a garment. A content holds the
//! rows that name it, so the card, which reads the disk on a worker, counts the same files and folders the scan does
//! ([`super::scan::files_held`]) and every screen says the number through [`held`].

use crate::state::front_door::KindWords as _;
use or_project::{Container, ModuleKind};

use super::{Finding, Place, Severity, Workbench};
use crate::state::front_door::counted;
use crate::ui::task_screens;

/// What a kind of content is called on screen.
pub fn word_of(kind: &ModuleKind) -> String {
    match task_screens::of_kind(kind) {
        Some(screen) => crate::i18n::tr(screen.content).to_string(),
        None => kind.words(),
    }
}

/// What a content of `kind` holds when it holds `n`: `1 asset`, `3 garments`, `2 peds`.
pub fn held(kind: &ModuleKind, n: usize) -> String {
    counted(
        n,
        match (task_screens::of_kind(kind), kind) {
            (Some(screen), _) => screen.count,
            (None, ModuleKind::AddonPed) => "ped",
            (None, _) => "asset",
        },
    )
}

impl Workbench {
    /// What `module` holds as the last scan read it, in words: `4 assets`, `3 garments`, `2 peds`, or that its pack could not be read.
    pub fn held_by(&self, module: &or_project::Module) -> String {
        // ⚠ A task screen's content says how it is counted when counting the rows of the Assets table is not it.
        if let Some(held_by) = task_screens::of_kind(&module.kind).and_then(|screen| screen.held) {
            return held_by(self, module);
        }
        match module.kind {
            ModuleKind::AddonPed => (self.peds().iter())
                .find(|(id, _)| *id == module.id)
                .map_or_else(
                    || tr!("peds not read").to_string(),
                    |(_, read)| match read {
                        Ok(n) => held(&module.kind, *n),
                        Err(_) => tr!("peds not read").to_string(),
                    },
                ),
            // ⚠ The files of a linked Files content are the project's it leads to, which the scan lists in no row.
            ModuleKind::Files if module.is_link() => held(
                &module.kind,
                self.link(&module.id).and_then(|l| l.files).unwrap_or(0),
            ),
            _ => held(&module.kind, self.assets_in(&module.id)),
        }
    }

    /// How many rows of the Assets table belong to the content `id`.
    pub fn assets_in(&self, id: &str) -> usize {
        self.rows().iter().filter(|row| row.content == id).count()
    }
}

/// What a content is called on screen: its kind (`Clothing`), and its id only when another content of the kind would read the same.
///
/// ⚠ The id is the project file's key, which nobody typed, so it is said only to tell two contents of one kind apart.
pub fn content_name(container: &Container, id: &str) -> String {
    let Some(module) = container.module(id) else {
        // The project that holds no Files content yet names the one it will get.
        return match id == or_project::container::IMPLICIT_FILES {
            true => word_of(&ModuleKind::Files),
            false => id.to_string(),
        };
    };
    let kind = word_of(&module.kind);
    let shared = container
        .modules
        .iter()
        .filter(|other| other.kind == module.kind)
        .count()
        > 1;
    if shared && !kind.eq_ignore_ascii_case(id) {
        format!("{kind} {id}")
    } else {
        kind
    }
}

/// What a build is called on screen: the name its resource has, which is the name the person gave it, and never the id its entry is keyed by.
pub fn build_name(container: &Container, id: &str) -> String {
    container
        .output(id)
        .map_or_else(|| id.to_string(), |output| output.name.clone())
}

/// How many errors and warnings: the one count every screen says, so the word `problems` is never a number of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub errors: usize,
    pub warnings: usize,
}

impl Counts {
    /// The worst kind there is, which colours the dot beside the count.
    pub fn worst(self) -> Option<Severity> {
        match (self.errors, self.warnings) {
            (0, 0) => None,
            (0, _) => Some(Severity::Warning),
            _ => Some(Severity::Error),
        }
    }

    /// `2 errors, 1 warning`, or `None` when there are none.
    pub fn said(self) -> String {
        match (self.errors, self.warnings) {
            (0, 0) => tr!("None").to_string(),
            (e, 0) => counted(e, "error"),
            (0, w) => counted(w, "warning"),
            (e, w) => format!("{}, {}", counted(e, "error"), counted(w, "warning")),
        }
    }

    /// What the errors do to a build: `nothing stops this build`, `1 error stops this build`, `3 errors stop this build`; its warnings, which stop nothing, after them.
    pub fn stopping(self) -> String {
        let stops = match self.errors {
            0 => tr!("nothing stops this build").to_string(),
            1 => tr!("1 error stops this build").to_string(),
            n => tr!("{n} errors stop this build", n = n),
        };
        match self.warnings {
            0 => stops,
            w => format!("{stops}; {}", counted(w, "warning")),
        }
    }
}

/// What a count is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    /// Everything the Problems panel lists.
    Project,
    /// What is wrong with a content, by its id: its files, its garments and the content itself.
    Content(&'a str),
    /// What a build would stop on, by its id: the errors of its plan, and the warnings plugins said of it.
    Build(&'a str),
}

impl Counts {
    /// The errors and the warnings among `findings`.
    pub fn of<'a>(findings: impl IntoIterator<Item = &'a Finding>) -> Self {
        findings
            .into_iter()
            .fold(Self::default(), |mut counts, finding| {
                match finding.severity {
                    Severity::Error => counts.errors += 1,
                    Severity::Warning => counts.warnings += 1,
                }
                counts
            })
    }
}

impl Workbench {
    /// How many errors and warnings there are in `scope`: the one place a count of the findings is made.
    pub fn counts(&self, scope: Scope) -> Counts {
        let within = self
            .findings
            .iter()
            .filter(|finding| match (scope, &finding.place) {
                (Scope::Project, _) => true,
                (
                    Scope::Content(id),
                    Place::Asset { content, .. }
                    | Place::Garment { content, .. }
                    | Place::Content(content),
                ) => content == id,
                // ⚠ A plan's problem that a row says already is not a finding of the build, so its errors are the plan's own, below.
                (Scope::Build(id), Place::Build(output)) => {
                    output == id && finding.severity == Severity::Warning
                }
                _ => false,
            });
        let mut counts = Counts::of(within);
        if let Scope::Build(id) = scope {
            counts.errors = self.plan_problems_of(id);
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_the_scan_has_not_reached_is_being_read_and_not_unread() {
        let wardrobe = Workbench::default();
        let clothing = or_project::Module::clothing("clothing", "clothing");
        assert_eq!(wardrobe.held_by(&clothing), "Reading…");
    }

    #[test]
    fn a_count_says_its_noun_in_the_plural_it_takes() {
        assert_eq!(held(&ModuleKind::Files, 1), "1 asset");
        assert_eq!(held(&ModuleKind::Files, 0), "0 assets");
        assert_eq!(held(&ModuleKind::Clothing, 3), "3 garments");
        assert_eq!(held(&ModuleKind::AddonPed, 1), "1 ped");
    }

    fn counts(errors: usize, warnings: usize) -> Counts {
        Counts { errors, warnings }
    }

    #[test]
    fn problems_are_said_by_their_kinds_and_never_as_a_bare_number() {
        assert_eq!(counts(0, 0).said(), "None");
        assert_eq!(counts(1, 0).said(), "1 error");
        assert_eq!(counts(0, 2).said(), "2 warnings");
        assert_eq!(counts(2, 1).said(), "2 errors, 1 warning");
    }

    #[test]
    fn what_stops_a_build_is_its_errors_and_a_warning_only_follows() {
        assert_eq!(counts(0, 0).stopping(), "nothing stops this build");
        assert_eq!(counts(1, 0).stopping(), "1 error stops this build");
        assert_eq!(counts(3, 0).stopping(), "3 errors stop this build");
        assert_eq!(
            counts(0, 2).stopping(),
            "nothing stops this build; 2 warnings"
        );
        assert_eq!(counts(1, 1).worst(), Some(Severity::Error));
        assert_eq!(counts(0, 1).worst(), Some(Severity::Warning));
        assert_eq!(counts(0, 0).worst(), None);
    }

    #[test]
    fn a_content_is_called_by_its_kind_and_by_its_id_only_to_tell_two_of_a_kind_apart() {
        use or_project::Module;

        let mut container = Container::default();
        container
            .modules
            .push(Module::clothing("uniforms", "uniforms"));
        assert_eq!(content_name(&container, "files"), "Files");
        assert_eq!(content_name(&container, "uniforms"), "Clothing");
        // A project that holds no Files content yet names the one it will get.
        assert_eq!(
            content_name(
                &Container {
                    modules: vec![],
                    outputs: vec![]
                },
                "files"
            ),
            "Files"
        );

        container
            .modules
            .push(Module::clothing("accessories", "accessories"));
        assert_eq!(content_name(&container, "uniforms"), "Clothing uniforms");
        assert_eq!(
            content_name(&container, "accessories"),
            "Clothing accessories"
        );
    }

    #[test]
    fn a_build_is_called_by_the_name_its_resource_has_and_not_the_id_it_is_keyed_by() {
        let mut container = Container::default();
        container.outputs.push(or_project::Output::fivem_resource(
            "fivem",
            "harbor_police",
            &["files"],
        ));
        assert_eq!(build_name(&container, "fivem"), "harbor_police");
        assert_eq!(build_name(&container, "gone"), "gone");
    }
}
