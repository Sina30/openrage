//! What a loaded plugin adds to the editor: its panels, commands and settings, the overlay it
//! draws over the open document, the clicks it takes, and the call into its guest each of them costs.
//!
//! ⚠ A plugin that wants to add to the editor stays loaded, so it is loaded only after the user
//! has answered for every capability it declared: at startup for a plugin already answered, and
//! from Automate ▸ Run plugin for one not yet asked. What the guest registers is the host's to show
//! and the user's to keep; revoking the answer takes the plugin out of the editor again.

use std::collections::BTreeMap;
use std::sync::Arc;

use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7};
use or_io::{Diagnostic, Severity};
use or_plugin::{
    Call, Capability, Event, HostError, Manifest, Outcome, Pick, Primitive, Registrations,
    SaveCheck, Session, SettingKind, SettingValue, Widget,
};

use super::transaction::{PluginCheck, ProjectFiles};
use super::{not_run, report_of, save_check, with_what_was_written, EditModeGate, InstalledPlugin};
use crate::state::{save_prefs, AppState, DocumentId};
use crate::ui::shortcuts::{self, ShortcutMods};

/// A plugin whose guest stays loaded.
pub(super) struct LivePlugin {
    manifest: Manifest,
    session: Session,
    /// The binding each of the guest's commands got, by command id; a command whose binding was taken has none.
    bindings: BTreeMap<String, (ShortcutMods, egui::Key)>,
    /// The document that was open when the guest last drew its overlay, which is the only one the overlay shows over.
    overlay_over: Option<DocumentId>,
}

/// The file a document would save, kept for the plugins that read its bytes until the document changes.
struct ServedFile {
    doc: DocumentId,
    edits: u64,
    /// ⚠ A reload gives the document another image without counting an edit, so the edit count alone would serve the file from before it.
    contents: u64,
    bytes: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct Surfaces {
    /// Whether the plugins already answered for have been loaded this session.
    loaded: bool,
    live: BTreeMap<String, LivePlugin>,
    file: Option<ServedFile>,
    /// Settings changed in memory and not yet written, because a drag changes one every frame.
    unsaved: bool,
}

/// A command a loaded plugin registered, as the command registry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    pub plugin: String,
    pub id: String,
    pub label: String,
    pub binding: Option<String>,
    /// Why it cannot run now.
    pub stopped: Option<String>,
}

/// ⚠ Each plugin's settings are read on their own: a value `serde` cannot parse fails the whole
/// preferences, so an unreadable entry costs only itself and the plugin's settings start at their defaults.
pub fn readable_plugin_settings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(plugin, settings)| {
            let settings = settings
                .as_object()?
                .iter()
                .map(|(id, value)| (id.clone(), value.clone()))
                .collect();
            Some((plugin.clone(), settings))
        })
        .collect())
}

fn stored(value: &SettingValue) -> serde_json::Value {
    match value {
        SettingValue::Bool(v) => serde_json::Value::from(*v),
        SettingValue::Number(v) => serde_json::Value::from(*v),
        SettingValue::Text(v) => serde_json::Value::from(v.as_str()),
    }
}

/// A stored value as `kind` holds it; one of another kind, or out of range, is not read.
fn read(kind: &SettingKind, value: &serde_json::Value) -> Option<SettingValue> {
    let value = match kind {
        SettingKind::Bool { .. } => SettingValue::Bool(value.as_bool()?),
        SettingKind::Number { .. } => SettingValue::Number(value.as_f64()?),
        SettingKind::Text { .. } => SettingValue::Text(value.as_str()?.to_string()),
    };
    kind.keep(value)
}

impl Surfaces {
    /// Forget every loaded plugin, so the next [`AppState::load_plugin_surfaces`] reads them again.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

impl AppState {
    /// Load, once per session, every plugin that adds to the editor and whose user has already answered yes.
    pub fn load_plugin_surfaces(&mut self) {
        if std::mem::replace(&mut self.plugins.surfaces.loaded, true) {
            return;
        }
        let answered: Vec<InstalledPlugin> = self
            .installed_plugins()
            .to_vec()
            .into_iter()
            .filter(|p| p.refused.is_none() && p.manifest.adds_to_the_editor())
            .filter(|p| self.is_consented(p))
            .collect();
        for plugin in answered {
            let report = self.open_plugin_surface(&plugin);
            self.report_quietly(&plugin.manifest, report);
        }
    }

