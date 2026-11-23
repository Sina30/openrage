//! The Particle workspace's state while a project holds particles: the particle contents as the last scan read them, the effect to show,
//! and the commands that give a project particles, add `.ypt` files to them and make an empty list.
//!
//! ⚠ **Thin by design.** Every rule, every read and every write of a particle file is `or-particles`'s and `or-source`'s; the
//! effects themselves are edited in the Particle workspace, in the document of the source, which saves back into the source.

use std::path::{Path, PathBuf};

use or_particles::{Asset, Depth, Facts, Finding, Particles};
use or_project::{Container, Module, ModuleKind};
use or_source::game_import::{Created, Report, Verdict, Watch};

use super::pack_import::{each_file, made, refused, PackJob};
use super::Workbench;
use crate::state::AppState;
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::ypt_effect_id;

/// What the scan read of one particles content.
#[derive(Debug, Clone)]
pub struct ParticlesRead {
    /// The id of the content.
    pub module: String,
    pub folder: PathBuf,
    /// The folder is another project's, which alone writes it.
    pub linked: bool,
    pub read: Result<ParticlesPack, String>,
}

/// A particles content read: its model, what the disk said of its sources and what is wrong with it.
#[derive(Debug, Clone)]
pub struct ParticlesPack {
    pub model: Particles,
    pub facts: Facts,
    pub findings: Vec<Finding>,
}

impl ParticlesPack {
    /// The effects of every file of the pack, each with the folder of its source and its place in the list its file holds.
    pub fn effects(&self) -> Vec<EffectRow> {
        let mut rows = Vec::new();
        for asset in &self.model.assets {
            let Some(source) = self.facts.source(&asset.source) else {
                continue;
            };
            let Some(Ok(inside)) = &source.inside else {
                continue;
            };
            for (place, effect) in inside.effects.iter().enumerate() {
                rows.push(EffectRow {
                    source: asset.source.clone(),
                    file: source.file_name(),
                    effect: effect.clone(),
                    place,
                });
            }
        }
        rows
    }
}

/// One effect of the pack: where it is and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRow {
    /// The source folder of its file, as the model names it.
    pub source: String,
    /// The file it cooks to, as `fx_smoke.ypt`.
    pub file: String,
    pub effect: String,
    /// Its place among the effects of the list, which is the index the Particle workspace selects it by.
    pub place: usize,
}

/// Every particles content of `container`, read in the project in `project_folder`.
pub fn read_all(container: &Container, project_folder: &Path) -> Vec<ParticlesRead> {
    (container.modules.iter())
        .filter(|m| m.kind == ModuleKind::Particles)
        .map(|module| ParticlesRead {
            module: module.id.clone(),
            folder: or_particles::module_folder(project_folder, module).unwrap_or_default(),
            linked: module.is_link(),
            read: or_particles::planned(project_folder, module, Depth::Open)
                .map(|(model, facts, planned)| ParticlesPack {
                    model,
                    facts,
                    findings: planned.findings,
                })
                .map_err(|e| e.to_string()),
        })
        .collect()
}

/// The effect the Effects panel was asked to show: the file its source cooks to, its place in the list, and whether the file was asked for.
#[derive(Debug, Clone)]
pub struct Want {
    pub path: PathBuf,
    pub place: usize,
    pub asked: bool,
}

/// The Particle workspace's own state while a project holds particles, reset with the project.
#[derive(Default)]
pub struct ParticlesWork {
    /// The particles content the Effects panel shows; the first when none is chosen.
    pub module: Option<String>,
    /// The text the list is filtered by.
    pub filter: String,
    /// An effect to show once the document of its file is open.
    pub want: Option<Want>,
}

impl Workbench {
    /// The particles contents as the last scan read them.
    pub fn particles_read(&self) -> &[ParticlesRead] {
        self.scan.as_ref().map_or(&[], |scan| &scan.particles)
    }
}

/// What a particles content holds, in words, as the last scan read it.
pub fn held_by(workbench: &Workbench, module: &Module) -> String {
    match workbench
        .particles_read()
        .iter()
        .find(|p| p.module == module.id)
    {
        None => tr!("Reading…").to_string(),
        Some(pack) => match &pack.read {
            Ok(read) => super::held(&module.kind, read.model.assets.len()),
            Err(_) => tr!("could not be read").to_string(),
        },
    }
}

/// How many particle files the model of a particles content names, none when its file cannot be read.
pub fn on_disk(project: &Path, module: &Module) -> Option<usize> {
    let folder = or_particles::module_folder(&or_project::project_folder(project), module).ok()?;
    Particles::read(&folder)
        .ok()
        .map(|model| model.assets.len())
}

