//! `openrage-cli bounds`: does a bounding volume recomputed from the
//! vertices agree with the one the file shipped?
//!
//! ⚠ **This is the only way to find out whether a recompute is safe to
//! offer.** A bound the engine authored may be tighter, looser or in a
//! different space from one derived from the positions, and the difference
//! only shows against shipped data. A recompute that quietly loosened every
//! box would pass every unit test written against itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use or_io::codec::mesh::{aabb_slot_of, aabb_slots, model_aabbs, Aabb, AabbBuilder, VertexReader};
use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_io::decl::{Gen8, Gen9, Shared};
use or_io::gta5::drawable::DrawableTarget;
use or_io::gta5::{
    File as _, GrmGeometry, GrmModel, GtaDrawable, GtaFragType, PgDictionary, RmcDrawable, RmcLod,
    SpdAABB, Vec4V,
};
use or_io::Generation;
use or_title::install::crawl::{self, ControlFlow};

use super::keys::resolve_crypto;
use super::search::resolve_game_root;

/// A box as the file ships it: two corners, each fourth lane zero.
fn to_box(box_: Aabb) -> SpdAABB {
    let corner = |p: [f32; 3]| Vec4V {
        x: p[0],
        y: p[1],
        z: p[2],
        ..Default::default()
    };
    SpdAABB {
        min: corner(box_.min),
        max: corner(box_.max),
        ..Default::default()
    }
}

fn from_box(box_: &SpdAABB) -> Aabb {
    Aabb {
        min: [box_.min.x, box_.min.y, box_.min.z],
        max: [box_.max.x, box_.max.y, box_.max.z],
    }
}

/// `computed`'s extents under `shipped`'s fourth lanes, which are not padding.
fn with_lanes_of(computed: SpdAABB, shipped: &SpdAABB) -> SpdAABB {
    let mut out = computed;
    out.min.w = shipped.min.w;
    out.max.w = shipped.max.w;
    out
}

/// The box over a geometry's positions; `None` when it has none to read.
fn geometry_aabb<T: DrawableTarget>(geometry: &GrmGeometry<T>) -> Option<SpdAABB> {
    VertexReader::new(&*geometry.vertex_buffer)?
        .aabb()
        .map(to_box)
}

/// The union of a model's geometry boxes; `None` when any geometry has none.
fn model_aabb<T: DrawableTarget>(model: &GrmModel<T>) -> Option<SpdAABB> {
    let mut union = AabbBuilder::new();
    for geometry in &model.geometries {
        union.absorb(&from_box(&geometry_aabb(geometry)?));
    }
    union.finish().map(to_box)
}

/// Every level of detail a drawable owns, by slot, then its container level at `usize::MAX`.
fn levels<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<(usize, &RmcLod<T>)> {
    let mut out: Vec<(usize, &RmcLod<T>)> = drawable
        .lod_group
        .lods
        .iter()
        .map(|l| &**l)
        .enumerate()
        .collect();
    if let Shared::Own(Some(container)) = &drawable.container {
        out.push((usize::MAX, &**container));
    }
    out
}

