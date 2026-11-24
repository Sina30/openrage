//! The water of the GTA VI world: the level's quads and the ground under them, each read once on a
//! worker and held, the day's medium, and the mesh of the quads around the camera.
//!
//! ⚠ The quads are the sea, the lakes and the rivers as level rectangles and the triangles a cut
//! leaves of them; the ground is the height map, not the surface, which is each quad's height.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use or_io::container::rpf::read_rpf9_body;
use or_io::gta6::water::height::WaterGround;
use or_io::gta6::water::WaterData;
use or_title::gta6_day::DayTables;

use crate::state::drivers::WeatherState;

#[path = "water_ground.rs"]
mod ground;
#[path = "water_mesh.rs"]
mod mesh;

pub use ground::ground;
pub use mesh::{mesh, tint, Medium};

/// The archive of an install the water document is in, and the document's path in it.
const ARCHIVE: &str = "levels.rpf";
const ENTRY: &str = "levels/americas/water";
/// The bytes the water document is read to.
const BODY_LIMIT: u64 = 16 << 20;

type Read = Arc<OnceLock<Option<Arc<WaterData>>>>;

static HELD: Mutex<Vec<(PathBuf, Read)>> = Mutex::new(Vec::new());

/// The water document at `offset` of the GTA VI archive at `archive`.
pub fn read(archive: &Path, offset: u64) -> Result<WaterData, String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let (_, body) = read_rpf9_body(&file, offset, BODY_LIMIT).map_err(|e| e.to_string())?;
    WaterData::open(&body).map_err(|e| e.to_string())
}

/// The water of the install at `root`, read once on a worker and held: `None` until it is read, and where the install's table gives no such document or it does not read.
pub fn water(root: &Path) -> Option<Arc<WaterData>> {
    let archive = root.join(ARCHIVE);
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    let slot = match held.iter().find(|(path, _)| *path == archive) {
        Some((_, slot)) => Arc::clone(slot),
        None => {
            let slot = Read::default();
            held.push((archive.clone(), Arc::clone(&slot)));
            let filled = Arc::clone(&slot);
            // ⚠ Where the key snapshot lies is found here, on the caller's thread, as the index finds it.
            let keys = super::index::keys();
            std::thread::spawn(move || {
                let found = keys.as_ref().and_then(|keys| {
                    or_title::gta6_mount::table::entry_offset(&archive, ENTRY, keys)
                });
                let water = match found.map(|offset| read(&archive, offset)) {
                    Some(Ok(water)) => Some(Arc::new(water)),
                    Some(Err(why)) => {
                        tracing::warn!(archive = %archive.display(), %why, "GTA VI's water is not read");
                        None
                    }
                    None => {
                        tracing::info!(archive = %archive.display(), "no key snapshot deciphers the archive's table, or it names no water document");
                        None
                    }
                };
                let _ = filled.set(water);
            });
            slot
        }
    };
    slot.get()?.clone()
}

/// What the mesh of the water is built of, by the place of each part that is held: a part that arrives or changes asks for it again.
pub type SeenKey = (usize, usize, usize, &'static str);

/// The water of an install as a frame takes it: its quads, the ground under them once read, and the day it is seen in.
#[derive(Clone)]
pub struct Seen {
    pub quads: Arc<WaterData>,
    pub ground: Option<Arc<WaterGround>>,
    day: Option<Arc<DayTables>>,
    hour: f32,
    /// The name of the day's cycle the weather stands for.
    weather: &'static str,
    y: f32,
}

impl Seen {
    /// The medium the water is at the hour and weather it was seen in; `None` until the day is read or where it states none.
    pub fn medium(&self) -> Option<Medium> {
        let moment = (self.day.as_ref()?).moment(self.hour, self.weather, self.y)?;
        Medium::of(|name| moment.own.get(name))
    }

    /// What a mesh built of this water is built of.
    // ⚠ The hour is not in the key: every water variable but the waves' strength is one value at every hour.
    pub fn key(&self) -> SeenKey {
        let of = |part: Option<usize>| part.unwrap_or(0);
        (
            Arc::as_ptr(&self.quads) as usize,
            of(self.ground.as_ref().map(|g| Arc::as_ptr(g) as usize)),
            of(self.day.as_ref().map(|d| Arc::as_ptr(d) as usize)),
            self.weather,
        )
    }
}

/// The water of the install at `root` as a camera `y` metres north sees it at `hour` in `weather`; `None` until its quads are read.
pub fn seen(root: &Path, hour: f32, weather: WeatherState, y: f32) -> Option<Seen> {
    Some(Seen {
        quads: water(root)?,
        ground: ground(root),
        day: super::sky::tables(root),
        hour,
        weather: super::sky::weather_name(weather),
        y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_that_is_not_there_is_an_error_and_not_a_panic() {
        assert!(read(Path::new("no such archive of water"), 0).is_err());
    }
}