/// The files under `paths`: a folder is walked for the files in it, a bounded way down.
fn files_under(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    const DEPTH: usize = 8;
    const FILES: usize = 2000;
    fn walk(path: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if out.len() >= FILES {
            return;
        }
        if path.is_dir() {
            if depth == 0 {
                return;
            }
            let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            entries.sort();
            for entry in entries {
                walk(&entry, depth - 1, out);
            }
        } else {
            out.push(path.to_path_buf());
        }
    }
    let mut out = Vec::new();
    for path in paths {
        walk(&path, DEPTH, &mut out);
    }
    out
}

impl AppState {
    /// Whether the project holds particles content.
    pub fn has_particles(&self) -> bool {
        (self.project_carried.container.modules.iter()).any(|m| m.kind == ModuleKind::Particles)
    }

    /// The particles content the Effects panel shows: the one chosen, else the first.
    pub fn particles_pack(&self) -> Option<&ParticlesRead> {
        let packs = self.workbench.particles_read();
        let chosen = self.workbench.particles.module.as_deref();
        chosen
            .and_then(|id| packs.iter().find(|p| p.module == id))
            .or_else(|| packs.first())
    }

    /// The id of the effect the active document has selected, when that document is the file the particle source `source` of the shown pack cooks to.
    pub fn effect_shown_from(&self, source: &str) -> Option<u64> {
        let pack = self.particles_pack()?;
        let key = pack
            .folder
            .join(source.replace('/', std::path::MAIN_SEPARATOR_STR))
            .display()
            .to_string();
        let output = self.workbench.row(&key)?.output.clone()?;
        let doc = self.active_document?;
        (self.document(doc)?.path.as_deref() == Some(output.as_path()))
            .then(|| self.selection.selected(doc))
            .flatten()
    }

    /// Make the project hold particles: a `particles` folder with an empty model, a content for it, and a place for it in every build.
    pub fn add_particle_content(&mut self) {
        if self.refused_as_read_only() {
            return;
        }
        let Some(project) = self.project_path.clone() else {
            return;
        };
        let taken: Vec<&str> = (self.project_carried.container.modules.iter())
            .map(|m| m.id.as_str())
            .collect();
        let id = super::commands::unique("particles", taken.into_iter());
        let module = Module::particles(&id, &id);
        let folder = or_project::project_folder(&project).join(&id);
        let refused = or_project::links::refuse_write(
            &self.project_carried.container,
            &project,
            &folder.join(or_particles::FILE),
        );
        if let Err(why) = refused {
            self.show_error_toast(why);
            return;
        }
        if let Err(why) =
            or_particles::adopt::start_module(&or_project::project_folder(&project), &module)
        {
            self.show_error_toast(why);
            return;
        }
        let container = &mut self.project_carried.container;
        container.modules.push(module);
        for output in &mut container.outputs {
            output.take.push(id.clone());
        }
        self.workbench.want_scan();
        self.workbench.particles.module = Some(id.clone());
        self.record_step(tr!("Added the particles content {id}", id = id));
        self.active_workspace = Workspace::Particle;
        self.show_toast(tr!(
            "Particles added: drop .ypt files on the Particle screen to fill it, or make a new list"
        ));
    }

    /// The folder of the particles content to write, none having said why when the window only reads the project or the content is linked.
    fn particles_folder_to_write(&mut self) -> Option<(String, PathBuf)> {
        if self.refused_as_read_only() {
            return None;
        }
        // ⚠ The content as the project file states it, not as the scan read it: a pack just made has not been scanned yet.
        let chosen = self.workbench.particles.module.as_deref();
        let modules = &self.project_carried.container.modules;
        let module = chosen
            .and_then(|id| {
                modules
                    .iter()
                    .find(|m| m.id == id && m.kind == ModuleKind::Particles)
            })
            .or_else(|| modules.iter().find(|m| m.kind == ModuleKind::Particles));
        let (Some(module), Some(project)) = (module, self.project_path.as_ref()) else {
            self.show_warning_toast(tr!(
                "The project holds no particles: Project ▸ Add a particle pack… adds some"
            ));
            return None;
        };
        if module.is_link() {
            let why = tr!("These particles are linked from another project, which alone edits them: open that project to change them");
            self.show_warning_toast(why);
            return None;
        }
        let folder = or_particles::module_folder(&or_project::project_folder(project), module);
        match folder {
            Ok(folder) => Some((module.id.clone(), folder)),
            Err(why) => {
                self.show_error_toast(why.to_string());
                None
            }
        }
    }

