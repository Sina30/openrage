//! A build says what it is on and how many of the plan's bytes are done while it writes, and a flag stops it: it
//! removes the folder it was writing and leaves the build it would have replaced as it was.

mod common;

use std::path::Path;

use common::{declare, planned, snapshot};
use or_io::gta5::shaders::shader_defs::ParamRemap;
use or_project::{build, build_all_watched, build_watched, BuildError, BuildStep, Plan, Watch};
use serde_json::{json, Value};

fn files(tag: &str) -> Vec<(String, Vec<u8>)> {
    (0..12)
        .map(|n| {
            (
                format!("data/f{n}.txt"),
                format!("{tag} {n}").repeat(200).into_bytes(),
            )
        })
        .collect()
}

/// A project of `files` with one build of `kind`.
fn plan_of(dir: &Path, files: &[(String, Vec<u8>)], kind: &str) -> Plan {
    let borrowed: Vec<(&str, Vec<u8>)> =
        files.iter().map(|(p, b)| (p.as_str(), b.clone())).collect();
    let output: Value = json!({ "id": "build", "kind": kind, "name": "pack", "take": ["files"] });
    let project = declare(dir, &borrowed, Value::Null, output);
    let mut plans = planned(&project);
    assert!(plans[0].is_buildable(), "{:?}", plans[0].problems);
    plans.remove(0)
}

fn stop_when_writing(watch: &Watch) -> impl Fn(BuildStep) + '_ {
    |step| {
        if step == BuildStep::Write {
            watch.cancel.set();
        }
    }
}

#[test]
fn the_counters_end_at_the_whole_of_the_plan_when_a_build_is_done() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let plan = plan_of(&dir.path().join("p"), &files("a"), "fivem-resource");
    let watch = Watch::default();
    let built = build_watched(
        &plan,
        out.path(),
        &ParamRemap::default(),
        None,
        &|_| {},
        &watch,
    )
    .expect("it builds");
    let now = watch.progress.snapshot();
    assert_eq!(now.total, plan.totals.bytes, "{now:?}");
    assert_eq!(now.attempted, now.total, "everything is done: {now:?}");
    assert!(!now.item.is_empty(), "it names the file it was on last");
    assert!(built.folder.join("data/f3.txt").is_file());
}

#[test]
fn a_build_stopped_while_it_writes_leaves_the_last_good_build_and_no_half_written_folder() {
    for kind in ["fivem-resource", "singleplayer-dlc", "oiv-package"] {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let project = dir.path().join("p");
        // The last good build.
        let mut first = files("old");
        if kind != "fivem-resource" {
            // A pack types its files by `file_types` or by extension; an archive's contents need none.
            first = (0..12)
                .map(|n| {
                    (
                        format!("x64/models/cdimages/a.rpf/f{n}.dat"),
                        format!("old {n}").into_bytes(),
                    )
                })
                .collect();
        }
        let plan = plan_of(&project, &first, kind);
        let done = build(&plan, out.path()).expect("the first build is written");
        let before = snapshot(&done.folder);

        // The same project changed, built again and stopped as it begins to write.
        let mut second = first.clone();
        for (_, bytes) in &mut second {
            bytes.extend_from_slice(b" and then some");
        }
        let plan = plan_of(&project, &second, kind);
        let watch = Watch::default();
        let stopped = build_watched(
            &plan,
            out.path(),
            &ParamRemap::default(),
            None,
            &stop_when_writing(&watch),
            &watch,
        );
        assert!(
            matches!(stopped, Err(BuildError::Stopped)),
            "{kind}: {stopped:?}"
        );
        assert_eq!(
            snapshot(&done.folder),
            before,
            "{kind}: the build it would have replaced is as it was"
        );
        let hidden: Vec<String> = std::fs::read_dir(out.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".new") || n.ends_with(".cook"))
            .collect();
        assert!(
            hidden.is_empty(),
            "{kind}: the folder it was writing is removed: {hidden:?}"
        );
    }
}

#[test]
fn a_build_stopped_before_it_starts_writes_nothing_and_a_run_stops_at_the_one_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let plan = plan_of(&dir.path().join("p"), &files("a"), "fivem-resource");
    let watch = Watch::default();
    watch.cancel.set();
    let (built, stopped) = build_all_watched(
        &[plan.clone(), plan],
        out.path(),
        &ParamRemap::default(),
        None,
        &|_| {},
        &watch,
    );
    assert!(built.is_empty());
    assert!(matches!(stopped, Some(BuildError::Stopped)), "{stopped:?}");
    assert!(!out.path().join("pack").exists());
}
