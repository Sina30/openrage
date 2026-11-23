//! Texture budgeting: a size and a format for each texture of a source that fit a memory target, with the PSNR of every choice,
//! as a dry run that changes nothing until its plan is accepted.

mod common;

use common::*;
use or_io::authoring::rgba_psnr;
use or_io::codec::texture::{resize_rgba, GrcTextureFormat, MipFilter, RgbaImage};
use or_source::budget::{self, Options};
use or_source::game_import::tree_digest;

const DXT5: GrcTextureFormat = GrcTextureFormat::DXT5;
const SIDE: u32 = 64;

/// A smooth sky: it loses almost nothing when it is made smaller. One texel is clear, so that the texture needs its alpha.
fn sky() -> RgbaImage {
    let mut image = RgbaImage::new(SIDE, SIDE);
    for y in 0..SIDE {
        for x in 0..SIDE {
            image.set_pixel(
                x,
                y,
                [
                    (60 + y * 100 / SIDE) as u8,
                    120,
                    (200 - x * 60 / SIDE) as u8,
                    255,
                ],
            );
        }
    }
    image.set_pixel(0, 0, [0, 0, 0, 0]);
    image
}

/// Fine noise: it loses a great deal when it is made smaller.
fn noise(salt: u32) -> RgbaImage {
    let mut image = RgbaImage::new(SIDE, SIDE);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let h = (x.wrapping_mul(73_856_093)
                ^ y.wrapping_mul(19_349_663)
                ^ salt.wrapping_mul(83_492_791))
            .wrapping_mul(2_654_435_761);
            image.set_pixel(
                x,
                y,
                [(h >> 8) as u8, (h >> 16) as u8, (h >> 24) as u8, 255],
            );
        }
    }
    image.set_pixel(0, 0, [0, 0, 0, 0]);
    image
}

/// The sum of the bytes of every mip of every texture of a compiled dictionary.
fn stored(bytes: &[u8]) -> u64 {
    legacy(bytes)
        .entries
        .iter()
        .flatten()
        .map(|t| mips(&**t).iter().map(|m| m.len() as u64).sum::<u64>())
        .sum()
}

const NAMES: [&str; 8] = [
    "sky_a", "sky_b", "grain_1", "grain_2", "grain_3", "grain_4", "grain_5", "grain_6",
];

fn source(scratch: &std::path::Path) -> (std::path::PathBuf, Vec<u8>) {
    let specs: Vec<Spec> = NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| Spec::new(name, DXT5, if i < 2 { sky() } else { noise(i as u32) }))
        .collect();
    let original = legacy_dictionary(&specs);
    (extract(scratch, "pack", &original), original)
}

#[test]
fn a_dry_run_fits_the_target_with_the_least_lost_and_changes_no_byte() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, original) = source(scratch.path());
    let before = tree_digest(&folder).unwrap();
    let total = stored(&original);

    let measured = budget::measure(&folder, &Options::default(), &|| false).unwrap();
    assert_eq!(measured.ladders.len(), 8, "{:?}", measured.left);
    assert_eq!(
        measured.bytes(),
        total,
        "the ladders start from what is stored"
    );

    // A target that the two skies can meet on their own: four fifths of what is stored.
    let target = total * 4 / 5;
    let chosen = budget::choose(&[&measured], target);
    assert!(chosen.met, "{chosen:?}");
    assert!(
        chosen.after <= target,
        "{} is over the target {target}",
        chosen.after
    );
    assert!(
        chosen.slack() < 0.10,
        "the plan stops within a tenth of the target below it: {chosen:?}"
    );
    assert_eq!(chosen.before, total);
    assert_eq!(
        tree_digest(&folder).unwrap(),
        before,
        "a dry run changes no byte"
    );

    // The smooth pictures give up what the noise would not: the grain is not touched while the skies have a step left.
    for (ladder, pick) in measured.ladders.iter().zip(&chosen.picks[0]) {
        if ladder.texture.starts_with("grain") {
            assert_eq!(*pick, 0, "{} was cut: {:?}", ladder.texture, chosen.picks);
        } else {
            assert!(*pick > 0, "{} was not cut", ladder.texture);
        }
    }
}

#[test]
fn the_psnr_of_a_choice_is_the_one_a_smaller_picture_measures_against_the_original() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, original) = source(scratch.path());
    let measured = budget::measure(&folder, &Options::default(), &|| false).unwrap();
    let half = |name: &str| {
        measured
            .ladders
            .iter()
            .find(|l| l.texture == name)
            .unwrap()
            .rungs
            .iter()
            .find(|r| r.width == SIDE / 2 && r.format == "DXT5")
            .unwrap_or_else(|| panic!("{name} has a half-size rung"))
            .clone()
    };
    for name in ["sky_a", "grain_3"] {
        // The oracle: the mip the file stores at half size, brought back to the full size and held to the full one.
        let top = level(&original, name, 0);
        let small = level(&original, name, 1);
        let up = resize_rgba(
            &small.data,
            (SIDE / 2, SIDE / 2),
            (SIDE, SIDE),
            MipFilter::Lanczos3,
            false,
        )
        .unwrap();
        let back = RgbaImage {
            width: SIDE,
            height: SIDE,
            data: up,
            is_srgb: false,
        };
        let want = rgba_psnr(&top, &back).unwrap().overall;
        let got = half(name).psnr.expect("a halved picture is not exact");
        assert!(
            (got - want).abs() < 0.01,
            "{name}: {got} dB against the oracle's {want}"
        );
    }
    // The sky halves far better than grain: that is why the plan cuts it first.
    assert!(half("sky_a").psnr.unwrap() > half("grain_3").psnr.unwrap() + 10.0);
}

