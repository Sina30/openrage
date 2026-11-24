//! What a scanned GTA VI map links to on the Map screen: each element's level-of-detail parent among
//! the map bodies open, its instances as points, an interior's rooms and portals counted, the
//! drawable dictionaries a scan-wide index of their keys resolves, and the models drawn in place
//! round the selection, read on a worker.
//!
//! ⚠ Where an interior's rooms and entities stand in the world is not measured, so they are listed
//! and never drawn.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use glam::{Mat3, Mat4, Vec3};
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9ZlibRun};
use or_io::gta6::headerless::{read_dictionary, Entry, Resource};
use or_io::gta6::map::links::{self as map_links, Interior, Links, LodTarget, MapLinks};
use or_io::gta6::map::{Archetype, ArchetypeClass, InstanceList, MapBody, MapData, MapError};
use or_job::{Cancel, Progress};
use or_renderer::LineVertex;

use super::{opened, ScannedMap, Source, BODY_LIMIT, KNOWN};
use crate::render::dressed::{DressedScene, Geometry, ScenePart};
use crate::state::jobs::{JobKind, Outcome, RunningJob, Then};
use crate::state::workbench::PickKind;
use crate::state::AppState;

/// The most instances the plan draws as points; the counts say how many there are.
pub const MOST_POINTS: usize = 20_000;
/// The most level-of-detail links drawn.
const MOST_LINKS: usize = 4_000;
/// The most models drawn in place at once, the nearest first.
pub const MOST_MODELS: usize = 48;
const LINK: [f32; 4] = [0.9, 0.55, 0.95, 0.9];

type LinksBy = BTreeMap<PathBuf, Result<Arc<Links>, String>>;

/// What each scanned map body opened links to, or why that was refused, by its path.
// ponytail: one map for the whole app, which no close empties, as the map bodies' is.
fn links() -> MutexGuard<'static, LinksBy> {
    static LINKS: Mutex<LinksBy> = Mutex::new(BTreeMap::new());
    LINKS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The interiors read, by the body open or the archive searched that holds them and their name word.
type InteriorsBy = BTreeMap<(PathBuf, u64), Arc<Interior>>;

fn interiors() -> MutexGuard<'static, InteriorsBy> {
    static INTERIORS: Mutex<InteriorsBy> = Mutex::new(BTreeMap::new());
    INTERIORS.lock().unwrap_or_else(|p| p.into_inner())
}

/// Each drawable dictionary body of an archive whose index is whole, by the key of each of its
/// drawables: every body that holds the key.
type KeysBy = BTreeMap<PathBuf, HashMap<u32, Vec<u64>>>;

fn dictionary_keys() -> MutexGuard<'static, KeysBy> {
    static KEYS: Mutex<KeysBy> = Mutex::new(BTreeMap::new());
    KEYS.lock().unwrap_or_else(|p| p.into_inner())
}

/// Keep what the map body at `path`, which the map reader read, links to.
pub(super) fn keep(path: &Path, virtual_section: &[u8], physical: &[u8]) {
    let Ok((_, read)) = map_links::open(virtual_section, physical) else {
        return;
    };
    if let Ok(Links::Types(found)) = &read {
        keep_interiors(path, found);
    }
    links().insert(
        path.to_path_buf(),
        read.map(Arc::new).map_err(|e| e.to_string()),
    );
    KNOWN.fetch_add(1, Ordering::Relaxed);
}

/// The interior of name word `word` that the first of `held_by`, bodies open or archives searched,
/// lays out.
pub(crate) fn interior_in(held_by: &[&Path], word: u64) -> Option<Arc<Interior>> {
    let kept = interiors();
    (held_by.iter()).find_map(|p| kept.get(&(p.to_path_buf(), word)).cloned())
}

