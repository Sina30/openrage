//! The lights a GTA VI map body states: the `CExtensionDefLightEffect` an entity or an archetype
//! holds among its `extensions` and that effect's `CLightAttrDef` records, and a light placed in
//! the world through the entity that owns it or that places its archetype.
//!
//! ⚠ An interior's entities, and those of its entity sets, hold their lights in the interior's own
//! space: such a light stands in the world through the entity's [`Pose`] inside the pose of each
//! `CMloInstanceDef` that places the interior ([`Pose::within`]). A room has a box and no transform.

use super::{
    broken, check_layouts, field, floats, u32_at, u64_at, Blocks, Entity, MapError, Member, Result,
    ARCHETYPE_SIZE, ARRAY, ARRAY_MEMBER, C_BASE_ARCHETYPE_DEF, C_ENTITY_DEF, C_MAP_DATA,
    C_MAP_TYPES, C_MLO_ARCHETYPE_DEF, C_MLO_INSTANCE_DEF, C_TIME_ARCHETYPE_DEF, FLOAT,
    MAP_DATA_SIZE, MAP_TYPES_SIZE, STRING_HASH, U32, VEC3, VEC3V,
};
use crate::hash::literal_string_hash as lit;
use crate::meta::binary::rbm::schema::RbmModel;

pub const C_EXTENSION_DEF_LIGHT_EFFECT: u32 = lit(b"CExtensionDefLightEffect");
pub const C_LIGHT_ATTR_DEF: u32 = lit(b"CLightAttrDef");
const C_MLO_ENTITY_SET: u32 = lit(b"CMloEntitySet");

const U8: u8 = 0x11;
const EFFECT_SIZE: usize = 0x50;
const ATTR_SIZE: usize = 0x2A0;
/// Where an entity and an archetype hold their `extensions`.
const ENTITY_EXTENSIONS: usize = 0x28;
const ARCHETYPE_EXTENSIONS: usize = 0x90;
/// Where a `CMloArchetypeDef` holds its `entities`.
const INTERIOR_ENTITIES: usize = 0xC0;
/// Where a `CMloArchetypeDef` holds its `entitySets`, and a `CMloEntitySet` its `entities`.
const INTERIOR_ENTITY_SETS: usize = 0xF0;
const SET_ENTITIES: usize = 0x18;
const ENTITY_SET_SIZE: usize = 0x28;
/// The bytes of a `CEntityDef` a pose is read from.
const ENTITY_POSE: usize = 0x5C;

const OWNER: &[Member] = &[(lit(b"extensions"), ENTITY_EXTENSIONS as u32, ARRAY)];
const ARCHETYPE: &[Member] = &[(lit(b"extensions"), ARCHETYPE_EXTENSIONS as u32, ARRAY)];
const EFFECT: &[Member] = &[
    (lit(b"name"), 0x08, STRING_HASH),
    (lit(b"offsetPosition"), 0x10, VEC3V),
    (lit(b"instances"), 0x20, ARRAY),
    (0xC4E9_6A00, 0x30, ARRAY),
];
const ATTR: &[Member] = &[
    (0x30EC_A388, 0x120, VEC3),
    (lit(b"posn"), 0x12C, ARRAY_MEMBER),
    (lit(b"intensity"), 0x138, FLOAT),
    (lit(b"flags"), 0x140, U32),
    (lit(b"timeFlags"), 0x144, U32),
    (lit(b"falloff"), 0x148, FLOAT),
    (lit(b"falloffExponent"), 0x14C, FLOAT),
    (lit(b"volIntensity"), 0x154, FLOAT),
    (lit(b"volSizeScale"), 0x158, FLOAT),
    (lit(b"volOuterIntensity"), 0x15C, FLOAT),
    (lit(b"coronaSize"), 0x168, FLOAT),
    (lit(b"volOuterExponent"), 0x16C, FLOAT),
    (lit(b"shadowNearClip"), 0x170, FLOAT),
    (lit(b"coronaIntensity"), 0x174, FLOAT),
    (lit(b"coronaZBias"), 0x178, FLOAT),
    (lit(b"direction"), 0x1A0, ARRAY_MEMBER),
    (lit(b"tangent"), 0x1AC, ARRAY_MEMBER),
    (lit(b"coneInnerAngle"), 0x1B8, FLOAT),
    (lit(b"coneOuterAngle"), 0x1BC, FLOAT),
    (lit(b"extents"), 0x1DC, ARRAY_MEMBER),
    (lit(b"volOuterColour"), 0x282, ARRAY_MEMBER),
    (lit(b"colour"), 0x288, ARRAY_MEMBER),
    (lit(b"flashiness"), 0x28B, U8),
    (lit(b"lightFadeDistance"), 0x28D, U8),
    (lit(b"shadowFadeDistance"), 0x28E, U8),
    (lit(b"specularFadeDistance"), 0x28F, U8),
    (lit(b"volumetricFadeDistance"), 0x290, U8),
    (lit(b"lightType"), 0x291, U8),
    (lit(b"groupId"), 0x292, U8),
    (lit(b"shadowBlur"), 0x293, U8),
    (lit(b"lightHash"), 0x295, U8),
];

