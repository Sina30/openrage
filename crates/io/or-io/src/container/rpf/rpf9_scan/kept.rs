//! A scan's finds as bytes a later run reads back, so an archive scanned to its end once is not
//! read again. A kept scan is one zlib stream, whose checksum refuses bytes that were altered.

use std::io::{self, Read, Write};

use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};

use crate::gta6::other_bodies::OtherBody;

use super::{
    Rpf9Block, Rpf9BodyKind, Rpf9Content, Rpf9Header, Rpf9Resource, Rpf9Role, Rpf9ScanFind,
    Rpf9Signature, Rpf9Walk, Rpf9ZlibRun, RPF9_HEADER_SIZE,
};

const MAGIC: [u8; 4] = *b"9SCN";
/// The version of a kept scan's bytes: a change to what [`Rpf9ScanFind::encode`] writes takes another.
const VERSION: u32 = 3;

/// The first of four tables a kept scan states a value by its place in.
///
/// ⚠ A value its table lacks is written as a tag no read accepts, so an archive that holds one
/// would be scanned again at every open.
const CONTENTS: [Rpf9Content; 17] = [
    Rpf9Content::Xml,
    Rpf9Content::Text,
    Rpf9Content::Pso,
    Rpf9Content::Rbf,
    Rpf9Content::ResourceShaped,
    Rpf9Content::ClipRoot,
    Rpf9Content::PipelineCache,
    Rpf9Content::Other,
    Rpf9Content::TextTable,
    Rpf9Content::AudioData,
    Rpf9Content::Identified(OtherBody::HeightMap),
    Rpf9Content::Identified(OtherBody::HeightGrid),
    Rpf9Content::Identified(OtherBody::SpanGrid),
    Rpf9Content::Identified(OtherBody::MoveNetwork),
    Rpf9Content::Identified(OtherBody::Gfx),
    Rpf9Content::Identified(OtherBody::Edup),
    Rpf9Content::Identified(OtherBody::ObjectTrees),
];
const ROLES: [Rpf9Role; 15] = [
    Rpf9Role::Bound,
    Rpf9Role::Drawable,
    Rpf9Role::Dictionary,
    Rpf9Role::DrawableWithBound,
    Rpf9Role::NavMesh,
    Rpf9Role::NavOctree,
    Rpf9Role::AccelerationStructure,
    Rpf9Role::Cloth,
    Rpf9Role::MorphTarget,
    Rpf9Role::VertexBuffers,
    Rpf9Role::KeyedBlob,
    Rpf9Role::KeyedBlobDescriptor,
    Rpf9Role::AssetIndex,
    Rpf9Role::NeighbourTable,
    Rpf9Role::ParticleEffects,
];
const SECTIONS: [&str; 2] = ["virtual", "physical"];
/// Every reason a walk refuses a resource's blocks for.
const REFUSALS: [&str; 12] = [
    "the block map lies past the zlib blocks of the virtual section",
    "the block map lies past the two blocks a virtual section has",
    "the block map is not zero beside its two counts, or counts no virtual block, or more than two in a section",
    "the block map lies past the virtual blocks it counts",
    "a stream after its section's head inflates to a whole number of 64 KiB, as no tail does",
    "a block before its section's last is stored raw, as no measured resource has one",
    "a block stored raw has no end within 64 KiB: no zero pad before a next item",
    "zero bytes or the file's end stand where a block the map counts would start",
    "a block before its section's last does not hold a whole number of 64 KiB",
    "the file ends inside a block",
    "a block inflates past 64 MiB",
    "a block's zlib stream does not inflate",
];

fn unreadable(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("a kept scan {why}"))
}

/// `plain` as a kept scan's bytes: a magic word, the version and its length, then `plain` as a zlib stream.
pub fn seal_rpf9_scan(plain: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = MAGIC.to_vec();
    out.extend(VERSION.to_le_bytes());
    put(&mut out, plain.len() as u64);
    let mut stream = ZlibEncoder::new(out, Compression::fast());
    stream.write_all(plain)?;
    stream.finish()
}

