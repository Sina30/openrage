//! The embedded plugin host: what a plugin declares, what the user answered,
//! and why a plugin did nothing.
//!
//! ⚠ A plugin's capabilities are declared in its manifest and answered once,
//! before it first runs; the host has no call for asking mid-run. Every
//! refusal is kept as the host's own diagnostic, key and sentence, because an
//! unshown refusal is a plugin that appears to work and does nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7, Rsc7Seal};
use or_io::gta5::formats::Format;
use or_io::{Diagnostic, Generation, Severity};
use or_plugin::{
    Because, Capability, Grants, HostError, Limits, Manifest, Outcome, Refused, Runtime, SaveCheck,
    Status, Subject, WriteGate, ABI_VERSION,
};

use self::transaction::{PluginCheck, ProjectFiles};
use super::payload_handlers::PayloadHandler;
use super::{save_prefs, AppState, Document, RetailGuard};

#[cfg(test)]
mod a_plugin_adds_to_the_editor;
#[cfg(test)]
mod a_plugin_writes_files_together;
#[cfg(test)]
mod a_validator_suggests_a_fix_and_reads_the_pack;
#[cfg(test)]
mod an_older_plugin_still_loads;
pub mod extensions;
mod install;
pub mod surface;
#[cfg(test)]
mod the_cloth_painter_installs_as_a_plugin;
pub mod transaction;
pub mod waivers;

/// The export the app runs in a guest.
const ENTRY: &str = "go";

/// What the user answered about a plugin, remembered in the preferences against its id.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Consent {
    /// The capability keys granted; a manifest that wants one more is asked again.
    Granted(BTreeSet<String>),
    Declined,
}

/// ⚠ Each answer is read on its own: a field `serde` cannot parse fails the
/// whole preferences, so an unreadable answer costs only itself and is asked again.
pub fn readable_consent<'de, D>(deserializer: D) -> Result<BTreeMap<String, Consent>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, answer)| Some((id.clone(), serde_json::from_value(answer.clone()).ok()?)))
        .collect())
}

/// A plugin directory: `plugin.json` and `plugin.wasm`.
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    pub wasm: PathBuf,
    /// Why it cannot run at all; kept, because a broken plugin that vanished from the list reads as one never installed.
    pub refused: Option<String>,
}

/// `plugin.json`: capabilities are named by their stable key (`cap.edit_open_document`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    id: String,
    name: String,
    abi: u32,
    wants: Vec<String>,
}

impl InstalledPlugin {
    /// What the Plugins panel says about the plugin beside what it asks for: that it is built for an older interface and still loads.
    pub fn notes(&self) -> Vec<String> {
        if self.refused.is_some() {
            return Vec::new();
        }
        self.manifest.age_note().into_iter().collect()
    }

    /// Read `dir`; a manifest the host refuses is kept with the reason.
    pub fn read(dir: &Path) -> Self {
        let wasm = dir.join("plugin.wasm");
        let manifest_path = dir.join("plugin.json");
        let file = std::fs::read(&manifest_path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<ManifestFile>(&bytes).map_err(|e| e.to_string())
            });
        let file = match file {
            Ok(file) => file,
            Err(why) => {
                let named = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return Self {
                    manifest: Manifest {
                        id: named.clone(),
                        name: named,
                        abi: ABI_VERSION,
                        wants: Vec::new(),
                    },
                    wasm,
                    refused: Some(format!("{}: {why}", manifest_path.display())),
                };
            }
        };

        let unknown: Vec<&str> = file
            .wants
            .iter()
            .map(String::as_str)
            .filter(|key| !Capability::ALL.iter().any(|c| c.key() == *key))
            .collect();
        let manifest = Manifest {
            id: file.id.clone(),
            name: file.name.clone(),
            abi: file.abi,
            wants: Capability::ALL
                .into_iter()
                .filter(|c| file.wants.iter().any(|key| key == c.key()))
                .collect(),
        };
        let refused = if unknown.is_empty() {
            manifest
                .accept()
                .err()
                .map(|r| r.to_string())
                .or_else(|| (!wasm.is_file()).then(|| format!("{} is missing", wasm.display())))
        } else {
            Some(format!(
                "the plugin asks for {}, which this build does not define",
                unknown.join(", ")
            ))
        };
        Self {
            manifest,
            wasm,
            refused,
        }
    }
}