    /// Instantiate `plugin`'s guest, run its `register` and put what it registered in the editor.
    pub(super) fn open_plugin_surface(&mut self, plugin: &InstalledPlugin) -> Vec<Diagnostic> {
        let id = plugin.manifest.id.clone();
        self.close_plugin_surface(&id);
        let wasm = match std::fs::read(&plugin.wasm) {
            Ok(wasm) => wasm,
            Err(e) => return not_run(format!("{}: {e}", plugin.wasm.display())),
        };
        let mut session =
            match Session::open(&wasm, plugin.manifest.requested(), self.plugins.limits) {
                Ok(session) => session,
                Err(e) => return report_of(Err(e)),
            };

        let chosen = self
            .prefs
            .plugin_settings
            .get(&id)
            .cloned()
            .unwrap_or_default();
        for spec in session.registered().settings.clone() {
            let value = chosen.get(&spec.id).and_then(|v| read(&spec.kind, v));
            if let Some(value) = value {
                session.set_setting(&spec.id, value);
            }
        }

        // ⚠ A registration the host refused is a panel, command or setting the user will look for and not find.
        let mut report: Vec<Diagnostic> = report_of(Ok(session.started().clone()))
            .into_iter()
            .filter(|d| d.key != "plugin.unchanged")
            .collect();
        let mut bindings = BTreeMap::new();
        for command in &session.registered().commands {
            let Some(text) = &command.shortcut else {
                continue;
            };
            let taken = |(mods, key)| {
                bindings
                    .values()
                    .any(|bound| shortcuts::binding_matches(*bound, mods, key))
            };
            match self.plugin_binding(text, taken) {
                Ok(binding) => {
                    bindings.insert(command.id.clone(), binding);
                }
                Err(why) => report.push(
                    Diagnostic::warning(
                        "plugin.shortcut",
                        format!("`{}` has no binding: {why}", command.label),
                    )
                    .with("command", command.id.as_str()),
                ),
            }
        }
        for panel in &session.registered().panels {
            crate::ui::panels::plugin_panel::register(&id, &panel.id);
        }

        let registered = session.registered();
        let noun = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
        report.push(Diagnostic::info(
            "plugin.loaded",
            format!(
                "it added {}, {} and {}",
                noun(registered.panels.len(), "panel"),
                noun(registered.commands.len(), "command"),
                noun(registered.settings.len(), "setting"),
            ),
        ));
        self.plugins.surfaces.live.insert(
            id,
            LivePlugin {
                manifest: plugin.manifest.clone(),
                session,
                bindings,
                overlay_over: None,
            },
        );
        report
    }

    /// Take a plugin out of the editor, with its overlay and its clicks; its panels stay registered and say it is not loaded.
    pub(super) fn close_plugin_surface(&mut self, id: &str) {
        self.plugins.surfaces.live.remove(id);
    }

    /// A binding a plugin's command may take: one that names a key, holds Ctrl or Alt, and no command already holds.
    fn plugin_binding(
        &self,
        text: &str,
        taken_by_its_own: impl Fn((ShortcutMods, egui::Key)) -> bool,
    ) -> Result<(ShortcutMods, egui::Key), String> {
        let Some((mods, key)) = shortcuts::parse_binding(text) else {
            return Err(format!("`{text}` does not name a key"));
        };
        if !(mods.ctrl || mods.alt) {
            return Err(format!(
                "`{text}` has no Ctrl or Alt, and a plugin does not take a key a field is typed with"
            ));
        }
        let taken = shortcuts::action_for(mods, key, &self.prefs).is_some()
            || self.plugin_command_bound_to(mods, key).is_some()
            || taken_by_its_own((mods, key));
        if taken {
            return Err(format!("another command already holds `{text}`"));
        }
        Ok((mods, key))
    }

    /// The plugin and command a key press runs, if a loaded plugin's command holds the key.
    pub fn plugin_command_bound_to(
        &self,
        mods: ShortcutMods,
        key: egui::Key,
    ) -> Option<(String, String)> {
        self.plugins
            .surfaces
            .live
            .iter()
            .find_map(|(plugin, live)| {
                live.bindings.iter().find_map(|(command, bound)| {
                    shortcuts::binding_matches(*bound, mods, key)
                        .then(|| (plugin.clone(), command.clone()))
                })
            })
    }

