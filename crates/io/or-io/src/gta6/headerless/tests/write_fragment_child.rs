use super::*;

/// Where a pointer at `at` of `img` aims.
fn aimed(img: &Image, at: usize) -> usize {
    u64_at(&img.bytes, at) as usize & 0x0FFF_FFFF
}

fn composite_of(img: &Image, root: usize) -> usize {
    aimed(img, aimed(img, root + 0x10) + 0x20)
}

fn bones_of(bytes: &[u8]) -> Vec<Bone> {
    read_skeleton(bytes, true)
        .expect("the skeleton reads")
        .expect("a skeleton")
        .bones
}

fn near(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
}

/// Child 2's place, half a metre above the root bone, which [`two_groups`]'s second group is on.
const ABOVE_ROOT: [f32; 3] = [0.0, 0.0, 1.5];

/// [`fragment`] with no vehicle glass and a second group on the root bone, holding a third child the
/// composite lists: a sphere half a metre up, given its own BVH leaf beside child 0's.
fn two_groups() -> (Image, Vec<u8>, usize, usize) {
    let (mut img, phys, parts) = fragment();
    img.u64(parts.root + 0xF0, 0);
    let composite = composite_of(&img, parts.root);
    let geometry = u64_at(&img.bytes, aimed(&img, composite + 0x70));
    let sphere = sphere_into(&mut img, [0.0; 3]);
    let pointers = img.alloc(16);
    img.u64(pointers, geometry);
    img.ptr(pointers + 8, sphere);
    let matrices = img.alloc(128);
    let first = img.bytes[parts.matrices..parts.matrices + 64].to_vec();
    img.put(matrices, &first);
    for r in 0..3 {
        img.put(matrices + 64 + 20 * r, &1.0f32.to_le_bytes());
    }
    img.vec3(matrices + 64 + 48, ABOVE_ROOT);
    let flags = img.alloc(32);
    img.u64(flags, 0x13);
    img.u64(flags + 16, 0x21);
    let nodes = img.alloc(48);
    img.u16(nodes + 0xC, 3);
    img.u16(nodes + 16 + 0xE, 1);
    img.u16(nodes + 32 + 0xC, 1);
    img.u16(nodes + 32 + 0xE, 1);
    let bvh = img.alloc(0x70);
    img.ptr(bvh + 0x50, nodes);
    img.u32(bvh + 0x58, 3);
    img.ptr(composite + 0x70, pointers);
    img.ptr(composite + 0x78, matrices);
    img.ptr(composite + 0x80, matrices);
    img.ptr(composite + 0x88, flags);
    img.ptr(composite + 0xA0, bvh);
    img.u16(composite + 0x98, 2);
    img.u16(composite + 0x9A, 2);

    let children = img.alloc(0x50 * 3);
    let held = img.bytes[parts.children..parts.children + 0xA0].to_vec();
    img.put(children, &held);
    let c = children + 0xA0;
    img.put(c + 12, &1.0f32.to_le_bytes());
    img.vec3(c + 0x10, [0.0, 0.0, 0.5]);
    img.put(c + 0x2C, &1.0f32.to_le_bytes());
    img.ptr(c + 0x30, sphere);
    img.u64(c + 0x38, 0x21);
    img.u16(c + 0x48, 1);
    img.u16(c + 0x4A, 1);
    img.ptr(parts.root + 0x28, children);
    let archetype = aimed(&img, parts.root + 0x10);
    let mass = MASSES[0] + 1.0;
    img.put(archetype + 0x4C, &mass.to_le_bytes());
    img.put(archetype + 0x5C, &(1.0 / mass).to_le_bytes());

    let groups = img.alloc(0x70 * 2);
    let held = img.bytes[parts.groups..parts.groups + 0x70].to_vec();
    img.put(groups, &held);
    let g = groups + 0x70;
    img.u32(g + 0x08, name_hash32(BONES[0].name.as_bytes()));
    img.u16(g + 0x10, 0);
    for (o, x) in [
        (0x14, 0xFFFF),
        (0x16, 0xFFFF),
        (0x18, 0),
        (0x1A, 2),
        (0x1C, 3),
        (0x1E, 3),
        (0x20, 3),
    ] {
        img.u16(g + o, x);
    }
    img.u32(g + 0x24, u32::MAX);
    img.ptr(parts.root + 0x20, groups);
    for (k, x) in [2u16, 2, 2, 3].iter().enumerate() {
        img.u16(parts.root + 0x30 + 2 * k, *x);
    }
    (img, phys, composite, nodes)
}

