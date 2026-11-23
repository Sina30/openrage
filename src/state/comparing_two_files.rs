//! Two `.ymt` files compared from their rows differ by record and field, a
//! texture dictionary in the mods overlay compares with the game's own, and
//! two texts line up side by side; each comparison opens as a tab of its own.

use std::path::Path;
use std::time::{Duration, Instant};

use or_io::meta::{MetaInstance, Value};

use super::compare::{By, Change, Comparison, FieldChange, Found};
use super::jobs::JobKind;
use super::opening_a_ydd::node_at;
use super::{AppState, DocPayload, DocumentId, DocumentKind, Prefs, RpfNodeId};
use crate::ui::commands::{self, Action, Target};

pub(super) fn state() -> AppState {
    AppState::from_prefs(Prefs::default())
}

/// The command `action` on `row` of `doc`, as the row's menu offers it.
pub(super) fn command(
    state: &AppState,
    action: Action,
    doc: DocumentId,
    row: RpfNodeId,
) -> commands::Command {
    let target = Target::rows(state, doc, vec![row], false).expect("a row of an open tree");
    commands::on(state, action, target).expect("a command that acts on a row")
}

/// Run `action` on `row` of `doc` from its menu, and wait for the comparison it starts.
pub(super) fn run(state: &mut AppState, action: Action, doc: DocumentId, row: RpfNodeId) {
    let command = command(state, action, doc, row);
    assert_eq!(command.enabled, Ok(()), "{action:?} is not offered");
    commands::run(state, &egui::Context::default(), &command);
    let deadline = Instant::now() + Duration::from_secs(60);
    while state
        .running_jobs()
        .iter()
        .any(|job| job.kind == JobKind::Compare)
    {
        assert!(Instant::now() < deadline, "the comparison did not end");
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The comparison the active tab shows, and its tab's title.
pub(super) fn shown(state: &AppState) -> (Comparison, String) {
    let doc = state.active_document().expect("a document is active");
    assert_eq!(
        doc.kind,
        DocumentKind::Compare,
        "the active tab is not a comparison"
    );
    assert!(
        state.tab_order().contains(&doc.id),
        "the comparison has no tab"
    );
    let DocPayload::Compare(compared) = &doc.payload else {
        unreachable!("a comparison's payload");
    };
    (compared.clone(), state.tab_title(doc))
}

pub(super) fn fields(compared: &Comparison) -> (By, Vec<FieldChange>) {
    match &compared.found {
        Found::Fields { by, changes, .. } => (*by, changes.clone()),
        other => panic!("not compared field by field: {other:?}"),
    }
}

/// The field a record goes by, as the comparison matches records: its first hash or text.
fn key_of(record: &MetaInstance) -> Option<usize> {
    record.fields.iter().position(|(_, value)| match value {
        Value::Hash(_) => true,
        Value::String(text) => !text.is_empty(),
        _ => false,
    })
}

/// A binary metadata file with a root array of at least three records, each
/// going by a name of its own, the first with a whole number to change.
struct Ymt {
    bytes: Vec<u8>,
    instance: MetaInstance,
    layout: Option<or_io::meta::binary::MetaLayout>,
    format: or_io::meta::BinaryFormat,
    array: String,
    number: String,
}

fn a_ymt_with_records() -> Option<Ymt> {
    let meta = or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        "OPENRAGE_META_CORPUS",
        "binary",
    );
    for path in or_test_support::Corpus::with_root(meta.path(), "ymt").find() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(format) = or_io::meta::detect_binary_format(&bytes) else {
            continue;
        };
        let doc = super::MetaDocument::from_bytes(bytes.clone());
        let Some(parsed) = doc.parsed else {
            continue;
        };
        for (array, value) in &parsed.instance.fields {
            let Value::Array(items) = value else {
                continue;
            };
            let records: Vec<&MetaInstance> = (items.iter())
                .filter_map(|item| match item {
                    Value::Struct(record) => Some(&**record),
                    _ => None,
                })
                .collect();
            if records.len() < 3 || records.len() != items.len() {
                continue;
            }
            let keys: Vec<String> = (records.iter())
                .filter_map(|r| key_of(r).map(|at| format!("{:?}", r.fields[at].1)))
                .collect();
            let distinct: std::collections::HashSet<&String> = keys.iter().collect();
            if keys.len() != records.len() || distinct.len() != keys.len() {
                continue;
            }
            let number = (records[0].fields.iter())
                .find(|(_, v)| matches!(v, Value::Int(_) | Value::UInt(_)))
                .map(|(name, _)| name.clone());
            let Some(number) = number else {
                continue;
            };
            if records[0]
                .fields
                .iter()
                .filter(|(n, _)| *n == number)
                .count()
                != 1
            {
                continue;
            }
            return Some(Ymt {
                bytes,
                instance: parsed.instance.clone(),
                layout: doc.layout,
                format,
                array: array.clone(),
                number,
            });
        }
    }
    None
}

