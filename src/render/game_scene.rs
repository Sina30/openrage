//! The open drawable as the surfaces and materials the game's own passes draw, with the textures the app resolves for it.

use or_interchange::extract::Extracted;
use or_interchange::{drawable_scene, ExtractOptions, LodSelection, Scene};
use or_io::gta5::RmcDrawable;
use or_renderer::game_shaders::drawable::{self, first_level, surfaces_of, Textures};
use or_renderer::game_shaders::mesh::{Image, Material, Surface};

use std::collections::HashMap;

use crate::state::{texture_resolution, AppState, DocPayload, DocumentId, MeshSource};
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::ydd_selected_entry_index;

/// The most detailed level of `drawable` as a scene; its own textures are left out, since the app finds each texture itself.
fn scene_of<T: Extracted>(drawable: &RmcDrawable<T>) -> Scene {
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: false,
    };
    drawable_scene("drawable", drawable, &options)
}

/// The scene of the first drawable of `asset`, or of entry `entry` where it is a dictionary; `None` for an asset that shows no drawable there.
fn scene_of_asset_entry(asset: &or_io::Asset, entry: Option<usize>) -> Option<Scene> {
    Some(match asset {
        or_io::Asset::Drawable(d) => scene_of(&d.base),
        or_io::Asset::DrawableGen9(d) => scene_of(&d.base),
        or_io::Asset::Fragment(f) => scene_of(&f.common_drawable.as_deref()?.base),
        or_io::Asset::FragmentGen9(f) => scene_of(&f.common_drawable.as_deref()?.base),
        or_io::Asset::DrawableDict(dict) => scene_of(&dict.entries.get(entry?)?.as_deref()?.base),
        or_io::Asset::DrawableDictGen9(dict) => {
            scene_of(&dict.entries.get(entry?)?.as_deref()?.base)
        }
        _ => return None,
    })
}

/// The scene of the drawable the active document shows, and the selection that finds its textures; `None` for a document with no drawable.
fn scene_of_document(state: &AppState) -> Option<(Scene, Option<u64>)> {
    let doc = state.active_document()?;
    let selection = state.selection.selected(doc.id);
    let DocPayload::Resource(asset) = &doc.payload else {
        return None;
    };
    let entry = selection.and_then(|s| ydd_selected_entry_index(&doc.payload, s));
    Some((scene_of_asset_entry(asset, entry)?, selection))
}

/// The tag of each bone of the skeleton a scene's drawable ships, by bone index; none where it ships none.
fn skeleton_tags(scene: &Scene) -> Vec<u16> {
    scene
        .objects
        .iter()
        .find(|object| !object.bones.is_empty())
        .map(|object| object.bones.iter().map(|bone| bone.tag).collect())
        .unwrap_or_default()
}

/// Bind the skinned `surfaces` to the bones of the skeleton the pose is of: a bone of the drawable's own skeleton, named by `own_tags`, becomes the bone `bound_index_of` finds for its tag, and keeps its index where it finds none.
///
/// A drawable that ships no skeleton is weighted by the indices of the bound one already, and is left as it is.
fn rebind_joints(
    surfaces: &mut [(Surface, Material)],
    own_tags: &[u16],
    bound_index_of: impl Fn(u16) -> Option<u16>,
) {
    if own_tags.is_empty() {
        return;
    }
    for (surface, _) in surfaces {
        for joint in surface.joints.iter_mut().flatten() {
            if let Some(bound) = own_tags
                .get(usize::from(*joint))
                .and_then(|tag| bound_index_of(*tag))
            {
                *joint = bound;
            }
        }
    }
}

/// What a decoded texture was decoded from: the dictionary that holds it, how many times that has changed, the selection that found it, how many recooks had been read, and its name.
type DecodeKey = (DocumentId, u64, Option<u64>, u64, String);

/// The first level of each texture the last scene was made from, kept to the next, so that an edit that leaves a texture alone does not decode it again.
#[derive(Default)]
pub(crate) struct DecodedTextures {
    made: HashMap<DecodeKey, (Option<Image>, bool)>,
}

