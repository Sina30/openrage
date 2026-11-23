//! A binary FBX file of version 7.1 or later read into a [`Scene`].
//!
//! ⚠ **Anything this cannot map is refused or counted in [`Scene::dropped`], never approximated**:
//! an importer is where a wrong guess becomes bytes the game reads. An ASCII file, an older
//! version and a document with no mesh are errors naming what was found, and a layer mapped by
//! edge is left out and counted.
//!
//! The scene's space is the drawable's: Z up, metres. A file's own axes and unit are read from
//! its `GlobalSettings` and carried into that space once, in one matrix, as the glTF reader does
//! with its root node; a file that is left-handed is mirrored by it, and its winding with it.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use fbxcel::low::v7400::AttributeValue;
use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;
use glam::{DMat3, DMat4, DVec3};

use crate::gltf::read::ReadError;
use crate::scene::{Bone, Dropped, LodSlot, Mesh, Object, Primitive, Scene};

type Read<T> = std::result::Result<T, ReadError>;

fn fail<T>(message: impl Into<String>) -> Read<T> {
    Err(ReadError(message.into()))
}

/// Binary FBX opens with this and then the version as a little-endian word.
const MAGIC: &[u8] = b"Kaydara FBX Binary  \0\x1a\0";
/// The oldest version read: 7.1, the first with the layout every later one shares.
const OLDEST: u32 = 7100;
/// `GrcFvfChannel::Tex0` and `::Diffuse`, which this module numbers a primitive's sets from.
const TEX0: u8 = 6;
const DIFFUSE: u8 = 4;
/// A drawable states eight sets of texture coordinates and two of colours.
const MOST_UVS: usize = 8;
const MOST_COLOURS: usize = 2;
/// How many models up a parent chain is followed.
const MOST_PARENTS: usize = 256;

/// One FBX object: its class, its name and where its data is.
struct Item<'a> {
    class: &'a str,
    subclass: String,
    name: String,
    node: NodeHandle<'a>,
}

/// The objects of a document and how they connect.
struct Doc<'a> {
    objects: HashMap<i64, Item<'a>>,
    /// Object ids in file order.
    order: Vec<i64>,
    /// `child -> parents`, by object-to-object connection.
    parents: HashMap<i64, Vec<i64>>,
    /// `parent -> children`, in file order.
    children: HashMap<i64, Vec<i64>>,
}

/// Reads `bytes` as a binary FBX document.
pub fn load(bytes: &[u8]) -> Read<Scene> {
    let patched;
    let bytes = match check_header(bytes)? {
        Header::Read => bytes,
        // ⚠ Versions 7.1 to 7.3 lay their nodes out as 7.4 does, and the parser reads only 7.4
        // and later, so it is told what they are. Seen on one 7.3 file, which parses; the
        // difference between the versions is in the footer, which is not read.
        Header::AsSevenFour => {
            let mut copy = bytes.to_vec();
            copy[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&7400u32.to_le_bytes());
            patched = copy;
            &patched
        }
    };
    let tree = match AnyTree::from_seekable_reader(Cursor::new(bytes)) {
        Ok(AnyTree::V7400(_, tree, _)) => tree,
        Ok(_) => return fail("this FBX version is not one that is read"),
        Err(e) => return fail(format!("the FBX does not parse: {e}")),
    };
    let root = tree.root();
    let doc = Doc::of(root)?;
    let axes = Axes::of(root)?;
    scene_of(&doc, &axes)
}

enum Header {
    Read,
    AsSevenFour,
}

fn check_header(bytes: &[u8]) -> Read<Header> {
    if bytes.starts_with(b"; FBX") || bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return fail("this is an ASCII FBX, which is not read; export it as binary");
    }
    if !bytes.starts_with(MAGIC) || bytes.len() < MAGIC.len() + 4 {
        return fail("this is not a binary FBX: it does not open with the FBX header");
    }
    let at = MAGIC.len();
    let version = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if version < OLDEST {
        return fail(format!(
            "this is FBX {}.{}, and 7.1 is the oldest version read; re-export it newer",
            version / 1000,
            version % 1000 / 100
        ));
    }
    Ok(if version < 7400 {
        Header::AsSevenFour
    } else {
        Header::Read
    })
}

/// What an attribute says as text, with the `\0\x01` class suffix of an object's name left off.
fn text(value: Option<&AttributeValue>) -> String {
    let all = value.and_then(AttributeValue::get_string).unwrap_or("");
    all.split('\0').next().unwrap_or("").to_string()
}

fn number(value: &AttributeValue) -> Option<f64> {
    match value {
        AttributeValue::F64(v) => Some(*v),
        AttributeValue::F32(v) => Some(f64::from(*v)),
        AttributeValue::I32(v) => Some(f64::from(*v)),
        AttributeValue::I64(v) => Some(*v as f64),
        AttributeValue::I16(v) => Some(f64::from(*v)),
        _ => None,
    }
}

