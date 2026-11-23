//! What the inspector says of a GTA VI morph target.

use or_io::gta6::headerless::MorphTarget;

/// The lines of `m`: its deltas, the base head level of detail its key names, whether it is empty,
/// and why it is not applied to a mesh.
pub(super) fn morph_lines(m: &MorphTarget, lines: &mut Vec<String>) {
    lines.push(tr!(
        "A morph target of {elements} deltas for the mesh hash_{key}. Which axis each of its three lanes holds is not known, so no delta is given as a vector.",
        elements = m.elements.len(),
        key = format!("{:08X}", m.mesh_key),
    ));
    if let Some((mesh, lod)) = m.mesh() {
        lines.push(tr!(
            "Its key names the base head mesh {mesh}, level of detail {lod}.",
            mesh = mesh,
            lod = lod,
        ));
    }
    if m.is_empty() {
        lines.push(tr!("Every step is zero: an empty target, which moves nothing.").to_string());
    }
    lines.push(
        tr!("Which vertex each delta moves is not known, so the target is not applied to a mesh.")
            .to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_morph_target_names_its_base_head_and_says_it_is_not_applied() {
        let key = or_io::gta6::hash::name_hash32(b"head_fo1_base_000_m_full_lod");
        let mut m = MorphTarget {
            mesh_key: key,
            origin: [-0.5; 3],
            step: [0.0; 3],
            elements: vec![[0; 3]; 4],
        };
        let mut lines = Vec::new();
        morph_lines(&m, &mut lines);
        assert!(lines[0].contains("4 deltas"), "{lines:?}");
        assert!(lines[1].contains("head_fo1_base_000_m_full_lod, level of detail 1"));
        assert!(lines[2].contains("empty target"), "{lines:?}");
        assert!(lines[3].contains("not applied to a mesh"), "{lines:?}");
        (m.mesh_key, m.step) = (key ^ 1, [1e-6; 3]);
        lines.clear();
        morph_lines(&m, &mut lines);
        assert_eq!(lines.len(), 2, "{lines:?}");
    }
}