#[test]
fn a_moved_child_s_composite_matrix_is_its_bone_times_its_transform_and_the_box_follows() {
    let (mut img, phys, parts) = fragment();
    let composite = composite_of(&img, parts.root);
    let centroid = [0.3, 0.8, 2.0];
    img.vec3(composite + 0x40, centroid);
    let mut bytes = img.bytes.clone();
    let unchanged = fragment_of(&img, &phys).expect("the fragment reads");
    write_fragment_level(&mut bytes, &phys, &unchanged).expect("nothing edited writes");
    assert_eq!(bytes, img.bytes, "an unchanged body saves byte-identical");

    let bones = bones_of(&img.bytes);
    let mut edited = fragment_of(&img, &phys).expect("the fragment reads");
    // A quarter metre along the child's own z: the head's turn about z leaves it along z.
    let to = [STEP[0], STEP[1], STEP[2] + 0.25];
    place_fragment_child(&mut edited, &bones, 0, TURN_X, to).expect("a unit turn and a step");
    let mut placed = PLACED;
    placed[3][2] += 0.25;
    let bound = edited.bound.as_ref().expect("a composite");
    for (row, want) in bound.children[0].matrix.iter().zip(placed) {
        assert!(near(row, &want), "{:?}", bound.children[0].matrix);
    }
    // The child's flat box, from (0, 0, 0) to (1, 1, 0), placed: x along y, y along z.
    let (lo, hi) = ([0.3, 0.35, 1.55], [0.3, 1.35, 2.55]);
    assert!(
        near(&bound.box_min, &lo) && near(&bound.box_max, &hi),
        "{bound:?}"
    );
    // The corner furthest from the centroid, (0.3, 1.35, 2.55).
    let far = (0.55f32 * 0.55 + 0.55 * 0.55).sqrt();
    assert!(
        (bound.radius - far).abs() < 1e-5,
        "{} for {far}",
        bound.radius
    );

    write_fragment_level(&mut bytes, &phys, &edited).expect("the move writes in place");
    assert_eq!(bytes.len(), img.bytes.len());
    let back = read_drawable_with_bound(&bytes, Some(phys.as_slice())).expect("it reads back");
    assert!(near(&back.fragment.children[0].translation, &to));
    let b = back.bound.as_ref().expect("a composite");
    for (row, want) in b.children[0].matrix.iter().zip(placed) {
        assert!(near(&row[..3], &want[..3]), "{:?}", b.children[0].matrix);
    }
    assert_eq!(
        (b.box_min, b.box_max, b.radius),
        (bound.box_min, bound.box_max, bound.radius)
    );

    let mut turned = fragment_of(&img, &phys).expect("the fragment reads");
    let half = [0.0, 0.0, 0.0, 0.5];
    assert!(
        place_fragment_child(&mut turned, &bones, 0, half, to).is_err(),
        "not a unit turn"
    );
    assert!(
        place_fragment_child(&mut turned, &bones, 2, TURN_X, to).is_err(),
        "no third child"
    );
}

#[test]
fn a_child_given_another_group_keeps_its_place_and_the_composite_follows_its_new_order() {
    let (img, phys, composite, nodes) = two_groups();
    let read = fragment_of(&img, &phys).expect("the built fragment reads");
    let words: Vec<u64> = read
        .fragment
        .children
        .iter()
        .map(|c| c.bound_word)
        .collect();
    let bones = bones_of(&img.bytes);
    let mut edited = fragment_of(&img, &phys).expect("the built fragment reads");
    let (q, t) = fragment_child_local_in(&edited.fragment, &bones, 0, 1).expect("both bones turn");
    // The root bone is a step up and not turned: the turn is where the child lies, the head's turn
    // about z after its own about x, and the translation where it lies less the step.
    assert!(near(&q, &[0.5; 4]) || near(&q, &[-0.5; 4]), "{q:?}");
    assert!(
        near(&t, &[PLACED[3][0], PLACED[3][1], PLACED[3][2] - 1.0]),
        "{t:?}"
    );
    assert_eq!(fragment_child_place(&edited.fragment, 0), Some(0));
    let now = regroup_fragment_child(&mut edited, 0, 1, 1, q, t).expect("a group on a bone");
    assert_eq!(now, 2, "after the group's own listed child");
    let f = &edited.fragment;
    assert_eq!(
        f.groups
            .iter()
            .map(|g| (g.children.clone(), g.second_set.clone()))
            .collect::<Vec<_>>(),
        [(0..0, 0..1), (1..3, 3..3)]
    );
    assert_eq!(
        f.children
            .iter()
            .map(|c| (c.bound_word, c.group, c.composite))
            .collect::<Vec<_>>(),
        [
            (words[1], 0, None),
            (words[2], 1, Some(0)),
            (words[0], 1, Some(1))
        ]
    );

    let mut bytes = img.bytes.clone();
    write_fragment_level(&mut bytes, &phys, &edited).expect("the group change writes in place");
    assert_eq!(bytes.len(), img.bytes.len());
    let back = read_drawable_with_bound(&bytes, Some(phys.as_slice())).expect("it reads back");
    assert_eq!(back.fragment.groups, edited.fragment.groups);
    let order: Vec<(u64, u16, Option<u16>)> = (back.fragment.children.iter())
        .map(|c| (c.bound_word, c.group, c.composite))
        .collect();
    assert_eq!(
        order,
        [
            (words[1], 0, None),
            (words[2], 1, Some(0)),
            (words[0], 1, Some(1))
        ]
    );
    let moved = &back.fragment.children[2];
    assert!(near(&moved.rotation, &q) && near(&moved.translation, &t));
    // It lies where it lay: the composite's matrix is the one it had, now second.
    let b = back.bound.as_ref().expect("a composite");
    for (row, want) in b.children[1].matrix.iter().zip(PLACED) {
        assert!(near(&row[..3], &want[..3]), "{:?}", b.children[1].matrix);
    }
    assert!(near(&b.children[0].matrix[3][..3], &ABOVE_ROOT));
    assert_eq!(
        b.children.iter().map(|c| c.flags).collect::<Vec<_>>(),
        [Some([0x21, 0]), Some([0x13, 0])]
    );
    let pointers = u64_at(&bytes, composite + 0x70) as usize & 0x0FFF_FFFF;
    assert_eq!(
        [u64_at(&bytes, pointers), u64_at(&bytes, pointers + 8)],
        [words[2], words[0]]
    );
    let leaf =
        |i: usize| u16::from_le_bytes([bytes[nodes + 16 * i + 0xC], bytes[nodes + 16 * i + 0xD]]);
    assert_eq!(
        [leaf(1), leaf(2)],
        [1, 0],
        "each leaf names its child's new place"
    );

    let (mut one, _, parts) = fragment();
    one.u64(parts.root + 0xF0, 0);
    let mut level = fragment_of(&one, &phys).expect("the fragment reads");
    assert!(
        regroup_fragment_child(&mut level, 0, 0, 0, TURN_X, STEP).is_ok(),
        "a place in its own group is no group change"
    );
}

