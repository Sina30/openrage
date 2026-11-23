//! `.yft`: a fragment — a drawable that breaks. Its common drawable and any
//! extra ones, the physics levels that say how it breaks (groups, children,
//! joints, archetypes), glass, and the cloth it may carry.
//!
//! ⚠ Every record here that points at a drawable takes the target as a
//! parameter, as the drawable spine does; nothing else in a fragment differs
//! between the targets.

use std::path::Path;
use std::sync::Arc;

use super::bounds::{check_bound, PhBound, PhBoundAny};
use super::cloth_dict::ClothController;
use super::drawable::{check_drawable, Aliasing, Broken as DrawableBroken, Buffers};
use super::drawable::{CLightAttr, DrawableTarget, RmcDrawable};
use super::shared::{DatBase, Mat34V, PgBase, Vec3V, Vec4V};
use crate::decl::*;
use crate::desc::value::{BlobCodecs, BlobReader, BlobSource};
use crate::desc::{Unresolvable, Value, ValueError};
use crate::write::{BlobHeap, BlobWriter, BlobWriters, BuildError, Owner, Placement};
use crate::Generation;

pub mod vehicle_glass;

pub use vehicle_glass::{FwVehicleGlassWindowData, ShatterMap, ShatterMapRow, VehicleGlassWindow};

impl Resource for GtaFragType<Gen8> {
    const EXT: &'static str = "yft";
    const TARGET: Generation = Generation::Gen8;
}

#[cfg(feature = "gen9")]
impl Resource for GtaFragType<Gen9> {
    const EXT: &'static str = "yft";
    const TARGET: Generation = Generation::Gen9;
}

impl GtaFragType {
    /// The Legacy fragment at `path`; an Enhanced one is refused by name, pointing at `or_io::open`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, crate::Error> {
        <Self as File>::open(path)
    }
}

/// `Matrix43`: a transform's three rows, each with its translation in the fourth lane.
#[record(size = 0x30)]
pub struct Matrix43 {
    pub x: Vec4V,
    pub y: Vec4V,
    pub z: Vec4V,
}

/// `fragDrawable`: a drawable as a fragment holds it, with its collision and where the collision sits.
#[record(size = 0x150)]
pub struct FragDrawable<T: DrawableTarget> {
    pub base: RmcDrawable<T>,
    pub bound_matrix: Mat34V,
    pub bound: Option<Box<PhBoundAny>>,
    #[len(count = "extra_bound_count", capacity = "extra_bound_capacity")]
    pub extra_bounds: Vec<Option<Box<PhBoundAny>>>,
    extra_bound_count: u16,
    #[pad_after(4)]
    extra_bound_capacity: u16,
    /// ⚠ One per slot the extra bounds allocate, not per bound: 64 in every child drawable shipped that has any.
    #[len(field = "extra_bound_capacity")]
    pub extra_bounds_matrices: Vec<Mat34V>,
    /// The extra bound slots in use, 1 to 19 where any are allocated; ⚠ every slot shipped is null.
    pub num_extra_bounds: u16,
    pub load_skeleton: Bool8,
    #[pad_after(4)]
    pad0: u8,
    #[note("an empty tree in every fragment drawable shipped")]
    locators: [u64; 3],
    pub skeleton_type_name: String,
    #[note("empty in every fragment drawable shipped")]
    animations: Vec<Ptr<Opaque>>,
    #[note("a runtime pointer, null in every fragment drawable shipped")]
    cloned_shader_group: Ptr<Opaque>,
}

/// A `fragDrawable` is an `rmcDrawable`, and its fields read as the base's.
impl<T: DrawableTarget> std::ops::Deref for FragDrawable<T> {
    type Target = RmcDrawable<T>;
    fn deref(&self) -> &RmcDrawable<T> {
        &self.base
    }
}

impl<T: DrawableTarget> std::ops::DerefMut for FragDrawable<T> {
    fn deref_mut(&mut self) -> &mut RmcDrawable<T> {
        &mut self.base
    }
}

