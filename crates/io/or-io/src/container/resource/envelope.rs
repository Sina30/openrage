//! The container's outer wrapper, as a contract with one implementation.
//!
//! ⚠ **An envelope is defined by the round trip, not by the read.** Anything
//! the format preserves and does not interpret has to survive `open` →
//! `reseal`, so [`Opened`] carries a [`ResourceEnvelope::Seal`] and not just
//! the three things a reader happens to want. `meta`'s RBM path is the
//! only thing in the tree that would notice the difference — an untouched
//! `.ymt` round-trips byte-identically only because the header it was opened
//! under comes back unchanged.

use flate2::Compression;

use crate::error::{IoError, Result};
use crate::Generation;

use crate::container::paging::chunk_map::DatResourceSizeInfo;

use super::builder::RSC_VIRTUAL_LEAF_SIZE;
use super::header::{
    DatResourceFileHeader, DatResourceFileType, DAT_RESOURCE_HEADER_SIZE, DAT_RESOURCE_MAGIC,
};
use super::versions_registry::{ResourceType, TitleId};

/// A container opened: its payload, the shape of it, and the token that puts
/// it back under the header it came out of.
#[derive(Debug, Clone)]
pub struct Opened<S> {
    /// The decompressed payload: the virtual half followed by the physical.
    pub data: Vec<u8>,
    pub virtual_size: u32,
    pub physical_size: u32,
    pub version: u32,
    pub seal: S,
}

/// Detect a container, open it, and seal a payload back into one.
pub trait ResourceEnvelope {
    /// Everything [`reseal`](ResourceEnvelope::reseal) needs that the payload
    /// does not carry.
    ///
    /// ⚠ **Opaque on purpose.** A caller that can assemble one field by field
    /// can assemble a wrong one, and the round trip is the only thing that
    /// would say so.
    type Seal: Clone;

    /// What a diagnostic calls this container.
    const NAME: &'static str;

    /// How many leading bytes [`detect`](ResourceEnvelope::detect) needs.
    const HEADER_LEN: usize;

    fn detect(bytes: &[u8]) -> bool;

    /// The header alone, without decompressing the payload behind it.
    fn peek(bytes: &[u8]) -> Result<Self::Seal>;

    fn open(bytes: &[u8]) -> Result<Opened<Self::Seal>>;

    /// Put `payload` back under the header `seal` came from.
    ///
    /// ⚠ Byte-identical for an unmodified payload. That is the contract, not
    /// an optimisation.
    fn reseal(payload: &[u8], seal: &Self::Seal, compression: Compression) -> Result<Vec<u8>>;
}

/// RAGE's RSC7 container, and the only implementation of [`ResourceEnvelope`].
pub struct Rsc7;

/// An RSC7 header, carried rather than interpreted.
///
/// The magic constant and the version-nibble arithmetic stay inside
/// `resource/`; a caller reads the version, page sizes and nibbles through it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rsc7Seal(DatResourceFileHeader);

impl Rsc7Seal {
    pub fn version(&self) -> u32 {
        self.0.version
    }

    pub fn virtual_size(&self) -> u32 {
        self.0.virtual_page_size()
    }

    pub fn physical_size(&self) -> u32 {
        self.0.physical_page_size()
    }

    pub fn total_size(&self) -> u32 {
        self.0.total_size()
    }

    /// The version nibble each flag word carries, virtual then physical.
    pub fn version_nibbles(&self) -> (u8, u8) {
        (
            super::header::extract_version_nibble(self.0.info.r#virtual),
            super::header::extract_version_nibble(self.0.info.physical),
        )
    }

    pub fn validate(&self, resource_type: ResourceType) -> Result<()> {
        self.0.validate(resource_type)
    }

    /// Validate against `title`'s version table: an RSC7 header says which
    /// version wrote it and never which game did.
    pub fn validate_for(&self, title: TitleId, resource_type: ResourceType) -> Result<()> {
        self.0.validate_for(title, resource_type)
    }

    /// The header as a diagnostic renders it, label and value, so no printer
    /// has to know the flag-word bitfield.
    pub fn describe(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Magic", format!("0x{:08X} ({})", self.0.magic, Rsc7::NAME)),
            (
                "Version",
                format!("{} (0x{:02X})", self.0.version, self.0.version),
            ),
            ("Virtual flags", format!("0x{:08X}", self.0.info.r#virtual)),
            ("Physical flags", format!("0x{:08X}", self.0.info.physical)),
            ("Virtual size", format!("{} bytes", self.virtual_size())),
            ("Physical size", format!("{} bytes", self.physical_size())),
        ]
    }

    /// The header's own bytes, for a caller that has to place them itself.
    ///
    /// The archive reader uses it to reattach the header an RPF entry
    /// stripped: it has bytes rather than a payload to reseal.
    pub fn header_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DAT_RESOURCE_HEADER_SIZE);
        self.0.write_to(&mut out);
        out
    }

