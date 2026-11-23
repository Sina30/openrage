//! Cropping a textures source to a window: every level of the cropped texture is the part of the same level of the source that the window covers, no block is encoded again, and what cannot be cropped is refused by name or left.

mod common;

use common::*;
use or_interchange::doctor::uv::{Axis, Window};
use or_io::codec::texture::GrcTextureFormat;
use or_source::crop::crop;

const DXT1: GrcTextureFormat = GrcTextureFormat::DXT1;

fn window(x: (u32, u32), y: (u32, u32)) -> Window {
    Window {
        x: Axis {
            index: x.0,
            power: x.1,
        },
        y: Axis {
            index: y.0,
            power: y.1,
        },
    }
}

fn nothing_stops() -> bool {
    false
}

#[test]
fn each_level_of_a_cropped_texture_is_the_part_of_the_same_level_the_window_covers() {
    for format in [DXT1, GrcTextureFormat::DXT5, GrcTextureFormat::A8R8G8B8] {
        let scratch = tempfile::tempdir().unwrap();
        let ytd = legacy_dictionary(&[Spec::new("big", format, ramp(256, 256, 3))]);
        let folder = extract(scratch.path(), "pack", &ytd);
        let w = window((1, 2), (0, 1));

        let report = crop(&folder, &w, &nothing_stops).expect("a power of two texture is cropped");
        assert_eq!(report.cropped.len(), 1, "{report:?}");
        let done = &report.cropped[0];
        assert_eq!((done.was.width, done.was.height), (256, 256));
        // A quarter of the width and half the height, with the mips shipped textures have: down to four texels on the shorter side.
        assert_eq!(
            (done.now.width, done.now.height, done.now.mips),
            (64, 128, 5),
            "{format:?}"
        );

        let after = compile(&folder);
        for level_no in 0..5u32 {
            let (src, cut) = (level(&ytd, "big", level_no), level(&after, "big", level_no));
            let (x0, y0) = (64u32 >> level_no, 0u32);
            assert_eq!(
                (cut.width, cut.height),
                (64 >> level_no, 128 >> level_no),
                "{format:?} level {level_no}"
            );
            for y in 0..cut.height {
                for x in 0..cut.width {
                    let (a, b) = (
                        (y * cut.width + x) as usize * 4,
                        ((y0 + y) * src.width + x0 + x) as usize * 4,
                    );
                    assert_eq!(
                        cut.data[a..a + 4],
                        src.data[b..b + 4],
                        "{format:?} level {level_no} at {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_texture_of_four_texels_or_fewer_on_a_side_is_left_and_listed() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = legacy_dictionary(&[
        Spec::new("big", DXT1, ramp(128, 128, 1)),
        Spec::new("speck", DXT1, flat(4, 4, [[10, 20, 30, 255]; 4])),
    ]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let speck_before = std::fs::read(folder.join("textures/speck.dds")).unwrap();

    let report = crop(&folder, &window((0, 1), (1, 1)), &nothing_stops).unwrap();
    assert_eq!(report.cropped.len(), 1);
    assert_eq!(report.left.len(), 1);
    assert_eq!(report.left[0].texture, "speck");
    assert_eq!(
        std::fs::read(folder.join("textures/speck.dds")).unwrap(),
        speck_before,
        "the speck is as it was"
    );
}

#[test]
fn a_window_too_small_for_a_texture_refuses_the_whole_source_and_writes_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = legacy_dictionary(&[
        Spec::new("big", DXT1, ramp(256, 256, 1)),
        Spec::new("small", DXT1, ramp(32, 32, 2)),
    ]);
    let folder = extract(scratch.path(), "pack", &ytd);
    let (big, small) = (
        std::fs::read(folder.join("textures/big.dds")).unwrap(),
        std::fs::read(folder.join("textures/small.dds")).unwrap(),
    );

    // An eighth by an eighth is 4 pixels of the 32, which is the least a crop keeps, and a sixteenth is not.
    assert!(crop(&folder, &window((3, 3), (3, 3)), &nothing_stops).is_ok());
    let again = extract(scratch.path(), "again", &ytd);
    let refused = crop(&again, &window((3, 4), (3, 4)), &nothing_stops)
        .expect_err("2 pixels of 32 is too few");
    assert!(
        refused
            .iter()
            .any(|p| p.texture.as_deref() == Some("small")),
        "{refused:?}"
    );
    assert_eq!(
        std::fs::read(again.join("textures/big.dds")).unwrap(),
        big,
        "nothing was written"
    );
    assert_eq!(
        std::fs::read(again.join("textures/small.dds")).unwrap(),
        small
    );
}

#[test]
fn a_texture_made_from_a_picture_is_refused_by_name() {
    let scratch = tempfile::tempdir().unwrap();
    let ytd = legacy_dictionary(&[Spec::new("big", DXT1, ramp(128, 128, 1))]);
    let folder = extract(scratch.path(), "pack", &ytd);
    save_image(&folder.join("textures/big.png"), &ramp(128, 128, 5));
    edit_rules(&folder, |rules| {
        entry(rules, "big")["source"] = "big.png".into();
    });
    let refused = crop(&folder, &window((0, 1), (0, 1)), &nothing_stops)
        .expect_err("a picture is not cropped here");
    assert!(
        refused.iter().any(|p| p.message.contains("picture")),
        "{refused:?}"
    );
}
