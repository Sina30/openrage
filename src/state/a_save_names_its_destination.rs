//! Saving a document opened from an archive asks where it goes, and writes only there.
//!
//! ⚠ Each case edits the same entry and varies only the destination, so a save
//! that wrote somewhere else, or everywhere, fails one of them.

use std::path::{Path, PathBuf};

use or_io::gta5::text::Gxt2;
use or_io::gta5::File as _;

use super::document::{Document, DocumentId, RpfNodeId, TextDocument};
use super::prefs::{EditMode, ExportFixes};
use super::rpf_session::{RpfEditSession, RpfEntryBinding};
use super::save_destination::SaveDestination;
use super::{AppState, DocPayload, DocumentKind, RpfNodeBacking, RpfOutputMode};

const SHIPPED: &[u8] = b"shipped";
const EDITED: &[u8] = b"edited";

struct Fixture {
    state: AppState,
    entry_doc: DocumentId,
    archive: PathBuf,
    overlay: PathBuf,
}

/// An install at `root` mounted with `update/update.rpf` holding `name`, and that entry open with `edited` bytes.
fn mounted(root: &Path, install: bool, mode: EditMode, name: &str, shipped: &[u8]) -> Fixture {
    std::fs::create_dir_all(root.join("update")).expect("install dirs");
    if install {
        std::fs::write(root.join("gta5.exe"), b"an install marker").expect("marker");
    }
    let archive_path = root.join("update").join("update.rpf");

    let mut state = AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = mode;
    state.prefs.export_fixes = ExportFixes::Ask;
    let bytes = super::document_io::serialize_archive_records(
        vec![(name.to_string(), shipped.to_vec(), false)],
        RpfOutputMode::Open,
        state.keys.provider(),
    )
    .expect("archive bytes");
    std::fs::write(&archive_path, bytes).expect("archive");
    let archive =
        super::rpf_archive_open::open_archive_from_disk(&archive_path, state.keys.provider())
            .expect("open the archive");

    state.open_game_root(root.to_path_buf());
    let game = state.documents[0].id;
    let entry = {
        let Some(DocPayload::Rpf(rpf)) = state.document_mut(game).map(|d| &mut d.payload) else {
            panic!("the install mounts as a GameRoot");
        };
        let root_id = rpf.session.root_id();
        let node = rpf
            .session
            .add_unopened_disk_archive(root_id, "update.rpf".to_string(), archive_path.clone())
            .expect("the archive's node");
        let id = rpf
            .session
            .register_archive(archive.clone(), Some(archive_path.clone()))
            .expect("an id for the archive");
        if let Some(RpfNodeBacking::Archive { archive_id, .. }) =
            rpf.session.node_mut(node).map(|n| &mut n.backing)
        {
            *archive_id = Some(id);
        }
        let listing = super::expander::list_archive(archive).expect("list");
        rpf.session
            .populate_archive_children(node, id, &listing)
            .expect("populate");
        *rpf.session.children_of(node).last().expect("the entry")
    };

    let entry_doc = open_entry(&mut state, game, entry, name);
    Fixture {
        state,
        entry_doc,
        overlay: root.join("mods").join("update").join("update.rpf"),
        archive: archive_path,
    }
}

fn open_entry(state: &mut AppState, game: DocumentId, entry: RpfNodeId, name: &str) -> DocumentId {
    let id = state.alloc_doc_id();
    let mut doc = Document::new(
        id,
        Some(PathBuf::from("update.rpf").join(name)),
        DocPayload::Text(TextDocument::decode(EDITED)),
    );
    doc.rpf_entry_binding = Some(RpfEntryBinding {
        parent_doc_id: game,
        entry_id: entry,
    });
    doc.mark_edited();
    state.documents.push(doc);
    state.active_document = Some(id);
    id
}

/// The entry's document as it opens, never edited, holding `payload`.
fn reopen_unedited(f: &mut Fixture, payload: DocPayload) {
    let doc = f.state.document_mut(f.entry_doc).expect("entry");
    let binding = doc.rpf_entry_binding;
    *doc = Document::new(doc.id, doc.path.clone(), payload);
    doc.rpf_entry_binding = binding;
}

/// The bytes `name` holds in the archive at `path`, read back from disk.
fn entry_on_disk(state: &AppState, path: &Path, name: &str) -> Vec<u8> {
    let archive = super::rpf_archive_open::open_archive_from_disk(path, state.keys.provider())
        .expect("the archive reopens");
    let session = RpfEditSession::from_archive((*archive).clone()).expect("the archive lists");
    let id = session
        .children_of(session.root_id())
        .into_iter()
        .find(|id| session.node(*id).is_some_and(|n| n.name == name))
        .expect("the entry is still there");
    session.bytes_for(id).expect("the entry reads").to_vec()
}

fn save(f: &mut Fixture, dest: SaveDestination) {
    assert!(f.state.save_active_document());
    let choice = f
        .state
        .save_destination
        .clone()
        .expect("saving an archive entry asks where it goes");
    assert_eq!(choice.doc_id, f.entry_doc);
    f.state.choose_save_destination(dest);
    assert!(f.state.save_destination.is_none());
}

