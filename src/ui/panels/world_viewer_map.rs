//! The World panel's overhead map and the camera's place in words: the maps that stream at rest
//! laid on a grid at run time from the index, the camera on it, and a click to look over a point.

use std::sync::Arc;

use glam::{Vec2, Vec3};

use super::world_viewer_places::Place;
use crate::state::gta6_world::worldview::index::{Enabled, WorldIndex};
use crate::state::gta6_world::worldview::Viewer;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// The cells a side of the map is counted in.
const SIDE: usize = 256;
/// A box wider than this, a far level of detail's, is not a building: it neither lights a cell nor names a place.
const WIDEST_BOX: f32 = 400.0;
/// The tallest the map is drawn, in points.
const MOST_HEIGHT: f32 = 280.0;
/// How far the camera moves before its place is put in words again, in metres.
const MOVED: f32 = 16.0;

/// A map as the overhead map counts it: the middle of its entities' box, its corners and its name.
pub struct Laid {
    pub low: Vec3,
    pub high: Vec3,
    pub name: u32,
}

/// The maps at rest counted on a square grid over the level.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// The south-west corner and the side of the square, in metres.
    pub origin: Vec2,
    pub side: f32,
    /// The maps whose middle lies in each cell, row 0 the northernmost.
    pub counts: Vec<u32>,
    /// The top of the tallest building box of each cell, `NEG_INFINITY` where none.
    pub tops: Vec<f32>,
}

impl Grid {
    /// The grid of `maps`, `None` where none has a finite box.
    pub fn of(maps: &[Laid]) -> Option<Self> {
        let middles = || {
            (maps.iter())
                .filter(|m| (m.high - m.low).truncate().max_element() <= WIDEST_BOX)
                .map(|m| ((m.low + m.high) / 2.0, m.high.z))
                .filter(|(middle, top)| middle.is_finite() && top.is_finite())
        };
        // ⚠ A few maps lie far out at sea or under the level: the square spans all but the outermost half percent each way.
        let (mut xs, mut ys): (Vec<f32>, Vec<f32>) = middles().map(|(m, _)| (m.x, m.y)).unzip();
        if xs.is_empty() {
            return None;
        }
        xs.sort_unstable_by(f32::total_cmp);
        ys.sort_unstable_by(f32::total_cmp);
        let (first, last) = (xs.len() / 200, xs.len() - 1 - xs.len() / 200);
        let (low, high) = (
            Vec2::new(xs[first], ys[first]),
            Vec2::new(xs[last], ys[last]),
        );
        // A square a little wider than the maps, so that none lies on its edge.
        let side = (high - low).max_element().max(1.0) * 1.04;
        let origin = (low + high) / 2.0 - Vec2::splat(side / 2.0);
        let mut grid = Self {
            origin,
            side,
            counts: vec![0; SIDE * SIDE],
            tops: vec![f32::NEG_INFINITY; SIDE * SIDE],
        };
        for (middle, top) in middles() {
            if let Some(at) = grid.cell(middle.truncate()) {
                grid.counts[at] += 1;
                grid.tops[at] = grid.tops[at].max(top);
            }
        }
        Some(grid)
    }

    /// Where `point` lies on the map, from 0 to 1 across and down.
    pub fn to_map(&self, point: Vec2) -> Vec2 {
        let along = (point - self.origin) / self.side;
        Vec2::new(along.x, 1.0 - along.y)
    }

    /// The point of the level at `at` on the map, from 0 to 1 across and down.
    pub fn to_level(&self, at: Vec2) -> Vec2 {
        self.origin + Vec2::new(at.x, 1.0 - at.y) * self.side
    }

    fn cell(&self, point: Vec2) -> Option<usize> {
        let at = self.to_map(point) * SIDE as f32;
        let (x, y) = (at.x.floor(), at.y.floor());
        let inside = (0.0..SIDE as f32).contains(&x) && (0.0..SIDE as f32).contains(&y);
        inside.then(|| y as usize * SIDE + x as usize)
    }

    /// The tallest building box within `reach` metres of `point`, where any.
    pub fn top_near(&self, point: Vec2, reach: f32) -> Option<f32> {
        let cells = (reach / self.side * SIDE as f32).ceil() as i32;
        let at = self.to_map(point) * SIDE as f32;
        let mut top = f32::NEG_INFINITY;
        for dy in -cells..=cells {
            for dx in -cells..=cells {
                let (x, y) = (at.x as i32 + dx, at.y as i32 + dy);
                if (0..SIDE as i32).contains(&x) && (0..SIDE as i32).contains(&y) {
                    top = top.max(self.tops[y as usize * SIDE + x as usize]);
                }
            }
        }
        top.is_finite().then_some(top)
    }

    /// The map as a mask the theme's text colour tints: a cell as opaque as the log of the maps it holds.
    fn image(&self) -> egui::ColorImage {
        let most = self.counts.iter().copied().max().unwrap_or(0).max(1) as f32;
        let pixels = (self.counts.iter())
            .map(|&n| {
                let light = (1.0 + n as f32).ln() / (1.0 + most).ln();
                egui::Color32::from_white_alpha((light * 230.0) as u8)
            })
            .collect();
        egui::ColorImage {
            size: [SIDE, SIDE],
            source_size: egui::vec2(SIDE as f32, SIDE as f32),
            pixels,
        }
    }
}