/// `grmMatrixSet`: the per-bone transforms a fragment's skeleton starts from.
#[record(size = 0x20, recompute = GrmMatrixSet::capacity)]
pub struct GrmMatrixSet {
    #[note("a runtime word, zero in every set shipped")]
    instance_id: u32,
    #[note("a runtime count, zero in every set shipped")]
    d3d_reference_count: u32,
    #[note("a runtime word, zero in every set shipped")]
    d3d_fence: u32,
    #[note("a runtime word, zero in every set shipped")]
    d3d_read_fence: u32,
    matrix_count: u8,
    #[note("the matrices allocated; kept, and grown to the matrices")]
    max_matrix_count: u8,
    pub is_skinned: u8,
    padding: u8,
    #[note("a runtime word, zero in every set shipped")]
    d3d_base_flush: u32,
    #[note("a runtime word, zero in every set shipped")]
    d3d_base_address: u32,
    #[note("a runtime word, zero in every set shipped")]
    d3d_size: u32,
    #[trailing]
    #[len(field = "matrix_count")]
    pub matrix_data: Vec<Matrix43>,
}

impl GrmMatrixSet {
    fn capacity(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        raw.max_matrix_count = raw.max_matrix_count.max(raw.matrix_count);
        Ok(())
    }
}

/// `bgPaneModelInfoBase`: a breakable pane, and how its texture projects onto it.
#[record(size = 0x40)]
pub struct BgPaneModelInfoBase {
    pub pos_base: Vec3V,
    pub pos_width: Vec3V,
    pub pos_height: Vec3V,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
}

/// `clothInstanceTuning`: how an environment cloth is tuned.
#[record(size = 0x40)]
pub struct ClothInstanceTuning {
    pub base: PgBase,
    pub rotation_rate: f32,
    #[pad_after(8)]
    pub angle_threshold: f32,
    pub extra_force: Vec3V,
    pub flags: u32,
    pub weight: f32,
    pub distance_threshold: f32,
    pub pin_vert: u8,
    pub non_pin_vert0: u8,
    pub non_pin_vert1: u8,
    padding: u8,
}

/// `environmentCloth`: cloth attached to a fragment, its tuning, the drawable it moves and its controller.
#[record(size = 0x80)]
pub struct EnvironmentCloth<T: DrawableTarget> {
    pub base: PgBase,
    pub tuning: Option<Box<ClothInstanceTuning>>,
    /// The fragment's common drawable again, in every environment cloth shipped.
    pub referenced_drawable: Option<Box<FragDrawable<T>>>,
    #[note("a runtime pointer, null in every environment cloth shipped")]
    behavior: Ptr<Opaque>,
    /// ⚠ Allocated 0x80 bytes in every file shipped, and the eight past the
    /// controller's own 0x78 are zero in all of them.
    pub cloth_controller: Option<Box<ClothController>>,
    padding0: [u8; 12],
    padding: [u8; 4],
    pub initial_position: Vec3V,
    pub force: Vec3V,
    pub user_data: Vec<i32>,
    #[note("a runtime pointer, null in every environment cloth shipped")]
    attached_to_frame: Ptr<Opaque>,
    #[pad_after(4)]
    pub flags: u32,
}

/// `phJointType`: what every joint starts with; `joint_type` selects the layout that follows.
///
/// Every joint the corpus holds is one of the two variants declared.
#[record(
    size = 0xa0,
    tag = "joint_type",
    variant(0 => PhJoint1DofType),
    variant(1 => PhJoint3DofType)
)]
pub struct PhJointType {
    pub base: PgBase,
    pub default_stiffness: f32,
    pub enforce_acceded_limits: Bool8,
    pub joint_type: u8,
    pub parent_link_index: u8,
    #[pad_after(8)]
    pub child_link_index: u8,
    pub orient_parent: Mat34V,
    pub orient_child: Mat34V,
}

/// `phJoint1DofType`: a joint turning about one axis.
#[record(size = 0xb0)]
pub struct PhJoint1DofType {
    pub base: PhJointType,
    pub hard_angle_min: f32,
    pub hard_angle_max: f32,
    pub max_muscle_torque: f32,
    pub min_muscle_torque: f32,
}