/// The values of the property `name` of the node's `Properties70`, after its four descriptors.
fn property(node: NodeHandle<'_>, name: &str) -> Option<Vec<f64>> {
    let properties = node.first_child_by_name("Properties70")?;
    let found = properties
        .children_by_name("P")
        .find(|p| p.attributes().first().and_then(AttributeValue::get_string) == Some(name))?;
    Some(
        found
            .attributes()
            .iter()
            .skip(4)
            .filter_map(number)
            .collect(),
    )
}

fn vec3(node: NodeHandle<'_>, name: &str, default: [f64; 3]) -> DVec3 {
    match property(node, name).as_deref() {
        Some([x, y, z, ..]) => DVec3::new(*x, *y, *z),
        _ => DVec3::from_array(default),
    }
}

impl<'a> Doc<'a> {
    fn of(root: NodeHandle<'a>) -> Read<Self> {
        let mut doc = Doc {
            objects: HashMap::new(),
            order: Vec::new(),
            parents: HashMap::new(),
            children: HashMap::new(),
        };
        if let Some(objects) = root.first_child_by_name("Objects") {
            for node in objects.children() {
                let attributes = node.attributes();
                let Some(id) = attributes.first().and_then(AttributeValue::get_i64) else {
                    continue;
                };
                doc.order.push(id);
                doc.objects.insert(
                    id,
                    Item {
                        class: node.name(),
                        name: text(attributes.get(1)),
                        subclass: text(attributes.get(2)),
                        node,
                    },
                );
            }
        }
        if let Some(connections) = root.first_child_by_name("Connections") {
            for connection in connections.children_by_name("C") {
                let attributes = connection.attributes();
                // `OP` joins an object to another's property, which nothing here reads.
                if attributes.first().and_then(AttributeValue::get_string) != Some("OO") {
                    continue;
                }
                let (Some(child), Some(parent)) = (
                    attributes.get(1).and_then(AttributeValue::get_i64),
                    attributes.get(2).and_then(AttributeValue::get_i64),
                ) else {
                    continue;
                };
                doc.parents.entry(child).or_default().push(parent);
                doc.children.entry(parent).or_default().push(child);
            }
        }
        Ok(doc)
    }

    fn item(&self, id: i64) -> Option<&Item<'a>> {
        self.objects.get(&id)
    }

    /// The children of `parent` of a class, in file order.
    fn children_of<'s>(
        &'s self,
        parent: i64,
        class: &'s str,
    ) -> impl Iterator<Item = (i64, &'s Item<'a>)> + 's {
        self.children
            .get(&parent)
            .into_iter()
            .flatten()
            .filter_map(move |id| Some((*id, self.item(*id)?)))
            .filter(move |(_, item)| item.class == class)
    }

    /// The model `child` hangs from, if it hangs from one.
    fn parent_model(&self, child: i64) -> Option<i64> {
        self.parents
            .get(&child)?
            .iter()
            .copied()
            .find(|p| self.item(*p).is_some_and(|i| i.class == "Model"))
    }

    /// A model's transform in the file's own space: its parents', then its own.
    fn world(&self, model: i64, cache: &mut HashMap<i64, DMat4>) -> DMat4 {
        self.world_below(model, cache, 0)
    }

    fn world_below(&self, model: i64, cache: &mut HashMap<i64, DMat4>, depth: usize) -> DMat4 {
        if let Some(done) = cache.get(&model) {
            return *done;
        }
        // ⚠ A cycle of parents is malformed and a document can carry one, and so can a chain a
        // few hundred deep, so a model already on the way up, or past that depth, is taken as the
        // identity rather than followed again.
        if depth > MOST_PARENTS {
            return DMat4::IDENTITY;
        }
        cache.insert(model, DMat4::IDENTITY);
        let local = self
            .item(model)
            .map_or(DMat4::IDENTITY, |m| local_of(m.node));
        let world = match self.parent_model(model) {
            Some(parent) => self.world_below(parent, cache, depth + 1) * local,
            None => local,
        };
        cache.insert(model, world);
        world
    }
}

/// The matrix of a rotation of `degrees` about `axis`.
fn about(axis: usize, degrees: f64) -> DMat4 {
    let angle = degrees.to_radians();
    DMat4::from_mat3(match axis {
        0 => DMat3::from_rotation_x(angle),
        1 => DMat3::from_rotation_y(angle),
        _ => DMat3::from_rotation_z(angle),
    })
}

/// A rotation of `angles` degrees about X, Y and Z, applied in the order the file's `RotationOrder` names.
fn euler(order: i32, angles: DVec3) -> DMat4 {
    // The first axis listed turns first, so its matrix is the last factor.
    let axes: [usize; 3] = match order {
        1 => [0, 2, 1],
        2 => [1, 2, 0],
        3 => [1, 0, 2],
        4 => [2, 0, 1],
        5 => [2, 1, 0],
        _ => [0, 1, 2],
    };
    let turn = |axis: usize| about(axis, angles[axis]);
    turn(axes[2]) * turn(axes[1]) * turn(axes[0])
}

