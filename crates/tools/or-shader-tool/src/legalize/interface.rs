//! Gives each interface location one variable: a translator splits a register's components into variables that share a location, which naga cannot read, and types a location by the components a shader uses, which Metal refuses where the stage beside it uses others.
//!
//! The variables of a location become `Private` copies of their own. A new vector variable takes the
//! location, and the entry point copies it into them on entry (an input) or fills it from them
//! before every return (an output), so the shader body is not rewritten.

use std::collections::{BTreeMap, HashMap, HashSet};

const OP_ENTRY_POINT: u32 = 15;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_FLOAT: u32 = 22;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_POINTER: u32 = 32;
const OP_CONSTANT_NULL: u32 = 46;
const OP_FUNCTION: u32 = 54;
const OP_FUNCTION_END: u32 = 56;
const OP_VARIABLE: u32 = 59;
const OP_LOAD: u32 = 61;
const OP_STORE: u32 = 62;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_IN_BOUNDS_ACCESS_CHAIN: u32 = 66;
const OP_DECORATE: u32 = 71;
const OP_MEMBER_DECORATE: u32 = 72;
const OP_VECTOR_SHUFFLE: u32 = 79;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_COMPOSITE_INSERT: u32 = 82;
const OP_LABEL: u32 = 248;
const OP_RETURN: u32 = 253;

const EXECUTION_MODEL_VERTEX: u32 = 0;
const EXECUTION_MODEL_FRAGMENT: u32 = 4;

const STORAGE_INPUT: u32 = 1;
const STORAGE_OUTPUT: u32 = 3;
const STORAGE_PRIVATE: u32 = 6;

const DECORATION_BUILT_IN: u32 = 11;
const BUILT_IN_BASE_VERTEX: u32 = 4424;
const BUILT_IN_BASE_INSTANCE: u32 = 4425;
const BUILT_IN_DRAW_INDEX: u32 = 4426;
const CAPABILITY_DRAW_PARAMETERS: u32 = 4427;
const BUILT_IN_CLIP_DISTANCE: u32 = 3;
const BUILT_IN_CULL_DISTANCE: u32 = 4;
const OP_CAPABILITY: u32 = 17;
const CAPABILITY_CLIP_DISTANCE: u32 = 32;
const CAPABILITY_CULL_DISTANCE: u32 = 33;
const DECORATION_NO_PERSPECTIVE: u32 = 13;
const DECORATION_FLAT: u32 = 14;
const DECORATION_PATCH: u32 = 15;
const DECORATION_CENTROID: u32 = 16;
const DECORATION_SAMPLE: u32 = 17;
const DECORATION_LOCATION: u32 = 30;
const DECORATION_COMPONENT: u32 = 31;
/// What a merged variable carries instead of its members.
const MOVED_DECORATIONS: [u32; 6] = [
    DECORATION_NO_PERSPECTIVE,
    DECORATION_FLAT,
    DECORATION_CENTROID,
    DECORATION_SAMPLE,
    DECORATION_LOCATION,
    DECORATION_COMPONENT,
];

fn opcode(inst: &[u32]) -> u32 {
    inst[0] & 0xffff
}

fn head(opcode: u32, words: usize) -> u32 {
    ((words as u32) << 16) | opcode
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty {
    Scalar,
    Vector { component: u32, count: u32 },
    Pointer { storage: u32, pointee: u32 },
}

/// One variable of a location: where its components start, how many, and its pointee type.
struct Member {
    var: u32,
    index: usize,
    offset: u32,
    count: u32,
    pointee: u32,
}

struct Group {
    storage: u32,
    location: u32,
    base: u32,
    members: Vec<Member>,
    /// The decorations every member shares besides location and component, which the merged variable takes.
    shared: Vec<(u32, Vec<u32>)>,
}

/// How many locations were merged, and how many were left as the translator wrote them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Merged {
    pub groups: usize,
    /// Locations a single variable held, taken to four components.
    pub widened: usize,
    pub kept: usize,
}