/// `phJoint3DofType`: a joint turning about three axes.
#[record(size = 0xf0)]
pub struct PhJoint3DofType {
    pub base: PhJointType,
    pub hard_first_lean_angle_max: f32,
    pub hard_second_lean_angle_max: f32,
    pub hard_twist_angle_max: f32,
    pub twist_offset: f32,
    pub use_child_for_twist_axis: Bool8,
    #[note("not zero in 32 of 204 Legacy joints shipped, so carried")]
    pad: [u8; 15],
    pub max_muscle_torque: Vec3V,
    pub min_muscle_torque: Vec3V,
    #[note("not zero in 32 of 204 Legacy joints shipped, so carried")]
    pad1: [u8; 16],
}

/// `phArticulatedBodyType`: the joints a fragment articulates on, and each link's inertia.
#[record(size = 0xb0)]
pub struct PhArticulatedBodyType {
    pub base: PgBase,
    pub joint_parent_index: [i32; 23],
    pub replace_upon_re_resource: f32,
    #[pad_after(4)]
    pub angular_decay_rate: f32,
    #[len(field = "num_joints")]
    pub ph_joint_types: Vec<Box<PhJointTypeAny>>,
    /// Each link's angular inertia in `x`, `y` and `z`, and its mass in `w`.
    #[len(field = "num_links")]
    pub resourced_ang_inertia_xyz_mass_w: Vec<Vec4V>,
    num_links: u8,
    num_joints: u8,
    pub joint_types: [u8; 22],
    pub locally_owned: Bool8,
    #[pad_after(12)]
    pad: [u8; 3],
}

/// `evtSet`: the events a fragment part raises, empty in every set shipped.
#[record(size = 0x30)]
pub struct EvtSet {
    pub base: DatBase,
    #[note("empty in every event set shipped")]
    instances: Vec<Ptr<Opaque>>,
    #[pad_after(4)]
    pub new_instance_type: i32,
    #[note("a runtime pointer, null in every event set shipped")]
    bank: Ptr<Opaque>,
    #[note("a runtime pointer, null in every event set shipped")]
    group: Ptr<Opaque>,
}

/// `fragTypeGroup`: children that break off together, and how strongly they hold.
#[record(size = 0xb0)]
pub struct FragTypeGroup {
    pub death_eventset: Option<Box<EvtSet>>,
    #[note("a runtime pointer, null in every group shipped")]
    death_event_player: Ptr<Opaque>,
    pub strength: f32,
    pub force_transmission_scale_up: f32,
    pub force_transmission_scale_down: f32,
    pub joint_stiffness: f32,
    pub min_soft_angle1: f32,
    pub max_soft_angle1: f32,
    pub max_soft_angle2: f32,
    pub max_soft_angle3: f32,
    pub rotation_speed: f32,
    pub rotation_strength: f32,
    pub restoring_strength: f32,
    pub restoring_max_torque: f32,
    pub latch_strength: f32,
    pub total_undamaged_mass: f32,
    pub total_damaged_mass: f32,
    pub child_groups_pointers_index: u8,
    pub parent_group_pointer_index: u8,
    pub child_index: u8,
    pub num_children: u8,
    pub num_child_groups: u8,
    pub glass_model_and_type: u8,
    pub glass_pane_model_info_index: u8,
    pub flags: u8,
    pub min_damage_force: f32,
    pub damage_health: f32,
    pub weapon_health: f32,
    pub weapon_scale: f32,
    pub vehicle_scale: f32,
    pub ped_scale: f32,
    pub ragdoll_scale: f32,
    pub explosion_scale: f32,
    pub object_scale: f32,
    pub ped_inv_mass_scale: f32,
    pub preset_applied: i32,
    /// NUL-padded ASCII.
    pub debug_name: [u8; 40],
    #[pad_after(4)]
    pub melee_scale: f32,
}

impl FragTypeGroup {
    /// The name up to its first NUL.
    pub fn name(&self) -> &[u8] {
        let end = self
            .debug_name
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(self.debug_name.len());
        &self.debug_name[..end]
    }
}

