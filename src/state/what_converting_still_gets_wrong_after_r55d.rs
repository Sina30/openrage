//! What converting still got wrong after R55d: a write that holds the extraction's one slot says what it
//! is, every question the app asks fits a window narrower than it is, and the jobs list's test is held to
//! every kind of job.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use or_interchange::Scene;
use or_io::gta5::File as _;

use super::converting_from_the_app::{question_frame_on, question_labels};
use super::delete_prompt::{DeleteFrom, PendingDelete};
use super::every_write_asks_the_jobs_reading_it::exported;
use super::extract_job::ExtractClash;
use super::gltf::{Asked, Import, ImportChoice};
use super::jobs::JobKind;
use super::payload_handlers::handler_for_payload;
use super::{AppState, DocPayload, Prefs};

fn told(state: &AppState, what: &str) -> bool {
    state.toasts.iter().any(|t| t.message.contains(what))
}

/// A job of `kind` holding the one slot, that runs until it is stopped.
fn holding_the_slot(state: &mut AppState, kind: JobKind, dest: &std::path::Path) {
    state.extract_jobs.start(
        kind,
        "prop.glb".into(),
        dest.to_path_buf(),
        Vec::new(),
        |cancel, progress| {
            or_job::run(0..1_000_000u32, &cancel, &progress, |_| {
                std::thread::sleep(Duration::from_millis(1));
                or_job::Step::Done(0)
            })
        },
    );
}

#[test]
fn a_second_write_names_the_job_that_holds_the_slot_as_the_list_does() {
    for (kind, noun) in [
        (JobKind::Export, "An export"),
        (JobKind::Conversion, "A conversion"),
        (JobKind::Extraction, "An extraction"),
    ] {
        let mut state = AppState::from_prefs(Prefs::default());
        let scratch = tempfile::tempdir().expect("a scratch folder");
        holding_the_slot(&mut state, kind, scratch.path());

        state.gltf.asked = Some(exported(Scene::default()));
        state.write_gltf(scratch.path().join("again.glb"));
        assert!(
            told(&state, &format!("{noun} is running")),
            "a glTF write under {kind:?} says: {:?}",
            state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
        state.cancel_job(kind);
    }
}

/// Each question the app asks, opened in a state of its own.
fn questions() -> Vec<(&'static str, AppState)> {
    let mut all = Vec::new();

    let mut state = AppState::from_prefs(Prefs::default());
    state.gltf.asked = Some(exported(Scene::default()));
    all.push(("the glTF export's", state));

    let mut state = AppState::from_prefs(Prefs::default());
    let payload = DocPayload::Resource(or_io::Asset::Drawable(Default::default()));
    state.gltf.asked = Some(Asked::Import(Import {
        doc: 1,
        name: "prop.ydr".into(),
        mesh: PathBuf::from("prop.glb"),
        bytes: Arc::new(Vec::new()),
        handler: Some(handler_for_payload(&payload).expect("a drawable has a handler")),
        gta6: None,
        edits: 0,
        choice: ImportChoice::default(),
        drawables: vec![(0, "prop".into())],
        made: Some(Err("the mesh has no triangles".into())),
    }));
    all.push(("the glTF import's", state));

    let mut state = AppState::from_prefs(Prefs::default());
    state.pending_delete = Some(PendingDelete {
        doc_id: 1,
        node_id: 1,
        name: "a_file_with_a_name.ytd".into(),
        files: 3,
        folders: 1,
        bytes: 4096,
        from: DeleteFrom::Disk,
    });
    all.push(("the delete's", state));

    let mut state = AppState::from_prefs(Prefs::default());
    state.extract_jobs.clash = Some(ExtractClash {
        doc_id: 1,
        nodes: vec![1],
        dest: PathBuf::from("C:/somewhere/out"),
        form: or_io::container::rpf::BodyForm::Decoded,
        filter: super::extract_job::TypeFilter::All,
        fixes: false,
        clashes: 2,
        planned: 5,
        first: "models/prop.ydr".into(),
    });
    all.push(("the clash's", state));
    all
}

#[test]
fn every_question_fits_a_window_narrower_than_it_is() {
    let narrow = egui::vec2(300.0, 700.0);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, narrow);
    for (which, mut state) in questions() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        for _ in 0..3 {
            question_frame_on(&ctx, &mut state, narrow, Vec::new());
        }
        let shown = question_labels(&question_frame_on(&ctx, &mut state, narrow, Vec::new()));
        assert!(!shown.is_empty(), "{which} question draws nothing");
        for (text, rect) in &shown {
            assert!(
                screen.contains_rect(*rect),
                "{which} question: `{text}` is not within a window {} wide: {rect:?}",
                narrow.x
            );
        }
    }
}

#[test]
fn convert_with_files_focused_and_nothing_chosen_says_to_choose_in_files() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(scratch.path().join("notes.txt"), b"a note").expect("a file to list");
    let (mut state, _) =
        super::converting_from_the_app::browsing(scratch.path(), &Default::default());
    let ws = state.active_workspace;
    crate::ui::dock_commands::reveal(
        state.workspace_docks.get_mut(ws),
        crate::ui::panels::registry::ids::FILES,
    );
    assert_eq!(
        state.workspace_docks.focused(ws),
        Some(crate::ui::panels::registry::ids::FILES),
        "Files is not the focused panel, so the test shows nothing"
    );
    let convert = crate::ui::commands::of(&state, crate::ui::commands::Action::Convert)
        .expect("Convert is a command");
    assert_eq!(
        convert.enabled,
        Err("Select a file or folder in Files first"),
        "Convert blames the open folder when the user is in Files with nothing chosen"
    );
}

#[test]
fn a_conversion_written_while_another_job_holds_the_slot_names_it_and_keeps_the_question() {
    for (kind, noun) in [
        (JobKind::Export, "An export"),
        (JobKind::Extraction, "An extraction"),
    ] {
        let mut state = AppState::from_prefs(Prefs::default());
        let scratch = tempfile::tempdir().expect("a scratch folder");
        holding_the_slot(&mut state, kind, scratch.path());

        let bytes = super::what_converting_still_gets_wrong_after_r55c::a_ytd_holding("paint")
            .to_bytes()
            .expect("a Legacy .ytd saves");
        let made = or_cli::retarget_file("paint.ytd", &bytes, &Default::default());
        state.convert.done = Some(super::convert::Converted {
            files: vec![super::convert::ConvertedFile {
                name: "paint.ytd".into(),
                outcome: Ok(made.expect("it converts")),
            }],
            without_database: false,
            of_document: false,
        });
        let dest = scratch.path().join("paint.ytd");
        state.write_conversion(dest.clone(), None);
        assert!(
            told(&state, &format!("{noun} is running")),
            "a conversion written under {kind:?} says: {:?}",
            state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
        assert!(
            state.convert.done.is_some() && !dest.exists(),
            "a refused write must keep the question and write nothing"
        );
        state.cancel_job(kind);
    }
}
