//! What a plugin may ask for, and what the host refuses.
//!
//! ⚠ WebAssembly isolates a plugin; it does not limit what the plugin may do. A
//! store with every host function registered is a sandbox around a plugin that
//! can already do anything the editor can, so the capability model here is the
//! control, and a refusal is the same whether the guest is WebAssembly, a
//! subprocess or a test.
//!
//! A Rust type cannot cross into a guest, so the host's surface is a described
//! tree — names, offsets and widths from [`or_io::desc`].
//!
//! [`Grants::default`] is [`Capability::ReadOpenDocument`] alone: opening a
//! plugin on a file is consent to it reading that file, and everything else is
//! asked for and recorded. `the_default_grant_can_do_almost_nothing` holds it.

use std::collections::BTreeSet;

use or_io::{Diagnostic, Severity};

pub mod abi;
pub mod extension;
pub mod host;
pub mod manifest;
pub mod runtime;
pub mod session;
pub mod surface;
pub mod transaction;

pub use abi::{decode, encode, CodecError, Status, MAX_DEPTH, MAX_ENCODED};
pub use extension::{Emitted, Extended, Finding, Level, SuggestedFix};
pub use host::{AcceptsAnything, Denied, SaveCheck, Site, WouldBeInvalid};
pub use manifest::{Manifest, Rejected, ABI_VERSION, OLDEST_ABI};
pub use runtime::{
    Document, Failure, HostError, Limits, Outcome, Runtime, Subject, GUEST_MEMORY, HOST_MODULE,
};
pub use session::{Call, Drawn, Session};
pub use surface::{
    CommandSpec, Event, PanelSpec, Pick, Primitive, Registrations, SettingKind, SettingSpec,
    SettingValue, Vertex, Widget,
};
pub use transaction::{FileWrite, Transaction};

/// One thing a plugin can be permitted to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Read the described tree of the document the plugin was opened on; the only default.
    ReadOpenDocument,

    /// Edit the open document by patching a described scalar.
    ///
    /// ⚠ Granting this is **not sufficient**. Every write also passes the
    /// embedder's [`WriteGate`], so a plugin cannot be a way around the
    /// application's own edit mode — see
    /// `a_granted_edit_is_still_refused_when_the_embedder_says_read_only`.
    EditOpenDocument,

    /// Read the bytes of the file the plugin was opened on, as the RSC7 file its document would save.
    ///
    /// ⚠ The bytes are copied into the guest's memory, which the guest pays for
    /// and the host does not meter; `what_a_files_bytes_cost_a_guest` measures it.
    ReadFileBytes,

    /// Replace the bytes of the file the plugin was opened on.
    ///
    /// ⚠ Also gated by [`WriteGate`], and the embedder's
    /// [`SaveCheck::why_the_file_would_be_invalid`] decides whether the bytes
    /// are a file it keeps; they become the open document, not the file on disk.
    WriteFileBytes,

    /// Read other files from the mounted install; a separate consent from
    /// reading the document the plugin was opened on.
    ReadInstall,

    /// Write files into the install or its archives.
    ///
    /// ⚠ Also gated by [`WriteGate`].
    WriteInstall,

    /// Read paths outside the install and outside the plugin's own directory.
    ///
    /// There is no matching write capability, deliberately: no use case has
    /// asked for one.
    ReadUserFiles,

    /// Reach the network.
    ///
    /// ⚠ Held separately and off by default: it turns a bad plugin into an
    /// exfiltration route rather than merely a broken tool.
    Network,

    /// Register panels, which the host draws from the widgets the guest describes each frame.
    AddPanels,

    /// Register commands, which the menus and the command palette offer.
    AddCommands,

    /// Declare settings, which Settings shows on a page of the plugin's own and the preferences keep.
    AddSettings,

    /// Draw points and lines over the open document in the viewport, and receive the clicks the guest asks for.
    DrawOverlays,

    /// Read the plan of a build as a JSON document: the files, items and figures it will write.
    ///
    /// ⚠ The plan, and nothing else of the project: not a file's bytes, and no folder.
    ReadPlan,

    /// Report findings about a build's plan, which the Problems list shows under the plugin's id.
    ReportFindings,

    /// Add text files to a build's plan.
    ///
    /// ⚠ Not a write in the sense of [`Capability::writes`]: an added file lands in the plan, which
    /// refuses a path that leaves the resource or collides, and is written only when the user
    /// builds. `what_a_plugin_that_extends_a_build_may_and_may_not_do` holds that.
    AddFiles,

    /// Write several files of the project together, all or nothing, as one step of Undo. Since ABI 1.
    ///
    /// ⚠ Also gated by [`WriteGate`], and the embedder's [`SaveCheck::why_the_files_would_be_invalid`]
    /// decides whether the set is one it keeps; a refusal keeps none of the files.
    WriteFiles,

    /// Read the clothing pack of a build as data, which a validator checks beyond what the plan says. Since ABI 1.
    ///
    /// ⚠ The pack's own document and the place of each pack in the project, and no path of the user's disk.
    ReadPack,

    /// Suggest a change to the project's files with a finding, which the person accepts with Fix. Since ABI 1.
    ///
    /// ⚠ Not a write in the sense of [`Capability::writes`]: a suggestion is data held with the
    /// finding, and the files change only when the person accepts it.
    SuggestFixes,
}