/// Rewrite every bounding volume a drawable owns from its vertices, bottom up.
///
/// ⚠ A model whose geometries cannot all be read keeps the boxes it shipped,
/// and the drawable's box is the union over every level, not the highest.
fn recompute_bounds<T: DrawableTarget>(drawable: &mut RmcDrawable<T>) -> bool {
    let mut all = true;
    let mut models: Vec<&mut GrmModel<T>> = Vec::new();
    for lod in drawable.lod_group.lods.iter_mut() {
        models.extend(lod.models.iter_mut().map(|m| &mut **m));
    }
    if let Shared::Own(Some(container)) = &mut drawable.container {
        models.extend(container.models.iter_mut().map(|m| &mut **m));
    }
    for model in models {
        let per_geometry = (model.geometries.iter())
            .map(|g| geometry_aabb(g).map(|b| from_box(&b)))
            .collect::<Option<Vec<_>>>();
        let Some(per_geometry) = per_geometry else {
            all = false;
            continue;
        };
        model.aabbs = model_aabbs(per_geometry)
            .into_iter()
            .enumerate()
            .map(|(slot, box_)| match model.aabbs.get(slot) {
                Some(shipped) => with_lanes_of(to_box(box_), shipped),
                None => to_box(box_),
            })
            .collect();
    }
    let mut union = AabbBuilder::new();
    for (_, lod) in levels(drawable) {
        let mut lod_box = AabbBuilder::new();
        let every = lod.models.iter().all(|m| match model_aabb(m) {
            Some(b) => {
                lod_box.absorb(&from_box(&b));
                true
            }
            None => false,
        });
        if let Some(b) = lod_box.finish().filter(|_| every) {
            union.absorb(&b);
        }
    }
    let Some(box_) = union.finish() else {
        return false;
    };
    let shipped = drawable.lod_group.bounds_box.clone();
    drawable.lod_group.bounds_box = with_lanes_of(to_box(box_), &shipped);
    let ([x, y, z], radius) = box_.bounding_sphere();
    let sphere = &mut drawable.lod_group.bounds_sphere.center_and_radius;
    (sphere.x, sphere.y, sphere.z, sphere.w) = (x, y, z, radius);
    all
}

/// Extensions whose resources carry an `rmcDrawable`.
const DRAWABLE: [&str; 3] = ["ydr", "ydd", "yft"];

/// The relative errors the report bins into, largest corner error over the
/// box's own extent.
///
/// ⚠ **Relative, and reported as a distribution rather than against one
/// threshold.** Any single tolerance decides the answer: at `1e-4` almost
/// every cloud-hat drawable reads as a disagreement over a difference of two
/// ten-thousandths, and at `1e-2` a genuinely rotated box reads as a match.
/// Binning says which it is.
const BINS: [f32; 5] = [1e-6, 1e-5, 1e-4, 1e-3, 1e-2];

/// Where a recompute has to be inside the shipped box to count as contained.
///
/// A bound that no longer covers its own mesh is a rendering defect; one
/// that covers more than it needs is only slack. So containment is the
/// question worth a yes or no, and the rest is a distribution.
const CONTAINMENT_SLACK: f32 = 1e-3;

/// Above this, a difference is a different box rather than a rounding of the
/// same one. What `--explain` prints.
const BIG: f32 = 1e-2;

#[derive(Default, Clone, Copy)]
struct Agreement {
    total: usize,
    exact: usize,
    /// One bin per entry of [`BINS`], plus a final bin for everything worse;
    /// `line` makes them cumulative.
    bins: [usize; BINS.len() + 1],
    contained: usize,
    worst: f32,
}

impl Agreement {
    fn record(&mut self, shipped: &SpdAABB, computed: &SpdAABB) {
        self.total += 1;
        if same_bits(shipped, computed) {
            self.exact += 1;
        }
        let error = relative_error(shipped, computed);
        let bin = BINS
            .iter()
            .position(|limit| error <= *limit)
            .unwrap_or(BINS.len());
        self.bins[bin] += 1;
        if contains(shipped, computed) {
            self.contained += 1;
        }
        if error > self.worst {
            self.worst = error;
        }
    }

    fn line(&self, label: &str) -> String {
        let total = self.total.max(1) as f64;
        let mut running = 0usize;
        let mut cumulative = String::new();
        for bin in &self.bins[..BINS.len()] {
            running += bin;
            cumulative.push_str(&format!("{:>9.2}%", 100.0 * running as f64 / total));
        }
        format!(
            "{label:<22} {:>8} {:>8.2}%{cumulative} {:>8} {:>8.2}%\n",
            self.total,
            100.0 * self.exact as f64 / total,
            self.bins[BINS.len()],
            100.0 * self.contained as f64 / total,
        )
    }
}

