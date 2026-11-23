use super::*;
use crate::dxbc_testing::{container, dwords};

/// A signature part of `(name, semantic index, system value, register, mask)`, each element `skip` bytes longer than 24 with a stream of 1 in front.
fn part(elements: &[(&str, u32, u32, u32, u8)], skip: usize) -> Vec<u8> {
    let stride = 24 + skip;
    let names_at = 8 + elements.len() * stride;
    let mut table = Vec::new();
    let mut names = Vec::new();
    for (name, index, system_value, register, mask) in elements {
        if skip == 4 {
            table.extend(dwords(&[1]));
        }
        table.extend(dwords(&[
            (names_at + names.len()) as u32,
            *index,
            *system_value,
            3,
            *register,
        ]));
        table.extend_from_slice(&[*mask, 0, 0, 0]);
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }
    let mut out = dwords(&[elements.len() as u32, 8]);
    out.extend(table);
    out.extend(names);
    out
}

fn read(parts: &[([u8; 4], Vec<u8>)]) -> (Vec<SignatureElement>, Vec<SignatureElement>) {
    let bytes = container(parts);
    signatures(&DxbcBlob {
        offset: 0,
        size: bytes.len(),
        bytes: &bytes,
    })
}

#[test]
fn the_input_and_output_signatures_are_read_with_their_registers_and_masks() {
    let (inputs, outputs) = read(&[
        (
            *b"ISGN",
            part(&[("POSITION", 0, 0, 0, 7), ("TEXCOORD", 3, 0, 2, 3)], 0),
        ),
        (*b"OSGN", part(&[("SV_Position", 0, 1, 0, 15)], 0)),
    ]);
    assert_eq!(inputs.len(), 2);
    assert_eq!(
        (
            inputs[0].semantic.as_str(),
            inputs[0].register,
            inputs[0].mask
        ),
        ("POSITION", 0, 7)
    );
    assert_eq!(
        (
            inputs[1].semantic.as_str(),
            inputs[1].index,
            inputs[1].register,
            inputs[1].mask
        ),
        ("TEXCOORD", 3, 2, 3)
    );
    assert_eq!(outputs.len(), 1);
    assert_eq!(
        (outputs[0].semantic.as_str(), outputs[0].system_value),
        ("SV_Position", 1)
    );
}

#[test]
fn a_geometry_shaders_outputs_lead_each_element_with_its_stream() {
    let (_, outputs) = read(&[(
        *b"OSG5",
        part(&[("TEXCOORD", 1, 0, 4, 15), ("COLOR", 0, 0, 5, 15)], 4),
    )]);
    let names: Vec<_> = outputs
        .iter()
        .map(|e| (e.semantic.as_str(), e.register))
        .collect();
    assert_eq!(names, vec![("TEXCOORD", 4), ("COLOR", 5)]);
}

#[test]
fn a_container_with_no_signature_or_a_cut_one_has_empty_signatures() {
    assert_eq!(read(&[]), (Vec::new(), Vec::new()));
    let mut cut = part(&[("POSITION", 0, 0, 0, 7)], 0);
    cut.truncate(cut.len() - 14);
    assert!(read(&[(*b"ISGN", cut)]).0.is_empty());
}

fn element(
    semantic: &str,
    index: u32,
    system_value: u32,
    register: u32,
    mask: u8,
) -> SignatureElement {
    SignatureElement {
        semantic: semantic.into(),
        index,
        system_value,
        component_type: 3,
        register,
        mask,
    }
}

#[test]
fn an_input_is_linked_to_the_output_of_its_semantic_and_the_worst_failure_is_the_linkage() {
    let outputs = [
        element("SV_Position", 0, 1, 0, 15),
        element("TEXCOORD", 0, 0, 1, 15),
        element("TEXCOORD", 1, 0, 2, 3),
    ];
    let one = |input: SignatureElement| link(&outputs, &[input]);
    assert_eq!(one(element("TEXCOORD", 0, 0, 1, 15)), Linkage::Exact);
    // Semantics compare without regard to case
    assert_eq!(one(element("texcoord", 0, 0, 1, 15)), Linkage::Exact);
    assert_eq!(one(element("TEXCOORD", 0, 0, 1, 7)), Linkage::Narrower);
    assert_eq!(one(element("TEXCOORD", 1, 0, 2, 7)), Linkage::Wider);
    assert_eq!(one(element("TEXCOORD", 0, 0, 4, 15)), Linkage::Moved);
    assert_eq!(one(element("COLOR", 0, 0, 3, 15)), Linkage::Unmatched);
    // A system value is not linked, whatever it is called
    assert_eq!(one(element("SV_SampleIndex", 0, 12, 7, 1)), Linkage::Exact);
    // Several inputs take the worst
    assert_eq!(
        link(
            &outputs,
            &[
                element("TEXCOORD", 0, 0, 1, 7),
                element("TEXCOORD", 1, 0, 9, 3),
                element("TEXCOORD", 0, 0, 1, 15),
            ]
        ),
        Linkage::Moved
    );
    assert_eq!(link(&outputs, &[]), Linkage::Exact);
}
