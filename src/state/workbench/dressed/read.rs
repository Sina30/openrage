//! Reading the files the dressed ped wears: the install mounted for the ped's own drawables and skeleton, a file of the pack or of the install made into a model, a texture or a tile.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_title::vfs::{build_mounted_game_with, DlcCutoff, MountedGame};
use or_title::GtaInstall;
use or_wardrobe::dress::BaseOutfit;
use or_wardrobe::Ped;
use std::collections::BTreeMap;

use super::{Kind, Loaded, Mounted, Need, Thumb, IDS, THUMB};
use crate::render::debug_channel::DebugChannel;
use crate::render::dressed::{Aux, Diffuse, Geometry, Model, LEVELS};
use crate::render::meshes::{meshes_of_asset_in, MeshUpload};
use crate::state::textures::Texture;
use crate::state::{DocPayload, DocumentKind};

/// A file as a sentence names it: its own name.
pub(super) fn describe(need: &Need) -> String {
    match need {
        Need::Pack(_, path, _) => path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
        Need::Base(_, _, file) => file.clone(),
    }
}

/// Mount the install's base game and read the variation metadata of both freemode peds from it.
pub(super) fn mount_install(
    root: PathBuf,
    crypto: Arc<dyn RpfCryptoProvider>,
    mount: or_title::MountOptions,
) -> Result<Arc<Mounted>, String> {
    let game = build_mounted_game_with(
        &GtaInstall { root },
        Arc::clone(&crypto),
        DlcCutoff::BaseGame,
        mount,
    )
    .map_err(|e| tr!("the install could not be mounted: {e}", e = e))?;
    let mut outfits = BTreeMap::new();
    let mut skeletons = BTreeMap::new();
    let mut bones = BTreeMap::new();
    for ped in Ped::ALL {
        let path = or_title::install::freemode::variation_path(ped.model());
        let outfit = game
            .vfs
            .open(&path)
            .ok_or_else(|| tr!("{model} is not in the install", model = ped.model()))
            .and_then(|bytes| {
                use or_io::decl::File;
                or_io::meta::CPedVariationInfo::from_bytes(&bytes)
                    .map(|info| BaseOutfit::of(&info))
                    .map_err(|e| {
                        tr!(
                            "{model}'s metadata does not read: {e}",
                            e = e,
                            model = ped.model()
                        )
                    })
            });
        outfits.insert(ped, outfit);
        let read = skeleton_of(&game, ped, &crypto);
        bones.insert(
            ped,
            read.as_ref().map(|r| r.1.clone()).map_err(Clone::clone),
        );
        skeletons.insert(ped, read.map(|r| r.0));
    }
    Ok(Arc::new(Mounted {
        game,
        outfits,
        skeletons,
        bones,
    }))
}

/// Where each bone a prop is worn at is, at rest, for both freemode peds, read from the install's base game: all a picture of a prop needs of the install.
pub(in crate::state::workbench) fn prop_bones(
    root: PathBuf,
    crypto: Arc<dyn RpfCryptoProvider>,
    mount: or_title::MountOptions,
) -> Result<BTreeMap<(Ped, &'static str), glam::Mat4>, String> {
    let game = build_mounted_game_with(
        &GtaInstall { root },
        Arc::clone(&crypto),
        DlcCutoff::BaseGame,
        mount,
    )
    .map_err(|e| tr!("the install could not be mounted: {e}", e = e))?;
    let mut found = BTreeMap::new();
    for ped in Ped::ALL {
        let (skeleton, _) = skeleton_of(&game, ped, &crypto)?;
        for prop in or_wardrobe::Prop::ALL {
            let bone = prop.bone();
            let at = (skeleton.bones.iter())
                .position(|b| b.name.eq_ignore_ascii_case(bone))
                .and_then(|at| skeleton.bind_world.get(at).copied())
                .ok_or_else(|| {
                    tr!(
                        "{model} has no bone {bone}",
                        bone = bone,
                        model = ped.model()
                    )
                })?;
            found.insert((ped, bone), at);
        }
    }
    Ok(found)
}

