//! A row drawn by its picture: wrapped names, picture files, textures and models.

use super::*;

/// Every text a frame paints, as laid out.
fn painted_galleys(output: &egui::FullOutput) -> Vec<Arc<egui::Galley>> {
    fn walk(shape: &egui::Shape, out: &mut Vec<Arc<egui::Galley>>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn icons_wrap_a_long_name_whole_where_one_line_cut_it() {
    const LONG: &str = "prop_bench_01a_long_lod_name.ydr";
    let (mut state, doc) = browsing();
    let root = node(&state, doc, "");
    if let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) {
        rpf.session
            .add_file_copy(root, LONG, vec![0; 8])
            .expect("a long name");
    }
    state.prefs.explorer.view = crate::state::explorer::ViewMode::Icons;
    let ctx = egui::Context::default();
    frame(&ctx, &mut state, Vec::new());
    let output = run(&ctx, &mut state, Vec::new(), egui::Modifiers::NONE);
    let galley = painted_galleys(&output)
        .into_iter()
        .find(|g| g.text().starts_with("prop_bench"))
        .expect("the tile names its file");
    assert!(!galley.elided, "the name is drawn whole, not cut to …");
    assert!(
        galley.rows.len() > 1,
        "it wraps onto more than one line: {} row(s)",
        galley.rows.len()
    );
    assert!(listed(&state, doc).contains(&LONG.to_string()));
}

/// A picture file made off the UI thread, waited for frame by frame.
fn wait_for_picture(
    ctx: &egui::Context,
    state: &mut AppState,
    doc: DocumentId,
    node: RpfNodeId,
    side: u32,
) -> crate::ui::panels::hierarchy::pictures::Shown {
    use crate::ui::panels::hierarchy::pictures::{picture, Shown};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let shown = picture(ctx, state, None, doc, node, side);
        if shown != Shown::Waiting || std::time::Instant::now() > deadline {
            return shown;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn icons_draw_a_picture_file_as_its_picture_and_a_file_with_none_by_its_glyph() {
    use crate::ui::panels::hierarchy::pictures::Shown;
    let (mut state, doc) = browsing();
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(300, 150, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("a picture");
    let root = node(&state, doc, "");
    let photo = match state.document_mut(doc).map(|d| &mut d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.add_file_copy(root, "photo.png", png),
        _ => panic!("the archive is open"),
    }
    .expect("a picture file");
    state.prefs.explorer.view = crate::state::explorer::ViewMode::Icons;
    let ctx = egui::Context::default();
    frame(&ctx, &mut state, Vec::new());

    match wait_for_picture(&ctx, &mut state, doc, photo, 96) {
        Shown::Ready { size, .. } => assert_eq!(
            size,
            egui::vec2(96.0, 48.0),
            "fitted to the tile, its shape kept"
        ),
        other => panic!("the picture never came: {other:?}"),
    }
    let notes = node(&state, doc, "notes.txt");
    assert_eq!(
        wait_for_picture(&ctx, &mut state, doc, notes, 96),
        Shown::None,
        "a text file has no picture, and its glyph stands for it"
    );
}

#[test]
fn a_texture_dictionary_is_drawn_as_its_first_texture_and_a_drawable_as_its_geometry() {
    use crate::state::files_view::pictures::{make, Made, PictureKind};
    let crypto: Arc<dyn or_io::container::RpfCryptoProvider> =
        Arc::new(or_io::container::NoOpCryptoProvider);
    let first = |ext: &str| {
        or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), ext)
            .find()
            .into_iter()
            .next()
    };
    let (Some(ytd), Some(ydr)) = (first("ytd"), first("ydr")) else {
        or_test_support::skip_corpus(
            "a_texture_dictionary_is_drawn_as_its_first_texture_and_a_drawable_as_its_geometry",
            "no .ytd and .ydr staged",
        );
        return;
    };
    let bytes = std::fs::read(&ytd).expect("the dictionary");
    match make(
        PictureKind::Texture,
        "staged.ytd",
        &bytes,
        128,
        crypto.clone(),
    ) {
        Made::Image(image) => {
            assert!(
                image.width.max(image.height) <= 128 && image.width > 0,
                "{ytd:?}"
            );
        }
        Made::Model { .. } => panic!("{ytd:?} made a model"),
        Made::Failed(why) => panic!("{ytd:?}: {why}"),
    }

    let bytes = std::fs::read(&ydr).expect("the drawable");
    let Made::Model { payload, kind } = make(PictureKind::Model, "staged.ydr", &bytes, 128, crypto)
    else {
        panic!("{ydr:?} made no model");
    };
    let state = AppState::from_prefs(Prefs::default());
    let geometry = crate::ui::panels::hierarchy::pictures::model_geometry(&state, *payload, kind);
    assert!(
        !geometry.is_empty(),
        "{ydr:?}: the drawable gave the thumbnail renderer nothing to draw"
    );
}
