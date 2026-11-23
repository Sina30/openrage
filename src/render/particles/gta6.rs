//! A scanned GTA VI list's sprites, drawn from the textures their keys resolve to, each rule's
//! atlas cut into the frame grid its texture's size gives, and its model rules' textures.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use or_io::codec::texture::GrcTextureFormat;
use or_renderer::game_shaders::drawable::first_level;
use or_renderer::game_shaders::mesh::Image;

use super::{infer_uv_atlas, ParticleTextureCache};
use crate::render::gta6_drawable::{meshes_at, srgb_key};
use crate::state::gta6_particle_models::{found_count, found_drawable};
use crate::state::gta6_particles::sprites::{self, Sprite, SHAPE_SLOT};
use crate::state::gta6_resource::{drawable_lines_stamp, texture_name};
use crate::state::{texture_resolution, AppState, DocumentId};

/// Each image by the name it is cached under; `None` for a name no texture was found for.
type Images = HashMap<String, Option<Arc<Image>>>;

/// The slot of a sprite with no alpha, drawn opaque.
const OPAQUE_SLOT: usize = 5;
/// The most times a grid's smallest cell is repeated along a side.
const GRID_STEPS: u32 = 64;
/// A colour ramp's height in texels.
const RAMP_HEIGHT: u16 = 4;

/// What a rule draws: the name its texture is cached by, and its atlas's rows and columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Drawn {
    pub texture: String,
    pub rows: u32,
    pub cols: u32,
}

/// Each rule's sprite and each model drawable's surface textures, and what they were resolved at.
#[derive(Default)]
pub(crate) struct Gta6Sprites {
    stamp: Option<(u64, usize)>,
    by_rule: HashMap<String, Drawn>,
    /// The texture each surface of a model drawable is drawn with, by the drawable's name hash.
    models: HashMap<u32, Vec<Option<String>>>,
}

/// What the rule `name` of a scanned GTA VI list draws; `None` for any other rule.
pub(crate) fn drawn(cache: &ParticleTextureCache, name: &str) -> Option<Drawn> {
    cache.gta6.by_rule.get(name).cloned()
}

/// The texture the model view picks for surface `surface` of the found drawable `drawable`.
pub(crate) fn model_texture(
    cache: &ParticleTextureCache,
    drawable: u32,
    surface: usize,
) -> Option<String> {
    cache
        .gta6
        .models
        .get(&drawable)?
        .get(surface)
        .cloned()
        .flatten()
}

/// Resolve the sprites and the model textures of `doc`, where it is a scanned GTA VI list, into
/// `cache`, once each time the documents and names they resolve against change.
pub(crate) fn prime(cache: &mut ParticleTextureCache, state: &AppState, doc: DocumentId) {
    let path = state.document(doc).and_then(|d| d.path.as_deref());
    let Some((path, kept)) = path.and_then(|p| Some((p, sprites::of(p)?))) else {
        return;
    };
    let lines = drawable_lines_stamp(state);
    let stamp = (lines, found_count());
    if cache.gta6.stamp == Some(stamp) {
        return;
    }
    // While the names stand, a drawable found before keeps its surfaces: only one found since is built.
    let earlier = match cache.gta6.stamp {
        Some((was, _)) if was == lines => std::mem::take(&mut cache.gta6.models),
        _ => HashMap::new(),
    };
    // A name no texture was found for is asked for again.
    cache.images.retain(|_, image| image.is_some());
    let mut by_rule = HashMap::new();
    for (rule, sprite) in kept.iter() {
        let mut texture = format!("{}#{}", texture_name(sprite.key), sprite.slot);
        let mut image = cached(&mut cache.images, &texture, || decoded(state, sprite));
        if let (Some(shape), Some(ramp)) = (image.clone(), sprite.ramp) {
            let coloured = format!("{texture}#{}", texture_name(ramp));
            let tinted = cached(&mut cache.images, &coloured, || ramped(state, &shape, ramp));
            // Grey until a pack holding the ramp is open.
            if tinted.is_some() {
                (texture, image) = (coloured, tinted);
            }
        }
        let size = image.as_deref().map(|i| (i.width, i.height));
        let (rows, cols) = grid(rule, sprite.frames, size);
        let drawn = Drawn {
            texture,
            rows,
            cols,
        };
        by_rule.insert(rule.clone(), drawn);
    }
    let models = model_textures(&mut cache.images, state, (doc, path), earlier);
    cache.gta6 = Gta6Sprites {
        stamp: Some(stamp),
        by_rule,
        models,
    };
}

