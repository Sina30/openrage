//! The script VM's instruction sets: one byte of opcode, then an operand whose
//! shape the opcode alone decides.
//!
//! ⚠ **There are two numberings, and the RSC7 version says which.** A later
//! build inserted three `LOCAL_U24` opcodes at 94, so every opcode from 94 up
//! moved by three, and the script version went from 10 to 12 with it. The same
//! bytes decode as different programs under the two, which is why an
//! instruction carries [`Op`], the number in the wider set, as well as the byte
//! it was read from.
//!
//! Which set each script uses, and every operand width, are measured by
//! decoding all 3,022 scripts of a retail Legacy and a retail Enhanced install
//! under both sets side by side (`or-title`'s `ysc_disasm_census`): the set
//! its version names decodes each script to exactly `code_length` bytes, with
//! no instruction crossing a code page and every jump, switch case and call
//! landing on an instruction. The same suite measures the big-endian native
//! index and the end-of-case switch offset against their rival readings. The
//! GTAMods wiki's `Script_Container` page agrees on the 131-opcode numbering,
//! `LOCAL_U24` at 94, and `IS_BIT_SET` as the last opcode of each set.

/// How the bytes after an opcode are laid out.
///
/// Multi-byte immediates are little-endian unless the shape says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    None,
    U8,
    U8U8,
    U8U8U8,
    U16,
    S16,
    U24,
    U32,
    F32,
    /// A signed 16-bit offset from the end of the instruction.
    Jump,
    /// An absolute 24-bit code address.
    Call,
    /// `(args << 2) | returns`, then the native slot as a **big-endian** `u16`.
    Native,
    /// Parameter count `u8`, frame size `u16`, then a length-prefixed name.
    Enter,
    /// Parameter count `u8`, return count `u8`.
    Leave,
    /// A `u8` case count, then per case a `u32` value and a [`Jump`](Self::Jump)
    /// offset measured from the end of *that case*.
    Switch,
}

/// An instruction's number in the 131-opcode set, whichever set it was read under.
pub type Op = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcodeInfo {
    pub mnemonic: &'static str,
    pub shape: Shape,
    pub op: Op,
}

/// Which numbering a script's bytecode uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InstructionSet {
    /// 128 opcodes; `IS_BIT_SET` is 127. RSC7 version 10.
    U16Locals,
    /// 131 opcodes, with `LOCAL_U24` at 94. RSC7 version 12.
    U24Locals,
}

impl InstructionSet {
    pub const ALL: [InstructionSet; 2] = [InstructionSet::U16Locals, InstructionSet::U24Locals];

    /// The set a script of this RSC7 version is compiled to.
    pub const fn for_version(version: u32) -> Option<Self> {
        match version {
            10 => Some(InstructionSet::U16Locals),
            12 => Some(InstructionSet::U24Locals),
            _ => None,
        }
    }

    pub fn table(self) -> &'static [OpcodeInfo] {
        match self {
            InstructionSet::U16Locals => &U16_LOCALS,
            InstructionSet::U24Locals => &U24_LOCALS,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            InstructionSet::U16Locals => "128 opcodes, no LOCAL_U24 (version 10)",
            InstructionSet::U24Locals => "131 opcodes, LOCAL_U24 at 94 (version 12)",
        }
    }
}

pub const NOP: Op = 0;
pub const PUSH_CONST_U8: Op = 37;
pub const PUSH_CONST_U32: Op = 40;
pub const NATIVE: Op = 44;
pub const ENTER: Op = 45;
pub const LEAVE: Op = 46;
pub const IADD_U8: Op = 61;
pub const PUSH_CONST_S16: Op = 67;
pub const J: Op = 85;
pub const CALL: Op = 93;
pub const LOCAL_U24: Op = 94;
pub const PUSH_CONST_U24: Op = 100;
pub const SWITCH: Op = 101;
pub const STRING: Op = 102;
/// Measured unused in every release script, as are [`THROW`], `F2I` and `STRINGHASH`.
pub const CATCH: Op = 109;
pub const THROW: Op = 110;
pub const PUSH_CONST_M1: Op = 112;
pub const PUSH_CONST_0: Op = 113;
pub const PUSH_CONST_7: Op = 120;
pub const IS_BIT_SET: Op = 130;

/// Where the wider set inserts its three opcodes.
const INSERTED_AT: usize = LOCAL_U24 as usize;
const INSERTED: usize = 3;

const fn op(mnemonic: &'static str, shape: Shape) -> OpcodeInfo {
    OpcodeInfo {
        mnemonic,
        shape,
        op: 0,
    }
}

/// The version-10 set, in its own byte order.
static U16_LOCALS: [OpcodeInfo; 128] = numbered(BASE);

/// The version-12 set: the version-10 set with `LOCAL_U24` inserted at 94.
static U24_LOCALS: [OpcodeInfo; 131] = insert_local_u24(numbered(BASE));

