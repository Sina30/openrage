//! `.yld`: character cloths keyed by name hash. A cloth is a controller over
//! up to four simulated levels, a bridge from them to the drawn mesh, and the
//! composite bound it collides with.
//!
//! ⚠ **`LOD_COUNT` is four.** CodeWalker reads three and under-reads every
//! per-level array, so a figure checked against it agrees with a bug.

use super::bounds::{self, PhBoundAny, PhBoundComposite};
use super::shared::{AtBitSet, Mat34V, PgBase, PgDictionary, Vec3V, Vec4V};
use crate::decl::*;
use crate::desc::Value;
use crate::Generation;

impl Resource for PgDictionary<CharacterCloth> {
    const EXT: &'static str = "yld";
    const TARGET: Generation = Generation::Gen8;
}

/// How many simulated levels a controller holds, and how many per-level runs a bridge does.
pub const LOD_COUNT: usize = 4;

/// A garment's cloth: its controller, the composite bound it collides with, and its parent transform.
#[record(size = 0xd0)]
pub struct CharacterCloth {
    pub base: PgBase,
    pub poses: Vec<Vec3V>,
    pub cloth_controller: Option<Box<CharacterClothController>>,
    pub bound_composite: Option<Box<PhBoundComposite>>,
    pub bone_id: Vec<i32>,
    pub force: Vec3V,
    pub parent_matrix: Mat34V,
    pub bone_index: Vec<i32>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    entity_vertices_ptr: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    entity_parent_matrix_ptr: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    bound_composite_ref: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    character_cloth_type: Ptr<Opaque>,
    pub is_active: Bool8,
    pub negate_force: Bool8,
    pub lock_counter: u8,
    #[pad_after(12)]
    pad: u8,
}

impl CharacterCloth {
    /// How many vertices one level's simulated mesh holds; zero for an empty slot.
    pub fn lod_vertex_count(&self, lod: usize) -> usize {
        self.cloth_controller
            .as_ref()
            .and_then(|c| c.base.cloth().get(lod).copied())
            .and_then(|c| c.as_ref())
            .map_or(0, |c| c.cloth_data.vertex_positions.len())
    }
}

/// `characterClothController::BindingInfo`: how much of a cloth vertex each of up to four bones holds.
#[record(size = 0x20)]
pub struct BindingInfo {
    pub weights: Vec4V,
    /// Entries of the controller's `bone_index_map`, not bone tags and not skeleton indices.
    pub blend_indices: [i32; 4],
}

impl BindingInfo {
    /// The four weights, in slot order.
    pub fn weight_array(&self) -> [f32; 4] {
        [
            self.weights.x,
            self.weights.y,
            self.weights.z,
            self.weights.w,
        ]
    }
}

/// `clothController`: a cloth's bridge, its morph controller and up to four simulated levels.
#[record(size = 0x78)]
pub struct ClothController {
    pub base: PgBase,
    pub bridge: Option<Box<ClothBridgeSimGfx>>,
    pub morph_controller: Option<Box<PhMorphController>>,
    /// ⚠ The engine's `cloth` is one array of four slots, declared a field per slot.
    pub cloth_0: Option<Box<PhVerletCloth>>,
    pub cloth_1: Option<Box<PhVerletCloth>>,
    pub cloth_2: Option<Box<PhVerletCloth>>,
    pub cloth_3: Option<Box<PhVerletCloth>>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    cloth_instance: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    owner: Ptr<Opaque>,
    pub flags: u32,
    pub normal_offset: i8,
    pub uv_offset: i8,
    pub lod: i8,
    pub indices_winding_order: Bool8,
    /// NUL-padded ASCII.
    #[pad_after(4)]
    pub name: [u8; 28],
}

/// What drives a garment's cloth: its controller, its pinning and its skin binding.
#[record(size = 0xf0)]
pub struct CharacterClothController {
    pub base: ClothController,
    pub pinning_radius_scale: f32,
    pad: [u8; 4],
    pub tri_indices: Vec<u16>,
    pub original_pos: Vec<Vec3V>,
    pub pin_radius_set_threshold: f32,
    padding: [u8; 12],
    pub bone_index_map: Vec<i32>,
    pub binding_info: Vec<BindingInfo>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    controller_type: Ptr<Opaque>,
    pub force_pin: u8,
    pub flags: u8,
    pub new_package_index: u8,
    pub package_index: u8,
    pub wind_scale: f32,
    pub bone_id_map: Vec<i32>,
}

