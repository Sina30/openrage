//! Decoding a script's bytecode into instructions, and checking the decode
//! against what the file itself states.
//!
//! ⚠ **A decode is only believed when the file agrees with it.** The checks in
//! [`check`] need nothing from outside the script: the code must decode to
//! exactly `code_length` bytes, no instruction may cross a code page (the
//! compiler pads with `NOP` so none does), every jump, switch case and call
//! must land on an instruction boundary, every call on an `ENTER` or on the
//! page padding in front of one, and every native slot an instruction names
//! must be inside the native table.
//!
//! ⚠ **They catch a wrong width in most scripts, not all.** `IADD_U8` read a
//! byte wide fails 2,270 of the 2,545 retail scripts that use it; the rest
//! only ever follow it with a one-byte instruction nothing jumps to, so the
//! decode falls back in step. A corpus is what makes the checks evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

use or_io::gta5::script::opcode::{self, InstructionSet, Op, OpcodeInfo, Shape};

use super::natives::NativeDb;
use super::{ScrProgram, CODE_PAGE_SIZE};
use crate::error::IoError;

#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    /// Byte offset of the opcode within the joined code pages.
    pub at: u32,
    /// The byte as the file holds it, in the script's own numbering.
    pub opcode: u8,
    /// The same instruction in the 131-opcode numbering, whichever set it came from.
    pub op: Op,
    /// Opcode plus operand, in bytes.
    pub len: u32,
    pub operand: Operand,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    None,
    U8(u8),
    U8U8(u8, u8),
    U8U8U8(u8, u8, u8),
    U16(u16),
    S16(i16),
    U24(u32),
    U32(u32),
    F32(f32),
    /// Resolved to an absolute code offset, which may lie outside the code.
    Jump {
        target: i64,
    },
    Call {
        target: u32,
    },
    Native {
        args: u8,
        returns: u8,
        slot: u16,
    },
    Enter {
        params: u8,
        frame: u16,
        name: Vec<u8>,
    },
    Leave {
        params: u8,
        returns: u8,
    },
    Switch(Vec<SwitchCase>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchCase {
    pub value: u32,
    pub target: i64,
}

impl Instruction {
    pub fn mnemonic(&self) -> &'static str {
        opcode::mnemonic(self.op)
    }

    /// Every code offset this instruction can transfer control to.
    pub fn targets(&self) -> Vec<i64> {
        match &self.operand {
            Operand::Jump { target } => vec![*target],
            Operand::Call { target } => vec![*target as i64],
            Operand::Switch(cases) => cases.iter().map(|c| c.target).collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeErrorKind {
    /// The script's RSC7 version names no instruction set.
    UnknownVersion(u32),
    UnknownOpcode(u8),
    /// The operand runs past the last byte of code.
    Truncated {
        needs: usize,
        has: usize,
    },
}

/// Where a decode stopped, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub at: u32,
    pub kind: DecodeErrorKind,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            DecodeErrorKind::UnknownVersion(v) => {
                write!(f, "RSC7 version {v} names no instruction set")
            }
            DecodeErrorKind::UnknownOpcode(op) => {
                write!(f, "unknown opcode {op} at code offset 0x{:06X}", self.at)
            }
            DecodeErrorKind::Truncated { needs, has } => write!(
                f,
                "instruction at code offset 0x{:06X} needs {needs} bytes and the code \
                 has {has} left",
                self.at
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<DecodeError> for IoError {
    fn from(e: DecodeError) -> Self {
        IoError::InvalidData {
            context: e.to_string(),
        }
    }
}

/// Decode `code` from its first byte to its last.
pub fn decode(code: &[u8], set: InstructionSet) -> Result<Vec<Instruction>, DecodeError> {
    decode_with(code, set.table())
}

/// [`decode`] against a caller's table, so a perturbed one can be shown to fail.
pub fn decode_with(code: &[u8], table: &[OpcodeInfo]) -> Result<Vec<Instruction>, DecodeError> {
    let mut out = Vec::with_capacity(code.len() / 2);
    let mut at = 0usize;
    while at < code.len() {
        let instruction = decode_one(code, at, table)?;
        at += instruction.len as usize;
        out.push(instruction);
    }
    Ok(out)
}

fn decode_one(code: &[u8], at: usize, table: &[OpcodeInfo]) -> Result<Instruction, DecodeError> {
    let opcode = code[at];
    let info = table.get(opcode as usize).ok_or(DecodeError {
        at: at as u32,
        kind: DecodeErrorKind::UnknownOpcode(opcode),
    })?;
    let rest = &code[at + 1..];
    let need = |n: usize| -> Result<&[u8], DecodeError> {
        rest.get(..n).ok_or(DecodeError {
            at: at as u32,
            kind: DecodeErrorKind::Truncated {
                needs: n + 1,
                has: rest.len() + 1,
            },
        })
    };

    let (operand, width) = match info.shape {
        Shape::None => (Operand::None, 0),
        Shape::U8 => (Operand::U8(need(1)?[0]), 1),
        Shape::U8U8 => {
            let b = need(2)?;
            (Operand::U8U8(b[0], b[1]), 2)
        }
        Shape::U8U8U8 => {
            let b = need(3)?;
            (Operand::U8U8U8(b[0], b[1], b[2]), 3)
        }
        Shape::U16 => (Operand::U16(u16_le(need(2)?)), 2),
        Shape::S16 => (Operand::S16(u16_le(need(2)?) as i16), 2),
        Shape::U24 => (Operand::U24(u24_le(need(3)?)), 3),
        Shape::U32 => (Operand::U32(u32_le(need(4)?)), 4),
        Shape::F32 => (Operand::F32(f32::from_bits(u32_le(need(4)?))), 4),
        Shape::Jump => {
            let offset = u16_le(need(2)?) as i16;
            let end = (at + 3) as i64;
            (
                Operand::Jump {
                    target: end + offset as i64,
                },
                2,
            )
        }
        Shape::Call => (
            Operand::Call {
                target: u24_le(need(3)?),
            },
            3,
        ),
        Shape::Native => {
            let b = need(3)?;
            (
                Operand::Native {
                    args: b[0] >> 2,
                    returns: b[0] & 3,
                    slot: u16::from_be_bytes([b[1], b[2]]),
                },
                3,
            )
        }
        Shape::Enter => {
            let head = need(4)?;
            let name_len = head[3] as usize;
            let all = need(4 + name_len)?;
            (
                Operand::Enter {
                    params: head[0],
                    frame: u16_le(&head[1..3]),
                    name: all[4..].to_vec(),
                },
                4 + name_len,
            )
        }
        Shape::Leave => {
            let b = need(2)?;
            (
                Operand::Leave {
                    params: b[0],
                    returns: b[1],
                },
                2,
            )
        }
        Shape::Switch => {
            let count = need(1)?[0] as usize;
            let all = need(1 + count * 6)?;
            let cases = (0..count)
                .map(|i| {
                    let case = &all[1 + i * 6..1 + (i + 1) * 6];
                    let end = (at + 2 + (i + 1) * 6) as i64;
                    SwitchCase {
                        value: u32_le(&case[..4]),
                        target: end + u16_le(&case[4..]) as i16 as i64,
                    }
                })
                .collect();
            (Operand::Switch(cases), 1 + count * 6)
        }
    };

    Ok(Instruction {
        at: at as u32,
        opcode,
        op: info.op,
        len: 1 + width as u32,
        operand,
    })
}

fn u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn u24_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], 0])
}

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// A count of failures and the code offsets of the first few.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Misses {
    pub count: usize,
    pub first: Vec<u32>,
}