/// A merged variable: the vector it holds, a zero to start an output from, and the members it stands for.
struct Merge<'a> {
    var: u32,
    vec4: u32,
    base: u32,
    null: u32,
    members: &'a [Member],
}

struct Builder<'a> {
    bound: &'a mut u32,
    tys: HashMap<u32, Ty>,
    defined_at: HashMap<u32, usize>,
    private_pointers: HashMap<u32, u32>,
    /// New instructions, keyed by the index of the instruction they go before.
    before: BTreeMap<usize, Vec<Vec<u32>>>,
}

impl Builder<'_> {
    fn fresh(&mut self) -> u32 {
        let id = *self.bound;
        *self.bound += 1;
        id
    }

    fn emit(&mut self, at: usize, inst: Vec<u32>) {
        self.before.entry(at).or_default().push(inst);
    }

    /// A `Private` pointer to `pointee`, declared before instruction `at` unless one is declared earlier.
    fn private_pointer(&mut self, pointee: u32, at: usize) -> u32 {
        if let Some(&id) = self.private_pointers.get(&pointee) {
            return id;
        }
        let wanted = Ty::Pointer {
            storage: STORAGE_PRIVATE,
            pointee,
        };
        // A type can be declared twice, one a pass of this module put ahead of the translator's own, and the lowest id is taken, since a hash map's order is not the same twice
        let existing = self
            .tys
            .iter()
            .filter(|&(id, &ty)| ty == wanted && self.defined_at.get(id).is_some_and(|&d| d < at))
            .map(|(&id, _)| id)
            .min();
        let id = existing.unwrap_or_else(|| {
            let id = self.fresh();
            self.emit(
                at,
                vec![head(OP_TYPE_POINTER, 4), id, STORAGE_PRIVATE, pointee],
            );
            self.tys.insert(id, wanted);
            id
        });
        self.private_pointers.insert(pointee, id);
        id
    }

    /// The vector type of four `base`, declared before instruction `at` unless one exists.
    fn vec4_of(&mut self, base: u32, at: usize) -> u32 {
        let wanted = Ty::Vector {
            component: base,
            count: 4,
        };
        let existing = self
            .tys
            .iter()
            .filter(|&(_, &ty)| ty == wanted)
            .map(|(&id, _)| id)
            .min();
        existing.unwrap_or_else(|| {
            let id = self.fresh();
            self.emit(at, vec![head(OP_TYPE_VECTOR, 4), id, base, 4]);
            self.tys.insert(id, wanted);
            id
        })
    }
}

/// A shared decoration list, sorted so two variables compare equal.
fn shared(decorations: &[(usize, u32, Vec<u32>)]) -> Vec<(u32, Vec<u32>)> {
    let mut list: Vec<(u32, Vec<u32>)> = decorations
        .iter()
        .filter(|(_, d, _)| *d != DECORATION_LOCATION && *d != DECORATION_COMPONENT)
        .map(|(_, d, operands)| (*d, operands.clone()))
        .collect();
    list.sort();
    list
}

type Decorations = HashMap<u32, Vec<(usize, u32, Vec<u32>)>>;

/// The storage class of the locations this module's stage has taken to four components: a vertex stage's outputs and a fragment stage's inputs.
///
/// ⚠ The stage beside it types the same location by what it uses, a `float3` where this one has a `float4`, and Metal refuses the pair. Taking every one of them to four components makes any two stages agree.
fn widened_storage(insts: &[Vec<u32>]) -> Option<u32> {
    let entry = insts
        .iter()
        .find(|i| opcode(i) == OP_ENTRY_POINT && i.len() > 3)?;
    match entry[1] {
        EXECUTION_MODEL_VERTEX => Some(STORAGE_OUTPUT),
        EXECUTION_MODEL_FRAGMENT => Some(STORAGE_INPUT),
        _ => None,
    }
}