/// Give each version-10 entry its number in the 131-opcode set.
const fn numbered(mut base: [OpcodeInfo; 128]) -> [OpcodeInfo; 128] {
    let mut i = 0;
    while i < base.len() {
        base[i].op = if i < INSERTED_AT { i } else { i + INSERTED } as Op;
        i += 1;
    }
    base
}

const fn insert_local_u24(base: [OpcodeInfo; 128]) -> [OpcodeInfo; 131] {
    let inserted = [
        op("LOCAL_U24", Shape::U24),
        op("LOCAL_U24_LOAD", Shape::U24),
        op("LOCAL_U24_STORE", Shape::U24),
    ];
    let mut out = [op("", Shape::None); 131];
    let mut i = 0;
    while i < out.len() {
        out[i] = if i < INSERTED_AT {
            base[i]
        } else if i < INSERTED_AT + INSERTED {
            let mut e = inserted[i - INSERTED_AT];
            e.op = i as Op;
            e
        } else {
            base[i - INSERTED]
        };
        i += 1;
    }
    out
}

/// The mnemonic for an [`Op`], in the 131-opcode numbering.
pub fn mnemonic(op: Op) -> &'static str {
    U24_LOCALS
        .get(op as usize)
        .map_or("?", |info| info.mnemonic)
}

