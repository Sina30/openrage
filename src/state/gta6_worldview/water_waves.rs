//! The waves of the GTA VI world's water: the ocean simulation's presets, read once on a worker and
//! held, and the sum of waves the surface is drawn with for the day's wind.
//!
//! The sea's spectrum is the game's: JONSWAP for a wind over a fetch, its peak sharpened by 3.3,
//! spread about the wind as the cosine squared. ⚠ Still assumed: the wind of a Beaufort number
//! (the WMO's), where the waves travel to, and how the twelve waves sample the spectrum. The presets
//! give each octave's strength along the wind and pick, by the nearest place each was set up at, the
//! kind of water.

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
/// How much the spectrum's peak is sharpened, and its width below and above the peak.
const PEAK_SHARPENING: f32 = 3.3;
const PEAK_WIDTH: [f32; 2] = [0.07, 0.09];
/// The fetch, in kilometres, where the day states none.
const FETCH_KM: f32 = 100.0;
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

/// The sea's spectrum a metre of wave number at `number` for a wind of `wind` metres a second over `fetch` metres: JONSWAP's, its height squared a metre of number.
fn spectrum(number: f32, wind: f32, fetch: f32) -> f32 {
    let wind = wind.max(0.001);
    let reach = GRAVITY * fetch / (wind * wind);
    let alpha = 0.076 * reach.powf(-0.22);
    let peak = 22.0 * GRAVITY / wind * reach.powf(-1.0 / 3.0);
    let omega = (GRAVITY * number.max(1e-6)).sqrt();
    let width = if omega <= peak {
        PEAK_WIDTH[0]
    } else {
        PEAK_WIDTH[1]
    };
    let off = (omega / peak - 1.0) / width;
    let sharpened = PEAK_SHARPENING.powf((-0.5 * off * off).exp());
    // The spectrum by frequency, alpha g^2 / omega^5, carried to wave number by d omega / d k = g / (2 omega).
    alpha * GRAVITY.powi(3) / (2.0 * omega.powi(6))
        * (-1.25 * (peak / omega).powi(4)).exp()
        * sharpened
}

/// The number a metre at the peak of the spectrum for a wind of `wind` metres a second over `fetch` metres.
fn peak_number(wind: f32, fetch: f32) -> f32 {
    let wind = wind.max(0.001);
    let peak = 22.0 * GRAVITY / wind * (GRAVITY * fetch / (wind * wind)).powf(-1.0 / 3.0);
    peak * peak / GRAVITY
}

/// The waves of a sea at Beaufort `beaufort` over a fetch of `fetch_km` of the kind `preset` is, `seconds` into the day: each wave's number a metre along x and y, its height and its phase now.
pub fn waves(
    preset: Option<&OceanPreset>,
    beaufort: f32,
    fetch_km: Option<f32>,
    seconds: f64,
) -> [[f32; 4]; WAVES] {
    let wind = (0.836 * beaufort.max(0.0).powf(1.5)).max(LEAST_WIND);
    let fetch = 1000.0 * fetch_km.filter(|f| *f > 0.0).unwrap_or(FETCH_KM);
    let peak = peak_number(wind, fetch);
    let along = (beaufort / LAST_BEAUFORT).clamp(0.0, 1.0);
    let each = SPREAD.len();
    // Each octave stands for the numbers from half to twice its own, its four waves for a quarter of the half circle about the wind each.
    let across = std::f32::consts::PI / each as f32;
    std::array::from_fn(|i| {
        let (octave, k) = (i / each, i % each);
        let Some((spread, stretch)) = SPREAD.get(k).zip(STRETCH.get(k)) else {
            return [0.0; 4];
        };
        let own = peak * OCTAVE_STEP.powi(octave as i32);
        let number = own / stretch;
        let strength = preset.map_or(1.0, |p| p.strength(octave, along));
        let spreading = 2.0 / std::f32::consts::PI * spread.to_radians().cos().powi(2);
        let angle = TRAVEL + (spread + OCTAVE_TURN * octave as f32).to_radians();
        // A sine of height a holds a^2 / 2 of the sea's height squared.
        let share = spectrum(number, wind, fetch) * spreading * 1.5 * own * across;
        let amplitude = (2.0 * share).sqrt() * strength;
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
    fn the_spectrum_peaks_where_jonswap_puts_it_and_a_longer_fetch_lengthens_it() {
        let (wind, fetch) = (8.0, 100_000.0);
        let peak = peak_number(wind, fetch);
        let sampled = (1..4000).map(|i| peak * i as f32 / 1000.0);
        let best = sampled
            .max_by(|a, b| spectrum(*a, wind, fetch).total_cmp(&spectrum(*b, wind, fetch)))
            .unwrap_or(0.0);
        // The frequency spectrum peaks at the peak; carried to wave number, over a falling d omega / d k, a little short of it.
        assert!(best < peak && best > 0.9 * peak, "{best} against {peak}");
        assert!(
            peak_number(wind, 2.0 * fetch) < peak,
            "a longer fetch, longer waves"
        );
        assert!(
            peak_number(2.0 * wind, fetch) < peak,
            "a stronger wind, longer waves"
        );
        // The sea's height squared, summed over the numbers: JONSWAP's own, alpha g^2 / (5 omega_p^4) times about 1.5 for the sharpened peak.
        let step = peak / 200.0;
        let m0: f32 = (1..20_000)
            .map(|i| spectrum(step * i as f32, wind, fetch) * step)
            .sum();
        let reach = GRAVITY * fetch / (wind * wind);
        let omega = (GRAVITY * peak).sqrt();
        let plain = 0.076 * reach.powf(-0.22) * GRAVITY * GRAVITY / (5.0 * omega.powi(4));
        assert!(m0 > 1.2 * plain && m0 < 2.0 * plain, "{m0} against {plain}");
    }

    #[test]
    fn the_waves_follow_the_wind_the_preset_and_the_clock() {
        let calm = waves(None, 0.0, None, 0.0);
        let fresh = waves(None, 5.0, None, 0.0);
        let height = |w: &[[f32; 4]; WAVES]| w.iter().map(|w| w[2]).sum::<f32>();
        assert!(
            height(&fresh) > 5.0 * height(&calm),
            "a fresh breeze raises the sea"
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
        assert!(
            fresh[8][2] < fresh[4][2] && fresh[4][2] < fresh[0][2],
            "shorter octaves are lower"
        );
        assert!(fresh[0][0] < 0.0, "the first wave travels west");
        let short = waves(None, 5.0, Some(1.0), 0.0);
        assert!(
            number(short[0]) > number(fresh[0]),
            "a short fetch, shorter waves"
        );
        let later = waves(None, 5.0, None, 3600.0 * 15.0);
        assert!(later.iter().zip(&fresh).all(|(a, b)| a[..3] == b[..3]));
        assert!(later
            .iter()
            .all(|w| (0.0..std::f32::consts::TAU).contains(&w[3])));
        assert_ne!(later[0][3], fresh[0][3], "the clock moves the phase");
        let still = OceanPreset {
            octaves: vec![vec![[0.0, 0.0], [1.0, 0.0]]],
            ..OceanPreset::default()
        };
        let stilled = waves(Some(&still), 5.0, None, 0.0);
        assert!(
            stilled[..4].iter().all(|w| w[2] == 0.0),
            "a preset's octave of no strength"
        );
        assert_eq!(stilled[4][2], fresh[4][2], "an octave it does not state");
    }
}
