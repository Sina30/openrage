pub mod enum_text;
pub mod hash_index;
pub mod layout;
pub mod pso;
pub mod public_names;
pub mod rbf;
pub mod rbm;
pub mod root_types;
pub mod synth;
pub mod type_tables;

use crate::meta::diagnostics::Diagnostics;
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::MetaInstance;
use crate::meta::schema::SchemaLibrary;

pub use layout::{ContainerLayout, MetaLayout};

/// Where an encoder is in the value tree, as `CMapData.entities[3].lodLevel`.
///
/// ⚠ **Only the innermost failure is located.** A refusal passes back up
/// through every member that contains it, and naming it at each level wrote
/// `CMapData.entities: CEntityDef.lodLevel:` with no element in it at all.
#[derive(Debug, Default)]
pub(crate) struct EncodePath {
    segments: Vec<String>,
    located: bool,
}

impl EncodePath {
    pub(crate) fn push(&mut self, segment: String) {
        self.segments.push(segment);
    }

    pub(crate) fn pop(&mut self) {
        self.segments.pop();
    }

    pub(crate) fn locate(&mut self, error: MetaError) -> MetaError {
        if self.located {
            return error;
        }
        self.located = true;
        error.in_context(&self.segments.concat())
    }
}

/// How deep a reader lets the value tree nest. The deepest file the metadata
/// corpus holds nests 12 levels (`or-io-verify --test meta`, at f5bd1d26).
const READ_DEPTH: usize = 32;

/// The structs, by address and type hash, and the arrays and maps a reader
/// is inside.
///
/// ⚠ A pointer, an array header and an element descriptor are what the file
/// states, so a corrupt one can lead back into what is still being read, and
/// following it recurses until the stack overflows, which aborts rather than
/// panics. A lenient read goes on to a failed member's siblings, which would
/// each nest as deep again, so once one `enter` is refused every later one is.
///
/// ⚠ The cap is [`READ_DEPTH`], not synthesis's 128: at `opt-level = 0`, as
/// hosted CI builds, 128 levels of a corrupt RBM overflow a 2 MiB thread stack.
#[derive(Debug, Default)]
pub(crate) struct Nesting {
    open: Vec<Option<(usize, u32)>>,
    refused: Option<String>,
}

impl Nesting {
    /// Enter the struct `Some((at, type_hash))`, or an array or map for `None`,
    /// returning the mark [`Nesting::leave`] takes.
    pub(crate) fn enter(&mut self, place: Option<(usize, u32)>) -> Result<usize> {
        if self.refused.is_none() {
            self.refused = match place {
                Some((at, type_hash)) if self.open.contains(&place) => Some(format!(
                    "binary meta: the struct {type_hash:#X} at {at:#X} contains itself"
                )),
                _ if self.open.len() > READ_DEPTH => Some(format!(
                    "binary meta: the value tree nests deeper than {READ_DEPTH} levels"
                )),
                _ => None,
            };
        }
        if let Some(why) = &self.refused {
            return Err(MetaError::Other(why.clone()));
        }
        self.open.push(place);
        Ok(self.open.len() - 1)
    }

