//! A synthetic script whose source shape is known decompiles to that shape.
//!
//! Each program is assembled from mnemonics, so an opcode number or a jump
//! offset is computed from the table and the bytes laid down, never written
//! by hand.

use std::collections::HashMap;

use or_io::gta5::script::decompile::{self, Decompiled};
use or_io::gta5::script::natives::{NativeDb, NativeName};
use or_io::gta5::script::opcode::InstructionSet;
use or_io::gta5::script::ScrProgram;

fn op(mnemonic: &str) -> u8 {
    InstructionSet::U24Locals
        .table()
        .iter()
        .find(|i| i.mnemonic == mnemonic)
        .unwrap_or_else(|| panic!("no opcode {mnemonic}"))
        .op
}

/// A version-12 program built from mnemonics, with labels resolved on finish.
#[derive(Default)]
struct Asm {
    code: Vec<u8>,
    labels: HashMap<&'static str, usize>,
    /// `(operand offset, label, offset the jump is measured from)`.
    fixups: Vec<(usize, &'static str, usize)>,
}

impl Asm {
    fn i(&mut self, mnemonic: &str, operand: &[u8]) -> &mut Self {
        self.code.push(op(mnemonic));
        self.code.extend_from_slice(operand);
        self
    }

    fn enter(&mut self, params: u8, frame: u16) -> &mut Self {
        let [lo, hi] = frame.to_le_bytes();
        self.i("ENTER", &[params, lo, hi, 0])
    }

    fn label(&mut self, name: &'static str) -> &mut Self {
        self.labels.insert(name, self.code.len());
        self
    }

    fn jump(&mut self, mnemonic: &str, label: &'static str) -> &mut Self {
        self.code.push(op(mnemonic));
        let at = self.code.len();
        self.code.extend([0, 0]);
        self.fixups.push((at, label, at + 2));
        self
    }

    fn call(&mut self, label: &'static str) -> &mut Self {
        self.code.push(op("CALL"));
        let at = self.code.len();
        self.code.extend([0, 0, 0]);
        self.fixups.push((at, label, usize::MAX));
        self
    }

