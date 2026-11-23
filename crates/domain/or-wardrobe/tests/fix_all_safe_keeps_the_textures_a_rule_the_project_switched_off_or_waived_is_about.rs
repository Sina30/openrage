//! Fix all safe holds to the project's own rules: a texture that a waiver sets aside, or a rule the project switched off, is kept as it
//! is by the Textures step, and the step says which rule kept it. The rest of what the step does is as it was.

mod common;

use std::path::PathBuf;

use common::{garment, snapshot, ytd_of, JBIB};
use or_io::codec::texture::GrcTextureFormat;
use or_source::game_import::{decompile, Facts as GameFacts};
use or_source::sample;
use or_wardrobe::edit::{Revision, Watch};
use or_wardrobe::fix::{all_safe, Report, Step, StepReport};
use or_wardrobe::{Ped, Waiver, Wardrobe};

type Texture = (&'static str, GrcTextureFormat, u32, u32, u32);

const ODD: Texture = ("odd", GrcTextureFormat::DXT1, 100, 60, 1);
const ODD_SOURCE: &str = "textures/odd.textures.src";

struct Pack {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

/// A pack of one jacket for each texture, each jacket pinned at a place of its own and naming a textures source made of its texture;
/// `tune` changes the pack's description before it is written.
fn a_pack(textures: &[Texture], tune: impl FnOnce(&mut Wardrobe)) -> Pack {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clothing");
    std::fs::create_dir_all(path.join("models")).unwrap();
    decompile(
        &sample::drawable_dictionary("jbib_000_u", 6, 0.0),
        "jbib_000_u.ydd",
        &path.join("models"),
        &GameFacts::default(),
    )
    .unwrap();
    std::fs::create_dir_all(path.join("textures")).unwrap();
    let mut w = Wardrobe::new("demo");
    for (n, texture) in textures.iter().enumerate() {
        decompile(
            &ytd_of(&[*texture]),
            &format!("{}.ytd", texture.0),
            &path.join("textures"),
            &GameFacts::default(),
        )
        .unwrap();
        w.garments.push(garment(
            Ped::Male,
            JBIB,
            Some(n as u32),
            "models/jbib_000_u.drawables.src",
            &[&format!("textures/{}.textures.src", texture.0)],
        ));
    }
    tune(&mut w);
    w.write(&path).unwrap();
    Pack { _dir: dir, path }
}

fn waive(w: &mut Wardrobe, code: &str, garment: Option<usize>, reason: &str) {
    let garment = garment.map(|n| w.garments[n].id.clone());
    w.waivers.push(Waiver {
        code: code.into(),
        garment,
        reason: reason.into(),
    });
}

fn switch_off(w: &mut Wardrobe, code: &str) {
    w.rules.set(code, |s| s.on = Some(false));
}

fn fix(pack: &Pack) -> (Report, Option<Revision>) {
    all_safe(&pack.path, &Watch::default()).expect("the pipeline runs")
}

fn textures_of(report: &Report) -> &StepReport {
    report
        .steps
        .iter()
        .find(|s| s.step == Step::Textures)
        .unwrap()
}

/// The source each garment names for its first variation.
fn named(pack: &Pack) -> Vec<String> {
    let w = Wardrobe::read(&pack.path).unwrap();
    (w.garments.iter())
        .map(|g| g.variations[0].texture.clone().unwrap())
        .collect()
}

#[test]
fn without_a_waiver_the_odd_texture_is_resized_and_given_its_mips() {
    let pack = a_pack(&[ODD], |_| {});

    let (report, revision) = fix(&pack);

    assert_eq!(named(&pack), ["textures/odd-2.textures.src"]);
    assert!(revision.is_some());
    let said = textures_of(&report).changed.join("\n");
    assert!(said.contains("becomes 128 by 64"), "{said}");
}

#[test]
fn a_waived_rule_keeps_what_it_names_and_the_others_still_apply() {
    // The power of two is waived: the mips are added and the size is kept.
    let pack = a_pack(&[ODD], |w| {
        waive(
            w,
            "texture-not-power-of-two",
            None,
            "an atlas that stays 100 by 60",
        );
    });
    let (report, _) = fix(&pack);
    let said = textures_of(&report).changed.join("\n");
    assert!(said.contains("becomes 100 by 60"), "{said}");
    assert!(!said.contains("128 by 64"), "{said}");

    // The mips are waived: the size goes to a power of two and the single mip is kept.
    let pack = a_pack(&[ODD], |w| {
        waive(w, "texture-mips", None, "a flat decal");
    });
    let (report, _) = fix(&pack);
    let said = textures_of(&report).changed.join("\n");
    assert!(said.contains("becomes 128 by 64 DXT1 with 1 mip"), "{said}");
}

#[test]
fn a_texture_both_rules_are_waived_for_is_not_copied_and_the_step_says_which_rules_kept_it() {
    let pack = a_pack(&[ODD], |w| {
        waive(
            w,
            "texture-not-power-of-two",
            None,
            "an atlas that stays 100 by 60",
        );
        waive(w, "texture-mips", Some(0), "a flat decal");
    });
    let before = snapshot(&pack.path);

    let (report, revision) = fix(&pack);

    assert!(revision.is_none(), "nothing changed, so no step of Undo");
    assert!(!report.changed(), "{report:?}");
    assert_eq!(named(&pack), [ODD_SOURCE]);
    assert_eq!(snapshot(&pack.path), before, "no byte and no folder");
    let step = textures_of(&report);
    assert!(step.changed.is_empty(), "{step:?}");
    let kept = step
        .refused
        .iter()
        .find(|r| r.file == ODD_SOURCE)
        .unwrap_or_else(|| panic!("the source is listed as kept: {step:?}"));
    for said in [
        "texture-not-power-of-two",
        "an atlas that stays 100 by 60",
        "texture-mips",
        "a flat decal",
    ] {
        assert!(kept.why.contains(said), "`{said}` in {kept:?}");
    }
}

#[test]
fn a_rule_the_project_switched_off_keeps_a_texture_like_a_waiver_does() {
    let pack = a_pack(&[ODD], |w| {
        switch_off(w, "texture-not-power-of-two");
        switch_off(w, "texture-mips");
    });
    let before = snapshot(&pack.path);

    let (report, revision) = fix(&pack);

    assert!(revision.is_none() && !report.changed(), "{report:?}");
    assert_eq!(named(&pack), [ODD_SOURCE]);
    assert_eq!(snapshot(&pack.path), before);
    let kept = &textures_of(&report).refused;
    assert!(
        kept.len() == 1
            && kept[0].why.contains("texture-mips")
            && kept[0].why.contains("switched off"),
        "{kept:?}"
    );
}

#[test]
fn a_waiver_of_one_garment_holds_back_the_sources_that_garment_names_and_no_other() {
    let second = ("second", GrcTextureFormat::DXT1, 100, 60, 1);
    let pack = a_pack(&[ODD, second], |w| {
        waive(w, "texture-not-power-of-two", Some(0), "an atlas");
        waive(w, "texture-mips", Some(0), "an atlas");
    });

    fix(&pack);

    assert_eq!(
        named(&pack),
        [ODD_SOURCE, "textures/second-2.textures.src"],
        "the first jacket keeps its source, the second is optimised"
    );
}

#[test]
fn a_source_shared_with_a_garment_that_waives_the_rule_is_kept_for_both() {
    let twin = ("twin", GrcTextureFormat::DXT1, 100, 60, 1);
    let pack = a_pack(&[ODD, twin], |w| {
        w.garments[1].variations[0].texture = Some(ODD_SOURCE.into());
        waive(w, "texture-not-power-of-two", Some(0), "an atlas");
        waive(w, "texture-mips", Some(0), "an atlas");
    });
    let before = snapshot(&pack.path);

    let (report, _) = fix(&pack);

    assert_eq!(named(&pack), [ODD_SOURCE, ODD_SOURCE]);
    assert_eq!(snapshot(&pack.path), before);
    assert!(!textures_of(&report).refused.is_empty(), "{report:?}");
}

#[test]
fn a_format_a_waiver_keeps_is_stored_as_it_is_and_one_nobody_waived_is_not() {
    let raw = ("raw", GrcTextureFormat::A8R8G8B8, 64, 64, 7);
    let plain = ("plain", GrcTextureFormat::A8R8G8B8, 64, 64, 7);
    let pack = a_pack(&[raw, plain], |w| {
        waive(w, "texture-format", Some(0), "the pixels are read back");
    });

    let (report, _) = fix(&pack);

    assert_eq!(
        named(&pack),
        ["textures/raw.textures.src", "textures/plain-2.textures.src"]
    );
    let step = textures_of(&report);
    assert!(
        (step.refused.iter())
            .any(|r| r.file == "textures/raw.textures.src" && r.why.contains("texture-format")),
        "{step:?}"
    );
}

#[test]
fn a_bc7_texture_a_waiver_keeps_stays_bc7_in_a_pack_built_for_legacy() {
    let seven = ("seven", GrcTextureFormat::BC7, 64, 64, 7);
    let waived = a_pack(&[seven], |w| {
        waive(
            w,
            "texture-bc7-legacy",
            None,
            "the pack is never run on DirectX 10",
        );
    });
    let unwaived = a_pack(&[seven], |_| {});

    fix(&waived);
    fix(&unwaived);

    assert_eq!(named(&waived), ["textures/seven.textures.src"]);
    assert_eq!(named(&unwaived), ["textures/seven-2.textures.src"]);
}

#[test]
fn a_waiver_with_no_reason_holds_nothing_back_since_the_rules_do_not_heed_it_either() {
    let pack = a_pack(&[ODD], |w| {
        waive(w, "texture-not-power-of-two", None, "  ");
        waive(w, "texture-mips", None, "");
    });

    let (_, revision) = fix(&pack);

    assert!(revision.is_some());
    assert_eq!(named(&pack), ["textures/odd-2.textures.src"]);
}

#[test]
fn a_waiver_of_another_rule_holds_nothing_back() {
    let pack = a_pack(&[ODD], |w| {
        waive(w, "texture-size", None, "big on purpose");
    });

    fix(&pack);

    assert_eq!(named(&pack), ["textures/odd-2.textures.src"]);
}