/// What [`seal_rpf9_scan`] sealed, of at most `limit` bytes; an error for another version and for bytes cut short or altered.
pub fn unseal_rpf9_scan(sealed: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    let bytes = &mut &*sealed;
    let (magic, version) = (take::<4>(bytes)?, u32::from_le_bytes(take(bytes)?));
    if magic != MAGIC || version != VERSION {
        return Err(unreadable("is of another version"));
    }
    let len = usize::try_from(word(bytes)?).unwrap_or(usize::MAX);
    if len > limit {
        return Err(unreadable("states more bytes than a scan holds"));
    }
    // ⚠ A stream cut short inflates to fewer bytes without an error: only its end, which holds
    // the checksum, and the stated length say the bytes are whole.
    let mut plain = Vec::with_capacity(len.saturating_add(1));
    match Decompress::new(true).decompress_vec(bytes, &mut plain, FlushDecompress::Finish) {
        Ok(Status::StreamEnd) if plain.len() == len => Ok(plain),
        _ => Err(unreadable("is cut short or altered")),
    }
}

fn put(out: &mut Vec<u8>, word: u64) {
    out.extend(word.to_le_bytes());
}

/// Whether `value` is there, then what `put` writes of it.
fn put_some<T>(out: &mut Vec<u8>, value: Option<T>, put: impl FnOnce(&mut Vec<u8>, T)) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        put(out, value);
    }
}

/// The place of `value` in `all`, which states it.
fn tag<T: PartialEq>(all: &[T], value: &T) -> u8 {
    let at = all.iter().position(|one| one == value);
    at.map_or(u8::MAX, |at| at as u8)
}

/// The next `N` bytes of `bytes`, which moves past them.
fn take<const N: usize>(bytes: &mut &[u8]) -> io::Result<[u8; N]> {
    let mut taken = [0; N];
    bytes.read_exact(&mut taken)?;
    Ok(taken)
}

fn byte(bytes: &mut &[u8]) -> io::Result<u8> {
    take::<1>(bytes).map(|[byte]| byte)
}

fn word(bytes: &mut &[u8]) -> io::Result<u64> {
    take(bytes).map(u64::from_le_bytes)
}

/// The next word of `bytes` as a narrower number.
fn narrow<T: TryFrom<u64>>(bytes: &mut &[u8]) -> io::Result<T> {
    T::try_from(word(bytes)?).map_err(|_| unreadable("states a number too large for its field"))
}

fn flag(bytes: &mut &[u8]) -> io::Result<bool> {
    match byte(bytes)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(unreadable("states a flag that is neither 0 nor 1")),
    }
}

/// What `get` reads, where the next byte of `bytes` says a value is there.
fn some<T>(
    bytes: &mut &[u8],
    get: impl FnOnce(&mut &[u8]) -> io::Result<T>,
) -> io::Result<Option<T>> {
    flag(bytes)?.then(|| get(bytes)).transpose()
}

/// The value of `all` the next byte of `bytes` states.
fn tagged<T: Copy>(all: &[T], bytes: &mut &[u8]) -> io::Result<T> {
    let at = usize::from(byte(bytes)?);
    (all.get(at).copied()).ok_or_else(|| unreadable("states a value this build does not know"))
}

impl Rpf9ScanFind {
    /// This find appended to `out`, as [`Self::decode`] reads it.
    pub fn encode(&self, out: &mut Vec<u8>) {
        put(out, self.offset);
        put(out, self.span);
        for word in [self.stored_len, self.inflated_len, self.container] {
            put_some(out, word, put);
        }
        match self.kind {
            Rpf9BodyKind::Archive(header) => {
                out.push(0);
                out.extend(header.raw);
            }
            Rpf9BodyKind::Zlib(run) => {
                out.push(1);
                put_run(out, run);
            }
            Rpf9BodyKind::StoredRsc8 { version, extent } => {
                out.extend([2, version]);
                put(out, extent);
            }
            Rpf9BodyKind::Bink2 { revision, length } => {
                out.extend([3, revision]);
                put(out, length);
            }
            Rpf9BodyKind::Awc {
                flags,
                streams,
                chunks,
            } => {
                out.push(4);
                for word in [flags.into(), streams.into(), chunks.into()] {
                    put(out, word);
                }
            }
            Rpf9BodyKind::Sgd5 { head_layouts } => {
                out.push(5);
                put(out, head_layouts.into());
            }
            Rpf9BodyKind::Names { archive } => {
                out.push(6);
                put(out, archive);
            }
            Rpf9BodyKind::FirstEntry { head } => {
                out.push(7);
                out.extend(head);
            }
            Rpf9BodyKind::RawDeflate { content } => {
                out.extend([8, tag(&CONTENTS, &content)]);
            }
        }
    }