    /// A seal that did not come out of a file, so that the round-trip
    /// contract can be shown to refuse one.
    #[cfg(test)]
    pub(super) fn of(header: DatResourceFileHeader) -> Self {
        Self(header)
    }
}

/// A file being written for the first time, which has no header to carry.
///
/// ⚠ Distinct from [`ResourceEnvelope::reseal`] on purpose: authoring
/// *invents* a header from the sizes the packer produced, and resealing
/// reproduces one. Only the second can be byte-identical, and conflating
/// them is how RBM's gate would quietly stop holding.
pub struct Authoring<'a> {
    pub file_type: DatResourceFileType,
    pub generation: Generation,
    /// The bucket layout the packer produced, when there is one.
    pub virtual_sizes: Option<&'a DatResourceSizeInfo>,
    pub physical_sizes: Option<&'a DatResourceSizeInfo>,
    pub compression: Compression,
}

impl<'a> Authoring<'a> {
    pub fn new(file_type: DatResourceFileType, compression: Compression) -> Self {
        Self {
            file_type,
            generation: Generation::Gen8,
            virtual_sizes: None,
            physical_sizes: None,
            compression,
        }
    }

    pub fn for_generation(mut self, generation: Generation) -> Self {
        self.generation = generation;
        self
    }

    pub fn with_sizes(
        mut self,
        virtual_sizes: &'a DatResourceSizeInfo,
        physical_sizes: Option<&'a DatResourceSizeInfo>,
    ) -> Self {
        self.virtual_sizes = Some(virtual_sizes);
        self.physical_sizes = physical_sizes;
        self
    }
}

impl Rsc7 {
    /// Write a new file, inventing its header from the sizes given.
    pub fn author(
        spec: Authoring<'_>,
        virtual_data: Vec<u8>,
        physical_data: Vec<u8>,
    ) -> Result<Vec<u8>> {
        match spec.virtual_sizes {
            Some(virtual_sizes) => super::header::DatResourceWriter::write_with_size_info_for(
                spec.file_type,
                spec.generation,
                virtual_data,
                physical_data,
                virtual_sizes,
                spec.physical_sizes,
                spec.compression,
            ),
            None => super::header::DatResourceWriter::write(
                spec.file_type,
                virtual_data,
                physical_data,
                spec.compression,
            ),
        }
    }

    /// A 16-byte header at `version` declaring no pages at all.
    ///
    /// ⚠ **Not a file**, and [`open`](ResourceEnvelope::open) will refuse
    /// what follows it. It lets a suite hand a reader something to reject
    /// *for its version* without spelling the magic.
    pub fn bare_header(version: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(DAT_RESOURCE_HEADER_SIZE);
        DatResourceFileHeader::new(version, 0, 0).write_to(&mut out);
        out
    }

    /// Invent a seal for a payload that has no header to carry: `virtual_len`
    /// bytes in **one** virtual chunk and no physical page.
    ///
    /// ⚠ **Distinct from [`Authoring`], which produces a file.** A caller
    /// that builds its own payload and then reseals needs the header *before*
    /// it can size the payload, because the page size the header can express
    /// is what the payload has to be padded to (`meta`'s RBM synthesiser).
    ///
    /// ⚠ **The version nibbles are not decoration.** An RPF7 resource entry
    /// overwrites the 16-byte header and keeps only the two flag words, so
    /// [`header_from_entry_flags`](Rsc7::header_from_entry_flags) recovers
    /// the version from the nibbles and from nothing else — a seal that left
    /// the physical word at zero would come back out of an archive as
    /// version 0. Shipped RBM content agrees: 2,338 of 2,338 carry nibble 2
    /// in a physical word that declares no pages at all.
    pub fn invent_single_chunk_seal(version: u32, virtual_len: usize) -> Result<Rsc7Seal> {
        let len = u32::try_from(virtual_len).map_err(|_| IoError::InvalidData {
            context: format!("{virtual_len} bytes is past what an RSC7 page size can state"),
        })?;
        let (virtual_nibble, physical_nibble) =
            DatResourceFileType::Custom(version).version_nibbles();
        let virtual_sizes = super::builder::DatResourceSizes::single_chunk(
            len,
            RSC_VIRTUAL_LEAF_SIZE,
            virtual_nibble,
        )
        .ok_or_else(|| IoError::InvalidData {
            context: format!("no single virtual chunk is large enough for {len} bytes"),
        })?;
        // A page count of zero that still carries the nibble; `get_size` of
        // this is zero, which is what makes the physical half absent.
        let physical_sizes = super::builder::DatResourceSizes {
            version: physical_nibble,
            ..Default::default()
        };
        Ok(Rsc7Seal(DatResourceFileHeader::new(
            version,
            virtual_sizes.to_flags(),
            physical_sizes.to_flags(),
        )))
    }

