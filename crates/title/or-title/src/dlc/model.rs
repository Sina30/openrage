//! What the three DLC documents say, as types an author can build.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DlcList {
    pub paths: Vec<ListItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetupContentType {
    #[default]
    Unknown,
    TitleUpdate,
    LevelPack,
    CompatPack,
    PaidPack,
}

impl SetupContentType {
    pub(crate) fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "EXTRACONTENT_TU_PACK" | "EXTRACONTENT_TITLE_UPDATE_PACK" => Self::TitleUpdate,
            "EXTRACONTENT_LEVEL_PACK" => Self::LevelPack,
            "EXTRACONTENT_COMPAT_PACK" => Self::CompatPack,
            "EXTRACONTENT_PAID_PACK" | "EXTRACONTENT_EPISODIC_PACK" => Self::PaidPack,
            _ => Self::Unknown,
        }
    }

    /// The spelling this writer emits when the document carried none of its own.
    ///
    /// ⚠ **The mapping is not injective, which is why `SetupData` also carries
    /// the file's own spelling.** Two names reach `TitleUpdate` and two reach
    /// `PaidPack`, so a writer working from the variant alone renames half the
    /// packs that use the other spelling.
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::TitleUpdate => "EXTRACONTENT_TU_PACK",
            Self::LevelPack => "EXTRACONTENT_LEVEL_PACK",
            Self::CompatPack => "EXTRACONTENT_COMPAT_PACK",
            Self::PaidPack => "EXTRACONTENT_PAID_PACK",
            Self::Unknown => "EXTRACONTENT_COMPAT_PACK",
        }
    }

    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::TitleUpdate => 0,
            Self::LevelPack => 1,
            Self::CompatPack => 2,
            Self::PaidPack => 3,
            Self::Unknown => 4,
        }
    }
}

/// One `<Item>` in one of these documents' many lists.
///
/// ⚠ **Any `<Item>` may carry a `platform`**, and the ones that do name
/// consoles — `ps5|xbsx`, `ps4|xboxone`. A `Vec<String>` reads them all as
/// unscoped, so a pack edited on PC and written back enables its console-only
/// files everywhere. Measured over two retail installs: 256 items across
/// `dlclist.xml`, `filesToEnable` and `mapChangeSetData`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ListItem {
    pub value: String,
    pub platform: String,
}

impl<S: Into<String>> From<S> for ListItem {
    fn from(value: S) -> Self {
        Self {
            value: value.into(),
            platform: String::new(),
        }
    }
}

impl std::ops::Deref for ListItem {
    type Target = str;

    fn deref(&self) -> &str {
        &self.value
    }
}

impl AsRef<str> for ListItem {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Display for ListItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.value)
    }
}

impl PartialEq<&str> for ListItem {
    fn eq(&self, other: &&str) -> bool {
        self.value == *other
    }
}

/// A named set of change sets the game applies together, `GROUP_STARTUP` and
/// `GROUP_MAP` being the two every shipped pack uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentChangeSetGroup {
    pub name_hash: String,
    pub content_change_sets: Vec<ListItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupData {
    pub name_hash: String,
    pub device_name: String,
    pub dat_file: String,
    pub content_type: SetupContentType,
    /// The `<type>` spelling the document carried, empty when authored here.
    pub content_type_name: String,
    pub order: i32,
    pub minor_order: i32,
    pub is_level_pack: bool,
    pub sub_pack_count: u32,
    /// ⚠ **`None` is "the document states no version", which is what a shipped
    /// `<requiredVersion />` says** — an element present and carrying nothing.
    /// Reading it as `0` and writing `value="0"` back puts a number in a file
    /// that did not have one.
    pub required_version: Option<i32>,
    pub time_stamp: String,
    pub content_change_sets: Vec<ListItem>,
    pub content_change_set_groups: Vec<ContentChangeSetGroup>,
    pub startup_script: String,
    pub script_callstack_size: i32,
    pub dependency_pack_hash: String,
    /// GTA VI's `platformDataUnderRoot`; `None` when the document states none, as every GTA V one does.
    pub platform_data_under_root: Option<bool>,
}

impl SetupData {
    pub fn mount_cmp(&self, other: &Self) -> std::cmp::Ordering {
        let (a, b) = (self.content_type.rank(), other.content_type.rank());
        if a != b {
            return a.cmp(&b);
        }
        if self.is_level_pack && other.is_level_pack {
            return other
                .order
                .cmp(&self.order)
                .then_with(|| other.minor_order.cmp(&self.minor_order));
        }
        self.order
            .cmp(&other.order)
            .then_with(|| self.name_hash.cmp(&other.name_hash))
    }

    pub fn content_device(&self) -> String {
        if self.minor_order <= 0 {
            format!("{}CRC", self.device_name)
        } else {
            self.device_name.clone()
        }
    }

    pub fn type_name(&self) -> &str {
        if self.content_type_name.is_empty() {
            self.content_type.canonical_name()
        } else {
            &self.content_type_name
        }
    }
}

/// ⚠ **`Option<bool>` is "did the document say", not "is it true".** Most
/// entries state three flags and omit the rest, so a writer that turns every
/// absent flag into `value="false"` grows the file by a third and states
/// things the author never did. The three the mount acts on stay plain
/// `bool`, because a pack author has to see them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataFileEntry {
    pub filename: String,
    pub file_type: String,
    pub overlay: bool,
    pub disabled: bool,
    pub persistent: bool,
    pub locked: Option<bool>,
    pub load_completely: Option<bool>,
    pub patch_file: Option<bool>,
    /// `CONTENTS_PROPS`, `CONTENTS_DLC_MAP_DATA` and friends; absent on most
    /// entries and load-bearing on the ones that carry it.
    pub contents: String,
    /// The `platform` attribute, spelled `ps4|xboxone` where it appears.
    pub platform: String,
}

