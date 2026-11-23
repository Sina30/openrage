//! Lowerings for three shapes a translator writes that naga's validator refuses and that mean the same in the form it takes.

use std::collections::{HashMap, HashSet};

use super::{header, opcode};

const OP_CAPABILITY: u32 = 17;
const OP_EXT_INST_IMPORT: u32 = 11;
const OP_EXT_INST: u32 = 12;
const OP_BITCAST: u32 = 124;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_ARRAY: u32 = 28;
const OP_TYPE_POINTER: u32 = 32;
const OP_CONSTANT: u32 = 43;
const OP_CONSTANT_NULL: u32 = 46;
const OP_FUNCTION: u32 = 54;
const OP_VARIABLE: u32 = 59;
const OP_LOAD: u32 = 61;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_DECORATE: u32 = 71;
const OP_COPY_OBJECT: u32 = 83;
const OP_S_NEGATE: u32 = 126;
const OP_I_SUB: u32 = 130;

const DECORATION_BUILT_IN: u32 = 11;
const DECORATION_NON_READABLE: u32 = 25;
const BUILT_IN_SAMPLE_MASK: u32 = 20;
const CAPABILITY_INTERPOLATION_FUNCTION: u32 = 52;
/// `InterpolateAtCentroid`, `InterpolateAtSample` and `InterpolateAtOffset` of the GLSL extended instruction set.
const INTERPOLATE_AT: [u32; 3] = [76, 77, 78];
/// `FindILsb`, `FindSMsb` and `FindUMsb`.
const FIND_BIT: [u32; 3] = [73, 74, 75];
const STORAGE_UNIFORM: u32 = 2;
const STORAGE_OUTPUT: u32 = 3;
const STORAGE_WORKGROUP: u32 = 4;
const STORAGE_BUFFER: u32 = 12;

/// Whether each type id is an unsigned integer or a vector of them, by the id of the type.
fn unsigned_types(insts: &[Vec<u32>]) -> HashSet<u32> {
    let mut unsigned = HashSet::new();
    for inst in insts {
        match opcode(inst) {
            OP_TYPE_INT if inst.len() == 4 && inst[3] == 0 => {
                unsigned.insert(inst[1]);
            }
            OP_TYPE_VECTOR if inst.len() == 4 && unsigned.contains(&inst[2]) => {
                unsigned.insert(inst[1]);
            }
            _ => {}
        }
    }
    unsigned
}

/// Subtract from nought what an unsigned integer is negated by: naga's `Negate` takes signed integers and floats only, and the two's complement of a bit pattern is the same either way.
pub(super) fn subtract_unsigned_negations(
    insts: Vec<Vec<u32>>,
    bound: &mut u32,
) -> (Vec<Vec<u32>>, usize) {
    let unsigned = unsigned_types(&insts);
    let first_function = insts
        .iter()
        .position(|i| opcode(i) == OP_FUNCTION)
        .unwrap_or(insts.len());
    let mut zeroes: HashMap<u32, u32> = HashMap::new();
    let mut declared: Vec<Vec<u32>> = Vec::new();
    let mut out = Vec::with_capacity(insts.len());
    let mut count = 0;
    for inst in insts {
        if opcode(&inst) == OP_S_NEGATE && inst.len() == 4 && unsigned.contains(&inst[1]) {
            let zero = *zeroes.entry(inst[1]).or_insert_with(|| {
                let id = *bound;
                *bound += 1;
                declared.push(vec![header(OP_CONSTANT_NULL, 3), inst[1], id]);
                id
            });
            out.push(vec![header(OP_I_SUB, 5), inst[1], inst[2], zero, inst[3]]);
            count += 1;
        } else {
            out.push(inst);
        }
    }
    // The zeroes are declared with the other constants, before any function uses them
    out.splice(first_function..first_function, declared);
    (out, count)
}

/// Let a storage buffer that a shader only writes be read as well: naga has no write-only buffer, and nothing reads it.
pub(super) fn make_written_buffers_readable(insts: Vec<Vec<u32>>) -> (Vec<Vec<u32>>, usize) {
    let buffers: HashSet<u32> = insts
        .iter()
        .filter(|i| {
            opcode(i) == OP_VARIABLE
                && i.len() >= 4
                && [STORAGE_UNIFORM, STORAGE_BUFFER].contains(&i[3])
        })
        .map(|i| i[2])
        .collect();
    let mut count = 0;
    let out = insts
        .into_iter()
        .filter(|i| {
            let drop = opcode(i) == OP_DECORATE
                && i.len() == 3
                && i[2] == DECORATION_NON_READABLE
                && buffers.contains(&i[1]);
            count += usize::from(drop);
            !drop
        })
        .collect();
    (out, count)
}

