//! Do the primitive bounds a fragment carries actually take part in collision?
//!
//! `phBoundCylinder` and `phBoundDisc` declare their dimensions in fields that
//! **every cylinder and every disc the game ships leaves at zero** — the
//! numbers are in the bound header instead, and the owned declaration keeps
//! those fields private. Tessellating from them gave
//! a cylinder no triangles, so a particle passed straight through it (six of
//! six), and a disc no thickness, so its rim was never emitted (270 of 270).
//! Neither failed loudly, so this suite asserts the *collider*: a test that
//! `cylinder_radius()` is non-zero passes against an accessor that returns a
//! constant.
//!
//! Both assertions compare a real bound against **the same bound with every
//! source of its dimensions zeroed** — the declared field, `phBound::Margin`,
//! and the bounding box — which reproduces the defect without knowing how many
//! slices the tessellator uses. With the bare-field accessors put back, the
//! cylinder test fails with *"tessellated to no triangles, so a particle passes
//! through it"* and the disc test with *"48 triangles and a thickness-free one
//! to 48, so the rim is missing"*.
//!
//! ⚠ Discs and cylinders occur **only inside the fragment physics tree** —
//! every bound a `.ybn` root reaches is a BVH. A suite that walked `.ybn` would
//! find none and pass. The reached counts are asserted rather than reported.

use or_io::gta5::{FragDrawable, GtaFragType, PhBound, PhBoundAny, PhBoundCylinder, PhBoundDisc};
use or_simulation::collision::BvhCollider;
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

fn collect(bound: &PhBoundAny, discs: &mut Vec<PhBoundDisc>, cyls: &mut Vec<PhBoundCylinder>) {
    match bound {
        PhBoundAny::PhBoundComposite(c) => {
            for child in c.bounds.iter().flatten() {
                collect(child, discs, cyls);
            }
        }
        PhBoundAny::PhBoundDisc(d) => discs.push(d.clone()),
        PhBoundAny::PhBoundCylinder(c) => cyls.push(c.clone()),
        _ => {}
    }
}

fn collect_drawable(
    drawable: &FragDrawable<or_io::decl::Gen8>,
    discs: &mut Vec<PhBoundDisc>,
    cyls: &mut Vec<PhBoundCylinder>,
) {
    for b in std::iter::once(&drawable.bound)
        .chain(&drawable.extra_bounds)
        .flatten()
    {
        collect(b, discs, cyls);
    }
}

fn primitives_of(
    frag: &GtaFragType,
    discs: &mut Vec<PhBoundDisc>,
    cyls: &mut Vec<PhBoundCylinder>,
) {
    for drawable in frag.common_drawable.iter().chain(&frag.extra_drawables) {
        collect_drawable(drawable, discs, cyls);
    }
    let Some(group) = frag.physics_lod_group.as_ref() else {
        return;
    };
    for lod in group.lods() {
        for child in &lod.children {
            for drawable in [&child.undamaged_entity, &child.damaged_entity]
                .into_iter()
                .flatten()
            {
                collect_drawable(drawable, discs, cyls);
            }
        }
        if let Some(b) = lod.composite_bounds.as_ref() {
            collect(b, discs, cyls);
        }
        for archetype in [&lod.phys_damp_undamaged, &lod.phys_damp_damaged]
            .into_iter()
            .flatten()
        {
            if let Some(b) = archetype.bound.as_ref() {
                collect(b, discs, cyls);
            }
        }
    }
}

fn corpus() -> (Vec<PhBoundDisc>, Vec<PhBoundCylinder>, usize) {
    let files = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "yft")
        .target(TARGET_GEN8)
        .find();
    let mut discs = Vec::new();
    let mut cyls = Vec::new();
    for path in &files {
        let Ok(frag) = GtaFragType::open(path) else {
            continue;
        };
        primitives_of(&frag, &mut discs, &mut cyls);
    }
    (discs, cyls, files.len())
}

fn margin(header: &PhBound) -> f32 {
    header.bounding_box_max_xyz_margin_w.w
}

/// `header` with its margin gone and its box as `min`..`max`.
fn reshaped(header: &mut PhBound, min: [f32; 3], max: [f32; 3]) {
    header.set_bounding_box_min(min);
    let slot = &mut header.bounding_box_max_xyz_margin_w;
    (slot.x, slot.y, slot.z, slot.w) = (max[0], max[1], max[2], 0.0);
}

