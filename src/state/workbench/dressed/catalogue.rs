//! The catalogue: a picture of each garment of the Wardrobe's pack, or several turned round it, drawn on a worker with no window and written as WebP files.
//!
//! ⚠ **The picture is drawn on a worker's own device.** The viewport's renderer is the one on screen;
//! a catalogue resizes and refills a renderer, so it gets one of its own and the preview goes on
//! drawing. The scenes are read here a garment a frame, where the pack is.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, sync_channel, Receiver, SyncSender, TrySendError};

use or_wardrobe::{Garment, Slot};

use crate::render::dressed::DressedScene;
use crate::state::AppState;

/// The side of a picture, in pixels.
pub const SIZE: u32 = 256;

/// How many pictures round a garment the catalogue offers; one is the garment seen from where its slot is best seen.
pub const TURNS: [u32; 4] = [1, 4, 8, 12];

/// Scenes read and not yet drawn: each holds a model and its texture.
const READ_AHEAD: usize = 2;

/// A garment to draw, read when its turn comes.
struct Order {
    id: String,
    variation: usize,
    /// The pictures' file stem.
    name: String,
    /// The garment as an earlier version of the pack held it, drawn in place of the pack's own.
    then: Option<Garment>,
}

/// A garment to draw.
struct Request {
    scene: DressedScene,
    id: String,
    slot: Option<Slot>,
    /// The pictures' file stem.
    name: String,
}

/// What a worker made of one garment: how many files it wrote, or why it wrote none.
type Made = Result<usize, String>;

/// A catalogue being made, or just made.
pub struct Catalogue {
    pub out: PathBuf,
    /// How many pictures are made of each garment.
    pub turns: u32,
    /// Garments not read yet.
    waiting: VecDeque<Order>,
    /// A garment read and not yet taken by the worker.
    pending: Option<Request>,
    sender: Option<SyncSender<Request>>,
    made: Receiver<Made>,
    /// How many garments the catalogue is of, and how many are done.
    pub total: usize,
    pub done: usize,
    /// The files written.
    pub written: usize,
    /// One sentence for each garment that was not drawn.
    pub failed: Vec<String>,
}

impl Catalogue {
    pub fn is_finished(&self) -> bool {
        self.done >= self.total
    }
}

/// `stem`, or `stem_2` and on when an earlier garment of the catalogue has taken it.
fn free_stem(stem: &str, taken: &mut HashSet<String>) -> String {
    let clean: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_^.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut name = clean.clone();
    let mut n = 2;
    while !taken.insert(name.to_ascii_lowercase()) {
        name = format!("{clean}_{n}");
        n += 1;
    }
    name
}

/// The files of the pictures of one garment: its stem alone for one picture, and its stem with the turn for more.
fn file_names(stem: &str, turns: u32) -> Vec<String> {
    match turns {
        0 | 1 => vec![format!("{stem}.webp")],
        n => (0..n).map(|k| format!("{stem}_{k:02}.webp")).collect(),
    }
}

/// Draw each request on a device of this thread's own, and say what became of each.
fn work(requests: Receiver<Request>, made: std::sync::mpsc::Sender<Made>, out: &Path, turns: u32) {
    let mut bridge = crate::self_check::headless_bridge();
    for request in requests {
        let result = match &mut bridge {
            Err(why) => Err(why.clone()),
            Ok(bridge) => (0..turns.max(1))
                .zip(file_names(&request.name, turns))
                .try_for_each(|(k, file)| {
                    let turn = std::f32::consts::TAU * k as f32 / turns.max(1) as f32;
                    crate::photo::picture_of_garment(
                        bridge,
                        &request.scene,
                        &request.id,
                        request.slot,
                        SIZE,
                        turn,
                    )
                    .and_then(|image| crate::photo::save(&image, &out.join(file)))
                })
                .map(|()| turns.max(1) as usize),
        };
        if made.send(result).is_err() {
            return;
        }
    }
}

impl AppState {
    /// Draw a picture of each garment of the Wardrobe's pack, `turns` of them round it, into `out`.
    pub fn start_catalogue(&mut self, out: PathBuf, turns: u32) -> Result<(), String> {
        let pack = self.wardrobe_pack().ok_or(tr!("no clothing is open"))?;
        let read = pack
            .read
            .as_ref()
            .ok()
            .ok_or(tr!("the clothing cannot be read"))?;
        let mut taken = HashSet::new();
        let waiting: VecDeque<Order> = (read.wardrobe.garments.iter())
            .filter_map(|garment| {
                let model = garment.model.as_deref()?;
                let stem = Path::new(model).file_stem()?.to_string_lossy();
                let variation = self.dressed_variation(&garment.id);
                let letter = char::from(b'a' + u8::try_from(variation).unwrap_or(25).min(25));
                let stem = match garment.variations.len() > 1 {
                    true => format!("{stem}_{letter}"),
                    false => stem.into_owned(),
                };
                Some(Order {
                    id: garment.id.clone(),
                    variation,
                    name: free_stem(&stem, &mut taken),
                    then: None,
                })
            })
            .collect();
        if waiting.is_empty() {
            return Err(tr!("the pack has no garment with a model to draw").into());
        }
        self.begin_catalogue(out, turns, waiting)
    }