impl DecodedTextures {
    /// The texture of `key`, decoded by `decode` at its first ask; one that will not decode is not tried again.
    fn get(&mut self, key: DecodeKey, decode: impl FnOnce() -> Option<Image>) -> Option<Image> {
        let (image, used) = self.made.entry(key).or_insert_with(|| (decode(), true));
        *used = true;
        image.clone()
    }

    /// Start a scene, which has named nothing yet.
    fn begin(&mut self) {
        for (_, used) in self.made.values_mut() {
            *used = false;
        }
    }

    /// End a scene: what it did not name is let go.
    fn end(&mut self) {
        self.made.retain(|_, (_, used)| *used);
    }
}

/// Every surface of the active document's drawable with its material, each texture it names found as the app finds it for a mesh; `None` where the document shows no drawable.
pub(crate) fn surfaces_of_active_document(state: &AppState) -> Option<Vec<(Surface, Material)>> {
    surfaces_of_document(state, &mut DecodedTextures::default(), false)
}

/// The wheels the active document's vehicle is drawn with, each with the matrix that puts it at its bone; none for a document that is no vehicle.
fn wheels_of_document(state: &AppState) -> Vec<(Scene, glam::Mat4)> {
    let Some(doc) = state.active_document() else {
        return Vec::new();
    };
    let DocPayload::Resource(asset) = &doc.payload else {
        return Vec::new();
    };
    let options = ExtractOptions {
        lods: LodSelection::Highest,
        textures: false,
    };
    match asset {
        or_io::Asset::Fragment(f) => drawable::fragment_wheels(&f.base, &options),
        or_io::Asset::FragmentGen9(f) => drawable::fragment_wheels(&f.base, &options),
        _ => Vec::new(),
    }
}

/// [`surfaces_of_active_document`], taking the textures `decoded` holds from the last scene and leaving it the ones this scene names, and with a vehicle's wheels at its wheel bones when `wheels` asks.
fn surfaces_of_document(
    state: &AppState,
    decoded: &mut DecodedTextures,
    wheels: bool,
) -> Option<Vec<(Surface, Material)>> {
    let (scene, selection) = scene_of_document(state)?;
    let wheels = if wheels {
        wheels_of_document(state)
    } else {
        Vec::new()
    };
    decoded.begin();
    let surfaces = surfaces_with_wheels(state, decoded, &scene, None, selection, &wheels);
    decoded.end();
    Some(surfaces)
}

/// The surfaces of `scene` and of the `wheels` carried to their bones, each texture found as the app finds it for what `from` names, and the skinned ones bound to the skeleton the pose is of.
fn surfaces_with_wheels(
    state: &AppState,
    decoded: &mut DecodedTextures,
    scene: &Scene,
    from: Option<MeshSource>,
    selection: Option<u64>,
    wheels: &[(Scene, glam::Mat4)],
) -> Vec<(Surface, Material)> {
    let mut found = Textures::new();
    let materials = scene
        .materials
        .iter()
        .chain(wheels.iter().flat_map(|(wheel, _)| &wheel.materials));
    for binding in materials.flat_map(|m| &m.textures) {
        let name = binding.texture.to_ascii_lowercase();
        if name.is_empty() || found.contains_key(&name) {
            continue;
        }
        let Some(resolved) = texture_resolution::resolve(state, from, selection, &binding.texture)
        else {
            continue;
        };
        let source = resolved.origin.document;
        let epoch = state.document(source).map_or(0, |d| d.payload_epoch());
        let key = (source, epoch, selection, state.recooked, name.clone());
        if let Some(image) = decoded.get(key, || first_level(&resolved.texture)) {
            found.insert(name, image);
        }
    }
    let mut surfaces = surfaces_of(scene, &found);
    let bound = state.anim.binding.as_ref().map(|binding| &binding.skeleton);
    rebind_joints(&mut surfaces, &skeleton_tags(scene), |tag| {
        bound.and_then(|skeleton| skeleton.index_for_tag(tag))
    });
    for (wheel, at) in wheels {
        let mut wheel = surfaces_of(wheel, &found);
        drawable::carried(&mut wheel, *at);
        surfaces.extend(wheel);
    }
    surfaces
}