fn record_mut<'a>(instance: &'a mut MetaInstance, array: &str) -> &'a mut Vec<Value> {
    match instance.get_mut(array) {
        Some(Value::Array(items)) => items,
        _ => unreachable!("chosen for its array"),
    }
}

#[test]
fn two_ymts_compared_from_their_rows_differ_by_record_and_field() {
    const WHAT: &str = "two .ymt files compared by record and field";
    let Some(ymt) = a_ymt_with_records() else {
        or_test_support::skip_corpus(WHAT, "no binary .ymt with a root array of named records");
        return;
    };
    // ⚠ Edited in place, as the file's own layout lays it out: a PSO layout
    // takes no longer string and no record moved into a shorter one's slot.
    let mut edited = ymt.instance.clone();
    let records = record_mut(&mut edited, &ymt.array);
    let Value::Struct(first) = &mut records[0] else {
        unreachable!("a record");
    };
    let before = first.get(&ymt.number).cloned().expect("the number");
    let after = match &before {
        Value::Int(n) => Value::Int(n.wrapping_add(1)),
        Value::UInt(n) => Value::UInt(n.wrapping_add(1)),
        _ => unreachable!("a whole number"),
    };
    first.set(ymt.number.clone(), after.clone());
    let Value::Struct(second) = &mut records[1] else {
        unreachable!("a record");
    };
    let at = key_of(second).expect("named");
    second.fields[at].1 = match &second.fields[at].1 {
        Value::Hash(_) => Value::Hash(or_io::meta::instance::HashedString::new("compare_renamed")),
        Value::String(name) => {
            let mut renamed = name.clone();
            let last = renamed.pop();
            renamed.push(if last == Some('X') { 'Y' } else { 'X' });
            Value::String(renamed)
        }
        _ => unreachable!("a name"),
    };
    let edited = or_io::meta::write_meta_binary(ymt.format, ymt.layout.as_ref(), &edited)
        .expect("the edited tree saves");

    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(dir.path().join("a.ymt"), &ymt.bytes).expect("a.ymt");
    std::fs::write(dir.path().join("b.ymt"), edited).expect("b.ymt");
    let mut state = state();
    state.open_folder(dir.path().to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let (a, b) = (
        node_at(&mut state, folder, "a.ymt"),
        node_at(&mut state, folder, "b.ymt"),
    );

    assert_eq!(
        command(&state, Action::CompareWithSelected, folder, b).enabled,
        Err("Select for compare on another file first")
    );
    run(&mut state, Action::SelectForCompare, folder, a);
    assert_eq!(
        command(&state, Action::CompareWithSelected, folder, a).enabled,
        Err("This is the file selected for compare: choose another")
    );
    assert_eq!(
        command(&state, Action::CompareWithSelected, folder, b).label,
        "Compare with a.ymt"
    );
    run(&mut state, Action::CompareWithSelected, folder, b);

    let (compared, title) = shown(&state);
    assert_eq!(title, "a.ymt ↔ b.ymt");
    assert_eq!(compared.headings, ["a.ymt", "b.ymt"]);
    let (by, changes) = fields(&compared);
    assert_eq!(by, By::RecordAndField);
    let found: Vec<(String, String, Change)> = (changes.iter())
        .map(|c| (c.record.clone(), c.field.clone(), c.change))
        .collect();
    let array = &ymt.array;
    assert_eq!(
        found,
        vec![
            (format!("{array}[0]"), ymt.number.clone(), Change::Changed),
            (format!("{array}[1]"), String::new(), Change::Removed),
            (format!("{array}[1]"), String::new(), Change::Added),
        ],
        "a renamed record is another record: {changes:#?}"
    );
    let shown_number = |v: &Value| match v {
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        _ => unreachable!(),
    };
    assert_eq!(
        (changes[0].left.clone(), changes[0].right.clone()),
        (shown_number(&before), shown_number(&after)),
        "a changed field shows both values"
    );
    assert!(!changes[1].left.is_empty() && changes[1].right.is_empty());
    assert!(changes[2].left.is_empty() && !changes[2].right.is_empty());
}

/// A Legacy dictionary of two textures or more, its bytes and its value.
fn a_dictionary() -> Option<(
    Vec<u8>,
    or_io::gta5::PgDictionary<or_io::gta5::gen8::GrcTexture>,
)> {
    let mut files = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ytd").find();
    files.sort_by_key(|path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX));
    files.into_iter().find_map(|path| {
        let bytes = std::fs::read(&path).ok()?;
        match or_io::from_bytes(&bytes) {
            Ok(or_io::Asset::TextureDict(dict)) if dict.entries.iter().flatten().count() >= 2 => {
                Some((bytes, dict))
            }
            _ => None,
        }
    })
}

