//! A `.mrf` move network as a document: its state machines, states,
//! transitions and nodes shown in the inspector, saved as it was read.

use or_io::mrf::MvNetworkDef;

use super::{AppState, DocPayload, DocumentId};

impl AppState {
    pub fn move_network(&self, doc: DocumentId) -> Result<&MvNetworkDef, String> {
        match self.document(doc).map(|d| &d.payload) {
            Some(DocPayload::MoveNetwork(network)) => Ok(network),
            _ => Err("not a .mrf document".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

    use super::*;
    use crate::state::opening_a_ydd::{finish_load, node_at, with_keys};
    use crate::state::{DocumentKind, LoadingState};

    /// The owner's report: this file, read through the tree, did not open.
    const OWNERS_NETWORK: &str =
        "update/update.rpf/x64/patch/anim/networkdefs.rpf/taskexitvehicle.mrf";

    fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
        state.load_file(path);
        let deadline = Instant::now() + Duration::from_secs(30);
        while state.load_receiver.is_some() && Instant::now() < deadline {
            state.poll_load_result();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            matches!(state.loading, LoadingState::Idle),
            "the load did not finish: {}",
            state.error_dialog_message
        );
        state.active_document.expect("the .mrf opened")
    }

    #[test]
    fn a_network_opens_as_its_own_kind_and_saves_as_it_was_read() {
        let bytes = MvNetworkDef {
            version: or_io::mrf::VERSION,
            xrefs: Vec::new(),
            requests: vec![or_io::mrf::KeyIndex { key: 7, index: 0 }],
            flags: Vec::new(),
            nodes: Vec::new(),
            string_table: Vec::new(),
        }
        .save()
        .expect("the network saves");
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("taskexitvehicle.mrf");
        std::fs::write(&path, &bytes).expect("written");
        let mut state = AppState::new();
        let doc = open(&mut state, path);
        assert_eq!(
            state.document(doc).expect("the document").kind,
            DocumentKind::MoveNetwork
        );
        let network = state.move_network(doc).expect("a move network");
        assert_eq!(network.requests.len(), 1);
        assert_eq!(network.save().expect("saves"), bytes);
    }

    #[test]
    fn the_owners_network_opens_from_the_tree() {
        const WHAT: &str = "the owner's taskexitvehicle.mrf opens from the Legacy install's tree";
        let Some(root) = game_roots().into_iter().find(|root| {
            root.join("GTA5.exe").is_file() && root.join("update/update.rpf").is_file()
        }) else {
            skip_install(WHAT, format_args!("no Legacy install; set {GAME_ROOT_ENV}"));
            return;
        };

        let mut state = with_keys(&root);
        state.open_game_root(root);
        let game = state.documents[0].id;
        let node = node_at(&mut state, game, OWNERS_NETWORK);
        state.clear_error();
        state.open_rpf_entry(game, node);
        let doc = finish_load(&mut state).expect("the owner's .mrf opens");
        let network = state.move_network(doc).expect("a move network");
        assert!(
            network.edges().iter().any(|e| !e.states.is_empty()),
            "{WHAT}: no state machine"
        );
    }
}
