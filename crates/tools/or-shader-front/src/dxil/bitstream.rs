//! The LLVM bitstream: abbreviations, `BLOCKINFO`, blocks and records, read whole into an ordered
//! tree, because value numbering depends on the order of records and sub-blocks.

use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BitcodeError {
    #[error("no LLVM bitcode magic")]
    NoMagic,
    #[error("the bitcode ends inside a {what} at bit {bit}")]
    Truncated { what: &'static str, bit: u64 },
    #[error("abbreviation {id} is not defined at bit {bit}")]
    NoAbbrev { id: u32, bit: u64 },
    #[error("blocks nest deeper than {0}")]
    TooDeep(usize),
    /// A record the module reader cannot interpret; block and code are LLVM's numbers.
    #[error("record {code} of block {block} is not read: {why}")]
    Record { block: u32, code: u32, why: String },
    #[error("{0} is not read by this front end")]
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub code: u32,
    pub ops: Vec<u64>,
    /// A blob operand's bytes, where the abbreviation has one.
    pub blob: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Record(Record),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: u32,
    pub items: Vec<Item>,
}

const MAGIC: [u8; 4] = [0x42, 0x43, 0xC0, 0xDE];
const END_BLOCK: u64 = 0;
const ENTER_SUBBLOCK: u64 = 1;
const DEFINE_ABBREV: u64 = 2;
const UNABBREV_RECORD: u64 = 3;
const FIRST_APPLICATION_ABBREV: u64 = 4;
const BLOCKINFO: u32 = 0;
const SETBID: u32 = 1;
/// DXIL nests three or four blocks deep; anything far deeper is a malformed stream.
const MAX_DEPTH: usize = 16;

/// One operand of an abbreviation; `Array` takes the operand after it as its element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Literal(u64),
    Fixed(u32),
    Vbr(u32),
    Array,
    Char6,
    Blob,
}

type Abbrev = Rc<[Op]>;

/// The top-level blocks of `bitcode` (the module block among them), `BLOCKINFO` applied and dropped.
pub fn read(bitcode: &[u8]) -> Result<Vec<Block>, BitcodeError> {
    if bitcode.get(..4) != Some(&MAGIC[..]) {
        return Err(BitcodeError::NoMagic);
    }
    let mut reader = Reader {
        data: bitcode,
        pos: 32,
        len: bitcode.len() as u64 * 8,
        blockinfo: HashMap::new(),
    };
    let mut blocks = Vec::new();
    // The top level is 32-bit aligned between blocks; zero padding after the last one is the end.
    while !reader.rest_is_zero() {
        let at = reader.pos;
        let id = reader.fixed(2, "abbreviation id")?;
        if id != ENTER_SUBBLOCK {
            return Err(BitcodeError::NoAbbrev {
                id: id as u32,
                bit: at,
            });
        }
        let block = reader.enter(1)?;
        if block.id != BLOCKINFO {
            blocks.push(block);
        }
    }
    Ok(blocks)
}

struct Reader<'a> {
    data: &'a [u8],
    /// In bits.
    pos: u64,
    /// In bits.
    len: u64,
    /// Abbreviations `BLOCKINFO` gave each block id, copied into a block when it is entered.
    blockinfo: HashMap<u32, Vec<Abbrev>>,
}