/// A model's own transform, by the formula the FBX SDK states for it.
///
/// `T * Roff * Rp * Rpre * R * Rpost^-1 * Rp^-1 * Soff * Sp * S * Sp^-1`, with every pivot and
/// offset zero where a file states none.
fn local_of(node: NodeHandle<'_>) -> DMat4 {
    let order = property(node, "RotationOrder")
        .and_then(|v| v.first().copied())
        .unwrap_or(0.0) as i32;
    let at = |name: &str| DMat4::from_translation(vec3(node, name, [0.0; 3]));
    let translation = at("Lcl Translation");
    let rotation = euler(order, vec3(node, "Lcl Rotation", [0.0; 3]));
    let pre = euler(order, vec3(node, "PreRotation", [0.0; 3]));
    let post = euler(order, vec3(node, "PostRotation", [0.0; 3]));
    let scale = DMat4::from_scale(vec3(node, "Lcl Scaling", [1.0; 3]));
    let (rp, sp) = (at("RotationPivot"), at("ScalingPivot"));
    translation
        * at("RotationOffset")
        * rp
        * pre
        * rotation
        * post.inverse()
        * rp.inverse()
        * at("ScalingOffset")
        * sp
        * scale
        * sp.inverse()
}

/// The file's axes and unit as one matrix into the drawable's space.
struct Axes {
    to_rage: DMat4,
}

impl Axes {
    fn of(root: NodeHandle<'_>) -> Read<Self> {
        let settings = root.first_child_by_name("GlobalSettings");
        let get = |name: &str, default: i32| {
            settings
                .and_then(|s| property(s, name))
                .and_then(|v| v.first().copied())
                .map_or(default, |v| v as i32)
        };
        // The FBX SDK's own defaults, which are Maya's: Y up, Z toward the viewer, X across.
        let axis = |name: &str, sign: &str, default: (i32, i32)| -> Read<DVec3> {
            let (index, sign) = (get(name, default.0), get(sign, default.1));
            if !(0..3).contains(&index) || sign == 0 {
                return fail(format!(
                    "{name} is {index} and its sign {sign}, which is no axis"
                ));
            }
            let mut v = DVec3::ZERO;
            v[index as usize] = f64::from(sign.signum());
            Ok(v)
        };
        let coord = axis("CoordAxis", "CoordAxisSign", (0, 1))?;
        let up = axis("UpAxis", "UpAxisSign", (1, 1))?;
        let front = axis("FrontAxis", "FrontAxisSign", (2, 1))?;
        if coord.abs() == up.abs() || coord.abs() == front.abs() || up.abs() == front.abs() {
            return fail("the file's coordinate, up and front axes are not three different axes");
        }
        // ⚠ The coordinate axis goes to X and up to Z, as a file's right and up do in a drawable,
        // and the front axis, which points at the viewer, to -Y. A file whose three axes are left-handed
        // makes this a mirror, which the winding follows through `determinant`.
        let rage = DMat3::from_cols(DVec3::X, DVec3::Z, -DVec3::Y);
        let file = DMat3::from_cols(coord, up, front);
        let to_rage = rage * file.transpose();
        // A file's unit is centimetres where it says one, and a drawable's is the metre.
        let unit = settings
            .and_then(|s| property(s, "UnitScaleFactor"))
            .and_then(|v| v.first().copied())
            .filter(|v| *v > 0.0)
            .unwrap_or(1.0);
        Ok(Self {
            to_rage: DMat4::from_scale(DVec3::splat(unit / 100.0)) * DMat4::from_mat3(to_rage),
        })
    }
}

/// How a layer element's values are looked up from a corner of a polygon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mapping {
    Corner,
    ControlPoint,
    Polygon,
    AllSame,
}

/// One layer element: normals, a set of coordinates, colours.
struct Layer {
    mapping: Mapping,
    /// Every value, `width` numbers each.
    values: Vec<f64>,
    width: usize,
    /// Which value a position names, when the layer states an index.
    index: Option<Vec<i32>>,
}