#[test]
fn a_child_moved_then_given_another_group_saves_with_its_composite_matrix_moved() {
    let (mut img, phys, composite, _) = two_groups();
    img.vec3(composite + 0x40, [0.3, 0.8, 2.0]);
    let bones = bones_of(&img.bytes);
    let mut edited = fragment_of(&img, &phys).expect("the built fragment reads");
    let to = [STEP[0], STEP[1], STEP[2] + 0.25];
    place_fragment_child(&mut edited, &bones, 0, TURN_X, to).expect("a unit turn and a step");
    let (q, t) = fragment_child_local_in(&edited.fragment, &bones, 0, 1).expect("both bones turn");
    let now = regroup_fragment_child(&mut edited, 0, 1, 1, q, t).expect("a group on a bone");
    let mut placed = PLACED;
    placed[3][2] += 0.25;

    let mut bytes = img.bytes.clone();
    write_fragment_level(&mut bytes, &phys, &edited).expect("the move and the group change write");
    let back = read_drawable_with_bound(&bytes, Some(phys.as_slice())).expect("it reads back");
    let moved = &back.fragment.children[now];
    assert!(near(&moved.rotation, &q) && near(&moved.translation, &t));
    let b = back.bound.as_ref().expect("a composite");
    let rank = usize::from(moved.composite.expect("still listed"));
    for (row, want) in b.children[rank].matrix.iter().zip(placed) {
        assert!(near(&row[..3], &want[..3]), "{:?}", b.children[rank].matrix);
    }
    let e = edited.bound.as_ref().expect("a composite");
    assert_eq!(
        (b.box_min, b.box_max, b.radius),
        (e.box_min, e.box_max, e.radius)
    );
}

#[test]
fn a_group_change_is_refused_where_vehicle_glass_names_the_children_by_index() {
    let (mut img, phys, ..) = two_groups();
    let glass = img.alloc(0x10);
    img.put(glass, b"HWGV\x70\x00");
    let root = 0;
    img.ptr(root + 0xF0, glass);
    let mut level = fragment_of(&img, &phys).expect("the fragment reads");
    assert!(regroup_fragment_child(&mut level, 0, 1, 0, TURN_X, STEP).is_err());
}

#[test]
fn a_type_and_include_entry_writes_with_the_child_s_copy_and_alone_is_refused() {
    let (img, phys, _) = fragment();
    let mut bytes = img.bytes.clone();
    let mut edited = fragment_of(&img, &phys).expect("the fragment reads");
    let flags = [0x55, 0x66];
    edited.fragment.children[0].flags = flags;
    if let Some(b) = edited.bound.as_mut() {
        b.children[0].flags = Some(flags);
    }
    write_fragment_level(&mut bytes, &phys, &edited).expect("the entry and its copy write");
    let back = read_drawable_with_bound(&bytes, Some(phys.as_slice())).expect("it reads back");
    assert_eq!(back.fragment.children[0].flags, flags);
    assert_eq!(back.bound.map(|b| b.children[0].flags), Some(Some(flags)));

    // The composite's entry without the child's copy is what no measured body holds.
    let mut alone = fragment_of(&img, &phys).expect("the fragment reads");
    if let Some(b) = alone.bound.as_mut() {
        b.children[0].flags = Some(flags);
    }
    assert!(write_fragment_level(&mut img.bytes.clone(), &phys, &alone).is_err());
}