/// Each class this reader reads beside the map reader's, its members, and the size of its records.
pub(super) type Layouts = [(u32, &'static str, &'static [Member], Option<u32>)];

const LAYOUTS: &Layouts = &[
    (C_ENTITY_DEF, "CEntityDef", OWNER, None),
    (C_MLO_INSTANCE_DEF, "CMloInstanceDef", OWNER, None),
    (C_BASE_ARCHETYPE_DEF, "CBaseArchetypeDef", ARCHETYPE, None),
    (C_TIME_ARCHETYPE_DEF, "CTimeArchetypeDef", ARCHETYPE, None),
    (
        C_MLO_ARCHETYPE_DEF,
        "CMloArchetypeDef",
        &[
            (lit(b"extensions"), ARCHETYPE_EXTENSIONS as u32, ARRAY),
            (lit(b"entities"), INTERIOR_ENTITIES as u32, ARRAY),
            (lit(b"entitySets"), INTERIOR_ENTITY_SETS as u32, ARRAY),
        ],
        None,
    ),
    (
        C_MLO_ENTITY_SET,
        "CMloEntitySet",
        &[(lit(b"entities"), SET_ENTITIES as u32, ARRAY)],
        Some(ENTITY_SET_SIZE as u32),
    ),
    (
        C_EXTENSION_DEF_LIGHT_EFFECT,
        "CExtensionDefLightEffect",
        EFFECT,
        Some(EFFECT_SIZE as u32),
    ),
    (
        C_LIGHT_ATTR_DEF,
        "CLightAttrDef",
        ATTR,
        Some(ATTR_SIZE as u32),
    ),
];

/// Refuse a body whose class table declares a class of `layouts` at another size, or a member of it
/// at another offset or type.
pub(super) fn check_table(model: &RbmModel, layouts: &'static Layouts) -> Result<()> {
    for (hash, class, members, size) in layouts {
        let Some(declared) = model.classes.get(hash) else {
            continue;
        };
        if let Some(expected) = size.filter(|s| *s != declared.data_size) {
            return Err(MapError::Size {
                class,
                found: declared.data_size,
                expected,
            });
        }
        for &(member, offset, code) in *members {
            let fits = (declared.entries.iter()).any(|e| {
                e.name_hash == member && e.offset == offset && e.data_type.to_byte() == code
            });
            if !fits {
                return Err(MapError::Layout {
                    class,
                    member,
                    offset,
                    code,
                });
            }
        }
    }
    Ok(())
}

