//! A garment of the Wardrobe's pack read and drawn by itself, for a picture of it: a prop placed at its bone of the ped's skeleton, and the surfaces the game's own passes draw it with.

use std::path::PathBuf;
use std::sync::Arc;

use or_renderer::game_shaders::mesh::{Material, Surface};
use or_wardrobe::Ped;

use super::{in_pack, pack_bytes, prop_bones, read_one, Base, Kind, Loaded, Need, Stamp};
use crate::render::dressed::{DressedScene, ScenePart};
use crate::state::textures::Texture;
use crate::state::{AppState, DocPayload, DocumentKind};

impl AppState {
    /// Starts reading, on a thread, where each bone a prop is worn at is for both freemode peds: the one thing a picture of a prop needs of the install.
    #[allow(clippy::type_complexity)]
    pub(crate) fn start_prop_bones(
        &self,
    ) -> Result<
        std::sync::mpsc::Receiver<
            Result<std::collections::BTreeMap<(Ped, &'static str), glam::Mat4>, String>,
        >,
        String,
    > {
        let root = (self.prefs.active_game_root().cloned())
            .ok_or_else(|| tr!("no game install is chosen").to_string())?;
        let (crypto, mount) = (self.keys.provider(), self.prefs.mount_options());
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(prop_bones(root, crypto, mount));
        });
        Ok(receiver)
    }

    /// Garment `id` of the Wardrobe's pack as the surfaces and materials the game's own passes draw, with the texture of `variation` for its diffuse and `beside` for every other texture it names; a prop is placed at its bone as [`Self::garment_alone`] places it.
    pub fn garment_surfaces(
        &self,
        id: &str,
        variation: usize,
        beside: &or_renderer::game_shaders::drawable::Textures,
    ) -> Result<Vec<(Surface, Material)>, String> {
        let pack = self.wardrobe_pack().ok_or(tr!("no clothing is open"))?;
        let read = pack
            .read
            .as_ref()
            .ok()
            .ok_or(tr!("the clothing cannot be read"))?;
        let garment = read.wardrobe.garment(id).ok_or(tr!("is not in the pack"))?;
        let model = garment.model.as_deref().ok_or(tr!("has no model"))?;
        let path = in_pack(&pack.folder, model)
            .ok_or_else(|| tr!("{model} leaves the pack", model = model))?;
        let (bytes, name) = pack_bytes(&path)?;
        let crypto = self.keys.provider();
        let DocPayload::Resource(asset) = crate::state::document_io::parse_payload(
            &name,
            DocumentKind::Ydd,
            &bytes,
            Arc::clone(&crypto),
        )?
        else {
            return Err(tr!("{model} is no model", model = model));
        };
        // ⚠ In the colour space the texture states, as a prop's is given: the preview's own copy is
        // marked sRGB for its shading, and the game's passes, which convert it themselves, drew it twice as dark.
        let diffuse = match (garment.variations.get(variation)).and_then(|v| v.texture.as_deref()) {
            Some(texture) => {
                let path = in_pack(&pack.folder, texture)
                    .ok_or_else(|| tr!("{texture} leaves the pack", texture = texture))?;
                let read = self.dictionary_textures(std::slice::from_ref(&path));
                Some(
                    read.into_values()
                        .next()
                        .ok_or_else(|| tr!("{texture} holds no texture", texture = texture))?,
                )
            }
            None => None,
        };
        // ⚠ A variation's dictionary is the diffuse whatever its texture is named: a pack's names are stale.
        let mut surfaces =
            crate::render::game_scene::surfaces_of_asset(&asset, |parameter, name| match parameter
                .eq_ignore_ascii_case("DiffuseSampler")
            {
                true => diffuse.clone(),
                false => beside.get(name).cloned(),
            })
            .ok_or_else(|| tr!("{model} holds no drawable", model = model))?;
        if let Some(or_wardrobe::Slot::Prop(prop)) = garment.slot {
            let at = self.ped_bone(garment.ped.unwrap_or(Ped::Male), prop.bone())?;
            crate::render::game_scene::carried(&mut surfaces, at);
        }
        if let Some(or_wardrobe::Slot::Component(
            or_wardrobe::Component::Lowr
            | or_wardrobe::Component::Uppr
            | or_wardrobe::Component::Hand,
        )) = garment.slot
        {
            for (_, material) in &mut surfaces {
                crate::photo::under_skin(material);
            }
        }
        Ok(surfaces)
    }

    /// Every texture of the dictionaries at `files`, by lower-case name, at its first level; a file that does not read is passed over.
    pub fn dictionary_textures(
        &self,
        files: &[PathBuf],
    ) -> or_renderer::game_shaders::drawable::Textures {
        use or_renderer::game_shaders::drawable::first_level;
        let crypto = self.keys.provider();
        let mut found = or_renderer::game_shaders::drawable::Textures::new();
        for file in files {
            let Ok(bytes) = std::fs::read(file) else {
                continue;
            };
            let payload = crate::state::document_io::parse_payload(
                file,
                DocumentKind::Ytd,
                &bytes,
                Arc::clone(&crypto),
            );
            let Ok(DocPayload::Resource(asset)) = payload else {
                continue;
            };
            let textures: Vec<Texture> = match &asset {
                or_io::Asset::TextureDict(dict) => (dict.entries.iter().flatten())
                    .map(|t| Texture::Owned(std::borrow::Cow::Borrowed(&**t)))
                    .collect(),
                or_io::Asset::TextureDictGen9(dict) => (dict.entries.iter().flatten())
                    .map(|t| Texture::Enhanced(std::borrow::Cow::Borrowed(&**t)))
                    .collect(),
                _ => Vec::new(),
            };
            for texture in textures {
                if let Some(image) = first_level(&texture) {
                    found.insert(texture.name().to_ascii_lowercase(), image);
                }
            }
        }
        found
    }

    /// Garment `id` of the Wardrobe's pack alone, with the texture of `variation`, read now; a prop is placed at its bone of the ped at rest.
    ///
    /// ⚠ A prop is stored in the axes of the bone it is worn at, so it is placed only once the install is mounted and
    /// its skeleton read (`props`).
    pub fn garment_alone(&self, id: &str, variation: usize) -> Result<DressedScene, String> {
        self.garment_alone_as(None, id, variation)
    }

    /// [`Self::garment_alone`] for `then`, the garment as an earlier version of the pack held it, in place of the one the pack holds now; its files are read from the pack's folder.
    pub fn garment_alone_as(
        &self,
        then: Option<&or_wardrobe::Garment>,
        id: &str,
        variation: usize,
    ) -> Result<DressedScene, String> {
        let pack = self.wardrobe_pack().ok_or(tr!("no clothing is open"))?;
        let read = pack
            .read
            .as_ref()
            .ok()
            .ok_or(tr!("the clothing cannot be read"))?;
        let garment = match then {
            Some(garment) => garment,
            None => read.wardrobe.garment(id).ok_or(tr!("is not in the pack"))?,
        };
        let crypto = self.keys.provider();
        let file = |name: &str, kind| {
            let path = in_pack(&pack.folder, name)
                .ok_or_else(|| tr!("{name} leaves the pack", name = name))?;
            let unseen = Stamp {
                len: 0,
                modified: None,
            };
            match read_one(&Need::Pack(kind, path, unseen), None, &crypto) {
                Loaded::Failed(why) => Err(format!("{name}: {why}")),
                loaded => Ok(loaded),
            }
        };
        let model = garment.model.as_deref().ok_or(tr!("has no model"))?;
        let Loaded::Model(held) = file(model, Kind::Model)? else {
            return Err(tr!("{model} is no model", model = model));
        };
        let geometry = held
            .at(0)
            .map(|level| Arc::clone(level.geometry))
            .ok_or_else(|| tr!("{model} holds no geometry", model = model))?;
        let texture = (garment.variations.get(variation)).and_then(|v| v.texture.as_deref());
        let diffuse = match texture.map(|t| file(t, Kind::Texture)).transpose()? {
            Some(Loaded::Texture(diffuse)) => Some(diffuse),
            _ => None,
        };
        let geometry = match garment.slot {
            Some(or_wardrobe::Slot::Prop(prop)) => {
                let ped = garment.ped.unwrap_or(Ped::Male);
                let at = self.ped_bone(ped, prop.bone())?;
                Arc::new(super::props::placed(&geometry, at))
            }
            _ => geometry,
        };
        let part = ScenePart {
            geometry,
            diffuse,
            tint: [1.0; 3],
            lift: 0.0,
            garment: Some(id.to_string()),
        };
        Ok(DressedScene::new(0, vec![part]))
    }

    /// Where the bone named `bone` of `ped`'s skeleton is at rest.
    fn ped_bone(&self, ped: Ped, bone: &str) -> Result<glam::Mat4, String> {
        let Base::Ready(mounted) = &self.workbench.wardrobe.dressed.base else {
            return Err(
                tr!("no game install is chosen, so the ped's skeleton is not read").to_string(),
            );
        };
        let skeleton = match mounted.skeletons.get(&ped) {
            Some(Ok(skeleton)) => skeleton,
            Some(Err(why)) => return Err(why.clone()),
            None => return Err(tr!("{model} has no skeleton", model = ped.model())),
        };
        (skeleton.bones.iter())
            .position(|b| b.name.eq_ignore_ascii_case(bone))
            .and_then(|at| skeleton.bind_world.get(at).copied())
            .ok_or_else(|| {
                tr!(
                    "{model} has no bone {bone}",
                    bone = bone,
                    model = ped.model()
                )
            })
    }
}
