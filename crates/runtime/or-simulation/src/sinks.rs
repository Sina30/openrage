use glam::{Vec3, Vec4};
#[derive(Debug, Clone, Copy)]
pub struct FireInfo {
    pub pos: Vec3,
    pub dir: Vec3,
    pub scale: f32,
    pub colour: Vec4,
    pub life_seconds: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct DecalInfo {
    pub pos: Vec3,
    pub normal: Vec3,
    pub size: Vec3,
    pub uv_mult: Vec4,
    pub colour: Vec4,
    pub life_seconds: f32,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub priority: u8,
    pub disallow_duplicates: bool,
    pub material_id: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct DecalPoolInfo {
    pub pos: Vec3,
    pub normal: Vec3,
    pub radius: f32,
    pub colour: Vec4,
    pub life_seconds: f32,
    pub material_id: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct DecalTrailInfo {
    pub start: Vec3,
    pub end: Vec3,
    pub width: f32,
    pub colour: Vec4,
    pub life_seconds: f32,
    pub material_id: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct LiquidInfo {
    pub pos: Vec3,
    pub normal: Vec3,
    pub size: Vec3,
    pub colour: Vec4,
    pub material_id: u8,
}

/// What lights a fog volume's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogLighting {
    /// The timecycle's fog brightness.
    #[default]
    FogHdr,
    /// The sun's colour and how far overhead it stands.
    Directional,
    None,
}

impl FogLighting {
    /// What the rule's `lighting_type` states; a number it does not know is the fog brightness.
    pub fn of(rule: u32) -> Self {
        match rule {
            1 => Self::Directional,
            2 => Self::None,
            _ => Self::FogHdr,
        }
    }
}

/// A sphere of fog the effect asks for: scaled and turned into an ellipsoid, `range` the radius before the scale.
#[derive(Debug, Clone, Copy)]
pub struct FogVolumeInfo {
    pub pos: Vec3,
    pub scale: Vec3,
    /// Euler angles about X, Y and Z, in radians.
    pub rotation: Vec3,
    pub range: f32,
    /// The tint, its alpha as the rule gives it.
    pub colour: Vec4,
    pub density: f32,
    /// How fast the fog thins towards the edge of the volume.
    pub falloff: f32,
    pub hdr_mult: f32,
    pub lighting: FogLighting,
    pub use_ground_fog_colour: bool,
    pub sort_with_particles: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct LightInfo {
    pub pos: Vec3,
    pub colour: Vec3,
    pub intensity: f32,
    pub range: f32,
    pub direction: Vec3,
    pub cone_half_angle: f32,
    pub falloff: f32,
    pub corona_size: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum AudioEvent<'a> {
    Start { name: &'a str, pos: Vec3 },
    Stop { name: &'a str, pos: Vec3 },
    Recycle { name: &'a str, pos: Vec3 },
}

pub trait FireSink: Send + Sync + std::fmt::Debug {
    fn add_fire(&self, info: FireInfo);
}
pub trait DecalSink: Send + Sync + std::fmt::Debug {
    fn add_decal(&self, info: DecalInfo);
    fn add_decal_trail(&self, info: DecalTrailInfo);
}
pub trait DecalPoolSink: Send + Sync + std::fmt::Debug {
    fn add_pool_decal(&self, info: DecalPoolInfo);
}
pub trait LiquidSink: Send + Sync + std::fmt::Debug {
    fn add_liquid(&self, info: LiquidInfo);
}
pub trait FogVolumeSink: Send + Sync + std::fmt::Debug {
    /// An update of the effects is about to add the volumes of its step, in place of the last one's.
    fn begin(&self) {}
    fn add_fog_volume(&self, info: FogVolumeInfo);
}
pub trait LightSink: Send + Sync + std::fmt::Debug {
    /// An update of the effects is about to add the lights of its step, in place of the last one's.
    fn begin(&self) {}
    fn add_light(&self, info: LightInfo);
}
pub trait AudioSink: Send + Sync + std::fmt::Debug {
    fn on_event(&self, event: AudioEvent<'_>);
}

#[derive(Clone, Copy)]
pub struct PtfxSinks<'a> {
    pub fire: Option<&'a (dyn FireSink + 'a)>,
    pub decal: Option<&'a (dyn DecalSink + 'a)>,
    pub decal_pool: Option<&'a (dyn DecalPoolSink + 'a)>,
    pub liquid: Option<&'a (dyn LiquidSink + 'a)>,
    pub fog_volume: Option<&'a (dyn FogVolumeSink + 'a)>,
    pub light: Option<&'a (dyn LightSink + 'a)>,
    pub audio: Option<&'a (dyn AudioSink + 'a)>,
}

impl<'a> PtfxSinks<'a> {
    /// An update of the effects is about to add what it gives, in place of the last one's.
    pub fn begin(&self) {
        if let Some(light) = self.light {
            light.begin();
        }
        if let Some(fog) = self.fog_volume {
            fog.begin();
        }
    }

    pub const fn none() -> Self {
        Self {
            fire: None,
            decal: None,
            decal_pool: None,
            liquid: None,
            fog_volume: None,
            light: None,
            audio: None,
        }
    }
}

impl<'a> Default for PtfxSinks<'a> {
    fn default() -> Self {
        Self::none()
    }
}