const BASE: [OpcodeInfo; 128] = [
    op("NOP", Shape::None),
    op("IADD", Shape::None),
    op("ISUB", Shape::None),
    op("IMUL", Shape::None),
    op("IDIV", Shape::None),
    op("IMOD", Shape::None),
    op("INOT", Shape::None),
    op("INEG", Shape::None),
    op("IEQ", Shape::None),
    op("INE", Shape::None),
    op("IGT", Shape::None),
    op("IGE", Shape::None),
    op("ILT", Shape::None),
    op("ILE", Shape::None),
    op("FADD", Shape::None),
    op("FSUB", Shape::None),
    op("FMUL", Shape::None),
    op("FDIV", Shape::None),
    op("FMOD", Shape::None),
    op("FNEG", Shape::None),
    op("FEQ", Shape::None),
    op("FNE", Shape::None),
    op("FGT", Shape::None),
    op("FGE", Shape::None),
    op("FLT", Shape::None),
    op("FLE", Shape::None),
    op("VADD", Shape::None),
    op("VSUB", Shape::None),
    op("VMUL", Shape::None),
    op("VDIV", Shape::None),
    op("VNEG", Shape::None),
    op("IAND", Shape::None),
    op("IOR", Shape::None),
    op("IXOR", Shape::None),
    op("I2F", Shape::None),
    op("F2I", Shape::None),
    op("F2V", Shape::None),
    op("PUSH_CONST_U8", Shape::U8),
    op("PUSH_CONST_U8_U8", Shape::U8U8),
    op("PUSH_CONST_U8_U8_U8", Shape::U8U8U8),
    op("PUSH_CONST_U32", Shape::U32),
    op("PUSH_CONST_F", Shape::F32),
    op("DUP", Shape::None),
    op("DROP", Shape::None),
    op("NATIVE", Shape::Native),
    op("ENTER", Shape::Enter),
    op("LEAVE", Shape::Leave),
    op("LOAD", Shape::None),
    op("STORE", Shape::None),
    op("STORE_REV", Shape::None),
    op("LOAD_N", Shape::None),
    op("STORE_N", Shape::None),
    op("ARRAY_U8", Shape::U8),
    op("ARRAY_U8_LOAD", Shape::U8),
    op("ARRAY_U8_STORE", Shape::U8),
    op("LOCAL_U8", Shape::U8),
    op("LOCAL_U8_LOAD", Shape::U8),
    op("LOCAL_U8_STORE", Shape::U8),
    op("STATIC_U8", Shape::U8),
    op("STATIC_U8_LOAD", Shape::U8),
    op("STATIC_U8_STORE", Shape::U8),
    op("IADD_U8", Shape::U8),
    op("IMUL_U8", Shape::U8),
    op("IOFFSET", Shape::None),
    op("IOFFSET_U8", Shape::U8),
    op("IOFFSET_U8_LOAD", Shape::U8),
    op("IOFFSET_U8_STORE", Shape::U8),
    op("PUSH_CONST_S16", Shape::S16),
    op("IADD_S16", Shape::S16),
    op("IMUL_S16", Shape::S16),
    op("IOFFSET_S16", Shape::S16),
    op("IOFFSET_S16_LOAD", Shape::S16),
    op("IOFFSET_S16_STORE", Shape::S16),
    op("ARRAY_U16", Shape::U16),
    op("ARRAY_U16_LOAD", Shape::U16),
    op("ARRAY_U16_STORE", Shape::U16),
    op("LOCAL_U16", Shape::U16),
    op("LOCAL_U16_LOAD", Shape::U16),
    op("LOCAL_U16_STORE", Shape::U16),
    op("STATIC_U16", Shape::U16),
    op("STATIC_U16_LOAD", Shape::U16),
    op("STATIC_U16_STORE", Shape::U16),
    op("GLOBAL_U16", Shape::U16),
    op("GLOBAL_U16_LOAD", Shape::U16),
    op("GLOBAL_U16_STORE", Shape::U16),
    op("J", Shape::Jump),
    op("JZ", Shape::Jump),
    op("IEQ_JZ", Shape::Jump),
    op("INE_JZ", Shape::Jump),
    op("IGT_JZ", Shape::Jump),
    op("IGE_JZ", Shape::Jump),
    op("ILT_JZ", Shape::Jump),
    op("ILE_JZ", Shape::Jump),
    op("CALL", Shape::Call),
    op("GLOBAL_U24", Shape::U24),
    op("GLOBAL_U24_LOAD", Shape::U24),
    op("GLOBAL_U24_STORE", Shape::U24),
    op("PUSH_CONST_U24", Shape::U24),
    op("SWITCH", Shape::Switch),
    op("STRING", Shape::None),
    op("STRINGHASH", Shape::None),
    op("TEXT_LABEL_ASSIGN_STRING", Shape::U8),
    op("TEXT_LABEL_ASSIGN_INT", Shape::U8),
    op("TEXT_LABEL_APPEND_STRING", Shape::U8),
    op("TEXT_LABEL_APPEND_INT", Shape::U8),
    op("TEXT_LABEL_COPY", Shape::None),
    op("CATCH", Shape::None),
    op("THROW", Shape::None),
    op("CALLINDIRECT", Shape::None),
    op("PUSH_CONST_M1", Shape::None),
    op("PUSH_CONST_0", Shape::None),
    op("PUSH_CONST_1", Shape::None),
    op("PUSH_CONST_2", Shape::None),
    op("PUSH_CONST_3", Shape::None),
    op("PUSH_CONST_4", Shape::None),
    op("PUSH_CONST_5", Shape::None),
    op("PUSH_CONST_6", Shape::None),
    op("PUSH_CONST_7", Shape::None),
    op("PUSH_CONST_FM1", Shape::None),
    op("PUSH_CONST_F0", Shape::None),
    op("PUSH_CONST_F1", Shape::None),
    op("PUSH_CONST_F2", Shape::None),
    op("PUSH_CONST_F3", Shape::None),
    op("PUSH_CONST_F4", Shape::None),
    op("PUSH_CONST_F5", Shape::None),
    op("PUSH_CONST_F6", Shape::None),
    op("PUSH_CONST_F7", Shape::None),
    op("IS_BIT_SET", Shape::None),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`Op`] constant names the mnemonic it claims, in both sets
    /// that hold it — computed from the tables, never transcribed.
    #[test]
    fn the_named_ops_are_where_the_tables_put_them() {
        for (op, name) in [
            (NATIVE, "NATIVE"),
            (ENTER, "ENTER"),
            (CALL, "CALL"),
            (IADD_U8, "IADD_U8"),
            (LOCAL_U24, "LOCAL_U24"),
            (PUSH_CONST_U24, "PUSH_CONST_U24"),
            (SWITCH, "SWITCH"),
            (STRING, "STRING"),
            (CATCH, "CATCH"),
            (THROW, "THROW"),
            (J, "J"),
            (PUSH_CONST_M1, "PUSH_CONST_M1"),
            (PUSH_CONST_0, "PUSH_CONST_0"),
            (PUSH_CONST_7, "PUSH_CONST_7"),
            (IS_BIT_SET, "IS_BIT_SET"),
        ] {
            assert_eq!(mnemonic(op), name);
            for set in InstructionSet::ALL {
                if let Some(info) = set.table().iter().find(|i| i.op == op) {
                    assert_eq!(info.mnemonic, name, "{set:?}");
                }
            }
        }
    }

    /// ⚠ The wider set is the narrower one with three opcodes inserted, so
    /// every shared mnemonic keeps its shape and moves by exactly three.
    #[test]
    fn the_wider_set_is_the_narrower_one_shifted_at_94() {
        let narrow = InstructionSet::U16Locals.table();
        let wide = InstructionSet::U24Locals.table();
        assert_eq!((narrow.len(), wide.len()), (128, 131));
        for (byte, info) in narrow.iter().enumerate() {
            let moved = if byte < 94 { byte } else { byte + 3 };
            assert_eq!(wide[moved].mnemonic, info.mnemonic);
            assert_eq!(wide[moved].shape, info.shape);
            assert_eq!(info.op as usize, moved);
        }
        for (byte, info) in wide.iter().enumerate() {
            assert_eq!(info.op as usize, byte);
        }
        let names: std::collections::HashSet<&str> = wide.iter().map(|i| i.mnemonic).collect();
        assert_eq!(names.len(), wide.len(), "no mnemonic twice");
    }
}
