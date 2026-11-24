//! What a particle list states beyond the fields of its records: the evolved keyframes, the shader
//! words, the texture slots, the frames, and the behaviours no keyframed value classes.

use super::*;

/// The ids of `ptxu_Colour`'s first two unnamed keyframed values, read as its minimum and maximum.
pub const COLOUR_RGBA_KFPS: [u32; 2] = [UNNAMED_KFPS[0].0, UNNAMED_KFPS[1].0];

/// The texture slots a sprite is drawn from, the first set first.
const SPRITE_SLOTS: [usize; 4] = [0, 6, 5, 12];

/// The ids of the keyframed value of the behaviours labelled by it.
const UNCLASSED: [u32; 3] = [0x89DC_E430, 0xDE74_8AF4, 0xFA18_3E19];
const AGE_BIT: u8 = 1;
const SPRITE_BIT: u8 = 27;
const SPRITE_SIZE: u64 = 0x50;
/// The highest alignment word a sprite rule's last behaviour holds as `ptxd_Sprite`.
const SPRITE_ALIGNMENTS: u32 = 11;

/// One evolution's keyframe of an evolved value.
#[derive(Debug, Clone, PartialEq)]
pub struct EvolvedKeyframe {
    pub entries: Vec<KfpEntry>,
    /// An index into its list's evolutions.
    pub evolution: u32,
    /// The evolution's source: 1 the LOD distance, 2 to 4 the camera's forward, top and side views.
    pub kind: u8,
    /// The bytes at +0x25 and +0x26, each 0 or 1, whose meaning is not known.
    pub flags: [u8; 2],
}

/// What a behaviour's own words state, or the class its place, size or id gives it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BehaviourFields {
    #[default]
    None,
    /// `ptxu_AnimateTexture`'s words after its keyframed value.
    AnimateTexture {
        keyframe_mode: i32,
        /// The atlas's last frame id; -1 reads as 0.
        last_frame_id: i32,
        loop_mode: i32,
        flags: [u8; 4],
    },
    /// `ptxu_ZCull`'s words after its keyframed values.
    ZCull {
        /// 0 to 3 are GTA V's cull and clamp modes; what 4 and 5 select is not known.
        mode: i32,
        /// GTA V's reference spaces.
        reference_space: i32,
    },
    /// `ptxu_Colour`'s word at +0x19C, read as its count of colours in use, 0 to 5.
    Colour { colours_used: u32 },
    /// `ptxu_Age` by its class bit.
    Age,
    /// `ptxd_Sprite` by its class bit.
    Sprite {
        /// Flip chance u and v.
        flip_chance: [f32; 2],
        /// 0 to 4 are GTA V's alignment modes; what 5 to 11 select is not known.
        alignment: u32,
        /// The bytes at +0x38, whose order is not known.
        bytes: [u8; 16],
    },
    /// The id of the keyframed value a behaviour of no named class holds at +0x08.
    Unclassed(u32),
}

/// A rule's `lightingmode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingMode {
    DiffuseOnly,
    NormalSpec,
    Unlit,
    Normal,
    Lightmap,
    Hash6F2262CB,
}

/// A rule's `projectionmode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionMode {
    None,
    Water,
    HashDD89CA9B,
    Hash56231EFB,
    Hash45835226,
}

/// A rule's `diffusemode`: whether the sprite's alpha is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffuseMode {
    Rgba,
    Rgb,
}

/// A shader word that is set by being held, with the value 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderSwitch {
    Soft,
    UvDistortion,
    Refract,
    Trail,
    ScreenSpace,
    HashC391FD51,
    Hash808D795F,
}

/// What a rule's shader words select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shading {
    pub lighting: LightingMode,
    pub projection: ProjectionMode,
    pub diffuse: DiffuseMode,
    /// In the rule's order; a switch held twice is listed twice.
    pub switches: Vec<ShaderSwitch>,
}