/// The maps of `index` that stream at rest, as the overhead map lays them.
fn laid(index: &WorldIndex) -> Vec<Laid> {
    let at_rest = Enabled::default();
    (index.maps.iter())
        .filter(|m| at_rest.allows(m))
        .map(|m| {
            let (low, high) = m.boxes().entities;
            Laid {
                low,
                high,
                name: m.name,
            }
        })
        .collect()
}

/// The point of the compass a camera turned by `yaw` degrees looks to: 0 north, 1 north-east, on to 7.
pub fn compass(yaw: f32) -> usize {
    // A growing yaw turns from north towards the west.
    let east_of_north = (-yaw).rem_euclid(360.0);
    ((east_of_north / 45.0).round() as usize) % 8
}

/// The smallest building box that holds `point` whose name `name` gives, and that name.
pub fn named_under(
    maps: &[Laid],
    point: Vec2,
    name: impl Fn(u32) -> Option<String>,
) -> Option<String> {
    let mut best: Option<(f32, u32)> = None;
    for map in maps {
        let size = (map.high - map.low).truncate();
        let holds = (map.low.truncate().cmple(point) & point.cmple(map.high.truncate())).all();
        if !holds || size.max_element() > WIDEST_BOX {
            continue;
        }
        let area = size.x * size.y;
        if best.is_some_and(|(least, _)| least <= area) {
            continue;
        }
        if name(map.name).is_some() {
            best = Some((area, map.name));
        }
    }
    best.and_then(|(_, map)| name(map))
}

/// What the overhead map holds between frames, by the index it was laid from.
#[derive(Default)]
pub struct Overview {
    of: usize,
    maps: Vec<Laid>,
    grid: Option<Grid>,
    texture: Option<egui::TextureHandle>,
    /// Where the map under the camera was last looked for, by the vocabulary it was named with, and its name.
    named_at: Option<(Vec2, usize)>,
    name: Option<String>,
}

impl crate::ui::panel_state::PanelState for Overview {
    const KEY: &'static str = "world-overview";
}

impl Overview {
    /// Lay the maps of `index` again where it is another index.
    fn hold(&mut self, ctx: &egui::Context, index: &Arc<WorldIndex>) {
        let of = Arc::as_ptr(index) as usize;
        if self.of == of {
            return;
        }
        // A pass over every map at rest, once an index.
        self.of = of;
        self.maps = laid(index);
        self.grid = Grid::of(&self.maps);
        self.texture = (self.grid.as_ref()).map(|grid| {
            ctx.load_texture("world-overview", grid.image(), egui::TextureOptions::LINEAR)
        });
        self.named_at = None;
    }
}

/// The overhead map: the camera on it, and a click that puts the camera to look over the point clicked.
pub fn draw(ui: &mut egui::Ui, overview: &mut Overview, viewer: &mut Viewer) {
    let Some(index) = viewer.index.clone() else {
        return;
    };
    overview.hold(ui.ctx(), &index);
    let (Some(grid), Some(texture)) = (&overview.grid, &overview.texture) else {
        return;
    };
    let side = ui.available_width().min(MOST_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    let response = named(
        response,
        tr!("The level from above: the brighter, the more maps; click to look over a place"),
    );
    let painter = ui.painter_at(rect);
    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(theme::RADIUS_SMALL),
        theme::BG_L0,
    );
    painter.image(texture.id(), rect, whole, theme::TEXT_PRIMARY.get());
    let on_screen = |at: Vec2| rect.min + egui::vec2(at.x, at.y) * rect.size();
    let camera = &viewer.camera;
    let at = on_screen(grid.to_map(camera.position.truncate()));
    let yaw = camera.yaw;
    // Yaw 0 looks along +Y, north, up the map; a growing yaw turns towards −X.
    let ahead = egui::vec2(-yaw.sin(), -yaw.cos()) * 12.0;
    let stroke = egui::Stroke::new(2.0_f32, theme::WARNING);
    painter.line_segment([at, at + ahead], stroke);
    painter.circle_filled(at, 3.5, theme::WARNING);
    if let Some(pointer) = response
        .interact_pointer_pos()
        .filter(|_| response.clicked())
    {
        let across = (pointer - rect.min) / rect.size();
        let point = grid.to_level(Vec2::new(across.x, across.y));
        let ground = (grid.top_near(point, 256.0))
            .or_else(|| viewer.ground_under(point.x, point.y))
            .unwrap_or(0.0);
        let place = Place {
            head: None,
            maps: 0,
            middle: point.extend(ground),
        };
        place.pose().put(&mut viewer.camera);
        viewer.cuts += 1;
        ui.ctx().request_repaint();
    }
    response.on_hover_text(tr!(
        "Click to look north over a place, from 250 m south of it"
    ));
}