/// Keep the interiors a map types body of `held_by`, a body open or an archive searched, links to.
pub(crate) fn keep_interiors(held_by: &Path, found: &[Interior]) {
    let mut kept = interiors();
    for interior in found {
        kept.insert(
            (held_by.to_path_buf(), interior.name_word),
            Arc::new(interior.clone()),
        );
    }
}

/// Keep the interiors of what a map types body of the archive `archive` links to.
pub(super) fn keep_read_interiors(archive: &Path, read: &Result<Links, MapError>) {
    if let Ok(Links::Types(found)) = read {
        keep_interiors(archive, found);
    }
}

/// The interior entity `index`'s archetype lays out, or why its links were refused; `None` for none read.
pub(crate) fn interior_of(
    scanned: &ScannedMap,
    index: usize,
) -> Option<Result<Arc<Interior>, String>> {
    let (source, a) = scanned.archetypes.get(index)?.as_ref()?;
    if a.class != ArchetypeClass::Interior {
        return None;
    }
    let held_by = match source {
        Source::Open(path) => {
            if let Some(Err(why)) = links().get(path) {
                return Some(Err(why.clone()));
            }
            path.clone()
        }
        Source::Scan(_) => crate::state::rpf9_scan::scanned_body(&scanned.path)?
            .0
            .to_path_buf(),
    };
    interiors().get(&(held_by, a.name_word)).cloned().map(Ok)
}

/// An element's level-of-detail parent, and what it names there once that map body is open.
#[derive(Debug, Clone)]
pub struct Parent {
    pub map: u32,
    pub index: i16,
    pub found: Option<(PathBuf, Option<LodTarget>)>,
}

/// What a scanned map links to as the screen shows it, worked out when the map is placed.
#[derive(Debug, Default)]
pub struct Shown {
    /// Why the map's links were refused, none when they read or were never read.
    pub refused: Option<String>,
    /// Each entity's, then each prop list's parent; `None` for one that names none.
    pub parents: Vec<Option<Parent>>,
    /// A line from each child to its parent, where the parent's map body is open.
    pub lines: Vec<LineVertex>,
    /// The instances' positions, at most [`MOST_POINTS`] of them.
    pub points: Vec<[f32; 3]>,
    pub prop_instances: usize,
    pub grass_instances: usize,
    /// The grass lists that name several archetypes, which instance takes which not being known.
    pub mixed_grass: usize,
}

fn centre(list: &InstanceList) -> Vec3 {
    (Vec3::from(list.batch_aabb.min) + Vec3::from(list.batch_aabb.max)) * 0.5
}

/// Where `target` stands in the parent map `p`, whose links are `pl` when they read.
fn parent_at(p: &MapData, pl: Option<&MapLinks>, target: LodTarget) -> Option<Vec3> {
    match target {
        LodTarget::Entity(i) => p.entities.get(i).map(|e| Vec3::from(e.position)),
        LodTarget::PropList { list, instance } => (instance)
            .and_then(|k| pl?.prop_points.get(list)?.get(k))
            .map(|w| Vec3::from(*w))
            .or_else(|| p.prop_lists.get(list).map(centre)),
        LodTarget::GrassList(i) => p.grass_lists.get(i).map(centre),
    }
}

