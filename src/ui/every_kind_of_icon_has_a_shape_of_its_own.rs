//! Colour is never the only thing that tells two icons apart: no two kinds of
//! document, and no two rows of a file's outline, share a glyph.
//!
//! ⚠ The kind colours are mostly greens and teals, so two kinds that differ
//! only by colour look alike to anyone who does not see them apart.

use std::collections::BTreeMap;

use crate::state::DocumentKind;
use crate::ui::icons::{self, IconBadge, YdrSection, YptItem};

/// The glyphs two or more of `icons` draw, with what each is called.
fn shared(icons: impl IntoIterator<Item = (String, IconBadge)>) -> Vec<String> {
    let mut by_glyph: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for (name, icon) in icons {
        by_glyph.entry(icon.glyph).or_default().push(name);
    }
    by_glyph
        .into_iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(glyph, names)| format!("{glyph:?}: {}", names.join(", ")))
        .collect()
}

#[test]
fn no_two_kinds_of_document_share_a_glyph() {
    let kinds = DocumentKind::NAMED.iter().map(|kind| {
        (
            kind.info().noun.to_string(),
            icons::doc_kind_icon(*kind, false),
        )
    });
    let found = shared(kinds);
    assert!(
        found.is_empty(),
        "kinds that differ by colour alone:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_two_rows_of_an_outline_share_a_glyph() {
    use YdrSection::*;
    use YptItem::*;
    let ypt = [
        EffectsSection,
        Effect,
        Emitter,
        Particle,
        EmitterRuleLibrary,
        ParticleRuleLibrary,
        TextureDict,
        ModelDict,
        Domain,
        Evolution,
        Behaviour,
    ]
    .map(|item| (format!("{item:?}"), icons::ypt_icon(item)));
    let ydr = [
        Lods, Lod, Model, Geometry, Shaders, Shader, Skeleton, Bone, Lights, Light, Bound,
    ]
    .map(|section| (format!("{section:?}"), icons::ydr_section_icon(section)));
    let found: Vec<String> = shared(ypt)
        .into_iter()
        .map(|line| format!("a particle file's outline, {line}"))
        .chain(
            shared(ydr)
                .into_iter()
                .map(|line| format!("a drawable's outline, {line}")),
        )
        .collect();
    assert!(
        found.is_empty(),
        "rows that differ by colour alone:\n{}",
        found.join("\n")
    );
}

/// ⚠ A file OpenRage cannot open is dimmed to the secondary colour, which is the colour of the generic binary file's icon, so the dimming says nothing there.
#[test]
fn an_unsupported_file_in_a_tree_is_marked_by_more_than_colour() {
    use crate::state::{RpfNodeBacking, RpfSessionNode, RpfSessionNodeKind};

    let mut unmarked = Vec::new();
    for (name, kind) in [
        ("blob.bin", RpfSessionNodeKind::BinaryFile),
        ("settings.ini", RpfSessionNodeKind::BinaryFile),
        ("thing.xyz", RpfSessionNodeKind::ResourceFile),
    ] {
        let node = RpfSessionNode {
            id: 1,
            parent: None,
            name: name.to_string(),
            kind,
            children: Vec::new(),
            backing: RpfNodeBacking::DiskFile {
                fs_path: name.into(),
                size_hint: 0,
                override_bytes: None,
            },
        };
        let opens = icons::rpf_node_icon(&node, false, false, true, false);
        let cannot = icons::rpf_node_icon(&node, false, false, false, false);
        let seen = (cannot.glyph, cannot.badge) != (opens.glyph, opens.badge);
        if !seen {
            unmarked.push(format!(
                "{name}: the same glyph and badge as a file that opens"
            ));
        } else if cannot.tooltip() == opens.tooltip() {
            unmarked.push(format!(
                "{name}: a tooltip that does not say it cannot open"
            ));
        }
    }
    assert!(
        unmarked.is_empty(),
        "unsupported files marked by colour alone:\n{}",
        unmarked.join("\n")
    );
}
