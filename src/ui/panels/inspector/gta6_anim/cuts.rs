//! A `cin@` clip dictionary's table at root 0x60, read from the document's sections the first time
//! it is opened.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use or_io::gta6::anim::{read_camera_cuts, CameraCut};

use super::*;

type Cuts = Arc<Result<Vec<CameraCut>, String>>;

// ponytail: one entry per document whose table was opened, never emptied, as the scanned lists are.
static CUTS: Mutex<BTreeMap<DocumentId, Cuts>> = Mutex::new(BTreeMap::new());

/// The table of the document `doc_id`, read from `section` once.
fn cuts_of(doc_id: DocumentId, section: &[u8]) -> Cuts {
    let mut held = CUTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let read = held
        .entry(doc_id)
        .or_insert_with(|| Arc::new(read_camera_cuts(section).map_err(|e| e.to_string())));
    Arc::clone(read)
}

/// The entries of the table root 0x60 points at, where it points at one.
pub(super) fn draw_camera_cuts(ui: &mut egui::Ui, doc_id: DocumentId, resource: &AnimResource) {
    let held = (resource.sections.as_deref()).filter(|_| resource.word_at_60 != 0);
    let Some(sections) = held else {
        return;
    };
    egui::CollapsingHeader::new(tr!("Index and time table at root 0x60"))
        .id_salt(("gta6_anim_cuts", doc_id))
        .show(ui, |ui| {
            let cuts = cuts_of(doc_id, &sections.virtual_section);
            let cuts = match cuts.as_ref() {
                Ok(cuts) => cuts,
                Err(why) => {
                    let said = tr!("The table root 0x60 points at does not read: {why}", why = why);
                    ui.colored_label(theme::WARNING, said);
                    return;
                }
            };
            ui.label(tr!("Its entries are named Index, MinTime and MaxTime and listed by index. That they are the cutscene's camera cuts is inferred, and what an entry's own name hashes is not known."));
            let row_height = ui.spacing().interact_size.y;
            egui::ScrollArea::vertical()
                .id_salt(("gta6_anim_cut_rows", doc_id))
                .max_height(row_height * ROWS_IN_VIEW)
                .auto_shrink([false, true])
                .show_rows(ui, row_height, cuts.len(), |ui, range| {
                    for c in &cuts[range] {
                        ui.monospace(format!(
                            "Index {:3}  MinTime {}  MaxTime {}  hash_{:08X}",
                            c.index, c.min_time, c.max_time, c.name
                        ));
                    }
                });
        });
}