/// A `CLightAttrDef`: one light, in the space of what owns it.
#[derive(Debug, Clone, PartialEq)]
pub struct LightAttr {
    /// A second point (`hash_30ECA388`); what it is beside `posn` is not known.
    pub hash_30eca388: [f32; 3],
    pub posn: [f32; 3],
    pub colour: [u8; 3],
    pub vol_outer_colour: [u8; 3],
    pub intensity: f32,
    pub flags: u32,
    /// One bit an hour of the day, bit 0 the first.
    pub time_flags: u32,
    /// The light's range, in metres.
    pub falloff: f32,
    pub falloff_exponent: f32,
    pub vol_intensity: f32,
    pub vol_size_scale: f32,
    pub vol_outer_intensity: f32,
    pub vol_outer_exponent: f32,
    pub corona_size: f32,
    pub corona_intensity: f32,
    pub corona_z_bias: f32,
    pub shadow_near_clip: f32,
    /// A unit vector.
    pub direction: [f32; 3],
    pub tangent: [f32; 3],
    /// In degrees.
    pub cone_inner_angle: f32,
    pub cone_outer_angle: f32,
    pub extents: [f32; 3],
    pub flashiness: u8,
    pub light_fade_distance: u8,
    pub shadow_fade_distance: u8,
    pub specular_fade_distance: u8,
    pub volumetric_fade_distance: u8,
    /// See [`LightAttr::light_type_name`].
    pub light_type: u8,
    pub group_id: u8,
    pub shadow_blur: u8,
    pub light_hash: u8,
}

impl LightAttr {
    /// What `light_type` names: 1 a point, 2 a spot, 4 a capsule; `None` for any other value.
    pub fn light_type_name(&self) -> Option<&'static str> {
        match self.light_type {
            1 => Some("point"),
            2 => Some("spot"),
            4 => Some("capsule"),
            _ => None,
        }
    }

    /// Whether the light is lit in the hour `hour` of the day, 0 to 23.
    pub fn lit_at(&self, hour: u8) -> bool {
        hour < 24 && self.time_flags >> hour & 1 == 1
    }
}

/// A `CExtensionDefLightEffect`: the lights of one entity or archetype.
#[derive(Debug, Clone, PartialEq)]
pub struct LightEffect {
    pub name: u32,
    /// Added to each light's `posn`.
    pub offset_position: [f32; 3],
    pub instances: Vec<LightAttr>,
    /// A second list of lights (`hash_C4E96A00`); what sets it apart is not known.
    pub hash_c4e96a00: Vec<LightAttr>,
}

impl LightEffect {
    /// Every light of both lists.
    pub fn lights(&self) -> impl Iterator<Item = &LightAttr> {
        self.instances.iter().chain(&self.hash_c4e96a00)
    }
}

/// Where a model stands in the space that holds it: its scale, then its turn, then its position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub position: [f32; 3],
    /// The quaternion that turns the model, x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
}

impl Pose {
    /// The pose of an entity of a map.
    pub fn of(entity: &Entity) -> Self {
        Self {
            position: entity.position,
            rotation: entity.model_rotation(),
            scale_xy: entity.scale_xy,
            scale_z: entity.scale_z,
        }
    }

    /// The pose a `CEntityDef` record states: it stores the conjugate of its model's turn.
    fn of_record(e: &[u8], what: &str) -> Result<Self> {
        if e.len() < ENTITY_POSE {
            return Err(broken(what, "it runs past its block"));
        }
        let [x, y, z, w]: [f32; 4] = floats(e, 0x10, what)?;
        Ok(Self {
            position: floats(e, 0x38, what)?,
            rotation: [-x, -y, -z, w],
            scale_xy: super::f32_at(e, 0x54, what)?,
            scale_z: super::f32_at(e, 0x58, what)?,
        })
    }

