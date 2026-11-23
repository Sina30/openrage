//! GTA VI's pipeline cache, a `PS` body: a header, graphics and compute
//! pipeline records, and the metadata and code blobs they reference.
//!
//! The code blobs are native GPU code of the console the cache was built for, so only the
//! structure is read: every count, index and reference closes, and every metadata blob holds to
//! its role's size rule. Read-only.

use std::collections::BTreeMap;

const MAGIC: &[u8; 4] = b"PS\x00\x09";
const HEADER: usize = 0x80;
const GRAPHICS: usize = 0x70;
const COMPUTE: usize = 24;
const INDEX: usize = 8;
const SLOTS: usize = 7;
const NONE: u32 = u32::MAX;
const CODE_ALIGN: u32 = 256;

/// Why a pipeline cache, or a part of one, is not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineCacheError {
    #[error("at {at:#X} the pipeline cache does not hold to the measured layout: {why}")]
    Invalid { at: usize, why: String },
    #[error("a code blob is native GPU code of the console the cache was built for: no other device runs it and nothing translates it, so it is not read")]
    NativeCode,
    #[error("a metadata blob's fields past its size rule are not decoded, so it is not read")]
    MetadataNotDecoded,
    #[error("saving a pipeline cache is refused: OpenRage reads GTA VI's files and writes none, and how a pipeline's key is formed is not known")]
    ReadOnly,
}

type Read<T> = Result<T, PipelineCacheError>;

