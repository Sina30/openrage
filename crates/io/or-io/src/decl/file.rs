//! A file opened as an owned value and saved from one: the calls every file
//! type shares, and the RSC7 resource that implements them for a declared root.

#[cfg(feature = "gta5")]
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use super::owned::Repair;
use crate::error::Error;
#[cfg(feature = "gta5")]
use crate::gta5::formats::{self, Format};
#[cfg(feature = "gta5")]
use crate::Generation;

#[cfg(feature = "gta5")]
use super::owned::{Cx, Owned};
#[cfg(feature = "gta5")]
use super::{Aligned, Described, Record, View};

/// Where an opened file came from, so `save` needs no arguments.
#[derive(Debug, Clone, Default)]
pub struct Origin(Option<Box<PathBuf>>);

impl Origin {
    pub fn new(path: &Path) -> Self {
        Self(Some(Box::new(path.to_path_buf())))
    }

    pub fn path(&self) -> Option<&Path> {
        self.0.as_deref().map(PathBuf::as_path)
    }
}

impl PartialEq for Origin {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// A value that remembers where it was opened from.
pub trait HasOrigin {
    fn origin(&self) -> &Origin;
    fn origin_mut(&mut self) -> &mut Origin;
}

/// A file type: opened from a path or bytes into an owned value, and saved back.
pub trait File: HasOrigin + Sized {
    /// The extension its files carry, lowercase and without the dot.
    const EXTENSION: &'static str;

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error>;

    /// The whole file; refused if the game would misread it, or if it rewrites a count the file states against its run.
    fn to_bytes(&self) -> Result<Vec<u8>, Error>;

    /// [`File::to_bytes`], rewriting a count the file states against its run as `repair` allows, and each count it rewrote.
    fn to_bytes_repairing(&self, repair: Repair) -> Result<(Vec<u8>, Vec<String>), Error> {
        let _ = repair;
        Ok((self.to_bytes()?, Vec::new()))
    }

    fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        check_kind(path, Self::EXTENSION)?;
        let bytes = std::fs::read(path).map_err(|source| Error::FileReadFailed {
            path: path.to_path_buf(),
            source,
        })?;
        let mut value = Self::from_bytes(&bytes)?;
        *value.origin_mut() = Origin::new(path);
        Ok(value)
    }

    fn save(&self) -> Result<(), Error> {
        let path = self.origin().path().ok_or(Error::NoPath)?.to_path_buf();
        self.save_as(path)
    }

    fn save_as(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let bytes = self.to_bytes()?;
        std::fs::write(path, bytes).map_err(|source| Error::FileWriteFailed {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// A declared RSC7 root: one format at one target, whose [`File`] calls read
/// and write through the format's description and canonical writer.
#[cfg(feature = "gta5")]
pub trait Resource: File + Owned
where
    Self::Raw: Record,
{
    const EXT: &'static str;
    const TARGET: Generation;

    fn format() -> &'static Format {
        formats::format(Self::EXT).expect("a resource's format is declared")
    }

    /// A zero-copy view for tools that only look.
    fn peek(file: &[u8]) -> Result<Peek<Self>, Error> {
        let (bytes, virtual_size, _) = open_resource::<Self>(file)?;
        Ok(Peek {
            bytes,
            virtual_size,
            _t: PhantomData,
        })
    }
}

#[cfg(feature = "gta5")]
impl<T> File for T
where
    T: Resource,
    T::Raw: Record,
{
    const EXTENSION: &'static str = T::EXT;

    /// Refused unless the file's RSC7 version names this root's layout.
    fn from_bytes(file: &[u8]) -> Result<Self, Error> {
        let (data, virtual_size, target) = open_resource::<Self>(file)?;
        let image = crate::desc::Image::new(data.bytes(), virtual_size);
        let tree = Self::format().read(target, image)?;
        Self::from_element(&tree)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.to_bytes_repairing(Repair::Refuse)
            .map(|(bytes, _)| bytes)
    }

    fn to_bytes_repairing(&self, repair: Repair) -> Result<(Vec<u8>, Vec<String>), Error> {
        let pack = Self::format().description(Self::TARGET)?;
        let cx = Cx::new(pack.plans().clone()).repairing(repair);
        let tree = self.to_element(&cx)?;
        let bytes = Self::format().save(Self::TARGET, &tree)?;
        Ok((bytes, cx.repairs()))
    }
}

/// A decompressed file held aligned, and a zero-copy view of its root.
#[cfg(feature = "gta5")]
pub struct Peek<T: Owned> {
    bytes: Aligned,
    virtual_size: usize,
    _t: PhantomData<fn() -> T>,
}

#[cfg(feature = "gta5")]
impl<T: Owned> Peek<T>
where
    T::Raw: Record,
{
    pub fn image(&self) -> crate::desc::Image<'_> {
        crate::desc::Image::new(self.bytes.bytes(), self.virtual_size)
    }

    /// The root, read in place: fields are loads, pointers are followed on request.
    pub fn root(&self) -> super::Result<View<'_, T::Raw>> {
        View::at_offset(self.image(), 0)
    }
}

// With no format declared, no extension names another kind.
#[cfg(not(feature = "gta5"))]
fn check_kind(_: &Path, _: &'static str) -> Result<(), Error> {
    Ok(())
}

#[cfg(feature = "gta5")]
fn check_kind(path: &Path, ext: &'static str) -> Result<(), Error> {
    let found = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match found {
        Some(f) if f != ext && formats::format(&f).is_some() => Err(Error::WrongKind {
            found: kind_name(&f),
            wanted: kind_name(ext),
        }),
        _ => Ok(()),
    }
}

/// How a user names a file type: "a fragment (.yft)", or "a .gxt2 file" for one no format row names.
pub fn kind_name(ext: &str) -> String {
    #[cfg(feature = "gta5")]
    if let Some(format) = formats::format(ext) {
        return format!("{} (.{})", format.noun, format.ext);
    }
    format!("a .{ext} file")
}

#[cfg(feature = "gta5")]
fn open_resource<T: Resource>(file: &[u8]) -> Result<(Aligned, usize, Generation), Error>
where
    T::Raw: Record,
{
    use crate::container::resource::ResourceEnvelope;
    let opened = crate::container::resource::Rsc7::open(file)
        .map_err(|e| Error::NotAResource(e.to_string()))?;
    let target = T::format().target_of_version(opened.version)?;
    if target != T::TARGET {
        return Err(Error::WrongTarget {
            ext: T::EXT,
            type_name: <T::Raw as Described>::name().into(),
            found: target.edition_label(),
        });
    }
    Ok((
        Aligned::from_bytes(&opened.data),
        opened.virtual_size as usize,
        target,
    ))
}
