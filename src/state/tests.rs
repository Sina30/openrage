use super::*;

fn fresh_state() -> AppState {
    let mut state = AppState::new();
    state.documents.clear();
    state.active_document = None;
    state
}

#[test]
fn a_fresh_state_starts_in_browse() {
    use crate::ui::dock_layout::restored_workspace;
    let dir = tempfile::tempdir().expect("a temp dir");
    let first_run = load_prefs_from(&dir.path().join("prefs.json"));
    assert_eq!(
        restored_workspace(&first_run.dock_layouts),
        Workspace::Browse,
        "a first run has no preferences to restore a workspace from"
    );
    // The workspace this thread's own preferences restore, which a draw of the shell may have saved.
    let state = AppState::default();
    assert_eq!(
        state.active_workspace,
        restored_workspace(&state.prefs.dock_layouts),
        "a state starts in the workspace its preferences restore"
    );
}

#[test]
fn test_app_state_show_toast() {
    let mut state = fresh_state();
    state.show_toast("Success!");
    assert_eq!(state.toasts.iter().count(), 1);
    assert_eq!(
        state.toasts.iter().next().expect("one toast").toast_type,
        ToastType::Success
    );
}

#[test]
fn test_app_state_close_document_updates_active() {
    let mut state = fresh_state();
    let id = state.insert_document(
        PathBuf::from("a.ypt"),
        DocPayload::Resource(or_io::Asset::Particles(Default::default())),
        None,
        None,
    );
    assert_eq!(state.active_document, Some(id));
    state.close_document(id);
    assert!(state.documents.is_empty());
    assert!(state.active_document.is_none());
}

#[test]
fn weather_state_change_marks_dirty() {
    let mut state = fresh_state();
    state.sim.pending_rebuild = false;
    assert!(!state.set_weather_state(drivers::WeatherState::Clear));
    assert!(!state.sim.pending_rebuild);
    assert!(state.set_weather_state(drivers::WeatherState::Rain));
    assert!(
        state.sim.pending_rebuild,
        "weather change must arm sim.pending_rebuild"
    );
    state.sim.pending_rebuild = false;
    assert!(!state.set_weather_state(drivers::WeatherState::Rain));
    assert!(!state.sim.pending_rebuild);
}

#[test]
fn ydd_document_lifecycle() {
    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());

    let payload = empty_payload_for_kind(DocumentKind::Ydd)
        .expect("a .ydd opens as an owned value, so its blank is its `Default`");
    assert_eq!(payload.kind(), DocumentKind::Ydd);
    let DocPayload::Resource(or_io::Asset::DrawableDict(dict)) = &payload else {
        panic!("empty_payload_for_kind gave the wrong variant");
    };
    assert!(dict.entries.is_empty());

    let bytes = serialize_payload(&payload, RpfOutputMode::Open, crypto.clone())
        .expect("serialize empty YDD");
    assert_eq!(&bytes[0..4], b"RSC7");
    let embedded = empty_file_bytes_for_kind(DocumentKind::Ydd).expect("embed bytes");
    assert_eq!(&embedded[0..4], b"RSC7");

    let reloaded = parse_payload(
        std::path::Path::new("untitled.ydd"),
        DocumentKind::Ydd,
        &bytes,
        crypto.clone(),
    )
    .expect("reload empty YDD with no pack");
    assert_eq!(reloaded.kind(), DocumentKind::Ydd);

    assert!(parse_payload(
        std::path::Path::new("inside.ydd"),
        DocumentKind::Ydd,
        &embedded,
        crypto,
    )
    .is_ok());
}

#[test]
fn ydd_routes_to_model_workspace() {
    use crate::ui::dock::Workspace;
    assert_eq!(Workspace::for_kind(DocumentKind::Ydd), Workspace::Model);
    assert!(Workspace::Model.handles_kind(DocumentKind::Ydd));
    assert!(Workspace::Texture.handles_kind(DocumentKind::Ydd));
}