#[test]
fn saving_an_archive_entry_offers_the_three_destinations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut f = mounted(tmp.path(), true, EditMode::Overlay, "a.txt", SHIPPED);
    let before = std::fs::read(&f.archive).expect("archive");

    assert!(f.state.save_active_document());
    let choice = f
        .state
        .save_destination
        .clone()
        .expect("saving an archive entry asks where it goes");
    assert_eq!(choice.name, "a.txt");
    assert_eq!(
        choice.archive.as_ref(),
        Some(&f.archive),
        "back into the archive"
    );
    assert_eq!(
        choice.overlay.as_ref(),
        Some(&f.overlay),
        "into the mods overlay"
    );
    assert_eq!(std::fs::read(&f.archive).expect("archive"), before);
    assert!(
        !f.overlay.exists(),
        "nothing is written before the user answers"
    );
    assert!(
        f.state.document(f.entry_doc).expect("the entry").dirty,
        "the edit is not saved anywhere yet"
    );

    f.state.cancel_save_destination();
    assert!(f.state.save_destination.is_none());
    assert_eq!(std::fs::read(&f.archive).expect("archive"), before);
}

#[test]
fn the_archive_destination_writes_the_archive() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut f = mounted(tmp.path(), false, EditMode::ReadOnly, "a.txt", SHIPPED);

    save(&mut f, SaveDestination::Archive);

    assert_eq!(entry_on_disk(&f.state, &f.archive, "a.txt"), EDITED);
    assert!(!f.state.document(f.entry_doc).expect("the entry").dirty);
}

#[test]
fn the_loose_destination_writes_a_loose_file_and_leaves_the_archive() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut f = mounted(tmp.path(), false, EditMode::ReadOnly, "a.txt", SHIPPED);
    let before = std::fs::read(&f.archive).expect("archive");
    let loose = tmp.path().join("out").join("a.txt");
    std::fs::create_dir_all(loose.parent().expect("parent")).expect("out dir");

    save(&mut f, SaveDestination::LooseFile(loose.clone()));

    assert_eq!(std::fs::read(&loose).expect("the loose file"), EDITED);
    assert_eq!(std::fs::read(&f.archive).expect("archive"), before);
}

#[test]
fn the_overlay_destination_writes_the_overlay_copy_and_leaves_the_shipped_archive() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut f = mounted(tmp.path(), true, EditMode::Overlay, "a.txt", SHIPPED);
    let before = std::fs::read(&f.archive).expect("archive");

    save(&mut f, SaveDestination::Overlay);

    assert_eq!(entry_on_disk(&f.state, &f.overlay, "a.txt"), EDITED);
    assert_eq!(std::fs::read(&f.archive).expect("archive"), before);
    assert!(!f.state.document(f.entry_doc).expect("the entry").dirty);

    // A second save goes to the copy the first one made.
    f.state.document_mut(f.entry_doc).expect("entry").payload =
        DocPayload::Text(TextDocument::decode(b"again"));
    f.state
        .document_mut(f.entry_doc)
        .expect("entry")
        .mark_edited();
    save(&mut f, SaveDestination::Overlay);
    assert_eq!(entry_on_disk(&f.state, &f.overlay, "a.txt"), b"again");
    assert_eq!(std::fs::read(&f.archive).expect("archive"), before);
}

#[test]
fn every_destination_is_behind_the_gate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut f = mounted(tmp.path(), true, EditMode::ReadOnly, "a.txt", SHIPPED);
    let before = std::fs::read(&f.archive).expect("archive");
    let loose = tmp.path().join("update").join("a.txt");

    for dest in [
        SaveDestination::Archive,
        SaveDestination::Overlay,
        SaveDestination::LooseFile(loose.clone()),
    ] {
        save(&mut f, dest.clone());
        assert_eq!(
            std::fs::read(&f.archive).expect("archive"),
            before,
            "{dest:?} wrote the shipped archive in ReadOnly"
        );
        assert!(
            !f.overlay.exists(),
            "{dest:?} wrote the overlay in ReadOnly"
        );
        assert!(
            !loose.exists(),
            "{dest:?} wrote into the install in ReadOnly"
        );
        assert!(f.state.document(f.entry_doc).expect("the entry").dirty);
        f.state.overlay_copy.offer = None;
    }

    // `Everything` asks first, and the answer is what writes.
    f.state.prefs.edit_mode = EditMode::Everything;
    save(&mut f, SaveDestination::Archive);
    assert!(f.state.pending_retail_edit.is_some(), "the gate asks first");
    assert_eq!(std::fs::read(&f.archive).expect("archive"), before);
    f.state
        .apply_pending_retail_edit(super::RetailEditDecision::YesSession);
    assert_eq!(entry_on_disk(&f.state, &f.archive, "a.txt"), EDITED);
}

