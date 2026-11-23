//! The commands of the mesh doctor: look at a garment's models, give them levels of detail, mend their rig, clean their weights and turn left and right over.

use or_wardrobe::mesh::Op;

use super::table::{none_if, Action, Spec, BASE};
use super::wardrobe::{a_garment_to_change, selected_id, the_garment};
use crate::state::AppState;

/// Why the doctor cannot work on the garment selected, none where it can.
fn a_model_to_doctor(state: &AppState) -> Result<(), &'static str> {
    a_garment_to_change(state)?;
    none_if(
        the_garment(state).is_some_and(|(g, _)| g.model.is_none()),
        tr!("This place is held with no model for the doctor to work on"),
    )
}

/// Run `op` over the models of the garment selected.
fn run(state: &mut AppState, op: Op) {
    if let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) {
        state.mesh_op(module, id, op);
    }
}

pub(super) const CHECK_MESH: Spec = Spec {
    action: Action::CheckMesh,
    id: "check_mesh",
    label: tr_key!("Check mesh"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Look at the models of the garment selected and write nothing: the triangles of each level of detail and the levels it lacks, a rig that names a group no skeleton knows, weights that are noise or do not add up, the heel of a pair of shoes, and how much of the texture the coordinates use"),
    enabled: a_model_to_doctor,
    run: |state, _| {
        if let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) {
            state.check_mesh(module, id);
        }
    },
    ..BASE
};

pub(super) const ADD_LEVELS: Spec = Spec {
    action: Action::AddLevels,
    id: "add_levels",
    label: tr_key!("Add levels of detail"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Give the models of the garment selected the Medium and Low levels they lack, made from the High level by collapsing edges: each keeps the High level's own shaders and vertex layout, seams and borders hold, and the High level is not touched. The shares are set in Details. The model becomes a new source the garment names, and Undo names the old one again"),
    enabled: a_model_to_doctor,
    run: |state, _| {
        if let (Some(id), Some(module)) = (selected_id(state), state.wardrobe_module_id()) {
            state.add_levels(module, id);
        }
    },
    ..BASE
};

pub(super) const REPAIR_RIG: Spec = Spec {
    action: Action::RepairRig,
    id: "repair_rig",
    label: tr_key!("Repair rig"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Mend the rig of the models of the garment selected: a bone spelt otherwise than the skeleton spells it takes its spelling, a weight on a group with no name of a bone, or a bone the skeleton does not have, goes to the nearest bone above it that it does have, and the weights are cleaned. The model becomes a new source the garment names, and Undo names the old one again"),
    enabled: a_model_to_doctor,
    run: |state, _| run(state, Op::Rig),
    ..BASE
};

pub(super) const CLEAN_WEIGHTS: Spec = Spec {
    action: Action::CleanWeights,
    id: "clean_weights",
    label: tr_key!("Clean weights"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Drop the weights under one percent of a vertex, make the rest add up to one and give a vertex that has none the skin of the nearest vertex that has. A model with nothing to clean is not changed"),
    enabled: a_model_to_doctor,
    run: |state, _| run(state, Op::Weights),
    ..BASE
};

pub(super) const MIRROR_MODEL: Spec = Spec {
    action: Action::MirrorModel,
    id: "mirror_model",
    label: tr_key!("Mirror model"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Turn left and right over in the models of the garment selected: every coordinate is turned about the middle line, the triangles are wound the other way and the skin goes to the bones on the other side. A skinned ped component needs the freemode skeleton, which is read from the game folder. Mirroring twice gives back what was"),
    enabled: a_model_to_doctor,
    run: |state, _| run(state, Op::Mirror),
    ..BASE
};

pub(super) const CROP_UVS: Spec = Spec {
    action: Action::CropUvs,
    id: "crop_uvs",
    label: tr_key!("Crop texture coordinates"),
    category: tr_key!("Project ▸ Fix"),
    hover: tr_key!("Move the texture coordinates of the garment's models into the part of the texture they use, and cut every texture they sample to that part: the colour textures of the pack and the normal and specular maps embedded in the models. The part is the smallest aligned power-of-two part that holds the coordinates, so each texture is cut by copying whole blocks and none is encoded again, and a crop that would keep more than half of a texture is not made. The models and textures become new sources the garment names, and Undo names the old ones again"),
    enabled: a_model_to_doctor,
    run: |state, _| run(state, Op::UvCrop),
    ..BASE
};