impl Reader<'_> {
    fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    fn rest_is_zero(&self) -> bool {
        self.pos >= self.len || self.data[(self.pos / 8) as usize..].iter().all(|b| *b == 0)
    }

    fn align32(&mut self) {
        self.pos = (self.pos + 31) & !31;
    }

    /// `width` is at most 64.
    fn fixed(&mut self, width: u32, what: &'static str) -> Result<u64, BitcodeError> {
        if self.pos + u64::from(width) > self.len {
            return Err(truncated(what, self.pos));
        }
        let mut value = 0u64;
        let mut got = 0u32;
        while got < width {
            let byte = self.data[(self.pos / 8) as usize];
            let shift = (self.pos % 8) as u32;
            let take = (8 - shift).min(width - got);
            let bits = (u64::from(byte) >> shift) & ((1u64 << take) - 1);
            value |= bits << got;
            got += take;
            self.pos += u64::from(take);
        }
        Ok(value)
    }

    /// `width` is 2 to 32.
    fn vbr(&mut self, width: u32, what: &'static str) -> Result<u64, BitcodeError> {
        let at = self.pos;
        let high = 1u64 << (width - 1);
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let chunk = self.fixed(width, what)?;
            let payload = chunk & (high - 1);
            if payload != 0 {
                if shift >= 64 || (shift > 0 && payload >> (64 - shift) != 0) {
                    return Err(unsupported(format!("a VBR over 64 bits at bit {at}")));
                }
                value |= payload << shift;
            }
            if chunk & high == 0 {
                return Ok(value);
            }
            shift = shift.saturating_add(width - 1);
        }
    }

    /// Reads a block header after its `ENTER_SUBBLOCK` id, then the block.
    fn enter(&mut self, depth: usize) -> Result<Block, BitcodeError> {
        let at = self.pos;
        let id = self.vbr(8, "block header")?;
        let width = self.vbr(4, "block header")?;
        self.align32();
        let words = self.fixed(32, "block header")?;
        if words * 32 > self.remaining() {
            return Err(truncated("block", at));
        }
        let Ok(id) = u32::try_from(id) else {
            return Err(unsupported(format!("block id {id} at bit {at}")));
        };
        if !(1..=32).contains(&width) {
            return Err(unsupported(format!("an abbreviation width of {width}")));
        }
        self.block(id, width as u32, depth)
    }

    fn block(&mut self, id: u32, width: u32, depth: usize) -> Result<Block, BitcodeError> {
        if depth > MAX_DEPTH {
            return Err(BitcodeError::TooDeep(MAX_DEPTH));
        }
        let mut abbrevs = self.blockinfo.get(&id).cloned().unwrap_or_default();
        let mut items = Vec::new();
        // In `BLOCKINFO`, the block id the abbreviations that follow belong to.
        let mut target: Option<u32> = None;
        loop {
            let at = self.pos;
            let record = match self.fixed(width, "abbreviation id")? {
                END_BLOCK => {
                    self.align32();
                    return Ok(Block { id, items });
                }
                ENTER_SUBBLOCK => {
                    let sub = self.enter(depth + 1)?;
                    if sub.id != BLOCKINFO {
                        items.push(Item::Block(sub));
                    }
                    continue;
                }
                DEFINE_ABBREV => {
                    let abbrev = self.define()?;
                    if id == BLOCKINFO {
                        let Some(target) = target else {
                            return Err(unsupported("BLOCKINFO before its SETBID".to_string()));
                        };
                        self.blockinfo.entry(target).or_default().push(abbrev);
                    } else {
                        abbrevs.push(abbrev);
                    }
                    continue;
                }
                UNABBREV_RECORD => self.unabbreviated(id)?,
                n => {
                    let index = (n - FIRST_APPLICATION_ABBREV) as usize;
                    let Some(abbrev) = abbrevs.get(index).cloned() else {
                        return Err(BitcodeError::NoAbbrev {
                            id: n as u32,
                            bit: at,
                        });
                    };
                    self.abbreviated(&abbrev, id)?
                }
            };
            if id == BLOCKINFO && record.code == SETBID {
                let block = record.ops.first().and_then(|b| u32::try_from(*b).ok());
                let Some(block) = block else {
                    return Err(BitcodeError::Record {
                        block: BLOCKINFO,
                        code: SETBID,
                        why: "SETBID names no block id".to_string(),
                    });
                };
                target = Some(block);
            }
            items.push(Item::Record(record));
        }
    }

    fn define(&mut self) -> Result<Abbrev, BitcodeError> {
        let at = self.pos;
        let count = self.vbr(5, "abbreviation definition")?;
        if count > self.remaining() {
            return Err(truncated("abbreviation definition", at));
        }
        let mut ops = Vec::new();
        for _ in 0..count {
            if self.fixed(1, "abbreviation definition")? == 1 {
                ops.push(Op::Literal(self.vbr(8, "abbreviation definition")?));
                continue;
            }
            let op = match self.fixed(3, "abbreviation definition")? {
                encoding @ (1 | 2) => {
                    let width = self.vbr(5, "abbreviation definition")?;
                    match (encoding, width) {
                        (_, 0) => Op::Literal(0),
                        (1, 1..=64) => Op::Fixed(width as u32),
                        (2, 2..=32) => Op::Vbr(width as u32),
                        _ => return Err(unsupported(format!("encoding {encoding} width {width}"))),
                    }
                }
                3 => Op::Array,
                4 => Op::Char6,
                5 => Op::Blob,
                encoding => return Err(unsupported(format!("abbreviation encoding {encoding}"))),
            };
            ops.push(op);
        }
        // The code comes first and is a scalar; an array is the last but one, of a scalar element
        // that reads bits: a literal element would let a record of no input hold one per bit left.
        let aggregate = |op: &Op| matches!(op, Op::Array | Op::Blob);
        let bad_array = |(i, op): (usize, &Op)| {
            *op == Op::Array
                && (i + 2 != ops.len()
                    || aggregate(&ops[i + 1])
                    || matches!(ops[i + 1], Op::Literal(_)))
        };
        if ops.first().is_none_or(aggregate) || ops.iter().enumerate().any(bad_array) {
            return Err(unsupported(format!("a bad abbreviation at bit {at}")));
        }
        Ok(Rc::from(ops))
    }

    fn scalar(&mut self, op: Op) -> Result<u64, BitcodeError> {
        match op {
            Op::Literal(value) => Ok(value),
            Op::Fixed(width) => self.fixed(width, "record"),
            Op::Vbr(width) => self.vbr(width, "record"),
            Op::Char6 => Ok(char6(self.fixed(6, "record")?)),
            Op::Array | Op::Blob => Err(unsupported(format!("an aggregate at bit {}", self.pos))),
        }
    }

    fn code(code: u64, block: u32) -> Result<u32, BitcodeError> {
        match u32::try_from(code) {
            Ok(code) => Ok(code),
            Err(_) => Err(unsupported(format!("code {code} in block {block}"))),
        }
    }

    fn unabbreviated(&mut self, block: u32) -> Result<Record, BitcodeError> {
        let at = self.pos;
        let code = Self::code(self.vbr(6, "record")?, block)?;
        let count = self.vbr(6, "record")?;
        if count > self.remaining() {
            return Err(truncated("record", at));
        }
        let mut ops = Vec::new();
        for _ in 0..count {
            ops.push(self.vbr(6, "record")?);
        }
        Ok(Record {
            code,
            ops,
            blob: None,
        })
    }

    fn abbreviated(&mut self, abbrev: &[Op], block: u32) -> Result<Record, BitcodeError> {
        // `define` holds the first operand to a scalar and an array's element to the one after it.
        let code = Self::code(self.scalar(abbrev[0])?, block)?;
        let mut ops = Vec::new();
        let mut blob = None;
        let mut i = 1;
        while i < abbrev.len() {
            match abbrev[i] {
                Op::Array => {
                    let at = self.pos;
                    let count = self.vbr(6, "array")?;
                    if count > self.remaining() {
                        return Err(truncated("array", at));
                    }
                    let element = abbrev[i + 1];
                    for _ in 0..count {
                        ops.push(self.scalar(element)?);
                    }
                    i += 2;
                    continue;
                }
                Op::Blob => {
                    let at = self.pos;
                    let count = self.vbr(6, "blob")?;
                    self.align32();
                    if self.pos > self.len || count > self.remaining() / 8 {
                        return Err(truncated("blob", at));
                    }
                    let start = (self.pos / 8) as usize;
                    blob = Some(self.data[start..start + count as usize].to_vec());
                    self.pos += count * 8;
                    self.align32();
                }
                op => ops.push(self.scalar(op)?),
            }
            i += 1;
        }
        Ok(Record { code, ops, blob })
    }
}