impl Layer {
    /// The layer element of `node` named `data`, or why it is none, as a note for the report.
    fn of(node: NodeHandle<'_>, data: &str, index: &str, width: usize) -> Result<Self, String> {
        let attribute = |name: &str| {
            node.first_child_by_name(name)
                .and_then(|n| n.attributes().first())
        };
        let mapping = match text(attribute("MappingInformationType")).as_str() {
            "ByPolygonVertex" => Mapping::Corner,
            "ByVertice" | "ByVertex" | "ByControlPoint" => Mapping::ControlPoint,
            "ByPolygon" => Mapping::Polygon,
            "AllSame" => Mapping::AllSame,
            other => return Err(format!("mapped {other:?}")),
        };
        let values: Vec<f64> = match attribute(data) {
            Some(AttributeValue::ArrF64(v)) => v.clone(),
            Some(AttributeValue::ArrF32(v)) => v.iter().map(|x| f64::from(*x)).collect(),
            _ => return Err(format!("with no {data}")),
        };
        let indexed = matches!(
            text(attribute("ReferenceInformationType")).as_str(),
            "IndexToDirect" | "Index"
        );
        let index = match (indexed, attribute(index)) {
            (true, Some(AttributeValue::ArrI32(v))) => Some(v.clone()),
            (true, _) => return Err(format!("indexed with no {index}")),
            (false, _) => None,
        };
        Ok(Self {
            mapping,
            values,
            width,
            index,
        })
    }

    /// The value at a corner of a polygon, whose control point is `point`.
    fn at(&self, corner: usize, point: usize, polygon: usize) -> Option<&[f64]> {
        let position = match self.mapping {
            Mapping::Corner => corner,
            Mapping::ControlPoint => point,
            Mapping::Polygon => polygon,
            Mapping::AllSame => 0,
        };
        let value = match &self.index {
            Some(index) => usize::try_from(*index.get(position)?).ok()?,
            None => position,
        };
        self.values
            .get(value * self.width..(value + 1) * self.width)
    }
}

/// The layers a mesh carries, each kind in the order the file lists it.
#[derive(Default)]
struct Layers {
    normals: Option<Layer>,
    tangents: Option<Layer>,
    binormals: Option<Layer>,
    uvs: Vec<Layer>,
    colours: Vec<Layer>,
    materials: Option<Layer>,
}

/// What a mesh's layers left out, for the report.
#[derive(Default)]
struct Left {
    /// A layer a corner could not be looked up in, by its kind.
    unreadable: Vec<String>,
    uv_sets: usize,
    colour_sets: usize,
}

fn layers_of(geometry: NodeHandle<'_>, left: &mut Left) -> Layers {
    let mut out = Layers::default();
    let mut read = |name: &str, data: &str, index: &str, width: usize| -> Vec<Layer> {
        let mut all = Vec::new();
        for node in geometry.children_by_name(name) {
            match Layer::of(node, data, index, width) {
                Ok(layer) => all.push(layer),
                Err(why) => left.unreadable.push(format!("{name} {why}")),
            }
        }
        all
    };
    out.normals = read("LayerElementNormal", "Normals", "NormalsIndex", 3)
        .into_iter()
        .next();
    out.tangents = read("LayerElementTangent", "Tangents", "TangentsIndex", 3)
        .into_iter()
        .next();
    out.binormals = read("LayerElementBinormal", "Binormals", "BinormalsIndex", 3)
        .into_iter()
        .next();
    out.uvs = read("LayerElementUV", "UV", "UVIndex", 2);
    out.colours = read("LayerElementColor", "Colors", "ColorIndex", 4);
    if let Some(node) = geometry.first_child_by_name("LayerElementMaterial") {
        // A material layer is a list of whole numbers, which no other layer is.
        let mapping = text(
            node.first_child_by_name("MappingInformationType")
                .and_then(|n| n.attributes().first()),
        );
        if let Some(AttributeValue::ArrI32(slots)) = node
            .first_child_by_name("Materials")
            .and_then(|n| n.attributes().first())
        {
            let mapping = match mapping.as_str() {
                "ByPolygon" => Some(Mapping::Polygon),
                "AllSame" => Some(Mapping::AllSame),
                _ => None,
            };
            match mapping {
                Some(mapping) => {
                    out.materials = Some(Layer {
                        mapping,
                        values: slots.iter().map(|s| f64::from(*s)).collect(),
                        width: 1,
                        index: None,
                    })
                }
                None => left
                    .unreadable
                    .push(format!("LayerElementMaterial mapped {mapping:?}")),
            }
        }
    }
    left.uv_sets += out.uvs.len().saturating_sub(MOST_UVS);
    out.uvs.truncate(MOST_UVS);
    left.colour_sets += out.colours.len().saturating_sub(MOST_COLOURS);
    out.colours.truncate(MOST_COLOURS);
    out
}

/// A mesh's polygons: the control point of each corner, and where each polygon's corners end.
struct Polygons {
    points: Vec<usize>,
    /// The index one past each polygon's last corner.
    ends: Vec<usize>,
}