/// `fragTypeChild`: one piece of a fragment, its masses, its bone and the drawables it shows whole and damaged.
#[record(size = 0x100)]
pub struct FragTypeChild<T: DrawableTarget> {
    pub base: DatBase,
    pub undamaged_mass: f32,
    pub damaged_mass: f32,
    pub owner_group_pointer_index: u8,
    pub flags: u8,
    pub bone_id: u16,
    pad_x: [u8; 12],
    pad0: [u8; 64],
    pad1: [u8; 64],
    pub undamaged_entity: Option<Box<FragDrawable<T>>>,
    pub damaged_entity: Option<Box<FragDrawable<T>>>,
    pub continuous_eventset: Option<Box<EvtSet>>,
    pub collision_eventset: Option<Box<EvtSet>>,
    pub break_eventset: Option<Box<EvtSet>>,
    pub break_from_root_eventset: Option<Box<EvtSet>>,
    #[note("a runtime pointer, null in every child shipped")]
    collision_event_player: Ptr<Opaque>,
    #[note("a runtime pointer, null in every child shipped")]
    break_event_player: Ptr<Opaque>,
    #[note("a runtime pointer, null in every child shipped")]
    break_from_root_event_player: Ptr<Opaque>,
    #[pad_after(12)]
    pad: [u8; 12],
}

/// `phArchetypeDamp`: how a fragment's rigid body moves, its mass, inertia, damping and collision.
#[record(size = 0xe0)]
pub struct PhArchetypeDamp {
    pub base: PgBase,
    #[pad_after(4)]
    pub r#type: i32,
    /// The undamaged archetype's name again, in the damaged one of 9 Legacy and 2 Enhanced levels shipped.
    pub filename: String,
    pub bound: Option<Box<PhBoundAny>>,
    pub type_flags: u32,
    pub include_flags: u32,
    pub property_flags: u16,
    #[init(1)]
    #[pad_after(4)]
    ref_count: u16,
    archetype_phys_pad: [u8; 8],
    pub mass: f32,
    pub inv_mass: f32,
    pub gravity_factor: f32,
    pub max_speed: f32,
    pub max_ang_speed: f32,
    #[pad_after(8)]
    pub buoyancy_factor: f32,
    pub ang_inertia: Vec3V,
    pub inv_ang_inertia: Vec3V,
    /// Linear C, V and V², then angular C, V and V².
    pub damping_constant: [Vec3V; 6],
}

/// `pgArray<Matrix34>`: a physics level's link attachments, trailing their header.
#[record(size = 0x20)]
pub struct PgArray {
    pub base: PgBase,
    count: u64,
    #[note("zero in every array shipped")]
    pad: u64,
    #[trailing]
    #[len(field = "count")]
    pub elements: Vec<Mat34V>,
}

/// `fragPhysicsLOD`: one physics level of detail, its groups, children, archetypes, joints and collision.
#[record(size = 0x130, recompute = FragPhysicsLOD::capacity)]
pub struct FragPhysicsLOD<T: DrawableTarget> {
    pub base: PgBase,
    pad00: [u8; 4],
    pub smallest_ang_inertia: f32,
    pub largest_ang_inertia: f32,
    pub min_move_force: f32,
    pub body_type: Option<Box<PhArticulatedBodyType>>,
    #[len(field = "num_children")]
    pub min_breaking_impulses: Vec<f32>,
    pub root_cg_offset: Vec3V,
    pub original_root_cg_offset: Vec3V,
    pub unbroken_cg_offset: Vec3V,
    /// Linear C, V and V², then angular C, V and V².
    pub damping_constant: [Vec3V; 6],
    /// ⚠ Each points into its group's `debug_name` in every level shipped, and a save writes strings of its own.
    #[len(field = "num_groups")]
    pub group_names: Vec<String>,
    #[len(field = "num_groups")]
    pub groups: Vec<Box<FragTypeGroup>>,
    #[len(field = "num_children")]
    pub children: Vec<Box<FragTypeChild<T>>>,
    pub phys_damp_undamaged: Option<Box<PhArchetypeDamp>>,
    pub phys_damp_damaged: Option<Box<PhArchetypeDamp>>,
    /// The undamaged archetype's bound again, in every level shipped.
    pub composite_bounds: Option<Box<PhBoundAny>>,
    #[len(field = "num_children")]
    pub undamaged_ang_inertia: Vec<Vec3V>,
    #[len(field = "num_children")]
    pub damaged_ang_inertia: Vec<Vec3V>,
    pub link_attachments: Option<Box<PgArray>>,
    #[len(field = "num_self_collisions")]
    pub self_collision_a: Vec<u8>,
    #[len(field = "num_self_collisions")]
    pub self_collision_b: Vec<u8>,
    num_self_collisions: u8,
    #[note("the pairs allocated; kept, and grown to the pairs")]
    max_num_self_collisions: u8,
    num_groups: u8,
    pub root_group_count: u8,
    pub num_root_damage_regions: u8,
    /// As many as the children, in every level shipped.
    pub num_bony_children: u8,
    num_children: u8,
    #[pad_after(4)]
    pad1: [u8; 13],
}