/// The groups of interface variables that share a location and can be merged, and a count of those that cannot.
///
/// A location held by one variable of fewer than four components is a group of one when its storage class is `widen`.
fn find_groups(
    insts: &[Vec<u32>],
    first_function: usize,
    tys: &HashMap<u32, Ty>,
    decorations: &Decorations,
    widen: Option<u32>,
) -> (Vec<Group>, usize) {
    type Found = (Member, u32, Vec<(u32, Vec<u32>)>);
    let mut by_location: BTreeMap<(u32, u32), Vec<Found>> = BTreeMap::new();
    for (index, inst) in insts[..first_function].iter().enumerate() {
        if opcode(inst) != OP_VARIABLE || inst.len() < 4 {
            continue;
        }
        let (pointer, var, storage) = (inst[1], inst[2], inst[3]);
        if storage != STORAGE_INPUT && storage != STORAGE_OUTPUT {
            continue;
        }
        let Some(list) = decorations.get(&var) else {
            continue;
        };
        if list
            .iter()
            .any(|(_, d, _)| *d == DECORATION_BUILT_IN || *d == DECORATION_PATCH)
        {
            continue;
        }
        let operand = |decoration: u32| {
            list.iter()
                .find(|(_, d, _)| *d == decoration)
                .map(|(_, _, operands)| operands.first().copied().unwrap_or(0))
        };
        let Some(location) = operand(DECORATION_LOCATION) else {
            continue;
        };
        let offset = operand(DECORATION_COMPONENT).unwrap_or(0);
        let Some(&Ty::Pointer { pointee, .. }) = tys.get(&pointer) else {
            continue;
        };
        let (base, count) = match tys.get(&pointee) {
            Some(Ty::Scalar) => (pointee, 1),
            Some(&Ty::Vector { component, count }) => (component, count),
            _ => continue,
        };
        by_location.entry((storage, location)).or_default().push((
            Member {
                var,
                index,
                offset,
                count,
                pointee,
            },
            base,
            shared(list),
        ));
    }

    let mut groups = Vec::new();
    let mut kept = 0;
    for ((storage, location), mut members) in by_location {
        let lone = members.len() == 1;
        if lone && (widen != Some(storage) || members[0].0.count >= 4) {
            continue;
        }
        members.sort_by_key(|(m, _, _)| m.offset);
        let base = members[0].1;
        let common = members[0].2.clone();
        let same = members.iter().all(|(_, b, s)| *b == base && *s == common);
        let disjoint = members
            .windows(2)
            .all(|w| w[0].0.offset + w[0].0.count <= w[1].0.offset)
            && members
                .last()
                .is_some_and(|(m, _, _)| m.offset + m.count <= 4);
        if same && disjoint {
            groups.push(Group {
                storage,
                location,
                base,
                members: members.into_iter().map(|(m, _, _)| m).collect(),
                shared: common,
            });
        } else {
            kept += 1;
        }
    }
    (groups, kept)
}

/// What the global section says about types and decorations, which the passes of this module read.
struct Scan {
    first_function: usize,
    tys: HashMap<u32, Ty>,
    defined_at: HashMap<u32, usize>,
    decorations: Decorations,
    last_annotation: Option<usize>,
}