/// One map's slice of a change set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapChangeSet {
    pub associated_map: String,
    pub files_to_invalidate: Vec<ListItem>,
    pub files_to_disable: Vec<ListItem>,
    pub files_to_enable: Vec<ListItem>,
    pub txd_to_load: Vec<ListItem>,
    pub txd_to_unload: Vec<ListItem>,
    pub resident_resources: Vec<ListItem>,
    pub unregister_resources: Vec<ListItem>,
}

/// ⚠ **Not a list.** `executionConditions` holds two condition *expressions*
/// — `$level=MO_JIM_L11` is the shipped spelling — rather than the `<Item>`s
/// every section beside it uses, so a reader that treats it like its
/// neighbours reads nothing and reports no error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionConditions {
    pub active_changeset_conditions: String,
    pub generic_conditions: String,
}

/// ⚠ **This is what turns a pack's files on.** Every `dataFiles` entry in a
/// shipped pack is `disabled="true"`, and a change set naming it in
/// `filesToEnable` is the only thing that enables it — so a generated pack
/// with data files and no change set is a pack that mounts and does nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentChangeSet {
    pub name: String,
    /// The `platform` attribute on the change set's own `<Item>`.
    pub platform: String,
    pub map_change_set_data: Vec<MapChangeSet>,
    pub files_to_invalidate: Vec<ListItem>,
    pub files_to_disable: Vec<ListItem>,
    pub files_to_enable: Vec<ListItem>,
    pub txd_to_load: Vec<ListItem>,
    pub txd_to_unload: Vec<ListItem>,
    pub resident_resources: Vec<ListItem>,
    pub unregister_resources: Vec<ListItem>,
    pub execution_conditions: Option<ExecutionConditions>,
    pub requires_loading_screen: Option<bool>,
    pub loading_screen_context: String,
    pub use_cache_loader: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentXml {
    pub data_files: Vec<DataFileEntry>,
    pub disabled_files: Vec<ListItem>,
    pub included_data_files: Vec<ListItem>,
    pub included_xml_files: Vec<String>,
    pub content_change_sets: Vec<ContentChangeSet>,
    pub patch_files: Vec<ListItem>,
}

impl ContentXml {
    /// Every file any change set enables, map-scoped ones included.
    ///
    /// ⚠ **Derived rather than stored**: a field filled beside the change sets
    /// it flattens was built and removed, because it is two models of one fact.
    pub fn files_to_enable(&self) -> Vec<ListItem> {
        let mut out = Vec::new();
        for cs in &self.content_change_sets {
            out.extend(cs.files_to_enable.iter().cloned());
            for map in &cs.map_change_set_data {
                out.extend(map.files_to_enable.iter().cloned());
            }
        }
        out
    }
}

/// One `fileList` item of a GTA VI change set, each field as the document spells it and empty when it states none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredFile {
    pub filename: String,
    /// ⚠ A name, not an enum: the names the readable documents use are not the whole set.
    pub file_type: String,
    /// The flag names `flags` states, split at white space as a RAGE document spells a set.
    pub flags: Vec<String>,
    pub contents: String,
}

/// The path macros GTA VI's documents write, which no document defines.
const PATH_MACROS: [&str; 3] = ["$(root)", "$(target)", "$(common)"];

/// A GTA VI change set, a `CDataFileMgr__ContentChangeSetDef` document or a content definition's `contentSets` item, its paths unexpanded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentChangeSetDef {
    pub id: String,
    pub registration_group: String,
    pub associated_map: String,
    pub file_list: Vec<RegisteredFile>,
    pub flags: Vec<String>,
    pub changeset_include_flags: Vec<String>,
    /// The stated value, `None` when the element is absent; every measured one is `0`.
    pub map_add_at_phase: Option<String>,
    pub map_remove_at_phase: Option<String>,
    pub execution_conditions: Option<ExecutionConditions>,
    /// Paths of the elements and attributes the reader does not know, each once, such as `fileList/Item/foo`.
    pub unread: Vec<String>,
}

impl ContentChangeSetDef {
    /// How many file names start with `$(root)`, `$(target)` or `$(common)`, left unexpanded because no document states what they stand for.
    pub fn unexpanded(&self) -> Vec<(&'static str, usize)> {
        PATH_MACROS
            .iter()
            .map(|m| {
                let n = self
                    .file_list
                    .iter()
                    .filter(|f| f.filename.starts_with(m))
                    .count();
                (*m, n)
            })
            .filter(|&(_, n)| n > 0)
            .collect()
    }
}

/// One `includedChangesets` item of a GTA VI content definition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncludedChangeset {
    pub load_path: String,
    pub changeset_include_flags: Vec<String>,
    pub platform: String,
    /// A second platform selector GTA V does not have, `original` or `trinity` beside `ps5`.
    pub subplatform: String,
}

/// A GTA VI content definition, root `CDataFileMgr__ContentDef`: what a setup's `datFile` names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentDef {
    pub content_sets: Vec<ContentChangeSetDef>,
    pub included_sets: Vec<ListItem>,
    pub included_changesets: Vec<IncludedChangeset>,
    /// Paths the reader does not know outside `contentSets`, whose sets report their own.
    pub unread: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleUpdateMount {
    pub device_name: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraTitleUpdateData {
    pub mounts: Vec<TitleUpdateMount>,
}

impl ExtraTitleUpdateData {
    pub fn mount_for(&self, device_name: &str) -> Option<&str> {
        let needle = device_name.trim_end_matches('/').trim_end_matches(':');
        self.mounts
            .iter()
            .find(|m| m.device_name.eq_ignore_ascii_case(needle))
            .map(|m| m.path.as_str())
    }
}