/// The image cached as `name`, made and cached by `make` the first time it is asked for.
fn cached(
    images: &mut Images,
    name: &str,
    make: impl FnOnce() -> Option<Image>,
) -> Option<Arc<Image>> {
    if let Some(image) = images.get(name) {
        return image.clone();
    }
    let image = make().map(Arc::new);
    images.insert(name.to_string(), image.clone());
    image
}

/// The texture each surface of every drawable a search found that the list at `path` names is
/// drawn with, as the model view picks it, each decoded once into `images`; a drawable `earlier`
/// holds keeps what it holds.
fn model_textures(
    images: &mut Images,
    state: &AppState,
    (doc, path): (DocumentId, &Path),
    earlier: HashMap<u32, Vec<Option<String>>>,
) -> HashMap<u32, Vec<Option<String>>> {
    let Some(named) = crate::state::gta6_particle_models::kept(path) else {
        return HashMap::new();
    };
    let srgb = |key: u64| srgb_key(state, doc, key);
    let mut out = earlier;
    out.retain(|hash, _| named.wanted.contains(hash));
    for &hash in &named.wanted {
        if out.contains_key(&hash) {
            continue;
        }
        let Some(drawable) = found_drawable(hash) else {
            continue;
        };
        // The surfaces the particle preview draws, in its order: those with vertices and corners.
        let names: Vec<Option<String>> = (meshes_at(&drawable, 0, None, None, &srgb).into_iter())
            .filter(|u| !u.vertices.is_empty() && !u.indices.is_empty())
            .map(|u| u.texture_name)
            .collect();
        for name in names.iter().flatten() {
            cached(images, name, || {
                first_level(&resolved(state, name)?.texture)
            });
        }
        out.insert(hash, names);
    }
    out
}

/// The texture `name` resolves to among the open packs and documents.
fn resolved<'a>(state: &'a AppState, name: &str) -> Option<texture_resolution::Resolved<'a>> {
    texture_resolution::resolve(state, None, None, name)
}

/// The first level of the texture `sprite`'s key resolves to, as its slot is drawn.
fn decoded(state: &AppState, sprite: &Sprite) -> Option<Image> {
    let found = resolved(state, &texture_name(sprite.key))?;
    let mut image = first_level(&found.texture)?;
    shown_as(&mut image.rgba, sprite.slot);
    Some(image)
}

/// The grey `shape` coloured through the ramp the key `ramp` resolves to; `None` where that
/// texture is not a ramp.
fn ramped(state: &AppState, shape: &Image, ramp: u64) -> Option<Image> {
    let found = resolved(state, &texture_name(ramp))?;
    let (format, height) = (found.texture.format(), found.texture.height());
    if !is_ramp(format, height) {
        tracing::debug!(
            ?format,
            height,
            "GTA VI slot-7 texture is not a ramp: drawn grey"
        );
        return None;
    }
    through_ramp(shape, &first_level(&found.texture)?)
}

/// Whether a slot-7 texture of `format` and `height` is a colour ramp: one row repeated over a
/// BC6H block's four.
fn is_ramp(format: GrcTextureFormat, height: u16) -> bool {
    format == GrcTextureFormat::BC6 && height == RAMP_HEIGHT
}