impl Capability {
    pub const ALL: [Capability; 18] = [
        Self::ReadOpenDocument,
        Self::EditOpenDocument,
        Self::ReadFileBytes,
        Self::WriteFileBytes,
        Self::ReadInstall,
        Self::WriteInstall,
        Self::ReadUserFiles,
        Self::Network,
        Self::AddPanels,
        Self::AddCommands,
        Self::AddSettings,
        Self::DrawOverlays,
        Self::ReadPlan,
        Self::ReportFindings,
        Self::AddFiles,
        Self::WriteFiles,
        Self::ReadPack,
        Self::SuggestFixes,
    ];

    /// The first ABI that has the host functions this capability buys: a plugin built for an older one cannot want it.
    pub fn since(self) -> u32 {
        match self {
            Self::WriteFiles | Self::ReadPack | Self::SuggestFixes => 1,
            _ => 0,
        }
    }

    /// The stable key a refusal carries, for logs and for tests.
    pub fn key(self) -> &'static str {
        match self {
            Self::ReadOpenDocument => "cap.read_open_document",
            Self::EditOpenDocument => "cap.edit_open_document",
            Self::ReadFileBytes => "cap.read_file_bytes",
            Self::WriteFileBytes => "cap.write_file_bytes",
            Self::ReadInstall => "cap.read_install",
            Self::WriteInstall => "cap.write_install",
            Self::ReadUserFiles => "cap.read_user_files",
            Self::Network => "cap.network",
            Self::AddPanels => "cap.add_panels",
            Self::AddCommands => "cap.add_commands",
            Self::AddSettings => "cap.add_settings",
            Self::DrawOverlays => "cap.draw_overlays",
            Self::ReadPlan => "cap.read_plan",
            Self::ReportFindings => "cap.report_findings",
            Self::AddFiles => "cap.add_files",
            Self::WriteFiles => "cap.write_files",
            Self::ReadPack => "cap.read_pack",
            Self::SuggestFixes => "cap.suggest_fixes",
        }
    }

    /// One sentence a consent prompt can show.
    pub fn asks_for(self) -> &'static str {
        match self {
            Self::ReadOpenDocument => "read the file you opened it on",
            Self::EditOpenDocument => "change the file you opened it on",
            Self::ReadFileBytes => "read every byte of the file you opened it on",
            Self::WriteFileBytes => "replace every byte of the file you opened it on",
            Self::ReadInstall => "read other files in your game install",
            Self::WriteInstall => "change files in your game install",
            Self::ReadUserFiles => "read files elsewhere on your computer",
            Self::Network => "connect to the internet",
            Self::AddPanels => "add panels to the editor",
            Self::AddCommands => "add commands to the menus and the command palette",
            Self::AddSettings => "add a page of settings to Settings",
            Self::DrawOverlays => {
                "draw over the open document in the viewport and receive your clicks there"
            }
            Self::ReadPlan => {
                "read the plan of your builds: the files and items a build will write"
            }
            Self::ReportFindings => "list problems it finds in your builds",
            Self::AddFiles => "add text files to your builds",
            Self::WriteFiles => {
                "change several files of your project together, all or none, as one step you can undo"
            }
            Self::ReadPack => "read the clothing packs your builds take, as data",
            Self::SuggestFixes => {
                "suggest a change to your files with a problem it finds, which changes them only if you accept it"
            }
        }
    }

    /// Does this capability extend a build (read its plan or its pack, report on it or add to it) and so make a plugin
    /// one that runs with every plan and not on a document?
    pub fn extends_a_build(self) -> bool {
        matches!(
            self,
            Self::ReadPlan
                | Self::ReadPack
                | Self::ReportFindings
                | Self::SuggestFixes
                | Self::AddFiles
        )
    }

    /// Does this capability add to the editor itself (a panel, a command, a setting or an overlay)
    /// and so need a plugin that stays loaded rather than one run once on a document?
    pub fn adds_to_the_editor(self) -> bool {
        matches!(
            self,
            Self::AddPanels | Self::AddCommands | Self::AddSettings | Self::DrawOverlays
        )
    }

    /// Does exercising this capability modify something?
    ///
    /// ⚠ What routes a capability through the [`WriteGate`]: a new capability
    /// that writes and is missing from this arm is not gated.
    pub fn writes(self) -> bool {
        matches!(
            self,
            Self::EditOpenDocument | Self::WriteFileBytes | Self::WriteInstall | Self::WriteFiles
        )
    }
}

/// What a plugin was actually granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grants(BTreeSet<Capability>);

impl Default for Grants {
    fn default() -> Self {
        Self(BTreeSet::from([Capability::ReadOpenDocument]))
    }
}

