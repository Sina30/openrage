//! An interchange mesh put back into a drawable.
//!
//! ⚠ **This invents vertices rather than editing them.** Everything here
//! rebuilds the runs on a clone, whose save states the counts they describe
//! and refuses what the game would misread. Nothing is mutated in place, so a
//! refusal leaves the caller's drawable untouched.
//!
//! ⚠ **A channel the target declares and the source does not state is an
//! error, not a default**, because a filled channel is invented data that
//! renders; `fill_missing` asks for it and the report names each one.

use std::collections::{BTreeMap, BTreeSet};

use or_io::codec::mesh::{
    elements_g9, encode, model_aabbs, Aabb, GrcDataSize, GrcFvfChannel, VertexLayout, VertexReader,
    VertexSource,
};
use or_io::decl::{File, Gen8, Gen9, Shared};
use or_io::error::{IoError, Result};
use or_io::gta5::{
    gen8, gen9, DrawableTarget, GrmGeometry, GrmModel, GtaDrawable, GtaFragType, PgDictionary,
    RmcDrawable, RmcLod, SpdAABB,
};

use crate::extract::{effect_name, Extracted};
use crate::gltf::read::{lod_and_model_in_name, strip_copy_suffix};
use crate::scene::{LodSlot, Mesh, Primitive, Scene};

/// What an import writes of a target's buffers; each target is written as itself.
pub trait Imported: Extracted + Clone {
    /// Replace the vertices, leaving no second copy that still holds the old ones.
    fn set_vertices(buffer: &mut Self::VertexBuffer, data: Vec<u8>);
    fn indices_mut(buffer: &mut Self::IndexBuffer) -> &mut Vec<u16>;
    /// How many channels the buffer's declaration states, whether or not a [`VertexLayout`] can name them.
    fn stated_channels(buffer: &Self::VertexBuffer) -> usize;
}

impl Imported for Gen8 {
    fn set_vertices(buffer: &mut gen8::GrcVertexBuffer, data: Vec<u8>) {
        buffer.data = data;
        buffer.lock_ptr = Shared::Same;
    }

    fn indices_mut(buffer: &mut gen8::GrcIndexBuffer) -> &mut Vec<u16> {
        &mut buffer.indices
    }

    /// Every one of an FVF's sixteen channels has a [`VertexLayout`] slot.
    fn stated_channels(buffer: &gen8::GrcVertexBuffer) -> usize {
        buffer.vertex_layout().map_or(0, |l| l.count())
    }
}

impl Imported for Gen9 {
    fn set_vertices(buffer: &mut gen9::GrcVertexBufferG9, data: Vec<u8>) {
        buffer.data = data;
    }

    fn indices_mut(buffer: &mut gen9::GrcIndexBufferG9) -> &mut Vec<u16> {
        &mut buffer.indices
    }

    fn stated_channels(buffer: &gen9::GrcVertexBufferG9) -> usize {
        buffer
            .info
            .as_deref()
            .map_or(0, |d| elements_g9(&d.offsets, &d.sizes, &d.types).len())
    }
}

/// A `matrix_palette` slot is addressed by one byte of the `Binding` channel.
const PALETTE_CEILING: usize = 256;

/// `vertices_count` and the index payload are both 16 bit.
const VERTEX_CEILING: usize = u16::MAX as usize + 1;

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Fill a channel the target geometry declares and the source omits.
    ///
    /// ⚠ Off by default. A normal filled with `+Z` or a colour filled with
    /// white is data the source never stated, and it renders.
    pub fill_missing: bool,

    /// Carry the target's own skin channels across when the mesh states none
    /// and the vertex count is unchanged.
    ///
    /// ⚠ A ped component's `.ydd` entry is skinned against a skeleton in
    /// another file, which glTF cannot state, so the exporter withholds its
    /// influences — 28 of the staged corpus's 416 drawables. Refused if the
    /// vertex count moved, because then vertex *i* is not the old vertex *i*.
    pub keep_skinning: bool,
}

/// What a file calls the parts of the drawable it goes back into, where the caller knows and the defaults of an export are not it.
///
/// ⚠ An export names a mesh `<drawable>_lod0_model1` and a material for its shader, and a
/// modelling tool keeps both. A caller whose people rename them states the names here.
#[derive(Debug, Clone, Default)]
pub struct ImportNames {
    /// The mesh name of a model, by level and model.
    pub models: Vec<(LodSlot, usize, String)>,
    /// The material name of each shader of the group, by its index.
    pub materials: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    pub geometries_replaced: usize,
    /// Geometries a mesh overflowed, which were cut into several.
    pub geometries_split: usize,
    /// `lod0/model0/geometry1: …`, once per geometry that was split, saying why and into how many.
    pub splits: Vec<String>,
    pub vertices_before: usize,
    pub vertices_after: usize,
    pub triangles_after: usize,
    /// `lod0/model0/geometry1: Normal`, once per filled channel.
    pub channels_filled: Vec<String>,
    pub palettes_rebuilt: usize,
    /// Geometries whose influences were carried over from the target rather
    /// than imported, because the mesh could not state them.
    pub skinning_kept: usize,
    pub models_reboxed: usize,
    /// The drawable's own box is authored, so an import leaves it as it was.
    pub drawable_box_left_alone: bool,
}

impl ImportReport {
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            format!(
                "{} replaced: {} in, {} out, {}",
                counted(self.geometries_replaced, "geometry", "geometries"),
                counted(self.vertices_before, "vertex", "vertices"),
                self.vertices_after,
                counted(self.triangles_after, "triangle", "triangles"),
            ),
            format!(
                "{} reboxed from {} own vertices; the drawable box left as authored",
                counted(self.models_reboxed, "model", "models"),
                if self.models_reboxed == 1 {
                    "its"
                } else {
                    "their"
                },
            ),
        ];
        out.extend(self.splits.iter().cloned());
        if self.palettes_rebuilt > 0 {
            out.push(format!(
                "{} rebuilt from the imported influences",
                counted(self.palettes_rebuilt, "bone palette", "bone palettes"),
            ));
        }
        if self.skinning_kept > 0 {
            out.push(format!(
                "{} kept the influences already in the file, because the mesh could not \
                 state them",
                counted(self.skinning_kept, "geometry", "geometries"),
            ));
        }
        if !self.channels_filled.is_empty() {
            let filled = self.channels_filled.len();
            out.push(format!(
                "{} the source did not state {} filled: {}",
                counted(filled, "channel", "channels"),
                if filled == 1 { "was" } else { "were" },
                self.channels_filled.join(", ")
            ));
        }
        out
    }
}