impl Misses {
    const KEPT: usize = 4;

    fn push(&mut self, at: u32) {
        self.count += 1;
        if self.first.len() < Self::KEPT {
            self.first.push(at);
        }
    }
}

/// What checking one script's decode against its own file found.
///
/// Every `*_checked` count is how many things reached a check, so a report
/// that checked nothing is distinguishable from one that found nothing wrong.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeCheck {
    pub code_length: usize,
    pub bytes_decoded: usize,
    pub instructions: usize,
    pub functions: usize,
    /// Instructions whose bytes run from one code page into the next.
    pub straddles: Misses,
    pub jumps_checked: usize,
    pub jumps_off_boundary: Misses,
    /// Jumps to exactly `code_length`, one past the last instruction.
    pub jumps_to_code_end: usize,
    pub cases_checked: usize,
    pub cases_off_boundary: Misses,
    pub calls_checked: usize,
    pub calls_off_boundary: Misses,
    /// Calls that land on page padding which runs into an `ENTER` at the
    /// start of the next code page.
    pub calls_through_padding: usize,
    /// Calls that reach no `ENTER`, directly or through padding.
    pub calls_not_to_enter: Misses,
    pub natives_checked: usize,
    pub native_slots_out_of_range: Misses,
    /// Native table slots no instruction names.
    pub native_slots_unreferenced: usize,
    /// Instructions per [`Op`].
    pub op_counts: Vec<u64>,
}

