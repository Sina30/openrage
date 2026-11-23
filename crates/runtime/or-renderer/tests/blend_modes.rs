use or_renderer::ParticleBlendMode;
use wgpu::{BlendFactor, BlendOperation};
#[derive(Debug, Clone, Copy)]
struct ExpectedBlend {
    color_src: BlendFactor,
    color_dst: BlendFactor,
    color_op: BlendOperation,
    alpha_src: BlendFactor,
    alpha_dst: BlendFactor,
    alpha_op: BlendOperation,
}

const EXPECTED: &[(ParticleBlendMode, ExpectedBlend)] = &[
    (
        ParticleBlendMode::Alpha,
        ExpectedBlend {
            color_src: BlendFactor::SrcAlpha,
            color_dst: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Additive,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Subtract,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Subtract,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::CompositeAlpha,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::CompositeAlphaSubtract,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOperation::ReverseSubtract,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Lightmap,
        ExpectedBlend {
            color_src: BlendFactor::OneMinusSrcAlpha,
            color_dst: BlendFactor::SrcAlpha,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::SrcAlpha,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Matte,
        ExpectedBlend {
            color_src: BlendFactor::DstAlpha,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Overwrite,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::Zero,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::Zero,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Dest,
        ExpectedBlend {
            color_src: BlendFactor::DstAlpha,
            color_dst: BlendFactor::OneMinusDstAlpha,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::DstAlpha,
            alpha_dst: BlendFactor::OneMinusDstAlpha,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::AlphaAdd,
        ExpectedBlend {
            color_src: BlendFactor::SrcAlpha,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::ReverseSubtract,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::ReverseSubtract,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::Min,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Min,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Min,
        },
    ),
    (
        ParticleBlendMode::Max,
        ExpectedBlend {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Max,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Max,
        },
    ),
    (
        ParticleBlendMode::AlphaSubtract,
        ExpectedBlend {
            color_src: BlendFactor::SrcAlpha,
            color_dst: BlendFactor::One,
            color_op: BlendOperation::Subtract,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOperation::Add,
        },
    ),
    (
        ParticleBlendMode::MultiplySrcDest,
        ExpectedBlend {
            color_src: BlendFactor::Dst,
            color_dst: BlendFactor::Zero,
            color_op: BlendOperation::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::Zero,
            alpha_op: BlendOperation::Add,
        },
    ),
];

#[test]
fn every_blend_mode_matches_engine_table() {
    for &(mode, expected) in EXPECTED {
        let bs = mode.wgpu_blend();
        assert_eq!(
            bs.color.src_factor, expected.color_src,
            "{mode:?} color.src_factor"
        );
        assert_eq!(
            bs.color.dst_factor, expected.color_dst,
            "{mode:?} color.dst_factor"
        );
        assert_eq!(bs.color.operation, expected.color_op, "{mode:?} color.op");
        assert_eq!(
            bs.alpha.src_factor, expected.alpha_src,
            "{mode:?} alpha.src_factor"
        );
        assert_eq!(
            bs.alpha.dst_factor, expected.alpha_dst,
            "{mode:?} alpha.dst_factor"
        );
        assert_eq!(bs.alpha.operation, expected.alpha_op, "{mode:?} alpha.op");
    }
}

#[test]
fn retail_fallback_classification_matches_engine_select() {
    let stock = [
        ParticleBlendMode::Alpha,
        ParticleBlendMode::Additive,
        ParticleBlendMode::Subtract,
        ParticleBlendMode::CompositeAlpha,
        ParticleBlendMode::CompositeAlphaSubtract,
    ];
    for &m in &stock {
        assert!(
            !m.retail_falls_back_to_normal(),
            "{m:?} should NOT be flagged as retail-fallback"
        );
    }
    let fallback = [
        ParticleBlendMode::Lightmap,
        ParticleBlendMode::Matte,
        ParticleBlendMode::Overwrite,
        ParticleBlendMode::Dest,
        ParticleBlendMode::AlphaAdd,
        ParticleBlendMode::ReverseSubtract,
        ParticleBlendMode::Min,
        ParticleBlendMode::Max,
        ParticleBlendMode::AlphaSubtract,
        ParticleBlendMode::MultiplySrcDest,
        ParticleBlendMode::Screen,
    ];
    for &m in &fallback {
        assert!(
            m.retail_falls_back_to_normal(),
            "{m:?} should be flagged as retail-fallback"
        );
    }
}
