//! The third picture of A/B: the ped wearing the pack as its previous kept version stated it.

use or_wardrobe::backup;

use crate::state::AppState;

impl AppState {
    /// Make B the pack as it was before its latest change, read from the versions the Wardrobe keeps; a pack with no earlier version says so.
    pub fn show_previous_version(&mut self) {
        let earlier = self.wardrobe_pack().and_then(|pack| {
            let read = pack.read.as_ref().ok()?;
            backup::previous(&pack.folder, &read.wardrobe)
        });
        match earlier {
            Some((_, pack)) => {
                let dressed = &mut self.workbench.wardrobe.dressed;
                dressed.earlier = Some(pack);
                dressed.previous = true;
                dressed.original = false;
            }
            None => self.show_info_toast(
                tr!("The pack has no earlier version to show: the first change made to it keeps the pack as it is now"),
            ),
        }
    }
}
