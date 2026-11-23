//! The keys the container adds to a project file: the modules it holds and the outputs it declares.
//!
//! ⚠ **Read field by field, through [`Value`].** A derived `Deserialize` turns one
//! unreadable field into an unreadable file. A field that fails to read costs itself and
//! a line in the findings, and a key this build does not know is carried and written back,
//! which is what lets an older build open a newer file and keep what it cannot read.

use std::collections::BTreeMap;

use or_io::Generation;
use serde_json::{Map, Value};

use crate::contribution::is_type;
use crate::paths::is_component;

pub const MODULES_KEY: &str = "modules";
pub const OUTPUTS_KEY: &str = "outputs";

/// The key of a module entry that says its folder is another project's.
pub const LINK_KEY: &str = "link";

/// The key of a Files content that states the type of each of its files, by path.
pub const FILE_TYPES_KEY: &str = "file_types";

/// The id of the Files module a project with no `modules` key is read as holding.
pub const IMPLICIT_FILES: &str = "files";

/// What a project holds and what it declares to build.
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub modules: Vec<Module>,
    pub outputs: Vec<Output>,
}

/// A project of today, which is one Files module over its documents and no output.
impl Default for Container {
    fn default() -> Self {
        Self {
            modules: vec![Module::files(IMPLICIT_FILES)],
            outputs: Vec::new(),
        }
    }
}

/// A typed part of a project, shown on screen as Content.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub id: String,
    pub kind: ModuleKind,
    /// Keys on this entry this build does not read, written back as they came.
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleKind {
    /// The project's documents, copied as they are.
    Files,
    /// A Wardrobe: garments of a freemode ped, in a folder of the project that `or-wardrobe` reads.
    Clothing,
    /// Addon peds: streamed ped models and the data that defines them, in a folder of the project that `or-addonped` reads.
    AddonPed,
    /// Weapons: their models, textures and the metadata that defines them, in a folder of the project that `or-weapons` reads.
    Weapons,
    /// Particles: the effects of `.ypt` files as sources, in a folder of the project that `or-particles` reads.
    Particles,
    /// Maps: the map files, type files, models and textures of map sets as sources, in a folder of the project that `or-maps` reads.
    Maps,
    /// A type a newer build wrote: carried, and refused by a plan.
    Unknown(String),
}

/// Something the project builds, shown on screen as a Build.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub id: String,
    pub kind: OutputKind,
    /// The folder the build writes, and the resource's name.
    pub name: String,
    /// The ids of the modules whose content it ships.
    pub take: Vec<String>,
    /// A ceiling on the files a build writes; none when absent.
    pub max_files: Option<u64>,
    /// A ceiling on the bytes a build writes; none when absent.
    pub max_bytes: Option<u64>,
    /// Ceilings on the named figures that content contributes, such as an entry count, and on `memory`, the streaming memory the RSC7 files of a build take.
    pub limits: BTreeMap<String, u64>,
    /// Whether a build over a ceiling divides at content boundaries instead of being refused.
    pub split: bool,
    /// Whether a source that holds the game's own content and has not changed ships anyway, as a pack that replaces the game's file needs.
    pub ship_unchanged: bool,
    /// The layout every drawable and texture dictionary it ships ends in; each ships as authored when absent.
    pub target: Option<Target>,
    /// Keys on this entry this build does not read, written back as they came.
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputKind {
    FivemResource,
    /// A `dlc.rpf` for singleplayer, with the `dlclist.xml` line it needs.
    SingleplayerDlc,
    /// An `.oiv` package, for OpenIV to install.
    OivPackage,
    /// A kind a newer build wrote: carried, and refused by a plan.
    Unknown(String),
}

/// The layout of resource a build ships, which the screen calls the edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Legacy,
    Enhanced,
}

impl Target {
    /// The word the project file stores.
    pub fn word(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Enhanced => "enhanced",
        }
    }

    /// The RSC7 layout it names.
    pub fn generation(self) -> Generation {
        match self {
            Self::Legacy => Generation::Gen8,
            Self::Enhanced => Generation::Gen9,
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "legacy" => Some(Self::Legacy),
            "enhanced" => Some(Self::Enhanced),
            _ => None,
        }
    }
}

impl Module {
    /// A clothing module whose content is the folder `path` of the project, relative to its folder.
    pub fn clothing(id: &str, path: &str) -> Self {
        Self::in_folder(id, ModuleKind::Clothing, path)
    }

    /// An addon ped module whose content is the folder `path` of the project, relative to its folder.
    pub fn addon_ped(id: &str, path: &str) -> Self {
        Self::in_folder(id, ModuleKind::AddonPed, path)
    }

    /// A weapons module whose content is the folder `path` of the project, relative to its folder.
    pub fn weapons(id: &str, path: &str) -> Self {
        Self::in_folder(id, ModuleKind::Weapons, path)
    }

    /// A particles module whose content is the folder `path` of the project, relative to its folder.
    pub fn particles(id: &str, path: &str) -> Self {
        Self::in_folder(id, ModuleKind::Particles, path)
    }

    /// A maps module whose content is the folder `path` of the project, relative to its folder.
    pub fn maps(id: &str, path: &str) -> Self {
        Self::in_folder(id, ModuleKind::Maps, path)
    }

    fn in_folder(id: &str, kind: ModuleKind, path: &str) -> Self {
        let mut extra = Map::new();
        extra.insert("path".into(), Value::String(path.to_string()));
        Self {
            id: id.to_string(),
            kind,
            extra,
        }
    }

