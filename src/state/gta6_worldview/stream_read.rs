//! A map body as the streamer holds it: the nodes it gives the level-of-detail forest, what draws
//! at each, and where each entity and each instance of its prop and grass lists stands.
//!
//! ⚠ A list of far stand-ins (`IsSLODList`) is a node, so that the lists under it find their
//! parent, and draws nothing: the model each of its instances takes is named by an id no map states.

use glam::{Mat4, Quat, Vec3};
use or_io::gta6::map::stream::{StreamGrassList, StreamLink, StreamMap};
use or_world::streaming::{Extent, Level, Node, ParentRef};

use super::super::index::{ModelRef, WorldIndex};
use super::grass;

/// The rows of a 3×4 matrix: the model's axes, with the translation in the fourth column.
pub type Rows = [[f32; 4]; 3];

/// The hours of an archetype that draws at every hour.
const EVERY_HOUR: u32 = 0x00FF_FFFF;
/// The level of a list that has neither parent nor children.
const ORPHAN: u8 = 5;

pub(super) fn rows_of(m: Mat4) -> Rows {
    let c = m.to_cols_array_2d();
    std::array::from_fn(|row| [c[0][row], c[1][row], c[2][row], c[3][row]])
}

pub(super) fn place(rows: &Rows, p: Vec3) -> Vec3 {
    let at = |r: [f32; 4]| r[0] * p.x + r[1] * p.y + r[2] * p.z + r[3];
    Vec3::new(at(rows[0]), at(rows[1]), at(rows[2]))
}

/// The length of the longest axis of `rows`, which a sphere's radius grows by.
pub(super) fn stretch(rows: &Rows) -> f32 {
    (0..3)
        .map(|axis| Vec3::new(rows[0][axis], rows[1][axis], rows[2][axis]).length())
        .fold(0.0, f32::max)
}

/// What draws at a node: its model, and the sphere of that model in its own space.
#[derive(Clone, Copy)]
pub(super) struct Drawn {
    /// `None` where no archetype or no body of the index gives one: the node draws nothing.
    pub(super) model: Option<ModelRef>,
    pub(super) centre: Vec3,
    pub(super) radius: f32,
    /// The hours it draws in, bit `h` for the hour from `h`: a time archetype's, else every hour.
    pub(super) hours: u32,
}

impl Drawn {
    /// Its model at the hour `hour`, 0 to 23: none outside the hours of a time archetype.
    pub(super) fn model_at(&self, hour: u32) -> Option<ModelRef> {
        self.model.filter(|_| (self.hours >> hour) & 1 == 1)
    }
}

/// Where the instances of a list stand.
pub(super) enum Instances {
    Placed(Vec<Rows>),
    /// As the body states them: an instance is placed when it draws.
    Grass(StreamGrassList),
}

/// A prop or grass list streamed in.
pub(super) struct List {
    pub(super) instances: Instances,
    /// Where an instance starts to fade and where it is gone, in metres from the camera; `None`
    /// for a list that draws whole.
    pub(super) reach: Option<(f32, f32)>,
    pub(super) level: Level,
}

impl List {
    /// How opaque an instance `away` from the camera is with distances scaled by `scale`; `None`
    /// where it is gone.
    pub(super) fn fade(&self, away: f32, scale: f32) -> Option<f32> {
        let Some((start, end)) = self.reach else {
            return Some(1.0);
        };
        let (start, end) = (start * scale, end * scale);
        if away > end {
            return None;
        }
        // A list whose fade starts at its end or past it has no band: an instance is there or gone.
        let fade = match start < end {
            true => ((end - away) / (end - start)).min(1.0),
            false => 1.0,
        };
        (fade > 0.0).then_some(fade)
    }

    /// Call `draw` with where each instance that draws for a camera at `eye` stands and how opaque
    /// it is, less the instances `skipped` names.
    // ponytail: every instance of a list that draws is measured at each selection; a grid of the list's box if the walk shows.
    pub(super) fn each(
        &self,
        eye: Vec3,
        scale: f32,
        skipped: &[u32],
        mut draw: impl FnMut(Rows, f32),
    ) {
        match &self.instances {
            Instances::Placed(placed) => {
                for (at, rows) in placed.iter().enumerate() {
                    if skipped.contains(&(at as u32)) {
                        continue;
                    }
                    let origin = Vec3::new(rows[0][3], rows[1][3], rows[2][3]);
                    if let Some(fade) = self.fade(eye.distance(origin), scale) {
                        draw(*rows, fade);
                    }
                }
            }
            Instances::Grass(list) => {
                for (at, instance) in list.instances.iter().enumerate() {
                    if skipped.contains(&(at as u32)) {
                        continue;
                    }
                    let origin = Vec3::from(list.position_of(instance));
                    if let Some(fade) = self.fade(eye.distance(origin), scale) {
                        draw(grass::rows(list, instance, origin), fade);
                    }
                }
            }
        }
    }
}

/// A map streamed in.
pub(super) struct Loaded {
    /// One for each node: the entities, then the prop lists, then the grass lists.
    pub(super) drawn: Vec<Drawn>,
    /// Where each entity stands.
    pub(super) entities: Vec<Rows>,
    /// The prop lists, then the grass lists.
    pub(super) lists: Vec<List>,
    /// The interiors the map places, each with its contents in the world.
    pub(super) interiors: Vec<super::super::interior::Placed>,
}

