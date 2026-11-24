//! Which maps a camera streams in, and which nodes of the loaded maps' level-of-detail forest draw
//! and how opaque: a parent and its children cross-fade by the distance to the parent, a parent is
//! held while a child is not ready, what comes into view fades in by time, and an instance of a
//! list is left out while the entity that stands in for it draws.
//!
//! ⚠ A child is judged by the distance to its **parent's** position, never its own, so the
//! children of one parent appear and go together.

use std::collections::HashMap;

use glam::Vec3;

/// How far past its box a map streams in, in metres.
pub const PRESTREAM: f32 = 50.0;
/// The band past the distance a node draws to over which it fades out, in metres.
pub const FADE: f32 = 20.0;
/// The band of an orphan that draws no farther than [`FADE`], in metres.
pub const FADE_SMALL: f32 = 5.0;
/// How much of a fade in time one second covers: 32 parts of 255 each step.
pub const FADE_RATE: f32 = 32.0 / 255.0 / STEP;
/// Faster than this nothing fades in time, in metres a second.
pub const FADE_MAX_SPEED: f32 = 80.0;
/// From this speed on a camera asks for no map of the high tier, in metres a second.
pub const HD_MAX_SPEED: f32 = 40.0;
/// The longest step a fade in time takes in one selection, in seconds.
const STEP: f32 = 0.033;

/// A node's level of detail, numbered as a map states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Level {
    Hd = 0,
    Lod = 1,
    Slod1 = 2,
    Slod2 = 3,
    Slod3 = 4,
    #[default]
    OrphanHd = 5,
    Slod4 = 6,
}

impl Level {
    pub const COUNT: usize = 7;

    /// The level a map's number names; a number past the last reads as an orphan.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Hd,
            1 => Self::Lod,
            2 => Self::Slod1,
            3 => Self::Slod2,
            4 => Self::Slod3,
            6 => Self::Slod4,
            _ => Self::OrphanHd,
        }
    }
}

/// The camera a selection is made for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub eye: Vec3,
    /// Metres a second.
    pub velocity: Vec3,
    /// What the distances of each level are multiplied by, by `Level as usize`.
    pub scales: [f32; Level::COUNT],
    /// Seconds since the selection before this one; 0 ends every fade in time at once.
    pub dt: f32,
}

impl View {
    /// A camera at rest at `eye`, every level scaled by `scale`, every fade in time ended.
    pub fn at(eye: Vec3, scale: f32) -> Self {
        Self {
            eye,
            velocity: Vec3::ZERO,
            scales: [scale; Level::COUNT],
            dt: 0.0,
        }
    }
}

/// The tier a map streams in by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    High,
    HighCritical,
    Medium,
    Low,
}

/// How far past its box a map of each tier streams in, by `Tier as usize`, in metres.
// One range a tier is the engine's shape; that the four are equal is not established for GTA VI.
const TIER_PRESTREAM: [f32; 4] = [PRESTREAM; 4];

impl Tier {
    /// The tier a map's content flags put it in.
    pub fn of(content_flags: u32) -> Self {
        const LOD: u32 = 1 << 1;
        // Container levels of detail, and the two kinds of distant lights.
        const LOW: u32 = (1 << 2) | (1 << 7) | (1 << 8);
        // Critical entities, and instance lists.
        const CRITICAL: u32 = (1 << 9) | (1 << 10);
        if content_flags & LOD != 0 {
            Self::Medium
        } else if content_flags & LOW != 0 {
            Self::Low
        } else if content_flags & CRITICAL != 0 {
            Self::HighCritical
        } else {
            Self::High
        }
    }
}

/// The two boxes a map states: the one it streams in by, and the one its entities stand in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapBox {
    pub streaming: (Vec3, Vec3),
    pub entities: (Vec3, Vec3),
}

impl MapBox {
    /// Whether a camera streams the map in; `content_flags` are the map's own.
    pub fn wanted(&self, view: &View, content_flags: u32) -> bool {
        let tier = Tier::of(content_flags);
        // ⚠ The high tier alone is held back at speed: a critical map still streams in.
        if tier == Tier::High && view.velocity.length() >= HD_MAX_SPEED {
            return false;
        }
        self.within(view.eye, TIER_PRESTREAM[tier as usize])
    }

