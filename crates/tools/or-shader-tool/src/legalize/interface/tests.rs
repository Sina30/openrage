use super::*;
use crate::legalize::{instructions, legalize};
use crate::spirv_testing::{
    dwords, entry, fragment_with_clip_distance_input, naga_reads, naga_validates,
    naga_validates_without_clip_distance, op, vertex_with_clip_distance,
};

/// A fragment shader that passes a vec3 and a float, which a translator would put at one location as two variables, to a vec3 and a float at another.
///
/// `second_input_is_uint` makes the inputs unmergeable; `through_chains` reaches the vec3's second component with an access chain.
fn split_fragment(second_input_is_uint: bool, through_chains: bool) -> Vec<u8> {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 40, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![4, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.extend([10, 11, 12, 13]);
    words.extend(op(15, &entry_words));
    words.extend(op(16, &[1, 7]));
    for (var, location, component) in [(10, 1, 0), (11, 1, 3), (12, 0, 0), (13, 0, 3)] {
        words.extend(op(71, &[var, 30, location]));
        words.extend(op(71, &[var, 31, component]));
    }
    // The two inputs are interpolated without perspective.
    words.extend(op(71, &[10, 13]));
    words.extend(op(71, &[11, 13]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[4, 32]));
    words.extend(op(21, &[16, 32, 0]));
    words.extend(op(23, &[5, 4, 3]));
    let second = if second_input_is_uint { 16 } else { 4 };
    words.extend(op(32, &[6, 1, 5]));
    words.extend(op(32, &[7, 1, second]));
    words.extend(op(32, &[8, 3, 5]));
    words.extend(op(32, &[9, 3, 4]));
    words.extend(op(59, &[6, 10, 1]));
    words.extend(op(59, &[7, 11, 1]));
    words.extend(op(59, &[8, 12, 3]));
    words.extend(op(59, &[9, 13, 3]));
    words.extend(op(43, &[16, 20, 1]));
    words.extend(op(32, &[21, 1, 4]));
    words.extend(op(32, &[22, 3, 4]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[14]));
    if through_chains {
        words.extend(op(65, &[21, 25, 10, 20]));
        words.extend(op(61, &[4, 26, 25]));
        words.extend(op(65, &[22, 27, 12, 20]));
        words.extend(op(62, &[27, 26]));
    }
    words.extend(op(61, &[5, 15, 10]));
    words.extend(op(62, &[12, 15]));
    if second_input_is_uint {
        words.extend(op(61, &[16, 28, 11]));
        words.extend(op(112, &[4, 29, 28]));
        words.extend(op(62, &[13, 29]));
    } else {
        words.extend(op(61, &[4, 16 + 14, 11]));
        words.extend(op(62, &[13, 30]));
    }
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

fn words_of(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[test]
fn naga_refuses_two_variables_at_one_location_until_they_are_merged() {
    let original = split_fragment(false, false);
    let before = naga_validates(&original).unwrap_err();
    assert!(before.contains("BindingCollision"), "{before}");
    let (legal, lowerings) = legalize(&original).unwrap();
    let module = naga_validates(&legal).expect("naga validates the merged module");
    assert_eq!(
        (
            lowerings.interface_groups_merged,
            lowerings.interface_groups_kept
        ),
        (2, 0)
    );
    let arguments = &module.entry_points[0].function.arguments;
    assert_eq!(arguments.len(), 1, "one input variable now");
    let Some(naga::Binding::Location {
        location,
        interpolation,
        ..
    }) = &arguments[0].binding
    else {
        panic!("a location binding");
    };
    assert_eq!(*location, 1);
    assert_eq!(*interpolation, Some(naga::Interpolation::Linear));
    assert!(matches!(
        module.types[arguments[0].ty].inner,
        naga::TypeInner::Vector {
            size: naga::VectorSize::Quad,
            ..
        }
    ));
}

#[test]
fn the_merged_input_is_copied_to_its_members_by_component_and_the_output_is_built_in_order() {
    let (legal, _) = legalize(&split_fragment(false, false)).unwrap();
    let words = words_of(&legal);
    let insts = instructions(&words).unwrap();
    let shuffles: Vec<&[u32]> = insts
        .iter()
        .filter(|i| opcode(i) == OP_VECTOR_SHUFFLE)
        .copied()
        .collect();
    assert_eq!(shuffles.len(), 1);
    assert_eq!(
        &shuffles[0][5..],
        &[0, 1, 2],
        "the vec3 takes components x, y and z"
    );
    let extracts: Vec<u32> = insts
        .iter()
        .filter(|i| opcode(i) == OP_COMPOSITE_EXTRACT && i.len() == 5)
        .map(|i| i[4])
        .collect();
    assert_eq!(
        extracts,
        vec![3, 0, 1, 2],
        "the input's float takes component w, then the output's vec3 is taken apart for x, y and z"
    );
    let inserts: Vec<u32> = insts
        .iter()
        .filter(|i| opcode(i) == OP_COMPOSITE_INSERT)
        .map(|i| i[5])
        .collect();
    assert_eq!(inserts, vec![0, 1, 2, 3], "the output is built x, y, z, w");
    let stores = insts.iter().filter(|i| opcode(i) == OP_STORE).count();
    assert_eq!(
        stores, 5,
        "two copies into members, the shader's own two stores and the merged output's store"
    );
}

#[test]
fn a_member_reached_by_an_access_chain_is_still_a_valid_pointer() {
    let original = split_fragment(false, true);
    assert!(naga_validates(&original).is_err());
    let (legal, lowerings) = legalize(&original).unwrap();
    naga_validates(&legal).expect("naga validates a module that indexes a member");
    assert_eq!(lowerings.interface_groups_merged, 2);
}

#[test]
fn variables_of_different_types_at_one_location_are_left_and_counted() {
    let original = split_fragment(true, false);
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!(
        (
            lowerings.interface_groups_merged,
            lowerings.interface_groups_kept
        ),
        (1, 1),
        "the outputs merge, the float and uint inputs do not"
    );
    let error = naga_validates(&legal).unwrap_err();
    assert!(error.contains("BindingCollision"), "{error}");
}

#[test]
fn a_module_with_no_shared_location_is_returned_as_it_was() {
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 5, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    words.extend(entry(4, 1));
    words.extend(op(16, &[1, 7]));
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[4]));
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    let original = dwords(&words);
    naga_reads(&original).unwrap();
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!(legal, original);
    assert_eq!(lowerings.interface_groups_merged, 0);
}

#[test]
fn the_id_bound_grows_to_cover_the_ids_the_merge_takes() {
    let original = split_fragment(false, false);
    let (legal, _) = legalize(&original).unwrap();
    assert!(words_of(&legal)[3] > words_of(&original)[3]);
}

/// The storage class of every global variable, and of every pointer type, in a module.
fn storage_classes(words: &[u32]) -> (HashMap<u32, u32>, HashMap<u32, u32>) {
    let mut variables = HashMap::new();
    let mut pointers = HashMap::new();
    for inst in instructions(words).unwrap() {
        match opcode(inst) {
            OP_VARIABLE => {
                variables.insert(inst[2], inst[3]);
            }
            OP_TYPE_POINTER => {
                pointers.insert(inst[1], inst[2]);
            }
            _ => {}
        }
    }
    (variables, pointers)
}

#[test]
fn only_the_merged_variables_keep_a_location_and_the_members_are_private() {
    let (legal, _) = legalize(&split_fragment(false, false)).unwrap();
    let words = words_of(&legal);
    let (variables, _) = storage_classes(&words);
    for inst in instructions(&words).unwrap() {
        if opcode(inst) == OP_DECORATE
            && matches!(
                inst[2],
                DECORATION_LOCATION | DECORATION_COMPONENT | DECORATION_NO_PERSPECTIVE
            )
        {
            assert!(
                matches!(variables[&inst[1]], STORAGE_INPUT | STORAGE_OUTPUT),
                "decoration {} sits on a variable of storage class {}",
                inst[2],
                variables[&inst[1]]
            );
        }
    }
    for member in [10, 11, 12, 13] {
        assert_eq!(variables[&member], STORAGE_PRIVATE);
    }
}

#[test]
fn an_access_chain_into_a_private_member_has_a_private_pointer_type() {
    let (legal, _) = legalize(&split_fragment(false, true)).unwrap();
    let words = words_of(&legal);
    let (variables, pointers) = storage_classes(&words);
    let mut chains = 0;
    for inst in instructions(&words).unwrap() {
        if opcode(inst) == OP_ACCESS_CHAIN && variables.get(&inst[3]) == Some(&STORAGE_PRIVATE) {
            chains += 1;
            assert_eq!(
                pointers[&inst[1]], STORAGE_PRIVATE,
                "the result of a chain into a private variable"
            );
        }
    }
    assert_eq!(chains, 2);
}

#[test]
fn a_clip_distance_a_device_lacks_is_made_private_and_counted() {
    let original = vertex_with_clip_distance(false);
    let before = naga_validates_without_clip_distance(&original).unwrap_err();
    assert!(before.contains("CLIP_DISTANCE"), "{before}");
    let (legal, lowerings) = legalize(&original).unwrap();
    naga_validates_without_clip_distance(&legal).expect("valid without the capability");
    assert_eq!(lowerings.clip_distances_dropped, 1);
    let words = words_of(&legal);
    let (variables, _) = storage_classes(&words);
    assert_eq!(
        variables[&6], STORAGE_PRIVATE,
        "the clip distance variable is private now"
    );
    assert!(
        !instructions(&words)
            .unwrap()
            .iter()
            .any(|i| opcode(i) == OP_CAPABILITY && i[1] == CAPABILITY_CLIP_DISTANCE),
        "the capability is gone"
    );
    let still_built_in = instructions(&words)
        .unwrap()
        .iter()
        .filter(|i| opcode(i) == OP_DECORATE && i[2] == DECORATION_BUILT_IN)
        .map(|i| i[1])
        .collect::<Vec<_>>();
    assert_eq!(
        still_built_in,
        vec![5],
        "only the position keeps its built-in decoration"
    );
}

#[test]
fn a_store_through_an_access_chain_into_a_dropped_clip_distance_is_retyped() {
    let original = vertex_with_clip_distance(true);
    assert!(naga_validates_without_clip_distance(&original).is_err());
    let (legal, lowerings) = legalize(&original).unwrap();
    naga_validates_without_clip_distance(&legal).expect("valid once the pointer is private");
    assert_eq!(lowerings.clip_distances_dropped, 1);
    let words = words_of(&legal);
    let (variables, pointers) = storage_classes(&words);
    let chains: Vec<&[u32]> = instructions(&words)
        .unwrap()
        .into_iter()
        .filter(|i| opcode(i) == OP_ACCESS_CHAIN && variables.get(&i[3]) == Some(&STORAGE_PRIVATE))
        .collect();
    assert_eq!(chains.len(), 1);
    assert_eq!(pointers[&chains[0][1]], STORAGE_PRIVATE);
}

#[test]
fn a_clip_distance_a_fragment_shader_reads_becomes_a_private_variable_it_can_still_read() {
    let original = fragment_with_clip_distance_input();
    let before = naga_validates(&original).unwrap_err();
    assert!(before.contains("ClipDistance"), "{before}");
    let (legal, lowerings) = legalize(&original).unwrap();
    naga_validates(&legal).expect("naga validates a fragment shader with no clip distance input");
    assert_eq!(lowerings.clip_distances_dropped, 1);
}

/// A shader of `model` (0 vertex, 4 fragment) with one variable of `storage` (1 input, 3 output) at `location` and `component`: `count` components of `scalar` (0 float, 1 unsigned integer), `flat` when it is decorated so.
fn lone_interface(
    model: u32,
    storage: u32,
    scalar: u32,
    count: u32,
    location: u32,
    component: u32,
    flat: bool,
) -> Vec<u8> {
    let base = if scalar == 0 { 4 } else { 5 };
    let pointee = if count == 1 { base } else { 6 };
    let mut words = vec![0x0723_0203, 0x0001_0000, 0, 12, 0];
    words.extend(op(17, &[1]));
    words.extend(op(14, &[0, 1]));
    let mut entry_words = vec![model, 1];
    entry_words.extend(crate::spirv_testing::string("main"));
    entry_words.push(8);
    words.extend(op(15, &entry_words));
    if model == 4 {
        words.extend(op(16, &[1, 7]));
    }
    words.extend(op(71, &[8, 30, location]));
    if component != 0 {
        words.extend(op(71, &[8, 31, component]));
    }
    if flat {
        words.extend(op(71, &[8, 14]));
    }
    words.extend(op(19, &[2]));
    words.extend(op(33, &[3, 2]));
    words.extend(op(22, &[4, 32]));
    words.extend(op(21, &[5, 32, 0]));
    if count > 1 {
        words.extend(op(23, &[6, base, count]));
    }
    words.extend(op(32, &[7, storage, pointee]));
    words.extend(op(59, &[7, 8, storage]));
    if storage == 3 {
        words.extend(op(46, &[pointee, 9]));
    }
    words.extend(op(54, &[2, 1, 0, 3]));
    words.extend(op(248, &[10]));
    if storage == 1 {
        words.extend(op(61, &[pointee, 11, 8]));
    } else {
        words.extend(op(62, &[8, 9]));
    }
    words.extend(op(253, &[]));
    words.extend(op(56, &[]));
    dwords(&words)
}

/// For each interface variable that has a location: its storage class, location and component count (1 for a scalar).
fn interface_shapes(bytes: &[u8]) -> Vec<(u32, u32, u32)> {
    let words = words_of(bytes);
    let insts = instructions(&words).unwrap();
    let mut locations = HashMap::new();
    let mut sizes = HashMap::new();
    let mut pointers = HashMap::new();
    for inst in &insts {
        match opcode(inst) {
            OP_DECORATE if inst[2] == DECORATION_LOCATION => {
                locations.insert(inst[1], inst[3]);
            }
            OP_TYPE_FLOAT | OP_TYPE_INT => {
                sizes.insert(inst[1], 1);
            }
            OP_TYPE_VECTOR => {
                sizes.insert(inst[1], inst[3]);
            }
            OP_TYPE_POINTER => {
                pointers.insert(inst[1], inst[3]);
            }
            _ => {}
        }
    }
    let mut shapes: Vec<_> = insts
        .iter()
        .filter(|i| opcode(i) == OP_VARIABLE && locations.contains_key(&i[2]))
        .map(|i| (i[3], locations[&i[2]], sizes[&pointers[&i[1]]]))
        .collect();
    shapes.sort();
    shapes
}

#[test]
fn a_vertex_output_of_three_components_becomes_a_four_component_output() {
    let original = lone_interface(0, 3, 0, 3, 1, 0, false);
    assert_eq!(interface_shapes(&original), vec![(3, 1, 3)]);
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!(interface_shapes(&legal), vec![(3, 1, 4)]);
    assert_eq!(lowerings.interface_locations_widened, 1);
    assert_eq!(lowerings.interface_groups_merged, 0);
}

#[test]
fn a_fragment_input_of_two_components_becomes_a_four_component_input_the_shader_still_reads_two_of()
{
    let original = lone_interface(4, 1, 0, 2, 0, 0, false);
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!(interface_shapes(&legal), vec![(1, 0, 4)]);
    assert_eq!(lowerings.interface_locations_widened, 1);
    let module = naga_reads(&legal).expect("naga reads the widened module");
    let argument = &module.entry_points[0].function.arguments[0];
    assert!(matches!(
        module.types[argument.ty].inner,
        naga::TypeInner::Vector {
            size: naga::VectorSize::Quad,
            ..
        }
    ));
    let words = words_of(&legal);
    let insts = instructions(&words).unwrap();
    let shuffles: Vec<&[u32]> = insts
        .iter()
        .filter(|i| opcode(i) == OP_VECTOR_SHUFFLE)
        .copied()
        .collect();
    assert_eq!(shuffles.len(), 1);
    assert_eq!(
        &shuffles[0][5..],
        &[0, 1],
        "the shader's vec2 takes x and y"
    );
}

#[test]
fn a_variable_at_a_component_offset_is_widened_with_its_own_lanes() {
    let original = lone_interface(4, 1, 0, 2, 2, 2, false);
    let (legal, lowerings) = legalize(&original).unwrap();
    assert_eq!(interface_shapes(&legal), vec![(1, 2, 4)]);
    assert_eq!(lowerings.interface_locations_widened, 1);
    let words = words_of(&legal);
    let insts = instructions(&words).unwrap();
    let shuffle = insts
        .iter()
        .find(|i| opcode(i) == OP_VECTOR_SHUFFLE)
        .expect("a shuffle");
    assert_eq!(&shuffle[5..], &[2, 3], "z and w, where the variable sits");
}

#[test]
fn a_flat_integer_input_is_widened_and_keeps_its_interpolation_and_kind() {
    let original = lone_interface(4, 1, 1, 2, 3, 0, true);
    let (legal, _) = legalize(&original).unwrap();
    let module = naga_reads(&legal).expect("naga reads it");
    let argument = &module.entry_points[0].function.arguments[0];
    let Some(naga::Binding::Location {
        location,
        interpolation,
        ..
    }) = &argument.binding
    else {
        panic!("a location binding");
    };
    assert_eq!(
        (*location, *interpolation),
        (3, Some(naga::Interpolation::Flat))
    );
    assert!(matches!(
        module.types[argument.ty].inner,
        naga::TypeInner::Vector {
            size: naga::VectorSize::Quad,
            scalar: naga::Scalar {
                kind: naga::ScalarKind::Uint,
                ..
            },
        }
    ));
}

#[test]
fn only_a_vertex_stages_outputs_and_a_fragment_stages_inputs_are_widened() {
    // A vertex input, which the buffer's own format decides, and a fragment output, which a render target decides
    for (model, storage) in [(0, 1), (4, 3)] {
        let original = lone_interface(model, storage, 0, 3, 1, 0, false);
        let (legal, lowerings) = legalize(&original).unwrap();
        assert_eq!(legal, original, "model {model} storage {storage}");
        assert_eq!(lowerings.interface_locations_widened, 0);
    }
    // A vertex output and a fragment input that are four components already
    for (model, storage) in [(0, 3), (4, 1)] {
        let original = lone_interface(model, storage, 0, 4, 1, 0, false);
        let (legal, lowerings) = legalize(&original).unwrap();
        assert_eq!(legal, original, "model {model} storage {storage}");
        assert_eq!(lowerings.interface_locations_widened, 0);
    }
}

#[test]
fn a_base_vertex_base_instance_or_draw_index_input_becomes_a_private_variable_of_nought() {
    for (name, built_in) in [
        ("base vertex", BUILT_IN_BASE_VERTEX),
        ("base instance", BUILT_IN_BASE_INSTANCE),
        ("draw index", BUILT_IN_DRAW_INDEX),
    ] {
        let original = crate::spirv_testing::vertex_reading_a_draw_parameter(built_in);
        // naga's strict reader refuses the capability they need, and the writers' own is not strict
        let direct3d = crate::backend_check::check_module(&original);
        assert!(
            direct3d[&crate::backend_check::Backend::Hlsl51].is_err(),
            "the {name} is not one Direct3D's writer takes"
        );

        let (legal, lowerings) = crate::legalize::legalize(&original).unwrap();
        assert_eq!(lowerings.base_parameters_zeroed, 1, "{name}");
        assert!(
            !lowerings.approximates(),
            "{name} is what a draw from nought reads"
        );
        let words: Vec<u32> = legal
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let insts = instructions(&words).unwrap();
        // The one variable that was an input is private and starts at a null of its type
        let variable = insts
            .iter()
            .find(|i| opcode(i) == OP_VARIABLE && i[2] == 5)
            .expect("the variable is still there");
        assert_eq!(
            (variable.len(), variable[3]),
            (5, STORAGE_PRIVATE),
            "{name}"
        );
        let init = insts
            .iter()
            .find(|i| opcode(i) == OP_CONSTANT_NULL && i[2] == variable[4])
            .expect("its initializer is a null");
        assert_eq!(init[1], 9, "a null of the unsigned integer type");
        assert!(
            !insts
                .iter()
                .any(|i| opcode(i) == OP_DECORATE && i[1] == 5 && i[2] == DECORATION_BUILT_IN),
            "{name}: the variable is no built-in any more"
        );
        assert!(
            !insts.iter().any(|i| opcode(i) == OP_CAPABILITY
                && i.len() == 2
                && i[1] == CAPABILITY_DRAW_PARAMETERS),
            "{name}: the capability for them is not asked for"
        );
        naga_validates(&legal).unwrap_or_else(|e| panic!("{name}: {e}"));
        let after = crate::backend_check::check_module(&legal);
        for (backend, result) in after {
            assert_eq!(result, Ok(()), "{name} through {backend:?}");
        }
    }
}

#[test]
fn a_module_with_no_draw_parameter_is_left_as_it_is() {
    let original = vertex_with_clip_distance(false);
    let words: Vec<u32> = original
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let insts: Vec<Vec<u32>> = instructions(&words)
        .unwrap()
        .into_iter()
        .map(<[u32]>::to_vec)
        .collect();
    let mut bound = words[3];
    let (out, zeroed) = zero_base_parameters(insts.clone(), &mut bound);
    assert_eq!((zeroed, bound, out), (0, words[3], insts));
}

/// A global section with two of each type a builder looks for, the way a pass leaves one ahead of the translator's own, then the first function.
fn duplicated_types() -> Vec<Vec<u32>> {
    vec![
        vec![head(OP_TYPE_FLOAT, 3), 1, 32],
        vec![head(OP_TYPE_POINTER, 4), 2, STORAGE_PRIVATE, 1],
        vec![head(OP_TYPE_POINTER, 4), 3, STORAGE_PRIVATE, 1],
        vec![head(OP_TYPE_VECTOR, 4), 4, 1, 4],
        vec![head(OP_TYPE_VECTOR, 4), 5, 1, 4],
        vec![head(OP_FUNCTION, 5), 6, 7, 0, 8],
    ]
}

/// A builder over `insts`, whose hash maps iterate in an order of their own.
fn builder_over<'a>(insts: &[Vec<u32>], bound: &'a mut u32) -> Builder<'a> {
    let scanned = scan(insts);
    Builder {
        bound,
        tys: scanned.tys,
        defined_at: scanned.defined_at,
        private_pointers: HashMap::new(),
        before: BTreeMap::new(),
    }
}

#[test]
fn a_type_declared_twice_is_reused_by_its_first_id_whatever_order_the_maps_iterate_in() {
    let insts = duplicated_types();
    for _ in 0..200 {
        let mut bound = 20;
        let mut builder = builder_over(&insts, &mut bound);
        assert_eq!(builder.private_pointer(1, 5), 2);
        assert_eq!(builder.vec4_of(1, 5), 4);
        assert!(builder.before.is_empty(), "nothing new was declared");
    }
}