/// A table as OpenRage writes it, with four bytes after its strings that a save drops.
fn altered_gxt2() -> Vec<u8> {
    let mut table = Gxt2::new();
    table.insert_key("HELLO", "hi");
    [
        table.to_bytes().expect("a one-string table writes"),
        vec![0; 4],
    ]
    .concat()
}

/// ⚠ Only an unedited save can keep the original bytes: an edited one has
/// no bytes but the canonical form that carry the edit.
#[test]
fn a_loose_save_of_an_unedited_entry_asks_before_it_alters_the_bytes() {
    for (apply, expect_original) in [(true, false), (false, true)] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let original = altered_gxt2();
        let mut f = mounted(tmp.path(), false, EditMode::ReadOnly, "t.gxt2", &original);
        let payload = super::document_io::parse_payload(
            Path::new("t.gxt2"),
            DocumentKind::from_path(Path::new("t.gxt2")).expect("a kind"),
            &original,
            f.state.keys.provider(),
        )
        .expect("the table opens");
        reopen_unedited(&mut f, payload);
        let loose = tmp.path().join("t.gxt2");

        save(&mut f, SaveDestination::LooseFile(loose.clone()));
        assert!(!loose.exists(), "nothing is written before the answer");
        assert!(
            f.state.pending_export_fixes.is_some(),
            "the export prompt asks"
        );
        f.state.apply_export_fixes(apply, false);

        let written = std::fs::read(&loose).expect("the loose file");
        assert_eq!(
            written == original,
            expect_original,
            "applying the fixes writes the canonical form, declining the original bytes"
        );
    }
}

/// A staged mod `.ydd` whose geometries state no primitives and no vertices beside their runs.
fn incoherent_ydd() -> Option<Vec<u8>> {
    const FILE: &str = "susanoo_2.ydd";
    let mods = or_test_support::mods_corpus_origin(env!("CARGO_MANIFEST_DIR"));
    let path = (or_test_support::Corpus::with_root(mods.path(), "ydd").all_targets())
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == FILE));
    let Some(path) = path else {
        let why = format!("{FILE} is not staged in {}", mods.describe());
        or_test_support::skip_corpus("a loose save repairing a mod .ydd", why);
        return None;
    };
    Some(std::fs::read(path).expect("a corpus file reads"))
}

/// ⚠ Unasked, the save is refused rather than repaired, so the prompt is
/// the only way the repaired bytes are written.
#[test]
fn a_loose_save_of_a_ydd_needing_a_repair_asks_and_repairs_only_on_yes() {
    let Some(original) = incoherent_ydd() else {
        return;
    };
    for apply in [true, false] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut f = mounted(tmp.path(), false, EditMode::ReadOnly, "m.ydd", &original);
        let payload = super::document_io::parse_payload(
            Path::new("m.ydd"),
            DocumentKind::from_path(Path::new("m.ydd")).expect("a kind"),
            &original,
            f.state.keys.provider(),
        )
        .expect("the drawables open");
        reopen_unedited(&mut f, payload);
        let loose = tmp.path().join("m.ydd");

        save(&mut f, SaveDestination::LooseFile(loose.clone()));
        assert!(!loose.exists(), "nothing is written before the answer");
        assert!(
            f.state.pending_export_fixes.is_some(),
            "the export prompt asks"
        );
        f.state.apply_export_fixes(apply, false);

        let written = std::fs::read(&loose).expect("the loose file");
        let toasts: Vec<String> = f.state.toasts.iter().map(|t| t.message.clone()).collect();
        let told = toasts
            .iter()
            .any(|t| t.contains("primitive_count 0 rewritten as"));
        if apply {
            assert_ne!(written, original, "the repair alters the bytes");
            let reopened =
                or_io::gta5::PgDictionary::<or_io::gta5::GtaDrawable>::from_bytes(&written);
            assert!(
                reopened.and_then(|dict| dict.to_bytes()).is_ok(),
                "the repaired file saves unasked"
            );
            assert!(told, "the user is told what was repaired: {toasts:?}");
        } else {
            assert_eq!(written, original, "declining keeps the original bytes");
            assert!(!told, "{toasts:?}");
        }
    }
}

#[test]
fn the_overlay_destination_keeps_what_the_copy_was_made_from() {
    use or_title::{record_file, BaseRecord, Staleness};

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut f = mounted(tmp.path(), true, EditMode::Overlay, "a.txt", SHIPPED);
    let records = super::overlay_refresh::base_records_dir().expect("a place to remember");
    assert_eq!(BaseRecord::read(&record_file(&records, &f.overlay)), None);

    save(&mut f, SaveDestination::Overlay);

    let record = BaseRecord::read(&record_file(&records, &f.overlay))
        .expect("the copy's first save records the archive it was made from");
    assert_eq!(
        record.verdict(&f.archive, &f.state.keys.provider()),
        Staleness::Fresh
    );
    std::fs::write(&f.archive, b"RPF7 gone").expect("the title replaces the archive");
    assert_eq!(
        record.verdict(&f.archive, &f.state.keys.provider()),
        Staleness::Stale
    );
}
