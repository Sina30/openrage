//! Reopening a built pack as a project: the garments it holds, every model and texture as a source, and what could not be placed.
//!
//! A `dlc.rpf`, an `.oiv` or a resource laid out as a FiveM resource is read file by file into a scratch folder in the
//! layout [`crate::import`] reads, the import makes the pack of it with everything its metadata states, and each model
//! and texture is turned into a source of the new clothing content, as a dropped file is. A file no content reads is
//! kept under `files/` in the Files content and named; a document the build writes again is named and not kept.
//!
//! ⚠ **This reads files, it does not say a pack loads in game.** A project reopened here builds again, and the build
//! reloads what it wrote through the readers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use or_project::adopt::{Adoption, Unplaced};
use or_project::built::{unpack, BuiltKind};
use or_project::{Container, Module, Output, OutputKind, Project, PROJECT_EXTENSION};

use crate::alloc::allocate;
use crate::edit::{Copying, Error as EditError, Source, Watch};
use crate::facts::{inspect, Depth};
use crate::import::{self, LeftOut};
use crate::intake::{self, Scratch};
use crate::model::Wardrobe;
use crate::names::{self, Ped};
use crate::rules::{check, name_of, Severity};

/// The folder of the project that holds the clothing content.
pub const CLOTHING: &str = "clothing";