/// `1 geometry`, `2 geometries`.
fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

fn refuse<T>(context: impl Into<String>) -> Result<T> {
    Err(IoError::InvalidData {
        context: context.into(),
    })
}

/// Every geometry of a drawable, in the order the exporter walks them.
fn addresses<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<(LodSlot, usize, usize)> {
    let mut out = Vec::new();
    for (lod, held) in crate::extract::lods_of(drawable, crate::LodSelection::All) {
        for (model, held) in held.models.iter().enumerate() {
            for geometry in 0..held.geometries.len() {
                out.push((lod, model, geometry));
            }
        }
    }
    out
}

/// Names, on each primitive of `scene`, the geometry of `drawable` an import would pair it with.
///
/// ⚠ For a scene read from a file that never said where its meshes came from, such as an FBX:
/// written out as it is, each primitive would state the address of the default, the first
/// geometry, and the cross-check of an import would refuse every file of more than one.
pub fn address<T: DrawableTarget>(drawable: &RmcDrawable<T>, scene: &mut Scene) {
    let mut targets: BTreeMap<LodSlot, Vec<(usize, usize)>> = BTreeMap::new();
    for (lod, model, geometry) in addresses(drawable) {
        targets.entry(lod).or_default().push((model, geometry));
    }
    let mut taken: BTreeMap<LodSlot, usize> = BTreeMap::new();
    for primitive in scene
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        let at = taken.entry(primitive.lod).or_default();
        if let Some(&(model, geometry)) = targets.get(&primitive.lod).and_then(|t| t.get(*at)) {
            primitive.model = model;
            primitive.geometry = geometry;
        }
        *at += 1;
    }
}

fn model_at<T: DrawableTarget>(
    drawable: &mut RmcDrawable<T>,
    lod: LodSlot,
    model: usize,
) -> Option<&mut GrmModel<T>> {
    let holder = match (lod, &mut drawable.container) {
        (LodSlot::Level(level), _) => &mut **drawable.lod_group.lods.get_mut(usize::from(level))?,
        (LodSlot::Container, Shared::Own(Some(own))) => &mut **own,
        (LodSlot::Container, _) => return None,
    };
    holder.models.get_mut(model).map(|m| &mut **m)
}

/// Replace a drawable's geometry from a scene, on a clone.
///
/// Pairs by name where the file names a model of the drawable, and by position within a level
/// where it names nothing; refuses a file whose stated addressing disagrees with where it landed.
pub fn replace_geometry<T: Imported>(
    drawable: &RmcDrawable<T>,
    scene: &Scene,
    options: &ImportOptions,
) -> Result<(RmcDrawable<T>, ImportReport)> {
    replace_geometry_named(drawable, scene, options, &ImportNames::default())
}

/// [`replace_geometry`], pairing by the names the caller states before the ones an export writes.
///
/// A mesh that overflows the 65,535 vertices of a geometry, or the 256 bones of a palette, is cut
/// into geometries that draw with the same shader and declare the same vertex, and the report says so.
pub fn replace_geometry_named<T: Imported>(
    drawable: &RmcDrawable<T>,
    scene: &Scene,
    options: &ImportOptions,
    names: &ImportNames,
) -> Result<(RmcDrawable<T>, ImportReport)> {
    let paired = pair(drawable, scene, names)?;

    let mut built = drawable.clone();
    let mut report = ImportReport {
        drawable_box_left_alone: true,
        ..ImportReport::default()
    };
    let mut touched: Vec<(LodSlot, usize)> = Vec::new();
    // What each paired geometry becomes: itself, or the several a mesh over the limits is cut into.
    let mut becomes = Becomes::<T>::new();

    for Pair {
        primitive,
        at: (lod, model, geometry),
        how,
    } in paired
    {
        let at = format!("{}/model{model}/geometry{geometry}", lod.label());

        // ⚠ A pairing by position keeps its stated cross-check. A file whose
        // meshes were reordered would otherwise put one geometry's vertices
        // under another's material. A name that only one geometry answers to
        // has put them right already.
        if let Some(stated) = primitive.stated_address {
            let agrees = match how {
                Pairing::Position => stated == (model, geometry),
                Pairing::Mesh => stated.1 == geometry,
                Pairing::Material => true,
            };
            if !agrees {
                return refuse(format!(
                    "{at}: the mesh says it came from model{} geometry{} and it arrived \
                     {}th in the walk. Its order changed after it was exported, so nothing \
                     here can say which geometry it belongs to",
                    stated.0, stated.1, report.geometries_replaced
                ));
            }
        }

        let Some(model_ref) = model_at(&mut built, lod, model) else {
            return refuse(format!("{at}: the drawable no longer holds that model"));
        };
        let Some(template) = model_ref.geometries.get(geometry) else {
            return refuse(format!("{at}: the drawable no longer holds that geometry"));
        };

        report.vertices_before += vertex_count(template);
        let pieces = cut_to_fit(&at, primitive, &mut report);
        let mut made = Vec::with_capacity(pieces.len());
        for piece in pieces.iter() {
            let mut target = (**template).clone();
            rebuild(&at, &mut target, piece, options, &mut report)?;
            report.vertices_after += vertex_count(&target);
            report.triangles_after += target.primitive_count() as usize;
            made.push(target);
        }
        report.geometries_replaced += 1;
        becomes
            .entry((lod, model))
            .or_default()
            .insert(geometry, made);

        if !touched.contains(&(lod, model)) {
            touched.push((lod, model));
        }
    }

    // A model's geometries and the shader each draws with share a subscript, so a geometry that
    // became several is laid down with its shader repeated, all in one pass.
    for ((lod, model), mut pieces) in becomes {
        let Some(model_ref) = model_at(&mut built, lod, model) else {
            continue;
        };
        let was = std::mem::take(&mut model_ref.geometries);
        let shaders = std::mem::take(&mut model_ref.shader_indices);
        if shaders.len() != was.len() {
            return refuse(format!(
                "{}/model{model}: the model states {} shader indices for {} geometries",
                lod.label(),
                shaders.len(),
                was.len()
            ));
        }
        for (slot, (geometry, shader)) in was.into_iter().zip(shaders).enumerate() {
            match pieces.remove(&slot) {
                Some(made) => {
                    for piece in made {
                        model_ref.geometries.push(Box::new(piece));
                        model_ref.shader_indices.push(shader);
                    }
                }
                None => {
                    model_ref.geometries.push(geometry);
                    model_ref.shader_indices.push(shader);
                }
            }
        }
    }

    // ⚠ Geometry and model boxes are recomputed and the drawable's is not.
    // `openrage-cli bounds` measured all three: the first two are bit-identical
    // to shipped 95.42% of the time on Legacy and 100% on Enhanced, and the
    // drawable's is authored — 30% of shipped ones do not contain their own
    // mesh. Recomputing that level changes files rather than repairing them.
    for (lod, model) in touched {
        if let Some(model_ref) = model_at(&mut built, lod, model) {
            if !recompute_aabbs(model_ref) {
                return refuse(format!(
                    "{}/model{model}: the boxes could not be recomputed from the imported \
                     vertices, and half-recomputed boxes describe neither mesh",
                    lod.label()
                ));
            }
            report.models_reboxed += 1;
        }
    }

    Ok((built, report))
}

