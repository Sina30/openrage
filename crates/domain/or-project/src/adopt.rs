//! Adoption: what a folder that already exists holds, grouped into content by what is recognised in it.
//!
//! Reading a folder writes nothing. A kind of content that knows its own files ([`Recogniser`])
//! takes them out of the Files content, says what it could not place, and may ask for a file of
//! its own to be added beside the folder's ([`Write`]); what no content claims is the Files
//! content, exactly as a folder nothing recognises always was. The recognisers are handed in
//! because the content that knows its files depends on this crate and not the other way round.
//!
//! ⚠ **Nothing is moved, copied or rewritten.** An adoption adds files and never replaces one.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::container::{Container, Module, ModuleKind, Output};
use crate::links::unique;
use crate::paths::{relative_inside, BUILD_DIR, PROJECT_EXTENSION, SHADOW_DIR};

/// How many files the Files content of an adopted folder takes at most, so a folder of a whole drive stays a list.
///
/// ⚠ Counted over what no content claimed: a content's own files do not use it up.
pub const MAX_FILES: usize = 10_000;
const MAX_DEPTH: usize = 8;
/// How many of the files or folders a bound leaves out are listed one by one, so what is left out of a whole drive stays a list too.
const MAX_LISTED: usize = 1_000;

/// The key of the project file that records what the adoption could not place.
pub const ADOPTED_KEY: &str = "adopted";

/// A file the adoption saw and did not place where it would have been used, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unplaced {
    /// Inside the folder, with `/` on every host.
    pub path: String,
    pub why: String,
}

/// A file the adoption adds beside the folder's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// What a kind of content found in a folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Recognised {
    /// The content it makes, with its folder as the project file will state it.
    pub module: Module,
    /// One line for the screen: what it found.
    pub summary: String,
    /// Files that are not Files content: the content ships them, writes them again or replaces them.
    pub claimed: Vec<PathBuf>,
    /// What it could not place, and what happens to it.
    pub unplaced: Vec<Unplaced>,
    pub writes: Vec<Write>,
}

/// A kind of content that knows its own files.
pub trait Recogniser {
    fn kind(&self) -> ModuleKind;

    /// Whether `folder` is, as it stands, a folder of this content: cheap, and what a link needs.
    fn holds(&self, folder: &Path) -> bool;

    /// Read `folder`, whose adoptable files are `files`, and say what of it is this content; none when none is.
    fn recognise(&self, folder: &Path, files: &[PathBuf]) -> Result<Option<Recognised>, String>;
}

/// Content an adoption found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub module: Module,
    pub summary: String,
}

/// What a folder holds, grouped.
#[derive(Debug, Clone, Default)]
pub struct Adoption {
    pub folder: PathBuf,
    pub found: Vec<Found>,
    /// The documents of the Files content: every adoptable file no content claimed.
    pub files: Vec<PathBuf>,
    pub unplaced: Vec<Unplaced>,
    /// Things to say that are no file's: a kind of content that could not be read, a folder cut at its bounds.
    pub notes: Vec<String>,
    pub writes: Vec<Write>,
    /// Part of the folder is in no content and not in the Files content: it was past a bound, and [`Adoption::unplaced`] lists it.
    pub truncated: bool,
}

/// Read `folder` and group what it holds.
pub fn analyse(folder: &Path, recognisers: &[&dyn Recogniser]) -> Adoption {
    let (all, too_deep) = files_of(folder);
    let mut adoption = Adoption {
        folder: folder.to_path_buf(),
        ..Adoption::default()
    };
    adoption.leave_out(
        &too_deep,
        format!(
            "{} folder(s) more than {MAX_DEPTH} levels down are not read, so nothing in them is in the project",
            too_deep.len()
        ),
        &format!("more than {MAX_DEPTH} levels down, so it is not read and nothing in it is in the project"),
    );
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    for recogniser in recognisers {
        match recogniser.recognise(folder, &all) {
            Ok(Some(found)) => {
                claimed.extend(found.claimed);
                adoption.unplaced.extend(found.unplaced);
                adoption.writes.extend(found.writes);
                adoption.found.push(Found {
                    module: found.module,
                    summary: found.summary,
                });
            }
            Ok(None) => {}
            Err(why) => adoption.notes.push(format!(
                "{} content in the folder could not be read, so its files stay Files: {why}",
                recogniser.kind().label()
            )),
        }
    }
    let mut files: Vec<PathBuf> = all
        .into_iter()
        .filter(|file| !claimed.contains(file))
        .collect();
    if files.len() > MAX_FILES {
        let cut = files.split_off(MAX_FILES);
        adoption.leave_out(
            &cut,
            format!(
                "The Files content takes {MAX_FILES} files at most, and {} more are not in the project",
                cut.len()
            ),
            "past the files the Files content takes, so it is not in the project",
        );
    }
    adoption.files = files;
    adoption
}

