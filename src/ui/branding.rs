/// What OpenRage is, under its name on the start page and the About pages.
pub const TAGLINE: &str =
    tr_key!("Reads, edits and writes the files of GTA V, Legacy and Enhanced");

/// What it covers, in the About pages.
pub const DESCRIPTION: &str = tr_key!(
    "Open source. Archives, models, textures, collision bounds, cloth, \
     animation clips, particle effects and metadata."
);

pub fn app_logo_png_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/app_logo_256.png"))
}

const APP_LOGO_SVG: &[u8] = include_bytes!("../../assets/brand/openrage-symbol.svg");

pub const APP_LOGO_URI: &str = "bytes://openrage-symbol.svg";

// egui_wgpu builds no mipmaps, so a raster logo drawn a quarter of its size
// aliases its outline; the SVG is rasterised at the exact on-screen pixel size.
pub fn app_logo_image(size: f32) -> egui::Image<'static> {
    egui::Image::from_bytes(APP_LOGO_URI, APP_LOGO_SVG)
        .fit_to_exact_size(egui::vec2(size, size))
        .maintain_aspect_ratio(true)
}

pub fn load_app_logo_window_icon() -> Option<winit::window::Icon> {
    let bytes = app_logo_png_bytes();
    let img = match image::load_from_memory_with_format(bytes, image::ImageFormat::Png) {
        Ok(img) => img.into_rgba8(),
        Err(err) => {
            tracing::warn!("Failed to decode embedded app logo PNG: {err}");
            return None;
        }
    };
    let (width, height) = img.dimensions();
    match winit::window::Icon::from_rgba(img.into_raw(), width, height) {
        Ok(icon) => Some(icon),
        Err(err) => {
            tracing::warn!("Failed to build winit Icon from app logo: {err}");
            None
        }
    }
}

// The resource id winresource's `set_icon` gives the icon in build.rs.
#[cfg(windows)]
const ICON_RESOURCE: u16 = 1;

/// Gives a window about to be created the executable's icon for its taskbar button.
// winit's window class has no icon, so a button the taskbar makes before the
// window has a big icon stays blank; winit applies this one before showing it.
#[cfg(windows)]
pub fn with_taskbar_icon(
    attrs: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
    match winit::window::Icon::from_resource(ICON_RESOURCE, None) {
        Ok(icon) => attrs.with_taskbar_icon(Some(icon)),
        Err(err) => {
            tracing::debug!("Executable icon resource unavailable for the taskbar: {err}");
            attrs
        }
    }
}

#[cfg(not(windows))]
pub fn with_taskbar_icon(
    attrs: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    attrs
}

/// Replaces the window's icons with frames of the executable's own `.ico`, sized for its scale.
// Windows shrinks a single 256 px icon to the 16 px title bar with a coarse stretch;
// `LoadImageW` at the drawn size picks the matching `.ico` frame instead.
#[cfg(windows)]
pub fn apply_native_window_icons(window: &winit::window::Window) {
    use winit::platform::windows::{IconExtWindows, WindowExtWindows};
    use winit::window::Icon;

    // SM_CXSMICON and SM_CXICON at 100% scaling.
    const SMALL: f64 = 16.0;
    const BIG: f64 = 32.0;

    let sized = |points: f64| {
        let side = (points * window.scale_factor()).round() as u32;
        Icon::from_resource(
            ICON_RESOURCE,
            Some(winit::dpi::PhysicalSize::new(side, side)),
        )
    };
    match (sized(SMALL), sized(BIG)) {
        (Ok(small), Ok(big)) => {
            window.set_window_icon(Some(small));
            window.set_taskbar_icon(Some(big));
        }
        (Err(err), _) | (_, Err(err)) => {
            tracing::debug!("Executable icon resource unavailable, keeping the PNG icon: {err}");
        }
    }
}

#[cfg(not(windows))]
pub fn apply_native_window_icons(_window: &winit::window::Window) {}

#[cfg(test)]
mod tests {
    use super::*;

    // `SizedTexture::size` is the source size egui lays out with, not the texels.
    fn drawn_texture_height(ppp: f32, size: f32) -> usize {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        for _ in 0..200 {
            let _ = ctx.run_ui(input.clone(), |ui| {
                assert_eq!(ui.pixels_per_point(), ppp);
                ui.add_sized(egui::vec2(size, size), app_logo_image(size));
            });
            let heights: Vec<usize> = ctx
                .tex_manager()
                .read()
                .allocated()
                .filter(|(_, meta)| meta.name == APP_LOGO_URI)
                .map(|(_, meta)| meta.size[1])
                .collect();
            if let [height] = heights[..] {
                return height;
            }
            assert!(
                heights.is_empty(),
                "more than one logo texture: {heights:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the logo never loaded at {ppp} pixels per point");
    }

    #[test]
    fn the_logo_is_rasterised_at_the_size_it_is_drawn() {
        for ppp in [1.0, 1.5, 2.0] {
            for size in [56.0, 72.0] {
                assert_eq!(
                    drawn_texture_height(ppp, size),
                    (size * ppp).round() as usize,
                    "a {size} pt logo at {ppp} pixels per point"
                );
            }
        }
    }
}