type Table<T, const N: usize> = [(u32, T, &'static str); N];

const fn word<T>(name: &'static str, value: T) -> (u32, T, &'static str) {
    (name_hash32(name.as_bytes()), value, name)
}

const LIGHTING_MODE: u32 = name_hash32(b"lightingmode");
const PROJECTION_MODE: u32 = name_hash32(b"projectionmode");
const DIFFUSE_MODE: u32 = name_hash32(b"diffusemode");

const LIGHTING: Table<LightingMode, 6> = [
    word("diffuseonly", LightingMode::DiffuseOnly),
    word("normalspec", LightingMode::NormalSpec),
    word("unlit", LightingMode::Unlit),
    word("normal", LightingMode::Normal),
    word("lightmap", LightingMode::Lightmap),
    (0x6F22_62CB, LightingMode::Hash6F2262CB, "hash_6F2262CB"),
];

const PROJECTION: Table<ProjectionMode, 5> = [
    word("none", ProjectionMode::None),
    word("water", ProjectionMode::Water),
    (0xDD89_CA9B, ProjectionMode::HashDD89CA9B, "hash_DD89CA9B"),
    (0x5623_1EFB, ProjectionMode::Hash56231EFB, "hash_56231EFB"),
    (0x4583_5226, ProjectionMode::Hash45835226, "hash_45835226"),
];

const DIFFUSE: Table<DiffuseMode, 2> = [
    word("rgba", DiffuseMode::Rgba),
    word("rgb", DiffuseMode::Rgb),
];

const SWITCHES: Table<ShaderSwitch, 7> = [
    word("soft", ShaderSwitch::Soft),
    word("uvdistortion", ShaderSwitch::UvDistortion),
    word("refract", ShaderSwitch::Refract),
    word("trail", ShaderSwitch::Trail),
    word("screenspace", ShaderSwitch::ScreenSpace),
    (0xC391_FD51, ShaderSwitch::HashC391FD51, "hash_C391FD51"),
    (0x808D_795F, ShaderSwitch::Hash808D795F, "hash_808D795F"),
];

fn of<T: Copy, const N: usize>(table: &Table<T, N>, hash: u32) -> Option<T> {
    let (_, value, _) = table.iter().find(|(h, _, _)| *h == hash)?;
    Some(*value)
}

fn name_of<T: PartialEq, const N: usize>(table: &Table<T, N>, value: &T) -> &'static str {
    match table.iter().find(|(_, v, _)| v == value) {
        Some(&(_, _, name)) => name,
        None => "",
    }
}

impl LightingMode {
    pub fn name(self) -> &'static str {
        name_of(&LIGHTING, &self)
    }
}

impl ProjectionMode {
    pub fn name(self) -> &'static str {
        name_of(&PROJECTION, &self)
    }
}

impl DiffuseMode {
    pub fn name(self) -> &'static str {
        name_of(&DIFFUSE, &self)
    }
}

impl ShaderSwitch {
    pub fn name(self) -> &'static str {
        name_of(&SWITCHES, &self)
    }
}

/// `slot` given `value`, refused where it is held already or `value` is none of those measured.
fn set<T>(slot: &mut Option<T>, value: Option<T>) -> Result<()> {
    need(slot.is_none(), "a shader enumeration is held twice")?;
    match value {
        Some(value) => {
            *slot = Some(value);
            Ok(())
        }
        None => refuse("a shader enumeration holds a value none of those measured"),
    }
}

impl ParticleRule {
    /// The texture slot its sprite is drawn from: the first of 0, 6, 5 and 12 it sets.
    pub fn sprite_slot(&self) -> Option<usize> {
        SPRITE_SLOTS
            .into_iter()
            .find(|&slot| self.texture_keys[slot] != 0)
    }

    /// What its shader words select; refused where a word is not one measured or breaks a rule.
    pub fn shading(&self) -> Result<Shading> {
        let (mut lighting, mut projection, mut diffuse) = (None, None, None);
        let mut switches = Vec::new();
        for &(variable, value) in &self.shader_words {
            match variable {
                LIGHTING_MODE => set(&mut lighting, of(&LIGHTING, value))?,
                PROJECTION_MODE => set(&mut projection, of(&PROJECTION, value))?,
                DIFFUSE_MODE => set(&mut diffuse, of(&DIFFUSE, value))?,
                _ => {
                    let Some(switch) = of(&SWITCHES, variable) else {
                        return refuse("a shader word's variable is none of the ten measured");
                    };
                    need(value == 0, "a shader switch holds a value")?;
                    switches.push(switch);
                }
            }
        }
        need(
            switches.contains(&ShaderSwitch::Trail) == (self.draw_type == 2),
            "a rule's trail switch and its draw type disagree",
        )?;
        match (lighting, projection, diffuse) {
            (Some(lighting), Some(projection), Some(diffuse)) => Ok(Shading {
                lighting,
                projection,
                diffuse,
                switches,
            }),
            _ => refuse("a rule lacks its lightingmode, projectionmode or diffusemode"),
        }
    }
}