/// The geometries each paired slot becomes, by level and model and then by the slot's geometry.
type Becomes<T> = BTreeMap<(LodSlot, usize), BTreeMap<usize, Vec<GrmGeometry<T>>>>;

/// How a primitive of the file came to be paired with a geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pairing {
    /// By its place in the level, so the address the file states must be where it landed.
    Position,
    /// Its mesh named the model, and it is the same place among that mesh's primitives.
    Mesh,
    /// Its mesh named the model and its material named the one shader that draws it.
    Material,
}

/// A primitive of the file and the geometry of the drawable it replaces.
struct Pair<'a> {
    primitive: &'a Primitive,
    /// Level, model and geometry.
    at: (LodSlot, usize, usize),
    how: Pairing,
}

/// A name without the copy suffix a tool adds, and in one case, to compare names by.
fn key(name: &str) -> String {
    strip_copy_suffix(name).to_ascii_lowercase()
}

fn count_refusal(at: &str, held: usize, brought: usize, what: &str) -> IoError {
    IoError::InvalidData {
        context: format!(
            "{at}: the drawable holds {held} geometries and {what} {brought}. Adding or \
             removing a geometry means choosing its material and resizing the box array, \
             which is authoring rather than import — split or join in the source tool"
        ),
    }
}

/// The name a file gives the material of the geometry that draws with shader `index`.
fn material_name<T: Imported>(
    drawable: &RmcDrawable<T>,
    names: &ImportNames,
    index: Option<&u16>,
) -> Option<String> {
    let index = usize::from(*index?);
    if let Some(stated) = names.materials.get(index) {
        return Some(stated.clone());
    }
    let group = drawable.shader_group.as_deref()?;
    Some(effect_name(T::effect_hash(group.shaders.get(index)?)))
}

/// Pairs each primitive of `scene` with the geometry it replaces.
///
/// ⚠ **By name first, and by position only for what no name settles.** A modelling tool keeps the
/// names of nodes, meshes and materials and drops everything else, and it orders nodes by name
/// (`model10` sorts before `model2`), so a file's order says nothing while what it calls its
/// meshes does. A mesh answers to the model its name ends in (`…_lod1_model0`) or the one the
/// caller names; a primitive answers to the geometry that draws with a shader of its material's
/// name. Where two geometries share a name, which is which is their order, and the file's stated
/// address is held to it.
fn pair<'a, T: Imported>(
    drawable: &RmcDrawable<T>,
    scene: &'a Scene,
    names: &ImportNames,
) -> Result<Vec<Pair<'a>>> {
    // ⚠ **Paired per LOD, against only the LODs the mesh brings.** An export
    // of the highest LOD is the ordinary case — `--lods all` is opt-in — and
    // pairing its primitives against every LOD's geometries counts three
    // targets for one source and refuses a file that is perfectly good. A
    // LOD the mesh says nothing about is left exactly as it was.
    let mut meshes: BTreeMap<LodSlot, Vec<&'a Mesh>> = BTreeMap::new();
    for mesh in scene
        .objects
        .iter()
        .flat_map(|o| o.meshes.iter())
        .filter(|m| !m.primitives.is_empty())
    {
        meshes.entry(mesh.lod).or_default().push(mesh);
    }
    let levels: BTreeMap<LodSlot, &RmcLod<T>> =
        crate::extract::lods_of(drawable, crate::LodSelection::All)
            .into_iter()
            .collect();

    let mut out = Vec::new();
    for (lod, brought) in meshes {
        let models: &[Box<GrmModel<T>>] = levels
            .get(&lod)
            .map_or(&[][..], |level| level.models.as_slice());
        // ⚠ An export writes no mesh for a model with no geometry, so none is owed for it, none
        // can name it and none is left over for it: it starts taken.
        let mut taken: Vec<bool> = models.iter().map(|m| m.geometries.is_empty()).collect();
        let mut owner: Vec<Option<usize>> = Vec::with_capacity(brought.len());
        for mesh in &brought {
            let named: Vec<usize> = names
                .models
                .iter()
                .filter(|(l, m, n)| *l == lod && *m < models.len() && key(n) == key(&mesh.name))
                .map(|(_, m, _)| *m)
                .chain(
                    lod_and_model_in_name(&mesh.name)
                        .filter(|(l, m)| *l == lod && *m < models.len())
                        .map(|(_, m)| m),
                )
                .filter(|m| !models[*m].geometries.is_empty())
                .collect();
            // ⚠ A tool that separates one object by material keeps its name on every piece
            // (`…_model0`, `…_model0.001`), so a name whose models are all taken is the last of
            // them again, and its mesh joins that model's geometries below.
            let answers = named
                .iter()
                .copied()
                .find(|m| !taken[*m])
                .or(named.last().copied());
            if let Some(model) = answers {
                taken[model] = true;
            }
            owner.push(answers);
        }

        if owner.iter().all(Option::is_none) {
            let wanted: usize = brought.iter().map(|m| m.primitives.len()).sum();
            let held: usize = models.iter().map(|m| m.geometries.len()).sum();
            if held != wanted {
                return Err(count_refusal(&lod.label(), held, wanted, "the mesh brings"));
            }
            let places = models
                .iter()
                .enumerate()
                .flat_map(|(model, held)| (0..held.geometries.len()).map(move |g| (model, g)));
            for (primitive, (model, geometry)) in
                brought.iter().flat_map(|m| m.primitives.iter()).zip(places)
            {
                out.push(Pair {
                    primitive,
                    at: (lod, model, geometry),
                    how: Pairing::Position,
                });
            }
            continue;
        }

        // What no name settled takes the models nothing named, in the file's order and the model's.
        let mut free = (0..models.len()).filter(|m| !taken[*m]);
        let unsettled = owner.iter().filter(|o| o.is_none()).count();
        if unsettled != models.len() - taken.iter().filter(|t| **t).count() {
            return refuse(format!(
                "{}: the drawable holds {} models and the file brings {} meshes. Adding or \
                 removing a model is authoring rather than import — rename the meshes for \
                 their models or split and join them in the source tool",
                lod.label(),
                models.len(),
                brought.len()
            ));
        }
        for slot in owner.iter_mut().filter(|o| o.is_none()) {
            *slot = free.next();
        }

        // The meshes that answer to one model are its geometries, in the file's order.
        let mut groups: Vec<(usize, Vec<&'a Mesh>)> = Vec::new();
        for (mesh, model) in brought
            .into_iter()
            .zip(owner.into_iter().map(|o| o.unwrap_or(0)))
        {
            match groups.iter_mut().find(|(m, _)| *m == model) {
                Some((_, joined)) => joined.push(mesh),
                None => groups.push((model, vec![mesh])),
            }
        }
        for (model, joined) in groups {
            within(
                drawable,
                names,
                lod,
                model,
                &models[model],
                &joined,
                &mut out,
            )?;
        }
    }
    Ok(out)
}