    /// Leave everything entered since `mark`, refused if anything was.
    pub(crate) fn leave(&mut self, mark: usize) -> Result<()> {
        self.open.truncate(mark);
        match &self.refused {
            Some(why) => Err(MetaError::Other(why.clone())),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BinaryFormat {
    Pso,
    Rbm,
    Rbf,
}

impl BinaryFormat {
    pub fn label(&self) -> &'static str {
        match self {
            BinaryFormat::Pso => "PSO",
            BinaryFormat::Rbm => "RBM",
            BinaryFormat::Rbf => "RBF",
        }
    }
}

const PSO_MAGIC_PSIN: [u8; 4] = *b"PSIN";
const RBF_MAGIC: [u8; 4] = *b"RBF0";
const RSC7_MAGIC: [u8; 4] = *b"RSC7";
pub(crate) const RBM_PRD0_MARKER_LE: [u8; 4] = [0x30, 0x44, 0x52, 0x50];

/// What the leading magic alone can say.
///
/// ⚠ Every `RSC7` answers `Rbm`, and most are not metadata: the `PRD0`
/// sentinel that separates an RBM from a drawable is at 0x10 of the
/// decompressed virtual heap, so a `.ydr`, `.ybn`, `.ynv` or `.yed` answers
/// `Rbm` here and is refused by the reader one step later. [`classify`] is
/// right at the cost of a decompression.
pub fn detect_binary_format(bytes: &[u8]) -> Option<BinaryFormat> {
    if bytes.len() < 4 {
        return None;
    }
    let head: [u8; 4] = bytes[..4].try_into().ok()?;
    if head == PSO_MAGIC_PSIN {
        return Some(BinaryFormat::Pso);
    }
    if head == RBF_MAGIC {
        return Some(BinaryFormat::Rbf);
    }
    if head == RSC7_MAGIC {
        return Some(BinaryFormat::Rbm);
    }
    None
}

/// What a blob is, once the cost of decompressing an `RSC7` has been paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Meta(BinaryFormat),
    /// An `RSC7` paged resource that is not metadata, and its version.
    Paged(u32),
    /// Not a container this crate recognises.
    Other,
}

impl Container {
    pub fn label(self) -> String {
        match self {
            Container::Meta(format) => format.label().to_string(),
            Container::Paged(version) => format!("RSC7 v{version}"),
            Container::Other => "plain".to_string(),
        }
    }

