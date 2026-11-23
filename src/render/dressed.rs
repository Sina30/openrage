//! What the viewport draws for the dressed ped: parts it is handed, none of them a document.
//!
//! The state reads the files and the bridge uploads what is here, so neither knows the other's
//! types. A part is drawn at the rest pose, where the stored positions of a ped component are the
//! positions the skeleton gives them, so nothing is skinned.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use or_anim::SkinnedMesh;
use or_renderer::game_shaders::mesh::{Image, Material, Surface};
use or_renderer::MeshVertex;

/// The surfaces and materials the game's own passes draw a drawable with.
pub type Surfaces = Arc<Vec<(Surface, Material)>>;

/// `surfaces` held to bone `bone` of the skeleton by a full weight, which a pose of the skeleton carries them by.
pub fn hold_to_bone(surfaces: &mut [(Surface, Material)], bone: usize) {
    for (surface, _) in surfaces {
        let count = surface.positions.len();
        surface.weights = vec![[1.0, 0.0, 0.0, 0.0]; count];
        surface.joints = vec![[bone as u16, 0, 0, 0]; count];
    }
}

/// One mesh's vertices and indices.
pub struct Geometry {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

/// The levels of detail a drawable can have models at: high, medium, low and very low.
pub const LEVELS: usize = 4;

const LEVEL_NAMES: [&str; LEVELS] = [
    tr_key!("High"),
    tr_key!("Medium"),
    tr_key!("Low"),
    tr_key!("Very low"),
];

/// What level `at` of detail is called, in the language in use.
pub fn level_name(at: usize) -> &'static str {
    crate::i18n::tr(LEVEL_NAMES[at])
}

/// What a set of meshes costs to draw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub vertices: usize,
    pub triangles: usize,
    /// How many parts the figures add.
    pub parts: usize,
}

impl Counts {
    pub fn of(geometry: &[Geometry]) -> Self {
        Self {
            vertices: geometry.iter().map(|g| g.vertices.len()).sum(),
            triangles: geometry.iter().map(|g| g.indices.len() / 3).sum(),
            parts: 1,
        }
    }

    pub fn add(&mut self, other: Counts) {
        self.vertices += other.vertices;
        self.triangles += other.triangles;
        self.parts += other.parts;
    }
}

/// What the debug views of the dressed ped show of one mesh that its plain vertices do not carry: the colours the first vertex colour and the first tangent of each vertex are drawn in, where the mesh has the channel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Aux {
    pub colour: Option<Vec<[f32; 4]>>,
    pub tangent: Option<Vec<[f32; 4]>>,
}

impl Aux {
    /// The colours of the channel `channel` names, none for a mesh that lacks it or a channel these views do not show.
    pub fn of(&self, channel: crate::render::debug_channel::DebugChannel) -> Option<&[[f32; 4]]> {
        use crate::render::debug_channel::DebugChannel;
        match channel {
            DebugChannel::Colour(0) => self.colour.as_deref(),
            DebugChannel::Tangent(0) => self.tangent.as_deref(),
            _ => None,
        }
    }
}

/// One level of a [`Model`].
pub struct Level<'a> {
    /// The level these meshes are of, which is not the one asked for where the file has no models there.
    pub lod: usize,
    pub geometry: &'a Arc<Vec<Geometry>>,
    /// The skinning of each mesh, by the same index.
    pub skins: &'a [Option<Arc<SkinnedMesh>>],
    /// The data of the debug views for each mesh, by the same index; none where the model was made without any.
    pub aux: &'a [Arc<Aux>],
}

/// The meshes of one model file at each level of detail it has models at, with the skinning of each.
#[derive(Default)]
pub struct Model {
    levels: [Option<Arc<Vec<Geometry>>>; LEVELS],
    skins: [Vec<Option<Arc<SkinnedMesh>>>; LEVELS],
    aux: [Vec<Arc<Aux>>; LEVELS],
    /// What the game's own passes draw the file with: its first drawable's surfaces and materials, lacking the texture it is worn with.
    shaded: Option<Surfaces>,
}

