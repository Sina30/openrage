use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParticleBlendMode {
    #[default]
    Alpha = 0,
    Additive = 1,
    Subtract = 2,
    Lightmap = 3,
    Matte = 4,
    Overwrite = 5,
    Dest = 6,
    AlphaAdd = 7,
    ReverseSubtract = 8,
    Min = 9,
    Max = 10,
    AlphaSubtract = 11,
    MultiplySrcDest = 12,
    CompositeAlpha = 13,
    CompositeAlphaSubtract = 14,
    Screen = 15,
}

impl ParticleBlendMode {
    pub const ALL: [Self; 16] = [
        Self::Alpha,
        Self::Additive,
        Self::Subtract,
        Self::Lightmap,
        Self::Matte,
        Self::Overwrite,
        Self::Dest,
        Self::AlphaAdd,
        Self::ReverseSubtract,
        Self::Min,
        Self::Max,
        Self::AlphaSubtract,
        Self::MultiplySrcDest,
        Self::CompositeAlpha,
        Self::CompositeAlphaSubtract,
        Self::Screen,
    ];

    pub fn from_core(mode: or_particle::rules::BlendMode) -> Self {
        use or_particle::rules::BlendMode as B;
        match mode {
            B::Alpha => Self::Alpha,
            B::Additive => Self::Additive,
            B::Subtract => Self::Subtract,
            B::Lightmap => Self::Lightmap,
            B::Matte => Self::Matte,
            B::Overwrite => Self::Overwrite,
            B::Dest => Self::Dest,
            B::AlphaAdd => Self::AlphaAdd,
            B::ReverseSubtract => Self::ReverseSubtract,
            B::Min => Self::Min,
            B::Max => Self::Max,
            B::AlphaSubtract => Self::AlphaSubtract,
            B::MultiplySrcDest => Self::MultiplySrcDest,
            B::CompositeAlpha => Self::CompositeAlpha,
            B::CompositeAlphaSubtract => Self::CompositeAlphaSubtract,
        }
    }

    pub fn from_engine_byte(b: u8) -> Self {
        if b >= 15 {
            return Self::Alpha;
        }
        Self::ALL.get(b as usize).copied().unwrap_or(Self::Alpha)
    }

    pub fn is_commutative(self) -> bool {
        matches!(
            self,
            Self::Additive | Self::AlphaAdd | Self::Min | Self::Max
        )
    }

    pub fn retail_falls_back_to_normal(self) -> bool {
        !matches!(
            self,
            Self::Alpha
                | Self::Additive
                | Self::Subtract
                | Self::CompositeAlpha
                | Self::CompositeAlphaSubtract
        )
    }

    #[inline]
    pub fn retail_equivalent(self) -> Self {
        if self.retail_falls_back_to_normal() {
            Self::Alpha
        } else {
            self
        }
    }