/// What a worker read of a map: its nodes for the forest, and what draws at each.
pub(in super::super) struct ReadMap {
    pub(super) nodes: Vec<Node>,
    pub(super) loaded: Loaded,
}

fn link(map: &StreamMap, own: u32, link: Option<StreamLink>) -> (Option<ParentRef>, Option<u32>) {
    let Some(link) = link else {
        return (None, None);
    };
    let name = match link.map {
        Some(selector) => map.lod_parents.get(usize::from(selector)).copied(),
        None => Some(own),
    };
    let parent = name.map(|name| ParentRef {
        map: u64::from(name),
        node: u32::from(link.index),
    });
    (parent, link.instance.map(u32::from))
}

/// The node of a list in the box `min` to `max`: it is measured to the box, and one that states
/// no distance takes its archetype's.
fn list_node(
    (min, max): (Vec3, Vec3),
    (lod_dist, archetype_dist): (f32, f32),
    child_lod_dist: f32,
    parent: Option<ParentRef>,
    level: u8,
) -> Node {
    Node {
        position: (min + max) / 2.0,
        extent: Extent::Box(min, max),
        lod_dist: if lod_dist > 0.0 {
            lod_dist
        } else {
            archetype_dist
        },
        child_lod_dist,
        parent,
        instance: None,
        level: Level::from_raw(level),
    }
}

/// The nodes of `map` and what draws at each, by the archetypes and models of `index`.
pub(in super::super) fn read_map(index: &WorldIndex, map: &StreamMap) -> ReadMap {
    let count = map.entities.len() + map.prop_lists.len() + map.grass.len();
    let (mut nodes, mut drawn) = (Vec::with_capacity(count), Vec::with_capacity(count));
    let of = |word: u64| {
        let archetype = index.archetype(word);
        let model = archetype.and_then(|a| index.model(a));
        let (centre, radius, lod_dist, hours) = archetype
            .map_or((Vec3::ZERO, 0.0, 0.0, EVERY_HOUR), |a| {
                (Vec3::from(a.centre), a.radius, a.lod_dist, a.hours)
            });
        (
            Drawn {
                model,
                centre,
                radius,
                hours,
            },
            lod_dist,
        )
    };
    let mut entities = Vec::with_capacity(map.entities.len());
    for e in &map.entities {
        let (what, archetype_dist) = of(e.archetype_word);
        let (parent, instance) = link(map, map.name, e.parent);
        nodes.push(Node {
            position: Vec3::from(e.position),
            extent: Extent::Point,
            // An entity that states no distance takes its archetype's.
            lod_dist: if e.lod_dist > 0.0 {
                e.lod_dist
            } else {
                archetype_dist
            },
            child_lod_dist: e.child_lod_dist,
            parent,
            instance,
            level: Level::from_raw(e.lod_level),
        });
        // An interior has no model of its own: its contents are placed by `interior::read_placed`.
        drawn.push(Drawn {
            model: what.model.filter(|_| !e.interior),
            ..what
        });
        entities.push(rows_of(Mat4::from_scale_rotation_translation(
            Vec3::new(e.scale_xy, e.scale_xy, e.scale_z),
            Quat::from_array(e.rotation).normalize(),
            Vec3::from(e.position),
        )));
    }
    let mut lists = Vec::with_capacity(map.prop_lists.len() + map.grass.len());
    for list in &map.prop_lists {
        let (what, archetype_dist) = of(list.archetype_word);
        let (parent, _) = link(map, map.name, list.parent);
        let corners = (
            Vec3::from(list.batch_aabb.min),
            Vec3::from(list.batch_aabb.max),
        );
        nodes.push(list_node(
            corners,
            (list.lod_dist, archetype_dist),
            list.child_lod_dist,
            parent,
            list.lod_level,
        ));
        drawn.push(what);
        // A list states each instance as its three axes, each with one part of the translation.
        let rows = |a: &[[f32; 4]; 3]| -> Rows {
            std::array::from_fn(|row| [a[0][row], a[1][row], a[2][row], a[row][3]])
        };
        // ⚠ Only a list that nothing stands in for draws by its instances' own distance: where
        // a parent draws a list's far instances, leaving them out one by one would leave holes.
        let alone = parent.is_none() && list.lod_level == ORPHAN && list.instance_lod_dist > 0.0;
        lists.push(List {
            instances: Instances::Placed(list.instances.iter().map(rows).collect()),
            reach: alone.then_some((list.instance_lod_dist, list.instance_lod_dist)),
            level: Level::from_raw(list.lod_level),
        });
    }
    for list in &map.grass {
        let (what, archetype_dist) = of(list.archetype_word);
        let corners = (
            Vec3::from(list.batch_aabb.min),
            Vec3::from(list.batch_aabb.max),
        );
        // A grass list names no parent and states no distance for children.
        nodes.push(list_node(
            corners,
            (list.lod_dist, archetype_dist),
            0.0,
            None,
            list.lod_level,
        ));
        drawn.push(Drawn {
            model: what.model.filter(|_| !list.slod),
            ..what
        });
        lists.push(List {
            instances: match list.slod {
                true => Instances::Placed(Vec::new()),
                false => Instances::Grass(list.clone()),
            },
            reach: Some((list.lod_fade_start_dist, list.instance_lod_dist)),
            level: Level::from_raw(list.lod_level),
        });
    }
    ReadMap {
        nodes,
        loaded: Loaded {
            drawn,
            entities,
            lists,
            interiors: Vec::new(),
        },
    }
}