    pub fn is_meta(self) -> bool {
        matches!(self, Container::Meta(_))
    }
}

/// Classify `bytes`, decompressing an `RSC7` to tell an RBM from a paged
/// asset, which nothing cheaper can.
pub fn classify(bytes: &[u8]) -> Container {
    match detect_binary_format(bytes) {
        Some(BinaryFormat::Rbm) => match rsc7_virtual_heap(bytes) {
            Some((_, heap)) if rbm_sentinel_present(&heap) => Container::Meta(BinaryFormat::Rbm),
            Some((version, _)) => Container::Paged(version),
            None => Container::Other,
        },
        Some(format) => Container::Meta(format),
        None => Container::Other,
    }
}

/// Whether a decompressed virtual heap carries the `PRD0` block an RBM opens
/// with.
pub fn rbm_sentinel_present(virtual_heap: &[u8]) -> bool {
    virtual_heap
        .get(0x10..0x14)
        .is_some_and(|word| word == RBM_PRD0_MARKER_LE)
}

fn rsc7_virtual_heap(bytes: &[u8]) -> Option<(u32, Vec<u8>)> {
    use crate::container::resource::{ResourceEnvelope, Rsc7};

    let version = Rsc7::peek(bytes).ok()?.version();
    let resource = Rsc7::open(bytes).ok()?;
    Some((version, resource.data))
}

pub fn parse_meta_binary(library: &SchemaLibrary, bytes: &[u8]) -> Result<MetaInstance> {
    let mut diags = Diagnostics::lenient();
    parse_meta_binary_with(library, bytes, &mut diags)
}

pub fn parse_meta_binary_with(
    library: &SchemaLibrary,
    bytes: &[u8],
    diags: &mut Diagnostics,
) -> Result<MetaInstance> {
    parse_meta_binary_full(library, bytes, diags).map(|(inst, _)| inst)
}

pub fn parse_meta_binary_full(
    library: &SchemaLibrary,
    bytes: &[u8],
    diags: &mut Diagnostics,
) -> Result<(MetaInstance, MetaLayout)> {
    let format = detect_binary_format(bytes)
        .ok_or_else(|| MetaError::Other("binary meta: unknown leading magic".into()))?;
    let (instance, container) = match format {
        BinaryFormat::Pso => {
            let (inst, parts) = pso::reader::parse_full(library, bytes, diags)?;
            (inst, ContainerLayout::Pso(parts.into_layout()))
        }
        BinaryFormat::Rbm => {
            let (inst, parts) = rbm::reader::parse_full(library, bytes, diags)?;
            (inst, ContainerLayout::Rbm(Box::new(parts.into_layout())))
        }
        BinaryFormat::Rbf => {
            let (inst, layout) = rbf::reader::parse_full(library, bytes, diags)?;
            (inst, ContainerLayout::Rbf(layout))
        }
    };
    Ok((instance, MetaLayout { format, container }))
}

pub fn write_meta_binary(
    format: BinaryFormat,
    layout: Option<&MetaLayout>,
    instance: &MetaInstance,
) -> Result<Vec<u8>> {
    if let Some(layout) = layout {
        if layout.format != format {
            return Err(MetaError::Other(format!(
                "binary meta: layout is {:?} but asked to write {:?}",
                layout.format, format
            )));
        }
    }
    match format {
        BinaryFormat::Pso => {
            let layout = match layout.map(|l| &l.container) {
                Some(ContainerLayout::Pso(p)) => Some(p),
                _ => None,
            };
            pso::writer::write(layout, instance)
        }
        BinaryFormat::Rbm => {
            let layout = match layout.map(|l| &l.container) {
                Some(ContainerLayout::Rbm(r)) => Some(r.as_ref()),
                _ => None,
            };
            rbm::writer::write(layout, instance)
        }
        BinaryFormat::Rbf => {
            let layout = match layout.map(|l| &l.container) {
                Some(ContainerLayout::Rbf(r)) => Some(r),
                _ => None,
            };
            rbf::writer::write(layout, instance)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_pso_magic() {
        assert_eq!(
            detect_binary_format(b"PSIN\0\0\0\0"),
            Some(BinaryFormat::Pso)
        );
    }

    #[test]
    fn detect_rbf_magic() {
        assert_eq!(
            detect_binary_format(b"RBF0\0\0\0\0"),
            Some(BinaryFormat::Rbf)
        );
    }

    #[test]
    fn detect_rsc7_magic_routes_to_rbm() {
        assert_eq!(
            detect_binary_format(b"RSC7\0\0\0\0"),
            Some(BinaryFormat::Rbm)
        );
    }

    #[test]
    fn the_sentinel_is_what_separates_an_rbm_from_a_paged_asset() {
        let mut heap = vec![0u8; 0x20];
        assert!(!rbm_sentinel_present(&heap));
        heap[0x10..0x14].copy_from_slice(&RBM_PRD0_MARKER_LE);
        assert!(rbm_sentinel_present(&heap));

        assert!(!rbm_sentinel_present(&heap[..0x13]));
        assert!(!rbm_sentinel_present(&[]));

        // The marker is the dword `PRD0` stored little-endian, `0DRP` byte by
        // byte, so a search for the ASCII spelling finds nothing.
        assert_eq!(&RBM_PRD0_MARKER_LE, b"0DRP");
        assert_eq!(
            u32::from_le_bytes(RBM_PRD0_MARKER_LE),
            u32::from_be_bytes(*b"PRD0")
        );
    }

    #[test]
    fn a_blob_that_is_not_a_container_classifies_as_other() {
        assert_eq!(
            classify(b"PSIN\0\0\0\0"),
            Container::Meta(BinaryFormat::Pso)
        );
        assert_eq!(
            classify(b"RBF0\0\0\0\0"),
            Container::Meta(BinaryFormat::Rbf)
        );
        assert_eq!(classify(b"not a container"), Container::Other);
        assert_eq!(classify(b""), Container::Other);
        // An RSC7 magic with nothing behind it cannot be decompressed, so it
        // is not claimed as either.
        assert_eq!(classify(b"RSC7\0\0\0\0"), Container::Other);

        assert_eq!(Container::Paged(165).label(), "RSC7 v165");
        assert_eq!(Container::Meta(BinaryFormat::Pso).label(), "PSO");
        assert!(!Container::Paged(165).is_meta());
        assert!(Container::Meta(BinaryFormat::Rbm).is_meta());
    }
}