    fn switch(&mut self, cases: &[(u32, &'static str)]) -> &mut Self {
        self.code.push(op("SWITCH"));
        self.code.push(cases.len() as u8);
        for (value, label) in cases {
            self.code.extend(value.to_le_bytes());
            let at = self.code.len();
            self.code.extend([0, 0]);
            self.fixups.push((at, label, at + 2));
        }
        self
    }

    fn finish(&mut self, string_heap: &[u8], natives: &[u64]) -> ScrProgram {
        for (at, label, from) in &self.fixups {
            let target = self.labels[label];
            if *from == usize::MAX {
                let bytes = (target as u32).to_le_bytes();
                self.code[*at..*at + 3].copy_from_slice(&bytes[..3]);
            } else {
                let offset = i16::try_from(target as i64 - *from as i64).expect("in range");
                self.code[*at..*at + 2].copy_from_slice(&offset.to_le_bytes());
            }
        }
        let code_length = self.code.len() as u32;
        ScrProgram {
            code_length,
            natives: natives
                .iter()
                .enumerate()
                .map(|(i, h)| h.rotate_right(code_length.wrapping_add(i as u32) & 0x3F))
                .collect(),
            code: self.code.clone(),
            string_heap: string_heap.to_vec(),
            resource_version: 12,
            name: "synthetic".into(),
            ..ScrProgram::default()
        }
    }
}

fn decompiled(program: &ScrProgram, names: Option<&NativeDb>) -> Decompiled {
    let out = decompile::decompile(program, names).expect("decodes");
    let stats = &out.stats;
    assert_eq!(
        (stats.instructions_consumed, stats.instructions_repeated),
        (stats.instructions, 0),
        "every instruction is emitted exactly once\n{}",
        out.text
    );
    assert_eq!(stats.stack_mismatches, 0, "{}", out.text);
    out
}

fn assert_shows(out: &Decompiled, expected: &str) {
    assert!(
        out.text.contains(expected),
        "expected\n{expected}\nin\n{}",
        out.text
    );
}

#[test]
fn an_if_else_inside_a_while_loop_decompiles_to_that_structure() {
    let mut a = Asm::default();
    a.enter(0, 4)
        .i("PUSH_CONST_0", &[])
        .i("LOCAL_U8_STORE", &[2])
        .label("top")
        .i("LOCAL_U8_LOAD", &[2])
        .i("PUSH_CONST_U8", &[10])
        .jump("ILT_JZ", "end")
        .i("LOCAL_U8_LOAD", &[2])
        .i("IADD_U8", &[1])
        .i("LOCAL_U8_STORE", &[2])
        .i("LOCAL_U8_LOAD", &[2])
        .i("PUSH_CONST_5", &[])
        .jump("IEQ_JZ", "else")
        .i("PUSH_CONST_1", &[])
        .i("LOCAL_U8_STORE", &[3])
        .jump("J", "endif")
        .label("else")
        .i("PUSH_CONST_2", &[])
        .i("LOCAL_U8_STORE", &[3])
        .label("endif")
        .jump("J", "top")
        .label("end")
        .i("LEAVE", &[0, 0]);
    let out = decompiled(&a.finish(b"", &[]), None);
    assert_shows(
        &out,
        "void func_0()
{
    var local0, local1;

    local0 = 0;
    while (local0 < 10) {
        local0 += 1;
        if (local0 == 5) {
            local1 = 1;
        } else {
            local1 = 2;
        }
    }
}
",
    );
    assert_eq!(out.stats.functions_with_goto, 0);
}

#[test]
fn a_nested_counting_loop_decompiles_to_nested_fors() {
    let mut a = Asm::default();
    a.enter(0, 5)
        .i("PUSH_CONST_0", &[])
        .i("LOCAL_U8_STORE", &[2])
        .label("outer")
        .i("LOCAL_U8_LOAD", &[2])
        .i("PUSH_CONST_3", &[])
        .jump("ILT_JZ", "outer_end")
        .i("PUSH_CONST_0", &[])
        .i("LOCAL_U8_STORE", &[3])
        .label("inner")
        .i("LOCAL_U8_LOAD", &[3])
        .i("PUSH_CONST_4", &[])
        .jump("ILT_JZ", "inner_end")
        .i("LOCAL_U8_LOAD", &[4])
        .i("LOCAL_U8_LOAD", &[2])
        .i("LOCAL_U8_LOAD", &[3])
        .i("IMUL", &[])
        .i("IADD", &[])
        .i("LOCAL_U8_STORE", &[4])
        .i("LOCAL_U8_LOAD", &[3])
        .i("IADD_U8", &[1])
        .i("LOCAL_U8_STORE", &[3])
        .jump("J", "inner")
        .label("inner_end")
        .i("LOCAL_U8_LOAD", &[2])
        .i("IADD_U8", &[1])
        .i("LOCAL_U8_STORE", &[2])
        .jump("J", "outer")
        .label("outer_end")
        .i("LEAVE", &[0, 0]);
    let out = decompiled(&a.finish(b"", &[]), None);
    assert_shows(
        &out,
        "    for (local0 = 0; local0 < 3; local0 += 1) {
        for (local1 = 0; local1 < 4; local1 += 1) {
            local2 += local0 * local1;
        }
    }
",
    );
    assert_eq!(out.stats.functions_with_goto, 0);
}

#[test]
fn a_switch_decompiles_to_its_cases_breaks_and_default() {
    let mut a = Asm::default();
    a.enter(1, 4)
        .i("LOCAL_U8_LOAD", &[0])
        .switch(&[(1, "one_or_two"), (2, "one_or_two"), (5, "five")])
        .jump("J", "default")
        .label("one_or_two")
        .i("PUSH_CONST_U8", &[10])
        .i("LOCAL_U8_STORE", &[3])
        .jump("J", "end")
        .label("five")
        .i("PUSH_CONST_U8", &[20])
        .i("LOCAL_U8_STORE", &[3])
        .jump("J", "end")
        .label("default")
        .i("PUSH_CONST_U8", &[30])
        .i("LOCAL_U8_STORE", &[3])
        .label("end")
        .i("LOCAL_U8_LOAD", &[3])
        .i("LEAVE", &[1, 1]);
    let out = decompiled(&a.finish(b"", &[]), None);
    assert_shows(
        &out,
        "var func_0(var param0)
{
    var local0;

    switch (param0) {
        case 1:
        case 2:
            local0 = 10;
            break;
        case 5:
            local0 = 20;
            break;
        default:
            local0 = 30;
    }
    return local0;
}
",
    );
    assert_eq!(out.stats.functions_with_goto, 0);
}

/// ⚠ A native the list names shows its name; one it does not stays a hash.
#[test]
fn a_native_call_a_string_and_a_call_between_functions() {
    let named: u64 = 0x4EDE_34FB_ADD9_67A6;
    let unnamed: u64 = 0x1111_2222_3333_4444;
    let mut a = Asm::default();
    a.enter(0, 3)
        .i("PUSH_CONST_1", &[])
        .i("STRING", &[])
        .i("PUSH_CONST_3", &[])
        .i("NATIVE", &[2 << 2, 0, 0])
        .i("NATIVE", &[1, 0, 1])
        .call("twice")
        .i("LOCAL_U8_STORE", &[2])
        .i("LEAVE", &[0, 0])
        .label("twice")
        .enter(1, 3)
        .i("LOCAL_U8_LOAD", &[0])
        .i("PUSH_CONST_2", &[])
        .i("IMUL", &[])
        .i("LEAVE", &[1, 1]);
    let program = a.finish(b"\0hello\0", &[named, unnamed]);
    let db = NativeDb::from_names(
        "test",
        [(
            named,
            NativeName {
                namespace: Some("HUD".into()),
                name: "PRINT".into(),
                ..NativeName::default()
            },
        )],
    );
    let out = decompiled(&program, Some(&db));
    assert_shows(&out, "    HUD::PRINT(\"hello\", 3);\n");
    assert_shows(&out, "    local0 = func_1(_0x1111222233334444());\n");
    assert_shows(
        &out,
        "var func_1(var param0)
{
    return param0 * 2;
}
",
    );
    assert_eq!(out.stats.functions, 2);
}

/// A jump into the middle of a loop has no structured form, so it is a goto
/// to a label, and the function is counted as needing one.
#[test]
fn a_jump_no_construct_explains_falls_back_to_a_labelled_goto() {
    let mut a = Asm::default();
    a.enter(1, 5)
        .i("LOCAL_U8_LOAD", &[0])
        .jump("JZ", "middle")
        .label("top")
        .i("PUSH_CONST_1", &[])
        .i("LOCAL_U8_STORE", &[3])
        .label("middle")
        .i("PUSH_CONST_2", &[])
        .i("LOCAL_U8_STORE", &[4])
        .jump("J", "top");
    let out = decompiled(&a.finish(b"", &[]), None);
    assert!(out.text.contains("goto label_"), "{}", out.text);
    assert!(out.text.contains("label_"), "{}", out.text);
    assert_eq!(out.stats.functions_with_goto, 1);
}

/// `a && b` compiles to a `DUP` and a `JZ` around `b`; it reads back as one condition.
#[test]
fn a_short_circuit_and_reads_back_as_one_condition() {
    let mut a = Asm::default();
    a.enter(2, 5)
        .i("LOCAL_U8_LOAD", &[0])
        .i("DUP", &[])
        .jump("JZ", "joined")
        .i("LOCAL_U8_LOAD", &[1])
        .i("IAND", &[])
        .label("joined")
        .jump("JZ", "end")
        .i("PUSH_CONST_1", &[])
        .i("LOCAL_U8_STORE", &[4])
        .label("end")
        .i("LEAVE", &[2, 0]);
    let out = decompiled(&a.finish(b"", &[]), None);
    assert_shows(
        &out,
        "    if (param0 && param1) {
        local0 = 1;
    }
",
    );
}

/// A list in the shape `alloc8or/gta5-nativedb-data` publishes, with a parameter and return type per native.
const NATIVEDB_JSON: &str = r#"{
    "PLAYER": {
        "0x00000000000000A1": {
            "name": "PLAYER_PED_ID", "jhash": "", "comment": "",
            "params": [], "return_type": "Ped", "build": "323"
        }
    },
    "ENTITY": {
        "0x00000000000000B2": {
            "name": "IS_ENTITY_DEAD", "jhash": "", "comment": "",
            "params": [{ "type": "Entity", "name": "entity" }, { "type": "BOOL", "name": "p1" }],
            "return_type": "BOOL", "build": "323"
        }
    }
}"#;