    /// What a loaded plugin registered.
    pub fn plugin_registrations(&self, plugin: &str) -> Option<&Registrations> {
        Some(self.plugins.surfaces.live.get(plugin)?.session.registered())
    }

    /// The loaded plugins that declared settings, each with its name, for Settings to give a page.
    pub fn plugin_setting_pages(&self) -> Vec<(String, String)> {
        let live = &self.plugins.surfaces.live;
        live.iter()
            .filter(|(_, p)| !p.session.registered().settings.is_empty())
            .map(|(id, p)| (id.clone(), p.manifest.name.clone()))
            .collect()
    }

    /// The commands every loaded plugin registered, in plugin order.
    pub fn plugin_commands(&self) -> Vec<PluginCommand> {
        let mut out = Vec::new();
        for (plugin, live) in &self.plugins.surfaces.live {
            for command in &live.session.registered().commands {
                out.push(PluginCommand {
                    plugin: plugin.clone(),
                    id: command.id.clone(),
                    label: command.label.clone(),
                    binding: live
                        .bindings
                        .get(&command.id)
                        .map(|(mods, key)| shortcuts::format_binding(*mods, *key)),
                    stopped: live.session.stopped().map(String::from),
                });
            }
        }
        out
    }

    /// The title a plugin's panel shows on its tab.
    pub fn plugin_panel_title(&self, plugin: &str, panel: &str) -> Option<String> {
        let panels = &self.plugin_registrations(plugin)?.panels;
        Some(panels.iter().find(|p| p.id == panel)?.title.clone())
    }

    /// Run plugin `plugin`'s `draw_panel` for `panel` and return the widgets it described.
    ///
    /// ⚠ Called every frame the panel is visible, on the thread that draws it: the fuel of one call
    /// is the longest a plugin stalls a frame.
    pub fn plugin_panel(&mut self, plugin: &str, panel: &str) -> Result<Vec<Widget>, String> {
        let Some(live) = self.plugins.surfaces.live.get(plugin) else {
            return Err("the plugin is not loaded; Automate ▸ Run plugin loads it".into());
        };
        if let Some(why) = live.session.stopped() {
            return Err(format!("it stopped: {why}"));
        }
        let Some(index) = (live.session.registered().panels.iter()).position(|p| p.id == panel)
        else {
            return Err(format!("it registers no panel `{panel}`"));
        };
        let manifest = live.manifest.clone();
        let (call, doc) = self.plugin_call(&manifest);
        let live = self
            .plugins
            .surfaces
            .live
            .get_mut(plugin)
            .expect("it was loaded");
        let before = live.session.overlay_epoch();
        let ran = live.session.draw_panel(index, call);
        self.mark_overlay(plugin, before);
        match ran {
            Ok(drawn) => {
                self.after_plugin_call(&manifest, doc, Ok(drawn.outcome), Tell::Refusals);
                Ok(drawn.widgets)
            }
            Err(error) => {
                let why = error.to_string();
                self.after_plugin_call(&manifest, doc, Err(error), Tell::Everything);
                Err(why)
            }
        }
    }

    /// What the user did in a plugin's panel, for that panel's next frame to answer with.
    pub fn deliver_plugin_events(&mut self, plugin: &str, panel: &str, events: Vec<Event>) {
        let Some(live) = self.plugins.surfaces.live.get_mut(plugin) else {
            return;
        };
        if let Some(index) = (live.session.registered().panels.iter()).position(|p| p.id == panel) {
            events
                .into_iter()
                .for_each(|event| live.session.deliver(index, event));
        }
    }

    /// Run a command a plugin registered.
    pub fn run_plugin_command(&mut self, plugin: &str, command: &str) {
        let Some(live) = self.plugins.surfaces.live.get(plugin) else {
            return;
        };
        let Some(index) = (live.session.registered().commands.iter()).position(|c| c.id == command)
        else {
            return;
        };
        let manifest = live.manifest.clone();
        let (call, doc) = self.plugin_call(&manifest);
        let live = self
            .plugins
            .surfaces
            .live
            .get_mut(plugin)
            .expect("it was loaded");
        let before = live.session.overlay_epoch();
        let ran = live.session.run_command(index, call);
        self.mark_overlay(plugin, before);
        self.after_plugin_call(&manifest, doc, ran, Tell::Everything);
    }

