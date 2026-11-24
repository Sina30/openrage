//! A GTA VI MoVE network's nodes as the inspector opens them one by one, kept for the document the
//! network opened as.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::gta6::move_network::{kind_name, MoveBody, MoveCondition, MoveOutline, MoveTransition};

// ponytail: one entry per network opened, never emptied, as the scanned lists are.
static OUTLINES: Mutex<BTreeMap<PathBuf, Arc<MoveOutline>>> = Mutex::new(BTreeMap::new());

fn outlines() -> MutexGuard<'static, BTreeMap<PathBuf, Arc<MoveOutline>>> {
    OUTLINES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keep `outline` for the document at `path`.
pub(super) fn keep(path: &Path, outline: MoveOutline) {
    outlines().insert(path.to_path_buf(), Arc::new(outline));
}

/// The network the document at `path` opened from; `None` for any other document.
pub(crate) fn outline(path: &Path) -> Option<Arc<MoveOutline>> {
    outlines().get(path).cloned()
}

/// A node's kind and id, as the document's lines name it.
pub(crate) fn label(outline: &MoveOutline, at: usize) -> String {
    super::node_label(outline, at)
}

/// The nodes `at` leads to: a machine's inputs and states, a state's output, a leaf's inputs.
pub(crate) fn children(outline: &MoveOutline, at: usize) -> Vec<usize> {
    match outline.node(at).map(|n| &n.body) {
        Some(MoveBody::Machine(m)) => (m.inputs.iter().copied())
            .chain(m.states.iter().map(|s| s.1))
            .collect(),
        Some(MoveBody::State(s)) => vec![s.output],
        Some(MoveBody::Leaf { inputs, .. }) => inputs.clone(),
        None => Vec::new(),
    }
}

/// The fields of the node at `at` as a label and a value each; a node not read says why.
pub(crate) fn rows(outline: &MoveOutline, at: usize) -> Vec<(String, String)> {
    let kind = |kind: u16| kind_name(kind).map_or(kind.to_string(), |n| format!("{kind} ({n})"));
    let mut rows = vec![(tr!("Offset").to_string(), format!("{at:#X}"))];
    let Some(node) = outline.node(at) else {
        if let Some(stop) = outline.stop(at) {
            rows.push((tr!("Kind").to_string(), kind(stop.kind)));
            rows.push((tr!("Child index").to_string(), stop.index.to_string()));
            rows.push((tr!("Id").to_string(), format!("hash_{:08X}", stop.id)));
            name_row(stop.id, &mut rows);
            rows.push((
                tr!("Word at +0x08").to_string(),
                format!("{:#010X}", stop.word),
            ));
            rows.push((
                tr!("Not read").to_string(),
                super::why(stop.reason).to_string(),
            ));
        }
        return rows;
    };
    rows.push((tr!("Kind").to_string(), kind(node.kind)));
    rows.push((tr!("Child index").to_string(), node.index.to_string()));
    rows.push((tr!("Id").to_string(), format!("hash_{:08X}", node.id)));
    name_row(node.id, &mut rows);
    rows.push((tr!("Bytes").to_string(), node.size.to_string()));
    match &node.body {
        MoveBody::Machine(m) => {
            let initial =
                (m.initial).map_or_else(|| tr!("none").to_string(), |s| label(outline, s));
            rows.push((tr!("Initial state").to_string(), initial));
            rows.push((tr!("Entries").to_string(), m.entries.len().to_string()));
            transitions(outline, &m.transitions, &mut rows);
            for e in &m.entries {
                rows.push((tr!("Entry").to_string(), super::entry_line(outline, e)));
                conditions(&e.conditions, &mut rows);
            }
        }
        MoveBody::State(s) => {
            rows.push((tr!("Children").to_string(), s.children.to_string()));
            rows.push((tr!("Output node").to_string(), label(outline, s.output)));
            transitions(outline, &s.transitions, &mut rows);
            for b in &s.bindings {
                rows.push((tr!("Binding").to_string(), super::binding_line(b)));
            }
        }
        MoveBody::Leaf { flags, inputs } => {
            rows.push((tr!("Flags").to_string(), format!("{flags:#010X}")));
            rows.push((tr!("Input nodes").to_string(), inputs.len().to_string()));
        }
    }
    rows
}

fn transitions(
    outline: &MoveOutline,
    transitions: &[MoveTransition],
    rows: &mut Vec<(String, String)>,
) {
    for t in transitions {
        let to = tr!(
            "transition to {target}, {size} bytes",
            target = label(outline, t.target),
            size = t.size,
        );
        rows.push((tr!("Transition").to_string(), to));
        rows.push((tr!("Transition").to_string(), super::transition_words(t)));
        conditions(&t.conditions, rows);
    }
}

/// A row of the name the harvested text holds for `id`, where it holds one.
fn name_row(id: u32, rows: &mut Vec<(String, String)>) {
    if let Some(name) = super::text_name(id) {
        rows.push((tr!("Name").to_string(), name));
    }
}

fn conditions(conditions: &[MoveCondition], rows: &mut Vec<(String, String)>) {
    for c in conditions {
        rows.push((tr!("Condition").to_string(), super::condition_line(c)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A network with every list empty whose root node is of `kind`, with `word` at its +0x08.
    fn network(kind: u16, word: u32) -> Vec<u8> {
        let mut bytes = b"MoVE\x13".to_vec();
        bytes.extend_from_slice(&[0; 15]);
        bytes.extend_from_slice(&0xCAFE_F00Du32.to_le_bytes());
        // The length word, the zero word, then empty lists A and B, the word after B and list D.
        bytes.extend_from_slice(&[0; 4 * 6]);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&word.to_le_bytes());
        let length = (bytes.len() - 0x2E) as u32;
        bytes[0x18..0x1C].copy_from_slice(&length.to_le_bytes());
        bytes
    }

    fn value<'a>(rows: &'a [(String, String)], label: &str) -> Option<&'a str> {
        (rows.iter())
            .find(|(l, _)| l == label)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_node_read_lists_its_fields_and_leads_to_its_inputs() {
        let outline = MoveOutline::open(&network(2, 0)).expect("opens");
        let rows = rows(&outline, outline.root);
        assert_eq!(value(&rows, "Kind"), Some("2 (Tail)"), "{rows:?}");
        assert_eq!(value(&rows, "Id"), Some("hash_00000007"));
        assert_eq!(value(&rows, "Bytes"), Some("12"));
        assert_eq!(value(&rows, "Input nodes"), Some("0"));
        assert!(children(&outline, outline.root).is_empty());
    }

    #[test]
    fn a_node_not_read_says_why_and_leads_nowhere() {
        let outline = MoveOutline::open(&network(28, 0x0A)).expect("opens");
        let rows = rows(&outline, outline.root);
        assert_eq!(
            value(&rows, "Word at +0x08"),
            Some("0x0000000A"),
            "{rows:?}"
        );
        assert!(value(&rows, "Not read").is_some(), "{rows:?}");
        assert!(children(&outline, outline.root).is_empty());
        let path = Path::new("0x3A.txt");
        keep(path, outline.clone());
        assert_eq!(super::outline(path).as_deref(), Some(&outline));
    }
}