    /// Add the `.ypt` files `paths` (and the files in the folders among them) to the particles: each becomes a source of the pack, made by the import job.
    ///
    /// ⚠ Not in the frame: a source is extracted, compiled, read again and compared, which takes seconds for a large file.
    pub fn add_particles(&mut self, paths: Vec<PathBuf>) {
        let Some((module, folder)) = self.particles_folder_to_write() else {
            return;
        };
        let files: Vec<PathBuf> = files_under(paths)
            .into_iter()
            .filter(|path| {
                (path.extension().and_then(|e| e.to_str()))
                    .is_some_and(|e| e.eq_ignore_ascii_case("ypt"))
            })
            .collect();
        if files.is_empty() {
            self.show_toast(tr!("No .ypt file was found among those dropped"));
            return;
        }
        self.start_pack_import(
            ModuleKind::Particles,
            module,
            folder,
            PackJob::Add { files, into: None },
        );
    }

    /// The import of particle files ended: its report is the import report, its step takes back what it made, and the Particle screen shows the pack.
    pub(super) fn finish_particles_import(
        &mut self,
        module: String,
        new: Option<String>,
        report: Report,
    ) {
        let [made, _, _, refused] = super::imports::counts(&report);
        let first = || {
            (report.files.iter())
                .find(|file| file.verdict == Verdict::Refused)
                .map(|file| format!("{} {}", file.input, file.reason))
                .unwrap_or_default()
        };
        let step = match (made, refused) {
            (0, _) => None,
            (_, 0) if new.is_some() => Some(tr!(
                "Added the particle list {name}",
                name = new.as_deref().unwrap_or_default()
            )),
            (made, 0) => Some(tr!(
                "Added {made} to the particles",
                made = crate::state::front_door::counted(made, "particle file")
            )),
            (made, refused) => Some(tr!(
                "Added {made}; {refused} could not be added: {first}",
                made = crate::state::front_door::counted(made, "particle file"),
                refused = refused,
                first = first()
            )),
        };
        self.workbench.particles.module = Some(module);
        self.active_workspace = Workspace::Particle;
        self.finish_import_as(report, step);
    }