/// Every file of `folder` an adopted project can hold, in path order, and the folders left unread for being too deep.
///
/// ⚠ Not the build and shadow folders, a hidden one, a source folder (the Assets table lists those
/// as sources), a link, or a project file: the files a build would copy, and nothing OpenRage wrote.
/// There is no bound on the files, because what a content claims must be all of them; [`MAX_FILES`]
/// bounds only what no content claims.
pub fn files_of(folder: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut too_deep: Vec<PathBuf> = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let left_alone = name.ends_with(".src")
                    || name.eq_ignore_ascii_case(BUILD_DIR)
                    || name.eq_ignore_ascii_case(SHADOW_DIR);
                if left_alone {
                    continue;
                }
                if depth < MAX_DEPTH {
                    stack.push((entry.path(), depth + 1));
                } else {
                    too_deep.push(entry.path());
                }
            } else if kind.is_file() && !is_project_file(&entry.path()) {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    too_deep.sort();
    (files, too_deep)
}

fn is_project_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(PROJECT_EXTENSION))
}

impl Adoption {
    /// Whether any content was recognised.
    pub fn recognised(&self) -> bool {
        !self.found.is_empty()
    }

    /// Leave `cut` out of the project: say so in a note that begins `what`, and list the first [`MAX_LISTED`] of them with `why`.
    fn leave_out(&mut self, cut: &[PathBuf], what: String, why: &str) {
        if cut.is_empty() {
            return;
        }
        let listed = cut.len().min(MAX_LISTED);
        self.truncated = true;
        self.notes
            .push(format!("{what}; the first {listed} are listed"));
        let folder = &self.folder;
        let left = cut[..listed].iter().map(|path| Unplaced {
            path: relative_inside(path, folder).unwrap_or_else(|| path.display().to_string()),
            why: why.to_string(),
        });
        self.unplaced.extend(left);
    }

    /// The container of the adopted project: the content found, the Files content over the rest, and one FiveM resource named `resource` that takes all of it.
    ///
    /// ⚠ A folder nothing recognised is the Files content alone, as adopting always made it; a folder
    /// whose files are all claimed has no Files content.
    pub fn container(&self, resource: &str) -> Container {
        let mut modules: Vec<Module> = self.found.iter().map(|f| f.module.clone()).collect();
        if !self.files.is_empty() || modules.is_empty() {
            let id = unique("files", |candidate| {
                modules.iter().any(|m| m.id == candidate)
            });
            modules.push(Module::files(&id));
        }
        let take: Vec<&str> = modules.iter().map(|m| m.id.as_str()).collect();
        let output = Output::fivem_resource("fivem", resource, &take);
        Container {
            modules,
            outputs: vec![output],
        }
    }

    /// What the project file records of this adoption, none when there is nothing to say.
    pub fn record(&self) -> Option<Value> {
        if self.unplaced.is_empty() && self.notes.is_empty() {
            return None;
        }
        let unplaced: Vec<Value> = self
            .unplaced
            .iter()
            .map(|u| json!({ "path": u.path, "why": u.why }))
            .collect();
        Some(json!({ "unplaced": unplaced, "notes": self.notes }))
    }

    /// Add the files the adoption asks for, replacing none; on a failure the ones this call added go again.
    pub fn write(&self) -> io::Result<Written> {
        if let Some(there) = self.writes.iter().find(|w| w.path.exists()) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is there already", there.path.display()),
            ));
        }
        let mut written = Written(Vec::new());
        for file in &self.writes {
            if let Err(e) = crate::project::replace_file(&file.path, &file.bytes) {
                written.undo();
                return Err(e);
            }
            written.0.push(file.path.clone());
        }
        Ok(written)
    }
}

/// The files one [`Adoption::write`] added, which it alone may take away again.
#[derive(Debug)]
pub struct Written(Vec<PathBuf>);

