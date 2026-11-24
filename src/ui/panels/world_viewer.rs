//! The World screen: GTA VI's world as the game streams it at rest, flown through with a camera
//! that has no pivot, and the panel that says what the mount found and what is streaming.
//!
//! ⚠ No orbit: a drag turns the camera where it stands, the keys move it, and the wheel sets how
//! fast. The keys are read only while the pointer is over the view or the view holds the focus.

use glam::Vec3;
use or_renderer::FlyCamera;

use crate::render::RenderBridge;
use crate::state::gta6_world::worldview::{self as gta6_worldview, Viewer};
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// The least the view is drawn when the dock is far too short.
const VIEW_MIN: f32 = 96.0;
/// The steepest a camera line looks up or down, in degrees: the fly camera's own bound.
const MOST_PITCH: f32 = 89.0;
/// The vertical fields of view a camera line may state, in degrees.
const FIELDS_OF_VIEW: std::ops::RangeInclusive<f32> = 5.0..=150.0;
/// The area and the layer a clean frame is sensed and painted through.
const CLEAN: &str = "world_viewer_clean";

/// Where a camera line puts the camera: metres, then degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    /// Degrees about Z: 0 looks along +Y, and a growing angle turns towards −X.
    pub yaw: f32,
    /// Degrees above the horizon.
    pub pitch: f32,
    /// The vertical field of view in degrees; `None` leaves the camera's own.
    pub fov: Option<f32>,
}

impl Pose {
    /// The pose of `camera`, its yaw within one turn.
    pub fn of(camera: &FlyCamera) -> Self {
        Self {
            position: camera.position,
            yaw: camera.yaw.to_degrees().rem_euclid(360.0),
            pitch: camera.pitch.to_degrees(),
            fov: Some(camera.fov_y.to_degrees()),
        }
    }

    /// Put `camera` where the pose says, leaving its speed and its planes.
    pub fn put(&self, camera: &mut FlyCamera) {
        camera.position = self.position;
        camera.yaw = self.yaw.to_radians();
        camera.pitch = self.pitch.to_radians();
        if let Some(fov) = self.fov {
            camera.fov_y = fov.to_radians();
        }
    }

    /// The line a shots script and the Capture section read back: `world camera X Y Z YAW PITCH FOV`.
    pub fn line(&self) -> String {
        let Vec3 { x, y, z } = self.position;
        let (yaw, pitch) = (self.yaw, self.pitch);
        match self.fov {
            Some(fov) => format!("world camera {x:.2} {y:.2} {z:.2} {yaw:.2} {pitch:.2} {fov:.2}"),
            None => format!("world camera {x:.2} {y:.2} {z:.2} {yaw:.2} {pitch:.2}"),
        }
    }

    /// The pose `line` states, with or without its leading `world camera`.
    pub fn read(line: &str) -> Result<Self, String> {
        let mut words: Vec<&str> = line.split_whitespace().collect();
        if words.starts_with(&["world", "camera"]) {
            words.drain(..2);
        }
        let mut numbers = Vec::with_capacity(words.len());
        for word in words {
            let number = word.parse::<f32>().ok().filter(|n| n.is_finite());
            numbers.push(number.ok_or_else(|| format!("`{word}` is not a number"))?);
        }
        let (x, y, z, yaw, pitch, fov) = match numbers[..] {
            [x, y, z, yaw, pitch] => (x, y, z, yaw, pitch, None),
            [x, y, z, yaw, pitch, fov] => (x, y, z, yaw, pitch, Some(fov)),
            _ => {
                return Err(format!(
                    "a camera is X Y Z YAW PITCH and an optional field of view, found {} number(s)",
                    numbers.len()
                ))
            }
        };
        if pitch.abs() > MOST_PITCH {
            return Err(format!(
                "a pitch is within {MOST_PITCH} degrees of the horizon"
            ));
        }
        if fov.is_some_and(|fov| !FIELDS_OF_VIEW.contains(&fov)) {
            return Err(format!(
                "a field of view is from {} to {} degrees",
                FIELDS_OF_VIEW.start(),
                FIELDS_OF_VIEW.end()
            ));
        }
        Ok(Self {
            position: Vec3::new(x, y, z),
            yaw,
            pitch,
            fov,
        })
    }
}