/// An unencrypted archive holding `files`, written at `path`.
pub(super) fn archive(path: &Path, files: &[(&str, &[u8])]) {
    use or_io::container::rpf::RpfBuilder;
    let mut builder = RpfBuilder::default();
    for (inner, bytes) in files {
        builder.insert_with(inner, *bytes, false).expect("an entry");
    }
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("its folder");
    builder.save(path).expect("the archive saves");
}

#[test]
fn a_texture_dictionary_in_the_overlay_compares_with_the_games_and_a_file_with_none_says_so() {
    use or_io::gta5::File as _;
    const WHAT: &str = "a texture dictionary in the mods overlay compared with the game's";
    let Some((shipped, dict)) = a_dictionary() else {
        or_test_support::skip_corpus(WHAT, "no Legacy .ytd of two textures or more");
        return;
    };
    let names: Vec<String> = (dict.entries.iter().flatten())
        .map(|t| t.name.clone())
        .collect();
    let mut modded = dict.clone();
    let first = modded.get_mut(&names[0]).expect("the first texture");
    first.backing_store.iter_mut().for_each(|b| *b = !*b);
    let mut added = first.clone();
    added.name = "compare_added".into();
    let gone = modded
        .entries
        .iter()
        .position(|t| t.as_ref().is_some_and(|t| t.name == names[1]));
    let gone = gone.expect("the second texture");
    modded.codes.remove(gone);
    modded.entries.remove(gone);
    modded.insert(added);
    let modded = modded.to_bytes().expect("the edited dictionary saves");

    let root = tempfile::tempdir().expect("a scratch install");
    std::fs::write(root.path().join("gta5.exe"), b"").expect("the marker an install has");
    const INNER: &str = "x64/textures/compare.ytd";
    archive(&root.path().join("update/update.rpf"), &[(INNER, &shipped)]);
    let overlay = root.path().join("mods/update/update.rpf");
    archive(
        &overlay,
        &[
            (INNER, &modded),
            ("x64/textures/only_in_mods.ytd", &shipped),
        ],
    );

    let mut state = state();
    let game = state.open_game_root(root.path().to_path_buf());
    let shipped_row = node_at(&mut state, game, &format!("update/update.rpf/{INNER}"));
    assert_eq!(
        command(&state, Action::CompareWithOriginal, game, shipped_row).enabled,
        Err("Only a file or an archive in the mods overlay has an original to compare with")
    );
    let modded_row = node_at(&mut state, game, &format!("mods/update/update.rpf/{INNER}"));
    run(&mut state, Action::CompareWithOriginal, game, modded_row);

    let (compared, title) = shown(&state);
    assert_eq!(title, "compare.ytd — game ↔ mods");
    assert_eq!(compared.headings, ["Game", "Mods overlay"]);
    assert!(
        compared.places[0].ends_with(&INNER.replace('/', "\\"))
            && !compared.places[0].contains("mods"),
        "the left is the game's own copy: {:?}",
        compared.places
    );
    let (by, changes) = fields(&compared);
    assert_eq!(by, By::Texture);
    let lower = |name: &str| name.to_ascii_lowercase();
    let found: Vec<(String, String, Change)> = (changes.iter())
        .map(|c| (lower(&c.record), c.field.clone(), c.change))
        .collect();
    let mut expected = vec![
        (lower(&names[0]), "Pixels".to_string(), Change::Changed),
        (lower(&names[1]), String::new(), Change::Removed),
        ("compare_added".to_string(), String::new(), Change::Added),
    ];
    expected.sort_by_key(|(name, _, _)| name.clone());
    assert_eq!(found, expected, "{changes:#?}");

    let documents = state.documents.len();
    let only = node_at(
        &mut state,
        game,
        "mods/update/update.rpf/x64/textures/only_in_mods.ytd",
    );
    run(&mut state, Action::CompareWithOriginal, game, only);
    assert_eq!(
        state.documents.len(),
        documents,
        "a file with no original opened a comparison"
    );
    let said = (state.jobs.ended().iter())
        .find(|job| job.kind == JobKind::Compare)
        .map(|job| job.said.clone());
    assert_eq!(
        said.as_deref(),
        Some("The game has no only_in_mods.ytd of its own: the mods overlay adds it")
    );
}