impl<T: DrawableTarget> FragPhysicsLOD<T> {
    fn capacity(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        raw.max_num_self_collisions = raw.max_num_self_collisions.max(raw.num_self_collisions);
        Ok(())
    }
}

/// `fragPhysicsLODGroup`: three physics levels of detail, any of them absent.
#[record(size = 0x30)]
pub struct FragPhysicsLODGroup<T: DrawableTarget> {
    pub base: PgBase,
    pub high_lod: Option<Box<FragPhysicsLOD<T>>>,
    pub medium_lod: Option<Box<FragPhysicsLOD<T>>>,
    #[pad_after(8)]
    pub low_lod: Option<Box<FragPhysicsLOD<T>>>,
}

impl<T: DrawableTarget> FragPhysicsLODGroup<T> {
    /// The levels it holds, highest first, skipping empty slots.
    pub fn lods(&self) -> impl Iterator<Item = &FragPhysicsLOD<T>> {
        [&self.high_lod, &self.medium_lod, &self.low_lod]
            .into_iter()
            .flatten()
            .map(|l| &**l)
    }
}

/// `fragType`: a fragment's drawables, cloth, glass and physics.
#[record(size = 0x110)]
pub struct FragType<T: DrawableTarget> {
    pub base: PgBase,
    #[pad_after(8)]
    pad0: [u8; 8],
    /// Centre in the first three, radius in the fourth.
    pub bounding_sphere: Vec4V,
    pub common_drawable: Option<Box<FragDrawable<T>>>,
    #[len(field = "num_extra_drawables")]
    pub extra_drawables: Vec<Box<FragDrawable<T>>>,
    #[len(field = "num_extra_drawables")]
    pub extra_drawable_names: Vec<String>,
    num_extra_drawables: i32,
    /// Which extra drawable is the damaged one, or -1.
    pub damaged_drawable: i32,
    #[note("a runtime pointer, null in every fragment shipped")]
    root_child: Ptr<Opaque>,
    pub tune_name: String,
    pub env_cloth: Vec<Box<EnvironmentCloth<T>>>,
    #[note("empty in every fragment shipped")]
    char_cloth: Vec<Ptr<Opaque>>,
    #[note("a runtime pointer, null in every fragment shipped")]
    user_data: Ptr<Opaque>,
    pub collision_eventset: Option<Box<EvtSet>>,
    #[note("a runtime pointer, null in every fragment shipped")]
    collision_event_player: Ptr<Opaque>,
    pad6: u32,
    pad7: u32,
    #[pad_after(4)]
    pad8: u32,
    pub shared_matrix_set: Option<Box<GrmMatrixSet>>,
    pub estimated_cache_size: u64,
    pub estimated_articulated_cache_size: u64,
    pub entity_class: u8,
    pub art_asset_id: i8,
    pub attach_bottom_end: Bool8,
    pad1: u8,
    pub flags: u16,
    pad2: u16,
    pub client_class_id: i32,
    pub unbroken_elasticity: f32,
    pub gravity_factor: f32,
    pub buoyancy_factor: f32,
    pub glass_attachment_bone: u8,
    num_glass_pane_model_infos: u8,
    #[pad_after(4)]
    pad3: [u8; 2],
    #[len(field = "num_glass_pane_model_infos")]
    pub glass_pane_model_infos: Vec<Box<BgPaneModelInfoBase>>,
    #[pad_after(4)]
    pad5: u32,
    pub physics_lod_group: Option<Box<FragPhysicsLODGroup<T>>>,
    pub common_cloth_drawable: Option<Box<FragDrawable<T>>>,
    #[pad_after(12)]
    pad4: u32,
}

