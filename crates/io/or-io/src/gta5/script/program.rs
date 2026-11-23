use byteorder::{LittleEndian as LE, ReadBytesExt};

use crate::constants::NULL_PTR;
use crate::container::resource::{ResourceEnvelope, ResourceType, Rsc7};
use crate::desc::{Image, Unresolvable};
use crate::error::{IoError, Result};
use crate::gta5::script::opcode::InstructionSet;

/// Bytes in one page of bytecode, of globals, or of the string heap.
///
/// ⚠ **No field states how many pages there are.** The count is
/// `length.div_ceil(CODE_PAGE_SIZE)` over the matching byte count, and the
/// last page holds the remainder. Measured by
/// `ysc_census::what_a_script_container_holds`: every page address a table
/// that rule derives resolves, in 3,022 of 3,022 scripts across a retail
/// Legacy and a retail Enhanced install.
pub const CODE_PAGE_SIZE: usize = 0x4000;

/// Bytes of `scrProgram` at the root of the virtual heap.
const HEADER_SIZE: usize = 0x80;

/// A compiled script: `scrProgram`, and the whole of a `.ysc`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrProgram {
    /// Identifies the global-variable layout this script was compiled against.
    ///
    /// ⚠ Not constant across an install: 20 distinct values over a Legacy
    /// install's 1,158 scripts and 28 over an Enhanced install's 1,864, so a
    /// shipped install holds scripts from several builds at once. That is what
    /// makes a native-name database per *build* rather than per install.
    pub globals_signature: u32,

    pub code_length: u32,

    /// ⚠ **Not the entry point's `ENTER` operand**, which is zero in every
    /// script measured. It counts the arguments the engine writes into the
    /// leading statics before the entry runs, and is non-zero in 595 of 1,158
    /// Legacy scripts.
    pub parameter_count: u32,

    pub globals_count: u32,

    /// The initial value of each static slot. The first
    /// [`parameter_count`](Self::parameter_count) of them are the arguments.
    pub statics: Vec<u64>,

    /// The natives this script calls, **as stored** — rotated per slot.
    ///
    /// Use [`native_hash`](Self::native_hash) rather than reading these
    /// directly; the stored value is not the hash and is not comparable
    /// between two scripts that call the same native.
    pub natives: Vec<u64>,

    pub reserved_48: u64,

    pub reserved_50: u64,

    /// `atStringHash` of [`name`](Self::name), for all 3,022 scripts measured.
    pub name_hash: u32,

    pub ref_count: u32,

    pub name: String,

    pub strings_length: u32,

    pub reserved_74: u32,

    pub reserved_78: u64,

    /// The bytecode, with its pages joined. `code_length` bytes.
    pub code: Vec<u8>,

    /// The string heap, split on its NUL terminators.
    pub strings: Vec<String>,

    /// The string heap as bytes, pages joined. `STRING` addresses it by offset.
    pub string_heap: Vec<u8>,

    /// The RSC7 version the file was sealed at, which names its instruction set.
    pub resource_version: u32,
}

impl ScrProgram {
    /// Read a whole `.ysc`; a version neither install ships is refused.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Rsc7::peek(bytes)?.validate(ResourceType::Ysc)?;
        let opened = Rsc7::open(bytes)?;
        let image = Image::new(&opened.data, opened.virtual_size as usize);
        let mut h = image
            .slice_at(0, HEADER_SIZE)
            .map_err(|why| invalid("the script header", why))?;