fn polygons_of(geometry: NodeHandle<'_>, control_points: usize, name: &str) -> Read<Polygons> {
    let Some(AttributeValue::ArrI32(raw)) = geometry
        .first_child_by_name("PolygonVertexIndex")
        .and_then(|n| n.attributes().first())
    else {
        return fail(format!("the mesh {name:?} has no PolygonVertexIndex"));
    };
    let mut out = Polygons {
        points: Vec::with_capacity(raw.len()),
        ends: Vec::new(),
    };
    for value in raw {
        // The last corner of each polygon is stored as its bitwise complement.
        let (point, last) = if *value < 0 {
            (!*value, true)
        } else {
            (*value, false)
        };
        let point = usize::try_from(point)
            .ok()
            .filter(|p| *p < control_points)
            .ok_or_else(|| {
                ReadError(format!(
                    "the mesh {name:?} names control point {point} of {control_points}"
                ))
            })?;
        out.points.push(point);
        if last {
            out.ends.push(out.points.len());
        }
    }
    if out.ends.last() != Some(&out.points.len()) {
        return fail(format!(
            "the mesh {name:?} ends in the middle of a polygon, so its last corner is not marked"
        ));
    }
    Ok(out)
}

/// A skinned mesh's influences: bone ids and weights for each control point.
struct Skin {
    influences: Vec<Vec<(i64, f64)>>,
    /// The mesh's transform the bones were bound at, in the file's space.
    bind: Option<DMat4>,
}

/// The skin of a geometry, with the bones it names in `bones` by model id.
fn skin_of(
    doc: &Doc<'_>,
    geometry: i64,
    control_points: usize,
    bones: &mut BindPoses,
) -> Read<Option<Skin>> {
    let mut skin: Option<Skin> = None;
    for (deformer, item) in doc.children_of(geometry, "Deformer") {
        if item.subclass != "Skin" {
            continue;
        }
        for (cluster, cluster_item) in doc.children_of(deformer, "Deformer") {
            if cluster_item.subclass != "Cluster" {
                continue;
            }
            let node = cluster_item.node;
            let bone = doc.children_of(cluster, "Model").map(|(id, _)| id).next();
            let Some(bone) = bone else { continue };
            let skin = skin.get_or_insert_with(|| Skin {
                influences: vec![Vec::new(); control_points],
                bind: None,
            });
            let matrix = |name: &str| match node
                .first_child_by_name(name)
                .and_then(|n| n.attributes().first())
            {
                Some(AttributeValue::ArrF64(m)) if m.len() == 16 => {
                    let mut all = [0.0; 16];
                    all.copy_from_slice(m);
                    Some(DMat4::from_cols_array(&all))
                }
                _ => None,
            };
            if skin.bind.is_none() {
                skin.bind = matrix("Transform");
            }
            if let Some(link) = matrix("TransformLink") {
                bones.entry(bone).or_insert(link);
            } else {
                bones.entry(bone).or_insert(DMat4::NAN);
            }
            let indexes = node
                .first_child_by_name("Indexes")
                .and_then(|n| n.attributes().first());
            let weights = node
                .first_child_by_name("Weights")
                .and_then(|n| n.attributes().first());
            let (Some(AttributeValue::ArrI32(indexes)), Some(AttributeValue::ArrF64(weights))) =
                (indexes, weights)
            else {
                continue;
            };
            if indexes.len() != weights.len() {
                return fail(format!(
                    "the cluster {:?} lists {} control points and {} weights",
                    cluster_item.name,
                    indexes.len(),
                    weights.len()
                ));
            }
            for (point, weight) in indexes.iter().zip(weights) {
                let slot = usize::try_from(*point).ok().filter(|p| *p < control_points);
                let Some(slot) = slot else {
                    return fail(format!(
                        "the cluster {:?} weights control point {point} of {control_points}",
                        cluster_item.name
                    ));
                };
                skin.influences[slot].push((bone, *weight));
            }
        }
    }
    Ok(skin)
}

/// The bind-pose world transform of each bone a cluster names, by model id (`NAN` where it states none).
type BindPoses = BTreeMap<i64, DMat4>;

/// `lod<N>` in a model's name, as `LodSlot`, or the highest level.
fn lod_of(name: &str) -> LodSlot {
    let lower = name.to_ascii_lowercase();
    if lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == "container")
    {
        return LodSlot::Container;
    }
    for (at, _) in lower.match_indices("lod") {
        let digits: String = lower[at + 3..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let bounded = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphabetic();
        if let (true, Ok(n)) = (bounded, digits.parse::<u8>()) {
            return LodSlot::Level(n.min(3));
        }
    }
    LodSlot::Level(0)
}

/// The bits a vertex is told apart by: two corners that state the same values are one vertex.
fn key(parts: &[&[f64]], point: usize) -> Vec<u64> {
    let mut bits = vec![point as u64];
    for part in parts {
        bits.extend(part.iter().map(|v| v.to_bits()));
    }
    bits
}

/// What one primitive is made of while its corners are read.
#[derive(Default)]
struct Builder {
    seen: HashMap<Vec<u64>, u32>,
    corner_points: Vec<usize>,
    primitive: Primitive,
    /// The unit-length normal, tangent and binormal of each vertex, in the file's space.
    frames: Vec<[Option<DVec3>; 3]>,
}