    /// The value of a setting a loaded plugin declared.
    pub fn plugin_setting(&self, plugin: &str, id: &str) -> Option<SettingValue> {
        self.plugins
            .surfaces
            .live
            .get(plugin)?
            .session
            .setting(id)
            .cloned()
    }

    /// Set a setting, kept to what the plugin declared; [`AppState::persist_plugin_settings`] writes it.
    pub fn set_plugin_setting(&mut self, plugin: &str, id: &str, value: SettingValue) {
        let Some(live) = self.plugins.surfaces.live.get_mut(plugin) else {
            return;
        };
        let Some(kept) = live.session.set_setting(id, value).cloned() else {
            return;
        };
        self.prefs
            .plugin_settings
            .entry(plugin.to_string())
            .or_default()
            .insert(id.to_string(), stored(&kept));
        self.plugins.surfaces.unsaved = true;
    }

    /// Write the settings changed since the last write; not while a drag is changing one every frame.
    pub fn persist_plugin_settings(&mut self) {
        if std::mem::take(&mut self.plugins.surfaces.unsaved) {
            save_prefs(&self.prefs);
        }
    }

    /// What loaded plugins draw over the document that is open, which is none over any other.
    pub fn plugin_overlays(&self) -> Vec<&[Primitive]> {
        let Some(active) = self.active_document else {
            return Vec::new();
        };
        let live = self.plugins.surfaces.live.values();
        live.filter(|p| p.overlay_over == Some(active))
            .map(|p| p.session.overlay())
            .filter(|overlay| !overlay.is_empty())
            .collect()
    }

    /// Whether a loaded plugin asked for the viewport's clicks, which then select nothing.
    pub fn plugin_takes_clicks(&self) -> bool {
        let mut live = self.plugins.surfaces.live.values();
        live.any(|p| p.session.takes_clicks() && p.session.stopped().is_none())
    }