    /// The find at the head of `bytes`, which moves past it; an error where the bytes are no find.
    pub fn decode(bytes: &mut &[u8]) -> io::Result<Self> {
        let (offset, span) = (word(bytes)?, word(bytes)?);
        let (stored_len, inflated_len) = (some(bytes, word)?, some(bytes, word)?);
        let container = some(bytes, word)?;
        let kind = match byte(bytes)? {
            0 => Rpf9BodyKind::Archive(
                Rpf9Header::read(&take::<RPF9_HEADER_SIZE>(bytes)?)
                    .map_err(|_| unreadable("states an archive header that is not RPF9's"))?,
            ),
            1 => Rpf9BodyKind::Zlib(run(bytes)?),
            2 => Rpf9BodyKind::StoredRsc8 {
                version: byte(bytes)?,
                extent: word(bytes)?,
            },
            3 => Rpf9BodyKind::Bink2 {
                revision: byte(bytes)?,
                length: word(bytes)?,
            },
            4 => Rpf9BodyKind::Awc {
                flags: narrow(bytes)?,
                streams: narrow(bytes)?,
                chunks: narrow(bytes)?,
            },
            5 => Rpf9BodyKind::Sgd5 {
                head_layouts: narrow(bytes)?,
            },
            6 => Rpf9BodyKind::Names {
                archive: word(bytes)?,
            },
            7 => Rpf9BodyKind::FirstEntry { head: take(bytes)? },
            8 => Rpf9BodyKind::RawDeflate {
                content: tagged(&CONTENTS, bytes)?,
            },
            _ => return Err(unreadable("states a kind of body this build does not know")),
        };
        Ok(Self {
            offset,
            kind,
            span,
            stored_len,
            inflated_len,
            container,
        })
    }
}

fn put_run(out: &mut Vec<u8>, run: Rpf9ZlibRun) {
    out.push(tag(&CONTENTS, &run.content));
    put(out, run.chunks.into());
    put(out, run.stored_pieces.into());
    for word in [run.first_chunk_len, run.first_chunk_inflated, run.inflated] {
        put(out, word);
    }
    put_some(out, run.signature, |out, signature| {
        out.extend(signature.classes);
        put_some(out, signature.role, |out, role| {
            out.push(tag(&ROLES, &role))
        });
    });
    put_some(out, run.resource, |out, resource| {
        out.extend([resource.virtual_blocks, resource.physical_blocks]);
        for block in resource.blocks {
            put_some(out, block, |out, block| {
                for word in [block.offset, block.stored_len, block.inflated_len] {
                    put(out, word);
                }
                out.extend([u8::from(block.zlib), u8::from(block.physical)]);
            });
        }
        put(out, resource.virtual_len);
        put(out, resource.physical_len);
        match resource.walk {
            Rpf9Walk::Whole => out.push(0),
            Rpf9Walk::NotOnDisk { at } => {
                out.push(1);
                put(out, at);
            }
            Rpf9Walk::Refused { at, why } => {
                out.extend([2, tag(&REFUSALS, &why)]);
                put(out, at);
            }
            Rpf9Walk::TailNotDeflate { at } => {
                out.push(3);
                put(out, at);
            }
            Rpf9Walk::PointersPast { section, targets } => {
                out.extend([4, tag(&SECTIONS, &section)]);
                put(out, targets as u64);
            }
        }
    });
}

fn run(bytes: &mut &[u8]) -> io::Result<Rpf9ZlibRun> {
    Ok(Rpf9ZlibRun {
        content: tagged(&CONTENTS, bytes)?,
        chunks: narrow(bytes)?,
        stored_pieces: narrow(bytes)?,
        first_chunk_len: word(bytes)?,
        first_chunk_inflated: word(bytes)?,
        inflated: word(bytes)?,
        signature: some(bytes, |bytes| {
            Ok(Rpf9Signature {
                classes: take(bytes)?,
                role: some(bytes, |bytes| tagged(&ROLES, bytes))?,
            })
        })?,
        resource: some(bytes, resource)?,
    })
}