/// A skinned primitive's place in the scene, and the influences of each of its vertices by model id.
struct Skinned {
    mesh: usize,
    primitive: usize,
    influences: Vec<Vec<(i64, f64)>>,
}

fn scene_of(doc: &Doc<'_>, axes: &Axes) -> Read<Scene> {
    let mut scene = Scene::default();
    let mut object = Object::default();
    let mut world_cache: HashMap<i64, DMat4> = HashMap::new();
    let mut bones = BindPoses::new();
    let mut left = Left::default();
    let (mut fanned, mut truncated, mut unweighted) = (0usize, 0usize, 0usize);
    let mut skinned_primitives: Vec<Skinned> = Vec::new();

    for &model in &doc.order {
        let Some(item) = doc.item(model) else {
            continue;
        };
        if item.class != "Model" || item.subclass != "Mesh" {
            continue;
        }
        for (geometry, geo) in doc.children_of(model, "Geometry") {
            if geo.subclass != "Mesh" {
                continue;
            }
            let node = geo.node;
            let Some(AttributeValue::ArrF64(raw)) = node
                .first_child_by_name("Vertices")
                .and_then(|n| n.attributes().first())
            else {
                return fail(format!("the mesh {:?} has no Vertices", item.name));
            };
            if raw.len() % 3 != 0 {
                return fail(format!(
                    "the mesh {:?} lists {} numbers for its vertices, which is not whole points",
                    item.name,
                    raw.len()
                ));
            }
            let points: Vec<DVec3> = raw
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| DVec3::new(p[0], p[1], p[2]))
                .collect();
            let polygons = polygons_of(node, points.len(), &item.name)?;
            let layers = layers_of(node, &mut left);
            let skin = skin_of(doc, geometry, points.len(), &mut bones)?;

            // A skinned mesh sits where its bones were bound; any other, where its model is.
            let file_space = match skin.as_ref().and_then(|s| s.bind) {
                Some(bind) => bind,
                None => doc.world(model, &mut world_cache),
            };
            let placement = axes.to_rage * file_space;
            let mirrored = placement.determinant() < 0.0;
            let normal_matrix = DMat3::from_mat4(placement).inverse().transpose();

            let mut by_material: BTreeMap<i64, Builder> = BTreeMap::new();
            let mut start = 0;
            for (polygon, &end) in polygons.ends.iter().enumerate() {
                let corners = start..end;
                start = end;
                if corners.len() < 3 {
                    continue;
                }
                if corners.len() > 4 {
                    fanned += 1;
                }
                let material = layers
                    .materials
                    .as_ref()
                    .and_then(|m| m.at(corners.start, polygons.points[corners.start], polygon))
                    .map_or(0, |v| v[0] as i64);
                let builder = by_material.entry(material).or_default();
                let mut vertices = Vec::with_capacity(corners.len());
                for corner in corners.clone() {
                    let point = polygons.points[corner];
                    vertices.push(builder.vertex(corner, point, polygon, &points, &layers));
                }
                for i in 1..vertices.len() - 1 {
                    let triangle = [vertices[0], vertices[i], vertices[i + 1]];
                    if mirrored {
                        builder
                            .primitive
                            .indices
                            .extend([triangle[0], triangle[2], triangle[1]]);
                    } else {
                        builder.primitive.indices.extend(triangle);
                    }
                }
            }

            let mut mesh = Mesh {
                name: item.name.clone(),
                lod: lod_of(&item.name),
                primitives: Vec::new(),
            };
            for (_, mut builder) in by_material {
                builder.finish(&placement, &normal_matrix, &layers, mesh.lod);
                if let Some(skin) = &skin {
                    let per_vertex = builder
                        .corner_points
                        .iter()
                        .map(|p| skin.influences[*p].clone())
                        .collect();
                    skinned_primitives.push(Skinned {
                        mesh: object.meshes.len(),
                        primitive: mesh.primitives.len(),
                        influences: per_vertex,
                    });
                }
                mesh.primitives.push(builder.primitive);
            }
            if !mesh.primitives.is_empty() {
                object.meshes.push(mesh);
            }
        }
    }
    if object.meshes.is_empty() {
        return fail("the document places no mesh on any model");
    }

    // The bones in the order the clusters first name them, which is the order joints index.
    if !bones.is_empty() {
        let ids: Vec<i64> = {
            let mut seen = Vec::new();
            for skinned in &skinned_primitives {
                for (bone, _) in skinned.influences.iter().flatten() {
                    if !seen.contains(bone) {
                        seen.push(*bone);
                    }
                }
            }
            for id in bones.keys() {
                if !seen.contains(id) {
                    seen.push(*id);
                }
            }
            seen
        };
        let joint_of: HashMap<i64, u16> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i as u16))
            .collect();
        object.bones = bones_of(doc, axes, &ids, &bones, &mut world_cache);
        for skinned in skinned_primitives {
            let target = &mut object.meshes[skinned.mesh].primitives[skinned.primitive];
            for lanes in skinned.influences {
                let (joints, weights, cut, none) = lanes_of(&lanes, &joint_of);
                target.joints.push(joints);
                target.weights.push(weights);
                truncated += usize::from(cut);
                unweighted += usize::from(none);
            }
        }
    }
    scene.name = "imported".to_string();
    object.name = scene.name.clone();
    scene.objects.push(object);
    scene.dropped = dropped_of(doc, &left, fanned, truncated, unweighted);
    Ok(scene)
}