/// `shape`'s texels, each the colour of `ramp`'s first row at the place its value gives along the
/// row, with its value as alpha; `None` for a ramp of no texel. A ramp's rows are one row.
// ponytail: the value as the place along the row is inferred; only the shader states it.
fn through_ramp(shape: &Image, ramp: &Image) -> Option<Image> {
    let width = usize::try_from(ramp.width).ok()?;
    let row = ramp.rgba.get(..width.checked_mul(4)?)?.as_chunks::<4>().0;
    let last = row.len().checked_sub(1)?;
    let mut rgba = shape.rgba.clone();
    for texel in rgba.as_chunks_mut::<4>().0 {
        let value = texel[0];
        let [r, g, b, _] = row[usize::from(value) * last / usize::from(u8::MAX)];
        *texel = [r, g, b, value];
    }
    Some(Image {
        width: shape.width,
        height: shape.height,
        rgba,
        srgb: ramp.srgb,
    })
}

/// RGBA texels as `slot` is drawn: slot 6's one channel grey in every lane, slot 5's alpha 1.
fn shown_as(rgba: &mut [u8], slot: usize) {
    for texel in rgba.as_chunks_mut::<4>().0 {
        match slot {
            SHAPE_SLOT => {
                let value = texel[0];
                texel.fill(value);
            }
            OPAQUE_SLOT => texel[3] = u8::MAX,
            _ => return,
        }
    }
}

/// The rows and columns `frames` frames of the rule `rule` are cut into on an atlas of `size`: the
/// grid of exactly that many square cells, else GTA V's guess, which is logged.
fn grid(rule: &str, frames: u32, size: Option<(u32, u32)>) -> (u32, u32) {
    if frames <= 1 {
        return (1, 1);
    }
    match size.and_then(|(width, height)| square_cells(frames, width, height)) {
        Some((rows, cols, whole)) => {
            if !whole {
                tracing::debug!(
                    rule,
                    frames,
                    rows,
                    cols,
                    "GTA VI atlas grid guessed: square cells of a fractional pixel size"
                );
            }
            (rows, cols)
        }
        None => {
            let (rows, cols) = infer_uv_atlas(frames - 1);
            tracing::debug!(
                rule,
                frames,
                rows,
                cols,
                "GTA VI atlas grid guessed: no grid of square cells holds exactly its frames"
            );
            (rows, cols)
        }
    }
}

