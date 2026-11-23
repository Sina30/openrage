use std::collections::BTreeMap;

use or_io::gta5::shaders::spirv_pack::ShaderStageKind;
use or_io::gta5::shaders::spirv_pack::{
    Fidelity, NagaReach, Outcome, Production, Program, Reach, Reflection, PACK_VERSION,
};

use super::*;

fn identity() -> Identity {
    Identity {
        translator: "dxbc-spirv abc".into(),
        legalizer: "0123".into(),
        pack_version: PACK_VERSION,
    }
}

fn program(outcome: Outcome) -> Program {
    Program {
        stage: ShaderStageKind::Vertex,
        source_bytes: 1,
        outcome,
        reflection: Reflection::default(),
    }
}

fn translated() -> Outcome {
    Outcome::Translated {
        spirv_bytes: 4,
        fidelity: Fidelity::Equivalent,
        lowerings: BTreeMap::new(),
        naga: NagaReach {
            reached: Reach::MslWritten,
            cause: None,
        },
    }
}

/// A pack written by `identity()` from the build `fingerprint`, with `translated_programs` programs translated and one refused.
fn pack(fingerprint: &str, translated_programs: usize) -> SpirvPack {
    let mut pack = SpirvPack::new(Production {
        translator: identity().translator,
        legalizer: identity().legalizer,
    });
    pack.source_build_hash = fingerprint.into();
    for i in 0..translated_programs {
        pack.programs.insert(format!("t{i}"), program(translated()));
    }
    pack.programs.insert(
        "refused".into(),
        program(Outcome::Refused {
            reason: "no".into(),
        }),
    );
    pack
}

fn write(root: &Path, pack: &SpirvPack) {
    std::fs::create_dir_all(root).expect("a folder");
    std::fs::write(
        root.join(MANIFEST_FILE),
        serde_json::to_vec(pack).expect("it serialises"),
    )
    .expect("the manifest is written");
}

#[test]
fn a_pack_is_current_only_when_who_wrote_it_and_what_it_was_made_from_are_what_they_are_now() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let root = dir.path();
    assert_eq!(
        current_pack(root, &identity(), "build"),
        None,
        "no manifest"
    );

    write(root, &pack("build", 3));
    assert_eq!(current_pack(root, &identity(), "build"), Some(3));

    for (why, other) in [
        (
            "another translator",
            Identity {
                translator: "other".into(),
                ..identity()
            },
        ),
        (
            "another legalizer",
            Identity {
                legalizer: "other".into(),
                ..identity()
            },
        ),
        (
            "another pack version",
            Identity {
                pack_version: PACK_VERSION + 1,
                ..identity()
            },
        ),
    ] {
        assert_eq!(current_pack(root, &other, "build"), None, "{why}");
    }
    assert_eq!(
        current_pack(root, &identity(), "a patched build"),
        None,
        "another build of the game"
    );

    write(root, &pack("build", 0));
    assert_eq!(
        current_pack(root, &identity(), "build"),
        None,
        "nothing translated"
    );

    std::fs::write(root.join(MANIFEST_FILE), b"not json").expect("a file");
    assert_eq!(
        current_pack(root, &identity(), "build"),
        None,
        "a manifest that does not read"
    );
}

#[test]
fn the_programs_a_pack_translated_are_counted_whoever_wrote_it() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    assert_eq!(translated_in(dir.path()), None);
    write(dir.path(), &pack("any", 2));
    assert_eq!(translated_in(dir.path()), Some(2));
    write(dir.path(), &pack("any", 0));
    assert_eq!(translated_in(dir.path()), None);
}