impl Builder {
    /// The vertex a corner makes, which is the one an earlier corner made where it states the same values.
    fn vertex(
        &mut self,
        corner: usize,
        point: usize,
        polygon: usize,
        points: &[DVec3],
        layers: &Layers,
    ) -> u32 {
        fn three(
            layer: &Option<Layer>,
            corner: usize,
            point: usize,
            polygon: usize,
        ) -> Option<&[f64]> {
            layer.as_ref().and_then(|l| l.at(corner, point, polygon))
        }
        let normal = three(&layers.normals, corner, point, polygon);
        let tangent = three(&layers.tangents, corner, point, polygon);
        let binormal = three(&layers.binormals, corner, point, polygon);
        let uvs: Vec<&[f64]> = layers
            .uvs
            .iter()
            .filter_map(|l| l.at(corner, point, polygon))
            .collect();
        let colours: Vec<&[f64]> = layers
            .colours
            .iter()
            .filter_map(|l| l.at(corner, point, polygon))
            .collect();
        let mut parts: Vec<&[f64]> = Vec::new();
        parts.extend(normal);
        parts.extend(tangent);
        parts.extend(binormal);
        parts.extend(&uvs);
        parts.extend(&colours);
        let bits = key(&parts, point);
        if let Some(found) = self.seen.get(&bits) {
            return *found;
        }
        let at = self.primitive.positions.len() as u32;
        self.seen.insert(bits, at);
        self.corner_points.push(point);
        let p = points[point];
        self.primitive
            .positions
            .push([p.x as f32, p.y as f32, p.z as f32]);
        let direction = |v: Option<&[f64]>| v.map(|v| DVec3::new(v[0], v[1], v[2]));
        self.frames
            .push([direction(normal), direction(tangent), direction(binormal)]);
        for (slot, uv) in uvs.iter().enumerate() {
            if self.primitive.uvs.len() <= slot {
                self.primitive.uvs.push(Vec::new());
            }
            // FBX measures V up from the bottom of the image and a drawable down from the top.
            self.primitive.uvs[slot].push([uv[0] as f32, 1.0 - uv[1] as f32]);
        }
        for (slot, colour) in colours.iter().enumerate() {
            if self.primitive.colours.len() <= slot {
                self.primitive.colours.push(Vec::new());
            }
            self.primitive.colours[slot].push([
                colour[0] as f32,
                colour[1] as f32,
                colour[2] as f32,
                colour[3] as f32,
            ]);
        }
        at
    }

    /// Carries the vertices into the drawable's space and names the channels.
    fn finish(&mut self, placement: &DMat4, normal_matrix: &DMat3, layers: &Layers, lod: LodSlot) {
        let primitive = &mut self.primitive;
        primitive.lod = lod;
        for position in &mut primitive.positions {
            let moved = placement.transform_point3(DVec3::new(
                f64::from(position[0]),
                f64::from(position[1]),
                f64::from(position[2]),
            ));
            *position = [moved.x as f32, moved.y as f32, moved.z as f32];
        }
        let rotate = |v: DVec3| (*normal_matrix * v).normalize_or_zero();
        let all_have = |at: usize| self.frames.iter().all(|f| f[at].is_some());
        if layers.normals.is_some() && all_have(0) {
            primitive.normals = self
                .frames
                .iter()
                .map(|f| rotate(f[0].unwrap_or_default()).as_vec3().to_array())
                .collect();
        }
        if !primitive.normals.is_empty() && layers.tangents.is_some() && all_have(1) {
            // The fourth lane is the side the bitangent lies on: the cross of the normal and the
            // tangent against the file's own bitangent, where it states one.
            primitive.tangents = self
                .frames
                .iter()
                .map(|f| {
                    let (n, t) = (f[0].unwrap_or_default(), f[1].unwrap_or_default());
                    let sign = match f[2] {
                        Some(b) if n.cross(t).dot(b) < 0.0 => -1.0,
                        _ => 1.0,
                    };
                    let t = rotate(t).as_vec3();
                    [t.x, t.y, t.z, sign]
                })
                .collect();
        }
        primitive.uv_channels = (0..primitive.uvs.len()).map(|n| TEX0 + n as u8).collect();
        primitive.colour_channels = (0..primitive.colours.len())
            .map(|n| DIFFUSE + n as u8)
            .collect();
    }
}