/// What the Capture section holds between frames.
#[derive(Default)]
pub struct Capture {
    /// The view is drawn alone over the whole window.
    pub clean: bool,
    /// The camera line typed or pasted into the field.
    line: String,
    /// The line last entered was not a camera line.
    refused: bool,
}

impl crate::ui::panel_state::PanelState for Capture {
    const KEY: &'static str = "world-capture";
}

/// The folder of the GTA VI install the app was given, where it still stands.
fn install(state: &AppState) -> Option<std::path::PathBuf> {
    (state.prefs.game_paths.get(or_title::Game::GtaVI))
        .filter(|root| root.is_dir())
        .cloned()
}

/// Whether the World screen has an install to show.
pub fn offered(state: &AppState) -> bool {
    install(state).is_some()
}

fn said(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(theme::TEXT_SECONDARY));
}

/// How far the keys held move the camera: forward, to the right and up, each −1 to 1, and whether fast.
fn keys(ui: &egui::Ui) -> (f32, f32, f32, bool) {
    use egui::Key;
    ui.input(|i| {
        let held = |keys: &[Key]| keys.iter().any(|k| i.key_down(*k)) as i32 as f32;
        // W A S D, and Z Q S D of a keyboard laid out the other way: a key means one thing in both.
        let along = held(&[Key::W, Key::Z, Key::ArrowUp]) - held(&[Key::S, Key::ArrowDown]);
        let across = held(&[Key::D, Key::ArrowRight]) - held(&[Key::A, Key::Q, Key::ArrowLeft]);
        let up = held(&[Key::E, Key::Space]) - held(&[Key::C]);
        (along, across, up, i.modifiers.shift)
    })
}