#[test]
fn an_install_is_named_by_the_size_and_age_of_its_executable() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    assert_eq!(install_fingerprint(dir.path()), "", "no executable");
    std::fs::write(dir.path().join("GTA5.exe"), [0u8; 10]).expect("a file");
    let small = install_fingerprint(dir.path());
    assert!(small.starts_with("a-"), "{small}");
    std::fs::write(dir.path().join("GTA5.exe"), [0u8; 11]).expect("a file");
    assert_ne!(
        install_fingerprint(dir.path()),
        small,
        "a patch changes the size"
    );
    // The Enhanced executable names an install too, and the first of the names wins
    let enhanced = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(enhanced.path().join("GTA5_Enhanced.exe"), [0u8; 255]).expect("a file");
    assert!(install_fingerprint(enhanced.path()).starts_with("ff-"));
}

#[test]
fn the_helper_is_the_one_named_else_the_one_beside_the_editor() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let name = format!("or-shader-tool{}", std::env::consts::EXE_SUFFIX);
    let beside = dir.path().join(&name);
    let editor = dir.path().join("openrage");
    assert_eq!(helper_in(None, Some(&editor)), None, "none beside it yet");
    std::fs::write(&beside, b"").expect("a file");
    assert_eq!(helper_in(None, Some(&editor)), Some(beside.clone()));
    let named = dir.path().join("elsewhere");
    assert_eq!(
        helper_in(Some(named.clone()), Some(&editor)),
        Some(beside.clone()),
        "a name that is no file is passed over"
    );
    std::fs::write(&named, b"").expect("a file");
    assert_eq!(
        helper_in(Some(named.clone()), Some(&editor)),
        Some(named),
        "the named comes first"
    );
    assert_eq!(helper_in(None, None), None);
}

#[test]
fn each_status_says_what_it_is_in_a_line() {
    use GamePackStatus::*;
    assert_eq!(Missing.short_label(), "Game shaders not baked");
    assert_eq!(
        Baking { done: 0, total: 0 }.short_label(),
        "Reading the game's shaders"
    );
    assert_eq!(
        Baking { done: 5, total: 40 }.short_label(),
        "Baking the game's shaders: 5 of 40"
    );
    assert_eq!(
        Ready { translated: 9 }.short_label(),
        "Game shaders ready (9 programs)"
    );
    assert!(NoHelper("gone".into()).short_label().contains("gone"));
    assert!(Failed {
        message: "boom".into()
    }
    .short_label()
    .contains("boom"));
}

/// A stand-in for the helper: a script that says what its arguments ask of it.
#[cfg(unix)]
mod with_a_helper {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    use super::*;