fn truncated(what: &'static str, bit: u64) -> BitcodeError {
    BitcodeError::Truncated { what, bit }
}

fn unsupported(what: String) -> BitcodeError {
    BitcodeError::Unsupported(what)
}

fn char6(value: u64) -> u64 {
    let c = match value {
        0..=25 => b'a' + value as u8,
        26..=51 => b'A' + (value - 26) as u8,
        52..=61 => b'0' + (value - 52) as u8,
        62 => b'.',
        _ => b'_',
    };
    u64::from(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bitstream writer, LSB first, as LLVM writes it.
    #[derive(Default)]
    struct Bits {
        bytes: Vec<u8>,
        bit: u64,
    }

    impl Bits {
        fn new() -> Self {
            let mut bits = Self::default();
            for byte in MAGIC {
                bits.fixed(u64::from(byte), 8);
            }
            bits
        }

        fn fixed(&mut self, value: u64, width: u32) {
            for i in 0..width {
                if self.bit.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (value >> i) & 1 == 1 {
                    let last = self.bytes.len() - 1;
                    self.bytes[last] |= 1 << (self.bit % 8);
                }
                self.bit += 1;
            }
        }

        fn vbr(&mut self, mut value: u64, width: u32) {
            let high = 1u64 << (width - 1);
            while value >= high {
                self.fixed((value & (high - 1)) | high, width);
                value >>= width - 1;
            }
            self.fixed(value, width);
        }

        fn align32(&mut self) {
            while !self.bit.is_multiple_of(32) {
                self.fixed(0, 1);
            }
        }

        /// Opens a block; the returned index is its length word, which `end` fills.
        fn enter(&mut self, width: u32, id: u64, inner: u32) -> usize {
            self.fixed(ENTER_SUBBLOCK, width);
            self.vbr(id, 8);
            self.vbr(u64::from(inner), 4);
            self.align32();
            let at = self.bytes.len();
            self.fixed(0, 32);
            at
        }

        fn end(&mut self, width: u32, at: usize) {
            self.fixed(END_BLOCK, width);
            self.align32();
            let words = ((self.bytes.len() - at - 4) / 4) as u32;
            self.bytes[at..at + 4].copy_from_slice(&words.to_le_bytes());
        }

        fn define(&mut self, width: u32, ops: &[Op]) {
            self.fixed(DEFINE_ABBREV, width);
            self.vbr(ops.len() as u64, 5);
            for op in ops {
                match *op {
                    Op::Literal(value) => {
                        self.fixed(1, 1);
                        self.vbr(value, 8);
                    }
                    Op::Fixed(width) => {
                        self.fixed(0, 1);
                        self.fixed(1, 3);
                        self.vbr(u64::from(width), 5);
                    }
                    Op::Vbr(width) => {
                        self.fixed(0, 1);
                        self.fixed(2, 3);
                        self.vbr(u64::from(width), 5);
                    }
                    Op::Array => {
                        self.fixed(0, 1);
                        self.fixed(3, 3);
                    }
                    Op::Char6 => {
                        self.fixed(0, 1);
                        self.fixed(4, 3);
                    }
                    Op::Blob => {
                        self.fixed(0, 1);
                        self.fixed(5, 3);
                    }
                }
            }
        }

        fn unabbreviated(&mut self, width: u32, code: u64, ops: &[u64]) {
            self.fixed(UNABBREV_RECORD, width);
            self.vbr(code, 6);
            self.vbr(ops.len() as u64, 6);
            for op in ops {
                self.vbr(*op, 6);
            }
        }
    }

    fn char6_code(c: u8) -> u64 {
        u64::from(match c {
            b'a'..=b'z' => c - b'a',
            b'A'..=b'Z' => c - b'A' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'.' => 62,
            _ => 63,
        })
    }

    const TEXT: &[u8] = b"aZ9._";
    const BLOB: [u8; 5] = [0x11, 0x22, 0x33, 0x44, 0x55];
    const WIDE: u64 = 1 << 40;

    /// Every encoding, an abbreviation from `BLOCKINFO`, an unabbreviated record and a sub-block.
    fn stream() -> Vec<u8> {
        let mut w = Bits::new();
        let info = w.enter(2, u64::from(BLOCKINFO), 3);
        w.unabbreviated(3, u64::from(SETBID), &[8]);
        w.define(3, &[Op::Literal(5), Op::Fixed(7), Op::Vbr(4)]);
        w.end(3, info);

        let module = w.enter(2, 8, 4);
        w.define(4, &[Op::Fixed(3), Op::Array, Op::Char6]);
        w.define(4, &[Op::Literal(9), Op::Blob]);
        // Abbreviation 4, from BLOCKINFO.
        w.fixed(4, 4);
        w.fixed(100, 7);
        w.vbr(1000, 4);
        // Abbreviation 5.
        w.fixed(5, 4);
        w.fixed(6, 3);
        w.vbr(TEXT.len() as u64, 6);
        for c in TEXT {
            w.fixed(char6_code(*c), 6);
        }
        // Abbreviation 6.
        w.fixed(6, 4);
        w.vbr(BLOB.len() as u64, 6);
        w.align32();
        for b in BLOB {
            w.fixed(u64::from(b), 8);
        }
        w.align32();
        w.unabbreviated(4, 7, &[3, WIDE]);
        let sub = w.enter(4, 12, 2);
        w.unabbreviated(2, 2, &[0]);
        w.end(2, sub);
        w.end(4, module);
        w.bytes
    }

    fn expected() -> Vec<Block> {
        let record = |code, ops: Vec<u64>, blob| Item::Record(Record { code, ops, blob });
        vec![Block {
            id: 8,
            items: vec![
                record(5, vec![100, 1000], None),
                record(6, TEXT.iter().map(|c| u64::from(*c)).collect(), None),
                record(9, vec![], Some(BLOB.to_vec())),
                record(7, vec![3, WIDE], None),
                Item::Block(Block {
                    id: 12,
                    items: vec![record(2, vec![0], None)],
                }),
            ],
        }]
    }

    #[test]
    fn every_encoding_reads_back_and_blockinfo_is_dropped() {
        assert_eq!(read(&stream()), Ok(expected()));
    }

    #[test]
    fn zero_padding_after_the_last_block_is_the_end() {
        let mut bytes = stream();
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(read(&bytes), Ok(expected()));
    }

    #[test]
    fn a_short_stream_is_truncated() {
        let mut bytes = stream();
        bytes.pop();
        assert!(matches!(read(&bytes), Err(BitcodeError::Truncated { .. })));
    }

    #[test]
    fn a_stream_without_the_magic_is_refused() {
        let mut bytes = stream();
        bytes[1] ^= 0xFF;
        assert_eq!(read(&bytes), Err(BitcodeError::NoMagic));
        assert_eq!(read(&MAGIC[..3]), Err(BitcodeError::NoMagic));
    }

    #[test]
    fn an_undefined_abbreviation_is_named() {
        let mut w = Bits::new();
        let block = w.enter(2, 8, 3);
        let bit = w.bit;
        w.fixed(7, 3);
        w.end(3, block);
        assert_eq!(read(&w.bytes), Err(BitcodeError::NoAbbrev { id: 7, bit }));
    }

    #[test]
    fn blocks_nested_past_the_limit_are_refused() {
        let mut w = Bits::new();
        let mut open = Vec::new();
        let mut width = 2;
        for depth in 0..=MAX_DEPTH {
            open.push((w.enter(width, depth as u64 + 1, 3), 3));
            width = 3;
        }
        while let Some((at, width)) = open.pop() {
            w.end(width, at);
        }
        assert_eq!(read(&w.bytes), Err(BitcodeError::TooDeep(MAX_DEPTH)));
    }

    #[test]
    fn an_array_of_literal_elements_is_refused() {
        let mut w = Bits::new();
        let _block = w.enter(2, 8, 4);
        w.define(4, &[Op::Literal(1), Op::Array, Op::Literal(0)]);
        assert!(matches!(read(&w.bytes), Err(BitcodeError::Unsupported(_))));
    }

    #[test]
    fn an_empty_blob_in_the_last_partial_word_is_truncated_and_never_a_panic() {
        let mut w = Bits::new();
        let _block = w.enter(2, 8, 4);
        w.define(4, &[Op::Literal(9), Op::Fixed(1), Op::Blob]);
        w.fixed(4, 4);
        w.fixed(1, 1);
        w.vbr(0, 6);
        assert_ne!(w.bytes.len() % 4, 0, "the stream ends inside a word");
        assert!(matches!(
            read(&w.bytes),
            Err(BitcodeError::Truncated { .. })
        ));
    }
}