/// Every plugin directory under `dir`, in name order; a directory with no `plugin.json` is not one.
pub fn installed_in(dir: &Path) -> Vec<InstalledPlugin> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("plugin.json").is_file())
        .collect();
    dirs.sort();
    dirs.iter().map(|d| InstalledPlugin::read(d)).collect()
}

/// Where plugins are installed, beside the preferences.
pub fn plugins_dir() -> Option<PathBuf> {
    super::prefs::config_root().map(|root| root.join("plugins"))
}

/// What this session knows about plugins.
/// What one plugin run may spend in the editor: 4 billion instructions, about 3 s under wasmi, covers a
/// guest parsing the median `.ydd` with `or-io` (100 to 300 million) and most large ones; the largest
/// shipped (3 to 10 billion) do not fit. ⚠ A run is synchronous on the UI thread, so this is also the
/// longest a spinning plugin freezes the editor (owner, 2026-10-05).
pub(crate) const EDITOR_LIMITS: Limits = Limits {
    fuel: Some(4_000_000_000),
    memory_pages: 256,
};

pub struct PluginSession {
    /// Read once, because the pages listing it draw every frame.
    pub(crate) scanned: Option<Vec<InstalledPlugin>>,
    /// A plugin waiting on the user's answer before it runs.
    pub pending_consent: Option<InstalledPlugin>,
    reports: BTreeMap<String, Vec<Diagnostic>>,
    /// What one run may spend: [`EDITOR_LIMITS`], not `Limits::default`, whose 10 million stops a guest
    /// that parses with `or-io` (`what_a_guest_parsing_a_ydd_spends_against_the_default_fuel`).
    pub(crate) limits: Limits,
    /// The plugins that stay loaded to add to the editor.
    pub(super) surfaces: surface::Surfaces,
}

impl Default for PluginSession {
    fn default() -> Self {
        Self {
            scanned: None,
            pending_consent: None,
            reports: BTreeMap::new(),
            limits: EDITOR_LIMITS,
            surfaces: surface::Surfaces::default(),
        }
    }
}

impl PluginSession {
    pub(super) fn set_report(&mut self, id: &str, report: Vec<Diagnostic>) {
        self.reports.insert(id.to_string(), report);
    }
}

/// The editor's edit mode as the host's write gate.
pub(super) struct EditModeGate(Option<String>);

impl WriteGate for EditModeGate {
    fn why_not_writable(&self) -> Option<String> {
        self.0.clone()
    }
}

/// The format's own read-back and canonical save, run on a patched image or a written file before the host keeps it.
pub(super) struct FormatSaveCheck {
    path: PathBuf,
    seal: Rsc7Seal,
    format: &'static Format,
    target: Generation,
    handler: PayloadHandler,
}

impl SaveCheck for FormatSaveCheck {
    fn why_the_document_would_be_invalid(&self, document: &[u8]) -> Option<String> {
        let sealed = match Rsc7::reseal(document, &self.seal, Compression::default()) {
            Ok(bytes) => bytes,
            Err(e) => return Some(format!("the patched document would not re-envelope: {e}")),
        };
        match (self.handler.parse)(&self.path, &sealed) {
            Ok(payload) => self.handler.save(&payload).err(),
            Err(e) => Some(format!("the patched document does not read back: {e}")),
        }
    }

    /// ⚠ The version is read first: a file at the other target reads with this one's layout and returns plausible nonsense rather than failing.
    fn why_the_file_would_be_invalid(&self, file: &[u8]) -> Option<String> {
        let seal = match Rsc7::peek(file) {
            Ok(seal) => seal,
            Err(e) => return Some(format!("the bytes are not a game resource file: {e}")),
        };
        match self.format.target_of_version(seal.version()) {
            Ok(target) if target == self.target => {}
            Ok(target) => {
                return Some(format!(
                    "the bytes are a {} file and the open document is a {} one",
                    target.edition_label(),
                    self.target.edition_label()
                ))
            }
            Err(e) => return Some(format!("the bytes are not a file of this format: {e}")),
        }
        match (self.handler.parse)(&self.path, file) {
            Ok(payload) => self.handler.save(&payload).err(),
            Err(e) => Some(format!("the bytes do not read back: {e}")),
        }
    }
}