fn resource(bytes: &mut &[u8]) -> io::Result<Rpf9Resource> {
    let (virtual_blocks, physical_blocks) = (byte(bytes)?, byte(bytes)?);
    let mut blocks = [None; 4];
    for block in &mut blocks {
        *block = some(bytes, |bytes| {
            Ok(Rpf9Block {
                offset: word(bytes)?,
                stored_len: word(bytes)?,
                inflated_len: word(bytes)?,
                zlib: flag(bytes)?,
                physical: flag(bytes)?,
            })
        })?;
    }
    Ok(Rpf9Resource {
        virtual_blocks,
        physical_blocks,
        blocks,
        virtual_len: word(bytes)?,
        physical_len: word(bytes)?,
        walk: match byte(bytes)? {
            0 => Rpf9Walk::Whole,
            1 => Rpf9Walk::NotOnDisk { at: word(bytes)? },
            2 => Rpf9Walk::Refused {
                why: tagged(&REFUSALS, bytes)?,
                at: word(bytes)?,
            },
            3 => Rpf9Walk::TailNotDeflate { at: word(bytes)? },
            4 => Rpf9Walk::PointersPast {
                section: tagged(&SECTIONS, bytes)?,
                targets: narrow(bytes)?,
            },
            _ => return Err(unreadable("states a walk this build does not know")),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A find of every kind, and a zlib run of every content, role, walk and refusal.
    fn finds() -> Vec<Rpf9ScanFind> {
        let find = |offset, kind| Rpf9ScanFind {
            offset,
            kind,
            span: offset / 2 + 16,
            stored_len: (offset % 3 > 0).then_some(offset + 5),
            inflated_len: (offset % 3 > 1).then_some(u64::MAX - offset),
            container: (offset % 2 > 0).then_some(0x1000),
        };
        let mut head = [0x5Au8; RPF9_HEADER_SIZE];
        head[..4].copy_from_slice(&super::super::RPF9_MAGIC.to_le_bytes());
        let header = Rpf9Header::read(&head).expect("an RPF9 head");
        let mut finds: Vec<Rpf9ScanFind> = [
            Rpf9BodyKind::Archive(header),
            Rpf9BodyKind::StoredRsc8 {
                version: 65,
                extent: 0x2350,
            },
            Rpf9BodyKind::Bink2 {
                revision: b'n',
                length: 0x1_2345,
            },
            Rpf9BodyKind::Awc {
                flags: 0xFF02,
                streams: 20_000,
                chunks: 100_000,
            },
            Rpf9BodyKind::Sgd5 { head_layouts: 3 },
            Rpf9BodyKind::Names { archive: 0x4000 },
            Rpf9BodyKind::FirstEntry {
                head: *b"\0\x01\xFE\xFF",
            },
            Rpf9BodyKind::RawDeflate {
                content: Rpf9Content::Rbf,
            },
        ]
        .into_iter()
        .zip(0x1230..)
        .map(|(kind, offset)| find(offset, kind))
        .collect();
        let walks = [
            Rpf9Walk::Whole,
            Rpf9Walk::NotOnDisk { at: 0x9000 },
            Rpf9Walk::TailNotDeflate { at: u64::MAX },
            Rpf9Walk::PointersPast {
                section: "virtual",
                targets: 2,
            },
            Rpf9Walk::PointersPast {
                section: "physical",
                targets: usize::MAX,
            },
        ]
        .into_iter()
        .chain(REFUSALS.map(|why| Rpf9Walk::Refused { at: 0x77, why }));
        let roles = ROLES.map(Some).into_iter().chain([None]).cycle();
        for (i, (walk, role)) in walks.zip(roles).enumerate() {
            let block = |physical: bool| Rpf9Block {
                offset: 0x2000 + i as u64,
                stored_len: 0x123,
                inflated_len: 0x4567,
                zlib: !physical,
                physical,
            };
            let run = Rpf9ZlibRun {
                content: CONTENTS[i % CONTENTS.len()],
                chunks: i as u32,
                stored_pieces: u32::MAX,
                first_chunk_len: 0x321,
                first_chunk_inflated: 0x1_0000,
                inflated: 0x2_0300,
                signature: Some(Rpf9Signature {
                    classes: *b"0PhPG0p0",
                    role,
                }),
                resource: Some(Rpf9Resource {
                    virtual_blocks: 2,
                    physical_blocks: (i % 3) as u8,
                    blocks: [Some(block(false)), None, Some(block(true)), None],
                    virtual_len: 0x300,
                    physical_len: 0x500,
                    walk,
                }),
            };
            finds.push(find(0x8000 + i as u64, Rpf9BodyKind::Zlib(run)));
            let bare = Rpf9ZlibRun {
                signature: None,
                resource: None,
                ..run
            };
            finds.extend((i == 0).then(|| find(0x7000, Rpf9BodyKind::Zlib(bare))));
        }
        finds
    }

    fn encoded(finds: &[Rpf9ScanFind]) -> Vec<u8> {
        let mut plain = Vec::new();
        finds.iter().for_each(|find| find.encode(&mut plain));
        plain
    }

    /// Every find of `bytes`, which holds finds and nothing else.
    fn decoded(mut bytes: &[u8]) -> io::Result<Vec<Rpf9ScanFind>> {
        let mut finds = Vec::new();
        while !bytes.is_empty() {
            finds.push(Rpf9ScanFind::decode(&mut bytes)?);
        }
        Ok(finds)
    }

    #[test]
    fn a_kept_find_of_every_kind_reads_back_as_it_was() {
        let finds = finds();
        for find in &finds {
            let read = decoded(&encoded(std::slice::from_ref(find)));
            assert_eq!(read.expect("decoded"), [*find]);
        }
        let sealed = seal_rpf9_scan(&encoded(&finds)).expect("sealed");
        let plain = unseal_rpf9_scan(&sealed, 1 << 20).expect("unsealed");
        assert_eq!(decoded(&plain).expect("decoded"), finds);
    }

    #[test]
    fn a_kept_scan_cut_short_or_altered_is_an_error_and_never_a_panic() {
        let plain = encoded(&finds());
        let sealed = seal_rpf9_scan(&plain).expect("sealed");
        assert!(unseal_rpf9_scan(&sealed, plain.len() - 1).is_err());
        for cut in 0..sealed.len() {
            let read = unseal_rpf9_scan(&sealed[..cut], 1 << 20);
            assert!(read.is_err(), "{cut} of {} bytes unsealed", sealed.len());
        }
        // The 16 bytes before the stream and the checksum that ends it are certain to refuse; a
        // byte of the stream may hold bits a zlib stream does not read.
        let certain = |at: usize| at < 16 || at + 4 >= sealed.len();
        for at in 0..sealed.len() {
            let mut altered = sealed.clone();
            altered[at] = !altered[at];
            let read = unseal_rpf9_scan(&altered, 1 << 20);
            assert!(
                read.as_ref().map_or(true, |read| *read == plain),
                "byte {at} altered unsealed as other bytes"
            );
            assert!(read.is_err() || !certain(at), "byte {at} altered unsealed");
        }
        // The finds themselves, were a checksum ever to pass altered bytes.
        let one = encoded(&finds()[..1]);
        for cut in 1..one.len() {
            assert!(decoded(&one[..cut]).is_err(), "{cut} bytes are a find");
        }
        for at in 0..plain.len() {
            let mut altered = plain.clone();
            altered[at] = !altered[at];
            let _ = decoded(&altered);
        }
    }

    #[test]
    fn every_reason_a_walk_refuses_a_block_for_is_one_a_kept_scan_states() {
        /// The first string after a marker, where no `)` closes the call before it.
        fn first_quoted(text: &str) -> Option<&str> {
            let (before, rest) = text.split_once('"')?;
            match before.contains(')') {
                true => None,
                false => rest.split_once('"').map(|(reason, _)| reason),
            }
        }
        let source = include_str!("../rpf9_scan.rs");
        use std::collections::BTreeSet;
        let stated: BTreeSet<&str> = ["why: ", "refused("]
            .into_iter()
            .flat_map(|marker| source.match_indices(marker))
            .filter_map(|(at, marker)| first_quoted(&source[at + marker.len()..]))
            .collect();
        let tagged: BTreeSet<&str> = REFUSALS.into_iter().collect();
        assert_eq!(
            stated, tagged,
            "a reason with no tag, or a tag no walk states"
        );
    }
}
