//! An edited GTA VI metadata document saved as a file of its own: a PSO or RBF body as its bytes,
//! a headerless resource as a loose resource of its edited sections; a PSO keeps its checksum.

use or_io::container::rpf::Rpf9Sections;

use super::document::DocPayload;
use super::extract_job::{ExtractItem, OnClash};
use super::{AppState, DocumentId};

/// The metadata row's `edited_sections`: a headerless resource's, once it is edited.
pub(super) fn edited_sections(payload: &DocPayload) -> Option<Result<Rpf9Sections, String>> {
    let DocPayload::Meta(meta) = payload else {
        return None;
    };
    meta.is_edited()
        .then(|| meta.headerless_sections())
        .flatten()
}

impl AppState {
    /// Says, as an edited GTA VI PSO document is saved, that its checksum is the one it was read
    /// with.
    pub(super) fn say_the_checksum_is_kept(&mut self, doc: DocumentId) {
        if matches!(self.document(doc).map(|d| &d.payload), Some(DocPayload::Meta(m)) if m.keeps_a_checksum() && m.is_edited())
        {
            self.show_warning_toast(tr!("The CHKS checksum is kept as it was read and is not recomputed, so the game may refuse the saved file."));
        }
        self.say_the_animation_hashes_are_kept(doc);
    }