    fn helper(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("helper.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("a script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("it is made to run");
        path
    }

    const WHO: &str = r#"{"event":"identity","translator":"dxbc-spirv abc","legalizer":"0123","pack_version":PACK}"#;

    fn who() -> String {
        WHO.replace("PACK", &PACK_VERSION.to_string())
    }

    /// The script of a helper that bakes: it says who it is, and for `bake-pack` runs `on_bake`.
    fn baking(dir: &Path, on_bake: &str) -> PathBuf {
        helper(
            dir,
            &format!(
                "if [ \"$1\" = pack-identity ]; then echo '{}'; exit 0; fi\n{on_bake}",
                who()
            ),
        )
    }

    /// Poll until the controller says something other than progress, or fail.
    fn until_it_ends(controller: &mut GamePackController) -> Vec<GamePackEvent> {
        let until = Instant::now() + Duration::from_secs(20);
        let mut told = Vec::new();
        while Instant::now() < until {
            if let Some(event) = controller.poll() {
                let ended = event != GamePackEvent::Progress;
                told.push(event);
                if ended {
                    return told;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the bake did not end: {told:?}");
    }

    #[test]
    fn the_helper_says_who_it_is_and_a_helper_that_cannot_says_why() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let good = baking(dir.path(), "exit 0");
        assert_eq!(identity_of(&good), Ok(identity()));
        let without = helper(
            dir.path(),
            "echo 'Error: this build has no DXBC translator.' >&2; exit 1",
        );
        assert_eq!(
            identity_of(&without),
            Err("Error: this build has no DXBC translator.".to_string())
        );
        assert!(identity_of(&dir.path().join("none")).is_err());
        let silent = helper(dir.path(), "exit 0");
        assert!(identity_of(&silent)
            .unwrap_err()
            .contains("did not say who"));
    }

    #[test]
    fn a_bake_reports_its_progress_and_ends_with_the_pack_it_wrote() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let fixture = dir.path().join("fixture.json");
        std::fs::write(&fixture, serde_json::to_vec(&pack("build", 2)).unwrap()).unwrap();
        let go = dir.path().join("go");
        // It stops after its first count until told to go on, so that the count is seen as it stands
        let script = baking(
            dir.path(),
            &format!(
                "echo '{{\"event\":\"reading\"}}'\n\
                 echo 'a log line, which is not an event'\n\
                 echo '{{\"event\":\"progress\",\"done\":1,\"total\":2}}'\n\
                 while [ ! -f '{}' ]; do sleep 0.01; done\n\
                 mkdir -p \"$4\" && cp '{}' \"$4/manifest.json\"\n\
                 echo '{{\"event\":\"progress\",\"done\":2,\"total\":2}}'",
                go.display(),
                fixture.display()
            ),
        );
        let root = dir.path().join("pack");
        let mut controller = GamePackController::at(root.clone(), false);
        assert_eq!(controller.start_with(Some(script), dir.path(), false), None);
        assert_eq!(
            controller.status(),
            &GamePackStatus::Baking { done: 0, total: 0 }
        );
        assert!(controller
            .job()
            .is_some_and(|j| j.kind == JobKind::GamePackBake && j.fraction.is_none()));
        let until = Instant::now() + Duration::from_secs(20);
        let counted = GamePackStatus::Baking { done: 1, total: 2 };
        while controller.status() != &counted {
            assert!(
                Instant::now() < until,
                "no count came: {:?}",
                controller.status()
            );
            controller.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let job = controller.job().expect("it is running");
        assert_eq!(job.fraction, Some(0.5));
        assert_eq!(job.detail, "1 of 2 programs");
        std::fs::write(&go, b"").unwrap();
        let told = until_it_ends(&mut controller);
        assert_eq!(
            told.last(),
            Some(&GamePackEvent::Ready { root, baked: true })
        );
        assert_eq!(
            controller.status(),
            &GamePackStatus::Ready { translated: 2 }
        );
        assert!(!controller.is_baking() && controller.job().is_none());
    }

    #[test]
    fn a_helper_that_fails_says_its_last_word() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let script = baking(dir.path(), "echo 'reading'; echo 'out of room' >&2; exit 3");
        let mut controller = GamePackController::at(dir.path().join("pack"), false);
        controller.start_with(Some(script), dir.path(), false);
        let told = until_it_ends(&mut controller);
        assert_eq!(
            told.last(),
            Some(&GamePackEvent::Failed {
                message: "out of room".into()
            })
        );
        assert!(matches!(controller.status(), GamePackStatus::Failed { .. }));
    }

    #[test]
    fn a_helper_that_exits_well_and_wrote_no_pack_is_a_failure() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let script = baking(dir.path(), "exit 0");
        let mut controller = GamePackController::at(dir.path().join("pack"), false);
        controller.start_with(Some(script), dir.path(), false);
        let told = until_it_ends(&mut controller);
        assert_eq!(
            told.last(),
            Some(&GamePackEvent::Failed {
                message: "the helper wrote no pack".into()
            })
        );
    }

    #[test]
    fn a_stopped_bake_ends_at_once_and_leaves_the_pack_missing() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let script = baking(dir.path(), "echo '{\"event\":\"reading\"}'\nexec sleep 60");
        let mut controller = GamePackController::at(dir.path().join("pack"), false);
        controller.start_with(Some(script), dir.path(), false);
        assert!(controller.is_baking());
        controller.cancel();
        assert!(controller.job().is_some_and(|j| j.stopping));
        let started = Instant::now();
        let told = until_it_ends(&mut controller);
        assert_eq!(told.last(), Some(&GamePackEvent::Stopped));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the helper was killed, not waited for"
        );
        assert_eq!(controller.status(), &GamePackStatus::Missing);
    }

    #[test]
    fn a_pack_that_is_current_is_not_baked_again_unless_asked() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let install = dir.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("GTA5.exe"), [0u8; 4]).unwrap();
        let fingerprint = install_fingerprint(&install);
        let root = dir.path().join("pack");
        write(&root, &pack(&fingerprint, 5));
        // A helper that knows who it is and refuses to bake, so a bake that starts is seen
        let script = baking(dir.path(), "echo 'baked again' >&2; exit 1");
        let mut controller = GamePackController::at(root.clone(), false);
        assert_eq!(
            controller.start_with(Some(script.clone()), &install, false),
            Some(GamePackEvent::Ready {
                root: root.clone(),
                baked: false
            })
        );
        assert!(!controller.is_baking());
        assert_eq!(
            controller.status(),
            &GamePackStatus::Ready { translated: 5 }
        );
        controller.start_with(Some(script), &install, true);
        assert!(controller.is_baking(), "asked to bake, it bakes");
        let told = until_it_ends(&mut controller);
        assert_eq!(
            told.last(),
            Some(&GamePackEvent::Failed {
                message: "baked again".into()
            })
        );
    }

    #[test]
    fn a_pack_of_a_patched_build_is_baked_again() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let install = dir.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("GTA5.exe"), [0u8; 4]).unwrap();
        let root = dir.path().join("pack");
        write(&root, &pack("the build before the patch", 5));
        let script = baking(dir.path(), "echo 'baked again' >&2; exit 1");
        let mut controller = GamePackController::at(root, false);
        controller.start_with(Some(script), &install, false);
        assert!(controller.is_baking());
        let _ = until_it_ends(&mut controller);
    }

    #[test]
    fn no_helper_or_one_that_cannot_translate_is_a_status_and_not_a_failure_to_retry() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let mut controller = GamePackController::at(dir.path().join("pack"), false);
        assert_eq!(controller.start_with(None, dir.path(), false), None);
        assert!(matches!(controller.status(), GamePackStatus::NoHelper(_)));
        let without = helper(dir.path(), "echo 'no translator' >&2; exit 1");
        assert_eq!(
            controller.start_with(Some(without), dir.path(), false),
            None
        );
        assert_eq!(
            controller.status(),
            &GamePackStatus::NoHelper("no translator".into())
        );
        assert!(!controller.is_baking());
    }

    #[test]
    fn a_pack_the_owner_named_is_read_and_never_baked_over() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let root = dir.path().join("pack");
        write(&root, &pack("anything", 4));
        let script = baking(dir.path(), "echo 'must not run' >&2; exit 1");
        let mut controller = GamePackController::at(root, true);
        assert_eq!(controller.start_with(Some(script), dir.path(), true), None);
        assert!(!controller.is_baking());
        assert_eq!(
            controller.status(),
            &GamePackStatus::Ready { translated: 4 }
        );
        let empty = GamePackController::at(dir.path().join("none"), true)
            .status()
            .clone();
        assert_eq!(empty, GamePackStatus::Missing);
    }

    #[test]
    fn one_automatic_bake_is_tried_a_run_and_none_starts_over_one_in_flight() {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let mut controller = GamePackController::at(dir.path().join("pack"), false);
        let mut tried = 0;
        for _ in 0..3 {
            controller.auto(|_| {
                tried += 1;
                None
            });
        }
        assert_eq!(tried, 1, "a run tries once");

        // A bake in flight is not started over, whatever has been tried
        let script = baking(dir.path(), "echo '{\"event\":\"reading\"}'\nexec sleep 60");
        let mut flying = GamePackController::at(dir.path().join("pack2"), false);
        flying.start_with(Some(script), dir.path(), false);
        flying.auto_attempted = false;
        let mut started = false;
        flying.auto(|_| {
            started = true;
            None
        });
        assert!(!started);
        flying.cancel();
        let _ = until_it_ends(&mut flying);
    }
}