#[test]
fn yld_document_lifecycle() {
    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());

    let payload = empty_payload_for_kind(DocumentKind::Yld)
        .expect("a .yld opens as an owned value, so its blank is its `Default`");
    assert_eq!(payload.kind(), DocumentKind::Yld);
    let DocPayload::Resource(or_io::Asset::ClothDict(dict)) = &payload else {
        panic!("empty_payload_for_kind gave the wrong variant");
    };
    assert!(dict.entries.is_empty());

    let bytes = serialize_payload(&payload, RpfOutputMode::Open, crypto.clone())
        .expect("serialize empty YLD");
    assert_eq!(&bytes[0..4], b"RSC7");
    let embedded = empty_file_bytes_for_kind(DocumentKind::Yld).expect("embed bytes");
    assert_eq!(&embedded[0..4], b"RSC7");

    let reloaded = parse_payload(
        std::path::Path::new("untitled.yld"),
        DocumentKind::Yld,
        &bytes,
        crypto.clone(),
    )
    .expect("reload empty YLD with no pack");
    assert_eq!(reloaded.kind(), DocumentKind::Yld);

    assert!(parse_payload(
        std::path::Path::new("inside.yld"),
        DocumentKind::Yld,
        &embedded,
        crypto,
    )
    .is_ok());
}

#[test]
fn yld_routes_to_model_workspace() {
    use crate::ui::dock::Workspace;
    assert_eq!(Workspace::for_kind(DocumentKind::Yld), Workspace::Model);
    assert!(Workspace::Model.handles_kind(DocumentKind::Yld));
}

#[test]
fn ycd_document_lifecycle() {
    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());

    let payload =
        empty_payload_for_kind(DocumentKind::Ycd).expect("a .ycd authors from the owned default");
    assert_eq!(payload.kind(), DocumentKind::Ycd);
    let DocPayload::Resource(or_io::Asset::Clips(dict)) = &payload else {
        panic!("empty_payload_for_kind gave the wrong variant");
    };
    assert!(or_anim::clip::clips(dict).next().is_none());

    let bytes = serialize_payload(&payload, RpfOutputMode::Open, crypto.clone())
        .expect("serialize empty YCD");
    assert_eq!(&bytes[0..4], b"RSC7");
    let embedded = empty_file_bytes_for_kind(DocumentKind::Ycd).expect("embed bytes");
    assert_eq!(&embedded[0..4], b"RSC7");

    let reloaded = parse_payload(
        std::path::Path::new("untitled.ycd"),
        DocumentKind::Ycd,
        &bytes,
        crypto.clone(),
    )
    .expect("reload empty YCD");
    assert_eq!(reloaded.kind(), DocumentKind::Ycd);

    assert!(parse_payload(
        std::path::Path::new("inside.ycd"),
        DocumentKind::Ycd,
        &embedded,
        crypto,
    )
    .is_ok());

    assert_eq!(payload.clone().kind(), DocumentKind::Ycd);
}

#[test]
fn ycd_routes_to_animation_workspace() {
    use crate::ui::dock::Workspace;
    assert_eq!(Workspace::for_kind(DocumentKind::Ycd), Workspace::Animation);
    assert!(Workspace::Animation.handles_kind(DocumentKind::Ycd));
    assert!(
        !Workspace::Model.handles_kind(DocumentKind::Ycd),
        "the Model viewport would render nothing for a clip dictionary"
    );
    assert!(Workspace::ALL.contains(&Workspace::Animation));
}

#[test]
fn every_registered_resource_extension_opens() {
    for ext in crate::state::format_handlers::known_extensions() {
        let path = format!("some_file.{ext}");
        assert!(
            DocumentKind::from_path(std::path::Path::new(&path)).is_some(),
            "`.{ext}` — the registry knows it and the app does not"
        );
    }
}

#[test]
fn ycd_is_recognised_from_its_path() {
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("move_m@generic.ycd")),
        Some(DocumentKind::Ycd)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("MOVE_M@GENERIC.YCD")),
        Some(DocumentKind::Ycd),
        "extension match must be case-insensitive"
    );
    assert_eq!(DocumentKind::Ycd.extension(), "ycd");
    assert_eq!(DocumentKind::Ycd.label(), "YCD");
}