/// The view: the world through the fly camera.
pub fn draw_view(ui: &mut egui::Ui, state: &mut AppState, bridge: Option<&mut RenderBridge>) {
    let Some(root) = install(state) else {
        ui.vertical_centered(|ui| {
            ui.add_space(theme::SPACING_SECTION);
            said(
                ui,
                tr!("Set the GTA VI folder in Settings ▸ Games to fly through its world"),
            );
        });
        return;
    };
    let held = state.panels.get::<Capture>();
    // A clean frame draws no control, so Escape is the way back from it.
    if held.clean && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        held.clean = false;
    }
    let clean = held.clean;
    let mut viewer = gta6_worldview::viewer();
    viewer.open(&root);
    viewer.poll();

    let (rect, response) = match clean {
        true => whole_window(ui.ctx()),
        false => {
            let size = egui::vec2(ui.available_width(), ui.available_height().max(VIEW_MIN));
            ui.allocate_exact_size(size, egui::Sense::click_and_drag())
        }
    };
    let response = named(response, tr!("The GTA VI world, flown through"));
    if response.clicked() || response.drag_started() {
        response.request_focus();
    }
    if response.dragged() {
        let by = response.drag_delta();
        viewer.camera.look(by.x, by.y);
    }
    let over = response.hovered() || response.has_focus();
    let mut moving = response.dragged();
    if over {
        // ⚠ The arrows are the camera's: without the lock egui moves the keyboard to the widget beside the view.
        crate::ui::focus_ring::reads_typed_text(ui.ctx(), response.id);
        let (along, across, up, fast) = keys(ui);
        if (along, across, up) != (0.0, 0.0, 0.0) {
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            viewer.camera.fly(along, across, up, fast, dt);
            moving = true;
        }
    }
    if response.hovered() {
        let notches = ui.input(|i| i.smooth_scroll_delta.y) / 50.0;
        if notches != 0.0 {
            viewer.camera.scale_speed(notches);
        }
    }
    let building = viewer.building();
    let waiting = viewer.streaming().maps_waiting + viewer.models_waiting + viewer.textures_waiting;
    let failed = viewer.failed.clone();
    let speed = viewer.camera.speed;
    drop(viewer);
    // The frame is drawn again while the camera moves and while anything is still arriving.
    if moving || waiting > 0 || building.is_some() {
        ui.ctx().request_repaint();
    }

    let painter = match clean {
        // ⚠ Over every other layer, the toasts among them: a clean frame holds the world alone.
        true => {
            let layer = egui::LayerId::new(egui::Order::Debug, egui::Id::new(CLEAN));
            ui.ctx().layer_painter(layer)
        }
        false => ui.painter_at(rect),
    };
    match bridge {
        Some(bridge) => {
            let ppp = ui.ctx().pixels_per_point();
            bridge.set_desired_size(
                (rect.width() * ppp).round() as u32,
                (rect.height() * ppp).round() as u32,
            );
            bridge.refresh_world(state.drivers.time_of_day.days, state.drivers.weather.state);
            bridge.push_inputs(Default::default());
            match bridge.texture_id() {
                Some(texture) => {
                    let whole =
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(texture, rect, whole, egui::Color32::WHITE);
                }
                None => {
                    painter.rect_filled(rect, 0.0, theme::BG_L2);
                }
            }
        }
        None => {
            painter.rect_filled(rect, 0.0, theme::BG_L2);
        }
    }
    if clean {
        return;
    }
    let line = match (building, failed) {
        (Some(done), _) => tr!(
            "Indexing the GTA VI world: {percent} %",
            percent = (done * 100.0).round() as u32
        ),
        (None, Some(why)) => tr!("The GTA VI world was not indexed: {why}", why = why),
        (None, None) => tr!(
            "Drag to look, W A S D or Z Q S D to move, E and C up and down, Shift faster; the wheel sets the speed: {speed} m/s",
            speed = speed.round() as u32
        ),
    };
    let at = rect.left_top() + egui::vec2(8.0, 6.0);
    let font = egui::FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(line, font, theme::TEXT_PRIMARY.get());
    let back = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
    painter.rect_filled(
        back,
        egui::CornerRadius::same(theme::RADIUS_SMALL),
        theme::BG_L2,
    );
    painter.galley(at, galley, theme::TEXT_PRIMARY.get());
}

/// The whole window as the view, sensed over everything the dock draws.
fn whole_window(ctx: &egui::Context) -> (egui::Rect, egui::Response) {
    let window = ctx.content_rect();
    egui::Area::new(egui::Id::new(CLEAN))
        .order(egui::Order::Foreground)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.allocate_exact_size(window.size(), egui::Sense::click_and_drag())
        })
        .inner
}

fn count(n: u64) -> String {
    n.to_string()
}