/// One cloth vertex's up to four bones named by tag, and the weight each holds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClothVertexTags {
    /// ⚠ `None` where the garment states an index and no tag: zero would name the root.
    pub tags: [Option<u16>; 4],
    pub weights: [f32; 4],
}

impl ClothVertexTags {
    /// Whether every slot carrying weight also names a tag.
    pub fn is_fully_tagged(&self) -> bool {
        (0..4).all(|i| self.weights[i] == 0.0 || self.tags[i].is_some())
    }
}

impl ClothController {
    /// The name up to its first NUL.
    pub fn name(&self) -> &[u8] {
        let end = self
            .name
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(self.name.len());
        &self.name[..end]
    }

    /// The four level slots, in order; an empty slot is `None`.
    pub fn cloth(&self) -> [&Option<Box<PhVerletCloth>>; LOD_COUNT] {
        [&self.cloth_0, &self.cloth_1, &self.cloth_2, &self.cloth_3]
    }

    /// The four level slots, to edit.
    pub fn cloth_mut(&mut self) -> [&mut Option<Box<PhVerletCloth>>; LOD_COUNT] {
        [
            &mut self.cloth_0,
            &mut self.cloth_1,
            &mut self.cloth_2,
            &mut self.cloth_3,
        ]
    }
}

impl CharacterClothController {
    /// The bone tag a blend index names, through `bone_id_map`.
    pub fn bone_tag_for_blend_index(&self, blend: i32) -> Option<u16> {
        // ⚠ A blend index addresses `bone_index_map`, whose entries index the
        // ped's skeleton, in another file; `bone_id_map` states the same bones
        // as tags, which survive a skeleton the indices do not.
        let tag = *self.bone_id_map.get(usize::try_from(blend).ok()?)?;
        u16::try_from(tag).ok()
    }

    /// The blend index that names `tag`, if this garment's `bone_id_map` carries it.
    pub fn blend_index_for_bone_tag(&self, tag: u16) -> Option<i32> {
        let i = self
            .bone_id_map
            .iter()
            .position(|id| u16::try_from(*id).is_ok_and(|t| t == tag))?;
        i32::try_from(i).ok()
    }

    /// The skeleton index a blend index names, meaningful only on the skeleton the garment was authored against.
    pub fn bone_index_for_blend_index(&self, blend: i32) -> Option<i32> {
        self.bone_index_map
            .get(usize::try_from(blend).ok()?)
            .copied()
    }

    /// Cloth vertex `vertex`'s bones as tags.
    pub fn vertex_weight_tags(&self, vertex: usize) -> Option<ClothVertexTags> {
        let binding = self.binding_info.get(vertex)?;
        Some(ClothVertexTags {
            tags: std::array::from_fn(|i| self.bone_tag_for_blend_index(binding.blend_indices[i])),
            weights: binding.weight_array(),
        })
    }

    /// The binding that stores `tags`; `None` where a weighted slot names a tag this garment's `bone_id_map` does not carry.
    pub fn binding_for_tags(&self, tags: &ClothVertexTags) -> Option<BindingInfo> {
        let mut blend_indices = [0i32; 4];
        for (slot, index) in blend_indices.iter_mut().enumerate() {
            match tags.tags[slot] {
                Some(tag) => *index = self.blend_index_for_bone_tag(tag)?,
                None if tags.weights[slot] != 0.0 => return None,
                None => {}
            }
        }
        let [x, y, z, w] = tags.weights;
        Some(BindingInfo {
            weights: Vec4V {
                x,
                y,
                z,
                w,
                ..Default::default()
            },
            blend_indices,
            ..Default::default()
        })
    }

    /// One level's simulation weight for one vertex.
    pub fn vertex_weight(&self, lod: usize, vertex: usize) -> Option<f32> {
        self.base
            .bridge
            .as_ref()?
            .vertex_weight(lod)?
            .get(vertex)
            .copied()
    }

    /// One level's pinning radius for one vertex.
    pub fn pin_radius(&self, lod: usize, vertex: usize) -> Option<f32> {
        self.base
            .bridge
            .as_ref()?
            .pin_radius(lod)?
            .get(vertex)
            .copied()
    }

    /// Whether the simulation may pin this vertex.
    pub fn is_pinnable(&self, vertex: usize) -> bool {
        self.base
            .bridge
            .as_ref()
            .is_some_and(|b| b.is_pinnable(vertex))
    }
}