    /// The point `v` of the model's space, in the space the pose stands in.
    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let scale = [self.scale_xy, self.scale_xy, self.scale_z];
        let turned = rotate(self.rotation, [0, 1, 2].map(|i| v[i] * scale[i]));
        [0, 1, 2].map(|i| self.position[i] + turned[i])
    }

    /// This pose, of an interior's space, in the space `outer` stands in: `outer` is the pose of
    /// the entity that places the interior.
    // ponytail: the scales multiply, which is exact while the outer scale is uniform, as an interior instance's is; compose matrices if one ever is not.
    pub fn within(&self, outer: &Pose) -> Pose {
        let ([x1, y1, z1, w1], [x2, y2, z2, w2]) = (outer.rotation, self.rotation);
        Pose {
            position: outer.apply(self.position),
            rotation: [
                w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
                w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
                w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
                w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2,
            ],
            scale_xy: self.scale_xy * outer.scale_xy,
            scale_z: self.scale_z * outer.scale_z,
        }
    }

    /// `light` of `effect` in the space the pose stands in.
    pub fn place(&self, effect: &LightEffect, light: &LightAttr) -> PlacedLight {
        let local = [0, 1, 2].map(|i| effect.offset_position[i] + light.posn[i]);
        PlacedLight {
            position: self.apply(local),
            direction: rotate(self.rotation, light.direction),
        }
    }
}

/// What holds a light effect, by its index in the body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightOwner {
    /// An entity of a `CMapData`: its lights are its own, in its model's space.
    Entity(usize),
    /// An archetype of a `CMapTypes`: its lights stand once at each entity that places it.
    Archetype(usize),
    /// An entity of an interior archetype of a `CMapTypes`, at `pose` in the interior's space.
    InteriorEntity {
        archetype: usize,
        entity: usize,
        pose: Pose,
    },
    /// An entity of entity set `set` of an interior archetype, at `pose` in the interior's space.
    EntitySetEntity {
        archetype: usize,
        set: usize,
        entity: usize,
        pose: Pose,
    },
}

/// The light effects of a map body, in the order of their owners.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapLights {
    pub effects: Vec<(LightOwner, LightEffect)>,
    /// The extensions of other classes the owners hold, which this does not read.
    pub other_extensions: usize,
}

impl MapLights {
    /// How many lights the effects hold.
    pub fn lights(&self) -> usize {
        self.effects.iter().map(|(_, e)| e.lights().count()).sum()
    }
}

/// The lights of the map body a headerless RBM resource holds, from its two inflated sections.
pub fn open(virtual_section: &[u8], physical: &[u8]) -> Result<MapLights> {
    read(&crate::meta::binary::rbm::gta6::read_model(
        virtual_section,
        physical,
    )?)
}

