//! Links: content whose folder lives somewhere else, by path, with one writer.
//!
//! A link is a module entry with `link: true` and a `path` that may leave the project's folder,
//! which is what a link is for. The project that holds the folder is its only writer: the project
//! that links it reads it into its own builds and writes nothing in it. A link is never followed
//! further, because it names a folder of content and not another project's links.
//!
//! A pack of clothing or addon peds is linked by its folder. A Files content is the files of a
//! project, so it is linked by the folder of the project that holds them, whose project file lists
//! them ([`linked_files`]); linking a whole project ([`add_project_links`]) links each content it
//! holds of its own, and none of the links it holds.

use std::path::{Component, Path, PathBuf};

use crate::container::{is_name, Container, Module, ModuleKind};
use crate::contribution::{Contribution, SourceFolder};
use crate::paths::{
    find_sources, folded, is_within, project_folder, resolve_stored, resolved, same_component,
};
use crate::project::Project;
use crate::PROJECT_EXTENSION;

/// A link as it stands on the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The id of the module.
    pub module: String,
    /// The path as the project file spells it.
    pub stored: String,
    /// Where it leads on this machine, whether or not anything is there.
    pub folder: PathBuf,
    /// The `.openrage-proj` of the project that holds the folder, which is the one that writes it.
    pub owner: Option<PathBuf>,
    /// Why a build of this project would stop on it, in a sentence.
    pub problem: Option<String>,
    /// The ids of the links that project holds itself, which are not followed here.
    pub not_followed: Vec<String>,
    /// How many files a linked Files content holds, when it is one and its project could be read.
    pub files: Option<usize>,
}

impl Link {
    /// What the banner over linked content says.
    pub fn banner(&self) -> String {
        let mut said = match &self.owner {
            Some(owner) => format!(
                "Linked from {}: it is edited there. This project reads it into its own builds and writes nothing in it.",
                leaf(owner)
            ),
            None => format!(
                "Linked from {}, which no project holds: nothing here edits it, and this project only reads it.",
                self.folder.display()
            ),
        };
        if !self.not_followed.is_empty() {
            said.push_str(&format!(
                " That project links {} itself; a link is not followed, so {} not read here: link {} in this project to ship {}.",
                self.not_followed.join(", "),
                if self.not_followed.len() == 1 { "it is" } else { "they are" },
                if self.not_followed.len() == 1 { "it" } else { "them" },
                if self.not_followed.len() == 1 { "it" } else { "them" },
            ));
        }
        said
    }
}

