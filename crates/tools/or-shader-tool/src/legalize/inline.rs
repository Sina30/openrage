//! Inlines the small helper functions a translator writes for a dot product, so no call result is left for naga's Metal writer to find unnamed in a loop's continue block.

use std::collections::{HashMap, HashSet};

use super::{header, opcode};

const OP_EXT_INST_IMPORT: u32 = 11;
const OP_EXT_INST: u32 = 12;
const OP_FUNCTION: u32 = 54;
const OP_FUNCTION_PARAMETER: u32 = 55;
const OP_FUNCTION_END: u32 = 56;
const OP_FUNCTION_CALL: u32 = 57;
const OP_COMPOSITE_CONSTRUCT: u32 = 80;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_COPY_OBJECT: u32 = 83;
const OP_F_NEGATE: u32 = 127;
const OP_F_ADD: u32 = 129;
const OP_F_SUB: u32 = 131;
const OP_F_MUL: u32 = 133;
const OP_F_DIV: u32 = 136;
const OP_SELECT: u32 = 169;
const OP_LABEL: u32 = 248;
const OP_RETURN_VALUE: u32 = 254;
/// `OpFOrdEqual` to `OpFOrdLessThanEqual`, which take two operands and give a bool.
const F_COMPARISONS: std::ops::RangeInclusive<u32> = 180..=188;

/// The positions of the ids an instruction reads, when it is one of the few whose layout is known here.
fn operands(inst: &[u32], glsl: u32) -> Option<Vec<usize>> {
    let len = inst.len();
    match opcode(inst) {
        OP_EXT_INST if len >= 5 && inst[3] == glsl => {
            Some(std::iter::once(3).chain(5..len).collect())
        }
        OP_COMPOSITE_CONSTRUCT if len >= 4 => Some((3..len).collect()),
        OP_COMPOSITE_EXTRACT if len >= 5 => Some(vec![3]),
        OP_F_NEGATE if len == 4 => Some(vec![3]),
        OP_F_ADD | OP_F_SUB | OP_F_MUL | OP_F_DIV if len == 5 => Some(vec![3, 4]),
        op if F_COMPARISONS.contains(&op) && len == 5 => Some(vec![3, 4]),
        OP_SELECT if len == 6 => Some(vec![3, 4, 5]),
        _ => None,
    }
}

/// A function that takes values and gives one, in a single block that calls nothing.
struct Callee {
    params: Vec<u32>,
    body: Vec<Vec<u32>>,
    returned: u32,
}

/// The helper functions of `insts` that can be inlined, by their ids.
fn callees(insts: &[Vec<u32>], glsl: u32) -> HashMap<u32, Callee> {
    let mut found = HashMap::new();
    let mut at = 0;
    while at < insts.len() {
        if opcode(&insts[at]) != OP_FUNCTION || insts[at].len() != 5 {
            at += 1;
            continue;
        }
        let id = insts[at][2];
        let end = insts[at..]
            .iter()
            .position(|i| opcode(i) == OP_FUNCTION_END)
            .map_or(insts.len(), |p| at + p);
        let inside = &insts[at + 1..end];
        let params: Vec<u32> = inside
            .iter()
            .take_while(|i| opcode(i) == OP_FUNCTION_PARAMETER && i.len() == 3)
            .map(|i| i[2])
            .collect();
        let rest = &inside[params.len()..];
        let shaped = rest.len() >= 2
            && opcode(&rest[0]) == OP_LABEL
            && opcode(&rest[rest.len() - 1]) == OP_RETURN_VALUE
            && rest[rest.len() - 1].len() == 2;
        if shaped {
            let body = &rest[1..rest.len() - 1];
            if body.iter().all(|i| operands(i, glsl).is_some()) {
                found.insert(
                    id,
                    Callee {
                        params,
                        body: body.to_vec(),
                        returned: rest[rest.len() - 1][1],
                    },
                );
            }
        }
        at = end + 1;
    }
    found
}

/// Replace each call of a helper function with the helper's own instructions, taking new ids from `bound`.
pub(super) fn inline_helpers(insts: Vec<Vec<u32>>, bound: &mut u32) -> (Vec<Vec<u32>>, usize) {
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
    let callees = callees(&insts, glsl);
    if callees.is_empty() {
        return (insts, 0);
    }
    let mut count = 0;
    let mut out = Vec::with_capacity(insts.len());
    for inst in insts {
        let callee = (opcode(&inst) == OP_FUNCTION_CALL && inst.len() >= 4)
            .then(|| callees.get(&inst[3]))
            .flatten()
            .filter(|c| inst.len() == 4 + c.params.len());
        let Some(callee) = callee else {
            out.push(inst);
            continue;
        };
        let (result_type, result) = (inst[1], inst[2]);
        let mut renamed: HashMap<u32, u32> = callee
            .params
            .iter()
            .copied()
            .zip(inst[4..].iter().copied())
            .collect();
        // What the function returns is what the call is, so its last instruction takes the call's id
        let defined_here: HashSet<u32> = callee.body.iter().map(|i| i[2]).collect();
        if defined_here.contains(&callee.returned) {
            renamed.insert(callee.returned, result);
        }
        for body in &callee.body {
            renamed.entry(body[2]).or_insert_with(|| {
                let id = *bound;
                *bound += 1;
                id
            });
        }
        for body in &callee.body {
            let mut copy = body.clone();
            copy[2] = renamed[&body[2]];
            for position in operands(body, glsl).unwrap_or_default() {
                if let Some(new) = renamed.get(&body[position]) {
                    copy[position] = *new;
                }
            }
            out.push(copy);
        }
        if !defined_here.contains(&callee.returned) {
            let value = renamed
                .get(&callee.returned)
                .copied()
                .unwrap_or(callee.returned);
            out.push(vec![header(OP_COPY_OBJECT, 4), result_type, result, value]);
        }
        count += 1;
    }
    (out, count)
}

#[cfg(test)]
mod tests;