/// Take the initializer off a workgroup variable: its contents start undefined in SPIR-V, and naga refuses an initial value there.
pub(super) fn drop_workgroup_initializers(insts: Vec<Vec<u32>>) -> (Vec<Vec<u32>>, usize) {
    let mut count = 0;
    let out = insts
        .into_iter()
        .map(|mut i| {
            if opcode(&i) == OP_VARIABLE && i.len() == 5 && i[3] == STORAGE_WORKGROUP {
                i.truncate(4);
                i[0] = header(OP_VARIABLE, 4);
                count += 1;
            }
            i
        })
        .collect();
    (out, count)
}

/// Write the coverage mask of a draw as the one integer naga has it as: SPIR-V declares it an array of one, which naga's reader gives a scalar initial value and then refuses.
pub(super) fn scalarize_sample_masks(
    insts: Vec<Vec<u32>>,
    bound: &mut u32,
) -> (Vec<Vec<u32>>, usize) {
    let masks: HashSet<u32> = insts
        .iter()
        .filter(|i| {
            opcode(i) == OP_DECORATE
                && i.len() == 4
                && i[2] == DECORATION_BUILT_IN
                && i[3] == BUILT_IN_SAMPLE_MASK
        })
        .map(|i| i[1])
        .collect();
    let constants: HashMap<u32, u32> = insts
        .iter()
        .filter(|i| opcode(i) == OP_CONSTANT && i.len() == 4)
        .map(|i| (i[2], i[3]))
        .collect();
    // An array of exactly one element, by its id, and the type of that element
    let arrays: HashMap<u32, u32> = insts
        .iter()
        .filter(|i| opcode(i) == OP_TYPE_ARRAY && i.len() == 4 && constants.get(&i[3]) == Some(&1))
        .map(|i| (i[1], i[2]))
        .collect();
    let pointers: HashMap<u32, (u32, u32)> = insts
        .iter()
        .filter(|i| opcode(i) == OP_TYPE_POINTER && i.len() == 4)
        .map(|i| (i[1], (i[2], i[3])))
        .collect();
    let mut changed: HashSet<u32> = HashSet::new();
    // The first pointer type of each pointee declared so far, which is all a variable may use
    let mut earlier: HashMap<(u32, u32), u32> = HashMap::new();
    let mut out = Vec::with_capacity(insts.len());
    for mut inst in insts {
        if opcode(&inst) == OP_TYPE_POINTER && inst.len() == 4 {
            earlier.entry((inst[2], inst[3])).or_insert(inst[1]);
        }
        if opcode(&inst) == OP_VARIABLE
            && inst.len() >= 4
            && inst[3] == STORAGE_OUTPUT
            && masks.contains(&inst[2])
        {
            let element = pointers
                .get(&inst[1])
                .and_then(|(_, array)| arrays.get(array));
            if let Some(&element) = element {
                let pointer = *earlier.entry((STORAGE_OUTPUT, element)).or_insert_with(|| {
                    let id = *bound;
                    *bound += 1;
                    out.push(vec![
                        header(OP_TYPE_POINTER, 4),
                        id,
                        STORAGE_OUTPUT,
                        element,
                    ]);
                    id
                });
                inst[1] = pointer;
                changed.insert(inst[2]);
            }
        } else if opcode(&inst) == OP_ACCESS_CHAIN && inst.len() == 5 && changed.contains(&inst[3])
        {
            // The one element is the integer itself
            inst = vec![header(OP_COPY_OBJECT, 4), inst[1], inst[2], inst[3]];
        }
        out.push(inst);
    }
    (out, changed.len())
}