/// The largest corner difference, over the larger box's own extent.
fn relative_error(a: &SpdAABB, b: &SpdAABB) -> f32 {
    let scale = corners(a)
        .iter()
        .chain(corners(b).iter())
        .fold(0.0f32, |acc, v| acc.max(v.abs()))
        .max(1.0);
    corners(a)
        .iter()
        .zip(corners(b).iter())
        .fold(0.0f32, |acc, (l, r)| acc.max((l - r).abs()))
        / scale
}

fn corners(box_: &SpdAABB) -> [f32; 6] {
    [
        box_.min.x, box_.min.y, box_.min.z, box_.max.x, box_.max.y, box_.max.z,
    ]
}

fn same_bits(a: &SpdAABB, b: &SpdAABB) -> bool {
    corners(a)
        .iter()
        .zip(corners(b).iter())
        .all(|(l, r)| l.to_bits() == r.to_bits())
}

/// Is `inner` inside `outer`, allowing [`CONTAINMENT_SLACK`]?
fn contains(outer: &SpdAABB, inner: &SpdAABB) -> bool {
    let extent = corners(outer)
        .iter()
        .fold(0.0f32, |acc, v| acc.max(v.abs()))
        .max(1.0);
    let slack = CONTAINMENT_SLACK * extent;
    inner.min.x >= outer.min.x - slack
        && inner.min.y >= outer.min.y - slack
        && inner.min.z >= outer.min.z - slack
        && inner.max.x <= outer.max.x + slack
        && inner.max.y <= outer.max.y + slack
        && inner.max.z <= outer.max.z + slack
}

fn plausible(box_: &SpdAABB) -> bool {
    corners(box_).iter().all(|v| v.is_finite() && v.abs() < 1e7)
        && box_.min.x <= box_.max.x
        && box_.min.y <= box_.max.y
        && box_.min.z <= box_.max.z
}

#[derive(Default)]
struct Census {
    files: usize,
    files_read: usize,
    refused: usize,
    unreadable: usize,

    drawables: usize,
    models: usize,
    /// Models every geometry of which yielded positions.
    models_measurable: usize,
    geometries: usize,

    geometry: Agreement,
    /// The same comparison against the neighbouring slot. This measurement's
    /// own floor: if a box agrees with its neighbour's as often as with its
    /// own, agreement is saying nothing.
    control: Agreement,
    model_prefix: Agreement,
    drawable_box: Agreement,
    /// The drawable box against the highest LOD alone, to say whether the
    /// shipped box covers every LOD or only the one that renders closest.
    drawable_box_top_lod: Agreement,
    /// ⚠ Split because a bone transform between the vertices and the
    /// drawable was the obvious explanation for the drawable level
    /// disagreeing, and the split refutes it: the skeletal half agrees
    /// *more* often, not less.
    drawable_box_boneless: Agreement,
    drawable_box_skeletal: Agreement,

    /// The shipped sphere against the shipped box, which tests the rule
    /// alone — a box the recompute got wrong would otherwise be read as a
    /// sphere rule that does not hold.
    sphere_rule_centre: usize,
    sphere_rule_radius: usize,
    sphere_vs_recomputed: usize,
    spheres: usize,

    /// The fourth lanes of a shipped `spdAABB`, by bit pattern.
    model_lanes: BTreeMap<u32, usize>,
    /// Of those, how many repeat the first lane of their own vector.
    model_lanes_repeat_x: usize,
    model_lane_pairs: usize,
    /// The lanes of the slot past the end of the engine's own array.
    overread_lanes: BTreeMap<u32, usize>,
    group_lanes: BTreeMap<u32, usize>,

    /// Models with one geometry, and how long their `aabbs` array came back.
    single_geometry: usize,
    /// Of those, how many carried an implausible box in the extra slot the
    /// reader asked for.
    single_geometry_extra_implausible: usize,
    /// Models whose array is longer than the pack's `count_field` can say.
    beyond_the_count_field: usize,

