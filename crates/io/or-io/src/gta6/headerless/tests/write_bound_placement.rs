use super::*;

/// The built bound with its box grown to hold its child moved, and the child moved.
fn moved(edited: &mut Bound) {
    edited.box_max = [4.0, 4.0, 2.0];
    edited.centroid = [1.0, 1.0, 1.0];
    edited.radius = 2.5;
    edited.children[0].matrix[3] = [2.0, 1.5, 1.0, 1.0];
}

#[test]
fn a_child_s_matrix_and_the_composite_s_box_write_in_place_and_read_back() {
    let (img, _) = bound();
    let mut bytes = img.bytes.clone();
    let mut edited = read_bound(&bytes).expect("the bound reads");
    moved(&mut edited);
    write_bound(&mut bytes, &edited).expect("the child moves inside the grown box");
    assert_eq!(bytes.len(), img.bytes.len());
    let read = read_bound(&bytes).expect("it reads back");
    assert_eq!(
        (read.box_min, read.box_max, read.centroid, read.radius),
        ([0.0; 3], [4.0, 4.0, 2.0], [1.0, 1.0, 1.0], 2.5)
    );
    assert_eq!(read.children[0].matrix[3], [2.0, 1.5, 1.0, 1.0]);
    // Nothing else moved: written again from what it reads, it is the same bytes.
    let again = bytes.clone();
    write_bound(&mut bytes, &read).expect("nothing edited writes");
    assert_eq!(bytes, again);
}

#[test]
fn a_child_moved_under_a_composite_bvh_fits_its_leaf_in_the_frame_of_the_grown_box() {
    let (mut img, parts) = bound();
    // A BVH over the one child: a single leaf naming child 0.
    let bvh = img.alloc(0x70);
    img.vec3(bvh + 0x10, [1.0, 1.0, 0.0]);
    img.vec3(bvh + 0x30, [65534.0; 3]);
    let nodes = img.alloc(16);
    img.u16(nodes + 0xE, 1);
    img.ptr(bvh + 0x50, nodes);
    img.u32(bvh + 0x58, 1);
    img.ptr(parts.root + 0xA0, bvh);
    let mut bytes = img.bytes.clone();
    let mut edited = read_bound(&bytes).expect("the bound reads with its BVH");
    assert!(edited.bvh.is_some());
    moved(&mut edited);
    write_bound(&mut bytes, &edited).expect("the child moves and its leaf follows");

    // The frame GTA V's SetExtents gives the composite's new box.
    let vec3 = |at: usize| -> [f32; 3] {
        std::array::from_fn(|a| f32::from_bits(u32_at(&bytes, at + 4 * a)))
    };
    let (lo, hi) = ([0.0f32; 3], [4.0f32, 4.0, 2.0]);
    let centre: [f32; 3] = std::array::from_fn(|a| (lo[a] + hi[a]) / 2.0);
    let scale: [f32; 3] = std::array::from_fn(|a| 65534.0 / (hi[a] - lo[a]));
    let quantum: [f32; 3] = std::array::from_fn(|a| (hi[a] - lo[a]) / 65534.0);
    assert_eq!(
        [vec3(bvh), vec3(bvh + 0x10), vec3(bvh + 0x20)],
        [lo, hi, centre]
    );
    assert_eq!([vec3(bvh + 0x30), vec3(bvh + 0x40)], [scale, quantum]);
    // The leaf: the child's box, 0 to 1 by 0 to 1 by 0, placed at (2, 1.5, 1) and rounded out.
    let (min, max) = ([2.0f32, 1.5, 1.0], [3.0f32, 2.5, 1.0]);
    let leaf: [i16; 6] = std::array::from_fn(|k| match k {
        0..3 => ((min[k] - centre[k]) * scale[k]).floor() as i16,
        _ => ((max[k - 3] - centre[k - 3]) * scale[k - 3]).ceil() as i16,
    });
    let word = |k: usize| i16::from_le_bytes([bytes[nodes + 2 * k], bytes[nodes + 2 * k + 1]]);
    let stored: [i16; 6] = std::array::from_fn(word);
    assert_eq!(stored, leaf);

    // Written again from what it reads, nothing changes.
    let read = read_bound(&bytes).expect("it reads back");
    let again = bytes.clone();
    write_bound(&mut bytes, &read).expect("nothing edited writes");
    assert_eq!(bytes, again);
}