    /// The folder of the project the module's content is in, as the file states it: a module with no folder of its own, such as Files, has none.
    pub fn path(&self) -> Option<&str> {
        self.extra.get("path")?.as_str()
    }

    pub fn files(id: &str) -> Self {
        Self {
            id: id.to_string(),
            kind: ModuleKind::Files,
            extra: Map::new(),
        }
    }

    /// A module whose folder lives elsewhere, at `stored` as the project file spells it.
    pub fn linked(id: &str, kind: ModuleKind, stored: &str) -> Self {
        let mut extra = Map::new();
        extra.insert("path".into(), Value::String(stored.to_string()));
        extra.insert(LINK_KEY.into(), Value::Bool(true));
        Self {
            id: id.to_string(),
            kind,
            extra,
        }
    }

    /// Whether its folder is another project's, read here and written there ([`crate::links`]).
    pub fn is_link(&self) -> bool {
        self.extra.get(LINK_KEY) == Some(&Value::Bool(true))
    }

    /// The types this Files content states for its files, by the path of each inside the project.
    pub fn file_types(&self) -> BTreeMap<String, String> {
        let stated = self.extra.get(FILE_TYPES_KEY).and_then(Value::as_object);
        (stated.into_iter().flatten())
            .filter_map(|(path, ty)| Some((path.clone(), ty.as_str()?.to_string())))
            .collect()
    }

    /// State the type the game is told for the file at `path` inside the project, or take the statement away.
    ///
    /// ⚠ Only the project's own Files content states one: a link's files are stated in the project that holds them.
    pub fn set_file_type(&mut self, path: &str, file_type: Option<&str>) -> Result<(), String> {
        if self.kind != ModuleKind::Files || self.is_link() {
            return Err(format!(
                "content `{}` is not the project's own Files content, so it states no file type",
                self.id
            ));
        }
        if !path.split('/').all(is_component) {
            return Err(format!(
                "`{path}` is not a path inside the project: it needs `/` between names, and names every host can create"
            ));
        }
        let mut types = match self.extra.remove(FILE_TYPES_KEY) {
            Some(Value::Object(types)) => types,
            _ => Map::new(),
        };
        let put_back = |extra: &mut Map<String, Value>, types: Map<String, Value>| {
            if !types.is_empty() {
                extra.insert(FILE_TYPES_KEY.into(), Value::Object(types));
            }
        };
        match file_type {
            Some(ty) if is_type(ty) => {
                types.insert(path.to_string(), Value::String(ty.to_string()));
            }
            Some(ty) => {
                put_back(&mut self.extra, types);
                return Err(format!(
                    "`{ty}` is not a type the game knows by name: capitals, digits and underscores, such as HANDLING_FILE"
                ));
            }
            None => {
                types.remove(path);
            }
        }
        put_back(&mut self.extra, types);
        Ok(())
    }
}

impl Output {
    pub fn of(kind: OutputKind, id: &str, name: &str, take: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            take: take.iter().map(|id| id.to_string()).collect(),
            max_files: None,
            max_bytes: None,
            limits: BTreeMap::new(),
            split: false,
            ship_unchanged: false,
            target: None,
            extra: Map::new(),
        }
    }

    pub fn fivem_resource(id: &str, name: &str, take: &[&str]) -> Self {
        Self::of(OutputKind::FivemResource, id, name, take)
    }
}

impl Container {
    /// Declare a build that writes `name` as a resource of `kind` from all the content, with every drawable and texture dictionary moved to `target` when there is one; its id.
    pub fn add_build(
        &mut self,
        name: &str,
        kind: OutputKind,
        target: Option<Target>,
    ) -> Result<String, String> {
        self.check_build_name(name, None)?;
        let id = unique_id(name, self.outputs.iter().map(|o| o.id.as_str()));
        let take: Vec<&str> = self.modules.iter().map(|m| m.id.as_str()).collect();
        let mut output = Output::of(kind, &id, name, &take);
        output.target = target;
        self.outputs.push(output);
        Ok(id)
    }

    /// Why `name` cannot be the name of the build `build` (none for a new one), none when it can.
    pub fn check_build_name(&self, name: &str, build: Option<&str>) -> Result<(), String> {
        if !is_name(name) {
            return Err(format!(
                "`{name}` cannot name a folder on every machine: it needs no slash, colon or other reserved character, no leading dot and no trailing dot or space"
            ));
        }
        let taken = (self.outputs.iter())
            .any(|o| Some(o.id.as_str()) != build && o.name.eq_ignore_ascii_case(name));
        if taken {
            return Err(format!("a build already writes a resource named `{name}`"));
        }
        Ok(())
    }

    /// The name a new build of `kind` for `target` is offered under, for the project called `base`: free, and never one a part of a split reads as.
    ///
    /// ⚠ `<name>_2` is what a split writes its second resource as, so a second build is named for what it is
    /// (`<base>_dlc`, `<base>_enhanced`) and, when that is taken too, `<base>_v2` and on.
    pub fn free_build_name(&self, base: &str, kind: &OutputKind, target: Option<Target>) -> String {
        let free = |name: &str| {
            !(self.outputs.iter())
                .any(|o| o.name.eq_ignore_ascii_case(name) || reads_as_a_part(&o.name, name))
        };
        let kind_word = match kind {
            OutputKind::SingleplayerDlc => Some("dlc"),
            OutputKind::OivPackage => Some("oiv"),
            _ => None,
        };
        let mut name = base.to_string();
        let mut candidates = vec![name.clone()];
        for word in [kind_word, target.map(Target::word)].into_iter().flatten() {
            name = format!("{name}_{word}");
            candidates.push(name.clone());
        }
        let fresh = (2..).map(|n| format!("{name}_v{n}"));
        (candidates.into_iter().chain(fresh))
            .find(|candidate| free(candidate))
            .unwrap_or(name)
    }