    /// A recompute run on a copy, and what it preserved.
    recomputed: usize,
    recompute_kept_length: usize,
    recompute_kept_lanes: usize,
    recompute_covers_mesh: usize,

    skinned_models: usize,
    skinned_geometry: Agreement,

    per_ext: BTreeMap<String, usize>,
    explained: usize,
    explain: usize,
    notes: Vec<String>,
}

impl Census {
    fn drawable<T: DrawableTarget + Clone>(&mut self, label: &str, drawable: &RmcDrawable<T>) {
        self.drawables += 1;

        let mut union = AabbBuilder::new();
        let mut top_lod = AabbBuilder::new();
        let mut every_model_measurable = true;

        for (level, lod) in levels(drawable) {
            for model in &lod.models {
                self.models += 1;
                let skinned = model.skin_flag_and_tess & 0x01 != 0;
                if skinned {
                    self.skinned_models += 1;
                }
                let count = model.geometries.len();
                if count == 1 {
                    self.single_geometry += 1;
                    if let Some(extra) = model.aabbs.get(1) {
                        if !plausible(extra) {
                            self.single_geometry_extra_implausible += 1;
                        }
                    }
                }
                if aabb_slots(count) > count {
                    self.beyond_the_count_field += 1;
                }
                for (slot, box_) in model.aabbs.iter().enumerate() {
                    if slot >= aabb_slots(count) {
                        *self.overread_lanes.entry(box_.min.w.to_bits()).or_default() += 1;
                        *self.overread_lanes.entry(box_.max.w.to_bits()).or_default() += 1;
                        continue;
                    }
                    *self.model_lanes.entry(box_.min.w.to_bits()).or_default() += 1;
                    *self.model_lanes.entry(box_.max.w.to_bits()).or_default() += 1;
                    self.model_lane_pairs += 2;
                    self.model_lanes_repeat_x +=
                        usize::from(box_.min.w.to_bits() == box_.min.x.to_bits())
                            + usize::from(box_.max.w.to_bits() == box_.max.x.to_bits());
                }

                let mut measurable = true;
                for (index, geometry) in model.geometries.iter().enumerate() {
                    self.geometries += 1;
                    let Some(computed) = geometry_aabb(geometry) else {
                        measurable = false;
                        continue;
                    };
                    union.absorb(&from_box(&computed));
                    if level == 0 {
                        top_lod.absorb(&from_box(&computed));
                    }

                    let Some(slot) = aabb_slot_of(count, index) else {
                        continue;
                    };
                    let Some(shipped) = model.aabbs.get(slot) else {
                        continue;
                    };
                    self.geometry.record(shipped, &computed);
                    if skinned {
                        self.skinned_geometry.record(shipped, &computed);
                    }
                    if relative_error(shipped, &computed) > BIG && self.explained < self.explain {
                        self.explained += 1;
                        self.notes.push(format!(
                            "  {label} model geom {index}/{count}\n    shipped {}\n    vertices {}",
                            show(shipped),
                            show(&computed)
                        ));
                    }

                    // The control: the same box against the slot next door.
                    if let Some(neighbour) =
                        aabb_slot_of(count, (index + 1) % count).and_then(|s| model.aabbs.get(s))
                    {
                        if count > 1 {
                            self.control.record(neighbour, &computed);
                        }
                    }
                }

                if measurable {
                    self.models_measurable += 1;
                    if count > 1 {
                        if let (Some(prefix), Some(computed)) =
                            (model.aabbs.first(), model_aabb(model))
                        {
                            self.model_prefix.record(prefix, &computed);
                        }
                    }
                } else {
                    every_model_measurable = false;
                }
            }
        }

        if !every_model_measurable {
            return;
        }

        let shipped_box = &drawable.lod_group.bounds_box;
        *self
            .group_lanes
            .entry(shipped_box.min.w.to_bits())
            .or_default() += 1;
        *self
            .group_lanes
            .entry(shipped_box.max.w.to_bits())
            .or_default() += 1;

        self.spheres += 1;
        let shipped_sphere = &drawable.lod_group.bounds_sphere.center_and_radius;
        let (centre, radius) = from_box(shipped_box).bounding_sphere();
        let scale = radius.abs().max(1.0);
        let agrees = |a: f32, b: f32| (a - b).abs() <= BINS[2] * scale;
        if agrees(shipped_sphere.x, centre[0])
            && agrees(shipped_sphere.y, centre[1])
            && agrees(shipped_sphere.z, centre[2])
        {
            self.sphere_rule_centre += 1;
        }
        if agrees(shipped_sphere.w, radius) {
            self.sphere_rule_radius += 1;
        }

        if let Some(computed) = union.finish().map(to_box) {
            self.drawable_box.record(shipped_box, &computed);
            if drawable.skeleton.is_some() {
                self.drawable_box_skeletal.record(shipped_box, &computed);
            } else {
                self.drawable_box_boneless.record(shipped_box, &computed);
            }
            if let Some(top) = top_lod.finish().map(to_box) {
                self.drawable_box_top_lod.record(shipped_box, &top);
            }
            if agrees(shipped_sphere.w, from_box(&computed).bounding_sphere().1) {
                self.sphere_vs_recomputed += 1;
            }
            self.recompute(drawable, &computed);
            if relative_error(shipped_box, &computed) > BIG && self.explained < self.explain {
                self.explained += 1;
                self.notes.push(format!(
                    "  {label} drawable box (skeleton {}, {} bone-bound models)\n    shipped {}\n    vertices {}",
                    drawable.skeleton.is_some(),
                    drawable
                        .lod_group
                        .lods
                        .iter()
                        .flat_map(|l| l.models.iter())
                        .filter(|m| m.matrix_index != 0)
                        .count(),
                    show(shipped_box),
                    show(&computed)
                ));
            }
        }
    }
}