/// ⚠ A native named through a crossmap types the local its result is stored in, and a literal handed
/// to a `BOOL` parameter reads as one; a local stored from nothing typed stays `var`.
#[test]
fn a_native_list_names_a_call_and_types_its_arguments_and_result() {
    let mut a = Asm::default();
    a.enter(0, 5)
        .i("NATIVE", &[1, 0, 0])
        .i("LOCAL_U8_STORE", &[2])
        .i("LOCAL_U8_LOAD", &[2])
        .i("PUSH_CONST_0", &[])
        .i("NATIVE", &[(2 << 2) | 1, 0, 1])
        .i("LOCAL_U8_STORE", &[3])
        .i("PUSH_CONST_7", &[])
        .i("LOCAL_U8_STORE", &[4])
        .i("LEAVE", &[0, 0]);
    // A version 12 script calls by current hash; the list is keyed by first hash.
    let program = a.finish(b"", &[0xC1, 0xC2]);
    let mut db = NativeDb::parse("natives.json", NATIVEDB_JSON.as_bytes()).expect("parses");
    let crossmap = b"{0x00000000000000A1, 0x00000000000000C1},
{0x00000000000000B2, 0x00000000000000C2},
";
    assert_eq!(db.cross_map("crossmap.txt", crossmap).expect("pairs"), 2);
    let out = decompiled(&program, Some(&db));
    assert_shows(
        &out,
        "    var local2;
    Ped local0;
    BOOL local1;

    local0 = PLAYER::PLAYER_PED_ID();
    local1 = ENTITY::IS_ENTITY_DEAD(local0, false);
    local2 = 7;
",
    );
}