/// A read cache: its header facts and its pipelines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineCache {
    pub version: u8,
    /// The NUL-padded text at 0x04, whose meaning is not known.
    pub ident: String,
    /// The NUL-padded text at 0x24, a tool or build version.
    pub build: String,
    /// The u64 at 0x48, whose meaning is not known.
    pub key: u64,
    /// The words at 0x60, high word first.
    pub stamp: u64,
    pub graphics: Vec<GraphicsPipeline>,
    pub compute: Vec<ComputePipeline>,
    pub metadata_blobs: usize,
    pub code_blobs: usize,
    /// Where each part of the body starts, in order.
    pub sections: [(&'static str, usize); 7],
    pub tail_len: usize,
    /// Metadata blobs per role, each of which holds to its role's size rule.
    pub roles: BTreeMap<String, usize>,
}

/// One graphics pipeline: its key, two metadata blobs and up to seven stage slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphicsPipeline {
    pub key: u64,
    pub layout: u32,
    /// The metadata blob that holds the vertex input layout.
    pub state: u32,
    pub hash128: [u8; 16],
    pub state_keys: [u64; 3],
    /// Slot 0 the pixel stage, 1 the vertex stage, 2 a mesh-type stage.
    pub slots: [Option<Slot>; SLOTS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputePipeline {
    pub key: u64,
    pub layout: u32,
    pub state: u32,
    pub slot: Option<Slot>,
}

/// A stage: its code blob, and its metadata blob where it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub code: u32,
    pub metadata: Option<u32>,
}

impl GraphicsPipeline {
    /// `x` per slot that holds a stage, `-` per empty one.
    pub fn pattern(&self) -> String {
        self.slots
            .iter()
            .map(|s| if s.is_some() { 'x' } else { '-' })
            .collect()
    }
}

impl PipelineCache {
    /// The inflated body in `bytes`.
    pub fn open(bytes: &[u8]) -> Result<Self, PipelineCacheError> {
        let b = bytes;
        let head = b
            .get(..HEADER)
            .ok_or_else(|| invalid(0, "shorter than its header"))?;
        if &head[..4] != MAGIC {
            return Err(invalid(0, "expected `PS`, 0 and version 9"));
        }
        if head[0x68..HEADER].iter().any(|x| *x != 0) {
            return Err(invalid(0x68, "header bytes 0x68 to 0x80 are not zero"));
        }
        let text = |at: usize, len: usize| {
            let field = &head[at..at + len];
            let end = field.iter().position(|c| *c == 0).unwrap_or(len);
            String::from_utf8_lossy(&field[..end]).into_owned()
        };
        let count = |at: usize| dword(head, at) as usize;
        let (graphics_count, compute_count) = (count(0x50), count(0x54));
        let (meta_count, code_count) = (count(0x58), count(0x5C));
        let tables = [GRAPHICS, COMPUTE, INDEX, INDEX];
        let mut starts = [HEADER as u64; 5];
        for (k, (n, stride)) in [graphics_count, compute_count, meta_count, code_count]
            .iter()
            .zip(tables)
            .enumerate()
        {
            starts[k + 1] = starts[k] + *n as u64 * stride as u64;
        }
        if starts[4] > b.len() as u64 {
            return Err(invalid(0x50, "its counts state tables past the body's end"));
        }
        let [_, compute_at, meta_index_at, code_index_at, meta_at] = starts.map(|s| s as usize);
        let index = |at: usize, n: usize| -> Vec<(u32, u32)> {
            (0..n)
                .map(|k| (dword(b, at + INDEX * k), dword(b, at + INDEX * k + 4)))
                .collect()
        };
        let (meta, code) = (
            index(meta_index_at, meta_count),
            index(code_index_at, code_count),
        );
        let mut meta_end = 0u64;
        for (k, (offset, size)) in meta.iter().enumerate() {
            if u64::from(*offset) != meta_end {
                return Err(invalid(
                    meta_index_at + INDEX * k,
                    "the metadata pool is not contiguous",
                ));
            }
            meta_end += u64::from(*size);
        }
        let mut code_end = 0u64;
        for (k, (offset, size)) in code.iter().enumerate() {
            if offset % CODE_ALIGN != 0 || u64::from(*offset) < code_end || (k == 0 && *offset != 0)
            {
                return Err(invalid(
                    code_index_at + INDEX * k,
                    "code blobs are not 256-aligned, in order and apart",
                ));
            }
            code_end = u64::from(*offset) + u64::from(*size);
        }
        let code_at = meta_at as u64 + meta_end;
        let tail_at = code_at + code_end;
        if tail_at > b.len() as u64 {
            return Err(invalid(
                code_index_at,
                "the code pool runs past the body's end",
            ));
        }
        let (code_at, tail_at) = (code_at as usize, tail_at as usize);
        if b[tail_at..].iter().any(|x| *x != 0) {
            return Err(invalid(
                tail_at,
                "bytes after the last code blob are not zero",
            ));
        }

        let slot = |code: u32, metadata: u32, at: usize| -> Read<Option<Slot>> {
            match (code, metadata) {
                (NONE, NONE) => Ok(None),
                (NONE, _) => Err(invalid(at, "a slot names metadata and no code")),
                (code, metadata) => Ok(Some(Slot {
                    code,
                    metadata: (metadata != NONE).then_some(metadata),
                })),
            }
        };
        let graphics = (0..graphics_count)
            .map(|i| {
                let r = HEADER + GRAPHICS * i;
                let mut slots = [None; SLOTS];
                for (s, out) in slots.iter_mut().enumerate() {
                    let at = r + 0x38 + 8 * s;
                    *out = slot(dword(b, at), dword(b, at + 4), at)?;
                }
                let mut hash128 = [0; 16];
                hash128.copy_from_slice(&b[r + 0x10..r + 0x20]);
                Ok(GraphicsPipeline {
                    key: qword(b, r),
                    layout: dword(b, r + 8),
                    state: dword(b, r + 12),
                    hash128,
                    state_keys: [qword(b, r + 0x20), qword(b, r + 0x28), qword(b, r + 0x30)],
                    slots,
                })
            })
            .collect::<Read<Vec<_>>>()?;
        let compute = (0..compute_count)
            .map(|i| {
                let r = compute_at + COMPUTE * i;
                Ok(ComputePipeline {
                    key: qword(b, r),
                    layout: dword(b, r + 8),
                    state: dword(b, r + 12),
                    slot: slot(dword(b, r + 16), dword(b, r + 20), r + 16)?,
                })
            })
            .collect::<Read<Vec<_>>>()?;

        let roles = check_references(&graphics, &compute, meta_count, code_count)?;
        let mut held = BTreeMap::new();
        for (k, role) in roles.iter().enumerate() {
            let (offset, size) = meta[k];
            let start = meta_at + offset as usize;
            let blob = &b[start..start + size as usize];
            if !role.holds(blob) {
                return Err(invalid(
                    start,
                    format!(
                        "metadata blob {k} breaks the size rule of a {}",
                        role.label()
                    ),
                ));
            }
            *held.entry(role.label()).or_insert(0) += 1;
        }
        let mut keys: Vec<u64> = (graphics.iter().map(|g| g.key))
            .chain(compute.iter().map(|c| c.key))
            .collect();
        keys.sort_unstable();
        if keys.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid(HEADER, "two pipelines have one key"));
        }
        Ok(Self {
            version: head[3],
            ident: text(0x04, 0x20),
            build: text(0x24, 0x24),
            key: qword(head, 0x48),
            stamp: u64::from(dword(head, 0x60)) << 32 | u64::from(dword(head, 0x64)),
            graphics,
            compute,
            metadata_blobs: meta_count,
            code_blobs: code_count,
            sections: [
                ("graphics", HEADER),
                ("compute", compute_at),
                ("metadata index", meta_index_at),
                ("code index", code_index_at),
                ("metadata", meta_at),
                ("code", code_at),
                ("tail", tail_at),
            ],
            tail_len: b.len() - tail_at,
            roles: held,
        })
    }

    /// Always refused: see [`PipelineCacheError::NativeCode`].
    pub fn code(&self, _index: usize) -> Result<&[u8], PipelineCacheError> {
        Err(PipelineCacheError::NativeCode)
    }

    /// Always refused: see [`PipelineCacheError::MetadataNotDecoded`].
    pub fn metadata(&self, _index: usize) -> Result<&[u8], PipelineCacheError> {
        Err(PipelineCacheError::MetadataNotDecoded)
    }

    /// Always refused: see [`PipelineCacheError::ReadOnly`].
    pub fn save(&self) -> Result<Vec<u8>, PipelineCacheError> {
        Err(PipelineCacheError::ReadOnly)
    }
}

