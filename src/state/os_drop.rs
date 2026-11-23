//! Files dragged between Explorer and an archive: what a drop means, the
//! import it becomes, and the files a drag out of the window hands over.

use std::path::{Path, PathBuf};

use super::document::{DocumentId, RpfNodeId};
use super::workbench::Question;
use super::{AppState, DocPayload, GateOutcome, PendingRetailEdit, PendingRetailEditPrompt};

/// What was under the pointer when files were dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// A folder row, or the folder a file row sits in.
    Folder {
        doc: DocumentId,
        folder: RpfNodeId,
    },
    /// The Wardrobe's panel, while the Wardrobe workspace is on screen.
    Wardrobe,
    /// The Weapons screen's list or assembled weapon, while that workspace is on screen.
    Weapons,
    /// The Map screen's list, plan or scene, while that workspace is on screen.
    Maps,
    /// The Effects panel of the Particle screen, while that workspace is on screen with a project that holds particles.
    Particles,
    /// The Project workspace's panels, while a project is open: a drop here is put in the project.
    Project,
    /// A tile of the dressed ped's texture variants, which `dressed.drop_tile` names.
    Variant,
    /// A texture tile of the gallery, which the gallery remembers: an image dropped on it replaces that texture.
    Texture,
    Nowhere,
}

/// An image dropped on a texture tile of the gallery, which the gallery replaces that texture from in its next frame.
#[derive(Default)]
pub struct DroppedOnATile(pub Option<PathBuf>);

impl crate::ui::panel_state::PanelState for DroppedOnATile {
    const KEY: &'static str = "dropped-on-a-tile";
}

