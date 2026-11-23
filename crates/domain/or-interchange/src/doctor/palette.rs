//! The bone palette a skinned geometry lacks.
//!
//! ⚠ A geometry whose vertices carry weights and bindings but whose `matrix_palette` is empty has nothing to turn a binding into a bone: the
//! game uploads no matrices for it and skins with whatever the registers held from the last thing it drew, so the part flies about the ped.
//! Every ped component shipped with the game carries a palette (almost all of them the identity over 128 bones, so a binding is a skeleton
//! index), and so does what the doctor writes.

use or_io::codec::mesh::{GrcFvfChannel, VertexReader};
use or_io::gta5::{DrawableTarget, GrmGeometry, RmcDrawable};

use crate::extract::channel_lanes;

/// The skeleton index of `SKEL_Pelvis`, which a part bound to one bone follows: the root (0) stays where the ped stands.
pub const PELVIS: u16 = 1;

/// The size of the identity palette the game's own components carry.
pub const IDENTITY: usize = 128;

/// What restoring the palettes of a drawable did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteReport {
    /// A sentence for each geometry given a palette (and the skeleton dropped, where one had to be).
    pub restored: Vec<String>,
    /// A sentence for each skinned geometry left without one: its bindings name no bone of the ped.
    pub left: Vec<String>,
}

/// The palette a geometry whose highest weighted binding is `highest` needs: the pelvis alone where every vertex binds slot 0 (a belt),
/// else the identity over the whole skeleton the game's components use. `None` past it: a binding that names no bone of the ped.
pub fn palette_for(highest: u8) -> Option<Vec<u16>> {
    match usize::from(highest) {
        0 => Some(vec![PELVIS]),
        h if h < IDENTITY => Some((0..IDENTITY as u16).collect()),
        _ => None,
    }
}

/// The highest binding slot a weighted lane of the geometry names, where it is skinned at all.
fn highest_binding<T: DrawableTarget>(geometry: &GrmGeometry<T>) -> Option<u8> {
    let reader = VertexReader::new(&*geometry.vertex_buffer)?;
    let layout = reader.layout();
    if !layout.has(GrcFvfChannel::Weight) || !layout.has(GrcFvfChannel::Binding) {
        return None;
    }
    let mut highest = None;
    for vertex in 0..reader.count() {
        let weights = channel_lanes(&reader, GrcFvfChannel::Weight, vertex);
        let slots = channel_lanes(&reader, GrcFvfChannel::Binding, vertex);
        for lane in 0..4 {
            if weights[lane] != 0 {
                highest = Some(highest.map_or(slots[lane], |h: u8| h.max(slots[lane])));
            }
        }
    }
    highest
}

/// Gives each skinned geometry of `drawable` that has no palette one.
///
/// ⚠ A ped component skins against the ped's skeleton, which its palette indexes, so a skeleton of its own with fewer bones than the palette
/// names (the one-bone skeleton a prop carries) is dropped: a save refuses a palette that names a bone the file's own skeleton lacks, and the
/// game has no use for it on a component.
pub fn restore_palettes<T: DrawableTarget>(drawable: &mut RmcDrawable<T>) -> PaletteReport {
    let mut report = PaletteReport::default();
    let mut highest_bone = 0u16;
    for (level, lod) in drawable.lod_group.lods.iter_mut().enumerate() {
        for (m, model) in lod.models.iter_mut().enumerate() {
            let mut needed = 0usize;
            for (g, geometry) in model.geometries.iter_mut().enumerate() {
                needed = needed.max(geometry.matrix_palette.len());
                if !geometry.matrix_palette.is_empty() {
                    continue;
                }
                let Some(highest) = highest_binding(geometry) else {
                    continue;
                };
                let at = format!("level {level} model {m} geometry {g}");
                match palette_for(highest) {
                    Some(palette) => {
                        report.restored.push(format!(
                            "{at}: a palette of {} bone(s) for bindings up to {highest}",
                            palette.len()
                        ));
                        needed = needed.max(palette.len());
                        highest_bone = highest_bone.max(palette.iter().copied().max().unwrap_or(0));
                        geometry.matrix_palette = palette;
                    }
                    None => report.left.push(format!(
                        "{at}: bindings up to {highest} name no bone of the ped"
                    )),
                }
            }
            // The model states how many matrices its geometries need, as the game's own do.
            if let Ok(count) = u8::try_from(needed) {
                if count > model.matrix_count {
                    model.matrix_count = count;
                }
            }
        }
    }
    if !report.restored.is_empty() {
        let own = drawable.skeleton.as_deref().map(|s| s.bones.len());
        if let Some(bones) = own.filter(|bones| usize::from(highest_bone) >= *bones) {
            drawable.skeleton = None;
            report.restored.push(format!(
                "the skeleton of {bones} bone(s) the file carried is dropped"
            ));
        }
    }
    report
}