impl CodeCheck {
    /// One line per check that failed; empty when the decode agrees with the file.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.bytes_decoded != self.code_length {
            out.push(format!(
                "decoded {} of {} code bytes",
                self.bytes_decoded, self.code_length
            ));
        }
        for (what, misses) in [
            ("instruction(s) cross a code page", &self.straddles),
            ("jump(s) land off an instruction", &self.jumps_off_boundary),
            (
                "switch case(s) land off an instruction",
                &self.cases_off_boundary,
            ),
            ("call(s) land off an instruction", &self.calls_off_boundary),
            (
                "call(s) land on something other than ENTER",
                &self.calls_not_to_enter,
            ),
            (
                "native slot(s) outside the native table",
                &self.native_slots_out_of_range,
            ),
        ] {
            if misses.count > 0 {
                let first: Vec<String> =
                    misses.first.iter().map(|a| format!("0x{a:06X}")).collect();
                out.push(format!(
                    "{} {what}, first at {}",
                    misses.count,
                    first.join(", ")
                ));
            }
        }
        out
    }

    pub fn is_clean(&self) -> bool {
        self.problems().is_empty()
    }
}

/// Decode `program`'s code and check it against the file.
///
/// A decode that stops short is an `Err`; a decode that reaches the end but
/// disagrees with the file is an `Ok` whose [`CodeCheck::problems`] say how.
pub fn check(program: &ScrProgram) -> Result<(Vec<Instruction>, CodeCheck), DecodeError> {
    let set = program.instruction_set().ok_or(DecodeError {
        at: 0,
        kind: DecodeErrorKind::UnknownVersion(program.resource_version),
    })?;
    check_with(program, set.table())
}

pub fn check_with(
    program: &ScrProgram,
    table: &[OpcodeInfo],
) -> Result<(Vec<Instruction>, CodeCheck), DecodeError> {
    let instructions = decode_with(&program.code, table)?;
    let report = check_decoded(program, &instructions);
    Ok((instructions, report))
}

/// Check an already-decoded instruction list against `program`.
pub fn check_decoded(program: &ScrProgram, instructions: &[Instruction]) -> CodeCheck {
    let code_length = program.code.len();
    let mut report = CodeCheck {
        code_length,
        instructions: instructions.len(),
        op_counts: vec![0; 256],
        ..CodeCheck::default()
    };

    // The instruction starting at each offset, and `u32::MAX` between them.
    let mut index_at = vec![u32::MAX; code_length];
    for (n, i) in instructions.iter().enumerate() {
        index_at[i.at as usize] = n as u32;
    }
    let at = |target: i64| -> Option<&Instruction> {
        let n = *index_at.get(usize::try_from(target).ok()?)?;
        instructions.get(n as usize)
    };
    let lands = |target: i64| at(target).is_some();

    let mut referenced = vec![false; program.natives.len()];
    for i in instructions {
        report.bytes_decoded += i.len as usize;
        report.op_counts[i.op as usize] += 1;

        let first_page = i.at as usize / CODE_PAGE_SIZE;
        let last_page = (i.at + i.len - 1) as usize / CODE_PAGE_SIZE;
        if first_page != last_page {
            report.straddles.push(i.at);
        }

        match &i.operand {
            Operand::Jump { target } => {
                report.jumps_checked += 1;
                if *target == code_length as i64 {
                    report.jumps_to_code_end += 1;
                } else if !lands(*target) {
                    report.jumps_off_boundary.push(i.at);
                }
            }
            Operand::Switch(cases) => {
                for case in cases {
                    report.cases_checked += 1;
                    if !lands(case.target) {
                        report.cases_off_boundary.push(i.at);
                    }
                }
            }
            Operand::Call { target } => {
                report.calls_checked += 1;
                match at(*target as i64) {
                    None => report.calls_off_boundary.push(i.at),
                    Some(t) if t.op == opcode::ENTER => {}
                    Some(t) if padding_leads_to_enter(t, at) => report.calls_through_padding += 1,
                    Some(_) => report.calls_not_to_enter.push(i.at),
                }
            }
            Operand::Native { slot, .. } => {
                report.natives_checked += 1;
                match referenced.get_mut(*slot as usize) {
                    Some(seen) => *seen = true,
                    None => report.native_slots_out_of_range.push(i.at),
                }
            }
            Operand::Enter { .. } => report.functions += 1,
            _ => {}
        }
    }
    report.native_slots_unreferenced = referenced.iter().filter(|seen| !**seen).count();
    report
}