/// Whether `path` is an image the texture importer reads, by its name alone.
pub fn is_an_image(path: &Path) -> bool {
    let known = or_io::authoring::texture_import::SourceFormat::EXTENSIONS;
    (path.extension().and_then(|e| e.to_str()))
        .is_some_and(|e| known.iter().any(|k| k.eq_ignore_ascii_case(e)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// A `.openrage-proj`, which opens as a project wherever it lands.
    Project(PathBuf),
    Open(PathBuf),
    Import {
        doc: DocumentId,
        folder: RpfNodeId,
        paths: Vec<PathBuf>,
    },
    /// Neither openable nor aimed at an archive, and no archive is open: it opens as its bytes.
    Ignored(PathBuf),
    /// A built pack (an `.oiv`, a grzyClothTool project, or a `.rpf` dropped on the Project workspace), which asks whether to reopen it as a project.
    ReopenBuilt(PathBuf),
    /// Files and folders dropped on the Wardrobe: the clothes they hold are added to the pack.
    AddClothes(Vec<PathBuf>),
    /// Files and folders dropped on the Weapons screen: the weapon files, models and textures they hold are added to the pack.
    AddWeapons(Vec<PathBuf>),
    /// Files and folders dropped on the Map screen: the map files, models and textures they hold are added to the pack.
    AddMaps(Vec<PathBuf>),
    /// Files and folders dropped on the Particle screen: the `.ypt` files they hold are added to the pack.
    AddParticles(Vec<PathBuf>),
    /// Files and folders dropped on the Project workspace: game files that become sources of the open project.
    ImportIntoProject(Vec<PathBuf>),
    /// An image dropped on a variation's tile: its texture is made from it.
    ReplaceTexture(PathBuf),
    /// An image dropped on a texture tile of the gallery: that texture's pixels are replaced from it.
    ReplaceTile(PathBuf),
}

/// Whether a drop of `path` on `target` asks to be reopened as a project.
///
/// ⚠ **The name alone says so and no file is read**: the hint under a held drag asks this every frame, where [`plan`] looks inside folders.
pub fn drop_asks_to_reopen(path: &Path, target: DropTarget) -> bool {
    // Aimed at an archive's folder everything imports, and an image dropped on a tile is that tile's.
    !matches!(target, DropTarget::Folder { .. } | DropTarget::Variant)
        && super::workbench::asks_to_reopen(path, target == DropTarget::Project)
}

/// What a drop does.
///
/// ⚠ **Aimed at a folder, everything imports**, a `.ydr` included: dropping a
/// drawable onto an archive's folder is putting it there, and opening it
/// instead is the surprise OpenIV users would report. Dropped anywhere else a
/// file this editor opens is opened, and one it does not goes into the active
/// archive's selected folder.
///
/// ⚠ **Aimed at the Project workspace, everything imports** except a folder that
/// holds a project, which is that project's to open and never content of this one.
///
/// ⚠ **A built pack asks to be reopened** (`.oiv` and grzyClothTool projects wherever they land but an archive's folder, a
/// `dlc.rpf` only on the Project workspace, where an archive cannot be put in the project): elsewhere a `.rpf` still opens,
/// and so does any other `.rpf`.
pub fn plan(
    paths: &[PathBuf],
    target: DropTarget,
    fallback: Option<(DocumentId, RpfNodeId)>,
    openable: impl Fn(&Path) -> bool,
) -> Vec<DropAction> {
    // ⚠ A project file is a project wherever it lands: it is never put into an archive, and never opened as bytes.
    let (projects, paths): (Vec<&PathBuf>, Vec<&PathBuf>) = paths
        .iter()
        .partition(|path| super::front_door::is_project_file(path));
    let mut out: Vec<DropAction> = projects
        .into_iter()
        .map(|path| DropAction::Project(path.clone()))
        .collect();
    let (built, paths): (Vec<&PathBuf>, Vec<&PathBuf>) = paths
        .into_iter()
        .partition(|path| drop_asks_to_reopen(path, target));
    out.extend(
        built
            .into_iter()
            .map(|p| DropAction::ReopenBuilt(p.clone())),
    );
    if target == DropTarget::Variant {
        // ⚠ One texture is made from one image: the first of a drop that holds several.
        if let Some(first) = paths.first() {
            out.push(DropAction::ReplaceTexture((*first).clone()));
        }
        return out;
    }
    // ⚠ A texture tile takes an image and nothing else: any other file dropped on one opens as it would anywhere.
    let target = match (target, paths.first()) {
        (DropTarget::Texture, Some(first)) if is_an_image(first) => {
            out.push(DropAction::ReplaceTile((*first).clone()));
            return out;
        }
        (DropTarget::Texture, _) => DropTarget::Nowhere,
        _ => target,
    };
    if target == DropTarget::Wardrobe {
        if !paths.is_empty() {
            out.push(DropAction::AddClothes(paths.into_iter().cloned().collect()));
        }
        return out;
    }
    if target == DropTarget::Weapons {
        if !paths.is_empty() {
            out.push(DropAction::AddWeapons(paths.into_iter().cloned().collect()));
        }
        return out;
    }
    if target == DropTarget::Maps {
        if !paths.is_empty() {
            out.push(DropAction::AddMaps(paths.into_iter().cloned().collect()));
        }
        return out;
    }
    if target == DropTarget::Particles {
        if !paths.is_empty() {
            out.push(DropAction::AddParticles(
                paths.into_iter().cloned().collect(),
            ));
        }
        return out;
    }
    if let DropTarget::Folder { doc, folder } = target {
        if !paths.is_empty() {
            out.push(DropAction::Import {
                doc,
                folder,
                paths: paths.into_iter().cloned().collect(),
            });
        }
        return out;
    }
    let mut into_fallback = Vec::new();
    let mut into_project = Vec::new();
    for path in paths {
        if target == DropTarget::Project {
            if super::front_door::new_project::project_file_in(path).is_some() {
                out.push(DropAction::Open(path.clone()));
            } else {
                into_project.push(path.clone());
            }
        } else if path.is_dir() || openable(path) {
            out.push(DropAction::Open(path.clone()));
        } else if fallback.is_some() {
            into_fallback.push(path.clone());
        } else {
            out.push(DropAction::Ignored(path.clone()));
        }
    }
    if let (Some((doc, folder)), false) = (fallback, into_fallback.is_empty()) {
        out.push(DropAction::Import {
            doc,
            folder,
            paths: into_fallback,
        });
    }
    if !into_project.is_empty() {
        out.push(DropAction::ImportIntoProject(into_project));
    }
    out
}

/// The most a drag out of the window stages, because staging is a synchronous
/// extraction and the mouse button is still held while it runs.
pub const DRAG_OUT_LIMIT: u64 = 512 * 1024 * 1024;

impl AppState {
    /// The folder a drop on this row lands in.
    pub fn drop_folder_for(&self, doc: DocumentId, node: RpfNodeId) -> Option<RpfNodeId> {
        let DocPayload::Rpf(rpf) = &self.document(doc)?.payload else {
            return None;
        };
        let n = rpf.session.node(node)?;
        if n.is_dir() {
            Some(node)
        } else {
            n.parent
        }
    }

    fn fallback_drop_folder(&self) -> Option<(DocumentId, RpfNodeId)> {
        let doc = self.active_document?;
        let DocPayload::Rpf(rpf) = &self.document(doc)?.payload else {
            return None;
        };
        Some((
            doc,
            rpf.session
                .preferred_drop_folder(self.selection.selected(doc)),
        ))
    }

    /// Carry out a drop from outside the window.
    pub fn handle_os_drop(&mut self, paths: Vec<PathBuf>, target: DropTarget) {
        let actions = plan(&paths, target, self.fallback_drop_folder(), |p| {
            super::DocumentKind::from_path(p).is_some()
        });
        for action in actions {
            match action {
                DropAction::Project(path) => self.open_project_asking(path),
                // ⚠ A folder is opened as one, which offers to make it a project; as bytes it is a load error.
                DropAction::Open(path) if path.is_dir() => self.open_dropped_folder(path),
                DropAction::Open(path) | DropAction::Ignored(path) => self.load_file(path),
                DropAction::ReopenBuilt(path) => self.ask_in_turn(path),
                DropAction::AddClothes(paths) => self.add_clothes(paths),
                DropAction::AddWeapons(paths) => self.add_weapons(paths),
                DropAction::AddMaps(paths) => self.add_maps(paths),
                DropAction::AddParticles(paths) => self.add_particles(paths),
                DropAction::ImportIntoProject(paths) => self.drop_into_project(paths),
                DropAction::ReplaceTexture(image) => self.replace_dropped_image(image),
                DropAction::ReplaceTile(image) => {
                    self.panels.get::<DroppedOnATile>().0 = Some(image);
                }
                DropAction::Import { doc, folder, paths } => {
                    self.import_paths_into_folder(doc, folder, paths)
                }
            }
        }
    }

    /// Open a folder that was dropped: a folder that holds another project, while a project is open, asks whether that project opens here or in a new window.
    ///
    /// ⚠ The folder of the project that is open is already open, so it asks nothing; any other folder is opened as one, which offers to make it a project.
    fn open_dropped_folder(&mut self, folder: PathBuf) {
        let held = super::front_door::new_project::project_file_in(&folder)
            .filter(|_| self.project_path.is_some());
        match held {
            Some(file) if self.project_path.as_ref() == Some(&file) => {
                self.show_info_toast(tr!("{folder} is already open", folder = folder.display()));
            }
            Some(file) => self.open_project_asking(file),
            None => self.open_folder(folder),
        }
    }

    /// Put what was dropped on the Project workspace into the project: game files become sources, and a folder that holds content is offered as that content.
    ///
    /// ⚠ A model or a texture dictionary may be clothes, so a project with no clothing asks before it makes them sources, for a file named in the drop and for a folder that holds some. A folder that holds a clothing pack or addon peds asks whether to link it as that content or to import it as sources, since no project type was chosen to say which.
    pub fn drop_into_project(&mut self, paths: Vec<PathBuf>) {
        let refused = self.import_paths_refused(&paths).is_some();
        if !refused {
            if let Some((folder, kind)) = self.content_a_dropped_folder_holds(&paths) {
                self.workbench.asking = Some(Question::DropContent {
                    folder,
                    kind,
                    paths,
                });
                return;
            }
        }
        let may_be_clothes = paths.iter().any(|path| {
            if path.is_dir() {
                return !refused && self.folder_holds_clothes(path);
            }
            (path.extension().and_then(|e| e.to_str()))
                .is_some_and(|e| e.eq_ignore_ascii_case("ydd") || e.eq_ignore_ascii_case("ytd"))
        });
        if may_be_clothes && !refused && !self.has_clothing() {
            self.workbench.asking = Some(Question::DropClothes { paths });
        } else {
            self.import_game_files(paths);
        }
    }

    /// The first dropped folder that, as it stands, holds a kind of content OpenRage knows, and what the kind is called.
    ///
    /// ⚠ The project's own folder and a folder around or inside it are no content to link, so they are imported as they were.
    fn content_a_dropped_folder_holds(&self, paths: &[PathBuf]) -> Option<(PathBuf, String)> {
        let project = or_project::project_folder(self.project_path.as_ref()?);
        paths.iter().filter(|p| p.is_dir()).find_map(|folder| {
            if folder.starts_with(&project) || project.starts_with(folder) {
                return None;
            }
            or_content::recognisers()
                .iter()
                .find(|recogniser| recogniser.holds(folder))
                .map(|recogniser| (folder.clone(), recogniser.kind().label()))
        })
    }

    /// Whether the folder holds models or textures a clothing pack would take, as an adoption would read them.
    fn folder_holds_clothes(&self, folder: &Path) -> bool {
        or_project::adopt::analyse(folder, &or_content::recognisers())
            .found
            .iter()
            .any(|found| found.module.kind == or_project::ModuleKind::Clothing)
    }

    /// Answer the question a drop of a folder of content asked: link it as that content, or import everything dropped as sources.
    pub fn answer_dropped_content(&mut self, link: bool) {
        let (folder, paths) = match self.workbench.asking.take() {
            Some(Question::DropContent { folder, paths, .. }) => (folder, paths),
            other => {
                self.workbench.asking = other;
                return;
            }
        };
        if !link {
            return self.import_game_files(paths);
        }
        match self.link_content(&folder) {
            Ok(id) => self.show_toast(tr!("Linked {id}; Save project keeps the link", id = id)),
            Err(why) => return self.show_error_toast(why),
        }
        let rest: Vec<PathBuf> = paths.into_iter().filter(|p| *p != folder).collect();
        if !rest.is_empty() {
            self.import_game_files(rest);
        }
    }

    /// Close the question a drop of a folder of content asked, doing neither.
    pub fn dismiss_dropped_content(&mut self) {
        if matches!(self.workbench.asking, Some(Question::DropContent { .. })) {
            self.workbench.asking = None;
        }
    }

    /// Answer the question a drop of clothes asked: make clothing for them, or import them like any game file.
    pub fn answer_dropped_clothes(&mut self, make_clothing: bool) {
        let paths = match self.workbench.asking.take() {
            Some(Question::DropClothes { paths }) => paths,
            other => {
                self.workbench.asking = other;
                return;
            }
        };
        if !make_clothing {
            return self.import_game_files(paths);
        }
        self.add_clothing_content();
        if self.has_clothing() {
            self.add_clothes(paths);
        }
    }

    /// Close the question a drop of clothes asked, doing neither.
    pub fn dismiss_dropped_clothes(&mut self) {
        if matches!(self.workbench.asking, Some(Question::DropClothes { .. })) {
            self.workbench.asking = None;
        }
    }

    /// Import files and folders from disk into an archive folder, asking the
    /// edit gate once for the whole drop.
    pub fn import_paths_into_folder(
        &mut self,
        doc: DocumentId,
        folder: RpfNodeId,
        paths: Vec<PathBuf>,
    ) {
        self.bring_paths_into_folder(doc, folder, paths, None);
    }

    /// Whether the write gate would refuse a drop into `folder` right now, as it does in Read only over the game's own folders.
    pub fn a_drop_into_is_refused(&self, doc: DocumentId, folder: RpfNodeId) -> bool {
        let target = self.affected_disk_path(doc, folder);
        matches!(
            self.retail_guard_for(target.as_deref()),
            super::pending_prompts::RetailGuard::Refused(_)
        )
    }

    /// Files staged from rows of an archive in `staged`, brought into `folder` as a copy: the same walk as a drop, which says Copied, and the staged files go once the gate is answered.
    pub(super) fn copy_paths_into_folder(
        &mut self,
        doc: DocumentId,
        folder: RpfNodeId,
        paths: Vec<PathBuf>,
        staged: std::sync::Arc<super::pending_prompts::Staged>,
    ) {
        self.bring_paths_into_folder(doc, folder, paths, Some(staged));
    }

    fn bring_paths_into_folder(
        &mut self,
        doc: DocumentId,
        folder: RpfNodeId,
        paths: Vec<PathBuf>,
        staged: Option<std::sync::Arc<super::pending_prompts::Staged>>,
    ) {
        if let Some(why) = self.importing_into_itself(doc, folder, &paths) {
            self.show_error_toast(why);
            return;
        }
        let target = self.affected_disk_path(doc, folder);
        match self.gate_permits(target.as_deref()) {
            GateOutcome::Proceed => {
                self.import_paths_into_folder_inner(doc, folder, paths, staged.is_some());
            }
            GateOutcome::Refused => {}
            GateOutcome::Confirm(path) => {
                self.pending_retail_edit = Some(PendingRetailEditPrompt {
                    action: PendingRetailEdit::ImportPaths {
                        doc_id: doc,
                        folder_id: folder,
                        paths,
                        staged,
                    },
                    target_path: path,
                    verb: tr!("copy into"),
                });
            }
        }
    }

    pub(super) fn import_paths_into_folder_inner(
        &mut self,
        doc: DocumentId,
        folder: RpfNodeId,
        paths: Vec<PathBuf>,
        copied: bool,
    ) {
        // Opened first, so a dropped folder merges with one the archive already holds.
        if let Err(e) = self.open_archive_row(doc, folder) {
            self.show_error_toast(e);
            return;
        }
        let (mut files, mut failures) = (0usize, Vec::new());
        let batch = |state: &mut Self, label: Option<String>| {
            let Some(history) = state.document_mut(doc).map(|d| &mut d.history) else {
                return;
            };
            match label {
                Some(label) => history.end_batch(label),
                None => history.begin_batch(),
            }
        };
        batch(self, None);
        for path in paths {
            self.import_one(doc, folder, &path, &mut files, &mut failures);
        }
        let (step, done) = if copied {
            (tr!("Copy"), tr!("Copied"))
        } else {
            (tr!("Add"), tr!("Added"))
        };
        batch(self, Some(format!("{step} {files} file(s)")));
        match (files, failures.len()) {
            (0, 0) => {}
            (n, 0) => self.show_toast(format!("{done} {n} file(s)")),
            (n, f) => self.show_error_toast(tr!(
                "{done} {n} file(s); {f} failed: {failures}",
                done = done,
                n = n,
                f = f,
                failures = failures.join("; ")
            )),
        }
    }

    /// Why `paths` cannot go into `folder`: a folder imported into itself or one inside it would copy itself without end.
    pub(super) fn importing_into_itself(
        &self,
        doc: DocumentId,
        folder: RpfNodeId,
        paths: &[PathBuf],
    ) -> Option<String> {
        let into = self
            .disk_path_for_node(doc, folder)
            .filter(|_| self.is_disk_folder(doc, folder))?;
        // ⚠ As the disk names them: a path spelt in another case or through a link is the same folder.
        let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let into = real(&into);
        let inside = paths
            .iter()
            .find(|p| p.is_dir() && into.starts_with(real(p)))?;
        Some(tr!(
            "`{inside}` cannot go into itself or a folder inside it",
            inside = inside.display()
        ))
    }

    pub(super) fn import_one(
        &mut self,
        doc: DocumentId,
        folder: RpfNodeId,
        path: &Path,
        files: &mut usize,
        failures: &mut Vec<String>,
    ) {
        if !path.is_dir() {
            match self.rpf_import_file_to_folder_inner(doc, folder, path.to_path_buf()) {
                Ok(_) => *files += 1,
                Err(e) => failures.push(e),
            }
            return;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("imported")
            .to_string();
        let existing = match self.document(doc).map(|d| &d.payload) {
            Some(DocPayload::Rpf(rpf)) => rpf.session.children_of(folder).into_iter().find(|&id| {
                rpf.session
                    .node(id)
                    .is_some_and(|n| n.is_dir() && n.name.eq_ignore_ascii_case(&name))
            }),
            _ => None,
        };
        let sub = match existing {
            Some(id) => id,
            None => match self.rpf_add_folder_inner(doc, folder, &name) {
                Ok(id) => id,
                Err(e) => {
                    failures.push(e);
                    return;
                }
            },
        };
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(path) {
            Ok(dir) => dir.flatten().map(|e| e.path()).collect(),
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                return;
            }
        };
        entries.sort();
        for entry in entries {
            self.import_one(doc, sub, &entry, files, failures);
        }
    }

    /// Extract rows into `staging` so they can be handed to Explorer, and
    /// return the paths of the top-level items it wrote.
    ///
    /// ⚠ **Each row lands under its own name**, a folder included: a folder
    /// dragged out arrives as that folder, not as its contents spilled into
    /// wherever it was dropped.
    ///
    /// ⚠ **Writes nothing while the export-fixes question is open**, and
    /// returns no paths: the drag cannot wait for the answer, so the next drag
    /// out takes it.
    ///
    /// ⚠ **Waits for nothing**: rows under the selection the tree has not opened
    /// are asked for and the drag is refused, naming how many are left; they go
    /// on opening over the frames that follow, which say when to drag again.
    pub fn stage_drag_out(
        &mut self,
        doc: DocumentId,
        nodes: &[RpfNodeId],
        staging: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        use super::extract_job::{
            plan_named_rows, run_extraction, total_size, OnClash, PendingExportFixes, TypeFilter,
        };
        self.open_rows_for_drag(doc, nodes)?;
        let DocPayload::Rpf(rpf) = &self
            .document(doc)
            .ok_or("the archive is no longer open")?
            .payload
        else {
            return Err(tr!("not an archive").into());
        };
        let (items, _) = plan_named_rows(&rpf.session, nodes, &TypeFilter::All);
        if items.is_empty() {
            return Err(tr!("nothing to drag: the selection holds no files").into());
        }
        let size = total_size(&rpf.session, nodes);
        if size > DRAG_OUT_LIMIT {
            return Err(tr!(
                "{size} is too much to drag; use Extract… instead",
                size = or_job::human_bytes(size)
            ));
        }
        let form = or_io::container::rpf::BodyForm::Decoded;
        let Some(fixes) = self
            .drag_out_fixes
            .take()
            .or_else(|| self.export_fixes_for(&items, form))
        else {
            self.pending_export_fixes = Some(PendingExportFixes::DragOut);
            return Ok(Vec::new());
        };
        let mut tops: Vec<PathBuf> = Vec::new();
        for item in &items {
            let top = staging.join(item.top_component());
            if !tops.contains(&top) {
                tops.push(top);
            }
        }
        let report = run_extraction(
            items,
            staging.to_path_buf(),
            form,
            fixes,
            // A drag stages into a directory of its own, so nothing is there to clash.
            OnClash::Replace,
            or_job::Cancel::new(),
            or_job::Progress::new(),
        );
        if report.counts.failed > 0 {
            return Err(tr!(
                "{failed} file(s) could not be extracted: {failures}",
                failed = report.counts.failed,
                failures = report.failures.join("; ")
            ));
        }
        Ok(tops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DocumentKind;

    fn ydr(p: &Path) -> bool {
        p.extension().is_some_and(|e| e == "ydr")
    }

    #[test]
    fn a_drop_on_a_folder_imports_everything_and_elsewhere_opens_what_it_can() {
        let paths = vec![PathBuf::from("C:/x/a.ydr"), PathBuf::from("C:/x/notes.bin")];
        assert_eq!(
            plan(&paths, DropTarget::Folder { doc: 1, folder: 7 }, None, ydr),
            vec![DropAction::Import {
                doc: 1,
                folder: 7,
                paths: paths.clone()
            }]
        );
        assert_eq!(
            plan(&paths, DropTarget::Nowhere, Some((2, 3)), ydr),
            vec![
                DropAction::Open(paths[0].clone()),
                DropAction::Import {
                    doc: 2,
                    folder: 3,
                    paths: vec![paths[1].clone()]
                }
            ]
        );
        assert_eq!(
            plan(&paths, DropTarget::Nowhere, None, ydr),
            vec![
                DropAction::Open(paths[0].clone()),
                DropAction::Ignored(paths[1].clone())
            ]
        );
    }

    fn archive() -> (AppState, DocumentId, RpfNodeId) {
        let mut state = AppState::default();
        let doc = state.new_empty_document(DocumentKind::Rpf);
        let DocPayload::Rpf(rpf) = &mut state.document_mut(doc).unwrap().payload else {
            panic!("an empty RPF document holds an RPF payload");
        };
        let root = rpf.session.root_id();
        let props = rpf.session.add_folder(root, "props").unwrap();
        rpf.session
            .add_file_copy(props, "chair.ydr", vec![7; 12])
            .unwrap();
        rpf.session
            .add_file_copy(root, "a.ytd", vec![1; 3])
            .unwrap();
        (state, doc, props)
    }

    fn names(state: &AppState, doc: DocumentId, folder: RpfNodeId) -> Vec<String> {
        let DocPayload::Rpf(rpf) = &state.document(doc).unwrap().payload else {
            unreachable!()
        };
        let mut out: Vec<String> = rpf
            .session
            .children_of(folder)
            .into_iter()
            .map(|id| rpf.session.node(id).unwrap().name.clone())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn a_dropped_folder_imports_as_a_folder_with_its_files() {
        let (mut state, doc, props) = archive();
        let disk = tempfile::tempdir().unwrap();
        let pack = disk.path().join("lamps");
        std::fs::create_dir_all(pack.join("lod")).unwrap();
        std::fs::write(pack.join("lamp.ydr"), b"one").unwrap();
        std::fs::write(pack.join("lod").join("lamp_lod.ydr"), b"two").unwrap();
        let loose = disk.path().join("sign.ytd");
        std::fs::write(&loose, b"three").unwrap();

        state.handle_os_drop(vec![pack, loose], DropTarget::Folder { doc, folder: props });
        assert_eq!(
            names(&state, doc, props),
            ["chair.ydr", "lamps", "sign.ytd"]
        );
        let DocPayload::Rpf(rpf) = &state.document(doc).unwrap().payload else {
            unreachable!()
        };
        let lamps = rpf
            .session
            .children_of(props)
            .into_iter()
            .find(|&id| rpf.session.node(id).unwrap().name == "lamps")
            .unwrap();
        let lod = rpf.session.children_of(lamps);
        assert_eq!(names(&state, doc, lamps), ["lamp.ydr", "lod"]);
        assert!(lod.len() == 2);
    }

    #[test]
    fn staging_a_drag_writes_each_row_under_its_own_name() {
        let (mut state, doc, props) = archive();
        let DocPayload::Rpf(rpf) = &state.document(doc).unwrap().payload else {
            unreachable!()
        };
        let root = rpf.session.root_id();
        let ytd = rpf
            .session
            .children_of(root)
            .into_iter()
            .find(|&id| rpf.session.node(id).unwrap().name == "a.ytd")
            .unwrap();
        let staging = tempfile::tempdir().unwrap();
        let tops = state.stage_drag_out(doc, &[props], staging.path()).unwrap();
        assert_eq!(tops, vec![staging.path().join("props")]);
        assert_eq!(
            std::fs::read(staging.path().join("props").join("chair.ydr")).unwrap(),
            vec![7; 12]
        );

        let two = tempfile::tempdir().unwrap();
        let tops = state
            .stage_drag_out(doc, &[ytd, props], two.path())
            .unwrap();
        assert_eq!(
            tops,
            vec![two.path().join("a.ytd"), two.path().join("props")]
        );
    }
}