/// Where the camera stands, in words: the map under it where the vocabulary names one, how high over the ground and which way it looks.
pub fn place_words(overview: &mut Overview, viewer: &Viewer) -> String {
    let camera = &viewer.camera;
    let point = camera.position.truncate();
    let vocabulary = crate::state::gta6_names::vocabulary();
    let named_with = Arc::as_ptr(&vocabulary) as usize;
    // ⚠ The name is a pass over every map at rest: it is looked for again only once the camera moved or the vocabulary changed.
    let fresh =
        (overview.named_at).is_some_and(|(at, of)| at.distance(point) < MOVED && of == named_with);
    if !fresh {
        overview.name = named_under(&overview.maps, point, |hash| {
            vocabulary.name32(hash).map(str::to_string)
        });
        overview.named_at = Some((point, named_with));
    }
    let direction = [
        tr!("north"),
        tr!("north-east"),
        tr!("east"),
        tr!("south-east"),
        tr!("south"),
        tr!("south-west"),
        tr!("west"),
        tr!("north-west"),
    ][compass(camera.yaw.to_degrees())];
    // ⚠ The height map is the level's open ground: a camera under it, in a basement or a tunnel, is said by its height alone.
    let ground =
        (viewer.ground_under(point.x, point.y)).filter(|ground| camera.position.z >= *ground);
    let height = (camera.position.z - ground.unwrap_or(0.0)).round() as i32;
    match (overview.name.clone(), ground) {
        (Some(name), Some(_)) => tr!(
            "Over {name}, {height} m above the ground, looking {direction}",
            name = name,
            height = height,
            direction = direction
        ),
        (Some(name), None) => tr!(
            "Over {name}, at {height} m, looking {direction}",
            name = name,
            height = height,
            direction = direction
        ),
        (None, Some(_)) => tr!(
            "{height} m above the ground, looking {direction}",
            height = height,
            direction = direction
        ),
        (None, None) => tr!(
            "At {height} m, looking {direction}",
            height = height,
            direction = direction
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(x: f32, y: f32, side: f32, top: f32, name: u32) -> Laid {
        Laid {
            low: Vec3::new(x - side / 2.0, y - side / 2.0, 0.0),
            high: Vec3::new(x + side / 2.0, y + side / 2.0, top),
            name,
        }
    }

    #[test]
    fn the_overhead_map_counts_buildings_north_up_and_a_click_reads_back_the_point() {
        let maps = [
            map(0.0, 0.0, 20.0, 30.0, 1),
            map(0.0, 0.0, 10.0, 90.0, 2),
            map(1000.0, 1000.0, 20.0, 10.0, 3),
            // A far level of detail's box lights nothing and widens nothing.
            map(9000.0, 9000.0, 4000.0, 500.0, 4),
        ];
        let grid = Grid::of(&maps).expect("maps with boxes");
        assert!(grid.side > 1000.0 && grid.side < 1100.0, "{}", grid.side);
        assert_eq!(grid.counts.iter().sum::<u32>(), 3);
        assert_eq!(
            grid.counts.iter().copied().max(),
            Some(2),
            "two middles share a cell"
        );
        let south_west = grid.to_map(Vec2::ZERO);
        let north_east = grid.to_map(Vec2::splat(1000.0));
        assert!(
            south_west.x < north_east.x && south_west.y > north_east.y,
            "north is up"
        );
        let back = grid.to_level(grid.to_map(Vec2::new(250.0, 750.0)));
        assert!(back.distance(Vec2::new(250.0, 750.0)) < 0.01, "{back}");
        assert_eq!(grid.top_near(Vec2::ZERO, 50.0), Some(90.0));
        assert_eq!(grid.top_near(Vec2::new(500.0, 500.0), 50.0), None);
        assert_eq!(Grid::of(&[]), None);
    }

    #[test]
    fn the_place_is_the_smallest_named_box_under_the_camera_and_the_compass_turns_west_with_the_yaw(
    ) {
        let maps = [
            map(0.0, 0.0, 300.0, 10.0, 1),
            map(0.0, 0.0, 40.0, 10.0, 2),
            map(0.0, 0.0, 10.0, 10.0, 3),
        ];
        let name = |hash: u32| (hash != 3).then(|| format!("map{hash}"));
        assert_eq!(
            named_under(&maps, Vec2::ZERO, name).as_deref(),
            Some("map2"),
            "the smallest named"
        );
        assert_eq!(
            named_under(&maps, Vec2::new(100.0, 0.0), name).as_deref(),
            Some("map1")
        );
        assert_eq!(named_under(&maps, Vec2::new(1000.0, 0.0), name), None);
        assert_eq!(compass(0.0), 0, "a yaw of 0 looks north");
        assert_eq!(compass(90.0), 6, "a yaw of 90 looks west");
        assert_eq!(compass(270.0), 2, "east");
        assert_eq!(compass(315.0), 1, "north-east");
        assert_eq!(compass(-45.0), 1);
        assert_eq!(compass(180.0), 4);
    }
}
