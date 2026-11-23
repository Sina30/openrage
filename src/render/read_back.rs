//! A texture's pixels copied back from the GPU.

/// The top-left `w` by `h` texels of an RGBA8 `texture` made with `COPY_SRC`, copied back from the GPU.
pub fn rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    w: u32,
    h: u32,
) -> Result<image::RgbaImage, String> {
    // ⚠ A buffer copy pads every row to 256 bytes.
    let row =
        (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("texture readback"),
        size: u64::from(row) * u64::from(h),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv()
        .map_err(|e| format!("the readback never finished: {e}"))?
        .map_err(|e| format!("the readback failed: {e}"))?;
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for line in mapped.chunks(row as usize).take(h as usize) {
        pixels.extend_from_slice(&line[..(w * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    image::RgbaImage::from_raw(w, h, pixels)
        .ok_or_else(|| "the readback was shorter than the frame".into())
}