/// The rows and columns of exactly `frames` square cells on a `width` by `height` atlas, and
/// whether each cell is a whole number of pixels.
fn square_cells(frames: u32, width: u32, height: u32) -> Option<(u32, u32, bool)> {
    let (mut a, mut b) = (width, height);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let (across, down) = (width.checked_div(a)?, height.checked_div(a)?);
    (1..=GRID_STEPS).find_map(|k| {
        let (cols, rows) = (across.checked_mul(k)?, down.checked_mul(k)?);
        if cols.checked_mul(rows)? != frames {
            return None;
        }
        let whole = width.checked_rem(cols)? == 0 && height.checked_rem(rows)? == 0;
        Some((rows, cols, whole))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_is_the_square_cells_that_hold_exactly_the_frames_on_the_atlas() {
        assert_eq!(square_cells(4, 512, 512), Some((2, 2, true)));
        assert_eq!(square_cells(32, 2048, 1024), Some((4, 8, true)));
        assert_eq!(square_cells(36, 256, 256), Some((6, 6, false)));
        assert_eq!(square_cells(64, 2048, 1024), None);
        assert_eq!(square_cells(4, 0, 0), None);
        assert_eq!(grid("rule", 32, Some((1024, 512))), (4, 8));
        assert_eq!(grid("rule", 9, None), infer_uv_atlas(8));
        assert_eq!(grid("rule", 8, Some((256, 256))), infer_uv_atlas(7));
        assert_eq!(grid("rule", 1, None), (1, 1));
    }

    #[test]
    fn slot_6_is_drawn_grey_and_slot_5_opaque() {
        let texels = [10, 0, 0, 255, 200, 7, 9, 30];
        let mut grey = texels;
        shown_as(&mut grey, SHAPE_SLOT);
        assert_eq!(grey, [10, 10, 10, 10, 200, 200, 200, 200]);
        let mut opaque = texels;
        shown_as(&mut opaque, OPAQUE_SLOT);
        assert_eq!(opaque, [10, 0, 0, 255, 200, 7, 9, 255]);
        let mut colour = texels;
        shown_as(&mut colour, 0);
        assert_eq!(colour, texels);
    }

    #[test]
    fn a_model_rules_surface_takes_the_texture_the_model_view_picks() {
        use crate::state::gta6_particle_models::{
            found_for_tests, named_for_tests, one_triangle, Named,
        };
        use or_io::gta6::headerless::Shader;
        const HASH: u32 = 0x0D7A_B1E5;
        const KEY: u64 = 0x1234_5678_9ABC_DEF0;
        let path = Path::new("a_model_rules_surface_takes_its_texture.particles");
        let mut drawable = one_triangle([1.0; 3]);
        drawable.shaders = vec![Shader {
            name_hash: 0,
            texture_keys: vec![None, Some(KEY)],
            parameters: Vec::new(),
        }];
        found_for_tests(HASH, drawable);
        let named = Named {
            wanted: std::collections::BTreeSet::from([HASH]),
            domains: Default::default(),
        };
        named_for_tests(path, named);
        let state = AppState::new();
        let mut images = Images::new();
        let models = model_textures(&mut images, &state, (7, path), HashMap::new());
        let name = texture_name(KEY);
        assert_eq!(models.get(&HASH), Some(&vec![Some(name.clone())]));
        // No pack holds the key yet, so it is asked for again once one opens.
        assert!(matches!(images.get(&name), Some(None)));
        // A drawable built before keeps its surfaces, and one no longer named is dropped.
        let earlier = HashMap::from([(HASH, vec![None]), (HASH ^ 1, vec![None])]);
        let models = model_textures(&mut images, &state, (7, path), earlier);
        assert_eq!(models, HashMap::from([(HASH, vec![None])]));
    }

    #[test]
    fn a_ramp_is_a_bc6h_texture_four_texels_high() {
        assert!(is_ramp(GrcTextureFormat::BC6, RAMP_HEIGHT));
        assert!(!is_ramp(GrcTextureFormat::BC6, 128), "an image");
        assert!(
            !is_ramp(GrcTextureFormat::BC7, RAMP_HEIGHT),
            "another format"
        );
    }

    #[test]
    fn a_grey_shape_takes_the_colour_its_value_places_on_the_ramps_first_row() {
        let image = |width: u32, height: u32, rgba: Vec<u8>| Image {
            width,
            height,
            rgba,
            srgb: false,
        };
        // Three colours along the first row; the second row is never read.
        let mut texels = vec![1, 2, 3, 9, 40, 50, 60, 9, 70, 80, 90, 9];
        texels.extend([255; 12]);
        let ramp = image(3, 2, texels);
        let shape = image(
            3,
            1,
            vec![0, 0, 0, 0, 255, 255, 255, 255, 128, 128, 128, 128],
        );
        let coloured = through_ramp(&shape, &ramp).expect("a ramp of texels");
        // Value 128 is place 128 * 2 / 255 = 1 of the row's three.
        let expected = [1, 2, 3, 0, 70, 80, 90, 255, 40, 50, 60, 128];
        assert_eq!(coloured.rgba, expected);
        assert_eq!((coloured.width, coloured.height), (3, 1));
        assert!(through_ramp(&shape, &image(0, 4, Vec::new())).is_none());
        assert!(
            through_ramp(&shape, &image(8, 1, vec![0; 4])).is_none(),
            "a row its bytes do not hold"
        );
    }
}