/// `clothBridgeSimGfx`: per-level runs that tie the simulated cloth to the drawn mesh.
#[record(size = 0x140)]
pub struct ClothBridgeSimGfx {
    pub base: PgBase,
    pub mesh_verts: [i32; LOD_COUNT],
    pub pin_radius_0: Vec<f32>,
    pub pin_radius_1: Vec<f32>,
    pub pin_radius_2: Vec<f32>,
    pub pin_radius_3: Vec<f32>,
    pub vertex_weight_0: Vec<f32>,
    pub vertex_weight_1: Vec<f32>,
    pub vertex_weight_2: Vec<f32>,
    pub vertex_weight_3: Vec<f32>,
    pub inflation_scale_0: Vec<f32>,
    pub inflation_scale_1: Vec<f32>,
    pub inflation_scale_2: Vec<f32>,
    pub inflation_scale_3: Vec<f32>,
    pub cloth_display_map_0: Vec<u16>,
    pub cloth_display_map_1: Vec<u16>,
    pub cloth_display_map_2: Vec<u16>,
    pub cloth_display_map_3: Vec<u16>,
    #[pad_after(4)]
    pad: [u8; 4],
    #[pad_after(8)]
    pub pinnable_list: AtBitSet,
}

macro_rules! per_lod {
    ($($get:ident, $get_mut:ident: $f0:ident $f1:ident $f2:ident $f3:ident;)*) => {
        impl ClothBridgeSimGfx {$(
            /// One level's run; `None` past the last level.
            pub fn $get(&self, lod: usize) -> Option<&Vec<f32>> {
                [&self.$f0, &self.$f1, &self.$f2, &self.$f3].get(lod).copied()
            }

            /// One level's run, to edit; `None` past the last level.
            pub fn $get_mut(&mut self, lod: usize) -> Option<&mut Vec<f32>> {
                [&mut self.$f0, &mut self.$f1, &mut self.$f2, &mut self.$f3]
                    .into_iter()
                    .nth(lod)
            }
        )*}
    };
}

per_lod! {
    pin_radius, pin_radius_mut: pin_radius_0 pin_radius_1 pin_radius_2 pin_radius_3;
    vertex_weight, vertex_weight_mut: vertex_weight_0 vertex_weight_1 vertex_weight_2 vertex_weight_3;
    inflation_scale, inflation_scale_mut: inflation_scale_0 inflation_scale_1 inflation_scale_2 inflation_scale_3;
}

impl ClothBridgeSimGfx {
    /// Whether `pinnable_list` sets this vertex's bit.
    pub fn is_pinnable(&self, vertex: usize) -> bool {
        vertex < usize::from(self.pinnable_list.bit_size)
            && self
                .pinnable_list
                .bits
                .get(vertex / 32)
                .is_some_and(|w| w & (1 << (vertex % 32)) != 0)
    }
}

/// `phMorphController`: up to four morph-map blocks.
#[record(size = 0x40)]
pub struct PhMorphController {
    pub base: PgBase,
    #[pad_after(4)]
    pad: [u8; 4],
    /// ⚠ The engine's `map_data` is one array of four slots, declared a field per slot.
    pub map_data_0: Option<Box<PhMapData>>,
    pub map_data_1: Option<Box<PhMapData>>,
    pub map_data_2: Option<Box<PhMapData>>,
    #[pad_after(8)]
    pub map_data_3: Option<Box<PhMapData>>,
}

/// `phMorphData`: one morph map, a run of weights and four of indices.
#[record(size = 0x50)]
pub struct PhMorphData {
    pub weights: Vec<Vec4V>,
    pub vtx_index: Vec<u16>,
    pub index0: Vec<u16>,
    pub index1: Vec<u16>,
    pub index2: Vec<u16>,
}

/// `phMapData`: four morph maps and four index runs beside them.
#[record(size = 0x188)]
pub struct PhMapData {
    pub m_map_0: PhMorphData,
    pub m_map_1: PhMorphData,
    pub m_map_2: PhMorphData,
    pub m_map_3: PhMorphData,
    pub i_map_0: Vec<u16>,
    pub i_map_1: Vec<u16>,
    pub i_map_2: Vec<u16>,
    pub i_map_3: Vec<u16>,
    #[pad_after(4)]
    pub poly_count: i32,
}

