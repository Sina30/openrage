//! A plugin that extends a build adds a rule and a file without a change inside the app: installed
//! beside it, answered yes for, it reads each plan the scan and a build make.
//!
//! ⚠ The sample guest is built with `--features extend` by a nested `cargo build` for
//! wasm32-unknown-unknown, as the other plugin suites build theirs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_plugin::{Capability, Level};
use or_project::{Container, Module, Output};

use super::super::builds::{Builds, Job, LastBuild};
use super::super::findings::{collect, Place, Severity};
use super::{read, Ask};
use crate::state::a_plugin_asks_before_it_runs::built_sample;
use crate::state::plugins::{Consent, InstalledPlugin};
use crate::state::AppState;

const WANTS: [Capability; 3] = [
    Capability::ReadPlan,
    Capability::ReportFindings,
    Capability::AddFiles,
];

fn guest() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("extend", "openrage-plugin-sample-extend"))
        .clone()
}

/// A plugin directory as a user installs one: `plugin.json` and `plugin.wasm`.
fn installed(plugins: &Path) -> InstalledPlugin {
    let dir = plugins.join("catalogue");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(guest(), dir.join("plugin.wasm")).unwrap();
    let wants: Vec<String> = WANTS.iter().map(|c| format!("\"{}\"", c.key())).collect();
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"id":"catalogue","name":"Catalogue","abi":0,"wants":[{}]}}"#,
            wants.join(",")
        ),
    )
    .unwrap();
    let plugin = InstalledPlugin::read(&dir);
    assert_eq!(plugin.refused, None, "the plugin directory is a plugin");
    assert!(plugin.manifest.validates() && plugin.manifest.emits());
    plugin
}

fn state_with(plugin: &InstalledPlugin, answered: bool) -> AppState {
    let mut state = AppState::default();
    state.plugins.scanned = Some(vec![plugin.clone()]);
    if answered {
        state.prefs.plugin_consent.insert(
            plugin.manifest.id.clone(),
            Consent::Granted(WANTS.iter().map(|c| c.key().to_string()).collect()),
        );
    }
    state
}

/// A project folder of loose files, one of which the sample warns about and, when `with_a_space`, one it refuses.
fn project(folder: &Path, with_a_space: bool) -> (Container, Vec<PathBuf>) {
    let mut members = Vec::new();
    let mut put = |name: &str, bytes: usize| {
        let path = folder.join(name);
        std::fs::write(&path, vec![b'x'; bytes]).unwrap();
        members.push(path);
    };
    put("ok.lua", 8);
    put("big.dat", (1 << 20) + 5);
    if with_a_space {
        put("a b.lua", 8);
    }
    let container = Container {
        modules: vec![Module::files("files")],
        outputs: vec![Output::fivem_resource("fivem", "pack", &["files"])],
    };
    (container, members)
}

fn ask(folder: &Path, container: &Container, members: &[PathBuf], state: &mut AppState) -> Ask {
    Ask {
        project: folder.join("p.openrage-proj"),
        folder: folder.to_path_buf(),
        container: container.clone(),
        members: members.to_vec(),
        extensions: state.extensions(),
    }
}

#[test]
fn an_installed_validator_and_emitter_change_what_the_scan_finds_and_plans() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    let mut state = state_with(&plugin, true);
    let folder = dir.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let (container, members) = project(&folder, true);

    let scan = read(&ask(&folder, &container, &members, &mut state));

    let plan = &scan.plans[0].resources[0].plan;
    let catalogue = plan
        .files
        .iter()
        .find(|f| f.path == "config/catalogue.json")
        .expect("the emitter's file is in the plan the table shows");
    assert!(matches!(catalogue.origin, or_project::Origin::Document(_)));
    assert!(
        plan.problems
            .iter()
            .any(|p| p.starts_with("plugin.catalogue.space-in-path: ")),
        "an error of the validator is a problem of the plan: {:?}",
        plan.problems
    );
    assert!(!plan.is_buildable());

    let large = scan
        .said
        .iter()
        .find(|s| s.level == Level::Warning && s.text.starts_with("plugin.catalogue.large-file"))
        .expect("the validator warned about the large file");
    assert_eq!(
        (large.plugin.as_str(), large.output.as_str()),
        ("catalogue", "fivem")
    );

    let listed = collect(&scan, None, &HashSet::new(), &[], &Builds::default());
    let about = |severity, text: &str| {
        listed.iter().any(|f| {
            f.severity == severity
                && f.place == Place::Build("fivem".into())
                && f.text.starts_with(text)
        })
    };
    assert!(
        about(Severity::Error, "plugin.catalogue.space-in-path"),
        "{listed:?}"
    );
    assert!(
        about(Severity::Warning, "plugin.catalogue.large-file"),
        "{listed:?}"
    );
    assert_eq!(
        listed
            .iter()
            .filter(|f| f.text.starts_with("plugin.catalogue.space-in-path"))
            .count(),
        1,
        "an error is said once, as the problem of the plan it is"
    );
}

#[test]
fn a_plugin_nobody_answered_yes_for_reads_no_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    let mut state = state_with(&plugin, false);
    assert!(state.extensions().is_empty(), "a request is not a grant");

    let folder = dir.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let (container, members) = project(&folder, true);
    let scan = read(&ask(&folder, &container, &members, &mut state));

    let plan = &scan.plans[0].resources[0].plan;
    assert!(plan.files.iter().all(|f| f.path != "config/catalogue.json"));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert!(scan.said.is_empty());
}