#[test]
fn test_documentkind_from_path() {
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("foo.YPT")),
        Some(DocumentKind::Ypt)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("foo.ydr")),
        Some(DocumentKind::Ydr)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("clothing.ydd")),
        Some(DocumentKind::Ydd)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("CLOTHING.YDD")),
        Some(DocumentKind::Ydd),
        "extension match must be case-insensitive"
    );
    assert_eq!(DocumentKind::Ydd.extension(), "ydd");
    assert_eq!(DocumentKind::Ydd.label(), "YDD");
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("uppr_000_u.yld")),
        Some(DocumentKind::Yld)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("UPPR_000_U.YLD")),
        Some(DocumentKind::Yld),
        "extension match must be case-insensitive"
    );
    assert_eq!(DocumentKind::Yld.extension(), "yld");
    assert_eq!(DocumentKind::Yld.label(), "YLD");
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("foo.meta")),
        Some(DocumentKind::Meta),
        "meta should map to Meta"
    );
    for ext in ["ymap", "ytyp", "YMAP"] {
        assert_eq!(
            DocumentKind::from_path(std::path::Path::new(&format!("map.{ext}"))),
            Some(DocumentKind::Meta),
            "a .{ext} is metadata the editor opens"
        );
    }
    for ext in ["xml", "ide", "ipl", "ini", "txt", "log", "json"] {
        let path_str = format!("foo.{ext}");
        assert_eq!(
            DocumentKind::from_path(std::path::Path::new(&path_str)),
            Some(DocumentKind::Text),
            "{ext} should map to Text"
        );
    }
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("foo.XML")),
        Some(DocumentKind::Text)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("foo.META")),
        Some(DocumentKind::Meta)
    );
    assert_eq!(
        DocumentKind::from_path(std::path::Path::new("foo.unknownext")),
        None
    );
}

#[test]
fn a_cut_opens_in_the_metadata_editor() {
    let path = std::path::Path::new("fbi_4_int.CUT");
    assert_eq!(DocumentKind::from_path(path), Some(DocumentKind::Meta));
    let Some(fixture) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "cut")
        .find()
        .into_iter()
        .next()
    else {
        or_test_support::skip_corpus("a_cut_opens_in_the_metadata_editor", "no .cut fixture");
        return;
    };
    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
    let (payload, _) =
        document_io::parse_payload_from_path(&fixture, DocumentKind::Meta, crypto).expect("opens");
    let DocPayload::Meta(meta) = payload else {
        panic!("{} opened as {:?}", fixture.display(), payload.kind());
    };
    assert_eq!(meta.parse_error, None, "{}", fixture.display());
}

#[test]
fn a_text_dat_opens_as_text_and_a_binary_one_as_hex_refused_by_name() {
    let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
    let text = std::path::Path::new("popcycle.dat");
    assert_eq!(DocumentKind::from_path(text), Some(DocumentKind::Text));
    let payload = document_io::parse_payload(
        text,
        DocumentKind::Text,
        b"POP_SCHEDULES\n\tSCHEDULE 0\n",
        std::sync::Arc::clone(&crypto),
    )
    .expect("a text table opens");
    assert!(matches!(payload, DocPayload::Text(_)));

    let binary = b"HMAP\x01\0\0\0\x10\0\0\0\xff\xfe";
    let payload = document_io::parse_payload(
        std::path::Path::new("table.dat"),
        DocumentKind::Text,
        binary,
        std::sync::Arc::clone(&crypto),
    )
    .expect("a binary table opens as bytes");
    let DocPayload::Hex(hex) = payload else {
        panic!("a binary table opened as {:?}", payload.kind());
    };
    assert!(hex.banner.contains("table.dat"), "{}", hex.banner);

    let dir = tempfile::tempdir().expect("tempdir");
    let on_disk = dir.path().join("table.dat");
    std::fs::write(&on_disk, binary).expect("write");
    let (payload, _) = document_io::parse_payload_from_path(&on_disk, DocumentKind::Text, crypto)
        .expect("a binary table on disk opens as bytes");
    let DocPayload::Hex(hex) = payload else {
        panic!("a binary table on disk opened as {:?}", payload.kind());
    };
    assert!(hex.banner.contains("table.dat"), "{}", hex.banner);
}

