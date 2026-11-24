//! The `compare` step: how far a shot is from a reference image, as two numbers and an image of
//! the difference saved beside the shot.
//!
//! ⚠ A reference that is not there fails nothing: the shot is then a candidate for it, and the
//! step never writes one, so no run is ever measured against a frame it made itself.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};

use super::script::{file_stem, split_word};

/// A pixel differs once a channel of it is more than this many of 255 levels from the reference.
const PAST: u8 = 16;

/// A shot to measure against a reference.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Compare {
    /// The name an earlier `shot` saved the frame under.
    pub shot: String,
    pub reference: PathBuf,
    /// The largest mean difference that passes, in levels of 255.
    pub tolerance: f32,
}

/// The comparison the words after `compare` name: a shot, a reference image, a tolerance.
pub(super) fn parse(rest: &str) -> Result<Compare, String> {
    let (shot, after) = split_word(rest);
    // The reference is a path, which may hold spaces: the tolerance is the last word.
    let (reference, tolerance) = after
        .rsplit_once(char::is_whitespace)
        .ok_or("needs a shot's name, a reference image and a tolerance")?;
    let reference = reference.trim_end();
    if reference.is_empty() {
        return Err("needs a reference image".into());
    }
    let tolerance = (tolerance.parse::<f32>().ok())
        .filter(|t| (0.0..=255.0).contains(t))
        .ok_or_else(|| {
            format!("`{tolerance}` is not a tolerance: a mean difference from 0 to 255")
        })?;
    Ok(Compare {
        shot: file_stem(shot)?,
        reference: PathBuf::from(reference),
        tolerance,
    })
}

/// How far a shot is from its reference.
#[derive(Debug)]
pub(super) struct Difference {
    /// The mean absolute difference of the red, green and blue of every pixel, in levels of 255.
    pub mean: f64,
    /// The share of the pixels with a channel more than [`PAST`] levels away, from 0 to 1.
    pub past: f64,
    /// Each pixel's absolute difference, channel by channel, opaque.
    pub image: RgbaImage,
}

/// Measure `shot` against `reference`, which must be of its size; opacity is not compared.
pub(super) fn difference(shot: &RgbaImage, reference: &RgbaImage) -> Result<Difference, String> {
    if shot.dimensions() != reference.dimensions() {
        return Err(format!(
            "the shot is {}×{} and the reference {}×{}: a reference is of the frame's own size",
            shot.width(),
            shot.height(),
            reference.width(),
            reference.height()
        ));
    }
    let mut image = RgbaImage::new(shot.width(), shot.height());
    let (mut sum, mut past) = (0u64, 0u64);
    for ((ours, theirs), out) in shot
        .pixels()
        .zip(reference.pixels())
        .zip(image.pixels_mut())
    {
        let apart = [0, 1, 2].map(|channel| ours.0[channel].abs_diff(theirs.0[channel]));
        sum += apart.iter().map(|d| u64::from(*d)).sum::<u64>();
        past += u64::from(apart.iter().any(|d| *d > PAST));
        *out = Rgba([apart[0], apart[1], apart[2], u8::MAX]);
    }
    let pixels = (u64::from(shot.width()) * u64::from(shot.height())).max(1) as f64;
    Ok(Difference {
        mean: sum as f64 / (pixels * 3.0),
        past: past as f64 / pixels,
        image,
    })
}