    /// Change the build `id` as `change` says, or refuse with the reason and change nothing.
    ///
    /// ⚠ What is kept is checked after the change: the name can name a folder and no other build writes it,
    /// every content it takes is the project's and is taken once, and a named limit is not one of the totals
    /// that `max_files` and `max_bytes` own. The id never changes.
    pub fn edit_build(&mut self, id: &str, change: impl FnOnce(&mut Output)) -> Result<(), String> {
        let at = (self.outputs.iter().position(|o| o.id == id))
            .ok_or_else(|| format!("the project declares no build `{id}`"))?;
        let mut edited = self.outputs[at].clone();
        change(&mut edited);
        edited.id = id.to_string();
        self.check_build_name(&edited.name, Some(id))?;
        for (n, taken) in edited.take.iter().enumerate() {
            if self.module(taken).is_none() {
                return Err(format!("the project holds no content `{taken}`"));
            }
            if edited.take[..n].contains(taken) {
                return Err(format!("build `{id}` takes `{taken}` twice"));
            }
        }
        if let Some(total) = TOTALS.iter().find(|t| edited.limits.contains_key(**t)) {
            return Err(format!(
                "`{total}` is not the name of a figure: the build's ceiling on {total} is `max_{total}`"
            ));
        }
        self.outputs[at] = edited;
        Ok(())
    }

    /// Make the build `build` take the content `content`, or leave it out, keeping the order of the content.
    pub fn set_taken(&mut self, build: &str, content: &str, taken: bool) -> Result<(), String> {
        if self.module(content).is_none() {
            return Err(format!("the project holds no content `{content}`"));
        }
        let order: Vec<String> = self.modules.iter().map(|m| m.id.clone()).collect();
        let place = move |id: &String| order.iter().position(|m| m == id).unwrap_or(usize::MAX);
        self.edit_build(build, |output| {
            // ⚠ A build whose order was chosen (a split cuts in the order of `take`) keeps it.
            let in_order = output.take.is_sorted_by_key(&place);
            output.take.retain(|id| id != content);
            if taken {
                output.take.push(content.to_string());
                if in_order {
                    output.take.sort_by_key(&place);
                }
            }
        })
    }

    /// Stop declaring the build `id`, and say whether there was one; what it wrote stays until a clean.
    pub fn remove_build(&mut self, id: &str) -> bool {
        let before = self.outputs.len();
        self.outputs.retain(|o| o.id != id);
        self.outputs.len() != before
    }

    pub fn module(&self, id: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.id == id)
    }

    pub fn output(&self, id: &str) -> Option<&Output> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// Read the container out of a project file's top-level keys, which it leaves as they are.
    ///
    /// ⚠ **A file with no `modules` key holds one Files module**, because that is a project of
    /// today and its documents are what that module holds. The key's presence decides and
    /// never the schema number, since an older build saves a newer file under its own number.
    pub fn read(top: &Map<String, Value>, findings: &mut Vec<String>) -> Self {
        let modules = match top.get(MODULES_KEY) {
            None | Some(Value::Null) => Self::default().modules,
            Some(Value::Array(entries)) => read_modules(entries, findings),
            Some(v) => {
                findings.push(format!(
                    "`modules` is {v}, not a list; the project holds none"
                ));
                Vec::new()
            }
        };
        let outputs = match top.get(OUTPUTS_KEY) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(entries)) => read_outputs(entries, findings),
            Some(v) => {
                findings.push(format!(
                    "`outputs` is {v}, not a list; the project declares none"
                ));
                Vec::new()
            }
        };
        Self { modules, outputs }
    }

    /// Write the container into a project file's top-level keys.
    pub fn write(&self, top: &mut Map<String, Value>) {
        let modules = self.modules.iter().map(|m| {
            let mut entry = m.extra.clone();
            entry.insert("id".into(), Value::String(m.id.clone()));
            entry.insert("type".into(), Value::String(m.kind.word().to_string()));
            Value::Object(entry)
        });
        top.insert(MODULES_KEY.into(), Value::Array(modules.collect()));
        let outputs = self.outputs.iter().map(|o| {
            let mut entry = o.extra.clone();
            entry.insert("id".into(), Value::String(o.id.clone()));
            entry.insert("kind".into(), Value::String(o.kind.word().to_string()));
            entry.insert("name".into(), Value::String(o.name.clone()));
            entry.insert(
                "take".into(),
                Value::Array(o.take.iter().cloned().map(Value::String).collect()),
            );
            for (key, limit) in [("max_files", o.max_files), ("max_bytes", o.max_bytes)] {
                if let Some(limit) = limit {
                    entry.insert(key.into(), limit.into());
                }
            }
            if let Some(target) = o.target {
                entry.insert("target".into(), Value::String(target.word().to_string()));
            }
            if o.split {
                entry.insert("split".into(), Value::Bool(true));
            }
            if o.ship_unchanged {
                entry.insert(SHIP_UNCHANGED_KEY.into(), Value::Bool(true));
            }
            if !o.limits.is_empty() {
                let limits = o
                    .limits
                    .iter()
                    .map(|(name, limit)| (name.clone(), (*limit).into()));
                entry.insert("limits".into(), Value::Object(limits.collect()));
            }
            Value::Object(entry)
        });
        top.insert(OUTPUTS_KEY.into(), Value::Array(outputs.collect()));
    }
}