impl Written {
    /// Remove what the write added, and nothing else.
    pub fn undo(self) {
        for path in self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(base: &Path, rel: &str, body: &[u8]) -> PathBuf {
        let path = base.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    /// A kind of content that claims the files it is told to, and asks for one file of its own.
    struct Claims {
        mine: Vec<&'static str>,
        asks: Option<&'static str>,
        refuses: bool,
    }

    impl Claims {
        fn of(mine: &[&'static str]) -> Self {
            Self {
                mine: mine.to_vec(),
                asks: None,
                refuses: false,
            }
        }
    }

    impl Recogniser for Claims {
        fn kind(&self) -> ModuleKind {
            ModuleKind::Clothing
        }
        fn holds(&self, folder: &Path) -> bool {
            folder.join("pack.json").is_file()
        }
        fn recognise(
            &self,
            folder: &Path,
            files: &[PathBuf],
        ) -> Result<Option<Recognised>, String> {
            if self.refuses {
                return Err("the pack is called Q, which is not a name".into());
            }
            let claimed: Vec<PathBuf> = self.mine.iter().map(|rel| folder.join(rel)).collect();
            if claimed.is_empty() {
                return Ok(None);
            }
            assert!(
                claimed.iter().all(|c| files.contains(c)),
                "a recogniser is told the files it can claim: {files:?}"
            );
            Ok(Some(Recognised {
                module: Module::clothing("clothing", "."),
                summary: format!("{} file(s) of a pack", claimed.len()),
                claimed,
                unplaced: vec![Unplaced {
                    path: "stream/odd.ydd".into(),
                    why: "its name states no slot; it ships as a loose file".into(),
                }],
                writes: self
                    .asks
                    .map(|rel| {
                        vec![Write {
                            path: folder.join(rel),
                            bytes: b"{}".to_vec(),
                        }]
                    })
                    .unwrap_or_default(),
            }))
        }
    }

    fn folder() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("pack");
        put(&folder, "client/main.lua", b"x");
        put(&folder, "stream/a.ydd", b"a");
        put(&folder, "stream/b.ytd", b"b");
        put(&folder, "stream/odd.ydd", b"o");
        put(&folder, ".git/config", b"hidden");
        put(&folder, "build/old/x.ydr", b"built");
        put(&folder, "vfs/common/y", b"shadow");
        put(&folder, "logos.textures.src/asset.json", b"{}");
        put(&folder, "pack.openrage-proj", b"{}");
        (dir, folder)
    }

    fn names(base: &Path, files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|f| {
                f.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn a_folder_nothing_recognises_is_the_files_content_over_every_file_it_can_hold() {
        let (_dir, folder) = folder();
        let none = Claims::of(&[]);
        let adoption = analyse(&folder, &[&none]);

        assert!(!adoption.recognised());
        assert_eq!(
            names(&folder, &adoption.files),
            [
                "client/main.lua",
                "stream/a.ydd",
                "stream/b.ytd",
                "stream/odd.ydd"
            ],
            "not hidden, build, shadow or source folders, and not the project file"
        );
        let container = adoption.container("pack");
        assert_eq!(container.modules, vec![Module::files("files")]);
        assert_eq!(container.outputs.len(), 1);
        assert_eq!(container.outputs[0].name, "pack");
        assert_eq!(container.outputs[0].take, ["files"]);
        assert!(
            adoption.record().is_none(),
            "nothing to say, nothing recorded"
        );
    }

    #[test]
    fn what_content_claims_is_not_files_and_what_it_could_not_place_is_listed() {
        let (_dir, folder) = folder();
        let mut pack = Claims::of(&["stream/a.ydd", "stream/b.ytd"]);
        pack.asks = Some("wardrobe.json");
        let adoption = analyse(&folder, &[&pack]);

        assert!(adoption.recognised());
        assert_eq!(adoption.found[0].summary, "2 file(s) of a pack");
        assert_eq!(
            names(&folder, &adoption.files),
            ["client/main.lua", "stream/odd.ydd"]
        );
        assert_eq!(adoption.unplaced[0].path, "stream/odd.ydd");
        assert_eq!(adoption.writes[0].path, folder.join("wardrobe.json"));

        let container = adoption.container("pack");
        let ids: Vec<&str> = container.modules.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["clothing", "files"]);
        assert_eq!(container.outputs[0].take, ["clothing", "files"]);

        let record = adoption.record().expect("it has something to say");
        assert_eq!(record["unplaced"][0]["path"], json!("stream/odd.ydd"));
    }

    #[test]
    fn content_that_takes_every_file_leaves_no_files_content() {
        let (_dir, folder) = folder();
        let all = Claims::of(&[
            "client/main.lua",
            "stream/a.ydd",
            "stream/b.ytd",
            "stream/odd.ydd",
        ]);
        let adoption = analyse(&folder, &[&all]);
        assert!(adoption.files.is_empty());
        let container = adoption.container("pack");
        let ids: Vec<&str> = container.modules.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["clothing"]);
        assert_eq!(container.outputs[0].take, ["clothing"]);
    }

    #[test]
    fn content_that_cannot_be_read_costs_itself_and_a_note_and_the_files_stay() {
        let (_dir, folder) = folder();
        let mut broken = Claims::of(&["stream/a.ydd"]);
        broken.refuses = true;
        let adoption = analyse(&folder, &[&broken]);
        assert!(!adoption.recognised());
        assert_eq!(adoption.files.len(), 4, "every file stays Files");
        assert!(
            adoption.notes.iter().any(|n| n.contains("not a name")),
            "{:?}",
            adoption.notes
        );
    }

    #[test]
    fn the_files_of_a_deep_folder_are_cut_and_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        for level in 0..12 {
            deep = deep.join(format!("l{level}"));
            put(&deep, "file.txt", b"x");
        }
        let (files, too_deep) = files_of(dir.path());
        assert!(!too_deep.is_empty(), "a folder twelve deep is cut");
        assert!(files.len() < 12, "{} files", files.len());
    }

    #[test]
    fn an_adoption_adds_its_files_and_replaces_none() {
        let (_dir, folder) = folder();
        let mut pack = Claims::of(&["stream/a.ydd"]);
        pack.asks = Some("wardrobe.json");
        let adoption = analyse(&folder, &[&pack]);

        let first = adoption.write().expect("a new file is added");
        assert_eq!(std::fs::read(folder.join("wardrobe.json")).unwrap(), b"{}");
        drop(first);

        put(&folder, "wardrobe.json", b"mine");
        let refused = adoption.write().unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(folder.join("wardrobe.json")).unwrap(),
            b"mine",
            "a file that is there is never replaced"
        );

        std::fs::remove_file(folder.join("wardrobe.json")).unwrap();
        let written = adoption.write().unwrap();
        assert!(folder.join("wardrobe.json").is_file());
        written.undo();
        assert!(!folder.join("wardrobe.json").exists());
    }