/// Pairs the primitives of the meshes that name a model with that model's geometries.
fn within<'a, T: Imported>(
    drawable: &RmcDrawable<T>,
    names: &ImportNames,
    lod: LodSlot,
    index: usize,
    model: &GrmModel<T>,
    meshes: &[&'a Mesh],
    out: &mut Vec<Pair<'a>>,
) -> Result<()> {
    let primitives: Vec<&'a Primitive> = meshes.iter().flat_map(|m| m.primitives.iter()).collect();
    let held = model.geometries.len();
    if held != primitives.len() {
        let called: Vec<String> = meshes.iter().map(|m| format!("`{}`", m.name)).collect();
        let what = match called.as_slice() {
            [one] => format!("the mesh {one} brings"),
            many => format!("the meshes {} bring", many.join(", ")),
        };
        return Err(count_refusal(
            &format!("{}/model{index}", lod.label()),
            held,
            primitives.len(),
            &what,
        ));
    }
    let drawn: Vec<Option<String>> = (0..held)
        .map(|g| material_name(drawable, names, model.shader_indices.get(g)).map(|n| key(&n)))
        .collect();
    let brought: Vec<Option<String>> = primitives
        .iter()
        .map(|p| p.material_name.as_deref().map(key))
        .collect();

    let sorted = |list: &[Option<String>]| -> Option<Vec<String>> {
        let mut all: Vec<String> = list.iter().cloned().collect::<Option<Vec<_>>>()?;
        all.sort();
        Some(all)
    };
    let by_material = matches!((sorted(&drawn), sorted(&brought)), (Some(a), Some(b)) if a == b);

    let mut used = vec![false; held];
    for geometry in 0..held {
        let primitive = if by_material {
            let found = (0..held).find(|i| !used[*i] && brought[*i] == drawn[geometry]);
            found.unwrap_or(geometry)
        } else {
            geometry
        };
        used[primitive] = true;
        // A name only one geometry answers to has said which it is; two with one name are told apart by their order.
        let alone = by_material && drawn.iter().filter(|n| **n == drawn[geometry]).count() == 1;
        out.push(Pair {
            primitive: primitives[primitive],
            at: (lod, index, geometry),
            how: if alone {
                Pairing::Material
            } else {
                Pairing::Mesh
            },
        });
    }
    Ok(())
}

/// Vertices a geometry holds: its count is a `u16` and the index payload is 16 bit beside it.
const PIECE_VERTICES: usize = VERTEX_CEILING - 1;

/// The bones the weighted lanes of a primitive name.
fn bones_used(primitive: &Primitive) -> BTreeSet<u16> {
    primitive
        .joints
        .iter()
        .zip(&primitive.weights)
        .flat_map(|(joints, weights)| {
            (0..4)
                .filter(|lane| weights[*lane] > 0.0)
                .map(|lane| joints[lane])
        })
        .collect()
}