/// `phEdgeData`: an edge between two cloth vertices and how it resists.
#[record(size = 0x10)]
pub struct PhEdgeData {
    pub vert_indices: [u16; 2],
    /// The squared rest length.
    pub edge_length2: f32,
    pub weight0: f32,
    pub compression_weight: f32,
}

/// `phClothData`: a simulated mesh's vertex runs.
#[record(size = 0x68)]
pub struct PhClothData {
    pub base: PgBase,
    pub vertex_pinned_positions: Vec<Vec3V>,
    pub vertex_initial_normals: Vec<Vec3V>,
    pub vertex_positions: Vec<Vec3V>,
    pub vertex_prev_positions: Vec<Vec3V>,
    #[note("zero in all 58 shipped .yld, beside a live position run: filled at load")]
    num_verts: u16,
    #[note("zero in all 58 shipped .yld: filled at load")]
    num_pin_verts: u16,
    padding: [u8; 4],
    pub switch_distance_up: f32,
    pub switch_distance_down: f32,
    #[pad_after(4)]
    pub flags: i32,
}

/// `phVerletCloth`: one simulated level, its mesh, edges and the bounds it collides with.
#[record(size = 0x180, recompute = PhVerletCloth::counts)]
pub struct PhVerletCloth {
    pub base: PgBase,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    verlet_cloth_type: Ptr<Opaque>,
    pub custom_bound: Option<Box<PhBoundAny>>,
    pad007: [u8; 16],
    pub bb_min: Vec3V,
    pub bb_max: Vec3V,
    pub cloth_data: PhClothData,
    pad005: [u8; 20],
    pad00: [u8; 20],
    pub virtual_bound: Option<Box<PhBoundAny>>,
    /// How many vertices are pinned; ⚠ `cloth_data`'s own pinned count is zero on disk.
    pub num_pinned_verts: i32,
    #[note("edge_data's length, in all 58 shipped .yld; written from it")]
    num_edges: i32,
    #[note("cloth_data.vertex_positions' length, in all 58 shipped .yld; written from it")]
    num_vertices: i32,
    pad01: [u8; 4],
    pub iterations: u16,
    pub flags: u16,
    pad006: [u8; 4],
    pub custom_edge_data: Vec<PhEdgeData>,
    pub edge_data: Vec<PhEdgeData>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    ped_bound0: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    ped_bound_matrix0: Ptr<Opaque>,
    pub collision_inst: Vec<u32>,
    pub dynamic_pin_list: AtBitSet,
    pub num_locked_edges_front: i32,
    pub num_locked_edges_back: i32,
    pub cloth_weight: f32,
    pad0: [u8; 4],
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    virtual_bound_mat: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    ped_bound1: Ptr<Opaque>,
    #[note("a runtime pointer, null in all 58 shipped .yld")]
    #[pad_after(8)]
    ped_bound_matrix1: Ptr<Opaque>,
}

impl PhVerletCloth {
    /// `num_edges` and `num_vertices` are the lengths of the runs they count, written from them.
    fn counts(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        let count = |what: &str, len: usize| {
            i32::try_from(len).map_err(|_| OwnedError::Invalid {
                what: format!("PhVerletCloth::{what}"),
                why: format!("{len} elements, and its count is 32 bits"),
            })
        };
        raw.num_edges = count("edge_data", self.edge_data.len())?;
        raw.num_vertices = count(
            "cloth_data.vertex_positions",
            self.cloth_data.vertex_positions.len(),
        )?;
        Ok(())
    }
}