    #[test]
    fn the_cap_counts_what_no_content_took_and_what_it_cuts_is_listed() {
        let (_dir, folder) = folder();
        // Two files stay Files beside these, so the Files content is MAX_FILES + MAX_LISTED + 5 files before the cap.
        for n in 0..MAX_FILES + MAX_LISTED + 3 {
            put(&folder, &format!("zbulk/n{n:05}.txt"), b"x");
        }
        let pack = Claims::of(&["stream/a.ydd", "stream/b.ytd"]);
        let adoption = analyse(&folder, &[&pack]);

        assert_eq!(adoption.files.len(), MAX_FILES);
        let kept = names(&folder, &adoption.files);
        assert!(
            kept.contains(&"client/main.lua".to_string()),
            "a file is not cut for the order it was found in: it sorts before the bulk"
        );
        assert!(
            !kept.contains(&"stream/a.ydd".to_string()),
            "what content took is not Files"
        );
        assert!(adoption.truncated);
        let cut = MAX_LISTED + 5;
        assert!(
            adoption
                .notes
                .iter()
                .any(|n| n.contains(&format!("{cut} more are not in the project"))),
            "the claimed files are not counted against the cap: {:?}",
            adoption.notes
        );
        let listed: Vec<&Unplaced> = adoption
            .unplaced
            .iter()
            .filter(|u| u.path.starts_with("zbulk/"))
            .collect();
        assert_eq!(
            listed.len(),
            MAX_LISTED,
            "what is cut is listed, up to a bound"
        );
        assert_eq!(listed[0].path, format!("zbulk/n{:05}.txt", MAX_FILES - 2));
        assert!(
            listed[0].why.contains("not in the project"),
            "{}",
            listed[0].why
        );
    }

    #[test]
    fn a_folder_too_deep_to_be_read_is_listed_and_not_only_counted() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        for level in 0..12 {
            deep = deep.join(format!("l{level}"));
            put(&deep, "file.txt", b"x");
        }
        let adoption = analyse(dir.path(), &[]);

        assert!(adoption.truncated);
        let cut = (0..=8)
            .map(|l| format!("l{l}"))
            .collect::<Vec<_>>()
            .join("/");
        let left = adoption
            .unplaced
            .iter()
            .find(|u| u.path == cut)
            .unwrap_or_else(|| panic!("{cut} is not listed: {:#?}", adoption.unplaced));
        assert!(left.why.contains("levels down"), "{}", left.why);
        assert!(
            adoption.notes.iter().any(|n| n.contains("not read")),
            "{:?}",
            adoption.notes
        );
        assert!(
            adoption.notes.iter().all(|n| !n.contains("are in it")),
            "no note says files are in the project that are not: {:?}",
            adoption.notes
        );
    }
}