        // Read in `scrProgram`'s field order; the first 16 bytes are `pgBase`.
        h.read_u64::<LE>()?;
        h.read_u64::<LE>()?;
        let code_pages = h.read_u64::<LE>()?;
        let globals_signature = h.read_u32::<LE>()?;
        let code_length = h.read_u32::<LE>()?;
        let parameter_count = h.read_u32::<LE>()?;
        let statics_count = h.read_u32::<LE>()?;
        let globals_count = h.read_u32::<LE>()?;
        let natives_count = h.read_u32::<LE>()?;
        let statics = h.read_u64::<LE>()?;
        let _globals_pages = h.read_u64::<LE>()?;
        let natives = h.read_u64::<LE>()?;
        let reserved_48 = h.read_u64::<LE>()?;
        let reserved_50 = h.read_u64::<LE>()?;
        let name_hash = h.read_u32::<LE>()?;
        let ref_count = h.read_u32::<LE>()?;
        let name = h.read_u64::<LE>()?;
        let string_pages = h.read_u64::<LE>()?;
        let strings_length = h.read_u32::<LE>()?;
        let reserved_74 = h.read_u32::<LE>()?;
        let reserved_78 = h.read_u64::<LE>()?;

        let name = match name {
            NULL_PTR => String::new(),
            at => image
                .resolve(at)
                .and_then(|at| image.string_at(at))
                .map_err(|why| invalid("the script's name", why))?,
        };
        let code = read_paged(image, code_pages, code_length as usize, "code")?;
        let string_heap = read_paged(image, string_pages, strings_length as usize, "string heap")?;
        let strings = string_heap
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();

        Ok(Self {
            globals_signature,
            code_length,
            parameter_count,
            globals_count,
            statics: read_slots(image, statics, statics_count as usize, "statics")?,
            natives: read_slots(image, natives, natives_count as usize, "natives")?,
            reserved_48,
            reserved_50,
            name_hash,
            ref_count,
            name,
            strings_length,
            reserved_74,
            reserved_78,
            code,
            strings,
            string_heap,
            resource_version: opened.version,
        })
    }

    /// The hash of native call slot `index`, undoing the per-script rotation.
    ///
    /// ⚠ **The key is the script's own `code_length`**, so the same native is
    /// a different stored value in every script that calls it. Measured over a
    /// retail Legacy install: 653,093 slots hold 161,254 distinct stored
    /// values and 7,402 distinct hashes once rotated back.
    pub fn native_hash(&self, index: usize) -> Option<u64> {
        let stored = *self.natives.get(index)?;
        Some(stored.rotate_left(self.code_length.wrapping_add(index as u32) & 0x3F))
    }

    /// Every native this script calls, by hash, in slot order.
    pub fn native_hashes(&self) -> Vec<u64> {
        (0..self.natives.len())
            .filter_map(|i| self.native_hash(i))
            .collect()
    }

    /// Why OpenRage reports a native as a number when no list is supplied.
    ///
    /// ⚠ **Shipping no names is a decision:** a name comes from a per-build
    /// community list and none carries a licence, so
    /// [`natives`](super::natives) reads one the user supplies instead.
    pub const fn native_names_are_unavailable() -> &'static str {
        "Native calls are reported by hash. A name needs a natives list for the \
         build the script was compiled against, and OpenRage ships none."
    }

    /// The opcode numbering this script's bytecode uses.
    ///
    /// ⚠ **The version names the instruction set, not the edition.** Both
    /// editions ship both versions; what the version decides is whether
    /// opcodes from 94 up sit three higher to make room for `LOCAL_U24`.
    pub fn instruction_set(&self) -> Option<InstructionSet> {
        InstructionSet::for_version(self.resource_version)
    }

    /// How many pages of `length` bytes a page table holds.
    pub const fn pages_for(length: usize) -> usize {
        length.div_ceil(CODE_PAGE_SIZE)
    }

    /// The NUL-terminated string starting at `offset` in the string heap.
    pub fn string_at(&self, offset: usize) -> Option<&[u8]> {
        let rest = self.string_heap.get(offset..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        Some(&rest[..end])
    }
}

fn invalid(what: &str, why: Unresolvable) -> IoError {
    IoError::InvalidData {
        context: format!("script {what}: {why}"),
    }
}