impl Census {
    /// Run the recompute on a copy and check what it left alone.
    ///
    /// ⚠ The three properties a recompute has to hold are not the ones a
    /// unit test reaches: that it writes the engine's own number of boxes,
    /// that it leaves the fourth lanes it does not describe, and that the
    /// box it produces covers the mesh it was grown from. Only shipped files
    /// have lanes worth preserving.
    fn recompute<T: DrawableTarget + Clone>(&mut self, drawable: &RmcDrawable<T>, mesh: &SpdAABB) {
        let mut copy = drawable.clone();
        if !recompute_bounds(&mut copy) {
            return;
        }
        self.recomputed += 1;

        let before = drawable.lod_group.lods.iter().flat_map(|l| l.models.iter());
        let after = copy.lod_group.lods.iter().flat_map(|l| l.models.iter());
        let mut lengths = true;
        let mut kept_lanes = true;
        for (was, now) in before.zip(after) {
            lengths &= now.aabbs.len() == aabb_slots(now.geometries.len());
            for (old, new) in was.aabbs.iter().zip(now.aabbs.iter()) {
                kept_lanes &= old.min.w.to_bits() == new.min.w.to_bits()
                    && old.max.w.to_bits() == new.max.w.to_bits();
            }
        }
        self.recompute_kept_length += usize::from(lengths);
        self.recompute_kept_lanes += usize::from(kept_lanes);
        self.recompute_covers_mesh += usize::from(contains(&copy.lod_group.bounds_box, mesh));
    }
}

fn show(box_: &SpdAABB) -> String {
    format!(
        "[{:.4} {:.4} {:.4}] .. [{:.4} {:.4} {:.4}]",
        box_.min.x, box_.min.y, box_.min.z, box_.max.x, box_.max.y, box_.max.z
    )
}