#[test]
fn test_save_entry_backed_doc_writes_to_parent_rpf_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent_path = dir.path().join("parent.rpf");
    let mut state = fresh_state();
    let mut session = RpfEditSession::empty();
    let root = session.root_id();
    // ⚠ **A text child, not a `.ypt`, and the swap is the point.** This test
    // is about the entry binding — that saving a document opened from inside
    // an archive writes back into its parent archive, chosen as its destination,
    // and nothing in it is about a particle list.
    //
    // A format's payload would tie it to that format's save path: a save that
    // refused would leave the parent unwritten, and the test would fail for a
    // reason that has nothing to do with what it measures.
    let entry_id = session
        .add_file_copy(root, "child.txt", b"before".to_vec())
        .expect("add child");
    let parent_id = state.alloc_doc_id();
    let mut parent = Document::new(
        parent_id,
        Some(parent_path.clone()),
        DocPayload::Rpf(RpfDocument {
            session,
            expanded: Default::default(),
            focused_node: None,
            is_game_root: false,
            entry_documents: Default::default(),
        }),
    );
    parent.dirty = false;
    state.documents.push(parent);

    let child_id = state.alloc_doc_id();
    let mut child = Document::new(
        child_id,
        Some(PathBuf::from("parent.rpf/child.txt")),
        DocPayload::Text(crate::state::document::TextDocument::decode(b"after")),
    );
    child.rpf_entry_binding = Some(RpfEntryBinding {
        parent_doc_id: parent_id,
        entry_id,
    });
    child.mark_edited();
    state.documents.push(child);
    state.active_document = Some(child_id);

    assert!(state.save_active_document());
    state.choose_save_destination(save_destination::SaveDestination::Archive);

    let parent_after = state.document(parent_id).expect("parent present");
    let DocPayload::Rpf(rpf) = &parent_after.payload else {
        panic!("expected rpf payload");
    };
    let updated = rpf.session.bytes_for(entry_id).expect("bytes updated");
    assert!(!updated.is_empty(), "saved payload should not be empty");
    assert!(parent_path.exists(), "the parent archive is written");

    let child_after = state.document(child_id).expect("child present");
    assert!(
        !child_after.dirty,
        "entry-backed child should be clean after save"
    );
}

/// The app answers a path question that crosses two archive boundaries.
///
/// ⚠ **The wiring is the thing under test, not the glob.** `vfs_session`'s
/// own suite checks the walk; this checks that `deep_find_tick` finds the
/// document, recognises the payload and mounts the archive the session
/// already holds — the three steps that can each silently answer nothing and
/// leave an empty result looking like an empty archive.
#[test]
fn a_deep_find_through_app_state_reaches_a_nested_archive() {
    use or_io::container::rpf::RpfBuilder;

    fn archive_of(entries: &[(&str, &[u8])]) -> RpfBuilder {
        let mut builder = RpfBuilder::default();
        for (path, bytes) in entries {
            builder.insert_with(path, *bytes, false).expect("add");
        }
        builder
    }

    let inner = archive_of(&[("deep.ydr", b"three")])
        .to_bytes()
        .expect("save");
    let mid = archive_of(&[("mid.ydr", b"two"), ("inner.rpf", &inner)])
        .to_bytes()
        .expect("save");
    let archive = archive_of(&[("top.ydr", b"one"), ("mid.rpf", &mid)])
        .build()
        .expect("opens");
    let session = RpfEditSession::from_archive(archive).expect("session");

    let mut state = fresh_state();
    let doc_id = state.alloc_doc_id();
    state.documents.push(Document::new(
        doc_id,
        Some(PathBuf::from("built.rpf")),
        DocPayload::Rpf(RpfDocument {
            session,
            expanded: Default::default(),
            focused_node: None,
            is_game_root: false,
            entry_documents: Default::default(),
        }),
    ));

    for _ in 0..300 {
        state.deep_find_tick(doc_id, "**/*.ydr");
        let found = state.deep_find.found_for(doc_id, "**/*.ydr");
        if found.is_some_and(|found| !found.indexing) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let found = state
        .deep_find
        .found_for(doc_id, "**/*.ydr")
        .expect("the find never came back");
    let mut matches = found.matches().to_vec();
    matches.sort();
    assert_eq!(
        matches,
        vec![
            "doc:/mid.rpf/inner.rpf/deep.ydr",
            "doc:/mid.rpf/mid.ydr",
            "doc:/top.ydr",
        ],
        "the app reached every depth"
    );
    assert!(found.is_complete());

    // ⚠ The half that makes the three above mean something: two of them name
    // files with no node in the session, because nothing expanded `mid.rpf`.
    let DocPayload::Rpf(rpf) = &state.document(doc_id).expect("present").payload else {
        panic!("expected an rpf payload");
    };
    let node_names: Vec<String> = rpf
        .session
        .all_nodes()
        .into_iter()
        .filter_map(|id| rpf.session.node(id).map(|n| n.name.clone()))
        .collect();
    assert!(
        !node_names.iter().any(|n| n == "deep.ydr"),
        "the session expanded the nested archive by itself, so this no longer \
         distinguishes a VFS walk from a tree walk: {node_names:?}"
    );
}

#[test]
fn test_serialize_rpf_payload_roundtrip() {
    let mut session = RpfEditSession::empty();
    let root = session.root_id();
    session
        .add_file_copy(root, "a.bin", b"hello".to_vec())
        .expect("insert file");
    let bytes = serialize_payload(
        &DocPayload::Rpf(RpfDocument {
            session,
            expanded: Default::default(),
            focused_node: None,
            is_game_root: false,
            entry_documents: Default::default(),
        }),
        RpfOutputMode::Open,
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new()),
    )
    .expect("serialize rpf");
    let archive = or_io::container::rpf::RpfArchive::from_bytes(bytes).expect("reload");
    assert_eq!(archive.file_count(), 1);
}