#[test]
fn two_texts_line_up_side_by_side_and_previous_and_next_step_between_their_changes() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let left = "<a>\n  <b>1</b>\n  <c>2</c>\n  <d>3</d>\n</a>\n";
    let right = "<a>\n  <b>5</b>\n  <c>2</c>\n  <d>3</d>\n  <e/>\n</a>\n";
    std::fs::write(dir.path().join("left.xml"), left).expect("left.xml");
    std::fs::write(dir.path().join("right.xml"), right).expect("right.xml");
    let mut state = state();
    state.open_folder(dir.path().to_path_buf());
    let folder = state.active_document.expect("the folder opens");
    let (a, b) = (
        node_at(&mut state, folder, "left.xml"),
        node_at(&mut state, folder, "right.xml"),
    );
    run(&mut state, Action::SelectForCompare, folder, a);
    run(&mut state, Action::CompareWithSelected, folder, b);

    let (compared, title) = shown(&state);
    assert_eq!(title, "left.xml ↔ right.xml");
    let Found::Lines(lines) = &compared.found else {
        panic!("not compared line by line: {:?}", compared.found);
    };
    let changed: Vec<(Option<usize>, Option<usize>, Change)> = (lines.iter())
        .filter_map(|l| {
            Some((
                l.left.as_ref().map(|s| s.0),
                l.right.as_ref().map(|s| s.0),
                l.change?,
            ))
        })
        .collect();
    assert_eq!(
        changed,
        vec![
            (Some(2), Some(2), Change::Changed),
            (None, Some(5), Change::Added)
        ]
    );
    assert_eq!(lines.len(), 6, "every line of both, the shared ones once");
    assert_eq!(
        compared.stops(),
        vec![1, 4],
        "a step lands on each run of changes"
    );
    for save in [Action::Save, Action::SaveAs] {
        let command = commands::of(&state, save).expect("a command");
        assert_eq!(
            command.enabled,
            Err("A comparison is shown, never saved"),
            "{save:?}"
        );
    }
}