impl ModuleKind {
    /// What the screen calls it.
    pub fn label(&self) -> String {
        match self {
            Self::Files => "Files".to_string(),
            Self::Clothing => "Clothing".to_string(),
            Self::AddonPed => "Addon ped".to_string(),
            Self::Weapons => "Weapons".to_string(),
            Self::Particles => "Particles".to_string(),
            Self::Maps => "Maps".to_string(),
            Self::Unknown(word) => format!("{word} (a newer version's)"),
        }
    }

    fn word(&self) -> &str {
        match self {
            Self::Files => "files",
            Self::Clothing => "clothing",
            Self::AddonPed => "addon-ped",
            Self::Weapons => "weapons",
            Self::Particles => "particles",
            Self::Maps => "maps",
            Self::Unknown(word) => word,
        }
    }
}

impl OutputKind {
    /// What the screen and the commands call it.
    pub fn label(&self) -> String {
        match self {
            Self::FivemResource => "FiveM resource".to_string(),
            Self::SingleplayerDlc => "singleplayer DLC".to_string(),
            Self::OivPackage => "OIV package".to_string(),
            Self::Unknown(word) => format!("{word} (a newer version's)"),
        }
    }
    /// The word the project file stores.
    pub fn word(&self) -> &str {
        match self {
            Self::FivemResource => "fivem-resource",
            Self::SingleplayerDlc => "singleplayer-dlc",
            Self::OivPackage => "oiv-package",
            Self::Unknown(word) => word,
        }
    }
}

const KNOWN_MODULE: [&str; 2] = ["id", "type"];
/// The key of an output entry that makes a build ship a source of the game's own content that has not changed.
pub const SHIP_UNCHANGED_KEY: &str = "ship_unchanged";

const KNOWN_OUTPUT: [&str; 10] = [
    "id",
    "kind",
    "name",
    "take",
    "max_files",
    "max_bytes",
    "limits",
    "split",
    SHIP_UNCHANGED_KEY,
    "target",
];

/// The names of the plan's own totals, whose ceilings are `max_files` and `max_bytes`.
pub(crate) const TOTALS: [&str; 2] = ["files", "bytes"];

fn read_modules(entries: &[Value], findings: &mut Vec<String>) -> Vec<Module> {
    let mut modules: Vec<Module> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(fields) = entry_of("content", index, entry, findings) else {
            continue;
        };
        let Some(id) = read_id(
            fields,
            "content",
            index,
            |id| modules.iter().any(|m| m.id == id),
            findings,
        ) else {
            continue;
        };
        let Some(word) = fields.get("type").and_then(Value::as_str) else {
            findings.push(format!("content `{id}` has no readable `type`; skipped"));
            continue;
        };
        let kind = match word {
            "files" => ModuleKind::Files,
            "clothing" => ModuleKind::Clothing,
            "addon-ped" => ModuleKind::AddonPed,
            "weapons" => ModuleKind::Weapons,
            "particles" => ModuleKind::Particles,
            "maps" => ModuleKind::Maps,
            other => ModuleKind::Unknown(other.to_string()),
        };
        modules.push(Module {
            id,
            kind,
            extra: unread(fields, &KNOWN_MODULE),
        });
    }
    modules
}

fn read_outputs(entries: &[Value], findings: &mut Vec<String>) -> Vec<Output> {
    let mut outputs: Vec<Output> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(fields) = entry_of("build", index, entry, findings) else {
            continue;
        };
        let Some(id) = read_id(
            fields,
            "build",
            index,
            |id| outputs.iter().any(|o| o.id == id),
            findings,
        ) else {
            continue;
        };
        let Some(word) = fields.get("kind").and_then(Value::as_str) else {
            findings.push(format!("build `{id}` has no readable `kind`; skipped"));
            continue;
        };
        let kind = match word {
            "fivem-resource" => OutputKind::FivemResource,
            "singleplayer-dlc" => OutputKind::SingleplayerDlc,
            "oiv-package" => OutputKind::OivPackage,
            other => OutputKind::Unknown(other.to_string()),
        };
        let name = match fields.get("name").and_then(Value::as_str) {
            Some(name) if is_name(name) => name.to_string(),
            Some(name) => {
                findings.push(format!(
                    "build `{id}` is named {name:?}, which is not a folder name; skipped"
                ));
                continue;
            }
            None => {
                findings.push(format!("build `{id}` has no readable `name`; skipped"));
                continue;
            }
        };
        let take = match fields.get("take") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| {
                    let module = item.as_str().map(str::to_string);
                    if module.is_none() {
                        findings.push(format!(
                            "build `{id}` takes {item}, not a content id; skipped"
                        ));
                    }
                    module
                })
                .collect(),
            Some(v) => {
                findings.push(format!(
                    "build `{id}` takes {v}, not a list; it takes nothing"
                ));
                Vec::new()
            }
        };
        outputs.push(Output {
            max_files: read_limit(fields, "max_files", &id, findings),
            max_bytes: read_limit(fields, "max_bytes", &id, findings),
            limits: read_limits(fields, &id, findings),
            split: read_flag(fields, "split", &id, "it does not split", findings),
            ship_unchanged: read_flag(
                fields,
                SHIP_UNCHANGED_KEY,
                &id,
                "it leaves out a source that has not changed from the game",
                findings,
            ),
            target: read_target(fields, &id, findings),
            extra: unread(fields, &KNOWN_OUTPUT),
            id,
            kind,
            name,
            take,
        });
    }
    outputs
}

fn entry_of<'a>(
    what: &str,
    index: usize,
    entry: &'a Value,
    findings: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    let fields = entry.as_object();
    if fields.is_none() {
        findings.push(format!("{what} {index} is {entry}, not an entry; skipped"));
    }
    fields
}