/// `gtaFragType`: the engine's fragment, then the lights, the vehicle glass and the tint the game adds.
#[record(size = 0x130)]
pub struct GtaFragType<T: DrawableTarget = Gen8> {
    pub base: FragType<T>,
    pub lights: Vec<CLightAttr>,
    pub vehicle_window_data: Option<Box<FwVehicleGlassWindowData>>,
    #[note("null in every fragment shipped")]
    tint_data: Ptr<Opaque>,
}

/// A `gtaFragType` is a `fragType`, and its fields read as the base's.
impl<T: DrawableTarget> std::ops::Deref for GtaFragType<T> {
    type Target = FragType<T>;
    fn deref(&self) -> &FragType<T> {
        &self.base
    }
}

impl<T: DrawableTarget> std::ops::DerefMut for GtaFragType<T> {
    fn deref_mut(&mut self) -> &mut FragType<T> {
        &mut self.base
    }
}

/// The prologue of the block `vehicle_window_data` points at; its codec reads the rest.
#[doc(hidden)]
#[record(size = 0x0c)]
pub struct FwVehicleGlassWindowDataHeader {
    magic: u32,
    header_stride: u16,
    item_count: u16,
    total_length: u32,
}

const GLASS: &str = "GtaFragType::vehicle_window_data";
const GLASS_FIELD: &str = "vehicle_window_data";
const BYTES: &str = "bytes";

fn glass_invalid(why: String) -> OwnedError {
    OwnedError::Invalid {
        what: GLASS.into(),
        why,
    }
}

impl Heap for FwVehicleGlassWindowData {
    fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let rows = self.windows.iter().map(|w| {
            let rows = &w.shatter_map.rows;
            rows.capacity() * size_of::<ShatterMapRow>()
                + rows
                    .iter()
                    .map(|r| r.data1.capacity() + r.data2.capacity())
                    .sum::<usize>()
        });
        self.windows.capacity() * size_of::<VehicleGlassWindow>() + rows.sum::<usize>()
    }
}

impl Owned for FwVehicleGlassWindowData {
    type Raw = Raw<FwVehicleGlassWindowDataHeader>;
    const LEAF: bool = false;

    fn from_raw(_: &Self::Raw, _: Kids<'_>, at: usize, _: &Rx<'_>) -> Result<Self, OwnedError> {
        Err(glass_invalid(format!(
            "a glass block is reached through a pointer, not embedded at {at:#x}"
        )))
    }

    fn to_raw(
        &self,
        _: &mut Vec<(u32, Value)>,
        at: usize,
        _: &Cx,
    ) -> Result<Self::Raw, OwnedError> {
        Err(glass_invalid(format!(
            "a glass block is reached through a pointer, not embedded at {at:#x}"
        )))
    }

    /// ⚠ Refuses a block that is not its own re-encoding, so a save never drops what the value could not state.
    fn from_value(v: &Value, _: &Rx<'_>) -> Result<Self, OwnedError> {
        let bytes = glass_bytes(v)
            .ok_or_else(|| glass_invalid("the value was not read by the glass codec".into()))?;
        let block =
            FwVehicleGlassWindowData::parse(bytes).map_err(|why| glass_invalid(why.to_string()))?;
        if block.write() != bytes {
            return Err(glass_invalid(
                "the block is not the encoding of its own windows: its offsets, length or \
                 padding differ"
                    .into(),
            ));
        }
        Ok(block)
    }

    fn to_element(&self, _: &Cx) -> Result<Value, OwnedError> {
        Ok(Value::Blob {
            codec: GLASS.into(),
            parts: vec![(BYTES.into(), Value::Bytes(self.write()))],
        })
    }
}

fn glass_bytes(blob: &Value) -> Option<&[u8]> {
    let Value::Blob { codec, .. } = blob else {
        return None;
    };
    if codec != GLASS {
        return None;
    }
    blob.part(BYTES)?.as_bytes()
}

/// The glass codec: the `HWGV` block, as long as its own prologue states.
struct GlassBlob;