/// Measure the shot `compare` names in `out` against its reference and save their difference
/// beside it; an error once the mean is over the tolerance, and nothing to do with no reference.
pub(super) fn run(compare: &Compare, out: &Path) -> Result<(), String> {
    let shot = out.join(format!("{}.png", compare.shot));
    if !shot.is_file() {
        return Err(format!(
            "no shot `{}` was taken before its comparison",
            compare.shot
        ));
    }
    if !compare.reference.is_file() {
        println!(
            "candidate  {}  (no reference at {})",
            shot.display(),
            compare.reference.display()
        );
        return Ok(());
    }
    let open = |path: &Path| {
        image::open(path)
            .map(|picture| picture.to_rgba8())
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let found = difference(&open(shot.as_path())?, &open(compare.reference.as_path())?)
        .map_err(|why| format!("{}: {why}", compare.shot))?;
    let beside = out.join(format!("{}.diff.png", compare.shot));
    (found.image.save(&beside)).map_err(|e| format!("cannot write {}: {e}", beside.display()))?;
    let measured = format!(
        "{}: mean difference {:.3} of 255, {:.2} % of pixels past {PAST}",
        compare.shot,
        found.mean,
        found.past * 100.0
    );
    println!("compare  {measured}  ({})", beside.display());
    if found.mean > f64::from(compare.tolerance) {
        return Err(format!(
            "{measured}; over the tolerance {}",
            compare.tolerance
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: [u8; 4] = [100, 100, 100, 255];

    fn flat(width: u32, height: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(colour))
    }

    #[test]
    fn a_frame_is_no_distance_from_itself() {
        let frame = flat(3, 2, GREY);
        let found = difference(&frame, &frame).expect("one size");
        assert_eq!((found.mean, found.past), (0.0, 0.0));
        assert_eq!(found.image, flat(3, 2, [0, 0, 0, 255]));
    }

    #[test]
    fn the_mean_counts_every_channel_and_the_share_counts_the_pixels_past_the_threshold() {
        let reference = flat(2, 2, GREY);
        let mut shot = reference.clone();
        let far = PAST + 14;
        shot.put_pixel(0, 0, Rgba([GREY[0] + far, GREY[1], GREY[2], 255]));
        // A channel at the threshold itself is not past it.
        shot.put_pixel(1, 0, Rgba([GREY[0], GREY[1] - PAST, GREY[2], 255]));
        let found = difference(&shot, &reference).expect("one size");
        let (pixels, channels) = (4.0, 3.0);
        assert_eq!(
            found.mean,
            f64::from(far + PAST) / (pixels * channels),
            "the mean is over every channel of every pixel"
        );
        assert_eq!(found.past, 1.0 / pixels);
        assert_eq!(found.image.get_pixel(0, 0), &Rgba([far, 0, 0, 255]));
        assert_eq!(found.image.get_pixel(1, 0), &Rgba([0, PAST, 0, 255]));
        assert_eq!(found.image.get_pixel(1, 1), &Rgba([0, 0, 0, 255]));
        let mirrored = difference(&reference, &shot).expect("one size");
        assert_eq!(
            (mirrored.mean, mirrored.past),
            (found.mean, found.past),
            "which of the two is the shot changes nothing"
        );
    }

    #[test]
    fn opacity_is_not_compared() {
        let clear = flat(2, 1, [GREY[0], GREY[1], GREY[2], 0]);
        let found = difference(&flat(2, 1, GREY), &clear).expect("one size");
        assert_eq!((found.mean, found.past), (0.0, 0.0));
    }

    #[test]
    fn a_reference_of_another_size_is_refused_by_both_sizes() {
        let why = difference(&flat(4, 2, GREY), &flat(2, 4, GREY)).expect_err("two sizes");
        assert!(why.contains("4×2") && why.contains("2×4"), "{why}");
    }

    #[test]
    fn the_step_names_a_shot_a_reference_and_a_tolerance() {
        assert_eq!(
            parse("01-city C:\\a b\\ref 1.png 2.5"),
            Ok(Compare {
                shot: "01-city".into(),
                reference: PathBuf::from("C:\\a b\\ref 1.png"),
                tolerance: 2.5,
            })
        );
        assert_eq!(parse("a b.png 0").map(|c| c.tolerance), Ok(0.0));
        for bad in [
            "",
            "a",
            "a b.png",
            "a b.png much",
            "a b.png -1",
            "a b.png 256",
            "a b.png NaN",
            "../a b.png 1",
        ] {
            assert!(parse(bad).is_err(), "`compare {bad}` should be refused");
        }
    }

    #[test]
    fn a_shot_over_its_tolerance_fails_and_one_with_no_reference_is_a_candidate() {
        let folder = tempfile::tempdir().expect("a scratch folder");
        let reference = folder.path().join("reference.png");
        let step = |tolerance| Compare {
            shot: "scene".into(),
            reference: reference.clone(),
            tolerance,
        };
        let no_shot = run(&step(255.0), folder.path()).expect_err("no shot yet");
        assert!(no_shot.contains("scene"), "{no_shot}");

        let off = 10;
        let shot = flat(8, 8, [GREY[0] + off, GREY[1], GREY[2], 255]);
        shot.save(folder.path().join("scene.png")).expect("a shot");
        assert_eq!(
            run(&step(0.0), folder.path()),
            Ok(()),
            "with no reference the shot is a candidate and nothing fails"
        );
        assert!(!reference.exists(), "the step never writes a reference");
        assert!(!folder.path().join("scene.diff.png").exists());

        flat(8, 8, GREY).save(&reference).expect("a reference");
        // One channel of three is `off` away in every pixel.
        let mean = f32::from(off) / 3.0;
        assert_eq!(run(&step(mean + 0.01), folder.path()), Ok(()));
        let over = run(&step(mean - 0.01), folder.path()).expect_err("over the tolerance");
        assert!(
            over.contains("scene") && over.contains("tolerance"),
            "{over}"
        );
        let saved = image::open(folder.path().join("scene.diff.png")).expect("the difference");
        assert_eq!(saved.to_rgba8(), flat(8, 8, [off, 0, 0, 255]));
    }
}