/// What the map `m`, the body at `path`, links to, with each parent looked for among the map bodies open.
pub(super) fn shown(path: &Path, m: &MapData) -> Shown {
    let read = links().get(path).cloned();
    let links_of = match read {
        None => return Shown::default(),
        Some(Err(why)) => {
            return Shown {
                refused: Some(why),
                ..Shown::default()
            }
        }
        Some(Ok(l)) => l,
    };
    let Links::Data(l) = &*links_of else {
        return Shown::default();
    };
    let maps: HashMap<u32, (PathBuf, Arc<MapBody>)> = (opened().iter())
        .filter_map(|(p, b)| match b.as_ref().ok().map(|b| (&**b, b)) {
            Some((MapBody::Data(d), b)) => Some((d.name, (p.clone(), Arc::clone(b)))),
            _ => None,
        })
        .collect();
    let parent_links: HashMap<PathBuf, Arc<Links>> = {
        let kept = links();
        (maps.values())
            .filter_map(|(p, _)| Some((p.clone(), Arc::clone(kept.get(p)?.as_ref().ok()?))))
            .collect()
    };
    let children =
        (m.entities.iter().map(|e| Vec3::from(e.position))).chain(m.prop_lists.iter().map(centre));
    let mut shown = Shown::default();
    for (link, from) in l.entities.iter().chain(&l.prop_lists).zip(children) {
        let Some(name) = link.map.and_then(|s| l.lod_parents.get(s)).map(|p| p.name) else {
            shown.parents.push(None);
            continue;
        };
        let found = maps.get(&name).and_then(|(p, body)| match &**body {
            MapBody::Data(d) => Some((p, d)),
            MapBody::Types(_) => None,
        });
        let found = found.map(|(p, d)| {
            let target = link.target(d.entities.len(), d.prop_lists.len(), d.grass_lists.len());
            let pl = parent_links.get(p).and_then(|l| match &**l {
                Links::Data(pl) => Some(pl),
                Links::Types(_) => None,
            });
            let to = target.and_then(|t| parent_at(d, pl, t));
            if let Some(to) = to.filter(|_| shown.lines.len() < 2 * MOST_LINKS) {
                shown.lines.push(LineVertex::new(from.to_array(), LINK));
                shown.lines.push(LineVertex::new(to.to_array(), LINK));
            }
            (p.clone(), target)
        });
        shown.parents.push(Some(Parent {
            map: name,
            index: link.parent_index,
            found,
        }));
    }
    let grass = l.grass_lists.iter().flat_map(|g| &g.points);
    shown.points = (l.prop_points.iter().flatten().chain(grass))
        .take(MOST_POINTS)
        .copied()
        .collect();
    shown.prop_instances = l.prop_points.iter().map(Vec::len).sum();
    shown.grass_instances = l.grass_lists.iter().map(|g| g.points.len()).sum();
    shown.mixed_grass = l
        .grass_lists
        .iter()
        .filter(|g| !g.archetypes.is_empty())
        .count();
    shown
}

/// The level-of-detail lines of the scanned map shown, none for any other.
pub(crate) fn lod_lines(state: &AppState) -> &[LineVertex] {
    match state.scanned_map() {
        Some(s) => &s.links.lines,
        None => &[],
    }
}

/// Index every drawable dictionary body at `offsets` of `file`, the archive at `archive`, by its
/// drawables' keys, unless that archive's index is whole already; nothing is kept when stopped.
pub(super) fn index_dictionaries(
    archive: &Path,
    file: &File,
    offsets: &[u64],
    (cancel, progress): (&Cancel, &Progress),
    (done, total): (&mut u64, u64),
) {
    if dictionary_keys().contains_key(archive) {
        return;
    }
    let mut keys: HashMap<u32, Vec<u64>> = HashMap::new();
    for offset in offsets {
        if cancel.is_set() {
            return;
        }
        progress.reached(*done, total);
        *done += 1;
        for code in drawable_keys_at(file, *offset) {
            keys.entry(code).or_default().push(*offset);
        }
    }
    dictionary_keys().insert(archive.to_path_buf(), keys);
    KNOWN.fetch_add(1, Ordering::Relaxed);
}