fn read_id(
    fields: &Map<String, Value>,
    what: &str,
    index: usize,
    declared: impl Fn(&str) -> bool,
    findings: &mut Vec<String>,
) -> Option<String> {
    let Some(id) = fields.get("id").and_then(Value::as_str) else {
        findings.push(format!("{what} {index} has no readable `id`; skipped"));
        return None;
    };
    if !is_name(id) {
        findings.push(format!(
            "{what} {index} has the id {id:?}, which is not a folder name; skipped"
        ));
        return None;
    }
    if declared(id) {
        findings.push(format!(
            "{what} `{id}` is declared twice; the first one holds"
        ));
        return None;
    }
    Some(id.to_string())
}

fn read_limit(
    fields: &Map<String, Value>,
    key: &str,
    id: &str,
    findings: &mut Vec<String>,
) -> Option<u64> {
    match fields.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => {
            let limit = v.as_u64();
            if limit.is_none() {
                findings.push(format!(
                    "build `{id}` has `{key}` {v}, not a count; it has no ceiling"
                ));
            }
            limit
        }
    }
}

/// A key that is true or false; one that is neither is a finding and reads as false, with `otherwise` saying what that means.
fn read_flag(
    fields: &Map<String, Value>,
    key: &str,
    id: &str,
    otherwise: &str,
    findings: &mut Vec<String>,
) -> bool {
    match fields.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(v) => {
            findings.push(format!(
                "build `{id}` has `{key}` {v}, not true or false; {otherwise}"
            ));
            false
        }
    }
}

fn read_target(
    fields: &Map<String, Value>,
    id: &str,
    findings: &mut Vec<String>,
) -> Option<Target> {
    match fields.get("target") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let target = v.as_str().and_then(Target::from_word);
            if target.is_none() {
                findings.push(format!(
                    "build `{id}` has `target` {v}, which is not `legacy` or `enhanced`; it ships each file as it is"
                ));
            }
            target
        }
    }
}

fn read_limits(
    fields: &Map<String, Value>,
    id: &str,
    findings: &mut Vec<String>,
) -> BTreeMap<String, u64> {
    let mut limits = BTreeMap::new();
    match fields.get("limits") {
        None | Some(Value::Null) => {}
        Some(Value::Object(entries)) => {
            for (name, v) in entries {
                if TOTALS.contains(&name.as_str()) {
                    findings.push(format!(
                        "build `{id}` has a limit named `{name}`, which `max_{name}` sets; skipped"
                    ));
                } else if let Some(limit) = v.as_u64() {
                    limits.insert(name.clone(), limit);
                } else {
                    findings.push(format!(
                        "build `{id}` has limit `{name}` {v}, not a count; it has no ceiling"
                    ));
                }
            }
        }
        Some(v) => findings.push(format!(
            "build `{id}` has `limits` {v}, not a map of counts; it has none"
        )),
    }
    limits
}

fn unread(fields: &Map<String, Value>, known: &[&str]) -> Map<String, Value> {
    let mut extra = fields.clone();
    extra.retain(|key, _| !known.contains(&key.as_str()));
    extra
}

/// `base` in lower case, or with `-2`, `-3` and so on after it until no id of `taken` is it.
pub fn unique_id<'a>(base: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    let id = base.to_lowercase();
    let mut candidate = id.clone();
    let mut n = 1;
    while taken.clone().any(|t| t == candidate) {
        n += 1;
        candidate = format!("{id}-{n}");
    }
    candidate
}

/// Whether `text` names a folder of a project or a build: a path component every host can create,
/// and not a hidden one, since the build keeps its own work in hidden folders beside the builds.
pub fn is_name(text: &str) -> bool {
    is_component(text) && !text.starts_with('.')
}