/// The lights of the map body of a headerless RBM resource's class table and data blocks.
pub fn read(model: &RbmModel) -> Result<MapLights> {
    check_layouts(model)?;
    check_table(model, LAYOUTS)?;
    let blocks = Blocks::new(&model.data_blocks);
    let root = blocks.block(u64::from(model.root_block_id), "the root block")?;
    let mut out = MapLights::default();
    match root.data_type {
        C_MAP_DATA if root.data.len() >= MAP_DATA_SIZE => {
            let entities = pointers(&blocks, &root.data, 0xD0, "entities")?;
            for (k, (class, e)) in entities.into_iter().enumerate() {
                let what = format!("entities[{k}]");
                if ![C_ENTITY_DEF, C_MLO_INSTANCE_DEF].contains(&class) {
                    return Err(not_of(what, class, "entity"));
                }
                let owner = LightOwner::Entity(k);
                effects(&blocks, e, ENTITY_EXTENSIONS, &what, owner, &mut out)?;
            }
        }
        C_MAP_TYPES if root.data.len() >= MAP_TYPES_SIZE => {
            let archetypes = pointers(&blocks, &root.data, 0x18, "archetypes")?;
            for (k, (class, a)) in archetypes.into_iter().enumerate() {
                let what = format!("archetypes[{k}]");
                if ![
                    C_BASE_ARCHETYPE_DEF,
                    C_TIME_ARCHETYPE_DEF,
                    C_MLO_ARCHETYPE_DEF,
                ]
                .contains(&class)
                {
                    return Err(not_of(what, class, "archetype"));
                }
                if a.len() < ARCHETYPE_SIZE {
                    return Err(broken(what, "it runs past its block"));
                }
                let owner = LightOwner::Archetype(k);
                effects(&blocks, a, ARCHETYPE_EXTENSIONS, &what, owner, &mut out)?;
                if class != C_MLO_ARCHETYPE_DEF {
                    continue;
                }
                let inside = format!("{what}.entities");
                for (j, e) in inside_entities(&blocks, a, INTERIOR_ENTITIES, &inside)? {
                    let what = format!("{inside}[{j}]");
                    let owner = LightOwner::InteriorEntity {
                        archetype: k,
                        entity: j,
                        pose: Pose::of_record(e, &what)?,
                    };
                    effects(&blocks, e, ENTITY_EXTENSIONS, &what, owner, &mut out)?;
                }
                if a.len() < INTERIOR_ENTITY_SETS + 0x10 {
                    return Err(broken(what, "it runs past its block"));
                }
                let sets = format!("{what}.entitySets");
                let records = blocks.records(
                    a,
                    INTERIOR_ENTITY_SETS,
                    C_MLO_ENTITY_SET,
                    ENTITY_SET_SIZE,
                    &sets,
                )?;
                for (set, record) in records.into_iter().enumerate() {
                    let inside = format!("{sets}[{set}].entities");
                    for (j, e) in inside_entities(&blocks, record, SET_ENTITIES, &inside)? {
                        let what = format!("{inside}[{j}]");
                        let owner = LightOwner::EntitySetEntity {
                            archetype: k,
                            set,
                            entity: j,
                            pose: Pose::of_record(e, &what)?,
                        };
                        effects(&blocks, e, ENTITY_EXTENSIONS, &what, owner, &mut out)?;
                    }
                }
            }
        }
        C_MAP_DATA | C_MAP_TYPES => {
            return Err(broken("the root block", "it is shorter than its class"))
        }
        other => return Err(MapError::NotAMap(other)),
    }
    Ok(out)
}

fn not_of(what: String, class: u32, wanted: &str) -> MapError {
    broken(
        what,
        format!("it points at a block of type {class:#010X}, no {wanted} class"),
    )
}

/// The `CEntityDef` records an interior or one of its entity sets names at `at`, each with its index.
fn inside_entities<'m>(
    blocks: &Blocks<'m>,
    bytes: &[u8],
    at: usize,
    what: &str,
) -> Result<Vec<(usize, &'m [u8])>> {
    (pointers(blocks, bytes, at, what)?.into_iter().enumerate())
        .map(|(j, (class, e))| match class {
            C_ENTITY_DEF => Ok((j, e)),
            other => Err(not_of(format!("{what}[{j}]"), other, "CEntityDef")),
        })
        .collect()
}

/// The record each element of an array of pointers at `at` names, with its class, to its block's end.
fn pointers<'m>(
    blocks: &Blocks<'m>,
    bytes: &[u8],
    at: usize,
    what: &str,
) -> Result<Vec<(u32, &'m [u8])>> {
    let Some((_, rest, count)) = blocks.array(bytes, at, what)? else {
        return Ok(Vec::new());
    };
    if rest.len() < 8 * count {
        return Err(broken(
            what,
            format!("its {count} pointers run past their block"),
        ));
    }
    (0..count)
        .map(|k| {
            let what = format!("{what}[{k}]");
            (blocks.target(u64_at(rest, 8 * k)?, &what)?)
                .ok_or_else(|| broken(what, "it is a null pointer"))
        })
        .collect()
}

