//! A frame as an image: the render target a shot draws into, the pass that paints egui onto it, and the PNG read back from it.

use std::path::Path;

pub(super) fn target_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("self-check shot"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: super::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

pub(super) fn paint(
    renderer: &mut egui_wgpu::Renderer,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    clear: wgpu::Color,
    primitives: &[egui::ClippedPrimitive],
    screen: &egui_wgpu::ScreenDescriptor,
) {
    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("self-check shot pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    renderer.render(&mut pass.forget_lifetime(), primitives, screen);
}

pub(super) fn save_png(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    w: u32,
    h: u32,
    path: &Path,
) -> Result<(), String> {
    crate::render::read_back::rgba(device, queue, texture, w, h)?
        .save(path)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}
