//! Writing the DLC documents back out.

use super::model::*;
use super::xml::XmlOut;

impl DlcList {
    pub fn to_xml(&self) -> String {
        let mut out = XmlOut::document("SMandatoryPacksData");
        out.item_list("Paths", &self.paths);
        out.finish("SMandatoryPacksData")
    }
}

impl SetupData {
    /// ⚠ **Element order follows what ships**, because the first thing a pack
    /// author does with a generated `setup2.xml` is diff it against one that
    /// works. The game's parser is name-driven and does not care.
    pub fn to_xml(&self) -> String {
        let mut out = XmlOut::document("SSetupData");
        out.text("deviceName", &self.device_name);
        out.text("datFile", &self.dat_file);
        out.text("timeStamp", &self.time_stamp);
        out.text("nameHash", &self.name_hash);
        out.item_list("contentChangeSets", &self.content_change_sets);

        if self.content_change_set_groups.is_empty() {
            out.empty("contentChangeSetGroups");
        } else {
            out.open("contentChangeSetGroups");
            for group in &self.content_change_set_groups {
                out.open("Item");
                out.text("NameHash", &group.name_hash);
                out.item_list("ContentChangeSets", &group.content_change_sets);
                out.close("Item");
            }
            out.close("contentChangeSetGroups");
        }

        out.text("startupScript", &self.startup_script);
        out.int_value("scriptCallstackSize", self.script_callstack_size as i64);
        out.text("type", self.type_name());
        out.int_value("order", self.order as i64);
        out.int_value("minorOrder", self.minor_order as i64);
        out.bool_value("isLevelPack", self.is_level_pack);
        out.text("dependencyPackHash", &self.dependency_pack_hash);
        match self.required_version {
            Some(v) => out.int_value("requiredVersion", v as i64),
            None => out.empty("requiredVersion"),
        }
        out.int_value("subPackCount", self.sub_pack_count as i64);
        out.optional_bool("platformDataUnderRoot", self.platform_data_under_root);
        out.finish("SSetupData")
    }
}

impl ContentXml {
    pub fn to_xml(&self) -> String {
        let mut out = XmlOut::document("CDataFileMgr__ContentsOfDataFileXml");
        out.item_list("disabledFiles", &self.disabled_files);

        if self.included_xml_files.is_empty() {
            out.empty("includedXmlFiles");
        } else {
            out.open("includedXmlFiles");
            for href in &self.included_xml_files {
                out.open("Item");
                out.include(href);
                out.close("Item");
            }
            out.close("includedXmlFiles");
        }

        out.item_list("includedDataFiles", &self.included_data_files);

        if self.data_files.is_empty() {
            out.empty("dataFiles");
        } else {
            out.open("dataFiles");
            for df in &self.data_files {
                if df.platform.is_empty() {
                    out.open("Item");
                } else {
                    out.open_attr("Item", "platform", &df.platform);
                }
                out.text("filename", &df.filename);
                out.text("fileType", &df.file_type);
                out.bool_value("overlay", df.overlay);
                out.bool_value("disabled", df.disabled);
                out.bool_value("persistent", df.persistent);
                out.optional_bool("locked", df.locked);
                out.optional_bool("loadCompletely", df.load_completely);
                out.optional_bool("patchFile", df.patch_file);
                if !df.contents.is_empty() {
                    out.text("contents", &df.contents);
                }
                out.close("Item");
            }
            out.close("dataFiles");
        }

        if self.content_change_sets.is_empty() {
            out.empty("contentChangeSets");
        } else {
            out.open("contentChangeSets");
            for cs in &self.content_change_sets {
                write_change_set(&mut out, cs);
            }
            out.close("contentChangeSets");
        }

        out.item_list("patchFiles", &self.patch_files);
        out.finish("CDataFileMgr__ContentsOfDataFileXml")
    }
}