/// `source`, or the pieces it is cut into when it holds more vertices or bones than a geometry can.
///
/// ⚠ Cut by triangle, in the file's order: a piece closes at the first triangle that would take it
/// past the limit, and a vertex or bone several pieces use is carried by each. A vertex no
/// triangle names is not carried at all.
fn cut_to_fit<'a>(
    at: &str,
    source: &'a Primitive,
    report: &mut ImportReport,
) -> Vec<std::borrow::Cow<'a, Primitive>> {
    use std::borrow::Cow;

    let vertices = source.positions.len();
    let bones = bones_used(source).len();
    if vertices <= PIECE_VERTICES && bones <= PALETTE_CEILING {
        return vec![Cow::Borrowed(source)];
    }
    let pieces = cut(source);
    // A mesh with no triangle has nothing to cut, and the rebuild says what is wrong with it.
    if pieces.is_empty() {
        return vec![Cow::Borrowed(source)];
    }
    let why = match (vertices > PIECE_VERTICES, bones > PALETTE_CEILING) {
        (true, true) => format!(
            "{vertices} vertices past the {PIECE_VERTICES} a geometry holds, and {bones} bones \
             past the {PALETTE_CEILING} a palette reaches"
        ),
        (true, false) => {
            format!("{vertices} vertices past the {PIECE_VERTICES} a geometry holds")
        }
        _ => format!("{bones} bones past the {PALETTE_CEILING} a palette reaches"),
    };
    report.geometries_split += 1;
    report.splits.push(format!(
        "{at}: {why}, so it is split into {}",
        counted(pieces.len(), "geometry", "geometries")
    ));
    pieces.into_iter().map(Cow::Owned).collect()
}

/// One piece of a primitive being cut, as the old numbers of its vertices.
#[derive(Default)]
struct Piece {
    /// New vertex of an old one.
    remap: std::collections::HashMap<u32, u32>,
    /// The old vertices, in the order they are laid down.
    order: Vec<u32>,
    indices: Vec<u32>,
    bones: BTreeSet<u16>,
}

fn cut(source: &Primitive) -> Vec<Primitive> {
    let skinned = !source.joints.is_empty()
        && source.joints.len() == source.positions.len()
        && source.weights.len() == source.positions.len();
    let bones_of = |vertex: u32| -> Vec<u16> {
        if !skinned {
            return Vec::new();
        }
        let (joints, weights) = (
            source.joints[vertex as usize],
            source.weights[vertex as usize],
        );
        (0..4)
            .filter(|lane| weights[*lane] > 0.0)
            .map(|lane| joints[lane])
            .collect()
    };

    let mut pieces: Vec<Piece> = Vec::new();
    let mut open = Piece::default();
    for triangle in source.indices.as_chunks::<3>().0 {
        let fits = |piece: &Piece| {
            let mut fresh: Vec<u32> = triangle
                .iter()
                .copied()
                .filter(|v| !piece.remap.contains_key(v))
                .collect();
            fresh.sort_unstable();
            fresh.dedup();
            let new_bones: BTreeSet<u16> = fresh
                .iter()
                .flat_map(|v| bones_of(*v))
                .filter(|b| !piece.bones.contains(b))
                .collect();
            piece.order.len() + fresh.len() <= PIECE_VERTICES
                && piece.bones.len() + new_bones.len() <= PALETTE_CEILING
        };
        if !fits(&open) && !open.order.is_empty() {
            pieces.push(std::mem::take(&mut open));
        }
        for &vertex in triangle {
            let next = open.order.len() as u32;
            let new = *open.remap.entry(vertex).or_insert(next);
            if new == next {
                open.order.push(vertex);
                open.bones.extend(bones_of(vertex));
            }
            open.indices.push(new);
        }
    }
    if !open.order.is_empty() {
        pieces.push(open);
    }

    pieces
        .into_iter()
        .map(|piece| {
            let all = source.positions.len();
            Primitive {
                material: source.material,
                material_name: source.material_name.clone(),
                positions: carried(&source.positions, &piece.order, all),
                normals: carried(&source.normals, &piece.order, all),
                tangents: carried(&source.tangents, &piece.order, all),
                uvs: (source.uvs.iter())
                    .map(|uv| carried(uv, &piece.order, all))
                    .collect(),
                uv_channels: source.uv_channels.clone(),
                colours: (source.colours.iter())
                    .map(|colour| carried(colour, &piece.order, all))
                    .collect(),
                colour_channels: source.colour_channels.clone(),
                joints: carried(&source.joints, &piece.order, all),
                weights: carried(&source.weights, &piece.order, all),
                indices: piece.indices,
                lod: source.lod,
                model: source.model,
                geometry: source.geometry,
                stated_address: source.stated_address,
            }
        })
        .collect()
}

/// The vertices of `channel` that `order` names, or none where the channel does not state every vertex.
fn carried<E: Copy>(channel: &[E], order: &[u32], vertices: usize) -> Vec<E> {
    if channel.len() != vertices {
        return Vec::new();
    }
    order.iter().map(|v| channel[*v as usize]).collect()
}

/// How many vertices a geometry's buffer holds, by its layout's stride where it states none.
fn vertex_count<T: DrawableTarget>(geometry: &GrmGeometry<T>) -> usize {
    VertexReader::new(&*geometry.vertex_buffer).map_or(0, |r| r.count())
}

/// Rewrite a model's boxes from its vertices, keeping each shipped box's fourth lanes.
///
/// ⚠ Refuses rather than half-writes: a model whose geometries cannot all
/// be read keeps the boxes it shipped. The `w` pair is not padding: a
/// model's carries engine scratch no vertex reproduces.
pub(crate) fn recompute_aabbs<T: DrawableTarget>(model: &mut GrmModel<T>) -> bool {
    let Some(per_geometry) = model
        .geometries
        .iter()
        .map(|g| VertexReader::new(&*g.vertex_buffer)?.aabb())
        .collect::<Option<Vec<Aabb>>>()
    else {
        return false;
    };
    let shipped = std::mem::take(&mut model.aabbs);
    model.aabbs = model_aabbs(per_geometry)
        .into_iter()
        .enumerate()
        .map(|(slot, computed)| {
            let mut out = SpdAABB::default();
            let lanes = shipped.get(slot);
            out.min.x = computed.min[0];
            out.min.y = computed.min[1];
            out.min.z = computed.min[2];
            out.min.w = lanes.map_or(0.0, |s| s.min.w);
            out.max.x = computed.max[0];
            out.max.y = computed.max[1];
            out.max.z = computed.max[2];
            out.max.w = lanes.map_or(0.0, |s| s.max.w);
            out
        })
        .collect();
    true
}

