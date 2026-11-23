use super::*;
use crate::dxbc_testing::{
    container, dwords, rdef, red_pixel_shader, signature, textured_pixel_shader,
};

#[test]
fn a_folder_of_dxbc_goes_through_the_translator_the_legalizer_and_naga() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("red.fxc"), red_pixel_shader()).unwrap();
    std::fs::write(tmp.path().join("again.bin"), red_pixel_shader()).unwrap();
    let tally = translate_check(tmp.path(), Target::Auto).unwrap();
    assert_eq!(tally.containers, 2);
    assert_eq!(tally.translated, 2);
    assert!(tally.refused.is_empty(), "{:?}", tally.refused);
    assert_eq!(tally.check.modules(), 2);
    let msl: usize = tally.check.stages.values().map(|s| s.msl).sum();
    assert_eq!(msl, 2, "{}", tally.report());
}

#[test]
fn dxil_and_a_container_with_no_program_are_set_aside_and_not_translated() {
    let dxil = container(&[(
        *b"DXIL",
        dwords(&[0x0001_0066, 6, 0x4C49_5844, 0x102, 16, 0]),
    )]);
    let neither = container(&[(*b"RDEF", vec![0; 8])]);
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("dxil.bin"), dxil).unwrap();
    std::fs::write(tmp.path().join("none.bin"), neither).unwrap();
    let tally = translate_check(tmp.path(), Target::Auto).unwrap();
    assert_eq!(tally.containers, 2);
    assert_eq!(tally.translated, 0);
    assert_eq!(tally.skipped["DXIL"], 1);
    assert_eq!(tally.skipped["no program part"], 1);
}

#[test]
fn a_program_the_translator_refuses_is_counted_with_its_message() {
    let broken = container(&[
        (*b"ISGN", signature("SV_Position", 1)),
        (*b"OSGN", signature("SV_Target", 0)),
        (*b"SHDR", dwords(&[0x40, 4, 0xffff_ffff, 0xffff_ffff])),
    ]);
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("broken.bin"), broken).unwrap();
    let tally = translate_check(tmp.path(), Target::Auto).unwrap();
    assert_eq!(tally.translated, 0, "{}", tally.report());
    assert_eq!(
        tally.refused.values().sum::<usize>(),
        1,
        "{}",
        tally.report()
    );
}

#[test]
fn the_report_and_the_json_carry_the_same_counts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("red.bin"), red_pixel_shader()).unwrap();
    let tally = translate_check(tmp.path(), Target::Auto).unwrap();
    assert!(tally
        .report()
        .starts_with("1 containers: 1 translated, 0 refused, 0 set aside"));
    assert_eq!(tally.to_json()["translated"], 1);
    assert_eq!(tally.to_json()["check"]["modules"], 1);
}

#[test]
fn what_a_folder_binds_is_counted_against_what_its_shaders_reflect() {
    let tmp = tempfile::tempdir().unwrap();
    let reflection = rdef(&[
        ("Globals", 0, 3),
        ("Diffuse", 2, 5),
        ("DiffuseSampler", 3, 2),
    ]);
    std::fs::write(
        tmp.path().join("agrees.bin"),
        textured_pixel_shader(3, 5, 2, Some(reflection.clone())),
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("moved.bin"),
        textured_pixel_shader(3, 5, 2, Some(rdef(&[("Diffuse", 2, 6)]))),
    )
    .unwrap();
    std::fs::write(tmp.path().join("bare.bin"), red_pixel_shader()).unwrap();
    let tally = translate_check(tmp.path(), Target::Auto).unwrap();
    assert_eq!(tally.translated, 3, "{}", tally.report());
    assert_eq!(tally.bindings.same, 1, "{}", tally.report());
    assert_eq!(tally.bindings.differs, 1, "{}", tally.report());
    assert_eq!(tally.bindings.no_reflection, 1, "{}", tally.report());
    assert!(tally.report().contains("bindings against reflection, of 3"));
    assert_eq!(tally.to_json()["bindings"]["same"], 1);
}