impl Model {
    /// A model of these meshes by level, each with its skinning where the file states one; a level with no mesh is a level the file has none at.
    pub fn new(levels: [Vec<(Geometry, Option<SkinnedMesh>)>; LEVELS]) -> Self {
        let mut model = Self::default();
        for (at, level) in levels.into_iter().enumerate() {
            if level.is_empty() {
                continue;
            }
            let (geometry, skins): (Vec<_>, Vec<_>) = level.into_iter().unzip();
            model.levels[at] = Some(Arc::new(geometry));
            model.skins[at] = skins.into_iter().map(|s| s.map(Arc::new)).collect();
        }
        model
    }

    /// This model with the data of the debug views of each mesh of each level, in the order of the meshes `new` was given.
    pub fn with_aux(mut self, aux: [Vec<Aux>; LEVELS]) -> Self {
        self.aux = aux.map(|level| level.into_iter().map(Arc::new).collect());
        self
    }

    /// This model with the surfaces the game's passes draw it with.
    pub fn with_shaded(mut self, shaded: Option<Surfaces>) -> Self {
        self.shaded = shaded;
        self
    }

    /// The surfaces and materials the game's passes draw the file with, none for a file that holds no drawable.
    pub fn shaded(&self) -> Option<&Surfaces> {
        self.shaded.as_ref()
    }

    /// Whether the file has models at exactly level `lod`.
    pub fn holds(&self, lod: usize) -> bool {
        self.levels.get(lod).is_some_and(Option::is_some)
    }

    /// Level `lod`, else the first level the file has models at, as the Model workspace draws a level a drawable lacks; none for a file with no models.
    pub fn at(&self, lod: usize) -> Option<Level<'_>> {
        let lod = if self.holds(lod) { lod } else { 0 };
        let lod = if self.holds(lod) {
            lod
        } else {
            (0..LEVELS).find(|l| self.holds(*l))?
        };
        Some(Level {
            lod,
            geometry: self.levels[lod].as_ref()?,
            skins: &self.skins[lod],
            aux: &self.aux[lod],
        })
    }
}

/// A texture decoded to the pixels a mesh samples.
pub struct Diffuse {
    /// Names these pixels: a texture read again gets another.
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub srgb: bool,
}

/// The meshes of one drawable with the texture they are drawn with.
pub struct ScenePart {
    pub geometry: Arc<Vec<Geometry>>,
    pub diffuse: Option<Arc<Diffuse>>,
    /// Multiplies the texture, as a vertex colour does.
    pub tint: [f32; 3],
    /// How far the part is raised, in metres.
    pub lift: f32,
    /// The id of the garment that wears it, none for the ped's own drawable.
    pub garment: Option<String>,
}

/// Everything the viewport draws for the dressed ped.
pub struct DressedScene {
    /// Changes whenever what is drawn does.
    pub key: u64,
    pub parts: Vec<ScenePart>,
    pub centre: Vec3,
    pub radius: f32,
    /// What the garments worn cost at each level of detail, whatever level is drawn; a garment with no model at a level costs its first.
    pub levels: [Counts; LEVELS],
    /// How many garments worn hold models at exactly each level.
    pub held: [usize; LEVELS],
    /// The skinning of each geometry of each part, by the same indices; empty for a scene that is not posed.
    skins: Vec<Vec<Option<Arc<SkinnedMesh>>>>,
    /// The data of the debug views for each geometry of each part, by the same indices; empty for a scene made without any.
    aux: Vec<Vec<Option<Arc<Aux>>>>,
    /// The surfaces the game's passes draw each part with, by the same indices; empty for a scene made without any.
    shaded: Vec<Option<Surfaces>>,
}

impl DressedScene {
    pub fn new(key: u64, parts: Vec<ScenePart>) -> Self {
        let (centre, radius) = bounds(&parts);
        Self {
            key,
            parts,
            centre,
            radius,
            levels: Default::default(),
            held: Default::default(),
            skins: Vec::new(),
            aux: Vec::new(),
            shaded: Vec::new(),
        }
    }

    pub fn with_levels(mut self, levels: [Counts; LEVELS], held: [usize; LEVELS]) -> Self {
        self.levels = levels;
        self.held = held;
        self
    }

    /// How many of the parts are a garment's, which the ped's own drawables are not.
    pub fn garments(&self) -> usize {
        self.parts.iter().filter(|p| p.garment.is_some()).count()
    }

    pub fn with_skins(mut self, skins: Vec<Vec<Option<Arc<SkinnedMesh>>>>) -> Self {
        self.skins = skins;
        self
    }

    pub fn with_aux(mut self, aux: Vec<Vec<Option<Arc<Aux>>>>) -> Self {
        self.aux = aux;
        self
    }