/// The index of each entry of a dictionary that holds a drawable.
fn held<T>(entries: &[Option<Box<T>>]) -> Vec<Option<usize>> {
    (0..entries.len())
        .filter(|at| entries[*at].is_some())
        .map(Some)
        .collect()
}

/// Every surface of the drawables the Animation preview draws, each source with the textures its own documents give it; `None` where it draws none.
fn surfaces_of_sources(
    state: &AppState,
    decoded: &mut DecodedTextures,
) -> Option<Vec<(Surface, Material)>> {
    let mut surfaces = Vec::new();
    decoded.begin();
    for (document, entry) in state.preview_scene.drawn_sources() {
        let Some(doc) = state.document(document) else {
            continue;
        };
        let DocPayload::Resource(asset) = &doc.payload else {
            continue;
        };
        // A dictionary named whole is each of its drawables, which share the textures the source finds
        let entries = match (asset, entry) {
            (or_io::Asset::DrawableDict(dict), None) => held(&dict.entries),
            (or_io::Asset::DrawableDictGen9(dict), None) => held(&dict.entries),
            _ => vec![entry],
        };
        let from = Some(MeshSource { document, entry });
        for at in entries {
            let Some(scene) = scene_of_asset_entry(asset, at) else {
                continue;
            };
            surfaces.extend(surfaces_with_wheels(
                state,
                decoded,
                &scene,
                from,
                None,
                &[],
            ));
        }
    }
    decoded.end();
    (!surfaces.is_empty()).then_some(surfaces)
}

/// What the viewport shows for the active document: its drawable, with a ground under it where that is asked for, or the ground under an effect; in the Animation workspace, the drawables the preview draws.
pub(crate) fn surfaces_shown(
    state: &AppState,
    decoded: &mut DecodedTextures,
) -> Option<Vec<(Surface, Material)>> {
    let shown = if state.active_workspace == Workspace::Animation {
        surfaces_of_sources(state, decoded)
    } else {
        surfaces_of_document(state, decoded, true)
    };
    shown
        .map(|mut surfaces| {
            if state.drivers.world_background.ground {
                drawable::lay_ground_under(&mut surfaces);
            }
            surfaces
        })
        .or_else(|| {
            let effect = matches!(
                state.active_document().map(|doc| &doc.payload),
                Some(DocPayload::Resource(or_io::Asset::Particles(_)))
            );
            (effect && state.drivers.world_background.ground).then(ground)
        })
}

/// A flat ground of a mild grey, sixty metres across, for the light of an effect to land on.
fn ground() -> Vec<(Surface, Material)> {
    vec![drawable::ground(glam::Vec3::ZERO, 30.0)]
}

/// Every surface of the first drawable of `asset` with its material, each texture found by `texture`, which is asked with the parameter that binds it and the texture's lower-case name; `None` for an asset that holds no drawable.
pub(crate) fn surfaces_of_asset(
    asset: &or_io::Asset,
    mut texture: impl FnMut(&str, &str) -> Option<or_renderer::game_shaders::mesh::Image>,
) -> Option<Vec<(Surface, Material)>> {
    let scene = scene_of_asset_entry(asset, Some(first_entry(asset)))?;
    let mut found = Textures::new();
    for binding in scene.materials.iter().flat_map(|m| &m.textures) {
        let name = binding.texture.to_ascii_lowercase();
        if name.is_empty() || found.contains_key(&name) {
            continue;
        }
        if let Some(image) = texture(&binding.parameter, &name) {
            found.insert(name, image);
        }
    }
    Some(surfaces_of(&scene, &found))
}

/// The index of the first entry of `asset` that holds a drawable, where it is a dictionary; nought for any other.
fn first_entry(asset: &or_io::Asset) -> usize {
    match asset {
        or_io::Asset::DrawableDict(dict) => dict.entries.iter().position(Option::is_some),
        or_io::Asset::DrawableDictGen9(dict) => dict.entries.iter().position(Option::is_some),
        _ => None,
    }
    .unwrap_or(0)
}

pub(crate) use or_renderer::game_shaders::drawable::carried;

#[cfg(test)]
mod tests;