impl Grants {
    /// No capabilities at all, for a plugin that has not been consented to yet.
    pub fn none() -> Self {
        Self(BTreeSet::new())
    }

    /// Every capability. ⚠ For tests only: `nothing_in_the_tree_grants_everything`
    /// refuses any file outside a `tests` directory that names it.
    pub fn trusted_with_everything() -> Self {
        Self(Capability::ALL.into_iter().collect())
    }

    pub fn granting(mut self, capability: Capability) -> Self {
        self.0.insert(capability);
        self
    }

    pub fn has(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Whether the embedding application currently permits writing at all.
///
/// ⚠ Separate from a grant: a plugin granted [`Capability::EditOpenDocument`]
/// is still refused while the application's edit gate says read-only, or
/// installing a plugin would make a read-only editor writable again. The
/// embedder implements this.
pub trait WriteGate {
    /// `None` to permit, or the reason to refuse.
    fn why_not_writable(&self) -> Option<String>;
}

impl WriteGate for Box<dyn WriteGate> {
    fn why_not_writable(&self) -> Option<String> {
        self.as_ref().why_not_writable()
    }
}

/// A gate that permits every write. ⚠ For tests and for a headless tool that has
/// no edit mode of its own — never for the application.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysWritable;

impl WriteGate for AlwaysWritable {
    fn why_not_writable(&self) -> Option<String> {
        None
    }
}

/// A gate that refuses, carrying the reason.
#[derive(Debug, Clone)]
pub struct NeverWritable(pub String);

impl WriteGate for NeverWritable {
    fn why_not_writable(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub capability: Capability,
    pub because: Because,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Because {
    /// The plugin never asked for it, or the user declined.
    NotGranted,
    /// It was granted, and the embedder does not permit writing right now.
    ///
    /// ⚠ Distinct from [`Because::NotGranted`] because the remedy is the edit
    /// mode, not the grant; conflating them sends a user to the wrong setting.
    EmbedderRefusesWrites(String),
}

impl Refused {
    /// A diagnostic carrying the capability's stable key.
    pub fn diagnostic(&self) -> Diagnostic {
        let message = match &self.because {
            Because::NotGranted => format!(
                "the plugin was not granted permission to {}",
                self.capability.asks_for()
            ),
            Because::EmbedderRefusesWrites(why) => format!(
                "the plugin may {} and writing is not permitted: {why}",
                self.capability.asks_for()
            ),
        };
        Diagnostic::new(Severity::Warning, "plugin.refused", message)
            .with("capability", self.capability.key())
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.diagnostic().message)
    }
}

/// The permission half of a plugin host; the runtime is the transport.
#[derive(Debug)]
pub struct Permissions<G: WriteGate> {
    grants: Grants,
    gate: G,
    /// Every refusal, in order. A host surfaces these rather than swallowing them,
    /// so a plugin that silently does nothing is diagnosable.
    refusals: Vec<Refused>,
}

impl<G: WriteGate> Permissions<G> {
    pub fn new(grants: Grants, gate: G) -> Self {
        Self {
            grants,
            gate,
            refusals: Vec::new(),
        }
    }

    /// May the plugin do this, right now?
    ///
    /// ⚠ Records any refusal as a side effect, which is why it takes `&mut self`:
    /// a check whose refusals are invisible leaves a plugin that appears to work
    /// and does nothing.
    pub fn permits(&mut self, capability: Capability) -> Result<(), Refused> {
        if !self.grants.has(capability) {
            let refused = Refused {
                capability,
                because: Because::NotGranted,
            };
            self.refusals.push(refused.clone());
            return Err(refused);
        }
        if capability.writes() {
            if let Some(why) = self.gate.why_not_writable() {
                let refused = Refused {
                    capability,
                    because: Because::EmbedderRefusesWrites(why),
                };
                self.refusals.push(refused.clone());
                return Err(refused);
            }
        }
        Ok(())
    }

    pub fn refusals(&self) -> &[Refused] {
        &self.refusals
    }

    pub fn grants(&self) -> &Grants {
        &self.grants
    }
}

/// What the host does not do yet.
///
/// ⚠ A payload reaches a guest only inside its file's bytes, copied into its
/// linear memory ([`Capability::ReadFileBytes`], measured by
/// `what_a_files_bytes_cost_a_guest`), and a guest cannot hold a GPU resource
/// at all. `a_payload_capability_does_not_exist_yet` asserts what landed and
/// what did not.
pub mod not_yet {
    /// There is no capability for a texture or a buffer apart from its file.
    pub const PAYLOAD_ACCESS: &str = "a guest reaches a payload as part of its file's bytes, \
                                      copied into its own memory; no capability hands it a \
                                      texture or a buffer apart from the file";

    /// There is no capability for touching a GPU resource.
    pub const GPU_ACCESS: &str = "a WASM guest cannot hold a GPU resource; \
                                  rendering stays the host's";
}
