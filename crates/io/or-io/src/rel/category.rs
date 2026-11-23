//! A category file's items, typed: `rel_type` 22, such as
//! `categories.dat22.rel`. Its one type opens with its class id and name table
//! offset in one word, and is packed.

use super::layout::{rel_items, rel_struct, NameTableOffset};

rel_struct! {
    pub struct Category {
        name_table_offset: NameTableOffset,
        flags: u32,
        parent_overrides: u16,
        volume: i16,
        pitch: i16,
        lpf_cutoff: u16,
        lpf_distance_curve: u32,
        hpf_cutoff: u16,
        hpf_distance_curve: u32,
        distance_roll_off_scale: u16,
        plateau_roll_off_scale: u16,
        occlusion_damping: u16,
        environmental_filter_damping: u16,
        source_reverb_damping: u16,
        distance_reverb_damping: u16,
        interior_reverb_damping: u16,
        environmental_loudness: u16,
        underwater_wet_level: u16,
        stoned_wet_level: u16,
        timer: u8,
        category_ref: Vec<CategoryRef> = u8,
    }
}

rel_struct! {
    pub struct CategoryRef {
        category_id: u32,
    }
}

rel_items! {
    pub enum CategoryItem, class byte 0 {
        0 => Category packed,
    }
}