    /// `name` in `dest`, the bytes of the edited PSO or RBF body `doc` shows, written on a worker;
    /// `None` for any other document, which saves as it did.
    pub(super) fn save_edited_meta(
        &mut self,
        doc: DocumentId,
        dest: std::path::PathBuf,
        name: String,
        reads: std::path::PathBuf,
    ) -> Option<bool> {
        let DocPayload::Meta(meta) = &self.document(doc)?.payload else {
            return None;
        };
        if !meta.is_edited() || meta.headerless_sections().is_some() {
            return None;
        }
        match meta.to_bytes() {
            Ok(bytes) => {
                let item = ExtractItem::bytes(name.clone(), bytes);
                Some(self.start_scanned_extraction(name, dest, reads, vec![item], OnClash::Save))
            }
            Err(e) => {
                self.show_error_toast(tr!("Cannot save: {reason}", reason = e));
                Some(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::{Duration, Instant};

    use or_io::container::rpf::{read_rpf9_body, Rpf9Sections};
    use or_io::meta::Value;

    use crate::state::opening_an_rpf9_archive::{
        headerless_rbm, row_at, rows, rpf9_head, said, scanned, settle, written, zlib,
    };
    use crate::state::{AppState, DocPayload, DocumentId, ToastType};

    const CHECKSUM: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

    fn frame(out: &mut Vec<u8>, ident: &[u8; 4], content: &[u8]) {
        out.extend_from_slice(ident);
        out.extend_from_slice(&(content.len() as u32 + 8).to_be_bytes());
        out.extend_from_slice(content);
    }

    /// A PSO body in GTA VI's table form whose root holds one u32, laid out as the rules lay one
    /// out, with a checksum word.
    fn table_form_pso() -> Vec<u8> {
        const ROOT: u32 = 0x1111_0001;
        let mut psin = [ROOT.to_le_bytes(), 4u32.to_le_bytes()].concat();
        psin.extend([0xAA; 8]);
        psin.extend([0x70; 8]);
        psin.extend(77u32.to_le_bytes());
        psin.resize(40, 0);
        let mut lsch = [ROOT.to_le_bytes(), 0x5151_5151u32.to_le_bytes()].concat();
        lsch.extend([0, 3, 0, 0, 0, 0, 0, 0]);
        lsch.extend([0xAA; 8]);
        lsch.extend([4, 0, 0, 0, 0, 0, 1, 0]);
        lsch.extend(0x2222_0000u32.to_le_bytes());
        lsch.extend([0, 0, 0, 0, 0x15, 0, 0, 0, 0, 0, 0, 0]);
        let mut lrsc = vec![0u8; 0x50];
        lrsc[0..4].copy_from_slice(b"0DRP");
        lrsc[4..6].copy_from_slice(&0x7Au16.to_le_bytes());
        lrsc[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        lrsc[0x18..0x38].fill(0xBB);
        lrsc[0x38..0x3A].copy_from_slice(&1u16.to_le_bytes());
        lrsc[0x3C..0x3E].copy_from_slice(&1u16.to_le_bytes());
        lrsc[0x40..0x48].fill(0xBB);
        let mut out = Vec::new();
        frame(&mut out, b"PSIN", &psin);
        frame(&mut out, b"LSCH", &lsch);
        frame(&mut out, b"LENM", &[]);
        frame(&mut out, b"LSNS", &0u32.to_le_bytes());
        frame(&mut out, b"LGID", &[]);
        frame(&mut out, b"LRSC", &lrsc);
        let mut chks = (out.len() as u32 + 20).to_be_bytes().to_vec();
        chks.extend(CHECKSUM);
        chks.extend(b"zppp");
        frame(&mut out, b"CHKS", &chks);
        out
    }

    fn finished(state: &mut AppState) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while state.extract_jobs.is_running() {
            state.poll_extract_job();
            assert!(Instant::now() < deadline, "the save never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Open the body at `offset` of the scan `doc` and set its root's u32 to `value`.
    fn opened_and_edited(state: &mut AppState, doc: DocumentId, offset: u64, value: u64) {
        state.open_scanned_body(doc, row_at(&rows(state, doc), offset));
        settle(state);
        let body = (state.active_document).filter(|&d| d != doc);
        let body = body.unwrap_or_else(|| panic!("the body: {}", state.error_dialog_message));
        let Some(DocPayload::Meta(meta)) = state.document_mut(body).map(|d| &mut d.payload) else {
            panic!("a metadata document");
        };
        let parsed = meta.parsed.as_mut().expect("it parsed");
        parsed.instance.fields[0].1 = Value::UInt(value);
        meta.after_visual_edit_in_place();
    }

    fn root_value(instance: &or_io::meta::MetaInstance) -> &Value {
        &instance.fields[0].1
    }

    #[test]
    fn a_value_typed_into_the_form_of_a_scanned_gta_vi_metadata_body_is_kept() {
        let _serial = crate::ui::theme::serial();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let mut bytes = rpf9_head(1, 0x1A0);
        bytes.resize(0x1000, 0);
        bytes.extend(zlib(&table_form_pso()));
        let mut state = AppState::new();
        let scan = scanned(&mut state, written(dir.path(), &bytes));
        state.open_scanned_body(scan, row_at(&rows(&state, scan), 0x1000));
        settle(&mut state);
        let body = (state.active_document)
            .filter(|&d| d != scan)
            .expect("the body");

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let frame = |state: &mut AppState, events| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 1000.0),
                )),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| crate::ui::panels::meta_editor::draw(ui, state));
            let update = output.platform_output.accesskit_update;
            (update.into_iter().flat_map(|u| u.nodes))
                .filter(|(_, n)| n.value() == Some("77"))
                .find_map(|(_, n)| n.bounds())
                .map(|b| egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
        };
        frame(&mut state, Vec::new());
        let at = frame(&mut state, Vec::new()).expect("the field holding 77");
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut state, vec![egui::Event::PointerMoved(at), press(true)]);
        frame(&mut state, vec![press(false)]);
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        frame(
            &mut state,
            vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
        );
        frame(&mut state, vec![egui::Event::Text("78".into())]);
        frame(
            &mut state,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        frame(&mut state, Vec::new());

        let Some(DocPayload::Meta(meta)) = state.document(body).map(|d| &d.payload) else {
            panic!("a metadata document");
        };
        let parsed = meta.parsed.as_ref().expect("it parsed");
        assert_eq!(root_value(&parsed.instance), &Value::UInt(78));
        assert!(!said(&state, ToastType::Warning, &["shown read-only"]));
    }

    #[test]
    fn an_edited_gta_vi_metadata_body_saves_as_a_file_that_opens_with_the_edit() {
        let _serial = crate::ui::theme::serial();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let mut bytes = rpf9_head(1, 0x1A0);
        bytes.resize(0x1000, 0);
        bytes.extend(zlib(&table_form_pso()));
        bytes.resize(0x2000, 0);
        // Room after the block map for the slot a loose resource of one block writes there.
        let mut headerless = headerless_rbm(0x5000_00E0);
        headerless.resize(0x110, 0);
        bytes.extend(zlib(&headerless));
        let mut state = AppState::new();
        let archive = written(dir.path(), &bytes);
        let doc = scanned(&mut state, archive.clone());

        // An XML export holds no checksum, and a save over the archive is refused: neither warns of one.
        opened_and_edited(&mut state, doc, 0x1000, 78);
        assert!(state.save_active_document_as(dir.path().join("edited.xml")));
        finished(&mut state);
        assert!(!state.save_active_document_as(archive));
        assert!(!said(&state, ToastType::Warning, &["CHKS checksum"]));

        // A PSO body: its bytes with the edit, the checksum as it was read, and a sentence that says so.
        state.toasts.clear();
        opened_and_edited(&mut state, doc, 0x1000, 78);
        let saved = dir.path().join("edited.pso");
        assert!(state.save_active_document_as(saved.clone()));
        finished(&mut state);
        assert!(said(
            &state,
            ToastType::Warning,
            &["CHKS checksum", "not recomputed"]
        ));
        let file = std::fs::read(&saved).expect("the file");
        assert_eq!(&file[file.len() - 8..file.len() - 4], &CHECKSUM);
        let reopened =
            or_io::meta::MetaDocument::parse(&Default::default(), file).expect("it reads");
        assert_eq!(root_value(&reopened.instance), &Value::UInt(78));

        // A headerless resource: a loose resource of its edited sections.
        state.toasts.clear();
        opened_and_edited(&mut state, doc, 0x2000, 79);
        let saved = dir.path().join("edited.rsc");
        assert!(state.save_active_document_as(saved.clone()));
        finished(&mut state);
        assert!(!said(&state, ToastType::Warning, &["CHKS checksum"]));
        let file = std::fs::read(&saved).expect("the file");
        let (find, read) = read_rpf9_body(Cursor::new(file), 0, u64::MAX).expect("it reads");
        let sections = Rpf9Sections::of(&find, &read).expect("a resource");
        let (instance, _) =
            or_io::meta::binary::rbm::gta6::open(&sections.virtual_section, &sections.physical)
                .expect("it opens");
        assert_eq!(root_value(&instance), &Value::UInt(79));
    }

    /// `typed` typed at a click into the Text panel over the active document.
    fn typed_into_the_text_panel(state: &mut AppState, typed: &str) {
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |state: &mut AppState, events: Vec<egui::Event>| {
            time += 1.0 / 30.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 300.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| crate::ui::panels::text_editor::draw(ui, state));
        };
        let at = egui::pos2(300.0, 60.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(state, Vec::new());
        frame(state, vec![egui::Event::PointerMoved(at), press(true)]);
        frame(state, vec![press(false)]);
        frame(state, Vec::new());
        frame(state, vec![egui::Event::Text(typed.into())]);
        frame(state, Vec::new());
    }

    #[test]
    fn xml_typed_into_the_text_panel_of_a_scanned_gta_vi_metadata_body_is_kept_and_saved() {
        use crate::state::TextDocument;

        let _serial = crate::ui::theme::serial();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let mut bytes = rpf9_head(1, 0x1A0);
        bytes.resize(0x1000, 0);
        bytes.extend(zlib(&table_form_pso()));
        let mut state = AppState::new();
        let scan = scanned(&mut state, written(dir.path(), &bytes));
        state.open_scanned_body(scan, row_at(&rows(&state, scan), 0x1000));
        settle(&mut state);
        let body = (state.active_document)
            .filter(|&d| d != scan)
            .expect("the body");
        let text = |state: &AppState| match state.document(body).map(|d| &d.payload) {
            Some(DocPayload::Meta(meta)) => meta.text.text.to_string(),
            _ => panic!("a metadata document"),
        };

        let opened = text(&state);
        typed_into_the_text_panel(&mut state, "x");
        assert_ne!(text(&state), opened, "the Text panel takes typing");
        assert!(state.document(body).is_some_and(|d| d.dirty));

        // The root's value as XML typed in its place, saved as the body's own form.
        let xml = opened.replacen("value=\"77\"", "value=\"78\"", 1);
        assert_ne!(xml, opened, "the root's u32 is written as an attribute");
        let Some(DocPayload::Meta(meta)) = state.document_mut(body).map(|d| &mut d.payload) else {
            panic!("a metadata document");
        };
        meta.text = TextDocument::decode(xml.as_bytes());
        meta.mark_text_edited();
        // As the form panel does on every frame: the typed XML is the tree, and the body's form stays.
        meta.ensure_parsed_current();
        let saved = dir.path().join("typed.pso");
        assert!(state.save_active_document_as(saved.clone()));
        finished(&mut state);
        let file = std::fs::read(&saved).expect("the file");
        let reopened =
            or_io::meta::MetaDocument::parse(&Default::default(), file).expect("it reads");
        assert!(
            reopened.source_format.is_binary(),
            "saved as the body's own form"
        );
        assert_eq!(root_value(&reopened.instance), &Value::UInt(78));

        // A body read-only for any other reason takes no typing.
        if let Some(doc) = state.document_mut(body) {
            doc.read_only = Some("read-only for another reason");
        }
        let before = text(&state);
        typed_into_the_text_panel(&mut state, "y");
        assert_eq!(text(&state), before);
    }
}