impl ParticleEffects {
    /// The frames `rule`'s atlas holds, by its `ptxu_AnimateTexture`'s last frame, else its own.
    pub fn frame_count(&self, rule: &ParticleRule) -> u32 {
        let mut last = rule.tex_frame_id[1];
        let held = |b: &usize| self.behaviours.get(*b);
        for b in rule.behaviours.iter().filter_map(held) {
            if let BehaviourFields::AnimateTexture { last_frame_id, .. } = b.fields {
                last = last_frame_id;
                break;
            }
        }
        u32::try_from(last).unwrap_or(0).saturating_add(1)
    }
}

/// Refused where `rule` sets one texture slot of a pair alone, or its frame range is not one.
pub(super) fn checked(rule: &ParticleRule) -> Result<()> {
    let held = |slot: usize| rule.texture_keys[slot] != 0;
    need(
        held(2) == held(3) && held(6) == held(7),
        "a rule sets texture slot 2, 3, 6 or 7 without its pair",
    )?;
    let [min, max] = rule.tex_frame_id;
    need(
        (0..=max).contains(&min),
        "a rule's first frame id is below 0 or above its last",
    )?;
    rule.shading().map(|_| ())
}

/// The evolved keyframes of the prop at `at`, of a list whose evolutions are `evolutions`.
pub(super) fn evolved_keyframes(
    v: &Bytes,
    at: usize,
    evolutions: &[Evolution],
) -> Result<Vec<EvolvedKeyframe>> {
    const WHAT: &str = "evolved keyframe";
    let (to, count, _) = array(v, WHAT, at, true)?;
    v.array(WHAT, to, count, 0x30)?;
    let mut out: Vec<EvolvedKeyframe> = Vec::with_capacity(count);
    for i in 0..count {
        let e = to + 0x30 * i;
        need(
            zero(v, WHAT, e + 0x10, 0x10)? && zero(v, WHAT, e + 0x27, 9)?,
            "an evolved keyframe's runtime or pad words are not 0",
        )?;
        let index = v.u32(WHAT, e + 0x20)?;
        let after = out.last().is_none_or(|last| index > last.evolution);
        let Some(evolution) = evolutions.get(index as usize).filter(|_| after) else {
            return refuse(
                "an evolved keyframe's evolution is not above the one before and in its list",
            );
        };
        let [kind, a, b] = u8s(v, WHAT, e + 0x24)?;
        need(
            kind == kind_of(&evolution.name),
            "an evolved keyframe's kind is not its evolution's: 1 on lod, 2 to 4 on camForwardView, camTopView and camSideView, else 0",
        )?;
        need(
            a <= 1 && b <= 1,
            "an evolved keyframe's flags are not 0 or 1",
        )?;
        out.push(EvolvedKeyframe {
            entries: evolved_entries(v, e)?,
            evolution: index,
            kind,
            flags: [a, b],
        });
    }
    Ok(out)
}

/// The kind an evolution's name gives its keyframes: the source of its level, the LOD distance or
/// one of the camera's three views, else 0 for a level set by hand.
fn kind_of(name: &str) -> u8 {
    match name.to_ascii_lowercase().as_str() {
        "lod" => 1,
        "camforwardview" => 2,
        "camtopview" => 3,
        "camsideview" => 4,
        _ => 0,
    }
}

/// The keys of the array at `at`, refused where their times break the key rules; a value lane
/// that is not finite is read as it is.
fn evolved_entries(v: &Bytes, at: usize) -> Result<Vec<KfpEntry>> {
    let (to, count, _) = array(v, "evolved keyframe", at, true)?;
    let raw = v.array("evolved keyframe entries", to, count, 0x20)?;
    let mut entries: Vec<KfpEntry> = Vec::with_capacity(count);
    for e in raw.as_chunks::<0x20>().0 {
        let f: [f32; 8] = std::array::from_fn(|i| f32_at(e, 4 * i));
        let time = f[..4].iter().all(|x| x.is_finite())
            && (0.0..=1.0).contains(&f[0])
            && f[2] == 0.0
            && f[3] == 0.0;
        let order = match entries.last() {
            None => f[1] == 0.0,
            Some(last) => f[0] >= last.time[0],
        };
        need(time && order, "an evolved key's time breaks the key rules")?;
        entries.push(KfpEntry {
            time: [f[0], f[1], f[2], f[3]],
            value: [f[4], f[5], f[6], f[7]],
        });
    }
    Ok(entries)
}