/// `count` contiguous 64-bit slots at `addr`.
fn read_slots(image: Image<'_>, addr: u64, count: usize, what: &str) -> Result<Vec<u64>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut bytes = image
        .resolve(addr)
        .and_then(|at| image.slice_at(at, count.saturating_mul(8)))
        .map_err(|why| invalid(&format!("{what} ({count} slots)"), why))?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(bytes.read_u64::<LE>()?);
    }
    Ok(out)
}

/// Join a page table's pages into one run of `length` bytes.
fn read_paged(image: Image<'_>, table: u64, length: usize, what: &str) -> Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    if table == NULL_PTR {
        return Err(IoError::InvalidData {
            context: format!("script states {length} bytes of {what} behind a null page table"),
        });
    }
    let pages = ScrProgram::pages_for(length);
    let addrs = read_slots(image, table, pages, &format!("{what} page table"))?;

    let mut out = Vec::with_capacity(length);
    for (i, addr) in addrs.into_iter().enumerate() {
        let want = (length - i * CODE_PAGE_SIZE).min(CODE_PAGE_SIZE);
        let page = image
            .resolve(addr)
            .and_then(|at| image.slice_at(at, want))
            .map_err(|why| invalid(&format!("{what} page {i} of {pages}"), why))?;
        out.extend_from_slice(page);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::FIXED_VIRTUAL_BASE;
    use crate::container::resource::{Compression, YSC_FILE_VERSION, YSC_FILE_VERSION_GEN8_ALT};

    /// A sealed script with two code pages, two statics, one native, a name and
    /// a string heap, laid out as the header's field order places them.
    fn script(version: u32, code_length: usize, natives: &[u64]) -> Vec<u8> {
        let code_at = HEADER_SIZE;
        let table_at = code_at + code_length;
        let statics_at = table_at + 8 * ScrProgram::pages_for(code_length);
        let natives_at = statics_at + 16;
        let name_at = natives_at + 8 * natives.len();
        let heap_at = name_at + 8;
        let heap_table_at = heap_at + 8;
        let addr = |at: usize| FIXED_VIRTUAL_BASE + at as u64;

        let mut header = Vec::new();
        for word in [0u64, 0, addr(table_at)] {
            header.extend(word.to_le_bytes());
        }
        let natives_count = natives.len() as u32;
        for word in [7u32, code_length as u32, 1, 2, 0, natives_count] {
            header.extend(word.to_le_bytes());
        }
        for word in [addr(statics_at), 0, addr(natives_at), 0, 0] {
            header.extend(word.to_le_bytes());
        }
        header.extend(0xABCDu32.to_le_bytes());
        header.extend(1u32.to_le_bytes());
        header.extend(addr(name_at).to_le_bytes());
        header.extend(addr(heap_table_at).to_le_bytes());
        header.extend(6u32.to_le_bytes());
        header.extend([0; 12]);
        assert_eq!(header.len(), HEADER_SIZE);

        let mut payload = header;
        payload.extend((0..code_length).map(|i| i as u8));
        for page in 0..ScrProgram::pages_for(code_length) {
            payload.extend(addr(code_at + page * CODE_PAGE_SIZE).to_le_bytes());
        }
        payload.extend(5u64.to_le_bytes());
        payload.extend(6u64.to_le_bytes());
        for native in natives {
            payload.extend(native.to_le_bytes());
        }
        payload.extend(b"main\0\0\0\0");
        payload.extend(b"ab\0cd\0\0\0");
        payload.extend(addr(heap_at).to_le_bytes());

        let seal = Rsc7::invent_single_chunk_seal(version, payload.len()).expect("a seal");
        payload.resize(seal.total_size() as usize, 0);
        Rsc7::reseal(&payload, &seal, Compression::fast()).expect("sealed")
    }

    #[test]
    fn a_script_reads_its_pages_statics_natives_and_strings() {
        let code_length = CODE_PAGE_SIZE + 3;
        let program = ScrProgram::from_bytes(&script(YSC_FILE_VERSION, code_length, &[9, 10]))
            .expect("reads");
        assert_eq!(program.code.len(), code_length);
        assert_eq!(program.code[CODE_PAGE_SIZE], (CODE_PAGE_SIZE % 256) as u8);
        assert_eq!(program.statics, [5, 6]);
        assert_eq!(program.natives, [9, 10]);
        assert_eq!(program.name, "main");
        assert_eq!(program.name_hash, 0xABCD);
        assert_eq!(program.globals_signature, 7);
        assert_eq!(program.parameter_count, 1);
        assert_eq!(program.strings, ["ab", "cd"]);
        assert_eq!(program.string_at(3), Some(&b"cd"[..]));
        assert_eq!(program.resource_version, YSC_FILE_VERSION);
    }

    /// ⚠ Both numbers a retail install ships, and neither names a generation.
    #[test]
    fn both_shipped_versions_open_and_a_third_does_not() {
        for version in [YSC_FILE_VERSION, YSC_FILE_VERSION_GEN8_ALT] {
            let program = ScrProgram::from_bytes(&script(version, 4, &[]));
            assert!(program.is_ok(), "version {version}: {program:?}");
        }
        let why = ScrProgram::from_bytes(&script(11, 4, &[]))
            .err()
            .map(|e| e.to_string())
            .expect("version 11 ships in neither install");
        assert!(why.contains("11"), "{why}");
    }

    #[test]
    fn a_page_table_past_the_heap_is_refused() {
        let mut bytes = script(YSC_FILE_VERSION, 4, &[]);
        let opened = Rsc7::open(&bytes).expect("opens");
        let mut payload = opened.data;
        payload[0x10..0x18].copy_from_slice(&(FIXED_VIRTUAL_BASE + 0x0FF0_0000).to_le_bytes());
        bytes = Rsc7::reseal(&payload, &opened.seal, Compression::fast()).expect("sealed");
        let why = ScrProgram::from_bytes(&bytes)
            .expect_err("refused")
            .to_string();
        assert!(why.contains("code page table"), "{why}");
    }

    /// The page rule, which no field states, at each of its edges.
    #[test]
    fn a_page_count_is_the_byte_count_rounded_up() {
        assert_eq!(ScrProgram::pages_for(0), 0);
        assert_eq!(ScrProgram::pages_for(1), 1);
        assert_eq!(ScrProgram::pages_for(CODE_PAGE_SIZE), 1);
        assert_eq!(ScrProgram::pages_for(CODE_PAGE_SIZE + 1), 2);
        // The largest script measured in a retail Legacy install.
        assert_eq!(ScrProgram::pages_for(485_953), 30);
    }

    /// ⚠ The rotation is by `code_length + index`, and getting either half
    /// wrong still returns a plausible 64-bit number. Both are exercised
    /// here against a value computed by the inverse rotation rather than
    /// transcribed.
    #[test]
    fn a_native_hash_is_the_slot_rotated_by_its_own_index() {
        let hash: u64 = 0x1234_5678_9ABC_DEF0;
        let code_length: u32 = 186_539;
        let program = ScrProgram {
            code_length,
            natives: (0..4)
                .map(|i| hash.rotate_right(code_length.wrapping_add(i) & 0x3F))
                .collect(),
            ..ScrProgram::default()
        };

        for i in 0..4 {
            assert_eq!(program.native_hash(i), Some(hash), "slot {i}");
        }
        assert_eq!(program.native_hash(4), None);

        // The stored values differ from one another, which is what makes the
        // de-rotation load-bearing rather than a formality.
        let distinct: std::collections::HashSet<u64> = program.natives.iter().copied().collect();
        assert_eq!(distinct.len(), 4);
    }

    #[test]
    fn a_script_says_why_it_cannot_name_a_native() {
        let why = ScrProgram::native_names_are_unavailable();
        assert!(why.contains("hash"), "{why}");
        assert!(why.contains("ships none"), "{why}");
    }
}