pub(super) fn save_check(doc: &Document) -> Option<FormatSaveCheck> {
    let image = doc.resource.as_ref()?;
    let format = or_io::gta5::formats::format(doc.kind.extension())?;
    let target = format.target_of_version(image.seal.version()).ok()?;
    let handler = super::payload_handlers::handler_for(doc.kind.resource_type()?, target)?;
    Some(FormatSaveCheck {
        path: doc.path.clone().unwrap_or_else(|| PathBuf::from(&doc.name)),
        seal: image.seal.clone(),
        format,
        target,
        handler,
    })
}

pub(super) fn not_run(why: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::new(Severity::Warning, "plugin.not_run", why)]
}

/// What a run is worth telling a user, refusals first.
pub(super) fn report_of(ran: Result<Outcome, HostError>) -> Vec<Diagnostic> {
    let outcome = match ran {
        Ok(outcome) => outcome,
        Err(HostError::Unresolved {
            needs: Some(capability),
            ..
        }) => {
            return vec![Refused {
                capability,
                because: Because::NotGranted,
            }
            .diagnostic()]
        }
        Err(HostError::StoppedAfterRefusal { why, refused }) => {
            let mut report: Vec<Diagnostic> = refused.iter().map(Refused::diagnostic).collect();
            report.push(Diagnostic::new(
                Severity::Warning,
                "plugin.failed",
                format!("it stopped after the refusal ({why}) and changed nothing"),
            ));
            return report;
        }
        Err(error) => {
            return vec![Diagnostic::new(
                Severity::Warning,
                "plugin.failed",
                error.to_string(),
            )]
        }
    };
    let mut report: Vec<Diagnostic> = outcome.refused.iter().map(Refused::diagnostic).collect();
    report.push(if outcome.touched {
        Diagnostic::info(
            "plugin.changed",
            "it changed the open document; save to keep the change",
        )
    } else {
        Diagnostic::info("plugin.unchanged", "it ran and changed nothing")
    });
    // ⚠ A short buffer and an absent child are how a guest reads, not faults;
    // a capability refusal is already reported above.
    report.extend(
        outcome
            .failures
            .iter()
            .filter(|f| {
                !matches!(
                    f.status,
                    Status::BufferTooSmall
                        | Status::NullPointer
                        | Status::NotGranted
                        | Status::NotWritable
                )
            })
            .map(|f| {
                let why = f.because.as_deref().unwrap_or(f.status.means());
                Diagnostic::new(
                    Severity::Info,
                    f.status.key(),
                    format!("{}: {why}", f.function),
                )
            }),
    );
    report
}

/// `report` with the line about the files the run wrote together; a run that wrote some did not "change nothing".
pub(super) fn with_what_was_written(
    report: Vec<Diagnostic>,
    wrote: Option<Diagnostic>,
) -> Vec<Diagnostic> {
    let wrote_some = wrote.is_some();
    (report.into_iter())
        .filter(|line| !(wrote_some && line.key == "plugin.unchanged"))
        .chain(wrote)
        .collect()
}

impl AppState {
    /// The installed plugins, read from [`plugins_dir`] once per session.
    pub fn installed_plugins(&mut self) -> &[InstalledPlugin] {
        self.plugins
            .scanned
            .get_or_insert_with(|| plugins_dir().map(|d| installed_in(&d)).unwrap_or_default())
    }

    pub fn rescan_plugins(&mut self) {
        self.plugins.scanned = None;
        self.plugins.surfaces.reset();
    }

    /// Whether the user has answered yes to every capability `plugin` declares.
    pub(super) fn is_consented(&self, plugin: &InstalledPlugin) -> bool {
        matches!(
            self.prefs.plugin_consent.get(&plugin.manifest.id),
            Some(Consent::Granted(keys)) if plugin.manifest.wants.iter().all(|c| keys.contains(c.key()))
        )
    }