/// A vertex's four heaviest influences as joint indices and weights that sum to one.
///
/// The booleans say whether more than four were cut and whether it was weighted at all.
fn lanes_of(
    influences: &[(i64, f64)],
    joint_of: &HashMap<i64, u16>,
) -> ([u16; 4], [f32; 4], bool, bool) {
    let mut merged: Vec<(u16, f64)> = Vec::new();
    for (bone, weight) in influences {
        let (Some(joint), true) = (joint_of.get(bone), *weight > 0.0) else {
            continue;
        };
        match merged.iter_mut().find(|m| m.0 == *joint) {
            Some(m) => m.1 += weight,
            None => merged.push((*joint, *weight)),
        }
    }
    merged.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let cut = merged.len() > 4;
    merged.truncate(4);
    let total: f64 = merged.iter().map(|m| m.1).sum();
    let (mut joints, mut weights) = ([0u16; 4], [0.0f32; 4]);
    if total <= 0.0 {
        // A skinned mesh's vertex with no weight is bound whole to the first joint.
        weights[0] = 1.0;
        return (joints, weights, false, true);
    }
    for (lane, (joint, weight)) in merged.iter().enumerate() {
        joints[lane] = *joint;
        weights[lane] = (weight / total) as f32;
    }
    (joints, weights, cut, false)
}

/// The bones a skin names, in the drawable's space, each with its local transform under its nearest parent among them.
fn bones_of(
    doc: &Doc<'_>,
    axes: &Axes,
    ids: &[i64],
    poses: &BindPoses,
    cache: &mut HashMap<i64, DMat4>,
) -> Vec<Bone> {
    // A bone's world transform in the drawable's space: the pose it was bound at where the file
    // states one, else where its model stands.
    let to_rage = axes.to_rage;
    let inverse = to_rage.inverse();
    let worlds: Vec<DMat4> = ids
        .iter()
        .map(|id| {
            let file = match poses.get(id) {
                Some(pose) if !pose.is_nan() => *pose,
                _ => doc.world(*id, cache),
            };
            to_rage * file * inverse
        })
        .collect();
    ids.iter()
        .enumerate()
        .map(|(at, id)| {
            // The nearest ancestor that is itself one of the bones.
            let mut parent = doc.parent_model(*id);
            let mut hops = 0;
            let found = loop {
                match parent {
                    Some(p) if ids.contains(&p) => break ids.iter().position(|i| *i == p),
                    Some(p) if hops < 64 => {
                        parent = doc.parent_model(p);
                        hops += 1;
                    }
                    _ => break None,
                }
            };
            let local = match found {
                Some(p) => worlds[p].inverse() * worlds[at],
                None => worlds[at],
            };
            let (scale, rotation, translation) = local.as_mat4().to_scale_rotation_translation();
            Bone {
                name: doc
                    .item(*id)
                    .map_or_else(|| format!("bone{at}"), |i| i.name.clone()),
                tag: 0,
                parent: found,
                translation: translation.to_array(),
                rotation: rotation.normalize().to_array(),
                scale: scale.to_array(),
            }
        })
        .collect()
}

/// What the file held that an edit of a drawable has no place for.
fn dropped_of(
    doc: &Doc<'_>,
    left: &Left,
    fanned: usize,
    truncated: usize,
    unweighted: usize,
) -> Vec<Dropped> {
    let count = |class: &str, subclass: Option<&str>| {
        doc.objects
            .values()
            .filter(|o| o.class == class && subclass.is_none_or(|s| o.subclass == s))
            .count()
    };
    let models = |subclass: &str| count("Model", Some(subclass));
    let blend_shapes = doc
        .objects
        .values()
        .filter(|o| o.class == "Deformer" && o.subclass == "BlendShape")
        .count();
    let mut out = Vec::new();
    let mut note = |what: &str, count: usize| {
        if count > 0 {
            out.push(Dropped::new(what, count));
        }
    };
    note("materials", count("Material", None));
    note("textures", count("Texture", None).max(count("Video", None)));
    note(
        "animation stacks",
        count("AnimStack", None).max(count("AnimationStack", None)),
    );
    note("cameras", models("Camera"));
    note("lights", models("Light"));
    note("blend shapes", blend_shapes);
    note("sets of coordinates after the eighth", left.uv_sets);
    note("sets of colours after the second", left.colour_sets);
    for why in &left.unreadable {
        note(&format!("layers ({why})"), 1);
    }
    note(
        "polygons of more than four corners (cut into fans, which a concave one is wrong in)",
        fanned,
    );
    note(
        "vertices with more than four influences (the four heaviest kept)",
        truncated,
    );
    note(
        "skinned vertices with no weight (bound whole to the first joint)",
        unweighted,
    );

    out
}