impl BlobReader for GlassBlob {
    fn read(&self, blob: &dyn BlobSource) -> Result<Value, ValueError> {
        let (_, at) = blob.blob();
        let image = blob.image();
        let bad = |reason| ValueError::BadPointer {
            field: GLASS_FIELD.to_string(),
            reason,
        };
        let head = image
            .slice_at(at, vehicle_glass::BLOCK_HEADER_LEN)
            .map_err(bad)?;
        let length = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
        if length < vehicle_glass::BLOCK_HEADER_LEN {
            return Err(bad(Unresolvable::Null));
        }
        let bytes = image.slice_at(at, length).map_err(bad)?.to_vec();
        Ok(Value::Blob {
            codec: GLASS.to_string(),
            parts: vec![(BYTES.to_string(), Value::Bytes(bytes))],
        })
    }
}

impl BlobWriter for GlassBlob {
    fn write(
        &self,
        heap: &mut dyn BlobHeap,
        owner: Owner<'_>,
        value: &Value,
    ) -> Result<Placement, BuildError> {
        let bytes = glass_bytes(value).ok_or_else(|| BuildError::Blob {
            type_name: owner.ty.name.clone(),
            field: GLASS_FIELD.to_string(),
            message: format!("the value is not a `{GLASS}`"),
        })?;
        let block = heap.alloc(bytes.len(), 16);
        heap.put(heap.offset_of(block), bytes)?;
        Ok(Placement {
            address: block,
            siblings: Vec::new(),
        })
    }
}

/// The readers a `.yft` tree is read with: the drawables' and the glass block's.
pub fn readers() -> BlobCodecs {
    super::drawable_codecs::readers().at("GtaFragType", GLASS_FIELD, Arc::new(GlassBlob))
}

/// The writers its canonical save is built with.
pub fn writers() -> BlobWriters {
    super::drawable_codecs::writers().at("GtaFragType", GLASS_FIELD, Arc::new(GlassBlob))
}

/// A fragment the game would misread, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("{at}: {why}")]
    Drawable { at: String, why: DrawableBroken },
    #[error("{at}: the collision bound: {why}")]
    Bounds {
        at: String,
        why: super::bounds::Broken,
    },
    #[error("{0}")]
    Aliased(#[from] Aliasing),
    #[error("{0}")]
    View(#[from] ViewError),
}

/// One of a fragment's drawables and its bound, and the buffers it points at.
fn check_fragment_drawable<T>(
    image: Image<'_>,
    d: &View<'_, Raw<FragDrawable<T>>>,
    at: &str,
    target: Generation,
    buffers: &mut Buffers,
) -> Result<(), Broken>
where
    T: DrawableTarget,
{
    let drawable = d.base();
    check_drawable(&drawable, target).map_err(|why| Broken::Drawable {
        at: at.to_string(),
        why,
    })?;
    buffers.visit(&drawable)?;
    if let Some(bound) = d.bound()? {
        let bound = View::<Raw<PhBound>>::at_offset(image, View::offset(&bound))?;
        check_bound(bound, "").map_err(|why| Broken::Bounds {
            at: at.to_string(),
            why,
        })?;
    }
    Ok(())
}

/// A fragment's drawable read in place, and how a refusal names it.
type Named<'a, T> = (String, View<'a, Raw<FragDrawable<T>>>);

