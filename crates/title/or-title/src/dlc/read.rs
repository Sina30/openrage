//! Reading the three DLC documents, plus the title-update mount table.

use super::model::*;
use super::tree::{parse_rooted, unread, Node};
use super::xml::Result;

impl DlcList {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = parse_rooted(bytes, "dlclist.xml", &["SMandatoryPacksData"])?;
        Ok(Self {
            paths: root.item_texts("Paths"),
        })
    }
}

impl SetupData {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = parse_rooted(bytes, "setup2.xml", &["SSetupData"])?;
        let type_name = root.text_of("type");
        Ok(Self {
            name_hash: root.text_of("nameHash"),
            device_name: root.text_of("deviceName"),
            dat_file: root.text_of("datFile"),
            content_type: SetupContentType::from_str(&type_name),
            content_type_name: type_name,
            order: root.int("order").unwrap_or(0) as i32,
            minor_order: root.int("minorOrder").unwrap_or(0) as i32,
            is_level_pack: root.flag("isLevelPack").unwrap_or(false),
            sub_pack_count: root.int("subPackCount").unwrap_or(0).max(0) as u32,
            required_version: root
                .int("requiredVersion")
                .map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
            time_stamp: root.text_of("timeStamp"),
            content_change_sets: root.item_texts("contentChangeSets"),
            content_change_set_groups: root
                .items("contentChangeSetGroups")
                .map(|item| ContentChangeSetGroup {
                    name_hash: item.text_of("NameHash"),
                    content_change_sets: item.item_texts("ContentChangeSets"),
                })
                .collect(),
            startup_script: root.text_of("startupScript"),
            script_callstack_size: root.int("scriptCallstackSize").unwrap_or(0) as i32,
            dependency_pack_hash: root.text_of("dependencyPackHash"),
            platform_data_under_root: root.flag("platformDataUnderRoot"),
        })
    }
}

impl ContentXml {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = parse_rooted(
            bytes,
            "content.xml",
            &["CDataFileMgr__ContentsOfDataFileXml", "dataFiles"],
        )?;
        // A `content.xml` is sometimes the `<dataFiles>` element itself rather
        // than a document wrapping one, which is how the base game registers
        // its own platform archives.
        let bare = root.name.eq_ignore_ascii_case("dataFiles");

        let data_files = if bare {
            root.children_named("Item").map(data_file).collect()
        } else {
            root.items("dataFiles").map(data_file).collect()
        };

        Ok(Self {
            data_files,
            disabled_files: root.item_texts("disabledFiles"),
            included_data_files: root.item_texts("includedDataFiles"),
            included_xml_files: included_xml_hrefs(&root),
            content_change_sets: root.items("contentChangeSets").map(change_set).collect(),
            patch_files: root.item_texts("patchFiles"),
        })
    }
}

fn data_file(item: &Node) -> DataFileEntry {
    let file_type = item.text_of("fileType");
    DataFileEntry {
        filename: item.text_of("filename"),
        file_type: if file_type.is_empty() {
            "RPF_FILE".to_string()
        } else {
            file_type
        },
        overlay: item.flag("overlay").unwrap_or(false),
        disabled: item.flag("disabled").unwrap_or(false),
        persistent: item.flag("persistent").unwrap_or(false),
        locked: item.flag("locked"),
        load_completely: item.flag("loadCompletely"),
        patch_file: item.flag("patchFile"),
        contents: item.text_of("contents"),
        platform: item.attr("platform").unwrap_or_default().to_string(),
    }
}

fn change_set(item: &Node) -> ContentChangeSet {
    ContentChangeSet {
        name: item.text_of("changeSetName"),
        platform: item.attr("platform").unwrap_or_default().to_string(),
        map_change_set_data: item
            .items("mapChangeSetData")
            .map(|m| MapChangeSet {
                associated_map: m.text_of("associatedMap"),
                files_to_invalidate: m.item_texts("filesToInvalidate"),
                files_to_disable: m.item_texts("filesToDisable"),
                files_to_enable: m.item_texts("filesToEnable"),
                txd_to_load: m.item_texts("txdToLoad"),
                txd_to_unload: m.item_texts("txdToUnload"),
                resident_resources: m.item_texts("residentResources"),
                unregister_resources: m.item_texts("unregisterResources"),
            })
            .collect(),
        files_to_invalidate: item.item_texts("filesToInvalidate"),
        files_to_disable: item.item_texts("filesToDisable"),
        files_to_enable: item.item_texts("filesToEnable"),
        txd_to_load: item.item_texts("txdToLoad"),
        txd_to_unload: item.item_texts("txdToUnload"),
        resident_resources: item.item_texts("residentResources"),
        unregister_resources: item.item_texts("unregisterResources"),
        execution_conditions: item
            .child("executionConditions")
            .map(|e| ExecutionConditions {
                active_changeset_conditions: e.text_of("activeChangesetConditions"),
                generic_conditions: e.text_of("genericConditions"),
            }),
        requires_loading_screen: item.flag("requiresLoadingScreen"),
        loading_screen_context: item.text_of("loadingScreenContext"),
        use_cache_loader: item.flag("useCacheLoader"),
    }
}