    /// This scene with the surfaces the game's passes draw each part with, by the order of the parts.
    pub fn with_shaded(mut self, shaded: Vec<Option<Surfaces>>) -> Self {
        self.shaded = shaded;
        self
    }

    /// What the game's own passes draw: each part's surfaces raised as the part is and with the texture it is worn with for their diffuse, at rest.
    pub fn shaded_surfaces(&self) -> Vec<(Surface, Material)> {
        let mut drawn = Vec::new();
        for (part, surfaces) in self.parts.iter().zip(&self.shaded) {
            let Some(surfaces) = surfaces else {
                continue;
            };
            let diffuse = part.diffuse.as_ref().map(|d| {
                let mut image = Image {
                    width: d.width,
                    height: d.height,
                    rgba: d.rgba.clone(),
                    srgb: d.srgb,
                };
                tinted(&mut image, part.tint);
                image
            });
            for (surface, material) in surfaces.iter() {
                let (mut surface, mut material) = (surface.clone(), material.clone());
                for position in &mut surface.positions {
                    position[2] += part.lift;
                }
                if let Some(image) = &diffuse {
                    material
                        .textures
                        .insert("DiffuseSampler".to_string(), image.clone());
                }
                drawn.push((surface, material));
            }
        }
        drawn
    }

    /// What the debug views show of mesh `geometry` of part `part`, none for a mesh made without any.
    pub fn aux(&self, part: usize, geometry: usize) -> Option<&Aux> {
        self.aux.get(part)?.get(geometry)?.as_deref()
    }

    /// What every part drawn costs.
    pub fn drawn(&self) -> Counts {
        let mut total = Counts::default();
        for part in &self.parts {
            total.add(Counts::of(&part.geometry));
        }
        total
    }

    /// Whether mesh `geometry` of part `part` has a skin to pose.
    pub fn is_skinned_mesh(&self, part: usize, geometry: usize) -> bool {
        (self.skins.get(part)).is_some_and(|skins| skins.get(geometry).is_some_and(Option::is_some))
    }

    /// The vertices of mesh `geometry` of part `part` with its skin at `skin`, none for a mesh the file does not skin.
    ///
    /// ⚠ The lift of the part is not applied: the bridge raises what it draws.
    pub fn posed(&self, part: usize, geometry: usize, skin: &[Mat4]) -> Option<Vec<MeshVertex>> {
        let skinning = self.skins.get(part)?.get(geometry)?.as_ref()?;
        let rest = self.parts.get(part)?.geometry.get(geometry)?;
        let (mut positions, mut normals) = (Vec::new(), Vec::new());
        skinning.skin_into(skin, &mut positions, &mut normals);
        Some(
            rest.vertices
                .iter()
                .zip(positions.iter().zip(&normals))
                .map(|(vertex, (position, normal))| MeshVertex {
                    position: *position,
                    normal: *normal,
                    ..*vertex
                })
                .collect(),
        )
    }
}