#[test]
fn an_opaque_texture_stored_with_alpha_gives_up_half_its_bytes_for_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let mut opaque = noise(9);
    opaque.set_pixel(0, 0, [1, 2, 3, 255]);
    let original = legacy_dictionary(&[Spec::new("solid", DXT5, opaque)]);
    let folder = extract(scratch.path(), "pack", &original);
    let measured = budget::measure(&folder, &Options::default(), &|| false).unwrap();
    let ladder = &measured.ladders[0];
    let one = ladder
        .rungs
        .iter()
        .find(|r| r.format == "DXT1" && r.width == SIDE)
        .expect("BC1 at the same size is a rung of an opaque BC3");
    assert_eq!(one.psnr, None, "the colour blocks of the two are the same");
    assert!(one.bytes * 2 <= ladder.rungs[0].bytes + 64);
    assert_eq!(ladder.rungs[1], *one, "and it is the first thing given up");
}

#[test]
fn an_accepted_plan_is_written_as_the_dry_run_said_and_meets_the_target() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, original) = source(scratch.path());
    let total = stored(&original);
    let measured = budget::measure(&folder, &Options::default(), &|| false).unwrap();
    let target = total * 3 / 5;
    let chosen = budget::choose(&[&measured], target);
    assert!(chosen.met, "{chosen:?}");

    let done = budget::apply(
        &folder,
        &measured,
        &chosen.picks[0],
        &Options::default(),
        &|| false,
    )
    .unwrap();
    assert!(!done.is_empty());
    let bytes = compile(&folder);
    assert_eq!(
        stored(&bytes),
        chosen.after,
        "the plan said what it would store, and it stored it"
    );
    assert!(stored(&bytes) <= target);
    assert!(chosen.slack() < 0.10, "{chosen:?}");
    let dictionary = legacy(&bytes);
    for (ladder, pick) in measured.ladders.iter().zip(&chosen.picks[0]) {
        let rung = &ladder.rungs[*pick];
        let texture = dictionary.get(&ladder.texture).unwrap();
        assert_eq!(u32::from(texture.width), rung.width, "{}", ladder.texture);
        assert_eq!(
            texture.format,
            GrcTextureFormat::from_name(&rung.format)
                .unwrap()
                .to_raw_format()
        );
        if *pick == 0 {
            assert_eq!(
                mips(texture),
                mips(legacy(&original).get(&ladder.texture).unwrap()),
                "{} gave nothing up, so it is the file it was",
                ladder.texture
            );
        }
    }
}

#[test]
fn a_target_below_what_can_be_given_up_is_said_not_met_and_a_stop_leaves_the_source_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, _) = source(scratch.path());
    let measured = budget::measure(&folder, &Options::default(), &|| false).unwrap();
    let chosen = budget::choose(&[&measured], 1);
    assert!(!chosen.met);
    assert!(
        chosen.after < chosen.before,
        "it gives up everything it can before it says so"
    );
    for (ladder, pick) in measured.ladders.iter().zip(&chosen.picks[0]) {
        assert_eq!(*pick, ladder.rungs.len() - 1, "{}", ladder.texture);
    }

    let digest = tree_digest(&folder).unwrap();
    let all = vec![1usize; measured.ladders.len()];
    assert!(budget::apply(&folder, &measured, &all, &Options::default(), &|| true).is_err());
    assert_eq!(tree_digest(&folder).unwrap(), digest);
    assert!(budget::measure(&folder, &Options::default(), &|| true).is_err());
}

#[test]
fn a_picture_made_texture_is_left_with_why_and_a_texture_changed_since_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let (folder, _) = source(scratch.path());
    save_image(&folder.join("textures/drawn.png"), &sky());
    let measured = budget::measure(&folder, &Options::default(), &|| false).unwrap();
    assert_eq!(measured.ladders.len(), 8);
    assert!(measured
        .left
        .iter()
        .any(|l| l.texture == "drawn" && l.why.contains("picture")));

    // The file under a plan is swapped for another of another size: the plan does not carry on over it, and no texture is left cut.
    let swap = |name: &str, picture: RgbaImage| {
        let other = legacy_dictionary(&[Spec::new("sky_a", DXT5, picture)]);
        std::fs::copy(
            extract(scratch.path(), name, &other).join("textures/sky_a.dds"),
            folder.join("textures/sky_a.dds"),
        )
        .unwrap();
    };
    swap("smaller", ramp(SIDE / 2, SIDE / 2, 3));
    let swapped = tree_digest(&folder).unwrap();
    let chosen = budget::choose(&[&measured], 1);
    let refused = budget::apply(
        &folder,
        &measured,
        &chosen.picks[0],
        &Options::default(),
        &|| false,
    )
    .unwrap_err();
    assert!(
        refused.iter().any(|p| {
            let said = p.to_string();
            said.contains("sky_a") && said.contains("not as it was when it was measured")
        }),
        "{refused:?}"
    );
    assert_eq!(
        tree_digest(&folder).unwrap(),
        swapped,
        "a refusal leaves every texture as it found it"
    );

    // The same size and format: nothing says it changed, and it is cut as it was measured.
    swap("same", sky());
    let done = budget::apply(
        &folder,
        &measured,
        &chosen.picks[0],
        &Options::default(),
        &|| false,
    );
    assert!(done.is_ok(), "{done:?}");
}