/// Whether `from` is padding — `NOP`s and unconditional jumps — that runs into
/// an `ENTER` at the first byte of a code page.
///
/// ⚠ **A function the compiler pushed onto the next page keeps the address
/// its callers were given.** The gap in front of it is filled with `NOP` or
/// with a `J` over it, and the call still names the gap's first byte. Only
/// the page start makes this padding rather than a call into a function body.
fn padding_leads_to_enter<'a>(
    from: &'a Instruction,
    at: impl Fn(i64) -> Option<&'a Instruction>,
) -> bool {
    let mut here = from;
    for _ in 0..CODE_PAGE_SIZE {
        let next = match (&here.operand, here.op) {
            (Operand::None, opcode::NOP) => here.at as i64 + 1,
            (Operand::Jump { target }, opcode::J) => *target,
            _ => {
                return here.op == opcode::ENTER
                    && (here.at as usize).is_multiple_of(CODE_PAGE_SIZE)
            }
        };
        match at(next) {
            Some(i) => here = i,
            None => return false,
        }
    }
    false
}

/// The integer an instruction pushes, when it pushes one constant.
pub fn pushed_constant(i: &Instruction) -> Option<i64> {
    match (i.op, &i.operand) {
        (opcode::PUSH_CONST_U8, Operand::U8(v)) => Some(*v as i64),
        (opcode::PUSH_CONST_S16, Operand::S16(v)) => Some(*v as i64),
        (opcode::PUSH_CONST_U24, Operand::U24(v)) => Some(*v as i64),
        (opcode::PUSH_CONST_U32, Operand::U32(v)) => Some(*v as i64),
        (op @ opcode::PUSH_CONST_M1..=opcode::PUSH_CONST_7, Operand::None) => {
            Some(op as i64 - opcode::PUSH_CONST_0 as i64)
        }
        _ => None,
    }
}

/// Write a listing of `instructions`, naming natives from `names` where it can.
///
/// ⚠ A native `names` does not cover prints as its hash and says so; it never
/// borrows a neighbour's name.
pub fn write_listing(
    out: &mut impl Write,
    program: &ScrProgram,
    instructions: &[Instruction],
    names: Option<&NativeDb>,
    show_bytes: bool,
) -> fmt::Result {
    let mut functions: BTreeMap<u32, String> = BTreeMap::new();
    for i in instructions {
        if let Operand::Enter { name, .. } = &i.operand {
            let label = if name.is_empty() {
                format!("func_{}", functions.len())
            } else {
                String::from_utf8_lossy(name).into_owned()
            };
            functions.insert(i.at, label);
        }
    }
    let jump_targets: BTreeSet<i64> = instructions
        .iter()
        .filter(|i| !matches!(i.operand, Operand::Call { .. }))
        .flat_map(Instruction::targets)
        .collect();

    let mut previous: Option<&Instruction> = None;
    for i in instructions {
        if let Some(label) = functions.get(&i.at) {
            writeln!(out)?;
            writeln!(out, "{label}:")?;
        } else if jump_targets.contains(&(i.at as i64)) {
            writeln!(out, "  @{:06X}:", i.at)?;
        }
        write!(out, "    {:06X}  ", i.at)?;
        if show_bytes {
            let bytes = &program.code[i.at as usize..(i.at + i.len) as usize];
            let mut hex: String = bytes.iter().take(6).map(|b| format!("{b:02X} ")).collect();
            if bytes.len() > 6 {
                hex.push_str("… ");
            }
            write!(out, "{hex:<21}")?;
        }
        write!(out, "{:<26}", i.mnemonic())?;
        write_operand(out, program, i, previous, &functions, names)?;
        writeln!(out)?;
        previous = Some(i);
    }
    Ok(())
}