/// `image` with the light of each colour channel multiplied by `tint`, where an image that states sRGB is multiplied in linear light, as a vertex colour multiplies the texture it is drawn with; the alpha is left alone.
fn tinted(image: &mut Image, tint: [f32; 3]) {
    if tint == [1.0; 3] {
        return;
    }
    let to_linear = |byte: u8| {
        let c = f32::from(byte) / 255.0;
        if !image.srgb {
            c
        } else if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let srgb = image.srgb;
    let to_byte = |light: f32| {
        let c = light.clamp(0.0, 1.0);
        let c = if !srgb {
            c
        } else if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round() as u8
    };
    // A byte has 256 values, so each channel's result is worked out once and not for every pixel
    let table: Vec<[u8; 256]> = tint
        .iter()
        .map(|by| std::array::from_fn(|byte| to_byte(to_linear(byte as u8) * by)))
        .collect();
    for pixel in image.rgba.as_chunks_mut::<4>().0 {
        for (byte, lookup) in pixel.iter_mut().zip(&table) {
            *byte = lookup[usize::from(*byte)];
        }
    }
}

/// The sphere that holds every part, lifted as it is drawn; a unit sphere at the origin where there is nothing.
fn bounds(parts: &[ScenePart]) -> (Vec3, f32) {
    let mut least = Vec3::splat(f32::MAX);
    let mut most = Vec3::splat(f32::MIN);
    for part in parts {
        for vertex in part.geometry.iter().flat_map(|g| &g.vertices) {
            let at = Vec3::from(vertex.position) + Vec3::Z * part.lift;
            least = least.min(at);
            most = most.max(at);
        }
    }
    if least.x > most.x {
        return (Vec3::ZERO, 1.0);
    }
    let centre = (least + most) / 2.0;
    (centre, ((most - least).length() / 2.0).max(0.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(at: [f32; 3], lift: f32) -> ScenePart {
        ScenePart {
            geometry: Arc::new(vec![Geometry {
                vertices: vec![MeshVertex::new(at, [0.0, 0.0, 1.0], [0.0, 0.0])],
                indices: vec![0],
            }]),
            diffuse: None,
            tint: [1.0; 3],
            lift,
            garment: None,
        }
    }

    /// A triangle of `count` triangles' worth of indices over `count + 2` vertices on the x axis.
    fn strip(count: usize) -> Geometry {
        Geometry {
            vertices: (0..count + 2)
                .map(|i| MeshVertex::new([i as f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0]))
                .collect(),
            indices: (0..count as u32).flat_map(|i| [i, i + 1, i + 2]).collect(),
        }
    }

    fn model(counts: [usize; LEVELS]) -> Model {
        Model::new(counts.map(|n| match n {
            0 => Vec::new(),
            n => vec![(strip(n), None)],
        }))
    }

    #[test]
    fn a_model_has_the_levels_it_holds_and_a_level_it_lacks_is_drawn_as_its_first() {
        let model = model([10, 0, 4, 0]);
        assert!(model.holds(0) && !model.holds(1) && model.holds(2));
        let tris = |lod| {
            model
                .at(lod)
                .map(|l| (l.lod, l.geometry[0].indices.len() / 3))
        };
        assert_eq!(tris(0), Some((0, 10)));
        assert_eq!(tris(2), Some((2, 4)));
        assert_eq!(tris(1), Some((0, 10)), "no medium level: the high one");
        assert_eq!(tris(3), Some((0, 10)));
        assert!(Model::new(Default::default()).at(0).is_none());
    }

    #[test]
    fn counts_are_the_vertices_and_triangles_of_what_is_drawn() {
        let scene = DressedScene::new(
            1,
            vec![
                ScenePart {
                    geometry: Arc::new(vec![strip(3), strip(2)]),
                    ..part([0.0; 3], 0.0)
                },
                part([0.0; 3], 0.0),
            ],
        );
        let drawn = scene.drawn();
        assert_eq!(drawn.triangles, 5);
        assert_eq!(drawn.vertices, 5 + 4 + 1);
        assert_eq!(drawn.parts, 2);
    }

    #[test]
    fn a_skinned_mesh_is_posed_by_the_skin_matrices_and_a_rigid_one_is_not() {
        use or_anim::skin::VertexInfluences;
        let mut moving = strip(1);
        moving.vertices.truncate(3);
        let skin = SkinnedMesh {
            influences: vec![VertexInfluences::rigid(1); 3],
            bind_positions: moving.vertices.iter().map(|v| v.position).collect(),
            bind_normals: moving.vertices.iter().map(|v| v.normal).collect(),
            ..SkinnedMesh::default()
        };
        let scene = DressedScene::new(
            1,
            vec![ScenePart {
                geometry: Arc::new(vec![moving, strip(1)]),
                ..part([0.0; 3], 0.0)
            }],
        )
        .with_skins(vec![vec![Some(Arc::new(skin)), None]]);
        let up = glam::Mat4::from_translation(Vec3::new(0.0, 0.0, 2.0));
        let skin_matrices = [glam::Mat4::IDENTITY, up];
        let posed = scene
            .posed(0, 0, &skin_matrices)
            .expect("the first mesh is skinned");
        assert_eq!(posed[2].position, [2.0, 0.0, 2.0]);
        assert!(
            scene.posed(0, 1, &skin_matrices).is_none(),
            "a mesh with no skin stays"
        );
        assert!(scene.posed(1, 0, &skin_matrices).is_none(), "no such part");
    }

    #[test]
    fn the_sphere_holds_the_parts_where_they_are_drawn_and_a_scene_of_nothing_is_a_unit() {
        assert_eq!(DressedScene::new(0, Vec::new()).radius, 1.0);
        let scene = DressedScene::new(
            1,
            vec![part([0.0, 0.0, 0.0], 0.0), part([0.0, 0.0, 1.0], 0.5)],
        );
        // One part is raised half a metre, so the bottom is 0 and the top 1.5.
        assert_eq!(scene.centre, Vec3::new(0.0, 0.0, 0.75));
        assert!((scene.radius - 0.75).abs() < 1e-6);
    }
}

#[cfg(test)]
mod shaded_tests {
    use super::*;
    use or_renderer::game_shaders::mesh::{Image, Material, Surface};

    fn surface() -> (Surface, Material) {
        let surface = Surface {
            positions: vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]],
            ..Surface::default()
        };
        (surface, Material::default())
    }

    fn texture(id: u64) -> Arc<Diffuse> {
        Arc::new(Diffuse {
            id,
            width: 1,
            height: 1,
            rgba: vec![10, 20, 30, 255],
            srgb: true,
        })
    }

    #[test]
    fn a_part_is_drawn_by_the_game_raised_and_with_the_texture_it_is_worn_with() {
        let part = |lift: f32, diffuse: Option<Arc<Diffuse>>| ScenePart {
            geometry: Arc::new(Vec::new()),
            diffuse,
            tint: [1.0; 3],
            lift,
            garment: None,
        };
        let scene = DressedScene::new(
            1,
            vec![
                part(0.5, Some(texture(7))),
                part(0.0, None),
                part(0.0, None),
            ],
        )
        .with_shaded(vec![
            Some(Arc::new(vec![surface()])),
            Some(Arc::new(vec![surface()])),
            None,
        ]);
        let drawn = scene.shaded_surfaces();
        assert_eq!(drawn.len(), 2, "a part with no surfaces draws none");
        assert_eq!(drawn[0].0.positions[0], [0.0, 0.0, 1.5], "raised");
        let Image { rgba, srgb, .. } = &drawn[0].1.textures["DiffuseSampler"];
        assert_eq!((rgba.as_slice(), *srgb), (&[10, 20, 30, 255][..], true));
        assert!(
            drawn[1].1.textures.is_empty(),
            "a part with no texture keeps what its file names"
        );
        assert_eq!(drawn[1].0.positions[0], [0.0, 0.0, 1.0]);
    }
}

#[cfg(test)]
mod held_tests {
    use super::*;

    #[test]
    fn a_prop_is_held_to_its_bone_by_a_full_weight_so_that_a_pose_carries_it() {
        let mut surfaces = vec![(
            Surface {
                positions: vec![[0.0; 3]; 3],
                ..Surface::default()
            },
            Material::default(),
        )];
        hold_to_bone(&mut surfaces, 41);
        let surface = &surfaces[0].0;
        assert_eq!(surface.joints, vec![[41, 0, 0, 0]; 3]);
        assert_eq!(surface.weights, vec![[1.0, 0.0, 0.0, 0.0]; 3]);
    }
}

#[cfg(test)]
mod tint_tests {
    use super::*;

    fn linear(byte: u8) -> f32 {
        let c = f32::from(byte) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    #[test]
    fn a_tint_multiplies_the_light_of_a_texture_and_leaves_its_alpha_and_a_white_tint_alone() {
        let part = |tint: [f32; 3]| ScenePart {
            geometry: Arc::new(Vec::new()),
            diffuse: Some(Arc::new(Diffuse {
                id: 1,
                width: 1,
                height: 1,
                rgba: vec![200, 200, 200, 128],
                srgb: true,
            })),
            tint,
            lift: 0.0,
            garment: None,
        };
        let surfaces = vec![Some(Arc::new(vec![(
            Surface::default(),
            Material::default(),
        )]))];
        let drawn = |tint: [f32; 3]| {
            DressedScene::new(1, vec![part(tint)])
                .with_shaded(surfaces.clone())
                .shaded_surfaces()
                .remove(0)
                .1
                .textures["DiffuseSampler"]
                .rgba
                .clone()
        };
        assert_eq!(drawn([1.0; 3]), vec![200, 200, 200, 128], "white leaves it");
        let half = drawn([0.5, 1.0, 0.0]);
        assert_eq!((half[1], half[2], half[3]), (200, 0, 128));
        let ratio = linear(half[0]) / linear(200);
        assert!(
            (ratio - 0.5).abs() < 0.02,
            "half the light, not half the byte: {ratio}"
        );
    }
}
