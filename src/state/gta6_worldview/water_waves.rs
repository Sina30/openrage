//! The waves of the GTA VI world's water: the ocean simulation's presets, read once on a worker and
//! held, and the sum of waves the surface is drawn with for the day's wind.
//!
//! ⚠ The game's wave program was not read: the length of the waves is that at the peak of Pierson
//! and Moskowitz's fully developed sea, their slopes Cox and Munk's measured spread for the wind,
//! the wind of a Beaufort number the WMO's, all assumed. The presets give each octave's strength
//! along the wind and pick, by the nearest place each was set up at, the kind of water.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use or_io::container::rpf::{read_rpf9_body, GtaViKeys};
use or_io::gta6::water::waves::{OceanPreset, OceanPresets};
use or_renderer::world::water::WAVES;
use or_title::gta6_mount::table::entry_offset;

use super::ground::{held, reading};

/// The archive and the document that state the presets.
const ARCHIVE: &str = "common.rpf";
const ENTRY: &str = "data/levels/americas/water/oceansimpresets.xml";
const BODY_LIMIT: u64 = 1 << 20;

/// Metres a second squared.
const GRAVITY: f32 = 9.81;
/// The Beaufort number a preset's curves end at.
const LAST_BEAUFORT: f32 = 12.0;
/// The least wind, in metres a second, so that a calm day still ripples.
const LEAST_WIND: f32 = 1.0;
/// Where the waves travel to: west, as the trade wind blows (assumed: the day states no wind's direction the water reads).
const TRAVEL: f32 = std::f32::consts::PI;
/// The waves of an octave: how far each turns from the wind, in degrees, and how its length stands to the octave's.
const SPREAD: [f32; 4] = [-31.0, -9.0, 13.0, 34.0];
const STRETCH: [f32; 4] = [1.0, 0.83, 1.19, 0.71];
/// How much shorter each octave's waves are than the one before.
const OCTAVE_STEP: f32 = 4.0;
/// How far each octave turns from the one before, in degrees, so that no two line up.
const OCTAVE_TURN: f32 = 17.0;

type Read = Arc<OnceLock<Option<Arc<OceanPresets>>>>;

static HELD: Mutex<Vec<(PathBuf, Read)>> = Mutex::new(Vec::new());

/// The ocean's presets of the install at `root`, read once on a worker and held: `None` until read, and where they do not read.
pub fn presets(root: &Path) -> Option<Arc<OceanPresets>> {
    held(&HELD, root, "the ocean's presets", read)
}

/// Whether the presets of the install at `root` were asked for and are still being read.
pub fn presets_reading(root: &Path) -> bool {
    reading(&HELD, root)
}

fn read(root: &Path, keys: &GtaViKeys) -> Result<OceanPresets, String> {
    let archive = root.join(ARCHIVE);
    let at = entry_offset(&archive, ENTRY, keys).ok_or("the table names no ocean presets")?;
    let file = File::open(&archive).map_err(|e| e.to_string())?;
    let (_, text) = read_rpf9_body(&file, at, BODY_LIMIT).map_err(|e| e.to_string())?;
    OceanPresets::parse(&String::from_utf8_lossy(&text)).map_err(|e| e.to_string())
}

/// The waves of a sea at Beaufort `beaufort` of the kind `preset` is, `seconds` into the day: each wave's number a metre along x and y, its height and its phase now.
pub fn waves(preset: Option<&OceanPreset>, beaufort: f32, seconds: f64) -> [[f32; 4]; WAVES] {
    let wind = (0.836 * beaufort.max(0.0).powf(1.5)).max(LEAST_WIND);
    // Pierson and Moskowitz: the length at the peak of a fully developed sea.
    let peak = std::f32::consts::TAU * wind * wind / (0.877 * 0.877 * GRAVITY);
    // Cox and Munk: the spread of the sea's slopes, both ways summed, shared alike among the waves.
    let slopes = 0.003 + 0.00512 * wind;
    let slope = (2.0 * slopes / WAVES as f32).sqrt();
    let along = (beaufort / LAST_BEAUFORT).clamp(0.0, 1.0);
    let each = SPREAD.len();
    std::array::from_fn(|i| {
        let (octave, k) = (i / each, i % each);
        let Some((spread, stretch)) = SPREAD.get(k).zip(STRETCH.get(k)) else {
            return [0.0; 4];
        };
        let shorter = OCTAVE_STEP.powi(octave as i32);
        let length = peak * stretch / shorter;
        // A sine of height a and number k has slopes of spread (a k)^2 / 2.
        let strength = preset.map_or(1.0, |p| p.strength(octave, along));
        let angle = TRAVEL + (spread + OCTAVE_TURN * octave as f32).to_radians();
        let number = std::f32::consts::TAU / length;
        let amplitude = slope / number * strength;
        let speed = f64::from((GRAVITY * number).sqrt());
        // A phase of its own at the day's start for each wave, and the phase now worked out in doubles.
        let start = f64::from((i as f32 * 2.399_963).rem_euclid(std::f32::consts::TAU));
        let phase = (start - speed * seconds).rem_euclid(std::f64::consts::TAU) as f32;
        [number * angle.cos(), number * angle.sin(), amplitude, phase]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_waves_follow_the_wind_the_preset_and_the_clock() {
        let calm = waves(None, 0.0, 0.0);
        let fresh = waves(None, 5.0, 0.0);
        let height = |w: &[[f32; 4]; WAVES]| w.iter().map(|w| w[2]).sum::<f32>();
        assert!(
            height(&fresh) > 5.0 * height(&calm),
            "a fresh breeze raises the sea"
        );
        let spread = |w: &[[f32; 4]; WAVES]| {
            (w.iter())
                .map(|w| (w[2] * w[0].hypot(w[1])).powi(2) / 2.0)
                .sum::<f32>()
        };
        let wind = 0.836 * 5f32.powf(1.5);
        assert!(
            (spread(&fresh) - (0.003 + 0.00512 * wind)).abs() < 1e-4,
            "Cox and Munk's slopes"
        );
        let number = |w: [f32; 4]| w[0].hypot(w[1]);
        assert!(
            number(fresh[0]) < number(calm[0]),
            "and lengthens its waves"
        );
        assert!(
            number(fresh[8]) > 10.0 * number(fresh[0]),
            "the third octave's waves are sixteen times shorter"
        );
        assert!(fresh[0][0] < 0.0, "the first wave travels west");
        let later = waves(None, 5.0, 3600.0 * 15.0);
        assert!(later.iter().zip(&fresh).all(|(a, b)| a[..3] == b[..3]));
        assert!(later
            .iter()
            .all(|w| (0.0..std::f32::consts::TAU).contains(&w[3])));
        assert_ne!(later[0][3], fresh[0][3], "the clock moves the phase");
        let still = OceanPreset {
            octaves: vec![vec![[0.0, 0.0], [1.0, 0.0]]],
            ..OceanPreset::default()
        };
        let stilled = waves(Some(&still), 5.0, 0.0);
        assert!(
            stilled[..4].iter().all(|w| w[2] == 0.0),
            "a preset's octave of no strength"
        );
        assert_eq!(stilled[4][2], fresh[4][2], "an octave it does not state");
    }
}