/// The one role a metadata blob plays, by the record field that references it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    GraphicsLayout,
    GraphicsState,
    GraphicsSlot(u8),
    ComputeLayout,
    ComputeState,
    ComputeSlot,
}

impl Role {
    fn label(self) -> String {
        match self {
            Self::GraphicsLayout => "graphics layout".into(),
            Self::GraphicsState => "graphics state".into(),
            Self::GraphicsSlot(s) => format!("graphics slot {s}"),
            Self::ComputeLayout => "compute layout".into(),
            Self::ComputeState => "compute state".into(),
            Self::ComputeSlot => "compute slot".into(),
        }
    }

    /// Whether `blob`'s size is what its own fields state for this role.
    fn holds(self, blob: &[u8]) -> bool {
        let n = blob.len();
        let word = |at: usize| (at + 4 <= n).then(|| dword(blob, at));
        match self {
            Self::GraphicsLayout => {
                word(0x28).is_some_and(|w| n == 28 + (w & 0xFFFF) as usize + (w >> 16) as usize)
            }
            Self::GraphicsState => {
                if n < 0x50 || word(4) != Some(0x1000) {
                    return false;
                }
                // Packed elements: u32 length, a name of that many bytes ending in NUL, 24 bytes.
                let mut at = 0x50;
                for _ in 0..word(0x4C).unwrap_or(0) {
                    let Some(len) = word(at)
                        .map(|l| l as usize)
                        .filter(|l| (1..=64).contains(l))
                    else {
                        return false;
                    };
                    if at + 4 + len + 24 > n || blob[at + 3 + len] != 0 {
                        return false;
                    }
                    at += 4 + len + 24;
                }
                n == at.next_multiple_of(4)
            }
            Self::ComputeLayout => n == 28,
            Self::ComputeState => n == 20,
            Self::GraphicsSlot(_) | Self::ComputeSlot => {
                let (head, magic) = match self {
                    Self::ComputeSlot => (24, 0x403),
                    _ => (16, 2),
                };
                match (word(0), word(4), word(head - 4)) {
                    (Some(m), Some(len), Some(w))
                        if m == magic && len as usize + head - 4 == n && w & 0x7FFF == 0x440 =>
                    {
                        let items = 2 * (w >> 16) as usize + ((w >> 15) & 1) as usize;
                        n == head + 32 * items
                    }
                    _ => false,
                }
            }
        }
    }
}

/// Each metadata blob's role, every reference in range, every blob of both pools referenced, and
/// each metadata blob in one role and each code blob in one slot role.
fn check_references(
    graphics: &[GraphicsPipeline],
    compute: &[ComputePipeline],
    meta_count: usize,
    code_count: usize,
) -> Read<Vec<Role>> {
    let mut meta: Vec<Option<Role>> = vec![None; meta_count];
    let mut code: Vec<Option<Role>> = vec![None; code_count];
    let mut stage = |slot: &Option<Slot>, role: Role| -> Read<()> {
        let Some(slot) = slot else { return Ok(()) };
        claim(&mut code, slot.code, role)?;
        slot.metadata
            .map_or(Ok(()), |index| claim(&mut meta, index, role))
    };
    for g in graphics {
        for (s, slot) in g.slots.iter().enumerate() {
            stage(slot, Role::GraphicsSlot(s as u8))?;
        }
    }
    for c in compute {
        stage(&c.slot, Role::ComputeSlot)?;
    }
    for g in graphics {
        claim(&mut meta, g.layout, Role::GraphicsLayout)?;
        claim(&mut meta, g.state, Role::GraphicsState)?;
    }
    for c in compute {
        claim(&mut meta, c.layout, Role::ComputeLayout)?;
        claim(&mut meta, c.state, Role::ComputeState)?;
    }
    if meta.iter().chain(&code).any(Option::is_none) {
        return Err(invalid(HEADER, "a blob no pipeline references"));
    }
    Ok(meta.into_iter().flatten().collect())
}

/// Blob `index` of `pool` given `role`, which must be the only role it has.
fn claim(pool: &mut [Option<Role>], index: u32, role: Role) -> Read<()> {
    let what = |why: &str| invalid(HEADER, format!("{} blob {index}: {why}", role.label()));
    let entry = pool
        .get_mut(index as usize)
        .ok_or_else(|| what("past its pool"))?;
    match entry {
        Some(held) if *held != role => Err(what("in two roles")),
        _ => {
            *entry = Some(role);
            Ok(())
        }
    }
}

fn invalid(at: usize, why: impl Into<String>) -> PipelineCacheError {
    PipelineCacheError::Invalid {
        at,
        why: why.into(),
    }
}

fn dword(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn qword(b: &[u8], at: usize) -> u64 {
    u64::from(dword(b, at)) | u64::from(dword(b, at + 4)) << 32
}

#[cfg(test)]
#[path = "pipeline_cache_tests.rs"]
mod tests;