/// `ptxu_AnimateTexture`'s words of the behaviour at `at`, which runs to `next`.
pub(super) fn animate_texture(v: &Bytes, at: usize, next: usize) -> Result<BehaviourFields> {
    const WHAT: &str = "ptxu_AnimateTexture";
    need(
        next >= at + 0x58,
        "a ptxu_AnimateTexture is shorter than its words",
    )?;
    let last_frame_id = v.u32(WHAT, at + 0x4C)? as i32;
    need(
        last_frame_id >= -1,
        "a ptxu_AnimateTexture's last frame id is below -1",
    )?;
    Ok(BehaviourFields::AnimateTexture {
        keyframe_mode: v.u32(WHAT, at + 0x48)? as i32,
        last_frame_id,
        loop_mode: v.u32(WHAT, at + 0x50)? as i32,
        flags: u8s(v, WHAT, at + 0x54)?,
    })
}

/// The id of a keyframed value at `at` whose id is its base id, as a class's first value is found.
fn kfp_id(v: &Bytes, at: usize) -> Result<Option<u32>> {
    const WHAT: &str = "behaviour";
    if at + KFP > v.len() {
        return Ok(None);
    }
    let held =
        v.u64(WHAT, at)? == 0 && zero(v, WHAT, at + 0x18, 0x10)? && zero(v, WHAT, at + 0x30, 0x10)?;
    let (id, base) = (v.u32(WHAT, at + 0x28)?, v.u32(WHAT, at + 0x2C)?);
    Ok((held && id != 0 && id == base).then_some(id))
}

/// The words of a sprite rule's last behaviour as `ptxd_Sprite`'s, where its zero words hold and
/// its alignment is one measured.
fn sprite(v: &Bytes, at: usize) -> Result<Option<BehaviourFields>> {
    const WHAT: &str = "behaviour";
    let flip_chance = floats(v, WHAT, at + 8)?;
    let alignment = v.u32(WHAT, at + 0x1C)?;
    let bytes = u8s(v, WHAT, at + 0x38)?;
    let zeros = zero(v, WHAT, at + 0x30, 8)? && zero(v, WHAT, at + 0x48, 4)?;
    if !zeros || alignment > SPRITE_ALIGNMENTS {
        return Ok(None);
    }
    Ok(Some(BehaviourFields::Sprite {
        flip_chance,
        alignment,
        bytes,
    }))
}

/// Each behaviour no keyframed value classes, labelled by the id of the value it holds, else by
/// its class bit.
pub(super) fn label(v: &Bytes, behaviours: &mut [Behaviour]) -> Result<()> {
    for held in behaviours {
        if held.class.is_some() || held.fields != BehaviourFields::None {
            continue;
        }
        let at = held.at as usize;
        let fields = match kfp_id(v, at + 8)? {
            Some(id) if UNCLASSED.contains(&id) => Some(BehaviourFields::Unclassed(id)),
            Some(_) => None,
            None if held.class_bit == Some(AGE_BIT) => Some(BehaviourFields::Age),
            None if held.class_bit == Some(SPRITE_BIT) && held.size >= SPRITE_SIZE => {
                sprite(v, at)?
            }
            None => None,
        };
        if let Some(fields) = fields {
            held.fields = fields;
        }
    }
    Ok(())
}

/// Refused where an event's evolved value is no keyframed value of the list's emitter rules,
/// domains or behaviours.
pub(super) fn event_props_held(
    v: &Bytes,
    effects: &[EffectRule],
    emitters: &[EmitterRule],
    behaviours: &[Behaviour],
) -> Result<()> {
    let mut props = (effects.iter().flat_map(|e| &e.events))
        .filter_map(|ev| ev.evolution_list.as_ref())
        .flat_map(|l| &l.props)
        .peekable();
    if props.peek().is_none() {
        return Ok(());
    }
    let mut ids = HashSet::new();
    for e in emitters {
        ids.extend(e.kfps.iter().map(|k| k.id));
        let domains = e.domains.iter().flatten();
        ids.extend(domains.flat_map(|d| d.kfps.iter().map(|k| k.id)));
    }
    for b in behaviours {
        let at = b.at as usize;
        let end = at + (b.size as usize).min(BEHAVIOUR_REACH);
        let mut o = at + 8;
        while o + KFP <= end {
            match kfp_id(v, o)? {
                Some(id) => {
                    ids.insert(id);
                    o += KFP;
                }
                None => o += 8,
            }
        }
    }
    for p in props {
        need(
            ids.contains(&p.id),
            "an event's evolved value is no keyframed value of the list",
        )?;
    }
    Ok(())
}
