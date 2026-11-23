//! A dynamic mix file's items, typed: `rel_type` 15, such as
//! `dynamix.dat15.rel`. Each item opens with its class id and name table
//! offset in one word, and every type is packed.

use super::layout::{rel_items, rel_struct, NameTableOffset};

rel_struct! {
    pub struct MixerPatch {
        name_table_offset: NameTableOffset,
        flags: u32,
        fade_in: u16,
        fade_out: u16,
        pre_delay: f32,
        duration: f32,
        apply_factor_curve: u32,
        apply_variable: u32,
        apply_smooth_rate: f32,
        mix_categories: Vec<MixCategories> = u8,
    }
}

rel_struct! {
    pub struct AudSceneState {
        name_table_offset: NameTableOffset,
        flags: u32,
        state: Vec<State> = u8,
    }
}

rel_struct! {
    pub struct MixerScene {
        name_table_offset: NameTableOffset,
        flags: u32,
        reference_count: i32,
        patch_group: Vec<PatchGroup> = u8,
    }
}

rel_struct! {
    pub struct MixGroup {
        name_table_offset: NameTableOffset,
        flags: u32,
        reference_count: i32,
        fade_time: f32,
        map: u32,
    }
}

rel_struct! {
    pub struct MixGroupList {
        name_table_offset: NameTableOffset,
        flags: u32,
        mix_group: Vec<MixGroupListMixGroup> = u8,
    }
}

rel_struct! {
    pub struct DynamicMixModuleSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        fade_in: u16,
        fade_out: u16,
        apply_variable: u32,
        duration: f32,
        module_type_settings: u32,
    }
}

rel_struct! {
    pub struct SceneVariableModuleSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        scene_variable: u32,
        input_output_curve: u32,
        input: u8,
        scale_min: f32,
        scale_max: f32,
    }
}

rel_struct! {
    pub struct SceneTransitionModuleSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        input: u8,
        threshold: f32,
        transition: u32,
    }
}

rel_struct! {
    pub struct VehicleCollisionModuleSettings {
        name_table_offset: NameTableOffset,
        flags: u32,
        input: u8,
        transition: u32,
    }
}

rel_struct! {
    pub struct MixGroupCategoryMap {
        name_table_offset: NameTableOffset,
        flags: u32,
        entry: Vec<Entry> = u16,
    }
}

rel_struct! {
    pub struct MixCategories {
        name: u32,
        volume: i16,
        volume_invert: u8,
        lpf_cutoff: u16,
        hpf_cutoff: u16,
        pitch: i16,
        frequency: f32,
        pitch_invert: u8,
        rolloff: f32,
    }
}

rel_struct! {
    pub struct State {
        name: u32,
        scene: u32,
    }
}

rel_struct! {
    pub struct PatchGroup {
        patch: u32,
        mix_group: u32,
    }
}

rel_struct! {
    pub struct MixGroupListMixGroup {
        mix_group_hash: u32,
    }
}

rel_struct! {
    pub struct Entry {
        category_hash: u32,
        parent_hash: u32,
    }
}

rel_items! {
    pub enum DynamicMixItem, class byte 0 {
        0 => MixerPatch packed,
        1 => AudSceneState packed,
        2 => MixerScene packed,
        3 => MixGroup packed,
        4 => MixGroupList packed,
        5 => DynamicMixModuleSettings packed,
        6 => SceneVariableModuleSettings packed,
        7 => SceneTransitionModuleSettings packed,
        8 => VehicleCollisionModuleSettings packed,
        9 => MixGroupCategoryMap packed,
    }
}