/// Read an input where it is, for each of the instructions that read it at a sample, a centroid or an offset: naga has none of them, and the three differ from it only for a draw with more than one sample, whose pixel they read at another place than the centre.
pub(super) fn load_interpolations(insts: Vec<Vec<u32>>) -> (Vec<Vec<u32>>, usize) {
    let glsl: Option<u32> = insts
        .iter()
        .find(|i| opcode(i) == OP_EXT_INST_IMPORT && i.len() >= 3)
        .filter(|i| {
            let text: Vec<u8> = i[2..].iter().flat_map(|w| w.to_le_bytes()).collect();
            text.starts_with(b"GLSL.std.450\0")
        })
        .map(|i| i[1]);
    let Some(glsl) = glsl else {
        return (insts, 0);
    };
    let mut count = 0;
    let mut out: Vec<Vec<u32>> = insts
        .into_iter()
        .map(|inst| {
            if opcode(&inst) == OP_EXT_INST
                && inst.len() >= 6
                && inst[3] == glsl
                && INTERPOLATE_AT.contains(&inst[4])
            {
                count += 1;
                vec![header(OP_LOAD, 4), inst[1], inst[2], inst[5]]
            } else {
                inst
            }
        })
        .collect();
    if count > 0 {
        out.retain(|i| {
            !(opcode(i) == OP_CAPABILITY
                && i.len() == 2
                && i[1] == CAPABILITY_INTERPOLATION_FUNCTION)
        });
    }
    (out, count)
}

/// The signedness of each integer type and of each vector of one, by the type's id: `true` for signed.
fn signedness(insts: &[Vec<u32>]) -> HashMap<u32, bool> {
    let mut by_type: HashMap<u32, bool> = HashMap::new();
    for inst in insts {
        match opcode(inst) {
            OP_TYPE_INT if inst.len() == 4 => {
                by_type.insert(inst[1], inst[3] == 1);
            }
            OP_TYPE_VECTOR if inst.len() == 4 => {
                if let Some(&signed) = by_type.get(&inst[2]) {
                    by_type.insert(inst[1], signed);
                }
            }
            _ => {}
        }
    }
    by_type
}

/// The type of each value an instruction of a known shape defines, by the value's id.
fn value_types(insts: &[Vec<u32>]) -> HashMap<u32, u32> {
    /// Constants, loads, conversions, integer arithmetic, bit operations, selects, phis, parameters and extended instructions: each `[opcode, type, id, ...]`.
    const TYPED: [u32; 22] = [
        43,
        44,
        46,
        55,
        61,
        81,
        169,
        245,
        OP_EXT_INST,
        OP_BITCAST,
        128,
        129,
        130,
        131,
        132,
        133,
        194,
        195,
        196,
        197,
        198,
        199,
    ];
    insts
        .iter()
        .filter(|i| i.len() >= 3 && TYPED.contains(&opcode(i)))
        .map(|i| (i[2], i[1]))
        .collect()
}

/// Give `FindILsb`, `FindSMsb` and `FindUMsb` the type of their operand and bitcast what they give to the type SPIR-V says it has: naga types the result as the operand and then refuses to add it to a number of the other signedness, and the bits are the same either way.
pub(super) fn retype_bit_searches(insts: Vec<Vec<u32>>, bound: &mut u32) -> (Vec<Vec<u32>>, usize) {
    let glsl: Option<u32> = insts
        .iter()
        .find(|i| opcode(i) == OP_EXT_INST_IMPORT && i.len() >= 3)
        .filter(|i| {
            let text: Vec<u8> = i[2..].iter().flat_map(|w| w.to_le_bytes()).collect();
            text.starts_with(b"GLSL.std.450\0")
        })
        .map(|i| i[1]);
    let Some(glsl) = glsl else {
        return (insts, 0);
    };
    let (signs, types) = (signedness(&insts), value_types(&insts));
    let mut count = 0;
    let mut out = Vec::with_capacity(insts.len());
    for inst in insts {
        let search = opcode(&inst) == OP_EXT_INST
            && inst.len() == 6
            && inst[3] == glsl
            && FIND_BIT.contains(&inst[4]);
        let operand_type = search.then(|| types.get(&inst[5])).flatten().copied();
        match operand_type {
            Some(operand_type)
                if signs.get(&operand_type).is_some_and(|operand| {
                    signs.get(&inst[1]).is_some_and(|result| result != operand)
                }) =>
            {
                let searched = *bound;
                *bound += 1;
                out.push(vec![
                    header(OP_EXT_INST, 6),
                    operand_type,
                    searched,
                    inst[3],
                    inst[4],
                    inst[5],
                ]);
                out.push(vec![header(OP_BITCAST, 4), inst[1], inst[2], searched]);
                count += 1;
            }
            _ => out.push(inst),
        }
    }
    (out, count)
}

#[cfg(test)]
mod tests;