#[test]
fn a_placement_every_measured_body_would_not_hold_is_refused_and_writes_nothing() {
    let (img, _) = bound();
    let refusals: [fn(&mut Bound); 6] = [
        // The child's box past the composite's.
        |b| b.children[0].matrix[3] = [3.5, 0.0, 0.0, 1.0],
        // An axis row of another length.
        |b| b.children[0].matrix[0] = [2.0, 0.0, 0.0, 0.0],
        // A row's fourth lane.
        |b| b.children[0].matrix[0][3] = 0.5,
        |b| b.centroid = [9.0, 0.0, 0.0],
        |b| b.radius = -1.0,
        |b| b.box_min = [2.0, 0.0, 0.0],
    ];
    for (k, refuse) in refusals.iter().enumerate() {
        let mut bytes = img.bytes.clone();
        let mut edited = read_bound(&bytes).expect("the bound reads");
        moved(&mut edited);
        refuse(&mut edited);
        assert!(
            matches!(
                write_bound(&mut bytes, &edited),
                Err(HeaderlessError::NotInPlace(_))
            ),
            "case {k}"
        );
        assert_eq!(bytes, img.bytes, "case {k} wrote nothing");
    }
}

#[test]
fn a_drawable_s_composite_writes_through_its_root_and_a_lone_bound_keeps_its_box() {
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let bound = bound_into(&mut img).root;
    img.ptr(parts.root + 0xC8, bound);
    let mut bytes = img.bytes.clone();
    let mut edited =
        (read_drawable(&bytes, Some(&phys)).expect("it reads").bound).expect("its composite");
    moved(&mut edited);
    write_drawable_bound(&mut bytes, &phys, &edited).expect("the composite writes");
    let read = (read_drawable(&bytes, Some(&phys))
        .expect("it reads back")
        .bound)
        .expect("its composite");
    assert_eq!(read.children[0].matrix[3], [2.0, 1.5, 1.0, 1.0]);
    assert_eq!(read.box_max, [4.0, 4.0, 2.0]);

    // A box alone at 0xC8: its material writes, its box and placement are its shape's.
    let (mut img, phys, parts) = drawable([1.0, 1.0, 0.0]);
    let bare = img.alloc(0x70);
    img.bytes[bare + 0x10] = 3;
    img.put(bare + 0x14, &0.75f32.sqrt().to_le_bytes());
    img.vec3(bare + 0x20, [0.5; 3]);
    img.vec3(bare + 0x30, [-0.5; 3]);
    img.u32(bare + 0x4C, 0x7646);
    img.ptr(parts.root + 0xC8, bare);
    let lone = |bytes: &[u8]| {
        (read_drawable(bytes, Some(&phys)).expect("it reads").bound).expect("its box")
    };
    let mut bytes = img.bytes.clone();
    let mut edited = lone(&bytes);
    edited.children[0].material_id = Some(0x7655);
    write_drawable_bound(&mut bytes, &phys, &edited).expect("the key writes");
    assert_eq!(lone(&bytes).children[0].material_id, Some(0x7655));
    let refusals: [fn(&mut Bound); 2] = [
        |b| b.box_max = [2.0; 3],
        |b| b.children[0].matrix[3] = [1.0, 0.0, 0.0, 1.0],
    ];
    for refuse in refusals {
        let mut edited = lone(&bytes);
        refuse(&mut edited);
        assert!(matches!(
            write_drawable_bound(&mut bytes.clone(), &phys, &edited),
            Err(HeaderlessError::NotInPlace(_))
        ));
    }
}
