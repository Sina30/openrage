//! The picture the quick look shows: a decoded mip of the selected texture, read on a worker.
//!
//! ⚠ **Nothing here reads a file on the UI thread.** The picture is asked for by the file and the size the scan saw
//! it at, so a file that changed is read again, and a file the quick look is no longer on is not waited for.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use or_io::authoring::texture_import;
use or_io::container::rpf::crypto::RpfCryptoProvider;

use super::dressed::{first_texture_near, thumb_with, Thumb};
use super::scan::{AssetRow, Kept, Seen};
use crate::render::dressed::Diffuse;
use crate::state::{DocPayload, DocumentKind};

/// The longest side of the picture, in pixels.
const SIDE: u32 = 192;

/// A file the picture is made from, as the scan saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub file: PathBuf,
    bytes: Option<u64>,
}

impl Want {
    /// The file the quick look draws `row` from: the first texture of a textures source, or the texture dictionary, DDS or image that is the asset; none for anything else.
    pub fn of(row: &AssetRow) -> Option<Self> {
        let file = match row.kept {
            Kept::Source => row
                .path
                .join(or_source::textures::DIR)
                .join(row.textures.first()?),
            Kept::File if row.seen == Seen::Present => row.path.clone(),
            Kept::File | Kept::Pack => return None,
        };
        is_a_picture(&file).then_some(Self {
            file,
            bytes: row.bytes,
        })
    }
}

impl Want {
    /// The first texture of the garment `row` stands for, which its pack holds as a texture dictionary or a textures source; none for a garment with none, or one that names its texture by a reference.
    pub fn of_garment(packs: &[super::wardrobe::Pack], row: &AssetRow) -> Option<Self> {
        let pack = packs.iter().find(|pack| pack.module == row.content)?;
        let read = pack.read.as_ref().ok()?;
        let garment = read.wardrobe.garment(row.id.as_deref()?)?;
        let name = garment
            .variations
            .iter()
            .find_map(|v| v.texture.as_deref())?;
        if or_project::reference::is_reference(name) {
            return None;
        }
        Self::of_file(
            pack.folder
                .join(name.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
    }

    /// The picture of `file` when it is one the quick look can draw: a garment's texture dictionary, or the textures source a build cooks into one.
    pub fn of_file(file: PathBuf) -> Option<Self> {
        if file.is_dir() {
            let described = or_source::cook::describe(&file).ok()?;
            return (described.kind == or_source::textures::KIND).then_some(Self {
                file,
                bytes: Some(described.bytes),
            });
        }
        let bytes = std::fs::metadata(&file).ok().map(|m| m.len());
        is_a_picture(&file).then_some(Self { file, bytes })
    }
}

/// Whether the quick look can draw `file`: a texture dictionary, a DDS or an image a texture compiles from.
fn is_a_picture(file: &Path) -> bool {
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    extension.eq_ignore_ascii_case("ytd")
        || texture_import::SourceFormat::from_extension(extension).is_some()
}

/// What the quick look has for the asset it is on.
pub enum Glimpse<'a> {
    /// The asset is no texture, so there is no picture to wait for.
    Nothing,
    Reading,
    Picture(&'a Arc<Thumb>),
    /// Why the file could not be drawn.
    Cannot(&'a str),
}

/// The picture being read and the one last read.
#[derive(Default)]
pub struct Glance {
    reading: Option<(Want, Receiver<Result<Thumb, String>>)>,
    got: Option<(Want, Result<Arc<Thumb>, String>)>,
}

impl Glance {
    /// What there is to show for `want`: the picture once a worker has read it, which this starts when nothing has.
    pub fn look(&mut self, want: Option<Want>, crypto: &Arc<dyn RpfCryptoProvider>) -> Glimpse<'_> {
        let Some(want) = want else {
            return Glimpse::Nothing;
        };
        if let Some((asked, receiver)) = &self.reading {
            if let Ok(result) = receiver.try_recv() {
                self.got = Some((asked.clone(), result.map(Arc::new)));
                self.reading = None;
            }
        }
        if self.got.as_ref().is_some_and(|(held, _)| *held == want) {
            return match &self.got {
                Some((_, Ok(thumb))) => Glimpse::Picture(thumb),
                Some((_, Err(why))) => Glimpse::Cannot(why),
                None => Glimpse::Nothing,
            };
        }
        if self
            .reading
            .as_ref()
            .is_none_or(|(asked, _)| *asked != want)
        {
            let (sender, receiver) = channel();
            let (file, crypto) = (want.file.clone(), Arc::clone(crypto));
            std::thread::spawn(move || {
                let _ = sender.send(read(&file, crypto));
            });
            self.reading = Some((want, receiver));
        }
        Glimpse::Reading
    }

    /// Whether a picture is being read, so the panel asks to be drawn again.
    pub fn is_reading(&self) -> bool {
        self.reading.is_some()
    }
}

/// The first texture of `file`, decoded at its first mip and cut down to the picture's size.
fn read(file: &Path, crypto: Arc<dyn RpfCryptoProvider>) -> Result<Thumb, String> {
    // A texture that is a source is cooked first, as a build cooks it.
    let (bytes, named) = super::dressed::pack_bytes(file)?;
    let (diffuse, full) = if named
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ytd"))
    {
        let name = named.clone();
        let asset = match crate::state::document_io::parse_payload(
            &name,
            DocumentKind::Ytd,
            &bytes,
            crypto,
        ) {
            Ok(DocPayload::Resource(asset)) => asset,
            Ok(_) => return Err(tr!("is not a texture dictionary").to_string()),
            Err(why) => return Err(why),
        };
        first_texture_near(&asset, Some(SIDE))?
    } else {
        let image = texture_import::decode_path(file, &bytes).map_err(|e| e.to_string())?;
        let own = [image.width, image.height];
        let diffuse = Diffuse {
            id: 0,
            width: image.width,
            height: image.height,
            rgba: image.data,
            srgb: image.is_srgb,
        };
        (diffuse, own)
    };
    Ok(thumb_with(&diffuse, SIDE, full))
}