pub fn run(args: &[String]) -> ExitCode {
    let mut game: Option<PathBuf> = None;
    let mut keys: Option<PathBuf> = None;
    let mut only: Vec<String> = Vec::new();
    let mut limit = usize::MAX;
    let mut explain = 0usize;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg, None),
        };
        match key {
            "-h" | "--help" | "help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            "--game" | "--keys" | "--ext" | "--limit" | "--explain" => {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("Error: {key} needs a value");
                                return ExitCode::from(2);
                            }
                        }
                    }
                };
                match key {
                    "--game" => game = Some(PathBuf::from(v)),
                    "--keys" => keys = Some(PathBuf::from(v)),
                    "--ext" => only.push(v.trim_start_matches('.').to_ascii_lowercase()),
                    "--limit" => match v.parse() {
                        Ok(n) => limit = n,
                        Err(_) => {
                            eprintln!("Error: --limit needs a number");
                            return ExitCode::from(2);
                        }
                    },
                    _ => match v.parse() {
                        Ok(n) => explain = n,
                        Err(_) => {
                            eprintln!("Error: --explain needs a number");
                            return ExitCode::from(2);
                        }
                    },
                }
            }
            other => {
                eprintln!("Error: unknown option '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let game_root = match resolve_game_root(game) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("Error: {message}");
            return ExitCode::from(2);
        }
    };

    let wanted: Vec<String> = if only.is_empty() {
        DRAWABLE.iter().map(|e| e.to_string()).collect()
    } else {
        only
    };

    let resolved = resolve_crypto(&game_root, keys.as_deref());
    let mut warnings = resolved.notes;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let _ = super::jobs::install_stop_handler(move || cancel.store(true, Ordering::Relaxed));
    }

    eprintln!(
        "openrage-cli: recomputing bounding volumes in {} (Ctrl-C stops and still reports) ...",
        game_root.display()
    );

    let census = survey(
        &game_root,
        resolved.provider,
        &wanted,
        limit,
        explain,
        &cancel,
        &mut warnings,
    );

    for w in &warnings {
        eprintln!("note: {w}");
    }
    print!("{}", report(&census));
    ExitCode::SUCCESS
}

#[allow(clippy::too_many_arguments)]
fn survey(
    game_root: &Path,
    provider: Arc<dyn RpfCryptoProvider>,
    wanted: &[String],
    limit: usize,
    explain: usize,
    cancel: &AtomicBool,
    warnings: &mut Vec<String>,
) -> Census {
    let mut census = Census {
        explain,
        ..Default::default()
    };
    let _ = crawl::crawl(game_root, provider, warnings, &mut |found| {
        if cancel.load(Ordering::Relaxed) || census.files >= limit {
            return ControlFlow::Break(());
        }
        let Some(ext) = found
            .leaf
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
        else {
            return ControlFlow::Continue(());
        };
        if !wanted.contains(&ext) {
            return ControlFlow::Continue(());
        }
        census.files += 1;
        let Ok(bytes) = found.read() else {
            census.unreadable += 1;
            return ControlFlow::Continue(());
        };
        *census.per_ext.entry(ext.clone()).or_default() += 1;
        if harvest(&mut census, &ext, found.path, &bytes) {
            census.files_read += 1;
        } else {
            census.refused += 1;
        }
        ControlFlow::Continue(())
    });
    census
}

fn harvest(census: &mut Census, ext: &str, logical: &str, data: &[u8]) -> bool {
    let path = PathBuf::from(logical);
    match crate::target_of(&path, data) {
        Generation::Gen9 => harvest_at::<Gen9>(census, ext, logical, data),
        _ => harvest_at::<Gen8>(census, ext, logical, data),
    }
}

fn harvest_at<T: DrawableTarget + Clone>(
    census: &mut Census,
    ext: &str,
    logical: &str,
    data: &[u8],
) -> bool
where
    GtaDrawable<T>: or_io::gta5::File,
    PgDictionary<GtaDrawable<T>>: or_io::gta5::File,
    GtaFragType<T>: or_io::gta5::File,
{
    match ext {
        "ydr" => match GtaDrawable::<T>::from_bytes(data) {
            Ok(d) => census.drawable(logical, &d),
            Err(_) => return false,
        },
        "ydd" => match PgDictionary::<GtaDrawable<T>>::from_bytes(data) {
            Ok(dict) => {
                for (i, entry) in dict.entries.iter().flatten().enumerate() {
                    census.drawable(&format!("{logical}[{i}]"), entry);
                }
            }
            Err(_) => return false,
        },
        "yft" => match GtaFragType::<T>::from_bytes(data) {
            Ok(frag) => {
                for (i, d) in frag.drawables().into_iter().enumerate() {
                    census.drawable(&format!("{logical}[{i}]"), d);
                }
            }
            Err(_) => return false,
        },
        _ => return false,
    }
    true
}

