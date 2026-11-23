use crate::context::{RenderContext, CLEAR_COLOR};
use crate::error::RenderError;
use std::time::Instant;
use tracing::{debug, trace, warn};
#[derive(Debug, Clone)]
pub struct FrameStats {
    pub frame_count: u64,
    pub total_time_secs: f64,
    start_time: Option<Instant>,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            frame_count: 0,
            total_time_secs: 0.0,
            start_time: None,
        }
    }
}

impl FrameStats {
    pub fn fps(&self) -> f64 {
        if self.total_time_secs > 0.0 {
            self.frame_count as f64 / self.total_time_secs
        } else {
            0.0
        }
    }

    pub fn record_frame(&mut self) {
        let now = Instant::now();

        if self.start_time.is_none() {
            self.start_time = Some(now);
        }

        self.frame_count += 1;
        self.total_time_secs = self.start_time.unwrap().elapsed().as_secs_f64();
    }

    pub fn reset(&mut self) {
        self.frame_count = 0;
        self.total_time_secs = 0.0;
        self.start_time = None;
    }
}

pub struct Renderer<'window> {
    context: RenderContext<'window>,
    stats: FrameStats,
}

impl<'window> Renderer<'window> {
    pub fn new(context: RenderContext<'window>) -> Self {
        Self {
            context,
            stats: FrameStats::default(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            debug!("Ignoring resize to zero dimension: {}x{}", width, height);
            return;
        }

        if width == self.context.config.width && height == self.context.config.height {
            trace!("Ignoring resize to same dimensions: {}x{}", width, height);
            return;
        }

        debug!("Resizing surface: {}x{}", width, height);
        self.context.config.width = width;
        self.context.config.height = height;
        self.context
            .surface
            .configure(&self.context.device, &self.context.config);
    }

    pub fn render_frame(&mut self) -> Result<(), RenderError> {
        trace!("Beginning render frame");

        let output = match self.context.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                warn!("Surface suboptimal, reconfiguring after this frame");
                t
            }
            status
            @ (wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost) => {
                warn!(?status, "Surface lost or outdated, reconfiguring");
                self.context
                    .surface
                    .configure(&self.context.device, &self.context.config);
                match self.context.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                    other => return Err(RenderError::SurfaceUnavailable(format!("{other:?}"))),
                }
            }
            other => return Err(RenderError::SurfaceUnavailable(format!("{other:?}"))),
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Clear Pass Encoder"),
                });

        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        self.context.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        self.stats.record_frame();
        trace!("Completed render frame {}", self.stats.frame_count);

        Ok(())
    }

    pub fn render_frame_with_recovery(&mut self) {
        match self.render_frame() {
            Ok(_) => {}
            Err(RenderError::SurfaceUnavailable(status)) => {
                warn!(%status, "Surface unavailable, reconfiguring");
                self.resize(self.context.config.width, self.context.config.height);
            }
            Err(e) => {
                warn!("Render frame error: {}", e);
            }
        }
    }

    pub fn stats(&self) -> &FrameStats {
        &self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats.reset();
    }

    pub fn width(&self) -> u32 {
        self.context.width()
    }

    pub fn height(&self) -> u32 {
        self.context.height()
    }

    pub fn context(&self) -> &RenderContext<'window> {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut RenderContext<'window> {
        &mut self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    #[test]
    fn test_frame_stats_fps_calculation() {
        let mut stats = FrameStats {
            frame_count: 60,
            total_time_secs: 1.0,
            ..Default::default()
        };
        assert_eq!(stats.fps(), 60.0);

        stats.frame_count = 120;
        stats.total_time_secs = 2.0;
        assert_eq!(stats.fps(), 60.0);
    }

    #[test]
    fn test_frame_stats_record_frame() {
        let mut stats = FrameStats::default();

        stats.record_frame();
        assert_eq!(stats.frame_count, 1);
        assert!(stats.start_time.is_some());
        assert!(stats.total_time_secs >= 0.0);

        thread::sleep(Duration::from_millis(10));
        stats.record_frame();
        assert_eq!(stats.frame_count, 2);
        assert!(stats.total_time_secs >= 0.01);
    }

    #[test]
    fn test_frame_stats_reset() {
        let mut stats = FrameStats::default();
        stats.record_frame();
        stats.record_frame();

        assert_eq!(stats.frame_count, 2);
        assert!(stats.start_time.is_some());

        stats.reset();
        assert_eq!(stats.frame_count, 0);
        assert_eq!(stats.total_time_secs, 0.0);
        assert!(stats.start_time.is_none());
    }
}