/// The skeleton of the freemode ped `ped`, from its fragment beside its folder, and its bones as a skin without a skeleton of its own names them.
fn skeleton_of(
    game: &MountedGame,
    ped: Ped,
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Result<(or_anim::Skeleton, Vec<or_interchange::Bone>), String> {
    let file = format!("{}.yft", ped.model());
    let path = format!("{}/{file}", or_title::install::freemode::PED_ARCHIVE);
    let bytes =
        (game.vfs.open(&path)).ok_or_else(|| tr!("{file} is not in the install", file = file))?;
    let payload = crate::state::document_io::parse_payload(
        Path::new(&file),
        DocumentKind::Yft,
        &bytes,
        Arc::clone(crypto),
    )?;
    let data = crate::state::bone_numbers::skeleton_of(&payload, 0)
        .ok_or_else(|| tr!("{file} holds no skeleton", file = file))?;
    Ok((
        or_anim::Skeleton::build(data),
        or_interchange::extract::bones_of(data),
    ))
}

/// The bytes of a file of the pack and the name to parse them under; a source folder that a garment names as its model is cooked first, as a build cooks it.
pub(in crate::state::workbench) fn pack_bytes(path: &Path) -> Result<(Vec<u8>, PathBuf), String> {
    if !path.is_dir() {
        let bytes = std::fs::read(path).map_err(|e| tr!("cannot be read: {e}", e = e))?;
        return Ok((bytes, path.to_path_buf()));
    }
    let (name, bytes) = or_wardrobe::facts::cooked(path)?;
    Ok((bytes, PathBuf::from(name)))
}

/// Read one file and make what it is for.
pub(super) fn read_one(
    need: &Need,
    install: Option<&Mounted>,
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Loaded {
    let (bytes, name) = match need {
        Need::Pack(_, path, _) => match pack_bytes(path) {
            Ok(read) => read,
            Err(why) => return Loaded::Failed(why),
        },
        Need::Base(_, ped, file) => {
            let Some(mounted) = install else {
                return Loaded::Failed(tr!("no game install is chosen").to_string());
            };
            match or_title::install::freemode::open(&mounted.game.vfs, ped.model(), file) {
                Some(bytes) => (bytes, PathBuf::from(file)),
                None => return Loaded::Failed(tr!("is not in the install").to_string()),
            }
        }
    };
    let kind = match need.kind() {
        Kind::Model => DocumentKind::Ydd,
        Kind::Texture | Kind::Thumb => DocumentKind::Ytd,
    };
    let payload = crate::state::document_io::parse_payload(&name, kind, &bytes, Arc::clone(crypto));
    let asset = match payload {
        Ok(DocPayload::Resource(asset)) => asset,
        Ok(_) => return Loaded::Failed(tr!("is not a file of that kind").to_string()),
        Err(why) => return Loaded::Failed(why),
    };
    match need.kind() {
        Kind::Model => {
            // The texture it is worn with is the slot's, put in when the scene is made
            let shaded = crate::render::game_scene::surfaces_of_asset(&asset, |_, _| None);
            Loaded::Model(Arc::new(model_of(&asset).with_shaded(shaded.map(Arc::new))))
        }
        Kind::Texture => match first_texture(&asset) {
            Ok(diffuse) => Loaded::Texture(Arc::new(diffuse)),
            Err(why) => Loaded::Failed(why),
        },
        Kind::Thumb => match first_texture_near(&asset, Some(THUMB)) {
            Ok((diffuse, full)) => Loaded::Thumb(Arc::new(thumb_with(&diffuse, THUMB, full))),
            Err(why) => Loaded::Failed(why),
        },
    }
}

/// The model a drawable file holds: its meshes at each level of detail with their skinning, and what the debug views of vertex colours and tangents show of each.
fn model_of(asset: &or_io::Asset) -> Model {
    let mut aux: [Vec<Aux>; LEVELS] = Default::default();
    let levels = std::array::from_fn(|lod| {
        let uploads = |channel| -> Vec<MeshUpload> {
            meshes_of_asset_in(asset, lod, channel)
                .into_iter()
                .filter(|m| !m.vertices.is_empty() && !m.indices.is_empty())
                .collect()
        };
        let plain = uploads(None);
        // ⚠ Read again for each channel, since a channel is packed into the vertices only when it is asked for; the colours are kept only where the mesh has the channel.
        let shown = |channel: DebugChannel| -> Vec<Option<Vec<[f32; 4]>>> {
            let drawn = uploads(Some(channel));
            (plain.iter().zip(drawn))
                .map(|(mesh, with)| {
                    mesh.channels
                        .contains(&channel)
                        .then(|| with.vertices.iter().map(|v| v.colour).collect())
                })
                .collect()
        };
        let (colours, tangents) = (
            shown(DebugChannel::Colour(0)),
            shown(DebugChannel::Tangent(0)),
        );
        aux[lod] = colours
            .into_iter()
            .zip(tangents)
            .map(|(colour, tangent)| Aux { colour, tangent })
            .collect();
        plain
            .into_iter()
            .map(|m| {
                let mesh = Geometry {
                    vertices: m.vertices,
                    indices: m.indices,
                };
                (mesh, m.skinning)
            })
            .collect()
    });
    Model::new(levels).with_aux(aux)
}

/// The first texture of a dictionary, decoded to the pixels a mesh samples.
pub(super) fn first_texture(asset: &or_io::Asset) -> Result<Diffuse, String> {
    first_texture_near(asset, None).map(|(diffuse, _)| diffuse)
}

/// The first texture of a dictionary, with its own size, decoded at the smallest stored mip that is still `longest` pixels on its longer side; mip 0 where none is asked for.
///
/// ⚠ A tile of a 2048 pixel texture is cut from a mip a few hundred pixels wide, not from four million decoded pixels.
pub(in crate::state::workbench) fn first_texture_near(
    asset: &or_io::Asset,
    longest: Option<u32>,
) -> Result<(Diffuse, [u32; 2]), String> {
    texture_near(asset, None, longest)
}

/// The entry of `all` that `name` says is called `called`, without regard to case, and the first where none is.
fn called_or_first<'a, T>(
    all: &[&'a T],
    called: Option<&str>,
    name: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    called
        .and_then(|c| {
            all.iter()
                .copied()
                .find(|t| name(t).eq_ignore_ascii_case(c))
        })
        .or_else(|| all.first().copied())
}

/// The texture of a dictionary called `called`, without regard to case, and the first where none is; decoded as [`first_texture_near`] does.
pub(in crate::state::workbench) fn texture_near(
    asset: &or_io::Asset,
    called: Option<&str>,
    longest: Option<u32>,
) -> Result<(Diffuse, [u32; 2]), String> {
    use or_io::codec::texture::{decode_level, ChannelView, GrcTextureFormat};
    let texture = match asset {
        or_io::Asset::TextureDict(dict) => {
            let all: Vec<_> = dict.entries.iter().flatten().collect();
            called_or_first(&all, called, |t| &t.name)
                .map(|t| Texture::Owned(std::borrow::Cow::Borrowed(&**t)))
        }
        or_io::Asset::TextureDictGen9(dict) => {
            let all: Vec<_> = dict.entries.iter().flatten().collect();
            called_or_first(&all, called, |t| &t.name)
                .map(|t| Texture::Enhanced(std::borrow::Cow::Borrowed(&**t)))
        }
        _ => None,
    }
    .ok_or_else(|| tr!("holds no texture").to_string())?;
    if !texture.has_pixel_data() {
        return Err(tr!("holds a texture with no pixels").to_string());
    }
    let own = [u32::from(texture.width()), u32::from(texture.height())];
    let level = longest.map_or(0, |want| {
        let side = own[0].max(own[1]);
        (1..u32::from(texture.mip_count()))
            .take_while(|l| side.checked_shr(*l).unwrap_or(0) >= want)
            .last()
            .unwrap_or(0)
    });
    // A level the file does not store is the first one's.
    let image = decode_level(&texture, level)
        .or_else(|_| decode_level(&texture, 0))
        .map_err(|e| tr!("its texture does not decode: {e}", e = e))?
        .with_channel_view(ChannelView::preview_default(texture.format()));
    let diffuse = Diffuse {
        id: IDS.fetch_add(1, Ordering::Relaxed),
        width: image.width,
        height: image.height,
        srgb: !matches!(
            texture.format(),
            GrcTextureFormat::A8 | GrcTextureFormat::L8
        ),
        rgba: image.data,
    };
    Ok((diffuse, own))
}

/// `diffuse` cut down to a tile by averaging boxes of pixels; `full` is the size of the texture the pixels were cut from, which is not their own when they are a mip.
pub(in crate::state::workbench) fn thumb_with(
    diffuse: &Diffuse,
    longest: u32,
    full: [u32; 2],
) -> Thumb {
    let (w, h) = (diffuse.width.max(1), diffuse.height.max(1));
    let scale = (w.max(h)).div_ceil(longest).max(1);
    let (tw, th) = (w.div_ceil(scale), h.div_ceil(scale));
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in (ty * scale)..((ty + 1) * scale).min(h) {
                for x in (tx * scale)..((tx + 1) * scale).min(w) {
                    let at = ((y * w + x) * 4) as usize;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(diffuse.rgba.get(at + c).copied().unwrap_or(0));
                    }
                    n += 1;
                }
            }
            let n = n.max(1);
            rgba.extend(sum.map(|s| (s / n) as u8));
        }
    }
    Thumb {
        id: IDS.fetch_add(1, Ordering::Relaxed),
        size: [tw as usize, th as usize],
        rgba,
        full,
    }
}

