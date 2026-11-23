//! The collections the installed game's DLC adds for the freemode peds: the list a pack that replaces the game's files chooses from.
//!
//! ⚠ Read once on a worker, with every DLC mounted, since the shop metadata each DLC registers is what names them. Nothing
//! here reads a file on the UI thread, and a game that is not mounted or has no keys yet leaves the list empty and says why.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;
use or_title::install::freemode::{dlc_collections, DlcCollection};
use or_title::vfs::{build_mounted_game_with, DlcCutoff};
use or_title::GtaInstall;

use crate::state::AppState;

#[derive(Default)]
enum Listing {
    #[default]
    Idle,
    Reading(Receiver<Result<Vec<DlcCollection>, String>>),
    Ready(Vec<DlcCollection>),
    Failed(String),
}

/// What the installed game's DLC list says, read when a window that offers it asks.
#[derive(Default)]
pub struct GameCollections {
    listing: Listing,
    /// The install the list was read from, so a different one is read again.
    root: Option<PathBuf>,
}

impl GameCollections {
    /// The collections the game's DLC adds, none while they are being read or when the game could not be.
    pub fn collections(&self) -> &[DlcCollection] {
        match &self.listing {
            Listing::Ready(found) => found,
            _ => &[],
        }
    }

    pub fn reading(&self) -> bool {
        matches!(self.listing, Listing::Reading(_))
    }

    /// Why there is no list, none while there is one or one is coming.
    pub fn failed(&self) -> Option<&str> {
        match &self.listing {
            Listing::Failed(why) => Some(why),
            _ => None,
        }
    }

    /// Take what the worker found, if it has finished.
    fn take(&mut self) {
        let Listing::Reading(receiver) = &self.listing else {
            return;
        };
        self.listing = match receiver.try_recv() {
            Ok(Ok(found)) => Listing::Ready(found),
            Ok(Err(why)) => Listing::Failed(why),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Listing::Failed(tr!("the game's DLC list could not be read").to_string())
            }
        };
    }

    /// Read the list of `root` on a worker, unless it is read or being read already.
    fn start(
        &mut self,
        root: Option<PathBuf>,
        crypto: Arc<dyn RpfCryptoProvider>,
        mount: or_title::MountOptions,
    ) {
        if self.root != root {
            self.root.clone_from(&root);
            self.listing = Listing::Idle;
        }
        let (Listing::Idle, Some(root)) = (&self.listing, root) else {
            return;
        };
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let read = build_mounted_game_with(&GtaInstall { root }, crypto, DlcCutoff::All, mount)
                .map(|game| dlc_collections(&game))
                .map_err(|e| tr!("the game could not be mounted: {e}", e = e));
            let _ = sender.send(read);
        });
        self.listing = Listing::Reading(receiver);
    }
}

impl AppState {
    /// Keep the list of the game's DLC collections coming while a window shows it.
    ///
    /// ⚠ Not before the game's keys are read: an archive with no key to open it fails the mount, and a retry does not mend that by itself.
    pub fn poll_game_collections(&mut self) {
        self.workbench.collections.take();
        if !self.keys.has_keys() {
            return;
        }
        let root = self.prefs.active_game_root().cloned();
        let (crypto, mount) = (self.keys.provider(), self.prefs.mount_options());
        self.workbench.collections.start(root, crypto, mount);
    }
}