    /// The editor's edit mode for `doc`'s path, as a write gate.
    ///
    /// ⚠ A plugin is refused where a person would be asked: a confirmation raised by a background
    /// call is one that gets clicked through.
    pub(super) fn plugin_write_gate(&self, doc: &Document) -> EditModeGate {
        EditModeGate(match self.retail_guard_for(doc.path.as_deref()) {
            RetailGuard::Allowed => None,
            RetailGuard::Refused(why) => Some(why),
            RetailGuard::NeedsConfirmation(path) => Some(format!(
                "`{}` is part of the game as shipped, and a plugin is not offered the \
                 confirmation a save is; save it yourself to be asked",
                path.display()
            )),
        })
    }

    /// What the last run of plugin `id` reported.
    pub fn plugin_report(&self, id: &str) -> Option<&[Diagnostic]> {
        self.plugins.reports.get(id).map(Vec::as_slice)
    }

    /// Run plugin `id` on the active document, asking first while it wants something unanswered.
    pub fn request_plugin_run(&mut self, id: &str) {
        let Some(plugin) = self
            .installed_plugins()
            .iter()
            .find(|p| p.manifest.id == id)
            .cloned()
        else {
            return;
        };
        if let Some(why) = &plugin.refused {
            let report = vec![Diagnostic::new(
                Severity::Warning,
                "plugin.rejected",
                why.clone(),
            )];
            return self.report_plugin(&plugin, report);
        }
        let wants = &plugin.manifest.wants;
        let report = match self.prefs.plugin_consent.get(id) {
            Some(Consent::Declined) => wants
                .iter()
                .map(|&capability| {
                    Refused {
                        capability,
                        because: Because::NotGranted,
                    }
                    .diagnostic()
                })
                .collect(),
            Some(Consent::Granted(keys)) if wants.iter().all(|c| keys.contains(c.key())) => {
                let grants = wants
                    .iter()
                    .fold(Grants::none(), |grants, c| grants.granting(*c));
                // A plugin that adds to the editor stays loaded; one that extends a build runs over
                // every plan from now on; the rest run once on the open document.
                if plugin.manifest.adds_to_the_editor() {
                    self.open_plugin_surface(&plugin)
                } else if plugin.manifest.extends_a_build() {
                    self.workbench.want_scan();
                    vec![Diagnostic::info(
                        "plugin.extends",
                        "it runs over the plan of every build from now on: what it finds is in the Problems list and what it adds is in the plan",
                    )]
                } else {
                    self.run_plugin(&plugin, grants)
                }
            }
            _ => {
                self.plugins.pending_consent = Some(plugin);
                return;
            }
        };
        self.report_plugin(&plugin, report);
    }

    /// Record the answer to the pending prompt in the preferences, then run or refuse as answered.
    pub fn answer_plugin_consent(&mut self, grant: bool) {
        let Some(plugin) = self.plugins.pending_consent.take() else {
            return;
        };
        let consent = if grant {
            Consent::Granted(
                plugin
                    .manifest
                    .wants
                    .iter()
                    .map(|c| c.key().to_string())
                    .collect(),
            )
        } else {
            Consent::Declined
        };
        self.prefs
            .plugin_consent
            .insert(plugin.manifest.id.clone(), consent);
        save_prefs(&self.prefs);
        self.request_plugin_run(&plugin.manifest.id);
    }

    /// Forget the answer about plugin `id`, so its next run asks again.
    pub fn revoke_plugin_consent(&mut self, id: &str) {
        self.close_plugin_surface(id);
        if self.prefs.plugin_consent.remove(id).is_some() {
            save_prefs(&self.prefs);
            // ⚠ A plugin that extends a build took part in the last plan, which is stale without it.
            self.workbench.want_scan();
        }
    }

    fn report_plugin(&mut self, plugin: &InstalledPlugin, report: Vec<Diagnostic>) {
        let name = &plugin.manifest.name;
        match report.iter().find(|d| d.severity > Severity::Info) {
            Some(refusal) => self.show_warning_toast(format!("{name}: {}", refusal.message)),
            None => {
                if let Some(first) = report.first() {
                    self.show_info_toast(format!("{name}: {}", first.message));
                }
            }
        }
        self.plugins
            .reports
            .insert(plugin.manifest.id.clone(), report);
    }

