use super::*;
use crate::game_shaders::test_pack::element;
use wgpu::{VertexAttribute as Attribute, VertexFormat as Format};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    First,
    Second,
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn layout() -> Layout<Field> {
    let attributes = [
        Attribute {
            format: Format::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        Attribute {
            format: Format::Float32x2,
            offset: 12,
            shader_location: 1,
        },
        Attribute {
            format: Format::Float32,
            offset: 20,
            shader_location: 2,
        },
    ];
    let inputs = [
        element("POSITION", 0, 0, 7),
        element("TEXCOORD", 0, 1, 3),
        element("TEXCOORD", 5, 2, 1),
    ];
    Layout::new(&attributes, 24, &inputs, |semantic, index| {
        match (semantic, index) {
            ("POSITION", 0) => Some(Field::First),
            ("TEXCOORD", 0) => Some(Field::Second),
            _ => None,
        }
    })
}

#[test]
fn a_record_has_each_input_where_the_layout_put_it_cut_to_its_words_and_the_rest_at_nought() {
    let mut bytes = Vec::new();
    layout().write(&mut bytes, 2, |record, field| match field {
        Field::First => [1.0 + record as f32, 2.0, 3.0, 4.0],
        Field::Second => [5.0, 6.0, 7.0, 8.0],
    });
    assert_eq!(bytes.len(), 48);
    let numbers = floats(&bytes);
    // A position of three words with its w left out, a pair of coordinates, and an input no field reads
    assert_eq!(&numbers[..6], &[1.0, 2.0, 3.0, 5.0, 6.0, 0.0]);
    assert_eq!(&numbers[6..], &[2.0, 2.0, 3.0, 5.0, 6.0, 0.0]);
}

#[test]
fn records_are_appended_after_what_the_bytes_already_hold() {
    let layout = layout();
    let mut bytes = Vec::new();
    layout.write(&mut bytes, 1, |_, _| [1.0; 4]);
    layout.write(&mut bytes, 1, |_, _| [2.0; 4]);
    let numbers = floats(&bytes);
    assert_eq!(numbers.len(), 12);
    assert_eq!(numbers[0], 1.0);
    assert_eq!(numbers[6], 2.0);
}

#[test]
fn an_attribute_whose_input_the_stage_does_not_list_is_left_out_of_the_layout() {
    let attributes = [Attribute {
        format: Format::Float32x4,
        offset: 0,
        shader_location: 9,
    }];
    let layout = Layout::new(&attributes, 16, &[element("POSITION", 0, 0, 15)], |_, _| {
        Some(Field::First)
    });
    let mut bytes = Vec::new();
    layout.write(&mut bytes, 1, |_, _| [1.0; 4]);
    assert_eq!(floats(&bytes), vec![0.0; 4]);
}