fn write_operand(
    out: &mut impl Write,
    program: &ScrProgram,
    i: &Instruction,
    previous: Option<&Instruction>,
    functions: &BTreeMap<u32, String>,
    names: Option<&NativeDb>,
) -> fmt::Result {
    match &i.operand {
        Operand::None => {
            if i.op == opcode::STRING {
                if let Some(text) = previous
                    .and_then(pushed_constant)
                    .and_then(|at| usize::try_from(at).ok())
                    .and_then(|at| program.string_at(at))
                {
                    write!(out, "; {:?}", String::from_utf8_lossy(text))?;
                }
            }
            Ok(())
        }
        Operand::U8(v) => write!(out, "{v}"),
        Operand::U8U8(a, b) => write!(out, "{a}, {b}"),
        Operand::U8U8U8(a, b, c) => write!(out, "{a}, {b}, {c}"),
        Operand::U16(v) => write!(out, "{v}"),
        Operand::S16(v) => write!(out, "{v}"),
        Operand::U24(v) => write!(out, "{v}"),
        Operand::U32(v) => write!(out, "{v}"),
        Operand::F32(v) => write!(out, "{v:?}"),
        Operand::Jump { target } => write!(out, "@{target:06X}"),
        Operand::Call { target } => match functions.get(target) {
            Some(label) => write!(out, "{label}"),
            None => match functions.range(target..).next() {
                Some((at, label))
                    if (*at as usize).is_multiple_of(CODE_PAGE_SIZE)
                        && ((*at - target) as usize) < CODE_PAGE_SIZE =>
                {
                    write!(out, "{label}  ; through page padding at 0x{target:06X}")
                }
                _ => write!(out, "0x{target:06X}"),
            },
        },
        Operand::Native {
            args,
            returns,
            slot,
        } => {
            write!(out, "args={args} returns={returns} ")?;
            match program.native_hash(*slot as usize) {
                None => write!(out, "slot {slot} outside the table"),
                Some(hash) => match names.and_then(|db| db.name(hash)) {
                    Some(name) => {
                        let signature = name.signature().unwrap_or_default();
                        write!(out, "{name}{signature}  ; 0x{hash:016X}")
                    }
                    None => write!(out, "0x{hash:016X}  ; unnamed"),
                },
            }
        }
        Operand::Enter {
            params,
            frame,
            name: _,
        } => write!(out, "params={params} frame={frame}"),
        Operand::Leave { params, returns } => write!(out, "params={params} returns={returns}"),
        Operand::Switch(cases) => {
            let shown: Vec<String> = cases
                .iter()
                .map(|c| format!("{} => @{:06X}", c.value as i32, c.target))
                .collect();
            write!(out, "[{}]", shown.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gta5::script::natives::NativeName;
    use opcode::{CALL, ENTER, J, LEAVE, NATIVE, NOP, SWITCH};

    const HASH: u64 = 0x4EDE_34FB_ADD9_67A6;

    fn program(code: Vec<u8>, natives: &[u64]) -> ScrProgram {
        let code_length = code.len() as u32;
        ScrProgram {
            code_length,
            natives: natives
                .iter()
                .enumerate()
                .map(|(i, h)| h.rotate_right(code_length.wrapping_add(i as u32) & 0x3F))
                .collect(),
            code,
            string_heap: b"\0hello\0".to_vec(),
            resource_version: 12,
            ..ScrProgram::default()
        }
    }

    fn two_functions() -> Vec<u8> {
        two_functions_calling(&[])
    }

    /// Two functions: a switch, a jump, a native call, a call, and a string.
    /// Offsets are computed from the bytes laid down, never written as literals.
    fn two_functions_calling(extra: &[u8]) -> Vec<u8> {
        let mut c = vec![ENTER, 0, 2, 0, 0];
        c.extend_from_slice(extra);
        c.extend([opcode::PUSH_CONST_U8, 7]);
        // One case, whose offset is from the end of the case: past the J below.
        c.extend([SWITCH, 1, 7, 0, 0, 0, 3, 0]);
        c.extend([J, 0, 0]);
        c.extend([NATIVE, 1 << 2, 0, 0]);
        let call_at = c.len();
        c.extend([CALL, 0, 0, 0]);
        c.extend([LEAVE, 0, 0]);
        let callee = c.len() as u8;
        c[call_at + 1] = callee;
        c.extend([
            ENTER,
            0,
            2,
            0,
            0,
            opcode::PUSH_CONST_0 + 1,
            opcode::STRING,
            43,
            LEAVE,
            0,
            0,
        ]);
        c
    }

    #[test]
    fn a_program_decodes_and_agrees_with_itself() {
        let p = program(two_functions(), &[HASH]);
        let (instructions, report) = check(&p).unwrap();
        assert!(report.is_clean(), "{:?}", report.problems());
        assert_eq!(report.bytes_decoded, p.code.len());
        assert_eq!(
            (
                report.functions,
                report.jumps_checked,
                report.cases_checked,
                report.calls_checked,
                report.natives_checked
            ),
            (2, 1, 1, 1, 1)
        );
        assert_eq!(report.native_slots_unreferenced, 0);
        let switch = instructions.iter().find(|i| i.op == SWITCH).unwrap();
        let j = instructions.iter().find(|i| i.op == J).unwrap();
        assert_eq!(
            switch.targets(),
            j.targets(),
            "the case and the jump land together"
        );
        assert_eq!(p.native_hash(0), Some(HASH));
    }

    /// ⚠ The same bytes under the version-10 set are a different program,
    /// and the version is what picks between them.
    #[test]
    fn the_version_picks_the_set_and_an_unknown_one_refuses() {
        let mut p = program(two_functions(), &[HASH]);
        assert_eq!(p.instruction_set(), Some(InstructionSet::U24Locals));
        let names = |set: InstructionSet| -> Vec<&str> {
            decode(&p.code, set)
                .map(|is| is.iter().map(Instruction::mnemonic).collect())
                .unwrap_or_default()
        };
        assert_ne!(
            names(InstructionSet::U24Locals),
            names(InstructionSet::U16Locals)
        );
        assert!(names(InstructionSet::U24Locals).contains(&"SWITCH"));

        p.resource_version = 10;
        assert_eq!(p.instruction_set(), Some(InstructionSet::U16Locals));
        p.resource_version = 11;
        assert_eq!(
            check(&p).unwrap_err().kind,
            DecodeErrorKind::UnknownVersion(11)
        );
    }

    /// ⚠ **Both halves are the point.** A jump read a byte wide swallows the
    /// instruction its own target names, and is noticed. `PUSH_CONST_U8` read
    /// a byte wide swallows the `SWITCH` opcode, the case bytes decode as
    /// one-byte instructions, and the decode is back in step at the `J` — so a
    /// width error can hide in a script, and only a corpus exposes it.
    #[test]
    fn an_operand_a_byte_off_is_noticed_unless_the_decode_falls_back_in_step() {
        let p = program(two_functions(), &[HASH]);
        let widened = |op: Op| {
            let mut table = InstructionSet::U24Locals.table().to_vec();
            let entry = table.iter_mut().find(|e| e.op == op).unwrap();
            entry.shape = match entry.shape {
                Shape::U8 => Shape::U16,
                _ => Shape::U24,
            };
            check_with(&p, &table).is_ok_and(|(_, r)| r.is_clean())
        };
        assert!(!widened(J), "a jump a byte wide lands inside itself");
        assert!(
            widened(opcode::PUSH_CONST_U8),
            "this program cannot see that one"
        );
    }

    #[test]
    fn an_instruction_across_a_page_is_reported() {
        let mut code = vec![ENTER, 0, 2, 0, 0];
        code.resize(CODE_PAGE_SIZE - 2, NOP);
        code.extend([opcode::PUSH_CONST_U32, 1, 2, 3, 4, LEAVE, 0, 0]);
        let (_, report) = check(&program(code, &[])).unwrap();
        assert_eq!(report.straddles.count, 1);
        assert_eq!(report.straddles.first, vec![(CODE_PAGE_SIZE - 2) as u32]);
    }

    /// ⚠ Padding counts only when it runs into an `ENTER` at a page start.
    #[test]
    fn a_call_into_page_padding_reaches_the_function_only_at_a_page_start() {
        let build = |enter_at: usize| {
            let mut code = vec![ENTER, 0, 2, 0, 0, CALL, 0, 0, 0, LEAVE, 0, 0];
            let pad = code.len();
            code[6] = pad as u8;
            code.resize(enter_at, NOP);
            code.extend([ENTER, 0, 2, 0, 0, LEAVE, 0, 0]);
            code
        };
        let (_, at_page) = check(&program(build(CODE_PAGE_SIZE), &[])).unwrap();
        assert!(at_page.is_clean(), "{:?}", at_page.problems());
        assert_eq!(at_page.calls_through_padding, 1);

        let (_, mid_page) = check(&program(build(64), &[])).unwrap();
        assert_eq!(mid_page.calls_through_padding, 0);
        assert_eq!(mid_page.calls_not_to_enter.count, 1);
    }

    #[test]
    fn a_jump_may_name_the_end_of_the_code_and_nothing_past_it() {
        let end = |past: u8| vec![ENTER, 0, 2, 0, 0, J, past, 0, LEAVE, 0, 0];
        let (_, to_end) = check(&program(end(3), &[])).unwrap();
        assert_eq!(
            (to_end.jumps_to_code_end, to_end.jumps_off_boundary.count),
            (1, 0)
        );
        let (_, past) = check(&program(end(4), &[])).unwrap();
        assert_eq!(past.jumps_off_boundary.count, 1);
    }

    #[test]
    fn a_native_slot_past_the_table_is_reported() {
        let code = vec![ENTER, 0, 2, 0, 0, NATIVE, 0, 0, 1, LEAVE, 0, 0];
        let (_, report) = check(&program(code, &[HASH])).unwrap();
        assert_eq!(report.native_slots_out_of_range.count, 1);
        assert_eq!(report.native_slots_unreferenced, 1);
    }

    /// ⚠ A named native shows its name; one the list does not hold stays a hash.
    #[test]
    fn a_listing_names_what_the_list_names_and_nothing_else() {
        let other = 0x1111_2222_3333_4444;
        let p = program(two_functions_calling(&[NATIVE, 0, 0, 1]), &[HASH, other]);
        let (instructions, report) = check(&p).unwrap();
        assert!(report.is_clean(), "{:?}", report.problems());
        let db = NativeDb::from_names(
            "test",
            [(
                HASH,
                NativeName {
                    namespace: Some("BUILTIN".into()),
                    name: "WAIT".into(),
                    ..NativeName::default()
                },
            )],
        );
        let mut out = String::new();
        write_listing(&mut out, &p, &instructions, Some(&db), false).unwrap();
        assert!(out.contains("BUILTIN::WAIT  ; 0x4EDE34FBADD967A6"), "{out}");

        let typed = NativeDb::from_names(
            "typed",
            [(
                HASH,
                NativeName {
                    namespace: Some("BUILTIN".into()),
                    name: "WAIT".into(),
                    params: vec![crate::gta5::script::natives::NativeParam {
                        ty: "int".into(),
                        name: "ms".into(),
                    }],
                    returns: Some("void".into()),
                },
            )],
        );
        let mut out = String::new();
        write_listing(&mut out, &p, &instructions, Some(&typed), false).unwrap();
        assert!(
            out.contains("BUILTIN::WAIT(int ms) -> void  ; 0x4EDE34FBADD967A6"),
            "a list's types print beside the name
{out}"
        );
        assert!(out.contains("0x1111222233334444  ; unnamed"), "{out}");
        assert!(out.contains("func_1"), "the call names its callee\n{out}");
        assert!(
            out.contains("; \"hello\""),
            "STRING shows the heap entry\n{out}"
        );
    }
}