fn leaf(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The folder a link's content is in: its `path` read against the project's folder.
pub fn folder_of(module: &Module, project_folder: &Path) -> Result<PathBuf, String> {
    let stored = module
        .path()
        .filter(|stored| !stored.is_empty())
        .ok_or_else(|| format!("content `{}` is a link and states no `path`", module.id))?;
    Ok(folded(&resolve_stored(stored, project_folder)))
}

/// The text a link stores for `target`: relative to the project's folder with `..` where the two share a root, else the absolute path.
pub fn store_link(target: &Path, project_folder: &Path) -> String {
    let (target, base) = (folded(target), folded(project_folder));
    let ours: Vec<Component<'_>> = base.components().collect();
    let theirs: Vec<Component<'_>> = target.components().collect();
    let common = ours
        .iter()
        .zip(&theirs)
        .take_while(|(a, b)| same_component(**a, **b))
        .count();
    // ⚠ Another drive leaves nothing in common, and no `..` reaches it.
    if common == 0 {
        return target.to_string_lossy().into_owned();
    }
    let up = ours[common..].iter().map(|_| "..".to_string());
    let down = theirs[common..]
        .iter()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    let parts: Vec<String> = up.chain(down).collect();
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// Every link of the container, as the disk shows it.
pub fn links(container: &Container, project: &Path) -> Vec<Link> {
    let folder = project_folder(project);
    container
        .modules
        .iter()
        .filter(|module| module.is_link())
        .map(|module| link_of(module, &folder))
        .collect()
}

fn link_of(module: &Module, folder: &Path) -> Link {
    let stored = module.path().unwrap_or_default().to_string();
    let target = match folder_of(module, folder) {
        Ok(target) => target,
        Err(why) => {
            return Link {
                module: module.id.clone(),
                stored,
                folder: PathBuf::new(),
                owner: None,
                problem: Some(why),
                not_followed: Vec::new(),
                files: None,
            }
        }
    };
    // ⚠ A Files content is the files of the project that sits in the folder; a pack belongs to the nearest project above it.
    let files = module.kind == ModuleKind::Files;
    let owner = if files {
        project_file_in(&target)
    } else {
        owner_of(&target)
    };
    let theirs = owner.as_ref().and_then(|file| Project::load(file).ok());
    let not_followed = (theirs.iter())
        .flat_map(|p| p.container.modules.iter())
        .filter(|m| m.is_link())
        .map(|m| m.id.clone())
        .collect();
    let problem = problem_of(module, &target, folder, theirs.as_ref());
    Link {
        module: module.id.clone(),
        stored,
        folder: target,
        owner,
        problem,
        not_followed,
        files: theirs.filter(|_| files).map(|p| p.members.len()),
    }
}

/// Why a build of the project would stop on this link, if it would.
fn problem_of(
    module: &Module,
    target: &Path,
    folder: &Path,
    theirs: Option<&Project>,
) -> Option<String> {
    let id = &module.id;
    if !target.is_dir() {
        return Some(format!(
            "link `{id}` leads to {}, which is not there",
            target.display()
        ));
    }
    if module.kind == ModuleKind::Files {
        let Some(theirs) = theirs else {
            return Some(format!(
                "link `{id}` leads to {}, which holds no project file that can be read, so it has no Files content to read",
                target.display()
            ));
        };
        if own_files(theirs).is_none() {
            return Some(format!(
                "link `{id}` leads to {}, a project with no Files content",
                target.display()
            ));
        }
    }
    let (there, here) = (resolved(target), resolved(folder));
    if is_within(&there, &here) {
        return Some(format!(
            "link `{id}` leads inside this project's folder, where the content is the project's own and no link"
        ));
    }
    is_within(&here, &there).then(|| {
        format!("link `{id}` leads to a folder around this project, which nothing else may write")
    })
}

/// The Files content of `project` that holds its own files: the first that is not a link.
fn own_files(project: &Project) -> Option<&Module> {
    (project.container.modules.iter()).find(|m| m.kind == ModuleKind::Files && !m.is_link())
}

/// The project file that sits in `folder` itself, the first by name when there are several.
pub fn project_file_in(folder: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(PROJECT_EXTENSION))
                && path.is_file()
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// The project file of the nearest folder at or above `folder` that holds one: the project that writes what is in `folder`.
fn owner_of(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().find_map(project_file_in)
}

/// Refuse a write at `target` that would land in linked content, whose writer is the project that holds it.
pub fn refuse_write(container: &Container, project: &Path, target: &Path) -> Result<(), String> {
    let wanted = resolved(target);
    for link in links(container, project) {
        if link.folder.as_os_str().is_empty() || !is_within(&wanted, &resolved(&link.folder)) {
            continue;
        }
        return Err(match &link.owner {
            Some(owner) => format!(
                "{} is in the linked content `{}`, which is edited in {}: this project only reads it",
                target.display(),
                link.module,
                leaf(owner)
            ),
            None => format!(
                "{} is in the linked content `{}`, which this project only reads",
                target.display(),
                link.module
            ),
        });
    }
    Ok(())
}

/// Why the file at `path` cannot be added to the Files content of the project, none when it can.
///
/// ⚠ A file outside the project's folder has no place in a build, and one inside linked content is
/// already read through the link: either is said, and never added silently.
pub fn refuse_member(container: &Container, project: &Path, path: &Path) -> Option<String> {
    let wanted = resolved(path);
    let folder = project_folder(project);
    // ⚠ Where each link leads and nothing more: the project that holds it is not looked for, since a drop of thousands of files asks this for each.
    let inside_a_link = container
        .modules
        .iter()
        .filter(|module| module.is_link())
        .filter_map(|module| Some((module, folder_of(module, &folder).ok()?)))
        .find(|(_, leads)| is_within(&wanted, &resolved(leads)));
    if let Some((module, _)) = inside_a_link {
        return Some(format!(
            "{} is in the linked content `{}`, which this project already reads: it is not added twice",
            path.display(),
            module.id
        ));
    }
    if is_within(&wanted, &resolved(&project_folder(project))) {
        return None;
    }
    Some(format!(
        "{} is outside the project's folder, so a build could not place it: copy it into the folder, or link its folder if that holds content",
        path.display()
    ))
}

/// Link the folder `target` into the project as content of `kind`, and every build takes it; its id.
///
/// A pack of clothing or addon peds is linked by its folder; Files content is linked by the folder of the
/// project that holds the files, which must have a project file there with a Files content of its own.
///
/// ⚠ Nothing is changed unless it is linked. `id` names the content, else its folder's name does.
pub fn add_link(
    container: &mut Container,
    project: &Path,
    kind: ModuleKind,
    target: &Path,
    id: Option<&str>,
) -> Result<String, String> {
    if let ModuleKind::Unknown(word) = &kind {
        return Err(format!(
            "content of type `{word}` has no folder this build can link"
        ));
    }
    if !target.is_dir() {
        return Err(format!("{} is not a folder", target.display()));
    }
    if kind == ModuleKind::Files {
        let file = project_file_in(target).ok_or_else(|| {
            format!(
                "{} holds no project file, so it has no Files content to link: Files content is linked by the folder of the project that holds the files",
                target.display()
            )
        })?;
        let theirs = Project::load(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if own_files(&theirs).is_none() {
            return Err(format!(
                "{} is a project with no Files content to link",
                target.display()
            ));
        }
    }
    let folder = project_folder(project);
    let (there, here) = (resolved(target), resolved(&folder));
    if is_within(&there, &here) {
        return Err(format!(
            "{} is inside this project's folder, so it is the project's own content and needs no link",
            target.display()
        ));
    }
    if is_within(&here, &there) {
        return Err(format!(
            "{} is a folder around this project: a link is content beside the project, not around it",
            target.display()
        ));
    }
    // ⚠ A folder may be linked once for each kind: an adopted project is both Files and a pack in one folder.
    let held = (container.modules.iter())
        .filter(|m| m.is_link() && m.kind == kind)
        .find(|m| folder_of(m, &folder).is_ok_and(|leads| resolved(&leads) == there));
    if let Some(held) = held {
        return Err(format!(
            "{} is already linked as `{}`",
            target.display(),
            held.id
        ));
    }
    let taken = |candidate: &str| container.modules.iter().any(|m| m.id == candidate);
    let id = match id {
        Some(id) if !is_name(id) => {
            return Err(format!(
                "`{id}` cannot name a folder on every machine: no slash, colon or other reserved character, no leading dot and no trailing dot or space"
            ))
        }
        Some(id) if taken(id) => return Err(format!("the project already holds content `{id}`")),
        Some(id) => id.to_string(),
        None => unique(&leaf_id(target), taken),
    };
    container
        .modules
        .push(Module::linked(&id, kind, &store_link(target, &folder)));
    for output in &mut container.outputs {
        if !output.take.contains(&id) {
            output.take.push(id.clone());
        }
    }
    Ok(id)
}

/// Stop linking `id`: the entry and every build's take of it go, and no file is touched.
pub fn remove_link(container: &mut Container, id: &str) -> Result<(), String> {
    match container.modules.iter().position(|m| m.id == id) {
        None => return Err(format!("the project holds no content `{id}`")),
        Some(at) if !container.modules[at].is_link() => {
            return Err(format!(
                "`{id}` is not a link: removing content is not unlinking it"
            ))
        }
        Some(at) => {
            container.modules.remove(at);
        }
    }
    for output in &mut container.outputs {
        output.take.retain(|taken| taken != id);
    }
    Ok(())
}

/// What linking a whole project linked, and what it left alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Linked {
    /// The id of each content now linked, in the order the project holds its content.
    pub ids: Vec<String>,
    /// The links that project holds itself, which a link does not follow.
    pub not_followed: Vec<String>,
    /// Content of that project this version of OpenRage cannot link, and why.
    pub left: Vec<String>,
}

/// Link every content the project that sits in the folder `target` holds of its own: its Files content by that folder, each pack by its own.
///
/// ⚠ A link is not followed: the links that project holds are named in [`Linked::not_followed`] and
/// not linked here. All or nothing, because a refusal of one content (it is already linked, or inside
/// this project) leaves the container as it was.
pub fn add_project_links(
    container: &mut Container,
    project: &Path,
    target: &Path,
) -> Result<Linked, String> {
    let file = project_file_in(target)
        .ok_or_else(|| format!("{} holds no project file", target.display()))?;
    let theirs = Project::load(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let there = theirs.folder();
    let mut staged = container.clone();
    let mut done = Linked::default();
    for module in &theirs.container.modules {
        if module.is_link() {
            done.not_followed.push(module.id.clone());
            continue;
        }
        let folder = match module.kind {
            ModuleKind::Files => target.to_path_buf(),
            ModuleKind::Clothing
            | ModuleKind::AddonPed
            | ModuleKind::Weapons
            | ModuleKind::Particles
            | ModuleKind::Maps => folder_of(module, &there)?,
            ModuleKind::Unknown(ref word) => {
                done.left.push(format!(
                    "content `{}` is of type `{word}`, which this version cannot link",
                    module.id
                ));
                continue;
            }
        };
        done.ids.push(add_link(
            &mut staged,
            project,
            module.kind.clone(),
            &folder,
            None,
        )?);
    }
    if done.ids.is_empty() {
        return Err(format!("{} holds no content to link", file.display()));
    }
    *container = staged;
    Ok(done)
}

/// What a Files content that is a link hands to a build: the files of the project it leads to, at the paths they have in that project.
///
/// `folders` are the source folders found inside the folders linked Files content leads to, which the build cooks.
/// A problem is the contribution's when the project cannot be read, so a build is refused and never short.
pub fn linked_files(
    module: &Module,
    project_folder: &Path,
    folders: &[SourceFolder],
) -> Contribution {
    let mut contribution = Contribution::of(&module.id);
    let read = folder_of(module, project_folder).and_then(|at| {
        let file = project_file_in(&at).ok_or_else(|| {
            format!(
                "link `{}` leads to {}, which holds no project file",
                module.id,
                at.display()
            )
        })?;
        let theirs = Project::load(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        Ok((at, theirs))
    });
    let (at, theirs) = match read {
        Ok(read) => read,
        Err(why) => {
            contribution.problems.push(why);
            return contribution;
        }
    };
    let Some(files) = own_files(&theirs) else {
        contribution.problems.push(format!(
            "link `{}` leads to {}, a project with no Files content",
            module.id,
            at.display()
        ));
        return contribution;
    };
    let inside: Vec<SourceFolder> = (folders.iter())
        .filter(|f| is_within(&folded(&f.folder), &at))
        .cloned()
        .collect();
    let held: Vec<_> = (theirs.sources().into_iter())
        .filter(|s| {
            !inside
                .iter()
                .any(|f| is_within(&folded(&s.path), &folded(&f.folder)))
        })
        .collect();
    Contribution::files(&module.id, &at, &held)
        .cooked(&at, &inside)
        .with_file_types(files.extra.get(crate::container::FILE_TYPES_KEY))
}

/// The `.src` folders inside the projects that Files content in `container` links, and whether the search stopped at its bounds.
///
/// ⚠ A caller describes these with its own source folders, so a build cooks a linked project's sources as it cooks its own.
pub fn linked_sources(container: &Container, project_folder: &Path) -> (Vec<PathBuf>, bool) {
    let (mut found, mut cut) = (Vec::new(), false);
    for module in container
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Files && m.is_link())
    {
        let Ok(at) = folder_of(module, project_folder) else {
            continue;
        };
        let Some(theirs) = project_file_in(&at).and_then(|file| Project::load(&file).ok()) else {
            continue;
        };
        let members: Vec<PathBuf> = theirs.members.iter().map(|m| m.path.clone()).collect();
        let (sources, truncated) = find_sources(&at, &members);
        found.extend(sources);
        cut |= truncated;
    }
    (found, cut)
}

/// The id a folder suggests for the content it holds: its name, lower case, with anything a folder cannot hold made `_`.
fn leaf_id(folder: &Path) -> String {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(['_', '-']).to_string();
    if is_name(&cleaned) {
        cleaned
    } else {
        "linked".to_string()
    }
}

/// `base`, or `base-2`, `base-3` and so on until no id is taken.
pub(crate) fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut candidate = base.to_string();
    let mut n = 1;
    while taken(&candidate) {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("a").join("police");
        let logos = dir
            .path()
            .join("b")
            .join("brand")
            .join("modules")
            .join("logos");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&logos).unwrap();
        std::fs::write(project.join("police.openrage-proj"), b"{}").unwrap();
        std::fs::write(
            dir.path()
                .join("b")
                .join("brand")
                .join("brand.openrage-proj"),
            b"{}",
        )
        .unwrap();
        (dir, project, logos)
    }

    fn empty() -> Container {
        Container {
            modules: Vec::new(),
            outputs: Vec::new(),
        }
    }

    #[test]
    fn a_link_beside_the_project_is_stored_relative_and_leads_back() {
        let (_dir, project, logos) = tree();
        let stored = store_link(&logos, &project);
        assert_eq!(stored, "../../b/brand/modules/logos");

        let module = Module::linked("logos", ModuleKind::Clothing, &stored);
        let back = folder_of(&module, &project).expect("a stored path leads somewhere");
        assert_eq!(resolved(&back), resolved(&logos));
    }

    #[cfg(windows)]
    #[test]
    fn a_link_to_another_drive_is_stored_absolute() {
        let stored = store_link(Path::new(r"D:\brand\logos"), Path::new(r"C:\mods\police"));
        assert_eq!(stored, r"D:\brand\logos");
        let module = Module::linked("logos", ModuleKind::Clothing, &stored);
        assert_eq!(
            folder_of(&module, Path::new(r"C:\mods\police")).unwrap(),
            PathBuf::from(r"D:\brand\logos")
        );
    }

    #[test]
    fn linked_content_names_the_project_that_holds_it_and_says_it_only_reads_it() {
        let (_dir, project, logos) = tree();
        let mut container = empty();
        let id = add_link(
            &mut container,
            &project.join("police.openrage-proj"),
            ModuleKind::Clothing,
            &logos,
            None,
        )
        .unwrap();
        assert_eq!(id, "logos");

        let found = links(&container, &project.join("police.openrage-proj"));
        assert_eq!(found.len(), 1);
        let link = &found[0];
        assert_eq!(link.problem, None);
        assert_eq!(
            link.owner
                .as_deref()
                .map(|p| p.file_name().unwrap().to_owned()),
            Some("brand.openrage-proj".into())
        );
        let banner = link.banner();
        assert!(
            banner.contains("brand.openrage-proj") && banner.contains("edited there"),
            "{banner}"
        );
    }

    #[test]
    fn a_link_is_refused_where_it_is_no_link() {
        let (dir, project, logos) = tree();
        let file = project.join("police.openrage-proj");
        let inside = project.join("modules").join("uniforms");
        std::fs::create_dir_all(&inside).unwrap();
        let mut container = empty();

        let around = add_link(
            &mut container,
            &file,
            ModuleKind::Clothing,
            dir.path(),
            None,
        );
        let within = add_link(&mut container, &file, ModuleKind::Clothing, &inside, None);
        let itself = add_link(&mut container, &file, ModuleKind::Clothing, &project, None);
        let gone = add_link(
            &mut container,
            &file,
            ModuleKind::Clothing,
            &dir.path().join("nowhere"),
            None,
        );
        let files = add_link(&mut container, &file, ModuleKind::Files, &logos, None);
        for (what, refused, says) in [
            ("a folder around the project", around, "around"),
            ("a folder inside it", within, "inside"),
            ("its own folder", itself, "inside"),
            ("a folder that is not there", gone, "not a folder"),
            (
                "a Files link to a folder that is no project",
                files,
                "no project file",
            ),
        ] {
            let why = refused.expect_err(what);
            assert!(why.contains(says), "{what}: {why}");
        }
        assert!(container.modules.is_empty(), "a refusal changes nothing");

        add_link(&mut container, &file, ModuleKind::Clothing, &logos, None).unwrap();
        let twice = add_link(&mut container, &file, ModuleKind::Clothing, &logos, None);
        assert!(twice.unwrap_err().contains("already linked"));
    }

    #[test]
    fn a_new_link_joins_the_builds_and_unlinking_takes_it_out_again() {
        let (_dir, project, logos) = tree();
        let file = project.join("police.openrage-proj");
        let mut container = Container {
            modules: vec![Module::files("files")],
            outputs: vec![crate::Output::fivem_resource("fivem", "pack", &["files"])],
        };
        let id = add_link(
            &mut container,
            &file,
            ModuleKind::Clothing,
            &logos,
            Some("brand"),
        )
        .unwrap();
        assert_eq!(id, "brand");
        assert_eq!(container.outputs[0].take, ["files", "brand"]);

        let kept = std::fs::read_dir(&logos).unwrap().count();
        remove_link(&mut container, "brand").unwrap();
        assert_eq!(container.outputs[0].take, ["files"]);
        assert_eq!(container.modules.len(), 1);
        assert_eq!(std::fs::read_dir(&logos).unwrap().count(), kept);

        let not_one = remove_link(&mut container, "files").unwrap_err();
        assert!(not_one.contains("not a link"), "{not_one}");
    }

    #[test]
    fn a_write_into_linked_content_is_refused_and_one_beside_it_is_not() {
        let (_dir, project, logos) = tree();
        let file = project.join("police.openrage-proj");
        let mut container = empty();
        add_link(&mut container, &file, ModuleKind::Clothing, &logos, None).unwrap();

        let refused = refuse_write(&container, &file, &logos.join("wardrobe.json")).unwrap_err();
        assert!(
            refused.contains("logos") && refused.contains("brand.openrage-proj"),
            "{refused}"
        );
        assert!(refuse_write(&container, &file, &logos.join("a").join("b.ydd")).is_err());
        assert!(refuse_write(&container, &file, &project.join("wardrobe.json")).is_ok());
        assert!(refuse_write(&container, &file, &logos.with_file_name("other")).is_ok());
    }

    #[test]
    fn a_file_outside_the_project_is_refused_with_a_sentence_and_one_in_a_link_is_not_added_twice()
    {
        let (_dir, project, logos) = tree();
        let file = project.join("police.openrage-proj");
        let mut container = empty();
        add_link(&mut container, &file, ModuleKind::Clothing, &logos, None).unwrap();

        assert_eq!(
            refuse_member(&container, &file, &project.join("client").join("main.lua")),
            None,
            "a file of the project's folder is a member"
        );
        let outside = refuse_member(
            &container,
            &file,
            &project.with_file_name("x").join("a.ydr"),
        )
        .expect("outside the folder");
        assert!(
            outside.contains("outside the project's folder"),
            "{outside}"
        );
        assert!(
            outside.contains("link"),
            "it says what else can be done: {outside}"
        );
        let linked = refuse_member(&container, &file, &logos.join("stream").join("a.ydd"))
            .expect("already read through the link");
        assert!(
            linked.contains("linked content `logos`") && linked.contains("not added twice"),
            "{linked}"
        );
    }

    #[test]
    fn a_link_to_a_folder_that_has_gone_says_where_it_led() {
        let (dir, project, logos) = tree();
        let file = project.join("police.openrage-proj");
        let mut container = empty();
        add_link(&mut container, &file, ModuleKind::Clothing, &logos, None).unwrap();
        std::fs::remove_dir_all(dir.path().join("b")).unwrap();

        let found = links(&container, &file);
        let problem = found[0]
            .problem
            .as_deref()
            .expect("a folder that is gone is a problem");
        assert!(
            problem.contains("logos") && problem.contains("not there"),
            "{problem}"
        );
        assert_eq!(found[0].owner, None);
    }

    #[test]
    fn a_link_in_a_file_survives_the_container_and_other_keys_are_carried() {
        let (_dir, project, logos) = tree();
        let file = project.join("police.openrage-proj");
        let mut container = empty();
        add_link(&mut container, &file, ModuleKind::Clothing, &logos, None).unwrap();
        container.modules[0]
            .extra
            .insert("colour".into(), serde_json::json!("red"));

        let mut top = serde_json::Map::new();
        container.write(&mut top);
        let mut findings = Vec::new();
        let read = Container::read(&top, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(read, container);
        assert!(read.modules[0].is_link());
        assert_eq!(top["modules"][0]["link"], serde_json::json!(true));
        assert_eq!(top["modules"][0]["colour"], serde_json::json!("red"));
    }
}