    /// Tell each plugin that asked for clicks what a click landed on.
    pub fn plugin_pick(&mut self, pick: Pick) {
        let asking: Vec<String> = (self.plugins.surfaces.live.iter())
            .filter(|(_, p)| p.session.takes_clicks() && p.session.stopped().is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for plugin in asking {
            let Some(live) = self.plugins.surfaces.live.get(&plugin) else {
                continue;
            };
            let manifest = live.manifest.clone();
            let (call, doc) = self.plugin_call(&manifest);
            let live = self
                .plugins
                .surfaces
                .live
                .get_mut(&plugin)
                .expect("it was loaded");
            let before = live.session.overlay_epoch();
            let ran = live.session.picked(pick, call);
            self.mark_overlay(&plugin, before);
            self.after_plugin_call(&manifest, doc, ran, Tell::Everything);
        }
    }

    /// Remember which document the overlay was drawn over, once a call changed it.
    fn mark_overlay(&mut self, plugin: &str, epoch_before: u64) {
        let active = self.active_document;
        if let Some(live) = self.plugins.surfaces.live.get_mut(plugin) {
            if live.session.overlay_epoch() != epoch_before {
                live.overlay_over = active;
            }
        }
    }

    /// What one call of `manifest`'s guest may reach: the open document's file for a plugin that reads or writes
    /// one, and for one that writes it the write gate and save check as well.
    ///
    /// ⚠ A frame is a call, so a plugin that holds neither capability pays for none of this: the gate
    /// asks the file system whether the path is in an install.
    fn plugin_call(&mut self, manifest: &Manifest) -> (Call, Option<DocumentId>) {
        let writes = manifest.wants.contains(&Capability::WriteFileBytes);
        let reads = writes || manifest.wants.contains(&Capability::ReadFileBytes);
        let files = manifest.wants.contains(&Capability::WriteFiles);
        let doc_id = self.active_document.filter(|_| reads);
        let doc = doc_id.and_then(|id| self.document(id));
        if doc.is_none() && !files {
            return (Call::without_a_document(), None);
        }
        // ⚠ One gate and one check for the call: a plugin that replaces the open file and writes files together is held to both.
        let mut call = if writes || files {
            let mut gate = match (writes, doc) {
                (true, Some(doc)) => self.plugin_write_gate(doc),
                (true, None) => EditModeGate(Some("no document is open for this call".into())),
                (false, _) => EditModeGate(None),
            };
            if files && gate.0.is_none() {
                gate = EditModeGate(self.read_only_reason());
            }
            let check = PluginCheck {
                document: doc
                    .filter(|_| writes)
                    .and_then(save_check)
                    .map(|check| Box::new(check) as Box<dyn SaveCheck>),
                files: files.then(|| ProjectFiles::of(self)).flatten(),
            };
            Call::new(gate, check)
        } else {
            Call::without_a_document()
        };
        let Some(doc_id) = doc_id.filter(|_| doc.is_some()) else {
            return (call, None);
        };
        if let Some(file) = self.served_file(doc_id) {
            call = call.with_file(file);
        }
        (call, Some(doc_id))
    }

    /// The file `doc_id` would save, as a stored RSC7 file, built again only once the document has changed.
    pub(in crate::state) fn served_file(&mut self, doc_id: DocumentId) -> Option<Arc<Vec<u8>>> {
        let doc = self.document(doc_id)?;
        let (edits, contents) = (doc.edit_epoch(), doc.payload_epoch());
        if let Some(file) = &self.plugins.surfaces.file {
            if file.doc == doc_id && file.edits == edits && file.contents == contents {
                return Some(file.bytes.clone());
            }
        }
        let image = doc.resource.as_ref()?;
        let bytes = Arc::new(Rsc7::reseal(&image.data, &image.seal, Compression::none()).ok()?);
        self.plugins.surfaces.file = Some(ServedFile {
            doc: doc_id,
            edits,
            contents,
            bytes: bytes.clone(),
        });
        Some(bytes)
    }

    /// What a call did to the document, and what the user is told about it.
    fn after_plugin_call(
        &mut self,
        manifest: &Manifest,
        doc: Option<DocumentId>,
        mut ran: Result<Outcome, HostError>,
        tell: Tell,
    ) {
        // The file the guest wrote, which the check has read back and saved, is the document's image.
        if let (Ok(Outcome { replaced, .. }), Some(doc_id)) = (&mut ran, doc) {
            if let Some(bytes) = replaced.take() {
                if let Err(why) = self.apply_plugin_file(doc_id, &manifest.name, bytes) {
                    ran = Err(HostError::Guest(why));
                }
            }
        }
        // The files it wrote together are written now, as one step of the project's History.
        let wrote = match &mut ran {
            Ok(outcome) => self.apply_what_a_plugin_wrote(&manifest.name, outcome),
            Err(_) => None,
        };
        let report = with_what_was_written(report_of(ran), wrote);
        match tell {
            Tell::Everything => self.report_quietly(manifest, report),
            // A frame is drawn many times a second: only what is wrong is kept, and nothing is a toast.
            Tell::Refusals => {
                if report.iter().any(|d| d.severity > Severity::Info) {
                    self.plugins.set_report(&manifest.id, report);
                }
            }
        }
    }

    fn apply_plugin_file(
        &mut self,
        doc_id: DocumentId,
        plugin: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        let kind = (self.document(doc_id).map(|d| d.kind)).ok_or("the open document was closed")?;
        let Some(written) =
            crate::state::document_io::resource_image_for(kind, &bytes).and_then(Arc::into_inner)
        else {
            return Err("the written file would not open as the document's image".into());
        };
        if let Some(doc) = self.document_mut(doc_id) {
            doc.patch_resource_image(&format!("{plugin} changed the document"), |image| {
                *image = written;
                true
            });
        }
        self.catch_up(doc_id);
        Ok(())
    }

    /// Keep a report where Settings shows it, and tell the user of a refusal or of a change.
    fn report_quietly(&mut self, manifest: &Manifest, report: Vec<Diagnostic>) {
        let name = &manifest.name;
        if let Some(refusal) = report.iter().find(|d| d.severity > Severity::Info) {
            self.show_warning_toast(format!("{name}: {}", refusal.message));
        } else if let Some(changed) =
            (report.iter()).find(|d| d.key == "plugin.changed" || d.key == "plugin.wrote")
        {
            self.show_info_toast(format!("{name}: {}", changed.message));
        }
        self.plugins.set_report(&manifest.id, report);
    }
}

/// What a call's report is worth to the user.
#[derive(Clone, Copy)]
enum Tell {
    /// A refusal or a change is a toast, and the report is kept.
    Everything,
    /// Only a refusal is kept, and silently.
    Refusals,
}