    /// Whether a camera at rest at `at` streams in a map of no stated tier.
    pub fn wants(&self, at: Vec3) -> bool {
        self.within(at, PRESTREAM)
    }

    fn within(&self, at: Vec3, grow: f32) -> bool {
        let (s, e) = (self.streaming, self.entities);
        // A streaming box that does not hold the entities is not the one the map streams by.
        let holds = s.0.cmple(e.0).all() && s.1.cmpge(e.1).all();
        let (min, max) = if holds { s } else { e };
        let grow = Vec3::splat(grow);
        at.cmpge(min - grow).all() && at.cmple(max + grow).all()
    }
}

/// A node's parent: the map it lies in, by the caller's key, and its place among that map's nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentRef {
    pub map: u64,
    pub node: u32,
}

/// What a node's distance is measured to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extent {
    /// Its position.
    Point,
    /// The box of a list of instances, the lowest corner and the highest: nought inside it.
    Box(Vec3, Vec3),
}

/// A node of the forest: an entity, or a whole list of instances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub position: Vec3,
    pub extent: Extent,
    /// How far it draws, in metres, the archetype's where the entity states none.
    pub lod_dist: f32,
    /// Within this its children draw in its place; not above 0 where it states none.
    pub child_lod_dist: f32,
    pub parent: Option<ParentRef>,
    /// The instance of its parent list it stands in for, where its parent is a list and it names one.
    pub instance: Option<u32>,
    pub level: Level,
}

/// Where a node's model stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// On the device: it draws.
    Ready,
    /// Asked for or not yet: its parent draws in its place until it arrives.
    Loading,
    /// It has no model and never will: it draws nothing and holds nothing back.
    Absent,
}

/// A node by its map's key and its place among the map's nodes.
pub type NodeId = (u64, u32);

/// An entity that draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeDraw {
    pub node: NodeId,
    /// How opaque, above 0 and to 1.
    pub fade: f32,
}

/// A list that draws, and the instances left out of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListDraw {
    pub node: NodeId,
    /// How opaque every instance of it drawn is, above 0 and to 1.
    pub fade: f32,
    /// A range of [`Selection::skipped`].
    pub skipped: std::ops::Range<usize>,
}

/// What a selection draws and what it waits for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    /// The entities that draw.
    pub drawn: Vec<NodeDraw>,
    /// The lists that draw.
    pub lists: Vec<ListDraw>,
    /// The instances left out of the lists, list after list.
    pub skipped: Vec<u32>,
    /// The nodes in range whose model is loading, nearest ancestors first.
    pub wanted: Vec<NodeId>,
    /// Whether a fade in time is under way: the next selection differs though nothing moves.
    pub fading: bool,
}

const NONE: u32 = u32::MAX;

struct Loaded {
    key: u64,
    nodes: Vec<Node>,
    /// Whether every wanted map that names this one as a parent is loaded.
    complete: bool,
    /// How opaque each node drew at the selection before this one, and at this one.
    was: Vec<f32>,
    now: Vec<f32>,
}

/// The links of the loaded nodes, numbered map after map; made again when a map comes or goes.
#[derive(Default)]
struct Links {
    /// The first node of each map.
    base: Vec<u32>,
    /// The map of each node, by its place among the maps.
    slot: Vec<u32>,
    parent: Vec<u32>,
    first_child: Vec<u32>,
    next_sibling: Vec<u32>,
    /// How far each node draws once its parent's `child_lod_dist` has taken the place of its own.
    range: Vec<f32>,
    /// Within this a node's children draw in its place: its own, else the farthest of its children's.
    child_range: Vec<f32>,
    /// The level of the children that stand in for the whole of each node, which scales `child_range`.
    child_level: Vec<Level>,
    roots: Vec<u32>,
}

impl Links {
    /// The map of node `at`, by its place among the maps, and the node's place in that map.
    fn place(&self, at: usize) -> (usize, usize) {
        let slot = self.slot[at] as usize;
        (slot, at - self.base[slot] as usize)
    }
}

/// The level-of-detail forest of the maps streamed in.
#[derive(Default)]
pub struct Forest {
    maps: Vec<Loaded>,
    slots: HashMap<u64, usize>,
    links: Links,
    stale: bool,
    /// Whether each node gives way to its children this selection: 0 not yet known, 1 yes, 2 no.
    yields: Vec<u8>,
}