    /// Draw each garment that differs between two versions of the pack as each version held it, `<name>_before` and `<name>_after`, into `out`.
    ///
    /// A garment added has only an after and one removed only a before; one with no model has no picture.
    pub fn start_review_pictures(
        &mut self,
        out: PathBuf,
        review: &or_wardrobe::review::Review,
        before: &or_wardrobe::Wardrobe,
        after: &or_wardrobe::Wardrobe,
    ) -> Result<(), String> {
        let mut taken = HashSet::new();
        let mut waiting: VecDeque<Order> = VecDeque::new();
        for change in &review.garments {
            for (side, pack) in [("before", before), ("after", after)] {
                let Some(garment) = pack.garment(&change.id).filter(|g| g.model.is_some()) else {
                    continue;
                };
                let stem = format!("{}_{side}", or_wardrobe::rules::name_of(garment));
                waiting.push_back(Order {
                    id: garment.id.clone(),
                    variation: 0,
                    name: free_stem(&stem, &mut taken),
                    then: Some(garment.clone()),
                });
            }
        }
        if waiting.is_empty() {
            return Err(tr!("no garment that differs has a model to draw").into());
        }
        self.begin_catalogue(out, 1, waiting)
    }

    /// Start the worker that draws `waiting` into `out`, `turns` pictures of each.
    fn begin_catalogue(
        &mut self,
        out: PathBuf,
        turns: u32,
        waiting: VecDeque<Order>,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&out)
            .map_err(|e| tr!("cannot make {out}: {e}", e = e, out = out.display()))?;
        let (sender, requests) = sync_channel(READ_AHEAD);
        let (reply, made) = channel();
        let (to, turns) = (out.clone(), turns.max(1));
        std::thread::spawn(move || work(requests, reply, &to, turns));
        self.workbench.wardrobe.dressed.catalogue = Some(Catalogue {
            out,
            turns,
            total: waiting.len(),
            waiting,
            pending: None,
            sender: Some(sender),
            made,
            done: 0,
            written: 0,
            failed: Vec::new(),
        });
        Ok(())
    }

    /// Leave the catalogue: a catalogue under way is stopped, and what it wrote stays.
    pub fn end_catalogue(&mut self) {
        self.workbench.wardrobe.dressed.catalogue = None;
    }

    /// Read the next garment of the catalogue and take what the worker has made; once a frame.
    pub(super) fn poll_catalogue(&mut self) {
        let Some(mut job) = self.workbench.wardrobe.dressed.catalogue.take() else {
            return;
        };
        while let Ok(made) = job.made.try_recv() {
            job.done += 1;
            match made {
                Ok(files) => job.written += files,
                Err(why) => job.failed.push(why),
            }
        }
        if job.pending.is_none() {
            if let Some(Order {
                id,
                variation,
                name,
                then,
            }) = job.waiting.pop_front()
            {
                match self.garment_alone_as(then.as_ref(), &id, variation) {
                    Ok(scene) => {
                        let slot = match &then {
                            Some(garment) => garment.slot,
                            None => self
                                .wardrobe_pack()
                                .and_then(|p| p.read.as_ref().ok())
                                .and_then(|read| read.wardrobe.garment(&id))
                                .and_then(|garment| garment.slot),
                        };
                        job.pending = Some(Request {
                            scene,
                            id,
                            slot,
                            name,
                        });
                    }
                    Err(why) => {
                        job.done += 1;
                        job.failed.push(format!("{name}: {why}"));
                    }
                }
            }
        }
        if let (Some(request), Some(sender)) = (job.pending.take(), &job.sender) {
            match sender.try_send(request) {
                Ok(()) => {}
                Err(TrySendError::Full(request)) => job.pending = Some(request),
                Err(TrySendError::Disconnected(request)) => {
                    job.failed
                        .push(tr!("{name}: the drawing stopped", name = request.name));
                    job.done += 1;
                }
            }
        }
        if job.waiting.is_empty() && job.pending.is_none() {
            // The worker ends once it has drawn what it was handed.
            job.sender = None;
        }
        self.workbench.wardrobe.dressed.catalogue = Some(job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stem_taken_by_an_earlier_garment_gets_a_number_and_a_name_a_file_cannot_carry_is_mended() {
        let mut taken = HashSet::new();
        assert_eq!(free_stem("jbib_000_u", &mut taken), "jbib_000_u");
        assert_eq!(free_stem("JBIB_000_U", &mut taken), "JBIB_000_U_2");
        assert_eq!(free_stem("a/b:c", &mut taken), "a_b_c");
    }

    #[test]
    fn one_picture_is_named_by_its_stem_and_a_turntable_numbers_each_picture_round() {
        assert_eq!(file_names("jbib", 1), ["jbib.webp"]);
        assert_eq!(
            file_names("jbib", 4),
            [
                "jbib_00.webp",
                "jbib_01.webp",
                "jbib_02.webp",
                "jbib_03.webp"
            ]
        );
    }
}