/// Run `job` to its end on the worker a build uses.
fn built(job: Job, id: &str) -> LastBuild {
    let mut builds = Builds::default();
    assert!(builds.start(job));
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        if let Some((_, last)) = builds.poll().into_iter().find(|(done, _)| done == id) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the build did not end");
}

fn job(folder: &Path, container: &Container, members: &[PathBuf], state: &mut AppState) -> Job {
    Job {
        container: container.clone(),
        folder: folder.to_path_buf(),
        root: folder.join("build"),
        members: members.to_vec(),
        outputs: vec!["fivem".into()],
        remap: Arc::new(Default::default()),
        extensions: state.extensions(),
        reproducible: false,
    }
}

#[test]
fn a_validators_error_refuses_the_build_and_an_emitters_file_is_written_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    let mut state = state_with(&plugin, true);
    let folder = dir.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();

    let (container, members) = project(&folder, true);
    let refused = built(job(&folder, &container, &members, &mut state), "fivem");
    let LastBuild::Failed(reasons) = refused else {
        panic!("a plan with an error of the validator is refused");
    };
    assert!(
        reasons
            .iter()
            .any(|r| r.starts_with("plugin.catalogue.space-in-path")),
        "{reasons:?}"
    );
    assert!(
        !folder.join("build").join("pack").exists(),
        "a refused build writes nothing"
    );

    // The same project without the file the validator refuses.
    std::fs::remove_file(folder.join("a b.lua")).unwrap();
    let members: Vec<PathBuf> = members.into_iter().filter(|m| m.exists()).collect();
    let LastBuild::Built(summary) = built(job(&folder, &container, &members, &mut state), "fivem")
    else {
        panic!("the plan has no error any more");
    };
    let written = std::fs::read_to_string(
        summary.parts[0]
            .folder
            .join("config")
            .join("catalogue.json"),
    )
    .expect("the emitted file is in the resource");
    let catalogue: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(catalogue["resource"], "pack");
    assert_eq!(
        summary.parts[0].checked, 4,
        "every file of the plan was read back, the emitted one among them: ok.lua, big.dat, the manifest and the catalogue"
    );
}

#[test]
fn a_plugin_that_extends_a_build_is_asked_about_first_and_reads_every_plan_once_answered_yes() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    let mut state = state_with(&plugin, false);

    state.request_plugin_run("catalogue");
    let asked = state
        .plugins
        .pending_consent
        .as_ref()
        .expect("a plugin nobody has answered about is asked about first");
    assert_eq!(asked.manifest.wants, WANTS.to_vec());
    assert!(
        state.extensions().is_empty(),
        "and nothing reads a plan before the answer"
    );

    state.answer_plugin_consent(true);
    assert_eq!(
        state
            .extensions()
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["catalogue"]
    );
    let report = state.plugin_report("catalogue").expect("a report");
    assert!(
        report.iter().any(|d| d.key == "plugin.extends"),
        "the user is told what answering yes did: {report:?}"
    );

    state.revoke_plugin_consent("catalogue");
    assert!(
        state.extensions().is_empty(),
        "revoking takes it out of every plan again"
    );
}

#[test]
fn a_finding_of_a_validator_is_waived_with_a_reason_and_stands_again_when_the_waiver_goes() {
    use super::super::findings::Fix;
    let dir = tempfile::tempdir().unwrap();
    let plugin = installed(&dir.path().join("plugins"));
    let mut state = state_with(&plugin, true);
    let folder = dir.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let (container, members) = project(&folder, true);
    let code = "plugin.catalogue.space-in-path";
    let listed = |state: &mut AppState| {
        let scan = read(&ask(&folder, &container, &members, state));
        let plan_buildable = scan.plans[0].resources[0].plan.is_buildable();
        let found = collect(&scan, None, &HashSet::new(), &[], &Builds::default());
        (plan_buildable, found)
    };

    // The finding offers a waiver, as a finding of a rule does.
    let (buildable, found) = listed(&mut state);
    assert!(!buildable);
    let row = found
        .iter()
        .find(|f| f.text.starts_with(code))
        .expect("the validator's error is a row of Problems");
    assert_eq!(
        row.fixes,
        [Fix::WaivePlugin {
            code: code.to_string()
        }]
    );

    // A waiver needs a reason, and then the error stops being a problem of the plan.
    assert!(state.waive_plugin_finding(code, "  ").is_err());
    state
        .waive_plugin_finding(code, "the server's paths have spaces")
        .expect("a reason waives it");
    let (buildable, found) = listed(&mut state);
    assert!(buildable, "a waived error no longer refuses the plan");
    assert!(!found.iter().any(|f| f.text.starts_with(code)), "{found:?}");
    assert!(
        found
            .iter()
            .any(|f| f.text.starts_with("plugin.catalogue.large-file")),
        "what is not waived still stands: {found:?}"
    );
    assert_eq!(state.plugin_waivers().len(), 1);

    // Take the waiver away and the finding stands again.
    state.remove_plugin_waiver(code);
    let (buildable, found) = listed(&mut state);
    assert!(!buildable);
    assert!(found.iter().any(|f| f.text.starts_with(code)), "{found:?}");
}