/// Rewrite one geometry's buffers from one primitive.
fn rebuild<T: Imported>(
    at: &str,
    target: &mut GrmGeometry<T>,
    source: &Primitive,
    options: &ImportOptions,
    report: &mut ImportReport,
) -> Result<()> {
    let count = source.positions.len();
    if count == 0 {
        return refuse(format!("{at}: the imported mesh has no vertices"));
    }
    if count >= VERTEX_CEILING {
        return refuse(format!(
            "{at}: {count} vertices, and `vertices_count` is a u16 with the index payload \
             16 bit beside it. Split the mesh at {} or below in the source tool",
            VERTEX_CEILING - 1
        ));
    }
    if !source.indices.len().is_multiple_of(3) {
        return refuse(format!(
            "{at}: {} indices, which is not a whole number of triangles",
            source.indices.len()
        ));
    }

    // ⚠ The target's channel set, and so its stride, is kept exactly. A
    // material's vertex declaration names the channels it reads, so a buffer
    // that gains or loses one renders wrongly however well formed it is.
    let Some(layout) = target.vertex_buffer.vertex_layout() else {
        return refuse(format!(
            "{at}: the geometry's buffer states no channel layout, so nothing here knows \
             what to encode"
        ));
    };
    let stride = match target.vertex_buffer.vertex_stride() {
        0 => layout.packed_stride(),
        stated => stated,
    };
    if stride == 0 {
        return refuse(format!("{at}: the geometry's buffer states a zero stride"));
    }
    // ⚠ A Gen9 element with no Gen8 channel or encoding is absent
    // from the layout, and rebuilding would zero it.
    let stated = T::stated_channels(&target.vertex_buffer);
    if stated != layout.count() {
        return refuse(format!(
            "{at}: the buffer declares {stated} channels and {} of them have an encoding this \
             import writes, so the rest would be lost",
            layout.count()
        ));
    }
    // ⚠ A channel stored as its own run sits at an offset that depends on
    // the vertex count; only an interleaved vertex can be rebuilt in place.
    if layout
        .present()
        .any(|(_, s)| s.stride != 0 && s.stride != stride)
    {
        return refuse(format!(
            "{at}: a channel is stored apart from the interleaved vertex, which this import \
             does not lay out"
        ));
    }

    let palette = rebuild_palette(at, target, source, report)?;

    // ⚠ Read before the buffer is replaced. The influences already in the
    // file are the only ones a mesh that cannot state them can have.
    let carried = if source.joints.is_empty() && options.keep_skinning {
        carried_skinning(at, target, &layout, stride, count, report)?
    } else {
        None
    };

    let mut data = vec![0u8; count * stride];
    for (channel, slot) in layout.present() {
        let base_of = |vertex: usize| vertex * stride + slot.offset;
        // ⚠ **`Weight` and `Binding` are lane payloads, not values, and the
        // encoding is how they are spelled rather than what they mean.** A
        // `Binding` declared `Color` put through the normalised encoder
        // clamps a bone slot of 5 to 1 and writes 255; a `Weight` declared
        // `UByte4` put through it writes 1 for a full weight.
        // `des_bridge_root.ydr` declares both skin channels `Color`.
        if matches!(channel, GrcFvfChannel::Weight | GrcFvfChannel::Binding) {
            let lanes = match carried.as_ref() {
                Some((weights, bindings)) => {
                    if channel == GrcFvfChannel::Weight {
                        weights.clone()
                    } else {
                        bindings.clone()
                    }
                }
                None => skin_lanes(at, channel, source, options, report, &palette, count)?,
            };
            for (vertex, value) in lanes.iter().enumerate() {
                let start = base_of(vertex);
                write_lanes(
                    slot.size,
                    *value,
                    &mut data[start..start + slot.size.byte_size()],
                );
            }
            continue;
        }
        let values = channel_values(at, channel, source, options, report, count)?;
        for (vertex, value) in values.iter().enumerate() {
            let start = base_of(vertex);
            encode(
                slot.size,
                *value,
                &mut data[start..start + slot.size.byte_size()],
            );
        }
    }

    // The counts are the runs' lengths, which a save states; the two
    // copies of the vertices are one allocation again.
    T::set_vertices(&mut target.vertex_buffer, data);
    target.vertex_data = Shared::Same;
    *T::indices_mut(&mut target.index_buffer) = source.indices.iter().map(|i| *i as u16).collect();
    target.take_counts_from_runs();
    target.matrix_palette = palette.bones;

    Ok(())
}

/// The bone palette the imported influences need, and the slot each bone takes.
struct Palette {
    bones: Vec<u16>,
    slot_of: BTreeMap<u16, u8>,
}

impl std::ops::Deref for Palette {
    type Target = Vec<u16>;
    fn deref(&self) -> &Vec<u16> {
        &self.bones
    }
}

fn rebuild_palette<T: DrawableTarget>(
    at: &str,
    target: &GrmGeometry<T>,
    source: &Primitive,
    report: &mut ImportReport,
) -> Result<Palette> {
    if source.joints.is_empty() {
        return Ok(Palette {
            bones: target.matrix_palette.clone(),
            slot_of: BTreeMap::new(),
        });
    }

    // ⚠ Only the bones a weighted lane actually names. A palette built from
    // every lane would carry bone 0 for every unweighted slot and burn
    // entries against a ceiling of 256.
    let mut used: Vec<u16> = Vec::new();
    for (joints, weights) in source.joints.iter().zip(source.weights.iter()) {
        for lane in 0..4 {
            if weights[lane] > 0.0 && !used.contains(&joints[lane]) {
                used.push(joints[lane]);
            }
        }
    }
    used.sort_unstable();

    if used.len() > PALETTE_CEILING {
        return refuse(format!(
            "{at}: the imported mesh is weighted to {} bones and a `Binding` lane addresses \
             the palette with one byte, so a geometry reaches {PALETTE_CEILING}. Split the \
             mesh by body part in the source tool",
            used.len()
        ));
    }

    report.palettes_rebuilt += 1;
    let slot_of = used
        .iter()
        .enumerate()
        .map(|(slot, bone)| (*bone, slot as u8))
        .collect();
    Ok(Palette {
        bones: used,
        slot_of,
    })
}