/// The keys of the drawables of the dictionary body at `offset`, none for another body.
fn drawable_keys_at(file: &File, offset: u64) -> Vec<u32> {
    let Ok((find, bytes)) = read_rpf9_body(file, offset, BODY_LIMIT) else {
        return Vec::new();
    };
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        ..
    }) = find.kind
    else {
        return Vec::new();
    };
    let (virtual_section, physical) = resource.sections(&bytes);
    match read_dictionary(virtual_section, Some(physical)) {
        Ok(dict) => (dict.entries.iter())
            .filter(|(_, e)| matches!(e, Ok(Entry::Drawable(_))))
            .map(|(code, _)| *code)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The one body that holds every key of `names`, none where no body or several do.
fn sole_holder(keys: &HashMap<u32, Vec<u64>>, names: &HashSet<u32>) -> Option<u64> {
    let mut held: Option<HashSet<u64>> = None;
    for name in names {
        let bodies: HashSet<u64> = keys.get(name)?.iter().copied().collect();
        held = Some(match held {
            None => bodies,
            Some(h) => h.intersection(&bodies).copied().collect(),
        });
    }
    let held = held?;
    match held.len() {
        1 => held.into_iter().next(),
        _ => None,
    }
}

/// Fill in each entity whose archetype names a drawable dictionary: the one body the index of
/// `archive` holds every key of, the keys being the names of the map's archetypes that name it.
pub(super) fn dictionary_models(
    archive: Option<&Path>,
    archetypes: &[Option<(Source, Archetype)>],
    models: &mut [Option<u64>],
) {
    let Some(archive) = archive else {
        return;
    };
    let index = dictionary_keys();
    let Some(keys) = index.get(archive) else {
        return;
    };
    let mut groups: HashMap<u64, HashSet<u32>> = HashMap::new();
    for (_, a) in archetypes.iter().flatten() {
        if a.drawable_dictionary != 0 {
            groups
                .entry(a.drawable_dictionary)
                .or_default()
                .insert(a.name);
        }
    }
    let resolved: HashMap<u64, u64> = (groups.iter())
        .filter_map(|(word, names)| Some((*word, sole_holder(keys, names)?)))
        .collect();
    for (model, a) in models.iter_mut().zip(archetypes) {
        let word = a.as_ref().map_or(0, |(_, a)| a.drawable_dictionary);
        if model.is_none() {
            *model = resolved.get(&word).copied();
        }
    }
}

/// The entities whose archetype names a drawable dictionary that no index resolves.
pub(crate) fn unresolved_dictionaries(scanned: &ScannedMap) -> usize {
    (scanned.archetypes.iter().zip(&scanned.models))
        .filter(|(a, m)| m.is_none() && a.as_ref().is_some_and(|(_, a)| a.drawable_dictionary != 0))
        .count()
}

/// Whether the resolved models are drawn in place round the entity selected, and what was read.
#[derive(Default)]
pub struct InPlace {
    pub on: bool,
    /// Metres round the entity selected whose models are drawn as well; 0 draws its own alone.
    pub radius: f32,
    /// The radius while its control is dragged: no model is read again until the drag ends.
    pub dragged: Option<f32>,
    /// The models read, which the scene draws.
    pub scene: Option<Arc<DressedScene>>,
    /// The map shown, the placing and the entity selected the scene was asked for, and the radius' bits.
    asked: Option<(PathBuf, (u64, usize), usize, u32)>,
    job: Option<ModelsRead>,
}

/// A model to draw: its body, the dictionary entry where the body is a dictionary, and its placing.
struct Target {
    offset: u64,
    entry: Option<u32>,
    place: Mat4,
}

/// The models of entity `at` and of those within `radius` metres of it, the nearest first.
fn targets(scanned: &ScannedMap, at: usize, radius: f32) -> Vec<Target> {
    let Some(here) = scanned.scene.placed.get(at).map(|p| p.obb.position) else {
        return Vec::new();
    };
    let mut near: Vec<(f32, usize)> = (scanned.models.iter().enumerate())
        .filter(|(_, m)| m.is_some())
        .filter_map(|(i, _)| {
            let d = scanned.scene.placed.get(i)?.obb.position.distance(here);
            (i == at || d <= radius).then_some((d, i))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(MOST_MODELS);
    (near.into_iter())
        .filter_map(|(_, i)| {
            let offset = scanned.models.get(i).copied().flatten()?;
            let (_, a) = scanned.archetypes.get(i)?.as_ref()?;
            let obb = scanned.scene.placed.get(i)?.obb;
            Some(Target {
                offset,
                entry: (a.drawable_dictionary != 0).then_some(a.name),
                place: Mat4::from_scale_rotation_translation(obb.scale, obb.rotation, obb.position),
            })
        })
        .collect()
}

/// The geometry of the model `t` names, placed in the world; `None` where it does not read.
fn model_in_place(file: &File, t: &Target) -> Option<Vec<Geometry>> {
    let (find, bytes) = read_rpf9_body(file, t.offset, BODY_LIMIT).ok()?;
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        signature: Some(signature),
        ..
    }) = find.kind
    else {
        return None;
    };
    let (virtual_section, physical) = resource.sections(&bytes);
    let read = or_io::gta6::headerless::read(signature, virtual_section, Some(physical)).ok()?;
    let drawable = match (&read, t.entry) {
        (Resource::Drawable(d), None) => d,
        (Resource::DrawableWithBound(r), None) => &r.drawable,
        (Resource::Dictionary(dict), Some(code)) => {
            (dict.entries.iter()).find_map(|(c, e)| match e {
                Ok(Entry::Drawable(d)) if *c == code => Some(&**d),
                _ => None,
            })?
        }
        _ => return None,
    };
    let normals = Mat3::from_mat4(t.place).inverse().transpose();
    let meshes = crate::render::gta6_drawable::meshes_at(drawable, 0, None, None, &|_| None);
    let placed = (meshes.into_iter())
        .map(|mesh| Geometry {
            vertices: (mesh.vertices.into_iter())
                .map(|mut v| {
                    v.position = t.place.transform_point3(Vec3::from(v.position)).to_array();
                    v.normal = (normals * Vec3::from(v.normal))
                        .normalize_or_zero()
                        .to_array();
                    v
                })
                .collect(),
            indices: mesh.indices,
        })
        .collect();
    Some(placed)
}

enum ModelsEnded {
    /// The models that read, of how many were asked for.
    Read(Vec<ScenePart>, usize),
    Stopped,
    Failed(String),
}

struct ModelsRead {
    doing: String,
    cancel: Cancel,
    progress: Progress,
    ended: Arc<Mutex<Option<ModelsEnded>>>,
}

impl ModelsRead {
    /// Read the models `targets` names from `archive`, on a worker.
    fn start(archive: PathBuf, targets: Vec<Target>) -> Self {
        let read = Self {
            doing: tr_n!(
                "Reading {count} GTA VI model to draw in place",
                "Reading {count} GTA VI models to draw in place",
                targets.len()
            ),
            cancel: Cancel::new(),
            progress: Progress::new(),
            ended: Arc::default(),
        };
        let (cancel, progress, ended) = (
            read.cancel.clone(),
            read.progress.clone(),
            Arc::clone(&read.ended),
        );
        std::thread::spawn(move || {
            let outcome = match File::open(&archive) {
                Err(e) => ModelsEnded::Failed(format!("{}: {e}", archive.display())),
                Ok(file) => {
                    let mut parts = Vec::new();
                    for (k, t) in targets.iter().enumerate() {
                        if cancel.is_set() {
                            break;
                        }
                        progress.reached(k as u64, targets.len() as u64);
                        if let Some(geometry) = model_in_place(&file, t) {
                            parts.push(ScenePart {
                                geometry: Arc::new(geometry),
                                diffuse: None,
                                tint: [1.0; 3],
                                lift: 0.0,
                                garment: None,
                            });
                        }
                    }
                    match cancel.is_set() {
                        true => ModelsEnded::Stopped,
                        false => ModelsEnded::Read(parts, targets.len()),
                    }
                }
            };
            *ended.lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome);
        });
        read
    }
}