fn lanes(seen: &BTreeMap<u32, usize>) -> String {
    let mut sorted: Vec<_> = seen.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    sorted
        .iter()
        .take(4)
        .map(|(bits, n)| format!("0x{bits:08X} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn report(census: &Census) -> String {
    let mut s = String::new();
    s.push_str("Bounding volumes, recomputed against shipped\n\n");
    s.push_str(&format!(
        "files read      {}\nrefused         {}\nunreadable      {}\n\
         drawables       {}\nmodels          {} ({} fully measurable)\n\
         geometries      {}\n\n",
        census.files_read,
        census.refused,
        census.unreadable,
        census.drawables,
        census.models,
        census.models_measurable,
        census.geometries,
    ));

    if census.geometry.total == 0 {
        s.push_str("No comparable bounding volumes found.\n");
        return s;
    }

    s.push_str(
        "level                     boxes    exact     <1e-6     <1e-5     <1e-4     <1e-3     <1e-2    worse  contains\n",
    );
    s.push_str(&census.geometry.line("geometry box"));
    s.push_str(&census.model_prefix.line("model prefix box"));
    s.push_str(&census.drawable_box.line("drawable box"));
    s.push_str(&census.drawable_box_top_lod.line("  (highest LOD only)"));
    s.push_str(&census.drawable_box_boneless.line("  (no skeleton)"));
    s.push_str(&census.drawable_box_skeletal.line("  (with skeleton)"));
    s.push_str(&census.skinned_geometry.line("geometry, skinned"));
    s.push_str(&census.control.line("CONTROL next slot"));
    s.push_str(&format!(
        "\nColumns after `exact` are cumulative: the share of boxes whose worst corner\n\
         differs from the shipped one by less than that fraction of the box's extent.\n\
         `contains` is the share the shipped box still covers, which is the property a\n\
         renderer needs. Worst relative error seen: geometry {:.3e}, drawable {:.3e}.\n\n\
         The control is the identical comparison against the slot next door. It is\n\
         this measurement's own floor: agreement means nothing unless a box agrees\n\
         with its own shipped slot far more often than with a neighbour's.\n\n",
        census.geometry.worst, census.drawable_box.worst,
    ));

    s.push_str(&format!(
        "shipped sphere over shipped box   {} of {} centres, {} radii\n\
         shipped sphere over recomputed box              {} radii\n",
        census.sphere_rule_centre,
        census.spheres,
        census.sphere_rule_radius,
        census.sphere_vs_recomputed,
    ));

    s.push_str(&format!(
        "\nfourth lanes      model    {}\n                  overread {}\n                  group    {}\n\
         model lanes repeating their own x  {} of {}\n",
        lanes(&census.model_lanes),
        lanes(&census.overread_lanes),
        lanes(&census.group_lanes),
        census.model_lanes_repeat_x,
        census.model_lane_pairs,
    ));

    s.push_str(&format!(
        "\nsingle-geometry models  {}\n  extra slot implausible {} ({:.2}%)\n\
         models shipping more boxes than `_geom_count` says  {} of {}\n",
        census.single_geometry,
        census.single_geometry_extra_implausible,
        100.0 * census.single_geometry_extra_implausible as f64
            / census.single_geometry.max(1) as f64,
        census.beyond_the_count_field,
        census.models,
    ));
    s.push_str(&format!(
        "\nrecompute on a copy   {} drawables\n  \
         array length the engine's  {}\n  fourth lanes left alone    {}\n  \
         box covers its own mesh    {}\n",
        census.recomputed,
        census.recompute_kept_length,
        census.recompute_kept_lanes,
        census.recompute_covers_mesh,
    ));
    s.push_str(&format!("\nskinned models    {}\n", census.skinned_models));

    s.push_str("\nper extension\n");
    for (ext, n) in &census.per_ext {
        s.push_str(&format!("  {ext:<6} {n:>8}\n"));
    }

    if !census.notes.is_empty() {
        s.push_str("\ndisagreements\n");
        for note in &census.notes {
            s.push_str(note);
            s.push('\n');
        }
    }
    s
}

fn print_usage() {
    println!(
        "openrage-cli bounds --game <dir> [--ext <ext>] [--limit <n>] [--explain <n>]\n\
         \x20                   [--keys <file>]\n\n\
         Recompute every bounding volume from the vertices it covers and compare it\n\
         with the one the file shipped.\n\n\
         Defaults to .ydr, .ydd and .yft."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(min: [f32; 3], max: [f32; 3]) -> SpdAABB {
        to_box(Aabb { min, max })
    }

    /// ⚠ An absolute error bins every map-scale box with the worst of them:
    /// at 4,000 units one mantissa step of an `f32` is already wider than a
    /// millimetre, so the error has to be read against the extent.
    #[test]
    fn the_error_is_read_against_the_box_it_judges() {
        let big = boxed([-4000.0, 0.0, 0.0], [4000.0, 1.0, 1.0]);
        let big_nudged = boxed([-4000.1, 0.0, 0.0], [4000.0, 1.0, 1.0]);
        assert!(relative_error(&big, &big_nudged) < 1e-4);

        let small = boxed([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let small_nudged = boxed([0.0, 0.0, 0.0], [1.1, 1.0, 1.0]);
        assert!(relative_error(&small, &small_nudged) > 1e-2);
    }

    /// ⚠ Containment is the property a renderer needs and it is not the
    /// same question as agreement: a box twice the size it needs to be is
    /// wasteful and correct, and one a millimetre too small culls the mesh.
    #[test]
    fn containment_is_asked_separately_from_agreement() {
        let outer = boxed([-2.0, -2.0, -2.0], [2.0, 2.0, 2.0]);
        let inner = boxed([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
        assert!(contains(&outer, &inner));
        assert!(!contains(&inner, &outer));

        let mut seen = Agreement::default();
        seen.record(&outer, &inner);
        assert_eq!(seen.contained, 1);
        assert_eq!(seen.exact, 0);
        assert_eq!(seen.bins[BINS.len()], 1, "half the size is not a rounding");
    }

    /// ⚠ Bit equality, not `==`: `-0.0 == 0.0` is true and they are
    /// different bytes, so a "byte-identical" claim made with `==` is false
    /// exactly where it matters.
    #[test]
    fn exact_means_the_same_bytes() {
        let zero = boxed([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let negative_zero = boxed([-0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert!(!same_bits(&zero, &negative_zero));
        assert_eq!(relative_error(&zero, &negative_zero), 0.0);

        let mut seen = Agreement::default();
        seen.record(&zero, &negative_zero);
        assert_eq!(seen.exact, 0);
        assert_eq!(seen.bins[0], 1);
    }

    #[test]
    fn an_implausible_box_is_recognised() {
        assert!(plausible(&boxed([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])));
        assert!(!plausible(&boxed([1.0, 0.0, 0.0], [-1.0, 1.0, 1.0])));
        assert!(!plausible(&boxed([f32::NAN, 0.0, 0.0], [1.0, 1.0, 1.0])));
        assert!(!plausible(&boxed([0.0, 0.0, 0.0], [1e30, 1.0, 1.0])));
    }

    #[test]
    fn the_report_survives_an_empty_census() {
        assert!(report(&Census::default()).contains("No comparable bounding volumes"));
    }
}