impl Forest {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the map `key` is loaded.
    pub fn holds(&self, key: u64) -> bool {
        self.slots.contains_key(&key)
    }

    /// How many maps and how many nodes are loaded.
    pub fn len(&self) -> (usize, usize) {
        let nodes = self.maps.iter().map(|m| m.nodes.len()).sum();
        (self.maps.len(), nodes)
    }

    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    /// Load the nodes of the map `key`, in place of any it held.
    pub fn insert(&mut self, key: u64, nodes: Vec<Node>) {
        let loaded = Loaded {
            key,
            complete: true,
            was: vec![0.0; nodes.len()],
            now: vec![0.0; nodes.len()],
            nodes,
        };
        match self.slots.get(&key) {
            Some(at) => self.maps[*at] = loaded,
            None => {
                self.slots.insert(key, self.maps.len());
                self.maps.push(loaded);
            }
        }
        self.stale = true;
    }

    /// Let the map `key` go.
    pub fn remove(&mut self, key: u64) {
        let Some(at) = self.slots.remove(&key) else {
            return;
        };
        self.maps.swap_remove(at);
        if let Some(moved) = self.maps.get(at) {
            self.slots.insert(moved.key, at);
        }
        self.stale = true;
    }

    /// Say whether every wanted map that names the map `key` as a parent is loaded: until then none
    /// of its nodes gives way to its children.
    pub fn set_complete(&mut self, key: u64, complete: bool) {
        if let Some(at) = self.slots.get(&key) {
            self.maps[*at].complete = complete;
        }
    }

    /// The node `id` names, where its map is loaded.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.maps
            .get(*self.slots.get(&id.0)?)?
            .nodes
            .get(id.1 as usize)
    }

    fn relink(&mut self) {
        let mut links = Links::default();
        let mut total = 0u32;
        for map in &self.maps {
            links.base.push(total);
            total += map.nodes.len() as u32;
        }
        let count = total as usize;
        links.slot = Vec::with_capacity(count);
        links.parent = vec![NONE; count];
        links.first_child = vec![NONE; count];
        links.next_sibling = vec![NONE; count];
        links.range = Vec::with_capacity(count);
        links.child_range = Vec::with_capacity(count);
        links.child_level = Vec::with_capacity(count);
        for (slot, map) in self.maps.iter().enumerate() {
            for node in &map.nodes {
                links.slot.push(slot as u32);
                links.range.push(node.lod_dist);
                links.child_range.push(node.child_lod_dist);
                links.child_level.push(node.level);
            }
        }
        let mut at = 0usize;
        for map in &self.maps {
            for node in &map.nodes {
                let parent = node.parent.and_then(|p| {
                    let slot = *self.slots.get(&p.map)?;
                    ((p.node as usize) < self.maps[slot].nodes.len())
                        .then(|| links.base[slot] + p.node)
                });
                // A node that names itself, or a parent not loaded, stands as a root.
                match parent.filter(|p| *p as usize != at) {
                    Some(parent) => {
                        links.parent[at] = parent;
                        links.next_sibling[at] = links.first_child[parent as usize];
                        links.first_child[parent as usize] = at as u32;
                    }
                    None => links.roots.push(at as u32),
                }
                at += 1;
            }
        }
        // A parent that states no distance for its children takes the farthest of theirs, and one
        // that states one gives it to each child that stands in for the whole of it.
        for child in 0..count {
            let parent = links.parent[child];
            let node = *self.node_at(&links, child);
            if parent == NONE || node.instance.is_some() {
                continue;
            }
            links.child_level[parent as usize] = node.level;
            let stated = self.node_at(&links, parent as usize).child_lod_dist;
            if stated > 0.0 {
                links.range[child] = stated;
            } else {
                let far = links.child_range[parent as usize].max(links.range[child]);
                links.child_range[parent as usize] = far;
            }
        }
        self.links = links;
        self.stale = false;
    }

    fn node_at(&self, links: &Links, at: usize) -> &Node {
        let (slot, node) = links.place(at);
        &self.maps[slot].nodes[node]
    }

    fn id_of(&self, at: u32) -> NodeId {
        let (slot, node) = self.links.place(at as usize);
        (self.maps[slot].key, node as u32)
    }

    /// The nodes that draw for `view` and how opaque each is, and those waited for, by where `state` says each model stands.
    pub fn select(&mut self, view: &View, state: &dyn Fn(NodeId) -> NodeState) -> Selection {
        if self.stale {
            self.relink();
        }
        self.yields.clear();
        self.yields.resize(self.links.slot.len(), 0);
        for map in &mut self.maps {
            std::mem::swap(&mut map.was, &mut map.now);
            map.now.fill(0.0);
        }
        // ⚠ The step is capped: a selection made long after the last one starts a fade, it does
        // not end it.
        let timed = view.dt > 0.0 && view.velocity.length() <= FADE_MAX_SPEED;
        let mut pass = Pass {
            forest: self,
            view,
            step: timed.then(|| FADE_RATE * view.dt.min(STEP)),
            state,
            out: Selection::default(),
        };
        for k in 0..pass.forest.links.roots.len() {
            let root = pass.forest.links.roots[k];
            pass.draw(root);
        }
        pass.out
    }
}