/// The panel beside the view: what the mount found, what is streaming, and the camera.
pub fn draw_side(ui: &mut egui::Ui, state: &mut AppState, bridge: Option<&mut RenderBridge>) {
    if !offered(state) {
        said(
            ui,
            tr!("Set the GTA VI folder in Settings ▸ Games to fly through its world"),
        );
        return;
    }
    let drawn = bridge.map(|b| b.world_drawn()).unwrap_or_default();
    let mut viewer = gta6_worldview::viewer();
    egui::ScrollArea::vertical().show(ui, |ui| {
        mount(ui, &viewer);
        ui.separator();
        ui.strong(tr!("Streaming"));
        let s = viewer.streaming();
        said(
            ui,
            &tr!(
                "{loaded} of {wanted} maps streamed in, {waiting} waited for; {nodes} entities and lists",
                loaded = s.maps_loaded,
                wanted = s.maps_wanted,
                waiting = s.maps_waiting,
                nodes = count(s.nodes as u64),
            ),
        );
        said(
            ui,
            &tr!(
                "{seen} of {selected} instances in view, in {draws} draws",
                seen = count(drawn.seen as u64),
                selected = count(drawn.selected as u64),
                draws = count(drawn.draws as u64),
            ),
        );
        said(
            ui,
            &tr!(
                "{models} models on the device ({megabytes} MB), {waiting} waited for",
                models = count(drawn.models as u64),
                megabytes = count(drawn.bytes >> 20),
                waiting = viewer.models_waiting,
            ),
        );
        said(
            ui,
            &tr!(
                "{textures} textures on the device, {unread} not read, {waiting} waited for",
                textures = count(drawn.textures as u64),
                unread = count(drawn.textures_unread as u64),
                waiting = viewer.textures_waiting,
            ),
        );
        said(
            ui,
            match drawn.lit_by_the_game {
                true => tr!("Lit by the game's own passes"),
                false => tr!("Lit by the plain pass: no game shader pack is read"),
            },
        );
        ui.separator();
        ui.strong(tr!("Camera"));
        ui.horizontal(|ui| {
            let position = &mut viewer.camera.position;
            for (axis, value) in [("X", &mut position.x), ("Y", &mut position.y), ("Z", &mut position.z)] {
                let field = ui.add(egui::DragValue::new(value).speed(1.0).prefix(format!("{axis} ")));
                named(field, &tr!("Camera position {axis}, in metres", axis = axis));
            }
        });
        let speed = ui.add(
            egui::Slider::new(&mut viewer.camera.speed, 1.0..=2000.0)
                .logarithmic(true)
                .text(tr!("Speed (m/s)")),
        );
        named(speed, tr!("How fast the camera flies, in metres a second"));
        let scale = ui.add(
            egui::Slider::new(&mut viewer.lod_scale, 0.5..=4.0).text(tr!("Level-of-detail scale")),
        );
        named(
            scale,
            tr!("The scale every level-of-detail distance is multiplied by"),
        );
        ui.separator();
        capture(ui, state, &mut viewer);
        ui.separator();
        let again = ui.add_enabled(
            viewer.building().is_none(),
            egui::Button::new(tr!("Build the index again")),
        );
        if again
            .on_hover_text(tr!(
                "Read the registration documents and every map of the level again, and keep the index in place of the one kept"
            ))
            .clicked()
        {
            viewer.rebuild();
        }
    });
}

/// The Capture section: a clean frame, the camera as a line a shots script reads, and a line to jump to.
fn capture(ui: &mut egui::Ui, state: &mut AppState, viewer: &mut Viewer) {
    ui.strong(tr!("Capture"));
    let held = state.panels.get::<Capture>();
    ui.checkbox(&mut held.clean, tr!("Clean frame")).on_hover_text(tr!(
        "Draw the world alone over the whole window, with no line and no panel over it; Escape brings them back"
    ));
    let copy = ui.button(tr!("Copy the camera")).on_hover_text(tr!(
        "Copy where the camera stands and looks, as a line a shots script reads and the field below takes"
    ));
    if copy.clicked() {
        ui.ctx().copy_text(Pose::of(&viewer.camera).line());
    }
    let field = egui::TextEdit::singleline(&mut held.line)
        .hint_text(tr!("Paste a camera line, then press Enter"))
        .desired_width(f32::INFINITY);
    let field = named(ui.add(field), tr!("A camera line to jump to"));
    if field.changed() {
        held.refused = false;
    }
    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        match Pose::read(&held.line) {
            Ok(pose) => {
                pose.put(&mut viewer.camera);
                held.line.clear();
                ui.ctx().request_repaint();
            }
            Err(_) => held.refused = true,
        }
    }
    if held.refused {
        ui.colored_label(
            theme::WARNING,
            tr!("That is not a camera line: copy one with Copy the camera"),
        );
    }
}