/// A dictionary the game would misread: its order, a bound, a binding or a simulated mesh.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("the dictionary's hashes are not ascending, so a lookup would find the wrong cloth")]
    HashesUnsorted,
    #[error("the dictionary holds {hashes} hashes and {entries} cloths")]
    Lengths { hashes: usize, entries: usize },
    #[error("cloth {hash:#010x}: {why}")]
    Bound { hash: u32, why: bounds::Broken },
    #[error("{at}cloth vertex {vertex} slot {slot} names bone-map entry {bone} of {map}, and it carries weight")]
    Binding {
        at: String,
        vertex: usize,
        slot: usize,
        bone: i32,
        map: usize,
    },
    #[error("{at}the controller maps {indices} bone index/indices and {ids} bone id(s)")]
    BoneMaps {
        at: String,
        indices: usize,
        ids: usize,
    },
    #[error("{at}LOD {lod} states {weights} simulation weight(s) and {inflation} inflation scale(s), and they are one each per vertex")]
    Inflation {
        at: String,
        lod: usize,
        weights: usize,
        inflation: usize,
    },
    #[error("{at}LOD {lod} states {radii} pinning radius/radii over {weights} vertex/vertices, which is not a whole number of sets")]
    PinRadii {
        at: String,
        lod: usize,
        radii: usize,
        weights: usize,
    },
    #[error(
        "{at}{verts} vertex/vertices against {len} {what}, and both runs are written at one count"
    )]
    Run {
        at: String,
        verts: usize,
        len: usize,
        what: &'static str,
    },
    #[error("{at}the cloth data states {stated} vertex/vertices and holds {verts}")]
    VertexCount {
        at: String,
        stated: u16,
        verts: usize,
    },
    #[error("{at}{pinned} pinned vertex/vertices of {verts}")]
    Pinned {
        at: String,
        pinned: i32,
        verts: usize,
    },
    #[error("{at}{what} {index} joins vertices {a} and {b} of {verts}")]
    Edge {
        at: String,
        what: &'static str,
        index: usize,
        a: u16,
        b: u16,
        verts: usize,
    },
    #[error("{at}the level states {stated} {what} and holds {held}")]
    Count {
        at: String,
        what: &'static str,
        stated: i32,
        held: usize,
    },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// The dictionary's order, and every bound, binding and simulated mesh its cloths hold.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let dict = View::<Raw<PgDictionary<CharacterCloth>>>::at_offset(image, root)?;
    let (codes, entries) = (dict.codes()?, dict.entries()?);
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            hashes: codes.len(),
            entries: entries.len(),
        });
    }
    if !PgDictionary::<CharacterCloth>::codes_are_sorted(&codes) {
        return Err(Broken::HashesUnsorted);
    }
    for (i, hash) in codes.iter().enumerate() {
        let Some(cloth) = entries.follow(i)? else {
            continue;
        };
        let bound = |why| Broken::Bound { hash: *hash, why };
        if let Some(b) = cloth.bound_composite()? {
            bounds::check_bound(View::at_offset(image, View::offset(&b))?, "").map_err(bound)?;
        }
        if let Some(controller) = cloth.cloth_controller()? {
            check_controller(&controller, *hash)?;
        }
    }
    Ok(())
}

/// A run's elements, copied out of `image`.
///
/// ⚠ Not a view: the canonical writer places a run straight after a
/// one-byte reservation for an empty run with a live pointer, which every
/// shipped level's `collision_inst` is, so every run it writes after one sits
/// off its alignment.
fn copied<T: Plain>(
    image: Image<'_>,
    run: Array<T>,
    field: &'static str,
) -> Result<Vec<T>, Broken> {
    if run.count() == 0 {
        return Ok(Vec::new());
    }
    let bad = |reason: String| ViewError::BadPointer {
        ty: "CharacterCloth".into(),
        field,
        address: run.raw(),
        reason: reason.into(),
    };
    let size = std::mem::size_of::<T>();
    let at = image.resolve(run.raw()).map_err(|r| bad(r.to_string()))?;
    let bytes = image
        .slice_at(at, run.count() * size)
        .map_err(|r| bad(r.to_string()))?;
    Ok(bytes.chunks_exact(size).filter_map(T::read_from).collect())
}

fn check_controller(c: &View<'_, Raw<CharacterClothController>>, hash: u32) -> Result<(), Broken> {
    let at = format!("{hash:#010x}: ");
    let image = View::image(c);
    // ⚠ A blend index addresses the bone map, which is far shorter than the vertex list.
    let map = { c.bone_index_map }.count();
    let bindings = copied(image, c.binding_info, "binding_info")?;
    for (vertex, binding) in bindings.iter().enumerate() {
        let weights = [
            binding.weights.x,
            binding.weights.y,
            binding.weights.z,
            binding.weights.w,
        ];
        for (slot, bone) in binding.blend_indices.iter().enumerate() {
            if weights[slot] != 0.0 && usize::try_from(*bone).map_or(true, |b| b >= map) {
                return Err(Broken::Binding {
                    at,
                    vertex,
                    slot,
                    bone: *bone,
                    map,
                });
            }
        }
    }
    // ⚠ The two bone maps are two statements about the same bones, and the engine reads both.
    let ids = { c.bone_id_map }.count();
    if ids != 0 && ids != map {
        return Err(Broken::BoneMaps {
            at,
            indices: map,
            ids,
        });
    }
    let base = c.base();
    let levels = [
        base.cloth_0()?,
        base.cloth_1()?,
        base.cloth_2()?,
        base.cloth_3()?,
    ];
    for (lod, level) in levels.into_iter().enumerate() {
        let Some(level) = level else {
            continue;
        };
        for b in [level.custom_bound()?, level.virtual_bound()?]
            .into_iter()
            .flatten()
        {
            bounds::check_bound(b, "").map_err(|why| Broken::Bound { hash, why })?;
        }
        check_level(&level, &format!("{at}LOD {lod}: "))?;
    }
    if let Some(bridge) = base.bridge()? {
        check_bridge(&bridge, &at)?;
    }
    Ok(())
}

