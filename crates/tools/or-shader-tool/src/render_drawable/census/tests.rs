use or_renderer::game_shaders::frame::Rendered;
use or_renderer::game_shaders::mesh::MeshRender;

use super::*;

fn drawn(effects: &[&str], skipped: &[(&str, MeshError)], pixel: u8) -> Drawn {
    let left = skipped.len();
    Drawn {
        effects: effects.iter().map(|e| e.to_string()).collect(),
        vertices: 3,
        render: MeshRender {
            rendered: Rendered {
                size: 2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                bytes: vec![pixel; 16],
            },
            drawn: effects.len() - left,
            skipped: skipped
                .iter()
                .map(|(effect, error)| (effect.to_string(), error.clone()))
                .collect(),
        },
    }
}

#[test]
fn a_drawable_that_drew_and_touched_a_pixel_is_shown() {
    let mut census = Census::default();
    census.add("a.ydr", Ok(&drawn(&["default", "default"], &[], 9)));
    assert_eq!(
        (
            census.tried,
            census.shown,
            census.blank,
            census.nothing_drawn
        ),
        (1, 1, 0, 0)
    );
    assert_eq!((census.surfaces, census.surfaces_drawn), (2, 2));
    assert_eq!(
        census.effects["default"],
        EffectTally {
            surfaces: 2,
            drawn: 2
        }
    );
}

#[test]
fn a_drawable_that_drew_and_touched_nothing_is_blank_and_one_that_drew_no_surface_is_neither() {
    let mut census = Census::default();
    census.add("a.ydr", Ok(&drawn(&["default"], &[], 0)));
    census.add(
        "a.ydr",
        Ok(&drawn(
            &["nothing"],
            &[("nothing", MeshError::NoEffect("nothing".into()))],
            0,
        )),
    );
    assert_eq!(
        (census.shown, census.blank, census.nothing_drawn),
        (0, 1, 1)
    );
}

#[test]
fn a_surface_left_out_is_counted_once_for_its_cause_and_once_against_its_effect() {
    let mut census = Census::default();
    census.add(
        "a.ydr",
        Ok(&drawn(
            &["default", "glass", "glass"],
            &[
                ("glass", MeshError::NoEffect("glass".into())),
                (
                    "glass",
                    MeshError::Pipeline("it failed at 12 and 345".into()),
                ),
            ],
            5,
        )),
    );
    assert_eq!(census.surfaces_drawn, 1);
    assert_eq!(census.left_out["the pack has no such effect"], 1);
    assert_eq!(census.left_out["it failed at N and N"], 1);
    assert_eq!(
        census.effects["glass"],
        EffectTally {
            surfaces: 2,
            drawn: 0
        }
    );
    assert_eq!(
        census.effects["default"],
        EffectTally {
            surfaces: 1,
            drawn: 1
        }
    );
    // Some of it drew, so the drawable is shown
    assert_eq!(census.shown, 1);
}

#[test]
fn a_drawable_that_would_not_read_is_counted_by_cause_with_its_numbers_folded() {
    let mut census = Census::default();
    census.add("a.ydr", Err("cannot read 12 bytes"));
    census.add("a.ydr", Err("cannot read 99 bytes"));
    assert_eq!((census.tried, census.shown), (2, 0));
    assert_eq!(census.unreadable["cannot read N bytes"], 2);
}

#[test]
fn the_report_gives_each_figure_with_what_it_is_of() {
    let mut census = Census {
        available: 40,
        ..Census::default()
    };
    census.add("a.ydr", Ok(&drawn(&["default"], &[], 1)));
    census.add(
        "a.ydr",
        Ok(&drawn(&["x"], &[("x", MeshError::NoPass("x".into()))], 0)),
    );
    census.add("a.ydr", Err("boom"));
    let report = census.report();
    assert!(
        report.contains("3 drawables of 40: 1 shown (33.3%)"),
        "{report}"
    );
    assert!(
        report.contains("surfaces: 1 drawn of 2 (50.0%)"),
        "{report}"
    );
    assert!(
        report.contains("the effect has no pass this draws"),
        "{report}"
    );
    assert!(report.contains("boom"), "{report}");
    assert_eq!(census.to_json()["shown"], 1);
}

#[test]
fn a_sample_is_evenly_spaced_from_the_first_and_all_of_a_short_list() {
    let paths: Vec<String> = (0..10).map(|i| i.to_string()).collect();
    let chosen: Vec<&str> = sample(&paths, 4).into_iter().map(String::as_str).collect();
    assert_eq!(chosen, ["0", "2", "5", "7"]);
    assert_eq!(sample(&paths, 10).len(), 10);
    assert_eq!(sample(&paths, 25).len(), 10);
    assert_eq!(sample(&paths, 0).len(), 10);
    assert!(sample(&[], 3).is_empty());
}

#[test]
fn the_first_few_paths_of_each_kind_that_did_not_show_are_kept_and_named_in_the_report() {
    let mut census = Census::default();
    for i in 0..10 {
        census.add(&format!("blank{i}.ydr"), Ok(&drawn(&["default"], &[], 0)));
    }
    census.add("bad.ydr", Err("boom"));
    census.add("fine.ydr", Ok(&drawn(&["default"], &[], 3)));
    assert_eq!(census.examples["drawn blank"].len(), EXAMPLES);
    assert_eq!(census.examples["drawn blank"][0], "blank0.ydr");
    assert_eq!(census.examples["unreadable"], ["bad.ydr"]);
    assert!(!census.examples.contains_key("no surface drawn"));
    let report = census.report();
    assert!(
        report.contains("drawn blank, for example:\n  blank0.ydr"),
        "{report}"
    );
    assert!(!report.contains("fine.ydr"), "{report}");
}

#[test]
fn a_surface_left_out_names_its_drawable_and_its_effect_under_the_kind_of_its_cause() {
    let mut census = Census::default();
    census.add(
        "a.ydr",
        Ok(&drawn(
            &["", "water_shallow"],
            &[
                ("", MeshError::NoEffect(String::new())),
                ("water_shallow", MeshError::NoPass("water_shallow".into())),
            ],
            0,
        )),
    );
    assert_eq!(
        census.examples["a surface left out, for want of its effect"],
        ["a.ydr (no material)"]
    );
    assert_eq!(
        census.examples["a surface left out, for want of a pass"],
        ["a.ydr (water_shallow)"]
    );
}

#[test]
fn a_blank_drawable_is_counted_under_the_effects_it_was_drawn_with_once_each_and_sorted() {
    let mut census = Census::default();
    census.add(
        "a.ydd",
        Ok(&drawn(&["ped_hair_spiked", "ped", "ped"], &[], 0)),
    );
    census.add("b.ydd", Ok(&drawn(&["ped", "ped_hair_spiked"], &[], 0)));
    census.add("c.ydd", Ok(&drawn(&[""], &[], 0)));
    census.add("d.ydd", Ok(&drawn(&["ped"], &[], 7)));
    assert_eq!(
        census.blank_by_effects,
        BTreeMap::from([
            ("no material".to_string(), 1),
            ("ped, ped_hair_spiked".to_string(), 2),
        ])
    );
    assert!(census
        .report()
        .contains("what the blank drawables were drawn with:\n"));
}