impl Drop for ModelsRead {
    fn drop(&mut self) {
        self.cancel.set();
    }
}

/// Names each scene of models read, so the viewport builds its draws again for a new one alone.
static READS: AtomicU64 = AtomicU64::new(1);

impl AppState {
    /// Read the models to draw in place again when the map, the entity selected or the radius
    /// changed since, and take what a read that ended found.
    pub(super) fn refresh_models_in_place(&mut self) {
        let selected = match self.workbench.maps.selected.as_ref().map(|p| p.kind) {
            Some(PickKind::Entity(i)) => Some(i),
            _ => None,
        };
        let maps = &mut self.workbench.maps;
        let (scanned, place) = (&maps.scanned, &mut maps.in_place);
        let now = match (scanned, place.on, selected) {
            (Some(s), true, Some(i)) => Some((&s.path, s.key, i, place.radius.to_bits())),
            _ => None,
        };
        let asked = (place.asked.as_ref()).map(|(p, key, i, radius)| (p, *key, *i, *radius));
        if asked != now {
            place.asked = now.map(|(p, key, i, radius)| (p.clone(), key, i, radius));
            place.job = None;
            place.scene = None;
            if let (Some(s), Some(i)) = (scanned, selected.filter(|_| place.on)) {
                let archive =
                    crate::state::rpf9_scan::scanned_body(&s.path).map(|(a, _)| a.to_path_buf());
                let wanted = targets(s, i, place.radius);
                if let (Some(archive), false) = (archive, wanted.is_empty()) {
                    place.job = Some(ModelsRead::start(archive, wanted));
                }
            }
        }
        let ended = (place.job.as_ref())
            .and_then(|j| j.ended.lock().unwrap_or_else(|p| p.into_inner()).take());
        let Some(ended) = ended else {
            return;
        };
        place.job = None;
        let (outcome, said) = match ended {
            ModelsEnded::Read(parts, of) => {
                let said = tr_n!(
                    "{count} of {of} GTA VI model drawn in place",
                    "{count} of {of} GTA VI models drawn in place",
                    parts.len(),
                    of = of
                );
                let key = READS.fetch_add(1, Ordering::Relaxed);
                place.scene = Some(Arc::new(DressedScene::new(key, parts)));
                (Outcome::Done(Then::Nothing), said)
            }
            ModelsEnded::Stopped => (
                Outcome::Stopped(Then::Nothing),
                tr!("Stopped before it finished").to_string(),
            ),
            ModelsEnded::Failed(why) => (
                Outcome::Failed,
                tr!("The models were not read: {why}", why = why),
            ),
        };
        self.jobs.record(JobKind::ArchetypeSearch, outcome, said);
    }