    fn run_plugin(&mut self, plugin: &InstalledPlugin, grants: Grants) -> Vec<Diagnostic> {
        let Some(doc_id) = self.active_document else {
            return not_run("a plugin runs on the open document, and none is open");
        };
        let Some(doc) = self.document(doc_id) else {
            return not_run("the open document was closed");
        };
        let Some((pack, root)) = super::described_root(doc) else {
            return not_run(
                "the open document has no described tree for a plugin to read: it is \
                 not a declared resource, or it was edited since it was opened",
            );
        };
        // ⚠ One check for the run: the format's own for the file it replaces, the project's for the files it writes together.
        let files = grants.has(Capability::WriteFiles);
        let check = PluginCheck {
            document: save_check(doc).map(|check| Box::new(check) as Box<dyn SaveCheck>),
            files: files.then(|| ProjectFiles::of(self)).flatten(),
        };
        let mut gate = self.plugin_write_gate(doc);
        if files && gate.0.is_none() {
            gate = EditModeGate(self.read_only_reason());
        }
        // ⚠ The file is the document as a stored RSC7 file, made once and only
        // for a plugin that asked for the file's bytes: a copy of the whole
        // document is not what every run should cost.
        let file =
            if grants.has(Capability::ReadFileBytes) || grants.has(Capability::WriteFileBytes) {
                let Some(image) = doc.resource.as_ref() else {
                    return not_run("the open document holds no image of its bytes");
                };
                match Rsc7::reseal(&image.data, &image.seal, Compression::none()) {
                    Ok(bytes) => Some(bytes),
                    Err(e) => return not_run(format!("the open document would not seal: {e}")),
                }
            } else {
                None
            };
        let kind = doc.kind;
        let wasm = match std::fs::read(&plugin.wasm) {
            Ok(wasm) => wasm,
            Err(e) => return not_run(format!("{}: {e}", plugin.wasm.display())),
        };

        let runtime = Runtime::new(self.plugins.limits);
        let mut ran = None;
        if let Some(doc) = self.document_mut(doc_id) {
            let label = format!("{} changed the document", plugin.manifest.name);
            doc.patch_resource_image(&label, |image| {
                // ⚠ A guest that patches and then fails returns no outcome to
                // say so, so a failed run puts every byte back.
                let before = image.data.clone();
                let mut subject =
                    Subject::editing(&pack, root.as_str(), &mut image.data, image.virtual_size);
                if let Some(file) = &file {
                    subject = subject.with_file(file);
                }
                let mut outcome = runtime.run(&wasm, ENTRY, subject, grants, gate, &check);
                // The file the guest wrote, which the check has read back and saved, is the document's image.
                if let Ok(Outcome {
                    replaced: Some(bytes),
                    ..
                }) = &outcome
                {
                    match super::document_io::resource_image_for(kind, bytes)
                        .and_then(Arc::into_inner)
                    {
                        Some(written) => *image = written,
                        None => {
                            outcome = Err(HostError::Guest(
                                "the written file would not open as the document's image".into(),
                            ))
                        }
                    }
                }
                let touched = matches!(&outcome, Ok(o) if o.touched);
                if outcome.is_err() {
                    image.data = before;
                }
                ran = Some(outcome);
                touched
            });
        }
        self.catch_up(doc_id);
        let wrote = match ran.as_mut() {
            Some(Ok(outcome)) => self.apply_what_a_plugin_wrote(&plugin.manifest.name, outcome),
            _ => None,
        };
        match ran {
            Some(ran) => with_what_was_written(report_of(ran), wrote),
            None => not_run("the open document holds no image of its bytes"),
        }
    }

    /// Write the files a plugin's call wrote together, as one step of the project's History, and say what came of it.
    pub(super) fn apply_what_a_plugin_wrote(
        &mut self,
        name: &str,
        outcome: &mut Outcome,
    ) -> Option<Diagnostic> {
        let set = outcome.transaction.take()?;
        let count = set.files.len();
        let noun = if count == 1 { "file" } else { "files" };
        Some(
            match self.apply_plugin_files(&format!("{name} wrote {count} {noun}"), &set) {
                Ok(applied) => Diagnostic::info("plugin.wrote", applied.said()),
                Err(why) => Diagnostic::warning("plugin.not_wrote", why),
            },
        )
    }
}