impl super::Dressed {
    /// The bones of `ped`'s skeleton, as a skin without a skeleton of its own is bound to them: the install's, once read.
    pub fn bones(&self, ped: Ped) -> Option<&[or_interchange::Bone]> {
        match &self.base {
            super::Base::Ready(mounted) => {
                mounted.bones.get(&ped)?.as_ref().ok().map(Vec::as_slice)
            }
            _ => None,
        }
    }
}

/// The asset the bytes of the file called `name` hold, as the document of its kind opens it.
fn asset_of(
    name: &Path,
    bytes: &[u8],
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Result<or_io::Asset, String> {
    let kind = DocumentKind::from_path(name)
        .ok_or_else(|| tr!("is not a file of that kind").to_string())?;
    match crate::state::document_io::parse_payload(name, kind, bytes, Arc::clone(crypto))? {
        DocPayload::Resource(asset) => Ok(asset),
        _ => Err(tr!("is not a file of that kind").to_string()),
    }
}

/// The model the bytes of the drawable file called `name` hold: its meshes at each level of detail.
pub(in crate::state::workbench) fn model_of_bytes(
    name: &Path,
    bytes: &[u8],
    crypto: &Arc<dyn RpfCryptoProvider>,
) -> Result<Model, String> {
    Ok(model_of(&asset_of(name, bytes, crypto)?))
}

/// The texture called `called` of the dictionary the bytes of the file called `name` hold, the first where none is, decoded to the pixels a mesh samples.
pub(in crate::state::workbench) fn diffuse_called_of_bytes(
    name: &Path,
    bytes: &[u8],
    crypto: &Arc<dyn RpfCryptoProvider>,
    called: &str,
) -> Result<Diffuse, String> {
    texture_near(&asset_of(name, bytes, crypto)?, Some(called), None).map(|(diffuse, _)| diffuse)
}

#[cfg(test)]
mod tests {
    use super::called_or_first;

    #[test]
    fn a_dictionary_gives_the_texture_called_like_the_model_and_the_first_where_none_is() {
        // A weapon's dictionary: the decals come first, and the diffuse is called as the model.
        let names = [
            "w_at_weapondecals",
            "w_pi_pistol_n",
            "W_PI_PISTOL",
            "w_pi_pistol_s",
        ];
        let all: Vec<&&str> = names.iter().collect();
        let pick = |called| called_or_first(&all, called, |n| *n).copied();
        assert_eq!(pick(Some("w_pi_pistol")), Some("W_PI_PISTOL"));
        assert_eq!(pick(Some("w_ar_rifle")), Some("w_at_weapondecals"));
        assert_eq!(pick(None), Some("w_at_weapondecals"));
        assert_eq!(called_or_first::<&str>(&[], Some("x"), |n| *n), None);
    }
}