/// The folder of the project that holds what could not be placed.
pub const FILES: &str = "files";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Refused(String),
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0}")]
    Import(#[from] import::Error),
    #[error("{0}")]
    Edit(#[from] EditError),
    #[error("{0}")]
    Model(#[from] crate::model::Error),
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The name of the pack, where its collections do not state one.
    pub pack: Option<String>,
}

/// What reopening a pack made.
#[derive(Debug, Clone)]
pub struct Reopened {
    /// The project file, in the folder the pack was reopened into.
    pub project: PathBuf,
    pub pack: String,
    pub garments: usize,
    pub textures: usize,
    /// Every file that was not placed where it would be used, and what became of it.
    pub unplaced: Vec<LeftOut>,
    /// What the rules of the engine find wrong in the pack made, errors and warnings: what the Problems list shows next, said here so that a server owner sees what to repair in a pack they did not make.
    pub broken: Vec<Broken>,
    /// What to know that is no file's.
    pub notes: Vec<String>,
}

/// One thing the rules find wrong in a reopened pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// What the garment is called in a sentence, none for the pack.
    pub garment: Option<String>,
    pub severity: Severity,
    /// The sentence the rule says, with no code in front.
    pub message: String,
}

/// What the rules find wrong in `wardrobe`, whose files are in `folder`: errors and warnings that no waiver sets aside, files opened through their readers.
fn broken_in(wardrobe: &Wardrobe, folder: &Path) -> Vec<Broken> {
    let facts = inspect(folder, wardrobe, Depth::Open);
    check(wardrobe, &allocate(wardrobe), &facts)
        .into_iter()
        .filter(|f| f.waived.is_none() && f.severity != Severity::Info)
        .map(|f| Broken {
            garment: (f.garment.as_deref())
                .and_then(|id| wardrobe.garment(id))
                .map(name_of),
            severity: f.severity,
            message: f.message,
        })
        .collect()
}

/// What a file of the pack is to the reopening.
enum Route {
    /// A model, texture, cloth or metadata file: the import reads it.
    Clothing,
    /// A document the build writes again: named and not kept.
    Written,
    /// Kept as a file of the project, for this reason.
    Keep(&'static str),
}

fn route(path: &str, in_pack: bool) -> Route {
    if !in_pack {
        return Route::Keep(
            "the package installs it in the game folder, outside the pack, and no content reads it",
        );
    }
    let lower = path.to_ascii_lowercase();
    if matches!(lower.as_str(), "content.xml" | "setup2.xml") {
        return Route::Written;
    }
    match lower.rsplit_once('.').map(|(_, ext)| ext) {
        Some("ydd" | "ytd" | "yld" | "yft" | "ymt" | "meta") => Route::Clothing,
        _ => Route::Keep("no kind of content here reads it"),
    }
}

/// The path a file of a pack is read at in the layout of a resource, where the folder of a ped's files names the collection and the file does not.
fn laid_out(path: &str) -> String {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = dir.rsplit('/').next().unwrap_or("");
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let compiled = matches!(ext.as_deref(), Some("ydd" | "ytd" | "yld"));
    let of_a_ped = Ped::ALL
        .iter()
        .any(|p| stem.to_ascii_lowercase().starts_with(p.model()));
    match compiled && of_a_ped && !name.contains('^') {
        true => format!("{dir}/{stem}^{name}"),
        false => path.to_string(),
    }
}

/// A file that is no clothing, staged to be kept.
struct Kept {
    staged: PathBuf,
    /// What the file is called in the pack it came from.
    shown: String,
    /// What it is called under `files/`, which every host can create.
    name: String,
    why: String,
}

/// `path` with each part of it made one every host can create: what the host refuses becomes `_`.
fn sanitised(path: &str) -> String {
    let parts: Vec<String> = (path.split('/'))
        .map(|part| {
            let mut name: String = part
                .chars()
                .map(|c| match c.is_control() || "<>:\"/\\|?*".contains(c) {
                    true => '_',
                    false => c,
                })
                .collect();
            while name.ends_with(['.', ' ']) {
                name.pop();
            }
            match name.as_str() {
                "" | "." | ".." => "_".into(),
                _ => name,
            }
        })
        .collect();
    parts.join("/")
}

/// The files of a pack on their way in: the clothing laid out for the import, and the rest staged to be kept.
pub(crate) struct Door {
    scratch: tempfile::TempDir,
    /// Where each file the import reads is in its folder and what it is called in the pack it came from, by its lower-cased path.
    shown: BTreeMap<String, (String, String)>,
    kept: Vec<Kept>,
    written: Vec<LeftOut>,
    watch: Watch,
    /// How many files of the pack went by, and how many there are where the pack says (0 where it does not).
    taken: u64,
    pub(crate) total: u64,
}

impl Door {
    pub(crate) fn new(watch: &Watch) -> Result<Self, Error> {
        let scratch = tempfile::tempdir()
            .map_err(|e| Error::Io("a scratch folder for the pack".into(), e))?;
        Ok(Self {
            scratch,
            shown: BTreeMap::new(),
            kept: Vec::new(),
            written: Vec::new(),
            watch: watch.clone(),
            taken: 0,
            total: 0,
        })
    }

    fn clothing(&self) -> PathBuf {
        self.scratch.path().join("clothing")
    }

    /// Say a file of the pack went by.
    fn counted(&mut self, path: &str) {
        self.taken += 1;
        self.watch.progress.reached(self.taken, self.total);
        self.watch.progress.now_on(path);
    }

    /// The error for `why` a file of the pack was refused: a stop, once somebody asked for one.
    pub(crate) fn refusal(&self, why: String) -> Error {
        match self.watch.check() {
            Ok(()) => Error::Refused(why),
            Err(stopped) => stopped.into(),
        }
    }

    /// Say a file of the pack is not read, and why.
    pub(crate) fn skip(&mut self, path: &str, why: &str) {
        self.counted(path);
        self.written.push(LeftOut {
            path: path.to_string(),
            why: why.to_string(),
        });
    }

    /// Take one file of the pack, which is at `path` in it.
    pub(crate) fn take(&mut self, path: &str, in_pack: bool, bytes: Vec<u8>) -> Result<(), String> {
        if self.watch.check().is_err() {
            return Err("stopped".into());
        }
        self.counted(path);
        let name = sanitised(path);
        let renamed = name != path;
        let place = laid_out(path);
        // ⚠ A folder of its own for each kept file: two names (`what?.txt` and `what_.txt`, `Readme.txt` and `readme.txt`) can be one on the host, and the second would replace the first.
        let apart = (self.scratch.path().join("kept")).join(self.kept.len().to_string());
        let (root, rel, why) = match (route(path, in_pack), renamed) {
            (Route::Written, _) => {
                self.written.push(LeftOut {
                    path: path.to_string(),
                    why: "the build writes it again from the content, so this copy is not carried"
                        .into(),
                });
                return Ok(());
            }
            (Route::Clothing, false) if !self.shown.contains_key(&place.to_lowercase()) => {
                (self.clothing(), place, None)
            }
            (Route::Clothing, false) => (
                apart,
                name,
                Some("another file of the pack is at the same place when the case of a name is ignored"),
            ),
            (Route::Keep(why), false) => (apart, name, Some(why)),
            (_, true) => (
                apart,
                name,
                Some("its name cannot be a file name on every machine, so it is kept under another name"),
            ),
        };
        let at = root.join(&rel);
        std::fs::create_dir_all(at.parent().unwrap_or(&root))
            .and_then(|()| std::fs::write(&at, bytes))
            .map_err(|e| format!("{path}: {e}"))?;
        match why {
            Some(why) => self.kept.push(Kept {
                staged: at,
                shown: path.to_string(),
                name: rel,
                why: why.to_string(),
            }),
            None => {
                self.shown
                    .insert(rel.to_lowercase(), (rel, path.to_string()));
            }
        }
        Ok(())
    }

    /// Make the project in `into` of everything taken, as a build of `kind` would write the pack again.
    pub(crate) fn finish(
        self,
        into: &Path,
        kind: OutputKind,
        options: &Options,
    ) -> Result<Reopened, Error> {
        self.watch.check()?;
        let imported = match import::import(
            &self.clothing(),
            &import::Options {
                pack: options.pack.clone(),
            },
        ) {
            Ok(imported) => imported,
            Err(import::Error::Nothing(_)) => {
                return Err(Error::Refused(
                    "the pack holds no clothing for a freemode ped: no model, texture or variation metadata names one".into(),
                ))
            }
            Err(e) => return Err(e.into()),
        };
        let pack = imported.wardrobe.pack.clone();
        if !names::is_identifier(&pack) {
            return Err(Error::Refused(format!(
                "the pack is called {pack:?}, which is not a lower case name: say the name of the pack to use instead"
            )));
        }
        let existed = into.exists();
        if existed && std::fs::read_dir(into).is_ok_and(|mut d| d.next().is_some()) {
            return Err(Error::Refused(format!(
                "{} is not empty: a pack is imported into a folder of its own",
                into.display()
            )));
        }
        let made = self.make(into, kind, imported, &pack);
        if made.is_err() {
            let _ = match existed {
                true => std::fs::read_dir(into).map(|entries| {
                    entries.flatten().for_each(|e| {
                        let _ = std::fs::remove_dir_all(e.path());
                    })
                }),
                false => std::fs::remove_dir_all(into),
            };
        }
        made
    }

    fn make(
        self,
        into: &Path,
        kind: OutputKind,
        imported: import::Imported,
        pack: &str,
    ) -> Result<Reopened, Error> {
        let import::Imported {
            mut wardrobe,
            report,
        } = imported;
        let from = self.clothing();
        let Door {
            scratch: _scratch,
            shown,
            kept,
            written,
            watch,
            ..
        } = self;
        let clothing = into.join(CLOTHING);
        std::fs::create_dir_all(&clothing)
            .map_err(|e| Error::Io(clothing.display().to_string(), e))?;

        // What the import read is the pack now, except the files that become sources or stay files.
        let mut claimed: BTreeSet<String> =
            report.metadata.iter().map(|p| p.to_lowercase()).collect();
        let mut sources = Sources {
            copying: Copying::new(&clothing),
            scratch: Scratch::default(),
            from: from.clone(),
            refused: BTreeMap::new(),
            claimed: &mut claimed,
        };
        for g in &mut wardrobe.garments {
            watch.check()?;
            if let Some(rel) = g.model.take() {
                g.model = sources.model(&rel)?;
            }
            for rel in std::mem::take(&mut g.alternates) {
                g.alternates.extend(sources.model(&rel)?);
            }
            // A cloth is named after its model, so it goes where the model went.
            if let Some(rel) = g.cloth.take().filter(|_| g.model.is_some()) {
                g.cloth = Some(sources.kept_compiled("cloth", &rel)?);
            }
            for v in &mut g.variations {
                if let Some(rel) = v.texture.take() {
                    v.texture = sources.texture(&rel)?;
                }
            }
        }
        for carried in &mut wardrobe.shop.carried {
            carried.file = sources.kept_compiled("shop", &carried.file)?;
        }
        let refused = sources.refused;
        wardrobe.write(&clothing)?;
        watch.check()?;
        let broken = broken_in(&wardrobe, &clothing);

        // The files of the pack that nothing used: the ones the import left out, the ones that were refused, and the rest.
        let left: BTreeMap<String, String> = (report.left_out.iter())
            .map(|l| (l.path.to_lowercase(), l.why.clone()))
            .collect();
        let mut kept = kept;
        for (key, (rel, shown)) in &shown {
            if claimed.contains(key) {
                continue;
            }
            let why = (refused
                .get(key)
                .map(|why| format!("it could not be turned into a source ({why})")))
            .or_else(|| left.get(key).cloned())
            .unwrap_or_else(|| "no kind of content here reads it".into());
            kept.push(Kept {
                staged: from.join(rel),
                shown: shown.clone(),
                name: shown.clone(),
                why,
            });
        }
        kept.sort_by(|a, b| a.shown.cmp(&b.shown));

        let mut unplaced = written;
        let mut members: Vec<PathBuf> = Vec::new();
        let files = into.join(FILES);
        for file in &kept {
            watch.check()?;
            let rel = free_name(&files, &file.name);
            let to = files.join(&rel);
            std::fs::create_dir_all(to.parent().unwrap_or(&files))
                .and_then(|()| std::fs::copy(&file.staged, &to).map(|_| ()))
                .map_err(|e| Error::Io(to.display().to_string(), e))?;
            unplaced.push(LeftOut {
                path: file.shown.clone(),
                why: format!("{}; kept as {FILES}/{rel}", file.why),
            });
            members.push(to);
        }
        unplaced.sort_by(|a, b| a.path.cmp(&b.path));

        let mut notes = Vec::new();
        let mut modules = vec![Module::clothing("wardrobe", CLOTHING)];
        if !members.is_empty() {
            modules.push(Module::files("files"));
        }
        let (id, take): (&str, Vec<&str>) = match kind {
            OutputKind::FivemResource => ("fivem", modules.iter().map(|m| m.id.as_str()).collect()),
            OutputKind::OivPackage => ("oiv", vec!["wardrobe"]),
            _ => ("dlc", vec!["wardrobe"]),
        };
        if !members.is_empty() && take.len() == 1 {
            notes.push(format!(
                "the {} file(s) under {FILES}/ are in the project and no build takes them: a pack registers a file only under a type the game is told, and a package installs a file where its path says, so state that before a build takes them",
                members.len()
            ));
        }
        let container = Container {
            outputs: vec![Output::of(kind, id, pack, &take)],
            modules,
        };
        let adoption = Adoption {
            unplaced: (unplaced.iter())
                .map(|l| Unplaced {
                    path: l.path.clone(),
                    why: l.why.clone(),
                })
                .collect(),
            notes: notes.clone(),
            ..Adoption::default()
        };
        let file = into.join(format!("{pack}.{PROJECT_EXTENSION}"));
        let mut extra = serde_json::Map::new();
        if let Some(record) = adoption.record() {
            extra.insert(or_project::adopt::ADOPTED_KEY.into(), record);
        }
        Project::create_with(&file, &container, &members, extra)
            .map_err(|e| Error::Io(file.display().to_string(), e))?;
        Ok(Reopened {
            project: file,
            pack: pack.to_string(),
            garments: wardrobe.garments.len(),
            textures: report.textures,
            unplaced,
            broken,
            notes,
        })
    }
}

/// The files of the import on their way into the clothing content.
struct Sources<'a> {
    copying: Copying<'a>,
    scratch: Scratch,
    /// The folder the import read.
    from: PathBuf,
    /// The files that could not be turned into a source, by lower-cased path, with why.
    refused: BTreeMap<String, String>,
    claimed: &'a mut BTreeSet<String>,
}

type Through = fn(&mut Copying, &mut Scratch, &Path) -> Result<String, EditError>;

impl Sources<'_> {
    fn through(&mut self, rel: &str, door: Through) -> Result<Option<String>, Error> {
        let from = self.from.join(rel);
        match door(&mut self.copying, &mut self.scratch, &from) {
            Ok(made) => {
                self.claimed.insert(rel.to_lowercase());
                Ok(Some(made))
            }
            Err(EditError::Refused(why)) => {
                self.refused.insert(rel.to_lowercase(), why);
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn model(&mut self, rel: &str) -> Result<Option<String>, Error> {
        self.through(rel, intake::model)
    }

    fn texture(&mut self, rel: &str) -> Result<Option<String>, Error> {
        self.through(rel, intake::texture)
    }

    /// A file with no source form (a cloth, a shop file the manifest registers), copied as it is into `dir`.
    fn kept_compiled(&mut self, dir: &str, rel: &str) -> Result<String, Error> {
        let source = Source {
            path: self.from.join(rel),
            shown: rel.to_string(),
        };
        let made = self.copying.put(dir, &source)?;
        self.claimed.insert(rel.to_lowercase());
        Ok(made)
    }
}

/// The path under `dir` for the file `shown`, a free one: `-2` before the extension and on when it is taken.
fn free_name(dir: &Path, shown: &str) -> String {
    let (stem, ext) = match shown.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => (stem, format!(".{ext}")),
        _ => (shown, String::new()),
    };
    (1..)
        .map(|n| match n {
            1 => shown.to_string(),
            n => format!("{stem}-{n}{ext}"),
        })
        .find(|candidate| !dir.join(candidate).exists())
        .expect("a free name is found")
}

/// Reopen the built pack `built` (a `dlc.rpf`, an `.oiv`, or the folder that holds one) as a project in `into`.
pub fn reopen(built: &Path, into: &Path, options: &Options) -> Result<Reopened, Error> {
    reopen_watched(built, into, options, &Watch::default())
}

/// [`reopen`] that counts the files of the pack it has read and, once `watch` is asked to stop, makes nothing.
pub fn reopen_watched(
    built: &Path,
    into: &Path,
    options: &Options,
    watch: &Watch,
) -> Result<Reopened, Error> {
    let mut door = Door::new(watch)?;
    let kind = unpack(built, &mut |path, in_pack, bytes| {
        door.take(path, in_pack, bytes)
    })
    .map_err(|why| door.refusal(why))?;
    let output = match kind {
        BuiltKind::Dlc => OutputKind::SingleplayerDlc,
        BuiltKind::Oiv => OutputKind::OivPackage,
    };
    door.finish(into, output, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Wardrobe;

    /// ⚠ The copy of the files kept under `files/` is the longest stretch with a pack's garments done, so a stop asked for there must end it.
    #[test]
    fn a_stop_asked_for_ends_the_copy_of_the_files_kept_under_files() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("readme.txt");
        std::fs::write(&staged, b"notes").unwrap();
        let watch = Watch::default();
        let mut door = Door::new(&watch).unwrap();
        door.kept.push(Kept {
            staged,
            shown: "readme.txt".into(),
            name: "readme.txt".into(),
            why: "no kind of content here reads it".into(),
        });
        let imported = import::Imported {
            wardrobe: Wardrobe::new("demo"),
            report: import::Report::default(),
        };
        let into = dir.path().join("made");

        watch.cancel.set();
        let made = door.make(&into, OutputKind::SingleplayerDlc, imported, "demo");

        assert!(
            matches!(made, Err(Error::Edit(EditError::Stopped))),
            "{made:?}"
        );
        assert!(
            !into.join(FILES).exists(),
            "no file was copied after the stop"
        );
    }
}
