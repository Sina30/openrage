use or_io::gta5::shaders::spirv_pack::Variable;

use super::*;

fn variable(name: &str, offset: u32, size: u32) -> Variable {
    Variable {
        name: name.into(),
        offset,
        size,
        used: true,
        scalar: "float".into(),
        rows: 1,
        columns: 4,
        elements: 0,
    }
}

/// A buffer of a matrix, a vector and a scalar, as a game's own would lay them out.
fn layout() -> ConstantBuffer {
    ConstantBuffer {
        name: "rage_matrices".into(),
        size: 96,
        variables: vec![
            variable("gWorldViewProj", 0, 64),
            variable("gTint", 64, 16),
            variable("gScale", 80, 4),
        ],
    }
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[test]
fn a_buffer_starts_at_zero_and_is_as_big_as_its_layout() {
    let layout = layout();
    let bytes = Filler::new(&layout).bytes();
    assert_eq!(bytes.len(), 96);
    assert!(bytes.iter().all(|b| *b == 0));
}

#[test]
fn a_variable_is_written_at_its_own_offset_and_nothing_else_changes() {
    let layout = layout();
    let mut filler = Filler::new(&layout);
    filler.set("gTint", &[0.25, 0.5, 0.75, 1.0]).unwrap();
    filler.set("gScale", &[3.0]).unwrap();
    let words = floats(&filler.bytes());
    assert_eq!(&words[..16], &[0.0; 16]);
    assert_eq!(&words[16..20], &[0.25, 0.5, 0.75, 1.0]);
    assert_eq!(words[20], 3.0);
    assert_eq!(words.len(), 24);
}

#[test]
fn a_matrix_goes_in_as_its_registers_one_after_the_other() {
    let layout = layout();
    let mut filler = Filler::new(&layout);
    let matrix: Vec<f32> = (0..16).map(|i| i as f32).collect();
    filler.set("gWorldViewProj", &matrix).unwrap();
    assert_eq!(&floats(&filler.bytes())[..16], &matrix[..]);
}

#[test]
fn fewer_words_than_the_variable_holds_leave_the_rest_alone() {
    let layout = layout();
    let mut filler = Filler::new(&layout);
    filler.set("gTint", &[1.0, 2.0]).unwrap();
    assert_eq!(&floats(&filler.bytes())[16..20], &[1.0, 2.0, 0.0, 0.0]);
}

#[test]
fn raw_words_are_written_as_they_are() {
    let layout = layout();
    let mut filler = Filler::new(&layout);
    filler.set_words("gScale", &[0xdead_beef]).unwrap();
    assert_eq!(&filler.bytes()[80..84], &0xdead_beef_u32.to_le_bytes());
}

#[test]
fn a_name_that_is_not_in_the_layout_and_too_many_words_are_errors() {
    let layout = layout();
    let mut filler = Filler::new(&layout);
    assert_eq!(
        filler.set("gMissing", &[1.0]),
        Err(ConstantError::NoSuchVariable("gMissing".into()))
    );
    assert_eq!(
        filler.set("gScale", &[1.0, 2.0]),
        Err(ConstantError::TooMany {
            name: "gScale".into(),
            room: 1,
            given: 2
        })
    );
    assert!(
        filler.bytes().iter().all(|b| *b == 0),
        "a refused write changes nothing"
    );
}

#[test]
fn a_planned_fill_holds_what_the_fill_by_name_holds_for_every_frame() {
    let layout = layout();
    let defaults = vec![("gScale".to_string(), vec![0.5f32.to_bits()])];
    let material = vec![
        ("gTint".to_string(), vec![0.1, 0.2, 0.3, 0.4]),
        ("gScale".to_string(), vec![0.75]),
    ];
    let frame = |scale: f32, tint: Vec<f32>| -> BTreeMap<&'static str, Vec<f32>> {
        [
            ("gScale", vec![scale]),
            ("gTint", tint),
            ("gNotHere", vec![9.0]),
            ("gWorldViewProj", vec![scale; 16]),
        ]
        .into()
    };
    let by_name = |values: &BTreeMap<&'static str, Vec<f32>>| {
        let mut filler = Filler::new(&layout);
        for (variable, words) in &defaults {
            let _ = filler.set_words(variable, words);
        }
        for (variable, numbers) in &material {
            let _ = filler.set(variable, numbers);
        }
        for (variable, numbers) in values {
            let _ = filler.set(variable, numbers);
        }
        filler.bytes()
    };
    let first = frame(2.0, vec![1.0; 4]);
    let mut plan = Plan::new(&layout, &defaults, &material, &first);
    for values in [
        first,
        frame(3.0, vec![0.0; 4]),
        // Five numbers do not fit gTint's four: the fill by name leaves it as the material set it
        frame(4.0, vec![7.0; 5]),
    ] {
        assert!(plan.matches(&values));
        plan.fill(&values);
        assert_eq!(plan.bytes, by_name(&values));
    }
    assert!(!plan.matches(&[("gScale", vec![1.0])].into()));
}