    /// Rebuild the header an archive entry stripped, from the two flag words
    /// the entry kept in its place.
    ///
    /// An RPF7 resource entry overwrites the 16-byte header slot with random
    /// bytes and stores the flag words in the entry itself; the version is
    /// recoverable from their nibbles and from nothing else.
    pub fn header_from_entry_flags(virtual_flags: u32, physical_flags: u32) -> Rsc7Seal {
        let version = u32::from(super::header::extract_version(
            virtual_flags,
            physical_flags,
        ));
        Rsc7Seal(DatResourceFileHeader::new(
            version,
            virtual_flags,
            physical_flags,
        ))
    }
}

impl ResourceEnvelope for Rsc7 {
    type Seal = Rsc7Seal;

    const NAME: &'static str = "RSC7";
    const HEADER_LEN: usize = DAT_RESOURCE_HEADER_SIZE;

    fn detect(bytes: &[u8]) -> bool {
        bytes.len() >= 4
            && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == DAT_RESOURCE_MAGIC
    }

    fn peek(bytes: &[u8]) -> Result<Rsc7Seal> {
        DatResourceFileHeader::parse(bytes).map(Rsc7Seal)
    }

    fn open(bytes: &[u8]) -> Result<Opened<Rsc7Seal>> {
        let info = super::header::DatResourceReader::read(bytes)?;
        Ok(Opened {
            virtual_size: info.header.virtual_page_size(),
            physical_size: info.header.physical_page_size(),
            version: info.header.version,
            seal: Rsc7Seal(info.header),
            data: info.data,
        })
    }