/// `<xi:include href="…">` wrapped in an `<Item>`, which is the one place
/// these documents carry a payload in an attribute rather than in text.
fn included_xml_hrefs(root: &Node) -> Vec<String> {
    let Some(section) = root.child("includedXmlFiles") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_hrefs(section, &mut out);
    out
}

fn collect_hrefs(node: &Node, out: &mut Vec<String>) {
    for child in &node.children {
        if child.name.eq_ignore_ascii_case("include") {
            if let Some(href) = child.attr("href") {
                let href = href.trim();
                if !href.is_empty() {
                    out.push(href.to_string());
                }
            }
        }
        collect_hrefs(child, out);
    }
}

const CHANGE_SET_FIELDS: &[&str] = &[
    "id",
    "registrationGroup",
    "associatedMap",
    "fileList",
    "fileList/Item",
    "fileList/Item/filename",
    "fileList/Item/fileType",
    "fileList/Item/flags",
    "fileList/Item/contents",
    "flags",
    "changesetIncludeFlags",
    "mapAddAtPhase",
    "mapRemoveAtPhase",
    "executionConditions",
    "executionConditions/activeChangesetConditions",
    "executionConditions/genericConditions",
];

const CONTENT_DEF_FIELDS: &[&str] = &[
    "contentSets",
    "contentSets/Item",
    "includedSets",
    "includedSets/Item",
    "includedSets/Item@platform",
    "includedChangesets",
    "includedChangesets/Item",
    "includedChangesets/Item@platform",
    "includedChangesets/Item@subplatform",
    "includedChangesets/Item/loadPath",
    "includedChangesets/Item/changesetIncludeFlags",
];

impl ContentChangeSetDef {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = parse_rooted(
            bytes,
            "GTA VI change set",
            &["CDataFileMgr__ContentChangeSetDef"],
        )?;
        Ok(change_set_def(&root))
    }
}

impl ContentDef {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = parse_rooted(
            bytes,
            "GTA VI content definition",
            &["CDataFileMgr__ContentDef"],
        )?;
        Ok(Self {
            content_sets: root.items("contentSets").map(change_set_def).collect(),
            included_sets: root.item_texts("includedSets"),
            included_changesets: root
                .items("includedChangesets")
                .map(|item| IncludedChangeset {
                    load_path: leaf(item, "loadPath"),
                    changeset_include_flags: flag_names(item, "changesetIncludeFlags"),
                    platform: item.attr("platform").unwrap_or_default().to_string(),
                    subplatform: item.attr("subplatform").unwrap_or_default().to_string(),
                })
                .collect(),
            unread: unread(&root, CONTENT_DEF_FIELDS, &["contentSets/Item"]),
        })
    }
}

fn leaf(node: &Node, name: &str) -> String {
    node.scalar(name).unwrap_or_default()
}

/// The names a flags element states, which white space separates.
fn flag_names(node: &Node, name: &str) -> Vec<String> {
    leaf(node, name)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn change_set_def(node: &Node) -> ContentChangeSetDef {
    ContentChangeSetDef {
        id: leaf(node, "id"),
        registration_group: leaf(node, "registrationGroup"),
        associated_map: leaf(node, "associatedMap"),
        file_list: node
            .items("fileList")
            .map(|item| RegisteredFile {
                filename: leaf(item, "filename"),
                file_type: leaf(item, "fileType"),
                flags: flag_names(item, "flags"),
                contents: leaf(item, "contents"),
            })
            .collect(),
        flags: flag_names(node, "flags"),
        changeset_include_flags: flag_names(node, "changesetIncludeFlags"),
        map_add_at_phase: node.scalar("mapAddAtPhase"),
        map_remove_at_phase: node.scalar("mapRemoveAtPhase"),
        execution_conditions: node
            .child("executionConditions")
            .map(|e| ExecutionConditions {
                active_changeset_conditions: leaf(e, "activeChangesetConditions"),
                generic_conditions: leaf(e, "genericConditions"),
            }),
        unread: unread(node, CHANGE_SET_FIELDS, &[]),
    }
}

impl ExtraTitleUpdateData {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let root = parse_rooted(
            bytes,
            "ExtraTitleUpdateData.meta",
            &["SExtraTitleUpdateData"],
        )?;
        let mounts = root
            .child("Mounts")
            .into_iter()
            .flat_map(|n| n.children_named("Item"))
            .map(|item| TitleUpdateMount {
                device_name: item
                    .text_of("deviceName")
                    .trim_end_matches('/')
                    .trim_end_matches(':')
                    .to_string(),
                path: item.text_of("path"),
            })
            .filter(|m| !m.device_name.is_empty() && !m.path.is_empty())
            .collect();
        Ok(Self { mounts })
    }
}