/// Each drawable a fragment holds, as a refusal names it: common, extras,
/// cloth, then each physics level's children, undamaged before damaged.
fn named_drawables<T: DrawableTarget>(
    image: Image<'_>,
    root: usize,
) -> Result<Vec<Named<'_, T>>, ViewError> {
    let fragment = View::<Raw<GtaFragType<T>>>::at_offset(image, root)?;
    let fragment = fragment.base();
    let mut out = Vec::new();
    if let Some(d) = fragment.common_drawable()? {
        out.push(("drawable".to_string(), d));
    }
    let extras = fragment.extra_drawables()?;
    for i in 0..extras.len() {
        if let Some(d) = extras.follow(i)? {
            out.push((format!("extra drawable {i}"), d));
        }
    }
    if let Some(d) = fragment.common_cloth_drawable()? {
        out.push(("cloth drawable".to_string(), d));
    }
    if let Some(group) = fragment.physics_lod_group()? {
        let lods = [group.high_lod()?, group.medium_lod()?, group.low_lod()?];
        for (l, lod) in lods.iter().flatten().enumerate() {
            let children = lod.children()?;
            for c in 0..children.len() {
                let Some(child) = children.follow(c)? else {
                    continue;
                };
                for (which, d) in [
                    ("", child.undamaged_entity()?),
                    (" damaged", child.damaged_entity()?),
                ] {
                    if let Some(d) = d {
                        out.push((format!("physics level {l}, child {c}{which}"), d));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Each drawable a fragment holds, in the order its validation visits them and [`FragType::drawables_mut`] gives them.
pub fn drawables<T: DrawableTarget>(
    image: Image<'_>,
    root: usize,
) -> Result<Vec<View<'_, Raw<RmcDrawable<T>>>>, ViewError> {
    Ok((named_drawables::<T>(image, root)?.iter())
        .map(|(_, d)| d.base())
        .collect())
}

impl<T: DrawableTarget> FragType<T> {
    /// Each drawable the fragment holds, with its bounds, in the order [`Self::drawables_mut`] gives them.
    pub fn drawables(&self) -> Vec<&FragDrawable<T>> {
        let mut out = Vec::new();
        out.extend(self.common_drawable.as_deref());
        out.extend(self.extra_drawables.iter().map(|d| &**d));
        out.extend(self.common_cloth_drawable.as_deref());
        for lod in self
            .physics_lod_group
            .as_deref()
            .into_iter()
            .flat_map(|g| g.lods())
        {
            for child in &lod.children {
                out.extend(child.undamaged_entity.as_deref());
                out.extend(child.damaged_entity.as_deref());
            }
        }
        out
    }

    /// How many physics children its levels hold between them.
    pub fn child_count(&self) -> usize {
        (self.physics_lod_group.as_deref())
            .map_or(0, |g| g.lods().map(|lod| lod.children.len()).sum())
    }

    /// Each drawable the fragment holds, in the order its validation visits them.
    pub fn drawables_mut(&mut self) -> Vec<&mut RmcDrawable<T>> {
        let mut out = Vec::new();
        out.extend(self.common_drawable.as_deref_mut().map(|d| &mut d.base));
        out.extend(self.extra_drawables.iter_mut().map(|d| &mut d.base));
        out.extend(
            self.common_cloth_drawable
                .as_deref_mut()
                .map(|d| &mut d.base),
        );
        if let Some(group) = self.physics_lod_group.as_deref_mut() {
            let lods = [
                &mut group.high_lod,
                &mut group.medium_lod,
                &mut group.low_lod,
            ];
            for lod in lods.into_iter().flatten() {
                for child in &mut lod.children {
                    out.extend(child.undamaged_entity.as_deref_mut().map(|d| &mut d.base));
                    out.extend(child.damaged_entity.as_deref_mut().map(|d| &mut d.base));
                }
            }
        }
        out
    }
}

fn check<T>(
    image: Image<'_>,
    root: usize,
    target: Generation,
    buffers: &mut Buffers,
) -> Result<(), Broken>
where
    T: DrawableTarget,
{
    for (at, d) in named_drawables::<T>(image, root)? {
        check_fragment_drawable(image, &d, &at, target, buffers)?;
    }
    let fragment = View::<Raw<GtaFragType<T>>>::at_offset(image, root)?;
    if let Some(group) = fragment.base().physics_lod_group()? {
        let lods = [group.high_lod()?, group.medium_lod()?, group.low_lod()?];
        for (l, lod) in lods.iter().flatten().enumerate() {
            if let Some(bound) = lod.composite_bounds()? {
                let bound = View::<Raw<PhBound>>::at_offset(image, View::offset(&bound))?;
                check_bound(bound, "").map_err(|why| Broken::Bounds {
                    at: format!("physics level {l}"),
                    why,
                })?;
            }
        }
    }
    Ok(())
}

/// A `.yft`'s root, checked for what the game assumes and never checks.
pub fn validate(image: Image<'_>, root: usize, target: Generation) -> Result<(), Broken> {
    Ok(buffers(image, root, target)?.refuse()?)
}

/// The buffers every drawable of a fragment points at, once each passes its checks.
pub fn buffers(image: Image<'_>, root: usize, target: Generation) -> Result<Buffers, Broken> {
    let mut buffers = Buffers::default();
    #[cfg(feature = "gen9")]
    if target == Generation::Gen9 {
        check::<Gen9>(image, root, target, &mut buffers)?;
        return Ok(buffers);
    }
    check::<Gen8>(image, root, target, &mut buffers)?;
    Ok(buffers)
}