    /// The read of the models to draw in place, while it runs, as the job list draws it.
    pub(super) fn models_in_place_job(&self) -> Option<RunningJob> {
        let job = self.workbench.maps.in_place.job.as_ref()?;
        Some(RunningJob {
            kind: JobKind::ArchetypeSearch,
            doing: job.doing.clone(),
            fraction: Some(job.progress.snapshot().fraction()),
            detail: String::new(),
            stopping: job.cancel.is_set(),
        })
    }

    pub(super) fn stop_models_in_place(&self) {
        if let Some(job) = &self.workbench.maps.in_place.job {
            job.cancel.set();
        }
    }

    /// The models the scene draws in place, none while there are none to draw.
    pub(crate) fn models_in_place(&self) -> Option<&DressedScene> {
        let place = &self.workbench.maps.in_place;
        place
            .scene
            .as_deref()
            .filter(|_| place.on && self.scanned_map().is_some())
    }

    /// Open the model of the entity selected on the shown scanned map, where one is known.
    pub(crate) fn open_selected_gta6_model(&mut self) {
        let Some(PickKind::Entity(i)) = self.workbench.maps.selected.as_ref().map(|p| p.kind)
        else {
            return;
        };
        let model = (self.scanned_map()).and_then(|s| s.models.get(i).copied().flatten());
        if let Some(offset) = model {
            self.open_gta6_model(offset);
        }
    }
}

#[cfg(test)]
#[path = "gta6_map_links_tests.rs"]
mod tests;