fn scan(insts: &[Vec<u32>]) -> Scan {
    let first_function = insts
        .iter()
        .position(|i| opcode(i) == OP_FUNCTION)
        .unwrap_or(insts.len());
    let mut found = Scan {
        first_function,
        tys: HashMap::new(),
        defined_at: HashMap::new(),
        decorations: HashMap::new(),
        last_annotation: None,
    };
    for (index, inst) in insts[..first_function].iter().enumerate() {
        match opcode(inst) {
            OP_TYPE_INT | OP_TYPE_FLOAT if inst.len() >= 3 => {
                found.tys.insert(inst[1], Ty::Scalar);
                found.defined_at.insert(inst[1], index);
            }
            OP_TYPE_VECTOR if inst.len() == 4 => {
                let ty = Ty::Vector {
                    component: inst[2],
                    count: inst[3],
                };
                found.tys.insert(inst[1], ty);
                found.defined_at.insert(inst[1], index);
            }
            OP_TYPE_POINTER if inst.len() == 4 => {
                let ty = Ty::Pointer {
                    storage: inst[2],
                    pointee: inst[3],
                };
                found.tys.insert(inst[1], ty);
                found.defined_at.insert(inst[1], index);
            }
            OP_DECORATE if inst.len() >= 3 => {
                found.decorations.entry(inst[1]).or_default().push((
                    index,
                    inst[2],
                    inst[3..].to_vec(),
                ));
                found.last_annotation = Some(index);
            }
            OP_MEMBER_DECORATE => found.last_annotation = Some(index),
            _ => {}
        }
    }
    found
}

/// Give each access chain into a variable made private a private pointer type, declared before the first function, keyed by the chain's index.
fn retype_chains(
    insts: &[Vec<u32>],
    first_function: usize,
    builder: &mut Builder<'_>,
    privatized: impl IntoIterator<Item = u32>,
) -> HashMap<usize, u32> {
    let mut chain_types: HashMap<usize, u32> = HashMap::new();
    let mut derived: HashSet<u32> = privatized.into_iter().collect();
    for (offset, inst) in insts[first_function..].iter().enumerate() {
        let is_chain = matches!(opcode(inst), OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN);
        if is_chain && inst.len() >= 5 && derived.contains(&inst[3]) {
            if let Some(&Ty::Pointer { pointee, .. }) = builder.tys.get(&inst[1]) {
                chain_types.insert(
                    first_function + offset,
                    builder.private_pointer(pointee, first_function),
                );
                derived.insert(inst[2]);
            }
        }
    }
    chain_types
}