/// Add the light effects among the extensions `record` holds at `at`, and count the others.
fn effects(
    blocks: &Blocks,
    record: &[u8],
    at: usize,
    what: &str,
    owner: LightOwner,
    out: &mut MapLights,
) -> Result<()> {
    let extensions = pointers(blocks, record, at, &format!("{what}.extensions"))?;
    for (k, (class, e)) in extensions.into_iter().enumerate() {
        if class != C_EXTENSION_DEF_LIGHT_EFFECT {
            out.other_extensions += 1;
            continue;
        }
        let what = format!("{what}.extensions[{k}]");
        if e.len() < EFFECT_SIZE {
            return Err(broken(what, "it runs past its block"));
        }
        let list = |at: usize, name: &str| -> Result<Vec<LightAttr>> {
            let what = format!("{what}.{name}");
            (blocks.records(e, at, C_LIGHT_ATTR_DEF, ATTR_SIZE, &what)?)
                .into_iter()
                .enumerate()
                .map(|(i, l)| light(l, &format!("{what}[{i}]")))
                .collect()
        };
        let effect = LightEffect {
            name: u32_at(e, 0x08)?,
            offset_position: floats(e, 0x10, &what)?,
            instances: list(0x20, "instances")?,
            hash_c4e96a00: list(0x30, "hash_C4E96A00")?,
        };
        out.effects.push((owner, effect));
    }
    Ok(())
}

fn light(l: &[u8], what: &str) -> Result<LightAttr> {
    let float = |at: usize| super::f32_at(l, at, what);
    let byte = |at: usize| field::<1>(l, at).map(|b| b[0]);
    let read = LightAttr {
        hash_30eca388: floats(l, 0x120, what)?,
        posn: floats(l, 0x12C, what)?,
        colour: field(l, 0x288)?,
        vol_outer_colour: field(l, 0x282)?,
        intensity: float(0x138)?,
        flags: u32_at(l, 0x140)?,
        time_flags: u32_at(l, 0x144)?,
        falloff: float(0x148)?,
        falloff_exponent: float(0x14C)?,
        vol_intensity: float(0x154)?,
        vol_size_scale: float(0x158)?,
        vol_outer_intensity: float(0x15C)?,
        vol_outer_exponent: float(0x16C)?,
        corona_size: float(0x168)?,
        corona_intensity: float(0x174)?,
        corona_z_bias: float(0x178)?,
        shadow_near_clip: float(0x170)?,
        direction: floats(l, 0x1A0, what)?,
        tangent: floats(l, 0x1AC, what)?,
        cone_inner_angle: float(0x1B8)?,
        cone_outer_angle: float(0x1BC)?,
        extents: floats(l, 0x1DC, what)?,
        flashiness: byte(0x28B)?,
        light_fade_distance: byte(0x28D)?,
        shadow_fade_distance: byte(0x28E)?,
        specular_fade_distance: byte(0x28F)?,
        volumetric_fade_distance: byte(0x290)?,
        light_type: byte(0x291)?,
        group_id: byte(0x292)?,
        shadow_blur: byte(0x293)?,
        light_hash: byte(0x295)?,
    };
    let norm = read.direction.iter().map(|x| x * x).sum::<f32>().sqrt();
    if (norm - 1.0).abs() >= 1e-3 {
        return Err(broken(
            what,
            format!("its direction's norm is {norm:.5}, not 1"),
        ));
    }
    if read.time_flags >> 24 != 0 {
        return Err(broken(
            what,
            format!(
                "its timeFlags {:#X} set a bit past the 24 hours",
                read.time_flags
            ),
        ));
    }
    Ok(read)
}

/// `v` turned by the unit quaternion `q`, stored x, y, z, w.
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [q[0], q[1], q[2]];
    let t = cross(u, v).map(|x| 2.0 * x);
    let c = cross(u, t);
    [0, 1, 2].map(|i| v[i] + q[3] * t[i] + c[i])
}

/// A light in the world: where it stands and where it points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedLight {
    pub position: [f32; 3],
    pub direction: [f32; 3],
}

/// `light` of `effect` in the world through `entity`, which owns the effect or places the
/// archetype that does: the entity's scale, then the turn of its model, then its position.
pub fn place(effect: &LightEffect, light: &LightAttr, entity: &Entity) -> PlacedLight {
    Pose::of(entity).place(effect, light)
}

#[cfg(test)]
#[path = "map_lights_tests.rs"]
pub(super) mod tests;