/// Whether `resource` is spelt as a part of a split of the build called `build`: its name, `_` and a number.
pub fn reads_as_a_part(build: &str, resource: &str) -> bool {
    let (build, resource) = (build.to_lowercase(), resource.to_lowercase());
    resource
        .strip_prefix(&build)
        .and_then(|rest| rest.strip_prefix('_'))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(top: Value) -> (Container, Vec<String>) {
        let Value::Object(top) = top else {
            panic!("a project file is an object")
        };
        let mut findings = Vec::new();
        let container = Container::read(&top, &mut findings);
        (container, findings)
    }

    fn written(container: &Container) -> Map<String, Value> {
        let mut top = Map::new();
        container.write(&mut top);
        top
    }

    fn declared() -> Container {
        let mut limited = Output::fivem_resource("police", "police_pack", &["files"]);
        limited.max_files = Some(40);
        limited.max_bytes = Some(1 << 20);
        Container {
            modules: vec![Module::files("files"), Module::files("scripts")],
            outputs: vec![limited, Output::fivem_resource("lite", "police_lite", &[])],
        }
    }

    #[test]
    fn a_build_is_added_from_all_the_content_under_a_free_id_and_a_taken_name_is_refused() {
        let mut container = Container {
            modules: vec![
                Module::files("files"),
                Module::clothing("uniforms", "uniforms"),
            ],
            outputs: vec![Output::fivem_resource("police_pack-2", "other", &["files"])],
        };

        let id = container
            .add_build(
                "police_pack",
                OutputKind::SingleplayerDlc,
                Some(Target::Enhanced),
            )
            .expect("a free name");

        assert_eq!(id, "police_pack");
        let added = container.output(&id).expect("declared");
        assert_eq!(added.take, ["files", "uniforms"]);
        assert_eq!(added.kind, OutputKind::SingleplayerDlc);
        assert_eq!(added.target, Some(Target::Enhanced));
        let again = container.add_build("Police_Pack", OutputKind::FivemResource, None);
        assert!(again.is_err_and(|why| why.contains("already writes")));
        let bad = container.add_build("a/b", OutputKind::FivemResource, None);
        assert!(bad.is_err_and(|why| why.contains("cannot name a folder")));
        assert_eq!(
            container.outputs.len(),
            2,
            "a refused build is not declared"
        );

        let clash = container.add_build("police_pack-2", OutputKind::FivemResource, None);
        assert_eq!(
            clash.unwrap(),
            "police_pack-2-2",
            "an id that another build holds is not reused"
        );
    }

    #[test]
    fn a_build_is_removed_by_its_id_and_only_that_one() {
        let mut container = declared();
        container
            .outputs
            .push(Output::fivem_resource("second", "second_pack", &["files"]));

        assert!(container.remove_build("second"));
        assert!(
            !container.remove_build("second"),
            "there was none to remove"
        );
        let left: Vec<&str> = container.outputs.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(left, ["police", "lite"]);
    }

    #[test]
    fn a_split_and_the_named_limits_round_trip_and_are_left_out_when_unset() {
        let mut output = Output::fivem_resource("police", "police_pack", &["files"]);
        output.split = true;
        output.limits.insert("metadata_entries".into(), 1000);
        let container = Container {
            modules: vec![Module::files("files")],
            outputs: vec![output],
        };

        let top = written(&container);
        assert_eq!(top[OUTPUTS_KEY][0]["split"], json!(true));
        assert_eq!(
            top[OUTPUTS_KEY][0]["limits"],
            json!({ "metadata_entries": 1000 })
        );
        let (read_back, findings) = read(Value::Object(top));
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(read_back, container);

        let plain = written(&declared());
        assert!(plain[OUTPUTS_KEY][0].get("split").is_none());
        assert!(plain[OUTPUTS_KEY][0].get("limits").is_none());
    }

    #[test]
    fn a_kind_and_a_target_round_trip_and_a_target_is_left_out_when_unset() {
        let mut dlc = Output::of(OutputKind::SingleplayerDlc, "dlc", "pack_dlc", &["files"]);
        dlc.target = Some(Target::Enhanced);
        let oiv = Output::of(OutputKind::OivPackage, "oiv", "pack_oiv", &["files"]);
        let container = Container {
            modules: vec![Module::files("files")],
            outputs: vec![dlc, oiv],
        };

        let top = written(&container);
        assert_eq!(top[OUTPUTS_KEY][0]["kind"], json!("singleplayer-dlc"));
        assert_eq!(top[OUTPUTS_KEY][0]["target"], json!("enhanced"));
        assert_eq!(top[OUTPUTS_KEY][1]["kind"], json!("oiv-package"));
        assert!(top[OUTPUTS_KEY][1].get("target").is_none());
        let (read_back, findings) = read(Value::Object(top));
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(read_back, container);
        assert_eq!(Target::Legacy.generation(), Generation::Gen8);
        assert_eq!(Target::Enhanced.generation(), Generation::Gen9);
    }

    #[test]
    fn an_unreadable_target_costs_only_itself() {
        let (container, findings) = read(json!({
            "outputs": [
                { "id": "a", "kind": "fivem-resource", "name": "pack", "target": "gen9" },
                { "id": "b", "kind": "fivem-resource", "name": "other", "target": 9 },
            ]
        }));

        assert_eq!(container.outputs.len(), 2);
        assert!(container.outputs.iter().all(|o| o.target.is_none()));
        assert_eq!(findings.len(), 2, "{findings:#?}");
    }

    #[test]
    fn an_unreadable_split_or_limit_costs_only_itself() {
        let (container, findings) = read(json!({
            "outputs": [
                { "id": "a", "kind": "fivem-resource", "name": "pack", "split": "yes",
                  "limits": { "entries": "many", "files": 3, "slots": 255 } },
                { "id": "b", "kind": "fivem-resource", "name": "other", "limits": [1] },
            ]
        }));

        let a = &container.outputs[0];
        assert!(!a.split);
        assert_eq!(a.limits, BTreeMap::from([("slots".to_string(), 255)]));
        assert!(container.outputs[1].limits.is_empty());
        // Not a flag, not a count, a name that `max_files` owns, and a list in place of a map.
        assert_eq!(findings.len(), 4, "{findings:#?}");
    }

    #[test]
    fn a_container_round_trips_through_its_keys() {
        let container = declared();
        let top = written(&container);
        assert_eq!(
            top[MODULES_KEY][0],
            json!({ "id": "files", "type": "files" })
        );
        assert_eq!(
            top[OUTPUTS_KEY][0],
            json!({
                "id": "police", "kind": "fivem-resource", "name": "police_pack",
                "take": ["files"], "max_files": 40, "max_bytes": 1 << 20
            })
        );
        let (read_back, findings) = read(Value::Object(top));
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(read_back, container);
    }

    #[test]
    fn a_file_with_no_modules_key_holds_its_documents_as_one_files_module() {
        let (container, findings) = read(json!({ "schema": 3, "documents": [] }));
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(container.modules, vec![Module::files("files")]);
        assert!(container.outputs.is_empty());

        // Saving declares it, so the next read does not depend on the schema number.
        let top = written(&container);
        assert_eq!(
            top[MODULES_KEY],
            json!([{ "id": "files", "type": "files" }])
        );
        assert_eq!(top[OUTPUTS_KEY], json!([]));
    }

    #[test]
    fn a_declared_empty_list_stays_empty() {
        let (container, findings) = read(json!({ "modules": [], "outputs": [] }));
        assert!(findings.is_empty(), "{findings:?}");
        assert!(container.modules.is_empty());
    }

    #[test]
    fn what_this_build_does_not_know_is_carried() {
        let (container, findings) = read(json!({
            "modules": [
                { "id": "uniforms", "type": "widgets", "path": "modules/uniforms", "link": true },
                { "id": "files", "type": "files", "colour": "red" }
            ],
            "outputs": [
                { "id": "dlc", "kind": "gamemode-patch", "name": "police_dlc", "take": ["uniforms"] },
                { "id": "police", "kind": "fivem-resource", "name": "police_pack",
                  "take": ["files"], "split_at": 100 }
            ]
        }));
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(
            container.modules[0].kind,
            ModuleKind::Unknown("widgets".into())
        );
        assert_eq!(
            container.outputs[0].kind,
            OutputKind::Unknown("gamemode-patch".into())
        );

        let top = written(&container);
        assert_eq!(top[MODULES_KEY][0]["path"], json!("modules/uniforms"));
        assert_eq!(top[MODULES_KEY][0]["link"], json!(true));
        assert_eq!(top[MODULES_KEY][0]["type"], json!("widgets"));
        assert_eq!(top[MODULES_KEY][1]["colour"], json!("red"));
        assert_eq!(top[OUTPUTS_KEY][0]["kind"], json!("gamemode-patch"));
        assert_eq!(top[OUTPUTS_KEY][1]["split_at"], json!(100));
    }

    #[test]
    fn one_unreadable_entry_costs_only_itself() {
        let (container, findings) = read(json!({
            "modules": [
                { "id": "files", "type": "files" },
                7,
                { "type": "files" },
                { "id": "files", "type": "files" },
                { "id": "../up", "type": "files" },
                { "id": "scripts" },
            ],
            "outputs": [
                { "id": "a", "kind": "fivem-resource", "name": "pack", "take": ["files", 3],
                  "max_files": "many", "max_bytes": -1 },
                { "id": "b", "kind": "fivem-resource" },
                { "id": "c", "kind": "fivem-resource", "name": "bad/name" },
                { "id": "d", "name": "pack" },
                { "id": "a", "kind": "fivem-resource", "name": "again" },
            ]
        }));
        let ids: Vec<&str> = container.modules.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["files"], "{container:?}");
        assert_eq!(container.outputs.len(), 1);
        let a = &container.outputs[0];
        assert_eq!((a.id.as_str(), a.name.as_str()), ("a", "pack"));
        assert_eq!(a.take, ["files"]);
        assert_eq!((a.max_files, a.max_bytes), (None, None));
        // Five unreadable modules (not an entry, no id, a repeat, a bad id, no type), then on
        // the first output one take entry and two limits, and four outputs that are unreadable.
        assert_eq!(findings.len(), 5 + 1 + 2 + 4, "{findings:#?}");
    }

    #[test]
    fn a_key_that_is_not_a_list_costs_the_list_and_a_finding() {
        let (container, findings) = read(json!({ "modules": "files", "outputs": 3 }));
        assert!(container.modules.is_empty());
        assert!(container.outputs.is_empty());
        assert_eq!(findings.len(), 2, "{findings:?}");
    }

    #[test]
    fn a_second_build_is_never_named_so_that_it_reads_as_a_part_of_a_split() {
        let mut container = Container::default();
        container
            .add_build("police", OutputKind::FivemResource, None)
            .unwrap();
        let fresh = |container: &Container, kind, target| {
            container.free_build_name("police", &kind, target)
        };

        // Named for what it is first, and numbered only when that is taken too.
        assert_eq!(
            fresh(&container, OutputKind::FivemResource, None),
            "police_v2"
        );
        assert_eq!(
            fresh(&container, OutputKind::SingleplayerDlc, None),
            "police_dlc"
        );
        assert_eq!(
            fresh(
                &container,
                OutputKind::FivemResource,
                Some(Target::Enhanced)
            ),
            "police_enhanced"
        );
        assert_eq!(
            fresh(&container, OutputKind::OivPackage, Some(Target::Legacy)),
            "police_oiv"
        );
        for _ in 0..5 {
            let name = fresh(&container, OutputKind::FivemResource, None);
            for build in &container.outputs {
                assert!(
                    !reads_as_a_part(&build.name, &name) && !build.name.eq_ignore_ascii_case(&name),
                    "`{name}` is taken or reads as a part of `{}`",
                    build.name
                );
            }
            container
                .add_build(&name, OutputKind::FivemResource, None)
                .unwrap();
        }
        // The first build of an empty project is the project's own name.
        let empty = Container::default();
        assert_eq!(
            empty.free_build_name("police", &OutputKind::FivemResource, None),
            "police"
        );
    }

    #[test]
    fn a_name_that_reads_as_a_part_of_an_existing_build_is_not_offered() {
        let mut container = Container::default();
        container
            .add_build("city", OutputKind::FivemResource, None)
            .unwrap();
        // A project called `city_2` is offered its own name first, and a split of `city` writes `city_2`.
        assert_eq!(
            container.free_build_name("city_2", &OutputKind::FivemResource, None),
            "city_2_v2"
        );
    }

    #[test]
    fn a_part_of_a_split_is_a_name_a_number_and_an_underscore() {
        assert!(reads_as_a_part("pack", "pack_2"));
        assert!(reads_as_a_part("Pack", "pack_12"));
        for not in ["pack", "pack_", "pack_v2", "pack2", "other_2", "pack_2_b"] {
            assert!(!reads_as_a_part("pack", not), "{not}");
        }
    }

    #[test]
    fn a_build_is_edited_and_a_change_that_is_refused_changes_nothing() {
        let mut container = Container {
            modules: vec![Module::files("files"), Module::clothing("hats", "hats")],
            outputs: vec![
                Output::fivem_resource("a", "pack_a", &["files", "hats"]),
                Output::fivem_resource("b", "pack_b", &["files"]),
            ],
        };

        container
            .edit_build("a", |o| {
                o.name = "renamed".into();
                o.kind = OutputKind::SingleplayerDlc;
                o.split = true;
                o.max_files = Some(3);
                o.limits.insert("clothing_drawables".into(), 128);
                o.id = "other".into();
            })
            .expect("a good change");
        let a = container.output("a").expect("the id is kept");
        assert_eq!(a.name, "renamed");
        assert_eq!(
            (a.kind.clone(), a.split, a.max_files),
            (OutputKind::SingleplayerDlc, true, Some(3))
        );

        let before = container.clone();
        let refusals = [
            (
                "a name another build writes",
                container.edit_build("a", |o| o.name = "PACK_B".into()),
            ),
            (
                "a name no folder can have",
                container.edit_build("a", |o| o.name = "a/b".into()),
            ),
            (
                "content the project lacks",
                container.edit_build("a", |o| o.take.push("ghost".into())),
            ),
            (
                "content taken twice",
                container.edit_build("a", |o| o.take.push("hats".into())),
            ),
            (
                "a total named as a figure",
                container.edit_build("a", |o| {
                    o.limits.insert("files".into(), 1);
                }),
            ),
            (
                "a build that is not there",
                container.edit_build("nope", |_| {}),
            ),
        ];
        for (what, refused) in refusals {
            assert!(refused.is_err(), "{what} was accepted");
        }
        assert_eq!(container, before, "a refusal changes nothing");
    }

    #[test]
    fn a_content_is_taken_and_left_out_keeping_the_order_of_the_content() {
        let mut container = Container {
            modules: ["a", "b", "c"].map(Module::files).to_vec(),
            outputs: vec![Output::fivem_resource("x", "pack", &["a", "c"])],
        };

        container.set_taken("x", "b", true).unwrap();
        assert_eq!(container.output("x").unwrap().take, ["a", "b", "c"]);
        container.set_taken("x", "a", false).unwrap();
        container.set_taken("x", "a", false).unwrap();
        assert_eq!(container.output("x").unwrap().take, ["b", "c"]);

        // A build whose order was chosen keeps it: a split cuts in the order of `take`.
        container.outputs[0].take = vec!["c".into(), "b".into()];
        container.set_taken("x", "a", true).unwrap();
        assert_eq!(container.output("x").unwrap().take, ["c", "b", "a"]);

        assert!(container.set_taken("x", "ghost", true).is_err());
        assert!(container.set_taken("nope", "a", true).is_err());
    }

    #[test]
    fn a_file_type_is_stated_taken_away_and_carried_in_the_file() {
        let mut files = Module::files("files");
        files
            .set_file_type("data/handling.meta", Some("HANDLING_FILE"))
            .unwrap();
        files
            .set_file_type("data/vehicles.meta", Some("VEHICLE_METADATA_FILE"))
            .unwrap();
        assert_eq!(
            files
                .file_types()
                .get("data/handling.meta")
                .map(String::as_str),
            Some("HANDLING_FILE")
        );

        let container = Container {
            modules: vec![files.clone()],
            outputs: Vec::new(),
        };
        let top = written(&container);
        assert_eq!(
            top[MODULES_KEY][0][FILE_TYPES_KEY]["data/vehicles.meta"],
            json!("VEHICLE_METADATA_FILE")
        );
        let (read_back, findings) = read(Value::Object(top));
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(read_back.modules[0].file_types(), files.file_types());

        files.set_file_type("data/handling.meta", None).unwrap();
        files.set_file_type("data/vehicles.meta", None).unwrap();
        assert!(
            files.extra.get(FILE_TYPES_KEY).is_none(),
            "the last statement taken away takes the key with it"
        );

        for (what, path, ty) in [
            ("a type in lower case", "a.meta", "handling"),
            ("an empty type", "a.meta", ""),
            (
                "a path that leaves the project",
                "../a.meta",
                "HANDLING_FILE",
            ),
            (
                "a path with a reserved character",
                "a:b.meta",
                "HANDLING_FILE",
            ),
        ] {
            assert!(files.set_file_type(path, Some(ty)).is_err(), "{what}");
        }
        assert!(
            files.extra.get(FILE_TYPES_KEY).is_none(),
            "a refusal states nothing"
        );

        let mut pack = Module::clothing("hats", "hats");
        assert!(pack.set_file_type("a.meta", Some("HANDLING_FILE")).is_err());
        let mut link = Module::linked("brand", ModuleKind::Files, "../brand");
        assert!(link.set_file_type("a.meta", Some("HANDLING_FILE")).is_err());
    }

    #[test]
    fn an_id_is_a_folder_name() {
        for good in ["files", "police_pack", "a-b", "v1.2", "Resource Name"] {
            assert!(is_name(good), "{good}");
        }
        for bad in [
            "",
            ".",
            "..",
            ".hidden",
            "a/b",
            "a\\b",
            "a:b",
            "a*",
            "trailing.",
            "trailing ",
            "tab\t",
            "nul\0",
        ] {
            assert!(!is_name(bad), "{bad:?}");
        }
    }
}