/// Merge every mergeable location of `insts`, which are the instructions after the header, taking new ids from `bound`.
pub(super) fn merge_components(insts: Vec<Vec<u32>>, bound: &mut u32) -> (Vec<Vec<u32>>, Merged) {
    let Scan {
        first_function,
        tys,
        defined_at,
        decorations,
        last_annotation,
    } = scan(&insts);
    let (groups, kept) = find_groups(
        &insts,
        first_function,
        &tys,
        &decorations,
        widened_storage(&insts),
    );
    if groups.is_empty() || last_annotation.is_none() {
        return (
            insts,
            Merged {
                groups: 0,
                widened: 0,
                kept,
            },
        );
    }
    let annotation_end = last_annotation.map_or(0, |i| i + 1);

    let mut builder = Builder {
        bound,
        tys,
        defined_at,
        private_pointers: HashMap::new(),
        before: BTreeMap::new(),
    };

    // Each member variable becomes a private copy, in module order so its pointer type precedes it.
    let mut removed: HashSet<usize> = HashSet::new();
    let mut replaced: HashMap<usize, Vec<u32>> = HashMap::new();
    let mut members: Vec<&Member> = groups.iter().flat_map(|g| g.members.iter()).collect();
    members.sort_by_key(|m| m.index);
    for member in &members {
        let pointer = builder.private_pointer(member.pointee, member.index);
        replaced.insert(
            member.index,
            vec![head(OP_VARIABLE, 4), pointer, member.var, STORAGE_PRIVATE],
        );
        for (index, decoration, _) in decorations.get(&member.var).into_iter().flatten() {
            if MOVED_DECORATIONS.contains(decoration) {
                removed.insert(*index);
            }
        }
    }

    // The merged variables, their types and decorations.
    let mut merges: Vec<(u32, Merge<'_>)> = Vec::new();
    let mut nulls: HashMap<u32, u32> = HashMap::new();
    for group in &groups {
        let vec4 = builder.vec4_of(group.base, first_function);
        let pointer = builder.fresh();
        builder.emit(
            first_function,
            vec![head(OP_TYPE_POINTER, 4), pointer, group.storage, vec4],
        );
        let var = builder.fresh();
        builder.emit(
            first_function,
            vec![head(OP_VARIABLE, 4), pointer, var, group.storage],
        );
        let null = *nulls.entry(vec4).or_insert_with(|| {
            let id = builder.fresh();
            builder.emit(first_function, vec![head(OP_CONSTANT_NULL, 3), vec4, id]);
            id
        });
        builder.emit(
            annotation_end,
            vec![
                head(OP_DECORATE, 4),
                var,
                DECORATION_LOCATION,
                group.location,
            ],
        );
        for (decoration, operands) in &group.shared {
            let mut inst = vec![head(OP_DECORATE, 3 + operands.len()), var, *decoration];
            inst.extend_from_slice(operands);
            builder.emit(annotation_end, inst);
        }
        merges.push((
            group.storage,
            Merge {
                var,
                vec4,
                base: group.base,
                null,
                members: &group.members,
            },
        ));
    }

    // An access chain into a member needs a private pointer type, declared before the first function.
    let chain_types = retype_chains(
        &insts,
        first_function,
        &mut builder,
        members.iter().map(|m| m.var),
    );

    let entries: HashSet<u32> = insts
        .iter()
        .filter(|i| opcode(i) == OP_ENTRY_POINT && i.len() > 3)
        .map(|i| i[2])
        .collect();
    let mut out: Vec<Vec<u32>> = Vec::with_capacity(insts.len() + 64);
    let mut in_entry = false;
    let mut label_due = false;
    let mut prologue_due = false;
    for (index, inst) in insts.iter().enumerate() {
        if let Some(extra) = builder.before.remove(&index) {
            out.extend(extra);
        }
        if removed.contains(&index) {
            continue;
        }
        if let Some(var) = replaced.remove(&index) {
            out.push(var);
            continue;
        }
        match opcode(inst) {
            OP_ENTRY_POINT => {
                let mut entry = inst.clone();
                entry.extend(merges.iter().map(|(_, m)| m.var));
                entry[0] = head(OP_ENTRY_POINT, entry.len());
                out.push(entry);
                continue;
            }
            OP_FUNCTION => {
                in_entry = entries.contains(&inst[2]);
                label_due = in_entry;
            }
            OP_FUNCTION_END => in_entry = false,
            OP_LABEL if label_due => {
                label_due = false;
                prologue_due = true;
            }
            OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN => {
                if let Some(&pointer) = chain_types.get(&index) {
                    let mut chain = inst.clone();
                    chain[1] = pointer;
                    out.push(chain);
                    continue;
                }
            }
            _ => {}
        }
        if prologue_due && !matches!(opcode(inst), OP_VARIABLE | OP_LABEL) {
            prologue_due = false;
            for (_, merge) in merges.iter().filter(|(s, _)| *s == STORAGE_INPUT) {
                copy_in(&mut builder, &mut out, merge);
            }
        }
        if in_entry && opcode(inst) == OP_RETURN {
            for (_, merge) in merges.iter().filter(|(s, _)| *s == STORAGE_OUTPUT) {
                copy_out(&mut builder, &mut out, merge);
            }
        }
        out.push(inst.clone());
    }
    let widened = groups.iter().filter(|g| g.members.len() == 1).count();
    let merged = Merged {
        groups: groups.len() - widened,
        widened,
        kept,
    };
    (out, merged)
}

/// Turn the clip and cull distance variables into private ones, which keeps the shader body and drops the distances.
///
/// The device offers neither, so a shader that writes one would not build. The distances no longer
/// clip anything: this is an approximation, and it is counted.
pub(super) fn drop_clip_distances(insts: Vec<Vec<u32>>, bound: &mut u32) -> (Vec<Vec<u32>>, usize) {
    let Scan {
        first_function,
        tys,
        defined_at,
        decorations,
        ..
    } = scan(&insts);
    let mut variables: Vec<(usize, u32, u32)> = Vec::new();
    let mut removed: HashSet<usize> = HashSet::new();
    for (index, inst) in insts[..first_function].iter().enumerate() {
        if opcode(inst) != OP_VARIABLE
            || inst.len() < 4
            || !matches!(inst[3], STORAGE_INPUT | STORAGE_OUTPUT)
        {
            continue;
        }
        let Some(list) = decorations.get(&inst[2]) else {
            continue;
        };
        let built_in = list.iter().find(|(_, d, operands)| {
            *d == DECORATION_BUILT_IN
                && matches!(
                    operands.first(),
                    Some(&BUILT_IN_CLIP_DISTANCE) | Some(&BUILT_IN_CULL_DISTANCE)
                )
        });
        let (Some((decoration_index, _, _)), Some(&Ty::Pointer { pointee, .. })) =
            (built_in, tys.get(&inst[1]))
        else {
            continue;
        };
        removed.insert(*decoration_index);
        variables.push((index, inst[2], pointee));
    }
    if variables.is_empty() {
        return (insts, 0);
    }

    let mut builder = Builder {
        bound,
        tys,
        defined_at,
        private_pointers: HashMap::new(),
        before: BTreeMap::new(),
    };
    let mut replaced: HashMap<usize, Vec<u32>> = HashMap::new();
    for &(index, var, pointee) in &variables {
        let pointer = builder.private_pointer(pointee, index);
        replaced.insert(
            index,
            vec![head(OP_VARIABLE, 4), pointer, var, STORAGE_PRIVATE],
        );
    }
    let chain_types = retype_chains(
        &insts,
        first_function,
        &mut builder,
        variables.iter().map(|&(_, var, _)| var),
    );

    let mut out: Vec<Vec<u32>> = Vec::with_capacity(insts.len() + 4);
    for (index, inst) in insts.iter().enumerate() {
        if let Some(extra) = builder.before.remove(&index) {
            out.extend(extra);
        }
        if removed.contains(&index) {
            continue;
        }
        if let Some(var) = replaced.remove(&index) {
            out.push(var);
            continue;
        }
        let capability = opcode(inst) == OP_CAPABILITY
            && inst.len() == 2
            && matches!(inst[1], CAPABILITY_CLIP_DISTANCE | CAPABILITY_CULL_DISTANCE);
        if capability {
            continue;
        }
        match chain_types.get(&index) {
            Some(&pointer) => {
                let mut chain = inst.clone();
                chain[1] = pointer;
                out.push(chain);
            }
            None => out.push(inst.clone()),
        }
    }
    (out, variables.len())
}

/// Turn the base vertex, base instance and draw index inputs into private variables that hold nought.
///
/// Direct3D 12 has no register for them and naga's HLSL writer refuses them, and a draw of this renderer's starts at vertex and instance nought of its first draw, so nought is what they read. A game shader's own vertex stage reads them as the translator's `InstanceIndex - BaseInstance`.
pub(super) fn zero_base_parameters(
    insts: Vec<Vec<u32>>,
    bound: &mut u32,
) -> (Vec<Vec<u32>>, usize) {
    let Scan {
        first_function,
        tys,
        defined_at,
        decorations,
        ..
    } = scan(&insts);
    let mut variables: Vec<(usize, u32, u32)> = Vec::new();
    let mut removed: HashSet<usize> = HashSet::new();
    for (index, inst) in insts[..first_function].iter().enumerate() {
        if opcode(inst) != OP_VARIABLE || inst.len() < 4 || inst[3] != STORAGE_INPUT {
            continue;
        }
        let Some(list) = decorations.get(&inst[2]) else {
            continue;
        };
        let built_in = list.iter().find(|(_, d, operands)| {
            *d == DECORATION_BUILT_IN
                && matches!(
                    operands.first(),
                    Some(&BUILT_IN_BASE_VERTEX)
                        | Some(&BUILT_IN_BASE_INSTANCE)
                        | Some(&BUILT_IN_DRAW_INDEX)
                )
        });
        let (Some((decoration_index, _, _)), Some(&Ty::Pointer { pointee, .. })) =
            (built_in, tys.get(&inst[1]))
        else {
            continue;
        };
        removed.insert(*decoration_index);
        variables.push((index, inst[2], pointee));
    }
    if variables.is_empty() {
        return (insts, 0);
    }

    let mut builder = Builder {
        bound,
        tys,
        defined_at,
        private_pointers: HashMap::new(),
        before: BTreeMap::new(),
    };
    let mut replaced: HashMap<usize, Vec<u32>> = HashMap::new();
    for &(index, var, pointee) in &variables {
        let pointer = builder.private_pointer(pointee, index);
        let zero = builder.fresh();
        builder.emit(index, vec![head(OP_CONSTANT_NULL, 3), pointee, zero]);
        replaced.insert(
            index,
            vec![head(OP_VARIABLE, 5), pointer, var, STORAGE_PRIVATE, zero],
        );
    }

    let mut out: Vec<Vec<u32>> = Vec::with_capacity(insts.len() + 4);
    for (index, inst) in insts.iter().enumerate() {
        if let Some(extra) = builder.before.remove(&index) {
            out.extend(extra);
        }
        if removed.contains(&index) {
            continue;
        }
        if let Some(var) = replaced.remove(&index) {
            out.push(var);
            continue;
        }
        let capability = opcode(inst) == OP_CAPABILITY
            && inst.len() == 2
            && inst[1] == CAPABILITY_DRAW_PARAMETERS;
        if !capability {
            out.push(inst.clone());
        }
    }
    (out, variables.len())
}

/// Load a merged input and hand each member variable its components.
fn copy_in(builder: &mut Builder<'_>, out: &mut Vec<Vec<u32>>, merge: &Merge<'_>) {
    let whole = builder.fresh();
    out.push(vec![head(OP_LOAD, 4), merge.vec4, whole, merge.var]);
    for member in merge.members {
        let value = builder.fresh();
        if member.count == 1 {
            out.push(vec![
                head(OP_COMPOSITE_EXTRACT, 5),
                merge.base,
                value,
                whole,
                member.offset,
            ]);
        } else {
            let mut shuffle = vec![
                head(OP_VECTOR_SHUFFLE, 5 + member.count as usize),
                member.pointee,
                value,
                whole,
                whole,
            ];
            shuffle.extend(member.offset..member.offset + member.count);
            out.push(shuffle);
        }
        out.push(vec![head(OP_STORE, 3), member.var, value]);
    }
}

/// Build a merged output from its member variables, starting from zero, and store it.
fn copy_out(builder: &mut Builder<'_>, out: &mut Vec<Vec<u32>>, merge: &Merge<'_>) {
    let mut whole = merge.null;
    for member in merge.members {
        let value = builder.fresh();
        out.push(vec![head(OP_LOAD, 4), member.pointee, value, member.var]);
        for lane in 0..member.count {
            let element = if member.count == 1 {
                value
            } else {
                let id = builder.fresh();
                out.push(vec![
                    head(OP_COMPOSITE_EXTRACT, 5),
                    merge.base,
                    id,
                    value,
                    lane,
                ]);
                id
            };
            let next = builder.fresh();
            out.push(vec![
                head(OP_COMPOSITE_INSERT, 6),
                merge.vec4,
                next,
                element,
                whole,
                member.offset + lane,
            ]);
            whole = next;
        }
    }
    out.push(vec![head(OP_STORE, 3), merge.var, whole]);
}

#[cfg(test)]
mod tests;