const UI_TYPED_BY_DESIGN: &[(&str, &str)] = &[
    (
        "active_workspace",
        "which preset the editor is in — read by document activation, the dock \
         and the tick, none of which is a panel",
    ),
    (
        "workspace_docks",
        "persisted dock layouts, one per workspace. Saved to disk and restored \
         at startup, so it outlives every panel it positions",
    ),
    (
        "panels",
        "the store itself. This is where a panel's state goes instead of here",
    ),
];

fn type_names_declared_in_ui() -> std::collections::BTreeSet<String> {
    fn walk(dir: &Path, out: &mut std::collections::BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                for kw in [
                    "pub struct ",
                    "pub enum ",
                    "pub type ",
                    "pub(crate) struct ",
                ] {
                    let Some(rest) = line.strip_prefix(kw) else {
                        continue;
                    };
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.insert(name);
                    }
                }
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("ui"),
        &mut out,
    );
    out
}

fn app_state_fields() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("state")
            .join("mod.rs"),
    )
    .expect("src/state/mod.rs");
    let start = text
        .find("pub struct AppState {")
        .expect("`pub struct AppState {` — if this was renamed, so was this test");
    let body = &text[start..];

    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut field = String::new();
    for line in body.lines().skip(1) {
        let trimmed = line.trim();
        if depth == 0 && trimmed == "}" {
            break;
        }
        if trimmed.starts_with("//") || trimmed.starts_with("#[") {
            continue;
        }
        field.push_str(trimmed);
        depth += trimmed.matches(['<', '(']).count() as i32;
        depth -= trimmed.matches(['>', ')']).count() as i32;
        if depth > 0 || !field.ends_with(',') {
            field.push(' ');
            continue;
        }
        let decl = field.trim_end_matches(',').to_string();
        field.clear();
        let Some((name, ty)) = decl.split_once(':') else {
            continue;
        };
        let name = name.trim().trim_start_matches("pub(super) ").trim();
        let name = name.trim_start_matches("pub ").trim();
        out.push((name.to_string(), ty.trim().to_string()));
    }
    out
}

fn ui_typed_fields<'a>(
    fields: &'a [(String, String)],
    ui_types: &std::collections::BTreeSet<String>,
) -> Vec<(&'a str, &'a str, String)> {
    fields
        .iter()
        .filter_map(|(name, ty)| {
            let hit = ui_types.iter().find(|t| {
                ty.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .any(|word| word == t.as_str())
            })?;
            Some((name.as_str(), ty.as_str(), hit.clone()))
        })
        .collect()
}

