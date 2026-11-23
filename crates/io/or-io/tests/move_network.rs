//! `.mrf` move network definitions: every network both installs ship reads,
//! resolves each state machine's states and each transition's target, and
//! saves byte-identical.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! The networks sit inside nested archives (`networkdefs.rpf`), so each
//! install's own executable supplies the keys and the bundled snapshot the NG
//! ones (D18).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfCryptoProvider};
use or_io::mrf::{MvNetworkDef, NodeBody};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// How deep a `.rpf` inside a `.rpf` is followed, as the install crawl does.
const MAX_NEST_DEPTH: u32 = 8;

fn provider(keys: &Arc<Gta5Keys>) -> Arc<dyn RpfCryptoProvider> {
    Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(keys)))
}

fn walk_archive(
    archive: &Arc<RpfArchive>,
    label: &str,
    depth: u32,
    visit: &mut dyn FnMut(String, Result<Vec<u8>, String>),
) {
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let name = node.name().to_ascii_lowercase();
        let path = format!("{label}/{}", node.path());
        let i = node.entry_index();
        if name.ends_with(".rpf") && depth < MAX_NEST_DEPTH {
            match archive.open_nested(i) {
                Ok(nested) => walk_archive(&Arc::new(nested), &path, depth + 1, visit),
                Err(e) => visit(path, Err(e.to_string())),
            }
        } else if name.ends_with(".mrf") {
            visit(path, archive.read_entry(i).map_err(|e| e.to_string()));
        }
    }
}

/// Every `.mrf` under `root`, loose or in an archive at any depth.
fn every_mrf(root: &Path, visit: &mut dyn FnMut(String, Result<Vec<u8>, String>)) {
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let lower = label.to_ascii_lowercase();
            if lower.ends_with(".mrf") {
                visit(label, std::fs::read(&path).map_err(|e| e.to_string()));
            } else if lower.ends_with(".rpf") {
                match RpfArchive::open_with(&path, provider(&keys)) {
                    Ok(archive) => walk_archive(&Arc::new(archive), &label, 0, visit),
                    Err(e) => visit(label, Err(e.to_string())),
                }
            }
        }
    }
}

/// Why `bytes` does not open, resolve every edge to a node, and save identical.
fn round_trip(bytes: &[u8]) -> Result<MvNetworkDef, String> {
    let network = MvNetworkDef::open(bytes).map_err(|e| format!("open: {e}"))?;
    for (i, (node, edges)) in network.nodes.iter().zip(network.edges()).enumerate() {
        let state_kind = !matches!(node.body, NodeBody::Other(_));
        if (state_kind && edges.initial.is_none())
            || edges
                .states
                .iter()
                .chain(&edges.transitions)
                .any(Option::is_none)
        {
            return Err(format!("node {i} has an edge to no node: {edges:?}"));
        }
    }
    let saved = network.save().map_err(|e| format!("save: {e}"))?;
    if saved != bytes {
        let at = saved.iter().zip(bytes).position(|(a, b)| a != b);
        return Err(format!(
            "saved {} bytes of {}, first difference at {at:?}",
            saved.len(),
            bytes.len()
        ));
    }
    Ok(network)
}

#[test]
fn every_shipped_network_reads_and_saves_byte_identical() {
    let what = "every shipped .mrf byte-identical";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let mut seen = 0usize;
        let (mut xrefs, mut requests, mut flags, mut strings) = (0, 0, 0, 0);
        let (mut transitions, mut conditions) = (0, 0);
        let mut owners_file = false;
        let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut failed: Vec<String> = Vec::new();
        every_mrf(&root, &mut |label, bytes| {
            seen += 1;
            match bytes.and_then(|b| round_trip(&b)) {
                Ok(network) => {
                    owners_file |= label
                        .to_ascii_lowercase()
                        .ends_with("networkdefs.rpf/taskexitvehicle.mrf");
                    xrefs += network.xrefs.len();
                    requests += network.requests.len();
                    flags += network.flags.len();
                    strings += network.string_table.len();
                    for node in &network.nodes {
                        *kinds.entry(node.node_type.name()).or_default() += 1;
                        let list = match &node.body {
                            NodeBody::StateMachine(sm) => &sm.transitions,
                            NodeBody::State(s) => &s.transitions,
                            _ => continue,
                        };
                        transitions += list.len();
                        conditions += list.iter().map(|t| t.conditions.len()).sum::<usize>();
                    }
                }
                Err(why) => failed.push(format!("{label}: {why}")),
            }
        });
        eprintln!(
            "{}: {seen} networks, {} failed; {xrefs} cross-references, {requests} requests, \
             {flags} flags, {strings} string table bytes, {transitions} transitions with \
             {conditions} conditions; nodes by kind {kinds:?}",
            root.display(),
            failed.len()
        );
        assert!(seen > 0, "{}: no .mrf", root.display());
        assert!(
            owners_file,
            "{}: taskexitvehicle.mrf in networkdefs.rpf did not round-trip",
            root.display()
        );
        assert!(
            failed.is_empty(),
            "{} of {seen} networks under {} failed:\n{}",
            failed.len(),
            root.display(),
            failed.join("\n")
        );
    }
}