    fn reseal(payload: &[u8], seal: &Rsc7Seal, compression: Compression) -> Result<Vec<u8>> {
        let expected = seal.total_size() as usize;
        if payload.len() != expected {
            return Err(IoError::InvalidData {
                context: format!(
                    "resealing an RSC7 payload of {} byte(s) under a header that \
                     declares {expected}; a header carried from `open` describes \
                     the payload it was opened with",
                    payload.len()
                ),
            });
        }
        let compressed = super::compression::dat_compress(payload, compression)?;
        let mut out = Vec::with_capacity(DAT_RESOURCE_HEADER_SIZE + compressed.len());
        seal.0.write_to(&mut out);
        out.extend(compressed);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::resource::versions::YDR_FILE_VERSION;

    fn a_sealed_resource() -> (Vec<u8>, Vec<u8>) {
        let virtual_data = vec![0xAB; 8192];
        let bytes = super::super::header::DatResourceWriter::write(
            super::super::header::DatResourceFileType::Ydr,
            virtual_data,
            Vec::new(),
            Compression::fast(),
        )
        .expect("authoring an RSC7 file");
        let payload = Rsc7::open(&bytes).expect("opens").data;
        (bytes, payload)
    }

    #[test]
    fn a_file_opens_to_its_own_shape() {
        let (bytes, payload) = a_sealed_resource();
        let opened = Rsc7::open(&bytes).expect("opens");

        assert!(Rsc7::detect(&bytes));
        assert_eq!(opened.version, YDR_FILE_VERSION);
        assert_eq!(
            opened.data.len(),
            (opened.virtual_size + opened.physical_size) as usize
        );
        assert_eq!(opened.data, payload);
        assert_eq!(opened.seal.version(), opened.version);
        assert_eq!(opened.seal.total_size(), opened.data.len() as u32);
    }

    /// The property the whole `Seal` exists for.
    #[test]
    fn an_untouched_payload_reseals_under_the_header_it_opened() {
        let (bytes, _) = a_sealed_resource();
        let opened = Rsc7::open(&bytes).expect("opens");

        let back = Rsc7::reseal(&opened.data, &opened.seal, Compression::fast()).expect("reseals");

        assert_eq!(
            &back[..Rsc7::HEADER_LEN],
            &bytes[..Rsc7::HEADER_LEN],
            "the header has to come back byte for byte; RBM's identity gate is \
             the only thing downstream that would notice"
        );
        assert_eq!(
            Rsc7::open(&back).expect("reopens").data,
            opened.data,
            "and the payload with it"
        );
    }

    /// A seal built from a *different* header must not be accepted for this
    /// payload.
    #[test]
    fn a_payload_cannot_be_resealed_under_a_header_of_another_size() {
        let (bytes, _) = a_sealed_resource();
        let opened = Rsc7::open(&bytes).expect("opens");

        let smaller = Rsc7Seal::of(DatResourceFileHeader::new(YDR_FILE_VERSION, 1, 0));
        assert_ne!(smaller.total_size(), opened.seal.total_size());
        assert!(
            Rsc7::reseal(&opened.data, &smaller, Compression::fast()).is_err(),
            "a header that describes a different payload is not this payload's"
        );
    }

    /// An invented seal has to be a seal the round trip accepts, or the
    /// synthesiser it exists for cannot reach `reseal` at all.
    #[test]
    fn an_invented_seal_reseals_a_payload_padded_to_the_page_it_names() {
        const RBM_VERSION: u32 = 2;
        for wanted in [1usize, 8192, 8193, 20_000, 131_073] {
            let seal = Rsc7::invent_single_chunk_seal(RBM_VERSION, wanted).expect("a page fits");
            let page = seal.total_size() as usize;
            assert!(page >= wanted, "{page} does not hold {wanted}");
            assert_eq!(seal.physical_size(), 0, "RBM declares no physical page");
            assert_eq!(seal.version(), RBM_VERSION);

            let payload = vec![0u8; page];
            let file = Rsc7::reseal(&payload, &seal, Compression::fast()).expect("reseals");
            let opened = Rsc7::open(&file).expect("opens");
            assert_eq!(opened.version, RBM_VERSION);
            assert_eq!(opened.data.len(), page);
            assert_eq!(opened.seal, seal, "the header survives its own round trip");
        }
    }

    /// ⚠ The nibbles are what an archive keeps when it throws the header
    /// away, so a seal that did not carry them would lose its version on the
    /// way through an RPF.
    #[test]
    fn an_invented_seal_survives_having_its_header_thrown_away() {
        let seal = Rsc7::invent_single_chunk_seal(2, 8192).expect("a page fits");
        assert_eq!(seal.version_nibbles(), (0, 2));

        let bytes = seal.header_bytes();
        let virtual_flags = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let physical_flags = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let recovered = Rsc7::header_from_entry_flags(virtual_flags, physical_flags);
        assert_eq!(
            recovered, seal,
            "an RPF entry keeps only the flag words, so the version has to be in them"
        );
    }

    #[test]
    fn peek_reads_the_header_without_the_payload() {
        let (bytes, _) = a_sealed_resource();
        let seal = Rsc7::peek(&bytes[..Rsc7::HEADER_LEN]).expect("just the header");
        assert_eq!(seal.version(), YDR_FILE_VERSION);
        assert_eq!(seal, Rsc7::open(&bytes).expect("opens").seal);
    }

    /// A file whose payload is `len` bytes that deflate to about their own length.
    fn a_resource_of_noise(len: usize) -> Vec<u8> {
        let mut state = 0x9E37_79B9u32;
        let payload = (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        super::super::header::DatResourceWriter::write(
            super::super::header::DatResourceFileType::Ydr,
            payload,
            Vec::new(),
            Compression::fast(),
        )
        .expect("authoring an RSC7 file")
    }

    /// ⚠ What a read of a header alone leans on: a file cut off after its first kilobytes opens,
    /// to the front of its payload, because `open` keeps what the stream inflated to when no
    /// attempt gives the length the header states.
    #[test]
    fn a_file_cut_short_opens_to_the_front_of_its_payload() {
        let bytes = a_resource_of_noise(40_000);
        let whole = Rsc7::open(&bytes).expect("opens");

        let front = Rsc7::open(&bytes[..4096]).expect("the front of the stream is enough");

        assert!(
            front.data.len() > 1000 && front.data.len() < whole.data.len(),
            "{} of {} bytes",
            front.data.len(),
            whole.data.len()
        );
        assert_eq!(front.data, whole.data[..front.data.len()]);
        assert_eq!(front.seal, whole.seal);
    }

    #[test]
    fn something_that_is_not_a_resource_is_not_detected() {
        assert!(!Rsc7::detect(b"RPF7\0\0\0\0"));
        assert!(!Rsc7::detect(b"RS"));
        assert!(!Rsc7::detect(&[]));
        assert!(Rsc7::peek(b"RPF7\0\0\0\0\0\0\0\0\0\0\0\0").is_err());
    }

    #[test]
    fn a_description_names_every_field_the_header_holds() {
        let (bytes, _) = a_sealed_resource();
        let described = Rsc7::open(&bytes).expect("opens").seal.describe();
        let labels: Vec<&str> = described.iter().map(|(label, _)| *label).collect();
        assert_eq!(
            labels,
            [
                "Magic",
                "Version",
                "Virtual flags",
                "Physical flags",
                "Virtual size",
                "Physical size"
            ],
            "a printer renders what this returns and nothing else, so a field \
             missing here is a field no diagnostic can show"
        );
    }
}