#[test]
fn the_detector_finds_the_fields_that_were_actually_there() {
    let removed: Vec<(String, String)> = [
        (
            "settings_active_category",
            "crate::ui::panels::settings::SettingsCategory",
        ),
        ("settings_query", "String"),
        (
            "cross_doc_find",
            "crate::ui::panels::text_editor::find_panel::CrossDocFindState",
        ),
        ("anim_timeline_state", "crate::ui::timeline::TimelineState"),
        (
            "hierarchy_state",
            "crate::ui::panels::hierarchy::HierarchyUiState",
        ),
        (
            "inspector_state",
            "crate::ui::panels::inspector::InspectorUiState",
        ),
        (
            "timeline_state",
            "crate::ui::panels::timeline::TimelineUiState",
        ),
        ("viewport_state", "ViewportUiState"),
        ("settings_open", "bool"),
        ("about_open", "bool"),
        ("command_palette_open", "bool"),
    ]
    .into_iter()
    .map(|(n, t)| (n.to_string(), t.to_string()))
    .collect();

    let mut ui_types = type_names_declared_in_ui();
    ui_types.insert("ViewportUiState".to_string());

    let flagged = ui_typed_fields(&removed, &ui_types);
    let names: Vec<&str> = flagged.iter().map(|(n, _, _)| *n).collect();
    println!(
        "detector flagged {} of {}: {names:?}",
        flagged.len(),
        removed.len()
    );

    for expected in [
        "settings_active_category",
        "cross_doc_find",
        "anim_timeline_state",
        "hierarchy_state",
        "inspector_state",
        "timeline_state",
        "viewport_state",
    ] {
        assert!(
            names.contains(&expected),
            "the gate would not have caught `{expected}`, so it is not \
             evidence about the seven it does claim to catch. flagged: {names:?}"
        );
    }
    for invisible in [
        "settings_query",
        "settings_open",
        "about_open",
        "command_palette_open",
    ] {
        assert!(
            !names.contains(&invisible),
            "`{invisible}` is a primitive and this gate has never been able to \
             see one. If that has changed, the doc comment above is now wrong."
        );
    }
    assert_eq!(flagged.len(), 7, "flagged: {names:?}");
}

#[test]
fn no_panel_reserves_a_field_on_app_state() {
    let ui_types = type_names_declared_in_ui();
    let fields = app_state_fields();
    assert!(
        fields.len() > 30,
        "parsed {} field(s) out of AppState, which cannot be right — the \
         parser is looking at the wrong text and would pass on anything",
        fields.len()
    );
    assert!(
        ui_types.len() > 50,
        "found {} type(s) declared under src/ui/, which is too few to be the \
         UI layer",
        ui_types.len()
    );

    let flagged = ui_typed_fields(&fields, &ui_types);
    let allowed_seen = flagged
        .iter()
        .filter(|(name, _, _)| UI_TYPED_BY_DESIGN.iter().any(|(f, _)| f == name))
        .count();
    let breaches: Vec<String> = flagged
        .iter()
        .filter(|(name, _, _)| !UI_TYPED_BY_DESIGN.iter().any(|(f, _)| f == name))
        .map(|(name, ty, hit)| format!("  {name}: {ty}  (`{hit}` is declared under src/ui/)"))
        .collect();

    println!(
        "AppState: {} field(s); {} declared under src/ui/ and allowed by name; \
         {} type(s) declared under src/ui/",
        fields.len(),
        allowed_seen,
        ui_types.len()
    );

    assert!(
        breaches.is_empty(),
        "docs/reference/Crate_Reference.md: `AppState` holds application state; no panel \
         reserves a field on it.\n{}\n\n\
         Put it in `crate::ui::panel_state` keyed by the surface instead — \
         `PanelStore::get` for a value touched at one point, `take`/`put` for \
         one interleaved with `state` across a draw. If it genuinely is \
         application state that happens to be spelled in the UI layer, add it \
         to UI_TYPED_BY_DESIGN with the reason.",
        breaches.join("\n")
    );

    let stale: Vec<&str> = UI_TYPED_BY_DESIGN
        .iter()
        .map(|(f, _)| *f)
        .filter(|f| !fields.iter().any(|(name, _)| name == f))
        .collect();
    assert!(
        stale.is_empty(),
        "UI_TYPED_BY_DESIGN names field(s) AppState no longer has: {stale:?}. \
         Delete the entries."
    );
}

#[test]
fn the_texture_cache_budget_preference_moves_the_eviction_threshold() {
    let mut state = fresh_state();
    state.set_texture_cache_budget_mib(128);
    assert_eq!(state.prefs.texture_cache_mib, 128);
    assert_eq!(state.texture_cache.budget(), 128 * 1024 * 1024);
    state.set_texture_cache_budget_mib(2048);
    assert_eq!(state.texture_cache.budget(), 2048 * 1024 * 1024);
}