fn check_bridge(b: &View<'_, Raw<ClothBridgeSimGfx>>, at: &str) -> Result<(), Broken> {
    let runs = [
        (b.vertex_weight_0, b.inflation_scale_0, b.pin_radius_0),
        (b.vertex_weight_1, b.inflation_scale_1, b.pin_radius_1),
        (b.vertex_weight_2, b.inflation_scale_2, b.pin_radius_2),
        (b.vertex_weight_3, b.inflation_scale_3, b.pin_radius_3),
    ];
    for (lod, (weights, inflation, radii)) in runs.into_iter().enumerate() {
        let (weights, inflation, radii) = (weights.count(), inflation.count(), radii.count());
        // ⚠ Nothing outside the bridge states how long its per-vertex runs
        // are: neither the level's vertex count nor `mesh_verts` does
        // (`cs_amandatownley_uppr_004_u.yld` states 213 against 212 of both).
        // Absent is not short: plenty of shipped cloths leave a run empty.
        if weights != 0 && inflation != 0 && weights != inflation {
            return Err(Broken::Inflation {
                at: at.into(),
                lod,
                weights,
                inflation,
            });
        }
        // ⚠ `pin_radius` is a set per vertex: 1, 2 or 4 times the weight run
        // across the shipped dictionaries (`cs_taocheng_uppr_002_u.yld`
        // doubles it, `player_zero_uppr_000_u.yld` quadruples it).
        if weights != 0 && radii != 0 && radii % weights != 0 {
            return Err(Broken::PinRadii {
                at: at.into(),
                lod,
                radii,
                weights,
            });
        }
    }
    Ok(())
}

fn check_level(level: &View<'_, Raw<PhVerletCloth>>, at: &str) -> Result<(), Broken> {
    let data = level.cloth_data();
    let verts = { data.vertex_positions }.count();
    for (what, len) in [
        (
            "previous position(s)",
            { data.vertex_prev_positions }.count(),
        ),
        ("initial normal(s)", { data.vertex_initial_normals }.count()),
    ] {
        if len != 0 && len != verts {
            return Err(Broken::Run {
                at: at.into(),
                verts,
                len,
                what,
            });
        }
    }
    // ⚠ `num_verts` is zero in every shipped cloth beside a live position
    // run (`a_f_y_beach_01.yld` states 0 against 105): zero or agreeing.
    if data.num_verts != 0 && usize::from(data.num_verts) != verts {
        return Err(Broken::VertexCount {
            at: at.into(),
            stated: data.num_verts,
            verts,
        });
    }
    for pinned in [i32::from(data.num_pin_verts), level.num_pinned_verts] {
        if usize::try_from(pinned).map_or(true, |p| p > verts) {
            return Err(Broken::Pinned {
                at: at.into(),
                pinned,
                verts,
            });
        }
    }
    let image = View::image(level);
    let edges = copied(image, level.edge_data, "edge_data")?;
    for (what, stated, held) in [
        ("edge(s)", level.num_edges, edges.len()),
        ("vertex/vertices", level.num_vertices, verts),
    ] {
        if usize::try_from(stated).ok() != Some(held) {
            return Err(Broken::Count {
                at: at.into(),
                what,
                stated,
                held,
            });
        }
    }
    let custom = copied(image, level.custom_edge_data, "custom_edge_data")?;
    for (what, run) in [("edge", edges), ("custom edge", custom)] {
        if let Some((index, e)) = run
            .iter()
            .enumerate()
            .find(|(_, e)| e.vert_indices.iter().any(|v| usize::from(*v) >= verts))
        {
            return Err(Broken::Edge {
                at: at.into(),
                what,
                index,
                a: e.vert_indices[0],
                b: e.vert_indices[1],
                verts,
            });
        }
    }
    Ok(())
}
