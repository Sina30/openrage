//! What a plugin declares about itself before it is allowed to run.
//!
//! ⚠ Capabilities are declared in the manifest and consented to once, at
//! install; there is no host function for asking mid-run, so a plugin that
//! wants more ships a new manifest and is consented to again.
//!
//! ⚠ **The ABI has a version, and an older plugin still loads.** The host implements every
//! version from [`OLDEST_ABI`] to [`ABI_VERSION`]: a plugin built for an older one is run as it
//! was built, and the manager says so. A plugin built for a newer one is refused with a sentence
//! that says to update OpenRage, and one that declares an older version may not want a capability
//! only a newer version has, since its code was not built against the function that capability
//! buys. `a_plugin_of_an_older_abi_still_loads` holds it.

use crate::{Capability, Grants};

/// The newest ABI this host implements.
pub const ABI_VERSION: u32 = 1;

/// The oldest ABI this host still runs, as it was built.
pub const OLDEST_ABI: u32 = 0;

/// What a plugin says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    /// Which ABI it was built against.
    pub abi: u32,
    /// What it wants to be allowed to do: a declaration, not a grant.
    pub wants: Vec<Capability>,
}

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// Built against an ABI newer than this host knows: the way out is a newer OpenRage.
    Newer { declared: u32, known: u32 },
    /// Built against an ABI that does not have a capability it wants.
    TooOldFor {
        capability: Capability,
        declared: u32,
    },
    /// A plugin has to be identifiable to be granted anything, because a grant is
    /// remembered against its id.
    NoId,
    /// It declares no capabilities, so it could do nothing if installed.
    WantsNothing,
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Newer { declared, known } => write!(
                f,
                "the plugin was built for plugin interface {declared}, and this OpenRage \
                 understands up to {known}: update OpenRage to run it"
            ),
            Self::TooOldFor {
                capability,
                declared,
            } => write!(
                f,
                "the plugin says it was built for plugin interface {declared} and asks to {}, \
                 which needs interface {} or newer: rebuild the plugin for the newer interface",
                capability.asks_for(),
                capability.since()
            ),
            Self::NoId => f.write_str(
                "the plugin declares no id, so a grant could not be remembered against it",
            ),
            Self::WantsNothing => f.write_str(
                "the plugin declares no capabilities, so it could do nothing if installed",
            ),
        }
    }
}

impl Manifest {
    /// Check a manifest before anything is consented to or run.
    pub fn accept(&self) -> Result<(), Rejected> {
        self.accept_up_to(ABI_VERSION)
    }

    /// [`Self::accept`] as a host that implements ABIs up to `newest` would: how an older OpenRage answers a plugin built later.
    pub fn accept_up_to(&self, newest: u32) -> Result<(), Rejected> {
        if self.abi > newest {
            return Err(Rejected::Newer {
                declared: self.abi,
                known: newest,
            });
        }
        if self.id.trim().is_empty() {
            return Err(Rejected::NoId);
        }
        if self.wants.is_empty() {
            return Err(Rejected::WantsNothing);
        }
        if let Some(&capability) = self.wants.iter().find(|c| c.since() > self.abi) {
            return Err(Rejected::TooOldFor {
                capability,
                declared: self.abi,
            });
        }
        Ok(())
    }

    /// Whether it was built for an older ABI than this host's newest, which still runs it as it was built.
    pub fn is_older(&self) -> bool {
        self.abi < ABI_VERSION
    }

    /// What the plugin manager says of a plugin built for an older ABI, none for one built for the newest.
    pub fn age_note(&self) -> Option<String> {
        self.is_older().then(|| {
            format!(
                "Built for plugin interface {}, an older one. It still loads and runs as it was built; \
                 rebuild it for interface {ABI_VERSION} to use what is newer.",
                self.abi
            )
        })
    }

    /// The capabilities this plugin asked for, as a grant set a consent prompt shows.
    ///
    /// ⚠ Not what the runtime is handed: passed straight to
    /// [`Runtime::run`](crate::Runtime::run), it grants whatever the plugin asked for.
    pub fn requested(&self) -> Grants {
        let mut grants = Grants::none();
        for capability in &self.wants {
            grants = grants.granting(*capability);
        }
        grants
    }

    /// Whether it wants to add to the editor, which is a plugin that stays loaded, not one run once.
    pub fn adds_to_the_editor(&self) -> bool {
        self.wants.iter().any(|c| c.adds_to_the_editor())
    }

    /// Whether it extends a build, which makes it run with every plan: a validator, an emitter or both.
    pub fn extends_a_build(&self) -> bool {
        self.wants.iter().any(|c| c.extends_a_build())
    }

    /// A validator reports findings about a plan, with a fix where it suggests one.
    pub fn validates(&self) -> bool {
        self.wants.contains(&Capability::ReportFindings)
            || self.wants.contains(&Capability::SuggestFixes)
    }

    /// An emitter adds files to a plan.
    pub fn emits(&self) -> bool {
        self.wants.contains(&Capability::AddFiles)
    }