    pub fn wgpu_blend(self) -> wgpu::BlendState {
        use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState};
        let bc = |src, dst, op| BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: op,
        };
        match self {
            Self::Alpha => BlendState::ALPHA_BLENDING,
            Self::Additive => BlendState {
                color: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            },
            Self::Subtract => BlendState {
                color: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Subtract),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            },
            Self::Lightmap => BlendState {
                color: bc(
                    BlendFactor::OneMinusSrcAlpha,
                    BlendFactor::SrcAlpha,
                    BlendOperation::Add,
                ),
                alpha: bc(BlendFactor::One, BlendFactor::SrcAlpha, BlendOperation::Add),
            },
            Self::Matte => BlendState {
                color: bc(BlendFactor::DstAlpha, BlendFactor::One, BlendOperation::Add),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            },
            Self::Overwrite => BlendState {
                color: bc(BlendFactor::One, BlendFactor::Zero, BlendOperation::Add),
                alpha: bc(BlendFactor::One, BlendFactor::Zero, BlendOperation::Add),
            },
            Self::Dest => BlendState {
                color: bc(
                    BlendFactor::DstAlpha,
                    BlendFactor::OneMinusDstAlpha,
                    BlendOperation::Add,
                ),
                alpha: bc(
                    BlendFactor::DstAlpha,
                    BlendFactor::OneMinusDstAlpha,
                    BlendOperation::Add,
                ),
            },
            Self::AlphaAdd => BlendState {
                color: bc(BlendFactor::SrcAlpha, BlendFactor::One, BlendOperation::Add),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            },
            Self::ReverseSubtract => BlendState {
                color: bc(
                    BlendFactor::One,
                    BlendFactor::One,
                    BlendOperation::ReverseSubtract,
                ),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            },
            Self::Min => BlendState {
                color: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Min),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Min),
            },
            Self::Max => BlendState {
                color: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Max),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Max),
            },
            Self::AlphaSubtract => BlendState {
                color: bc(
                    BlendFactor::SrcAlpha,
                    BlendFactor::One,
                    BlendOperation::Subtract,
                ),
                alpha: bc(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            },
            Self::MultiplySrcDest => BlendState {
                color: bc(BlendFactor::Dst, BlendFactor::Zero, BlendOperation::Add),
                alpha: bc(BlendFactor::One, BlendFactor::Zero, BlendOperation::Add),
            },
            Self::CompositeAlpha => BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            Self::Screen => BlendState {
                color: bc(
                    BlendFactor::OneMinusDst,
                    BlendFactor::One,
                    BlendOperation::Add,
                ),
                alpha: bc(
                    BlendFactor::OneMinusDstAlpha,
                    BlendFactor::One,
                    BlendOperation::Add,
                ),
            },
            Self::CompositeAlphaSubtract => BlendState {
                color: bc(
                    BlendFactor::One,
                    BlendFactor::OneMinusSrcAlpha,
                    BlendOperation::ReverseSubtract,
                ),
                alpha: bc(
                    BlendFactor::One,
                    BlendFactor::OneMinusSrcAlpha,
                    BlendOperation::Add,
                ),
            },
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ParticleAlignment {
    #[default]
    Camera = 0,
    Velocity = 1,
    World = 2,
    Effect = 3,
    Emitter = 4,
}

impl ParticleAlignment {
    pub fn from_core(mode: or_io::gta5::particle::AlignmentMode) -> Self {
        use or_io::gta5::particle::AlignmentMode as A;
        match mode {
            A::Camera => Self::Camera,
            A::Velocity => Self::Velocity,
            A::World => Self::World,
            A::Effect => Self::Effect,
            A::Emitter => Self::Emitter,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ParticleInstance {
    pub position: [f32; 3],
    pub life_ratio: f32,
    pub dimensions: [f32; 4],
    pub rotation: [f32; 4],
    pub color: [f32; 4],
    pub velocity: [f32; 3],
    pub _pad0: f32,
    pub tex_frame_a: f32,
    pub tex_frame_b: f32,
    pub tex_blend: f32,
    pub _pad1: f32,
    pub uv_offset: [f32; 2],
    pub _pad2: [f32; 2],
    pub custom1: [f32; 4],
    pub custom2: [f32; 4],
}

pub struct ParticleBatch {
    pub blend: ParticleBlendMode,
    pub alignment: ParticleAlignment,
    pub uv_atlas_rows: u32,
    pub uv_atlas_cols: u32,
    /// Whether the technique refracts what is behind it, which draws after the rest of the particles.
    pub refract: bool,
    pub cull: ParticleCullMode,
    pub depth_write: bool,
    pub depth_test: bool,
    pub align_axis: [f32; 3],
    /// The values of the rule's vector and keyframe shader variables this frame, by the hash of each one's name, which the pass's constants of that name take.
    pub constants: Vec<(u32, [f32; 4])>,
    /// The technique of `ptfx_sprite` the rule names, which the game's own passes draw the batch with; none draws nothing there.
    pub shader_technique: String,
    /// The decoded diffuse texture the game's own passes bind, white where there is none.
    pub diffuse_image: Option<Arc<crate::game_shaders::mesh::Image>>,
    pub instances: Vec<ParticleInstance>,
}

impl ParticleBatch {
    pub fn new(
        blend: ParticleBlendMode,
        alignment: ParticleAlignment,
        instances: Vec<ParticleInstance>,
    ) -> Self {
        Self {
            blend,
            alignment,
            uv_atlas_rows: 1,
            uv_atlas_cols: 1,
            refract: false,
            cull: ParticleCullMode::None,
            depth_write: false,
            depth_test: true,
            align_axis: [0.0, 0.0, 1.0],
            constants: Vec::new(),
            shader_technique: String::new(),
            diffuse_image: None,
            instances,
        }
    }

    pub fn with_uv_atlas(mut self, rows: u32, cols: u32) -> Self {
        self.uv_atlas_rows = rows.max(1);
        self.uv_atlas_cols = cols.max(1);
        self
    }

    pub fn collapse_to_retail(&mut self) {
        self.blend = self.blend.retail_equivalent();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleCullMode {
    None,
    Front,
    Back,
}

impl Default for ParticleCullMode {
    fn default() -> Self {
        Self::None
    }
}

impl ParticleCullMode {
    pub fn from_engine(raw: u32) -> Self {
        match raw {
            1 => Self::Front,
            2 => Self::Back,
            _ => Self::None,
        }
    }

    pub fn to_wgpu(self) -> Option<wgpu::Face> {
        match self {
            Self::None => None,
            Self::Front => Some(wgpu::Face::Front),
            Self::Back => Some(wgpu::Face::Back),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_blend_set_round_trips_through_renderer() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..15u8 {
            let m = ParticleBlendMode::from_engine_byte(i);
            assert!(
                seen.insert(m as u8),
                "byte {i} aliases an earlier blend mode ({m:?})"
            );
        }
        assert_eq!(
            ParticleBlendMode::from_engine_byte(99),
            ParticleBlendMode::Alpha
        );
    }

    #[test]
    fn lightmap_factors_match_engine_modulate() {
        let blend = ParticleBlendMode::Lightmap.wgpu_blend();
        assert_eq!(blend.color.src_factor, wgpu::BlendFactor::OneMinusSrcAlpha);
        assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(blend.color.operation, wgpu::BlendOperation::Add);
        assert!(ParticleBlendMode::AlphaAdd.is_commutative());
        assert!(ParticleBlendMode::Min.is_commutative());
        assert!(ParticleBlendMode::Max.is_commutative());
        assert!(!ParticleBlendMode::Alpha.is_commutative());
        assert!(!ParticleBlendMode::CompositeAlpha.is_commutative());
        assert!(!ParticleBlendMode::Subtract.is_commutative());
    }

    #[test]
    fn cull_mode_engine_round_trip() {
        assert_eq!(ParticleCullMode::from_engine(0), ParticleCullMode::None);
        assert_eq!(ParticleCullMode::from_engine(1), ParticleCullMode::Front);
        assert_eq!(ParticleCullMode::from_engine(2), ParticleCullMode::Back);
        assert_eq!(ParticleCullMode::from_engine(99), ParticleCullMode::None);
    }
}