struct Pass<'a> {
    forest: &'a mut Forest,
    view: &'a View,
    /// How much a fade in time gains this selection; `None` where nothing fades in time.
    step: Option<f32>,
    state: &'a dyn Fn(NodeId) -> NodeState,
    out: Selection,
}

impl Pass<'_> {
    fn node(&self, at: u32) -> Node {
        *self.forest.node_at(&self.forest.links, at as usize)
    }

    /// How far the camera is from what node `at` is measured to.
    fn away(&self, at: u32) -> f32 {
        let (node, eye) = (self.node(at), self.view.eye);
        match node.extent {
            Extent::Point => eye.distance(node.position),
            Extent::Box(min, max) => eye.distance(eye.clamp(min, max)),
        }
    }

    /// How far the camera is from what node `at` is judged by: its parent where it stands in for
    /// the whole of one, itself otherwise.
    fn judged(&self, at: u32) -> f32 {
        let parent = self.forest.links.parent[at as usize];
        let whole_child = parent != NONE && self.node(at).instance.is_none();
        self.away(if whole_child { parent } else { at })
    }

    /// How far node `at` draws whole, by the scale of its level.
    fn reach(&self, at: u32) -> f32 {
        self.forest.links.range[at as usize] * self.view.scales[self.node(at).level as usize]
    }

    /// The distance within which the children of node `at` draw whole, by the scale of their level.
    fn child_reach(&self, at: u32) -> f32 {
        let links = &self.forest.links;
        links.child_range[at as usize] * self.view.scales[links.child_level[at as usize] as usize]
    }

    /// How opaque the distance alone makes node `at`: whole within the distance it draws to, and
    /// gone a band past it.
    fn by_distance(&self, at: u32) -> f32 {
        // ⚠ The band is picked by the distance the node states, before any scale.
        let small =
            self.node(at).level == Level::OrphanHd && self.forest.links.range[at as usize] <= FADE;
        let band = if small { FADE_SMALL } else { FADE };
        ((self.reach(at) + band - self.judged(at)) / band).clamp(0.0, 1.0)
    }

    fn first_child(&self, at: u32) -> u32 {
        self.forest.links.first_child[at as usize]
    }

    fn next_sibling(&self, at: u32) -> u32 {
        self.forest.links.next_sibling[at as usize]
    }

    /// Whether node `at` gives way to the children that stand in for the whole of it: it has some,
    /// its map's child maps are loaded, the camera is within the band before their distance and
    /// each is ready.
    fn yields(&mut self, at: u32) -> bool {
        match self.forest.yields[at as usize] {
            1 => return true,
            2 => return false,
            _ => {}
        }
        let complete = self.forest.maps[self.forest.links.slot[at as usize] as usize].complete;
        let near = self.away(at) <= self.child_reach(at) + FADE;
        let (mut any, mut ready) = (false, true);
        let mut child = if near { self.first_child(at) } else { NONE };
        // Every child is asked, so that each one still loading is waited for.
        while child != NONE {
            if self.node(child).instance.is_none() {
                any = true;
                ready &= self.ready(child);
            }
            child = self.next_sibling(child);
        }
        let yields = any && ready && complete;
        self.forest.yields[at as usize] = if yields { 1 } else { 2 };
        yields
    }

    /// Whether node `at` can take its parent's place: its model is on the device, or it gives way
    /// to children that can.
    fn ready(&mut self, at: u32) -> bool {
        if self.yields(at) {
            return true;
        }
        match (self.state)(self.forest.id_of(at)) {
            NodeState::Ready | NodeState::Absent => true,
            NodeState::Loading => {
                self.out.wanted.push(self.forest.id_of(at));
                false
            }
        }
    }

    /// Whether node `at` fades in by time: one of the high level, an orphan, or one with no parent.
    fn fades_in_time(&self, at: u32) -> bool {
        matches!(self.node(at).level, Level::Hd | Level::OrphanHd)
            || self.forest.links.parent[at as usize] == NONE
    }

    /// How opaque node `at` draws now where `target` is what the distances allow: what drew less
    /// a selection ago gains one step, and the rest is there at once.
    fn settle(&mut self, at: u32, target: f32) -> f32 {
        let (slot, node) = self.forest.links.place(at as usize);
        let was = self.forest.maps[slot].was[node];
        let step = (self.step).filter(|_| target > was && self.fades_in_time(at));
        let fade = step.map_or(target, |step| (was + step).min(target));
        self.out.fading |= fade < target;
        self.forest.maps[slot].now[node] = fade;
        fade
    }

    /// Draw node `at`, whose parent has given way to it or which has none; whether it still fades
    /// in by time where the distance alone draws it whole.
    fn draw(&mut self, at: u32) -> bool {
        let whole = self.by_distance(at);
        if whole <= 0.0 {
            return false;
        }
        let mut target = whole;
        if self.yields(at) {
            let mut held = false;
            let mut child = self.first_child(at);
            while child != NONE {
                if self.node(child).instance.is_none() {
                    held |= self.draw(child);
                }
                child = self.next_sibling(child);
            }
            // Its children draw whole from their distance inwards and it fades out over the band
            // inside that; while one of them still fades in by time it stays whole.
            if !held {
                let inside = self.child_reach(at) - self.away(at);
                target *= (1.0 - inside / FADE).clamp(0.0, 1.0);
            }
            if target <= 0.0 {
                return false;
            }
        }
        let id = self.forest.id_of(at);
        match (self.state)(id) {
            NodeState::Ready => {}
            NodeState::Loading => {
                self.out.wanted.push(id);
                return false;
            }
            NodeState::Absent => return false,
        }
        let fade = self.settle(at, target);
        match self.node(at).extent {
            Extent::Box(..) => self.draw_list(at, id, fade),
            Extent::Point => self.out.drawn.push(NodeDraw { node: id, fade }),
        }
        whole >= 1.0 && fade < target
    }

    /// Whether the entity `at`, which stands in for one instance of a list, is within the distance
    /// it draws to by its own place.
    fn stands_in(&self, at: u32) -> bool {
        self.away(at) <= self.reach(at)
    }

    /// Draw the list `at` less each instance whose own entity draws in its place.
    // ponytail: an entity takes its instance's place at once, with no fade; a fade an instance if the swap shows.
    fn draw_list(&mut self, at: u32, id: NodeId, fade: f32) {
        let from = self.out.skipped.len();
        let mut child = self.first_child(at);
        while child != NONE {
            if let Some(instance) = self.node(child).instance.filter(|_| self.stands_in(child)) {
                let child_id = self.forest.id_of(child);
                match (self.state)(child_id) {
                    NodeState::Ready => {
                        self.out.drawn.push(NodeDraw {
                            node: child_id,
                            fade: 1.0,
                        });
                        self.out.skipped.push(instance);
                    }
                    NodeState::Loading => self.out.wanted.push(child_id),
                    // An entity with no model leaves its instance drawn.
                    NodeState::Absent => {}
                }
            }
            child = self.next_sibling(child);
        }
        self.out.lists.push(ListDraw {
            node: id,
            fade,
            skipped: from..self.out.skipped.len(),
        });
    }
}

#[cfg(test)]
#[path = "streaming_tests.rs"]
mod tests;
