//! The samplers the game's screen passes read their targets by, all clamped at the edges.

fn clamped(
    device: &wgpu::Device,
    filter: wgpu::FilterMode,
    mipmap: wgpu::MipmapFilterMode,
    compare: Option<wgpu::CompareFunction>,
) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: mipmap,
        compare,
        ..Default::default()
    })
}

/// Reads a texel as it is, which a pass reads a G-buffer or a depth by.
pub fn point(device: &wgpu::Device) -> wgpu::Sampler {
    clamped(
        device,
        wgpu::FilterMode::Nearest,
        wgpu::MipmapFilterMode::Nearest,
        None,
    )
}

/// Blends the texels around a point, which a pass reads a frame it scales by.
pub fn linear(device: &wgpu::Device) -> wgpu::Sampler {
    clamped(
        device,
        wgpu::FilterMode::Linear,
        wgpu::MipmapFilterMode::Nearest,
        None,
    )
}

/// Blends the texels and the levels around a point, which a map of the sky is read by.
pub fn smooth(device: &wgpu::Device) -> wgpu::Sampler {
    clamped(
        device,
        wgpu::FilterMode::Linear,
        wgpu::MipmapFilterMode::Linear,
        None,
    )
}

/// Compares what it reads with a depth, and blends the results of the texels around, which a map of a shadow is read by.
pub fn comparing(device: &wgpu::Device) -> wgpu::Sampler {
    clamped(
        device,
        wgpu::FilterMode::Linear,
        wgpu::MipmapFilterMode::Nearest,
        Some(wgpu::CompareFunction::LessEqual),
    )
}
