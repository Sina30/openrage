//! Native names, from a list the user supplies or asked OpenRage to download.
//!
//! ⚠ **OpenRage ships no names, and that is a decision rather than a gap.** A
//! script carries only hashes. The names come from community lists, and the
//! two maintained ones — `alloc8or/gta5-nativedb-data` and `citizenfx/natives`
//! — carry no licence, while a hash-to-name table of thousands of engine
//! names is a compilation in the sense `Licence_And_Provenance_Policy.md` §2
//! gives the word. So this module reads a list and never contains one.
//!
//! ⚠ **A version 12 script calls a native by its current hash, which no list
//! is keyed by.** The lists are keyed by each native's first hash; a crossmap
//! of `first, current` pairs ([`NativeDb::cross_map`]) carries a name across.
//!
//! ⚠ **A list belongs to a build, and one install holds several.** A script's
//! hashes are the ones of the build it was compiled against, so a list is
//! judged per script by how many of that script's hashes it names —
//! [`NativeDbSet::best_for`] — and the answer is reported with the list's
//! label rather than assumed.
//!
//! ⚠ **An unnamed hash stays a hash.** A list entry whose name is only the
//! hash spelled `_0x…` names nothing and is not loaded, so it cannot inflate
//! a coverage figure or stand in for a name.
//!
//! Two input forms are read. JSON, either nested by namespace as the
//! community lists publish it (`{"NS": {"0xHASH": {"name": "…"}}}`) or flat
//! (`{"0xHASH": "NAME"}`); and text, one `0xHASH NAME` per line, where a
//! `# build: LABEL` line names the build.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ScrProgram;
use crate::error::{IoError, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeName {
    pub namespace: Option<String>,
    pub name: String,
    /// Empty when the list states no parameters, which a text list never does.
    pub params: Vec<NativeParam>,
    /// The return type as the list spells it, `None` when it states none.
    pub returns: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeParam {
    pub ty: String,
    pub name: String,
}

impl NativeName {
    /// `(Entity entity, BOOL p1) -> BOOL`, or `None` when the list states no types.
    pub fn signature(&self) -> Option<String> {
        if self.params.is_empty() && self.returns.is_none() {
            return None;
        }
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| format!("{} {}", p.ty, p.name))
            .collect();
        Some(format!(
            "({}) -> {}",
            params.join(", "),
            self.returns.as_deref().unwrap_or("void")
        ))
    }
}

impl std::fmt::Display for NativeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.namespace {
            Some(ns) => write!(f, "{ns}::{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// One build's native names, keyed by hash.
#[derive(Clone, Debug, Default)]
pub struct NativeDb {
    label: String,
    names: BTreeMap<u64, NativeName>,
    /// Entries read and not loaded because their name was only their hash.
    unnamed: usize,
}

impl NativeDb {
    /// Parse a list; `label` names the build unless the list names it itself.
    pub fn parse(label: impl Into<String>, bytes: &[u8]) -> Result<Self> {
        let mut db = NativeDb {
            label: label.into(),
            ..Self::default()
        };
        let text = std::str::from_utf8(bytes).map_err(|e| IoError::InvalidData {
            context: format!("natives list `{}` is not UTF-8: {e}", db.label),
        })?;
        let text = text.trim_start_matches('\u{feff}');
        if text.trim_start().starts_with('{') {
            db.parse_json(text)?;
        } else {
            db.parse_text(text)?;
        }
        if db.names.is_empty() {
            return Err(IoError::InvalidData {
                context: format!(
                    "natives list `{}` names no native ({} entr(ies) carried only a hash)",
                    db.label, db.unnamed
                ),
            });
        }
        Ok(db)
    }

    pub fn from_names(
        label: impl Into<String>,
        names: impl IntoIterator<Item = (u64, NativeName)>,
    ) -> Self {
        NativeDb {
            label: label.into(),
            names: names.into_iter().collect(),
            unnamed: 0,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Entries the list held whose name was only their own hash.
    pub fn unnamed(&self) -> usize {
        self.unnamed
    }

    pub fn name(&self, hash: u64) -> Option<&NativeName> {
        self.names.get(&hash)
    }

    /// How many of `program`'s distinct natives this list names.
    pub fn coverage(&self, program: &ScrProgram) -> Coverage {
        let distinct: HashSet<u64> = program.native_hashes().into_iter().collect();
        Coverage {
            named: distinct
                .iter()
                .filter(|h| self.names.contains_key(h))
                .count(),
            distinct: distinct.len(),
        }
    }

    /// Name each crossmap pair's current hash as the list names its first; returns how many it named.
    ///
    /// ⚠ A line holding exactly two hashes is a `first, current` pair and any
    /// other line is ignored, which reads the `{0x…, 0x…},` arrays the
    /// community crossmaps publish. A hash the list already names keeps its name.
    pub fn cross_map(&mut self, label: &str, bytes: &[u8]) -> Result<usize> {
        let text = std::str::from_utf8(bytes).map_err(|e| IoError::InvalidData {
            context: format!("crossmap `{label}` is not UTF-8: {e}"),
        })?;
        let mut pairs = 0;
        let mut named = 0;
        for line in text.lines() {
            let hashes: Vec<u64> = line
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter_map(parse_hash)
                .collect();
            let [first, current] = hashes[..] else {
                continue;
            };
            pairs += 1;
            if first == current || self.names.contains_key(&current) {
                continue;
            }
            if let Some(name) = self.names.get(&first).cloned() {
                self.names.insert(current, name);
                named += 1;
            }
        }
        if pairs == 0 {
            return Err(IoError::InvalidData {
                context: format!("crossmap `{label}` holds no `first, current` hash pair"),
            });
        }
        self.label = format!("{} through {label}", self.label);
        Ok(named)
    }

    fn insert(&mut self, hash: u64, namespace: Option<&str>, name: NativeName) {
        let trimmed = name.name.trim();
        if trimmed.is_empty() || names_only_a_hash(trimmed) {
            self.unnamed += 1;
            return;
        }
        self.names.insert(
            hash,
            NativeName {
                namespace: namespace.map(str::to_owned),
                name: trimmed.to_owned(),
                ..name
            },
        );
    }

    fn parse_json(&mut self, text: &str) -> Result<()> {
        let root: Value = serde_json::from_str(text).map_err(|e| IoError::InvalidData {
            context: format!("natives list `{}` is not valid JSON: {e}", self.label),
        })?;
        let Value::Object(top) = root else {
            return Err(self.shape_error("the top level is not an object"));
        };
        for (key, value) in &top {
            if let Some(hash) = parse_hash(key) {
                self.insert_json_entry(hash, None, value)?;
                continue;
            }
            if key == "build" {
                if let Value::String(build) = value {
                    self.label = build.clone();
                }
                continue;
            }
            let Value::Object(entries) = value else {
                return Err(self.shape_error(&format!("`{key}` is neither a hash nor a namespace")));
            };
            for (hash_key, entry) in entries {
                let hash = parse_hash(hash_key).ok_or_else(|| {
                    self.shape_error(&format!("`{key}` holds `{hash_key}`, which is not a hash"))
                })?;
                self.insert_json_entry(hash, Some(key), entry)?;
            }
        }
        Ok(())
    }

    fn insert_json_entry(
        &mut self,
        hash: u64,
        namespace: Option<&str>,
        value: &Value,
    ) -> Result<()> {
        let name = match value {
            Value::String(name) => NativeName {
                name: name.clone(),
                ..NativeName::default()
            },
            Value::Object(fields) => {
                let Some(Value::String(name)) = fields.get("name") else {
                    return Err(self.shape_error(&format!("0x{hash:016X} has no `name` string")));
                };
                // `return_type` is alloc8or's key and `results` citizenfx's.
                let returns = ["return_type", "results"]
                    .iter()
                    .find_map(|k| fields.get(*k).and_then(Value::as_str))
                    .map(str::to_owned);
                let params = match fields.get("params") {
                    Some(Value::Array(params)) => params
                        .iter()
                        .map(|p| NativeParam {
                            ty: p["type"].as_str().unwrap_or("Any").to_owned(),
                            name: p["name"].as_str().unwrap_or("").to_owned(),
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                NativeName {
                    name: name.clone(),
                    params,
                    returns,
                    ..NativeName::default()
                }
            }
            _ => {
                return Err(
                    self.shape_error(&format!("0x{hash:016X} is neither a name nor an object"))
                )
            }
        };
        self.insert(hash, namespace, name);
        Ok(())
    }

    fn parse_text(&mut self, text: &str) -> Result<()> {
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if let Some(comment) = line.strip_prefix('#') {
                if let Some(build) = comment.trim().strip_prefix("build:") {
                    self.label = build.trim().to_owned();
                }
                continue;
            }
            if line.is_empty() {
                continue;
            }
            let (hash_text, name) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let hash = parse_hash(hash_text).ok_or_else(|| {
                self.shape_error(&format!(
                    "line {} does not start with a hash: `{line}`",
                    n + 1
                ))
            })?;
            let named = |name: &str| NativeName {
                name: name.to_owned(),
                ..NativeName::default()
            };
            match name.trim().split_once("::") {
                Some((ns, name)) => self.insert(hash, Some(ns), named(name)),
                None => self.insert(hash, None, named(name)),
            }
        }
        Ok(())
    }

    fn shape_error(&self, what: &str) -> IoError {
        IoError::InvalidData {
            context: format!("natives list `{}`: {what}", self.label),
        }
    }
}

/// `named` of a script's `distinct` natives have a name in one list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub named: usize,
    pub distinct: usize,
}

impl Coverage {
    pub fn is_complete(&self) -> bool {
        self.named == self.distinct
    }
}

/// The lists a user has supplied, one per build.
#[derive(Clone, Debug, Default)]
pub struct NativeDbSet {
    dbs: Vec<NativeDb>,
}

impl NativeDbSet {
    pub fn new(dbs: Vec<NativeDb>) -> Self {
        NativeDbSet { dbs }
    }

    pub fn is_empty(&self) -> bool {
        self.dbs.is_empty()
    }

    pub fn dbs(&self) -> &[NativeDb] {
        &self.dbs
    }

    /// The list naming the most of `program`'s natives, and how many it names.
    ///
    /// ⚠ Ties go to the list supplied first, and a list naming none of the
    /// script's natives is not an answer.
    pub fn best_for(&self, program: &ScrProgram) -> Option<(&NativeDb, Coverage)> {
        let mut best: Option<(&NativeDb, Coverage)> = None;
        for db in &self.dbs {
            let coverage = db.coverage(program);
            if coverage.named == 0 {
                continue;
            }
            if best.is_none_or(|(_, b)| coverage.named > b.named) {
                best = Some((db, coverage));
            }
        }
        best
    }

    /// What a download left in `dir`, each list labelled by its upstream commit and carried through every crossmap; `None` when `dir` holds no download.
    pub fn from_download(dir: &Path) -> Result<Option<Self>> {
        let manifest = match std::fs::read(dir.join(DOWNLOAD_MANIFEST)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let files: Vec<DownloadedFile> =
            serde_json::from_slice(&manifest).map_err(|e| IoError::InvalidData {
                context: format!("{}: {e}", dir.join(DOWNLOAD_MANIFEST).display()),
            })?;
        let read = |f: &DownloadedFile| std::fs::read(dir.join(&f.file));
        let mut dbs = Vec::new();
        for f in files.iter().filter(|f| !f.crossmap) {
            dbs.push(NativeDb::parse(f.label(), &read(f)?)?);
        }
        for f in files.iter().filter(|f| f.crossmap) {
            let bytes = read(f)?;
            for db in &mut dbs {
                db.cross_map(&f.label(), &bytes)?;
            }
        }
        Ok(Some(NativeDbSet::new(dbs)))
    }
}

/// The file a download writes beside what it fetched, naming each file's upstream commit.
pub const DOWNLOAD_MANIFEST: &str = "source.json";

/// One file a download fetched, and the upstream commit it came from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadedFile {
    /// The file's name in the download directory.
    pub file: String,
    pub repo: String,
    pub path: String,
    pub commit: String,
    /// A crossmap of `first, current` hash pairs rather than a list of names.
    pub crossmap: bool,
}

impl DownloadedFile {
    /// `repo/path@commit`, the commit cut to twelve digits.
    pub fn label(&self) -> String {
        let commit = self.commit.get(..12).unwrap_or(&self.commit);
        format!("{}/{}@{commit}", self.repo, self.path)
    }
}

fn parse_hash(text: &str) -> Option<u64> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    if digits.is_empty() || digits.len() > 16 {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

/// A name that is a hash spelled as an identifier, which names nothing.
fn names_only_a_hash(name: &str) -> bool {
    name.strip_prefix("_0x")
        .or_else(|| name.strip_prefix("0x"))
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program_calling(hashes: &[u64]) -> ScrProgram {
        let code_length = 0x1234;
        ScrProgram {
            code_length,
            natives: hashes
                .iter()
                .enumerate()
                .map(|(i, h)| h.rotate_right(code_length.wrapping_add(i as u32) & 0x3F))
                .collect(),
            ..ScrProgram::default()
        }
    }

    #[test]
    fn a_nested_list_names_by_namespace_and_skips_a_hash_spelled_as_a_name() {
        let json = br#"{
            "ENTITY": {
                "0x3FEF770D40960D5A": { "name": "GET_ENTITY_COORDS", "params": [] },
                "0x0000000000000ABC": { "name": "_0x0000000000000ABC" }
            },
            "PLAYER": { "0xD80958FC74E988A6": { "name": "PLAYER_PED_ID" } }
        }"#;
        let db = NativeDb::parse("nested.json", json).unwrap();
        assert_eq!(db.len(), 2);
        assert_eq!(db.unnamed(), 1);
        assert_eq!(
            db.name(0x3FEF770D40960D5A).unwrap().to_string(),
            "ENTITY::GET_ENTITY_COORDS"
        );
        assert!(
            db.name(0xABC).is_none(),
            "a hash spelled as a name is not a name"
        );
    }

    #[test]
    fn a_text_list_names_its_own_build() {
        let text = b"# build: 3095\n0xD80958FC74E988A6 PLAYER::PLAYER_PED_ID\n\n0x1 WAIT\n";
        let db = NativeDb::parse("file.txt", text).unwrap();
        assert_eq!(db.label(), "3095");
        assert_eq!(db.name(1).unwrap().to_string(), "WAIT");
        assert_eq!(db.len(), 2);
    }

    #[test]
    fn a_list_that_names_nothing_is_refused_rather_than_loaded_empty() {
        let why = NativeDb::parse("empty", br#"{"NS": {"0x1": {"name": "_0x1"}}}"#)
            .unwrap_err()
            .to_string();
        assert!(why.contains("names no native"), "{why}");
        assert!(NativeDb::parse("bad", b"GET_ENTITY_COORDS 0x1").is_err());
    }

    /// ⚠ Coverage is over the script's *de-rotated* hashes; handing the
    /// stored slots to the list would name nothing.
    #[test]
    fn the_best_list_for_a_script_is_the_one_naming_most_of_its_natives() {
        let program = program_calling(&[1, 2, 3, 3]);
        let older = NativeDb::from_names("older", [(1, name("A"))]);
        let newer = NativeDb::from_names("newer", [(1, name("A")), (2, name("B"))]);
        let unrelated = NativeDb::from_names("unrelated", [(99, name("Z"))]);
        let set = NativeDbSet::new(vec![unrelated, older, newer]);

        let (db, coverage) = set.best_for(&program).unwrap();
        assert_eq!(db.label(), "newer");
        assert_eq!(
            coverage,
            Coverage {
                named: 2,
                distinct: 3
            }
        );
        assert!(!coverage.is_complete());

        let none = NativeDbSet::new(vec![NativeDb::from_names("x", [(99, name("Z"))])]);
        assert!(none.best_for(&program).is_none());
    }

    /// ⚠ The pair is `first, current`; read the other way round it names nothing a script calls.
    #[test]
    fn a_crossmap_names_a_current_hash_as_the_list_names_its_first() {
        let json = br#"{"PLAYER": {"0x00000000000000A1": {
            "name": "PLAYER_PED_ID", "params": [], "return_type": "Ped"}}}"#;
        let mut db = NativeDb::parse("natives.json", json).unwrap();
        let crossmap =
            b"g_crossmap[] {\n  {0x00000000000000A1, 0x00000000000000C3},\n  {0xA2, 0xC4},\n};\n";
        assert_eq!(db.cross_map("crossmap.txt", crossmap).unwrap(), 1);
        let current = db.name(0xC3).expect("the current hash is named");
        assert_eq!(current.to_string(), "PLAYER::PLAYER_PED_ID");
        assert_eq!(current.signature().as_deref(), Some("() -> Ped"));
        assert!(
            db.name(0xC4).is_none(),
            "an unnamed first hash names nothing"
        );
        assert_eq!(db.label(), "natives.json through crossmap.txt");
        assert!(db.cross_map("empty", b"no pairs here").is_err());
    }

    #[test]
    fn a_download_loads_each_list_under_its_commit_through_the_crossmap() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path();
        assert!(NativeDbSet::from_download(dir).unwrap().is_none());
        std::fs::write(
            dir.join("natives.json"),
            br#"{"NS": {"0xA1": {"name": "A"}}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("crossmap.txt"), b"{0xA1, 0xC3},\n").unwrap();
        let files = [
            DownloadedFile {
                file: "natives.json".into(),
                repo: "owner/names".into(),
                path: "natives.json".into(),
                commit: "0123456789abcdef0123".into(),
                crossmap: false,
            },
            DownloadedFile {
                file: "crossmap.txt".into(),
                repo: "owner/map".into(),
                path: "crossmap.txt".into(),
                commit: "fedcba9876543210".into(),
                crossmap: true,
            },
        ];
        std::fs::write(
            dir.join(DOWNLOAD_MANIFEST),
            serde_json::to_vec(&files).unwrap(),
        )
        .unwrap();
        let set = NativeDbSet::from_download(dir).unwrap().unwrap();
        let [db] = set.dbs() else { panic!("one list") };
        assert_eq!(
            db.label(),
            "owner/names/natives.json@0123456789ab through owner/map/crossmap.txt@fedcba987654"
        );
        assert_eq!(db.name(0xC3).unwrap().to_string(), "NS::A");
    }

    fn name(n: &str) -> NativeName {
        NativeName {
            name: n.to_owned(),
            ..NativeName::default()
        }
    }
}