    /// One line per capability, for the consent prompt.
    pub fn consent_lines(&self) -> Vec<String> {
        self.wants
            .iter()
            .map(|c| format!("{} — {}", self.name, c.asks_for()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            id: "cloth".into(),
            name: "Cloth Tool".into(),
            abi: ABI_VERSION,
            wants: vec![Capability::ReadOpenDocument, Capability::ReadInstall],
        }
    }

    #[test]
    fn a_well_formed_manifest_is_accepted() {
        assert_eq!(manifest().accept(), Ok(()));
    }

    #[test]
    fn an_abi_version_newer_than_the_host_knows_is_refused_with_the_way_out() {
        for declared in [ABI_VERSION + 1, ABI_VERSION + 7, 99] {
            let refused = Manifest {
                abi: declared,
                ..manifest()
            }
            .accept()
            .expect_err("a newer ABI must be refused");
            assert_eq!(
                refused,
                Rejected::Newer {
                    declared,
                    known: ABI_VERSION
                }
            );
            assert!(
                refused.to_string().contains("update OpenRage"),
                "the refusal has to say what to do: {refused}"
            );
        }
    }

    #[test]
    fn an_older_host_refuses_a_plugin_built_later_with_a_sentence() {
        let built_later = Manifest {
            abi: 1,
            wants: vec![Capability::WriteFiles],
            ..manifest()
        };
        assert_eq!(built_later.accept(), Ok(()));
        let refused = built_later
            .accept_up_to(0)
            .expect_err("the host that knows only interface 0 cannot run it");
        let said = refused.to_string();
        assert!(
            said.contains("interface 1") && said.contains("up to 0") && said.contains("update"),
            "{said}"
        );
    }

    #[test]
    fn every_abi_from_the_oldest_to_the_newest_is_accepted_for_what_it_could_want() {
        for abi in OLDEST_ABI..=ABI_VERSION {
            let wants = Capability::ALL
                .into_iter()
                .filter(|c| c.since() <= abi)
                .collect();
            let plugin = Manifest {
                abi,
                wants,
                ..manifest()
            };
            assert_eq!(plugin.accept(), Ok(()), "interface {abi}");
        }
    }

    #[test]
    fn a_plugin_may_not_want_what_only_a_newer_interface_has() {
        let refused = Manifest {
            abi: 0,
            wants: vec![Capability::ReadOpenDocument, Capability::WriteFiles],
            ..manifest()
        }
        .accept()
        .expect_err("interface 0 has no function for it");
        assert_eq!(
            refused,
            Rejected::TooOldFor {
                capability: Capability::WriteFiles,
                declared: 0
            }
        );
        assert!(
            refused.to_string().contains("rebuild the plugin"),
            "{refused}"
        );
    }

    #[test]
    fn a_plugin_of_an_older_abi_says_so_in_the_manager_and_one_of_the_newest_says_nothing() {
        let old = Manifest {
            abi: 0,
            ..manifest()
        };
        assert!(old.is_older());
        let note = old.age_note().expect("an older plugin is said to be");
        assert!(
            note.contains("interface 0") && note.contains("still loads"),
            "{note}"
        );
        assert_eq!(manifest().age_note(), None);
    }

    #[test]
    fn a_manifest_with_no_id_or_no_wants_is_refused() {
        assert_eq!(
            Manifest {
                id: "  ".into(),
                ..manifest()
            }
            .accept(),
            Err(Rejected::NoId)
        );
        assert_eq!(
            Manifest {
                wants: Vec::new(),
                ..manifest()
            }
            .accept(),
            Err(Rejected::WantsNothing)
        );
    }

    #[test]
    fn a_plugin_that_wants_a_panel_stays_loaded_and_one_that_wants_a_file_runs_once() {
        assert!(!manifest().adds_to_the_editor());
        let panel = Manifest {
            wants: vec![Capability::AddPanels],
            ..manifest()
        };
        assert!(panel.adds_to_the_editor());
        let surface: Vec<Capability> = Capability::ALL
            .into_iter()
            .filter(|c| c.adds_to_the_editor())
            .collect();
        assert_eq!(
            surface,
            [
                Capability::AddPanels,
                Capability::AddCommands,
                Capability::AddSettings,
                Capability::DrawOverlays
            ]
        );
    }

    #[test]
    fn a_request_is_not_a_grant() {
        let wanted = manifest().requested();
        assert!(wanted.has(Capability::ReadInstall), "it asked for this");

        assert!(
            !Grants::default().has(Capability::ReadInstall),
            "if the default ever includes what a manifest merely requested, the \
             consent step has stopped meaning anything"
        );
    }

    #[test]
    fn every_wanted_capability_gets_a_line_a_user_can_read() {
        let lines = manifest().consent_lines();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            assert!(line.starts_with("Cloth Tool — "), "got {line:?}");
            assert!(line.len() > 20, "a prompt needs a sentence: {line:?}");
        }
    }
}