/// The influences already in the target, read off its own buffer.
///
/// `None` when the geometry declares no skin channels, so there is nothing
/// to carry and the ordinary path applies.
#[allow(clippy::type_complexity)]
fn carried_skinning<T: DrawableTarget>(
    at: &str,
    target: &GrmGeometry<T>,
    layout: &VertexLayout,
    _stride: usize,
    count: usize,
    report: &mut ImportReport,
) -> Result<Option<(Vec<[u8; 4]>, Vec<[u8; 4]>)>> {
    if !layout.has(GrcFvfChannel::Weight) || !layout.has(GrcFvfChannel::Binding) {
        return Ok(None);
    }
    let Some(reader) = VertexReader::new(&*target.vertex_buffer) else {
        return Ok(None);
    };
    // ⚠ Refused rather than padded or truncated. With a different count,
    // vertex *i* is not the old vertex *i*, and pairing them anyway weights
    // a new mesh by an old one's bones.
    if reader.count() != count {
        return refuse(format!(
            "{at}: the mesh brings {count} vertices where the geometry had {}, and its \
             influences cannot come back through glTF — this drawable is skinned against a \
             skeleton it does not carry. Keeping the old weights needs the same vertices; \
             edit the mesh without changing its vertex count, or re-author the skin",
            reader.count()
        ));
    }

    let weights = (0..count)
        .map(|v| crate::extract::channel_lanes(&reader, GrcFvfChannel::Weight, v))
        .collect();
    let bindings = (0..count)
        .map(|v| crate::extract::channel_lanes(&reader, GrcFvfChannel::Binding, v))
        .collect();
    report.skinning_kept += 1;
    Ok(Some((weights, bindings)))
}

/// Four lane bytes into whichever width the channel is spelled at.
///
/// ⚠ The inverse of `or_interchange::extract::channel_lanes`, and it must
/// stay so: the two read and write the same payload and anything else makes
/// a round trip renumber every bone.
fn write_lanes(size: GrcDataSize, lanes: [u8; 4], out: &mut [u8]) {
    match size {
        GrcDataSize::UByte4 | GrcDataSize::Color => {
            for (slot, value) in out.iter_mut().zip(lanes) {
                *slot = value;
            }
        }
        GrcDataSize::Short4 => {
            for (pair, value) in out.as_chunks_mut::<2>().0.iter_mut().zip(lanes) {
                pair.copy_from_slice(&(u16::from(value) << 8).to_le_bytes());
            }
        }
        // ⚠ Any other width cannot hold four lanes, and zeroing is the only
        // thing that does not invent a bone. `validate_drawable_geometry`
        // catches a palette a zeroed binding would then disagree with.
        _ => out.fill(0),
    }
}

/// The two skin channels, as the lane bytes they are.
fn skin_lanes(
    at: &str,
    channel: GrcFvfChannel,
    source: &Primitive,
    options: &ImportOptions,
    report: &mut ImportReport,
    palette: &Palette,
    count: usize,
) -> Result<Vec<[u8; 4]>> {
    let missing = |report: &mut ImportReport, value: [u8; 4]| -> Result<Vec<[u8; 4]>> {
        if !options.fill_missing {
            return refuse(format!(
                "{at}: the geometry declares a {} channel and the imported mesh does not \
                 state one. Export it from the source tool, or accept a stated default",
                channel.name()
            ));
        }
        report
            .channels_filled
            .push(format!("{at}: {}", channel.name()));
        Ok(vec![value; count])
    };

    if source.joints.is_empty() || source.weights.is_empty() {
        // A whole weight on the first palette slot is the rigid case.
        return missing(
            report,
            if channel == GrcFvfChannel::Weight {
                [255, 0, 0, 0]
            } else {
                [0; 4]
            },
        );
    }

    let mut out = Vec::with_capacity(count);
    for (joints, weights) in source.joints.iter().zip(source.weights.iter()) {
        let quantised = whole_weights(*weights);
        if channel == GrcFvfChannel::Weight {
            out.push(quantised);
            continue;
        }
        let mut lanes = [0u8; 4];
        for lane in 0..4 {
            if quantised[lane] == 0 {
                continue;
            }
            // ⚠ An unmapped bone refuses rather than taking slot 0, which
            // would weight the vertex to whatever bone happens to be first.
            let Some(slot) = palette.slot_of.get(&joints[lane]) else {
                return refuse(format!(
                    "{at}: a vertex is weighted to bone {} and the palette does not hold it",
                    joints[lane]
                ));
            };
            lanes[lane] = *slot;
        }
        out.push(lanes);
    }
    Ok(out)
}

/// One channel's value per vertex, in the encoding the target declares.
fn channel_values(
    at: &str,
    channel: GrcFvfChannel,
    source: &Primitive,
    options: &ImportOptions,
    report: &mut ImportReport,
    count: usize,
) -> Result<Vec<[f32; 4]>> {
    use GrcFvfChannel as C;

    let filled = |report: &mut ImportReport, value: [f32; 4]| -> Result<Vec<[f32; 4]>> {
        if !options.fill_missing {
            return refuse(format!(
                "{at}: the geometry declares a {} channel and the imported mesh does not \
                 state one. Export it from the source tool, or accept a stated default",
                channel.name()
            ));
        }
        report
            .channels_filled
            .push(format!("{at}: {}", channel.name()));
        Ok(vec![value; count])
    };

    match channel {
        C::Position => Ok(source
            .positions
            .iter()
            .map(|p| [p[0], p[1], p[2], 1.0])
            .collect()),
        C::Normal => {
            if source.normals.is_empty() {
                return filled(report, [0.0, 0.0, 1.0, 0.0]);
            }
            Ok(source
                .normals
                .iter()
                .map(|n| [n[0], n[1], n[2], 0.0])
                .collect())
        }
        C::Tangent0 | C::Tangent1 => {
            if source.tangents.is_empty() {
                return filled(report, [1.0, 0.0, 0.0, 1.0]);
            }
            Ok(source.tangents.clone())
        }
        // ⚠ The two skin channels never come through here: they are lane
        // payloads and `skin_lanes` writes them as bytes.
        C::Weight | C::Binding => refuse(format!(
            "{at}: the {} channel is a lane payload and was routed through the value \
             encoder, which would renumber every bone",
            channel.name()
        )),
        C::Diffuse | C::Specular => {
            let ordinal = source
                .colour_channels
                .iter()
                .position(|c| *c == channel as u8);
            match ordinal.and_then(|o| source.colours.get(o)) {
                Some(values) => Ok(values.clone()),
                None => filled(report, [1.0, 1.0, 1.0, 1.0]),
            }
        }
        C::Tex0 | C::Tex1 | C::Tex2 | C::Tex3 | C::Tex4 | C::Tex5 | C::Tex6 | C::Tex7 => {
            let ordinal = source.uv_channels.iter().position(|c| *c == channel as u8);
            match ordinal.and_then(|o| source.uvs.get(o)) {
                Some(values) => Ok(values.iter().map(|uv| [uv[0], uv[1], 0.0, 0.0]).collect()),
                None => filled(report, [0.0; 4]),
            }
        }
    }
}