fn write_change_set(out: &mut XmlOut, cs: &ContentChangeSet) {
    if cs.platform.is_empty() {
        out.open("Item");
    } else {
        out.open_attr("Item", "platform", &cs.platform);
    }
    out.text("changeSetName", &cs.name);

    if cs.map_change_set_data.is_empty() {
        out.empty("mapChangeSetData");
    } else {
        out.open("mapChangeSetData");
        for map in &cs.map_change_set_data {
            out.open("Item");
            out.text("associatedMap", &map.associated_map);
            out.item_list("filesToInvalidate", &map.files_to_invalidate);
            out.item_list("filesToDisable", &map.files_to_disable);
            out.item_list("filesToEnable", &map.files_to_enable);
            out.item_list("txdToLoad", &map.txd_to_load);
            out.item_list("txdToUnload", &map.txd_to_unload);
            out.item_list("residentResources", &map.resident_resources);
            out.item_list("unregisterResources", &map.unregister_resources);
            out.close("Item");
        }
        out.close("mapChangeSetData");
    }

    out.item_list("filesToInvalidate", &cs.files_to_invalidate);
    out.item_list("filesToDisable", &cs.files_to_disable);
    out.item_list("filesToEnable", &cs.files_to_enable);
    out.item_list("txdToLoad", &cs.txd_to_load);
    out.item_list("txdToUnload", &cs.txd_to_unload);
    out.item_list("residentResources", &cs.resident_resources);
    out.item_list("unregisterResources", &cs.unregister_resources);
    if let Some(conditions) = &cs.execution_conditions {
        out.open("executionConditions");
        out.text(
            "activeChangesetConditions",
            &conditions.active_changeset_conditions,
        );
        out.text("genericConditions", &conditions.generic_conditions);
        out.close("executionConditions");
    }
    out.optional_bool("requiresLoadingScreen", cs.requires_loading_screen);
    if !cs.loading_screen_context.is_empty() {
        out.text("loadingScreenContext", &cs.loading_screen_context);
    }
    out.optional_bool("useCacheLoader", cs.use_cache_loader);
    out.close("Item");
}

impl ContentChangeSetDef {
    pub fn to_xml(&self) -> String {
        let root = "CDataFileMgr__ContentChangeSetDef";
        let mut out = XmlOut::document(root);
        write_change_set_def(&mut out, self);
        out.finish(root)
    }
}

/// A change set's elements, in the order GTA VI's documents state them.
fn write_change_set_def(out: &mut XmlOut, set: &ContentChangeSetDef) {
    out.text("id", &set.id);
    out.text("registrationGroup", &set.registration_group);
    if set.file_list.is_empty() {
        out.empty("fileList");
    } else {
        out.open("fileList");
        for file in &set.file_list {
            out.open("Item");
            out.text("filename", &file.filename);
            out.text("fileType", &file.file_type);
            out.text("flags", &file.flags.join(" "));
            out.text("contents", &file.contents);
            out.close("Item");
        }
        out.close("fileList");
    }
    out.text("flags", &set.flags.join(" "));
    out.text(
        "changesetIncludeFlags",
        &set.changeset_include_flags.join(" "),
    );
    for (tag, phase) in [
        ("mapAddAtPhase", &set.map_add_at_phase),
        ("mapRemoveAtPhase", &set.map_remove_at_phase),
    ] {
        if let Some(phase) = phase {
            out.value(tag, phase);
        }
    }
    if !set.associated_map.is_empty() {
        out.text("associatedMap", &set.associated_map);
    }
    if let Some(conditions) = &set.execution_conditions {
        out.open("executionConditions");
        out.text(
            "activeChangesetConditions",
            &conditions.active_changeset_conditions,
        );
        out.text("genericConditions", &conditions.generic_conditions);
        out.close("executionConditions");
    }
}

impl ContentDef {
    pub fn to_xml(&self) -> String {
        let root = "CDataFileMgr__ContentDef";
        let mut out = XmlOut::document(root);
        if self.content_sets.is_empty() {
            out.empty("contentSets");
        } else {
            out.open("contentSets");
            for set in &self.content_sets {
                out.open("Item");
                write_change_set_def(&mut out, set);
                out.close("Item");
            }
            out.close("contentSets");
        }
        out.item_list("includedSets", &self.included_sets);
        if self.included_changesets.is_empty() {
            out.empty("includedChangesets");
        } else {
            out.open("includedChangesets");
            for item in &self.included_changesets {
                out.open_attrs(
                    "Item",
                    &[
                        ("platform", item.platform.as_str()),
                        ("subplatform", item.subplatform.as_str()),
                    ],
                );
                out.text("loadPath", &item.load_path);
                out.text(
                    "changesetIncludeFlags",
                    &item.changeset_include_flags.join(" "),
                );
                out.close("Item");
            }
            out.close("includedChangesets");
        }
        out.finish(root)
    }
}

impl ExtraTitleUpdateData {
    pub fn to_xml(&self) -> String {
        let mut out = XmlOut::document("SExtraTitleUpdateData");
        if self.mounts.is_empty() {
            out.empty("Mounts");
        } else {
            out.open("Mounts");
            for m in &self.mounts {
                out.open("Item");
                out.text("deviceName", &format!("{}:/", m.device_name));
                out.text("path", &m.path);
                out.close("Item");
            }
            out.close("Mounts");
        }
        out.finish("SExtraTitleUpdateData")
    }
}
