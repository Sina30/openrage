use super::*;
use crate::dxbc_testing::{rdef, textured_pixel_shader, vertex_shader_with_a_buffer};

/// The textured shader's own reflection: `b3`, `t5` and `s2`.
fn its_reflection() -> Vec<u8> {
    rdef(&[
        ("Globals", 0, 3),
        ("Diffuse", 2, 5),
        ("DiffuseSampler", 3, 2),
    ])
}

fn reflect(rdef: &[u8]) -> DxbcReflection {
    DxbcReflection::parse(rdef).expect("the hand-built RDEF parses")
}

fn translated(container: &[u8]) -> Vec<u8> {
    or_shader_front::dxbc_to_spirv(container).expect("the translator takes the shader")
}

#[test]
fn each_kind_of_binding_is_the_class_of_descriptor_it_becomes() {
    assert_eq!(class_of(DxbcBindKind::CBuffer), Some(ResourceClass::Cbv));
    assert_eq!(
        class_of(DxbcBindKind::Sampler),
        Some(ResourceClass::Sampler)
    );
    assert_eq!(class_of(DxbcBindKind::Texture), Some(ResourceClass::Srv));
    assert_eq!(class_of(DxbcBindKind::Structured), Some(ResourceClass::Srv));
    assert_eq!(class_of(DxbcBindKind::UavTyped), Some(ResourceClass::Uav));
    assert_eq!(class_of(DxbcBindKind::UavAppend), Some(ResourceClass::Uav));
    assert_eq!(class_of(DxbcBindKind::Other(99)), None);
}

#[test]
fn a_reflected_register_is_expected_where_the_layout_places_it() {
    let reflection = reflect(&its_reflection());
    let layout = BindingLayout::OPENRAGE;
    // b3 at 3, s2 at 16 + 2, t5 at 32 + 5
    assert_eq!(
        expected(&reflection, &layout),
        BTreeSet::from([(0, 3), (0, 18), (0, 37)])
    );
}

#[test]
fn an_append_buffer_is_expected_to_bind_a_counter_too() {
    let reflection = reflect(&rdef(&[
        ("Visible", 9, 1),
        ("Plain", 4, 2),
        ("Consumed", 10, 3),
        ("Counted", 11, 4),
    ]));
    // Each u# at 160 + register, with its counter at 224 + register for the append, consume and counted kinds and not the plain one
    assert_eq!(
        expected(&reflection, &BindingLayout::OPENRAGE),
        BTreeSet::from([
            (0, 161),
            (0, 162),
            (0, 163),
            (0, 164),
            (0, 225),
            (0, 227),
            (0, 228)
        ])
    );
}

#[test]
fn a_register_the_layout_has_no_room_for_is_not_expected() {
    let reflection = reflect(&rdef(&[("Far", 0, 16)]));
    assert!(expected(&reflection, &BindingLayout::OPENRAGE).is_empty());
}

#[test]
fn the_descriptors_a_translated_module_decorates_are_read_back() {
    // The pixel stage's registers are in set 1
    let spirv = translated(&textured_pixel_shader(3, 5, 2, None));
    assert_eq!(
        decorated(&spirv),
        BTreeSet::from([(1, 3), (1, 18), (1, 37)])
    );
}

#[test]
fn a_vertex_shader_agrees_with_its_reflection_in_the_vertex_stages_set() {
    let reflection = rdef(&[("Globals", 0, 3)]);
    let container = vertex_shader_with_a_buffer(3, Some(reflection));
    let spirv = translated(&container);
    assert_eq!(decorated(&spirv), BTreeSet::from([(0, 3)]));
    assert_eq!(
        agreement(&container, &spirv, &BindingLayout::OPENRAGE),
        Agreement::Same
    );
}

#[test]
fn a_module_whose_stage_the_layout_put_in_the_wrong_set_does_not_agree() {
    // A pixel shader translated for the vertex stage's set binds set 0, and its reflection is judged in set 1
    let container = textured_pixel_shader(3, 5, 2, Some(its_reflection()));
    let in_the_vertex_set =
        or_shader_front::dxbc_to_spirv_with(&container, &BindingLayout::OPENRAGE).unwrap();
    assert_eq!(
        agreement(&container, &in_the_vertex_set, &BindingLayout::OPENRAGE),
        Agreement::Differs
    );
}

#[test]
fn a_module_that_binds_what_its_reflection_lists_agrees_with_it() {
    let container = textured_pixel_shader(3, 5, 2, Some(its_reflection()));
    let spirv = translated(&container);
    assert_eq!(
        agreement(&container, &spirv, &BindingLayout::OPENRAGE),
        Agreement::Same
    );
}

#[test]
fn a_reflection_that_lists_a_resource_the_module_does_not_bind_is_told_apart_from_one_that_lists_less(
) {
    let layout = BindingLayout::OPENRAGE;
    let spirv = translated(&textured_pixel_shader(3, 5, 2, None));
    let with = |extra: &[(&str, u32, u32)]| {
        let mut all = vec![
            ("Globals", 0, 3),
            ("Diffuse", 2, 5),
            ("DiffuseSampler", 3, 2),
        ];
        all.extend_from_slice(extra);
        textured_pixel_shader(3, 5, 2, Some(rdef(&all)))
    };
    // An unread constant buffer at b9 that the translator leaves out
    assert_eq!(
        agreement(&with(&[("Unread", 0, 9)]), &spirv, &layout),
        Agreement::ModuleBindsLess
    );
    // Nothing missing from the reflection but the sampler
    let no_sampler =
        textured_pixel_shader(3, 5, 2, Some(rdef(&[("Globals", 0, 3), ("Diffuse", 2, 5)])));
    assert_eq!(
        agreement(&no_sampler, &spirv, &layout),
        Agreement::ModuleBindsMore
    );
    // A texture that sits in t6 for the reflection and t5 for the module
    let elsewhere = textured_pixel_shader(
        3,
        5,
        2,
        Some(rdef(&[
            ("Globals", 0, 3),
            ("Diffuse", 2, 6),
            ("DiffuseSampler", 3, 2),
        ])),
    );
    assert_eq!(agreement(&elsewhere, &spirv, &layout), Agreement::Differs);
}

#[test]
fn a_container_with_no_reflection_or_with_a_broken_one_is_counted_as_such() {
    let layout = BindingLayout::OPENRAGE;
    let bare = textured_pixel_shader(3, 5, 2, None);
    let spirv = translated(&bare);
    assert_eq!(agreement(&bare, &spirv, &layout), Agreement::NoReflection);
    let broken = textured_pixel_shader(3, 5, 2, Some(vec![0xff; 8]));
    assert_eq!(agreement(&broken, &spirv, &layout), Agreement::Unreadable);
}

#[test]
fn the_tally_counts_each_agreement_in_its_own_slot_and_merges() {
    let mut tally = Tally::default();
    for agreement in [
        Agreement::Same,
        Agreement::Same,
        Agreement::ModuleBindsLess,
        Agreement::ModuleBindsMore,
        Agreement::Differs,
        Agreement::NoReflection,
        Agreement::Unreadable,
    ] {
        tally.add(agreement);
    }
    let mut other = Tally::default();
    other.add(Agreement::Differs);
    tally.merge(other);
    assert_eq!(
        (
            tally.same,
            tally.module_binds_less,
            tally.module_binds_more,
            tally.differs,
            tally.no_reflection,
            tally.unreadable,
            tally.total()
        ),
        (2, 1, 1, 2, 1, 1, 8)
    );
    assert!(tally.report().contains("of 8 translated: 2 the same"));
    assert_eq!(tally.to_json()["differs"], 2);
}