    /// An empty list of a name nobody holds yet, as a source of the pack.
    pub fn new_particles(&mut self) {
        let Some((module, folder)) = self.particles_folder_to_write() else {
            return;
        };
        let held: Vec<String> = self
            .particles_pack()
            .and_then(|p| p.read.as_ref().ok())
            .map(|p| {
                p.model
                    .assets
                    .iter()
                    .map(|a| a.source.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let mut n = 1;
        let name = loop {
            let name = if n == 1 {
                "fx_new".to_string()
            } else {
                format!("fx_new_{n}")
            };
            let source = format!("{name}.particles.src");
            if !held.contains(&source) && !folder.join(&source).exists() {
                break name;
            }
            n += 1;
        };
        self.start_pack_import(ModuleKind::Particles, module, folder, PackJob::New(name));
    }

    /// Show the effect at `place` of the particle file `source` of the content `module` in the Particle workspace, playing it.
    ///
    /// ⚠ A source that is not cooked yet is cooked first, as a saved file in it would be, and its file opens once the cook has written it.
    pub fn show_effect(&mut self, module: &str, source: &str, place: usize) {
        let Some(pack) = self
            .workbench
            .particles_read()
            .iter()
            .find(|p| p.module == module)
        else {
            return;
        };
        let key = pack
            .folder
            .join(source.replace('/', std::path::MAIN_SEPARATOR_STR))
            .display()
            .to_string();
        let Some(output) = self.workbench.row(&key).and_then(|row| row.output.clone()) else {
            return;
        };
        self.workbench.particles.module = Some(module.to_string());
        self.workbench.particles.want = Some(Want {
            path: output.clone(),
            place,
            asked: false,
        });
        self.active_workspace = Workspace::Particle;
        if !output.is_file() {
            self.recook_asset(&key);
        }
        self.apply_wanted_effect();
    }

    /// Open the file of the wanted effect once its source is cooked, and select the effect once the document is open.
    pub fn apply_wanted_effect(&mut self) {
        let Some(want) = self.workbench.particles.want.clone() else {
            return;
        };
        let open = (self.documents.iter())
            .find(|d| d.path.as_deref() == Some(want.path.as_path()))
            .map(|d| d.id);
        let Some(id) = open else {
            if want.path.is_file() && !want.asked {
                if let Some(want) = self.workbench.particles.want.as_mut() {
                    want.asked = true;
                }
                self.load_file(want.path);
            }
            return;
        };
        self.workbench.particles.want = None;
        self.active_document = Some(id);
        self.selection.select(id, ypt_effect_id(want.place));
    }
}
impl AppState {
    /// The folder of the particles source whose compile is the file at `path`, none for any other file.
    pub(in crate::state) fn particles_source_of(&self, path: &Path) -> Option<PathBuf> {
        (self.workbench.rows().iter())
            .find(|row| {
                row.output.as_deref() == Some(path)
                    && row.source_kind.as_deref() == Some(or_source::particles::KIND)
            })
            .map(|row| row.path.clone())
    }
}

/// The work of the import job for particles: each of `files` becomes a source of the pack in `folder`, one at a time, so that the job names the file in hand and a stop keeps what is made.
///
/// ⚠ A file the source form cannot hold, or that cannot be read, is refused in the report with its sentence and the others still come in.
pub(super) fn add_to_pack(folder: &Path, files: &[PathBuf], watch: &Watch<'_>) -> Report {
    each_file(files, watch, |path, name, report| {
        let added = std::fs::read(path)
            .map_err(|e| format!("cannot be read: {e}"))
            .and_then(|bytes| {
                let incoming = or_particles::import::Incoming {
                    name,
                    bytes: &bytes,
                    from_game: false,
                    virtual_path: None,
                };
                or_particles::import::add_files(folder, &[incoming])
            });
        match added {
            Err(why) => refused(report, name, why),
            Ok(added) => {
                for (_, why) in added.refused {
                    refused(report, name, why);
                }
                for dir in added.made {
                    made(report, name, dir);
                }
            }
        }
    })
}

/// The work of the import job for a new list: an empty source called `name` in the pack in `folder`.
pub(super) fn new_in_pack(folder: &Path, name: &str) -> Report {
    let mut report = Report::default();
    match or_particles::import::new_asset(folder, name) {
        Ok(dir) => made(&mut report, name, dir),
        Err(why) => refused(&mut report, name, why),
    }
    report
}

impl AppState {
    /// A source folder that Undo took away is no asset of the pack that named it: the pack goes back to what it held before the files were added.
    pub(super) fn forget_removed_particle_sources(&mut self, created: &[Created]) {
        self.edit_particle_models(|folder, model| {
            let before = model.assets.len();
            model.assets.retain(|asset| {
                let source = source_folder(folder, &asset.source);
                source.exists() || !created.iter().any(|made| made.path == source)
            });
            model.assets.len() != before
        });
    }

    /// A source folder that Redo put back is an asset of the pack again, under an id of its own: the id it had went with it.
    pub(super) fn adopt_restored_particle_sources(&mut self, restored: &[PathBuf]) {
        self.edit_particle_models(|folder, model| {
            let mut changed = false;
            for path in restored.iter().filter(|path| path.is_dir()) {
                let Some(source) = (path.strip_prefix(folder).ok())
                    .and_then(|inside| inside.to_str())
                    .filter(|inside| {
                        inside.ends_with(".particles.src") && !inside.contains(['/', '\\'])
                    })
                else {
                    continue;
                };
                if !model.assets.iter().any(|asset| asset.source == source) {
                    model.assets.push(Asset::new(source));
                    changed = true;
                }
            }
            changed
        });
    }

    /// Let `edit` change the model of each particles content the project holds in its own folder, and write the ones it says it changed.
    fn edit_particle_models(&mut self, edit: impl Fn(&Path, &mut Particles) -> bool) {
        let Some(project) = self.project_path.as_deref() else {
            return;
        };
        let root = or_project::project_folder(project);
        for module in (self.project_carried.container.modules.iter())
            .filter(|m| m.kind == ModuleKind::Particles && !m.is_link())
        {
            let Ok(folder) = or_particles::module_folder(&root, module) else {
                continue;
            };
            let Ok(mut model) = Particles::read(&folder) else {
                continue;
            };
            if edit(&folder, &mut model) {
                if let Err(why) = model.write(&folder) {
                    tracing::warn!(%why, "the particles of a pack were not written");
                }
            }
        }
    }
}

/// The folder of the source `source` of the pack in `folder`, which the model names with `/`.
fn source_folder(folder: &Path, source: &str) -> PathBuf {
    folder.join(source.replace('/', std::path::MAIN_SEPARATOR_STR))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_stop_keeps_the_sources_made_and_lists_the_files_not_reached() {
        let dir = tempfile::tempdir().unwrap();
        let ypt = or_particle::sample::list_of("fx_one", &["fx_spark"]);
        let files: Vec<PathBuf> = ["fx_one", "fx_two"]
            .iter()
            .map(|name| {
                let path = dir.path().join("in").join(format!("{name}.ypt"));
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &ypt).unwrap();
                path
            })
            .collect();
        // Asked before each file: it says stop from the second on.
        let asked = Cell::new(0);
        let stop = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };
        let watch = Watch {
            cancelled: Some(&stop),
            progress: None,
        };

        let report = add_to_pack(&dir.path().join("pack"), &files, &watch);
        assert!(report.cancelled);
        assert_eq!(report.created.len(), 1, "the first file was made");
        assert!(report.created[0].untouched());
        assert_eq!(report.left_out.len(), 1, "the second was not reached");
        assert!(report.left_out[0].path.ends_with("fx_two.ypt"));
    }
}