/// What the mount found: the documents read, the archives bound and the maps that stream at rest.
fn mount(ui: &mut egui::Ui, viewer: &Viewer) {
    ui.strong(tr!("Mount"));
    let Some(index) = &viewer.index else {
        said(
            ui,
            match viewer.building() {
                Some(_) => tr!("The registration documents and the level's maps are being read"),
                None => tr!("No index of this install"),
            },
        );
        return;
    };
    let r = &index.report;
    said(
        ui,
        &tr!(
            "{packs} packs; the level's change sets: {read} read, {unread} not readable",
            packs = r.packs,
            read = r.sets_read,
            unread = r.sets_unread,
        ),
    );
    said(
        ui,
        &tr!(
            "{bound} archives bound to the {named} the level names",
            bound = count(r.bound_archives),
            named = count(r.named_archives),
        ),
    );
    said(
        ui,
        &tr!(
            "{registered} of {maps} maps are the level's: {rest} stream at rest, {scripted} are script managed, {empty} hold nothing",
            registered = count(r.registered),
            maps = count(r.maps),
            rest = count(r.at_rest),
            scripted = count(r.script_managed),
            empty = count(r.empty),
        ),
    );
    said(
        ui,
        &tr!(
            "{archetypes} archetypes, {models} models; {unread} bodies did not read",
            archetypes = count(r.archetypes),
            models = count(r.models),
            unread = count(r.unread),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> FlyCamera {
        FlyCamera {
            position: Vec3::new(-1234.5, 678.25, 91.0),
            yaw: 2.0,
            pitch: -0.35,
            fov_y: 60f32.to_radians(),
            ..FlyCamera::default()
        }
    }

    #[test]
    fn a_copied_camera_line_puts_another_camera_where_the_first_stood() {
        let from = camera();
        let line = Pose::of(&from).line();
        assert!(line.starts_with("world camera "), "{line}");
        let mut to = FlyCamera {
            speed: 7.0,
            ..FlyCamera::default()
        };
        Pose::read(&line).expect("its own line").put(&mut to);
        // A line holds two decimals of a metre and of a degree.
        assert!(to.position.distance(from.position) < 0.01, "{line}");
        for (got, want) in [
            (to.yaw, from.yaw),
            (to.pitch, from.pitch),
            (to.fov_y, from.fov_y),
        ] {
            assert!(
                (got - want).abs() < 0.01f32.to_radians(),
                "{line}: {got} for {want}"
            );
        }
        assert_eq!(to.speed, 7.0, "a line says nothing of the speed");
        assert_eq!(
            Pose::of(&to).line(),
            line,
            "a line read and copied again is the same line"
        );
    }

    #[test]
    fn a_camera_line_reads_with_or_without_its_step_and_its_field_of_view() {
        let with = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            yaw: 90.0,
            pitch: -10.0,
            fov: Some(50.0),
        };
        assert_eq!(Pose::read("world camera 1 2 3 90 -10 50"), Ok(with));
        assert_eq!(Pose::read("  1 2 3 90 -10 50 "), Ok(with));
        let without = Pose::read("1 2 3 90 -10").expect("a line with no field of view");
        assert_eq!(without.fov, None);
        let mut camera = camera();
        let own = camera.fov_y;
        without.put(&mut camera);
        assert_eq!(
            camera.fov_y, own,
            "a line with no field of view leaves the camera's own"
        );
        // A yaw of 90 degrees looks along −X: the angles are the fly camera's own, in degrees.
        let pitch = (-10f32).to_radians();
        let ahead = Vec3::new(-pitch.cos(), 0.0, pitch.sin());
        assert!(
            camera.forward().distance(ahead) < 1e-5,
            "{}",
            camera.forward()
        );
    }

    #[test]
    fn a_line_that_is_no_camera_is_refused() {
        for bad in [
            "",
            "world camera",
            "1 2 3 4",
            "1 2 3 4 5 6 7",
            "1 2 three 4 5",
            "1 2 3 4 inf",
            "1 2 3 0 89.5",
            "1 2 3 0 -90",
            "1 2 3 0 0 0",
            "1 2 3 0 0 170",
            "world hour 9",
        ] {
            assert!(Pose::read(bad).is_err(), "`{bad}` should be refused");
        }
        assert!(
            Pose::read("1 2 3 0 89").is_ok(),
            "the bound itself is a pitch"
        );
    }
}