/// Four weights as bytes that sum to exactly 255.
///
/// ⚠ **Rounding each lane on its own does not sum to 255**, and the engine
/// blends by the stored bytes: a vertex whose weights sum to 254 is scaled
/// down by every bone at once. Largest-remainder puts the shortfall on the
/// lane with the largest discarded fraction.
fn whole_weights(weights: [f32; 4]) -> [u8; 4] {
    let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    if total <= 0.0 {
        return [255, 0, 0, 0];
    }
    let exact = weights.map(|w| w.max(0.0) / total * 255.0);
    let mut whole = exact.map(|v| v.floor() as i32);
    let mut short = 255 - whole.iter().sum::<i32>();
    // Lanes by descending discarded fraction, so the shortfall lands where
    // it was closest to rounding up anyway.
    let mut order: Vec<usize> = (0..4).collect();
    order.sort_by(|a, b| {
        (exact[*b] - exact[*b].floor())
            .partial_cmp(&(exact[*a] - exact[*a].floor()))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for lane in order {
        if short <= 0 {
            break;
        }
        // A lane with no weight must keep none, or a bone nothing named
        // gains one.
        if exact[lane] <= 0.0 {
            continue;
        }
        whole[lane] += 1;
        short -= 1;
    }
    whole.map(|v| v.clamp(0, 255) as u8)
}

/// Which drawable of a file the mesh goes back into.
///
/// ⚠ A `.ydd` holds many drawables and a glTF states no partition of its
/// meshes between them, so the caller names one rather than the import
/// guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Ydr,
    /// By index into the dictionary's entries, in file order.
    Ydd(usize),
    /// 0 is the common drawable; 1 and up are the extra drawables in order.
    Yft(usize),
}

/// A drawable file's bytes with one drawable's geometry replaced.
///
/// ⚠ The bytes are the edited file's save, which refuses a document the game
/// would misread, so an import cannot emit one. On any refusal the caller's
/// bytes are untouched.
pub fn replace_in_file<T: Imported>(
    original: &[u8],
    destination: Destination,
    scene: &Scene,
    options: &ImportOptions,
) -> Result<(Vec<u8>, ImportReport)>
where
    GtaDrawable<T>: File,
    PgDictionary<GtaDrawable<T>>: File,
    GtaFragType<T>: File,
{
    match destination {
        Destination::Ydr => {
            let mut document = GtaDrawable::<T>::from_bytes(original)?;
            let (base, report) = replace_geometry(&document.base, scene, options)?;
            document.base = base;
            Ok((document.to_bytes()?, report))
        }
        Destination::Ydd(entry) => {
            let mut document = PgDictionary::<GtaDrawable<T>>::from_bytes(original)?;
            let held = document.entries.len();
            let Some(held) = document
                .entries
                .get_mut(entry)
                .and_then(|e| e.as_deref_mut())
            else {
                return refuse(format!(
                    "the dictionary holds {held} entries and entry {entry} was asked for"
                ));
            };
            let (base, report) = replace_geometry(&held.base, scene, options)?;
            held.base = base;
            Ok((document.to_bytes()?, report))
        }
        Destination::Yft(index) => {
            let mut document = GtaFragType::<T>::from_bytes(original)?;
            let held = match index {
                0 => document.common_drawable.as_deref_mut(),
                other => document
                    .extra_drawables
                    .get_mut(other - 1)
                    .map(|d| &mut **d),
            };
            let Some(held) = held else {
                return refuse(format!(
                    "the fragment holds no drawable at {index}; 0 is the common drawable"
                ));
            };
            let (base, report) = replace_geometry(&held.base, scene, options)?;
            held.base = base;
            Ok((document.to_bytes()?, report))
        }
    }
}

/// What a drawable's channel set needs of a source, before anything is built.
pub fn describe_channels<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Vec<String> {
    let mut out = Vec::new();
    for (lod, held) in crate::extract::lods_of(drawable, crate::LodSelection::All) {
        for (model, held) in held.models.iter().enumerate() {
            for (geometry, target) in held.geometries.iter().enumerate() {
                let channels = VertexReader::new(&*target.vertex_buffer)
                    .map(|reader| {
                        reader
                            .layout()
                            .present()
                            .map(|(c, s)| format!("{}:{}", c.name(), label(s.size)))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_else(|| "none".to_string());
                out.push(format!(
                    "{}/model{model}/geometry{geometry}: {channels}",
                    lod.label()
                ));
            }
        }
    }
    out
}

fn label(size: GrcDataSize) -> String {
    format!("{size:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_counts_one_of_anything_in_the_singular() {
        let report = ImportReport {
            geometries_replaced: 1,
            vertices_before: 1,
            vertices_after: 1,
            triangles_after: 1,
            channels_filled: vec!["lod0/model0/geometry0: Normal".into()],
            palettes_rebuilt: 1,
            skinning_kept: 1,
            models_reboxed: 1,
            ..Default::default()
        };
        let said = report.lines().join("\n");
        for wrong in [
            "1 geometries",
            "1 vertices",
            "1 triangles",
            "1 models",
            "1 bone palettes",
            "1 channels",
        ] {
            assert!(!said.contains(wrong), "`{wrong}` in:\n{said}");
        }
    }
}