fn flattened_cylinder(source: &PhBoundCylinder) -> PhBoundCylinder {
    let mut out = source.clone();
    reshaped(&mut out.base, [0.0; 3], [0.0; 3]);
    out
}

fn flattened_disc(source: &PhBoundDisc) -> PhBoundDisc {
    let mut out = source.clone();
    let [_, min_y, min_z] = out.base.bounding_box_min();
    let max = &out.base.bounding_box_max_xyz_margin_w;
    let (max_y, max_z) = (max.y, max.z);
    reshaped(&mut out.base, [0.0, min_y, min_z], [0.0, max_y, max_z]);
    out
}

#[test]
fn a_shipped_cylinder_bound_produces_collision_triangles() {
    let (_, cylinders, files) = corpus();
    if files == 0 {
        or_test_support::skip_corpus(
            "a_shipped_cylinder_bound_produces_collision_triangles",
            "no .yft corpus",
        );
        return;
    }
    assert!(
        !cylinders.is_empty(),
        "no cylinder bound was reached across {files} fragment(s), so this \
         proved nothing. They live in the physics tree — check the walk before \
         believing a pass"
    );

    let mut worst = usize::MAX;
    for cylinder in &cylinders {
        let live = BvhCollider::from_phbound(&PhBoundAny::PhBoundCylinder(cylinder.clone()))
            .triangle_count();
        let flat =
            BvhCollider::from_phbound(&PhBoundAny::PhBoundCylinder(flattened_cylinder(cylinder)))
                .triangle_count();
        worst = worst.min(live);

        assert!(
            live > 0,
            "a shipped cylinder tessellated to no triangles, so a particle \
             passes through it. Its bounding box is {:?}..{:?} — if it is not \
             empty, the collider has stopped reading the radius off it",
            cylinder.base.bounding_box_min(),
            cylinder.base.bounding_box_max_xyz_margin_w,
        );
        assert_eq!(
            flat, 0,
            "a cylinder with no declared radius and no bounding box still \
             tessellated to {flat} triangles, so this comparison no longer \
             reproduces the defect and the assertion above proves less than it \
             appears to"
        );
    }

    eprintln!(
        "  {} cylinder(s) across {files} fragment(s); the thinnest collider has \
         {worst} triangle(s), and each collapses to 0 with its dimensions \
         stripped",
        cylinders.len()
    );
}

#[test]
fn a_shipped_disc_bound_collides_with_thickness() {
    let (discs, _, files) = corpus();
    if files == 0 {
        or_test_support::skip_corpus(
            "a_shipped_disc_bound_collides_with_thickness",
            "no .yft corpus",
        );
        return;
    }
    assert!(
        !discs.is_empty(),
        "no disc bound was reached across {files} fragment(s), so this proved \
         nothing"
    );

    let mut thinnest = f32::MAX;
    for disc in &discs {
        let live =
            BvhCollider::from_phbound(&PhBoundAny::PhBoundDisc(disc.clone())).triangle_count();
        let flat = BvhCollider::from_phbound(&PhBoundAny::PhBoundDisc(flattened_disc(disc)))
            .triangle_count();
        let [min_x, _, _] = disc.base.bounding_box_min();
        let half_box = (disc.base.bounding_box_max_xyz_margin_w.x - min_x) * 0.5;
        thinnest = thinnest.min(if margin(&disc.base) > 0.0 {
            margin(&disc.base)
        } else {
            half_box
        });

        assert!(
            flat > 0,
            "the flattened disc emitted nothing, so it is not the comparison \
             this test wants — it should keep its radius and lose only its \
             thickness"
        );
        assert!(
            live > flat,
            "a shipped disc tessellated to {live} triangles and a thickness-free \
             one to {flat}, so the rim is missing and the collider is a flat \
             sheet. phBound::Margin is {} and half its box along X {half_box} — \
             if either is not zero, the collider has stopped falling back to them",
            margin(&disc.base),
        );
    }

    eprintln!(
        "  {} disc(s) across {files} fragment(s); the thinnest has a \
         half-thickness of {thinnest}, and every one loses triangles when it is \
         taken away",
        discs.len()
    );
}
