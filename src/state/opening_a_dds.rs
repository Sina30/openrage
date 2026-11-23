//! A loose `.dds` opens as a one-texture document in the texture workspace,
//! shows each stored mip, exports, takes new pixels and saves back as DDS.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{decode_level, format_of_dds, GrcTextureFormat, RgbaImage};
use or_io::dds::{caps, caps2, header_flags, pixel_format_flags, Dds, DdsHeader};

use super::textures::{self, TextureSource};
use super::{AppState, DocPayload, DocumentId, DocumentKind, LoadingState};
use crate::ui::dock::Workspace;

/// Each level's one solid colour, as RGB565 and as the RGBA it decodes to.
const LEVELS: [(u16, [u8; 4]); 4] = [
    (0xF800, [255, 0, 0, 255]),
    (0x07E0, [0, 255, 0, 255]),
    (0x001F, [0, 0, 255, 255]),
    (0xFFFF, [255, 255, 255, 255]),
];

/// An 8x8 DXT1 with its four mips, each a different solid colour.
fn eight_by_eight_dxt1() -> Dds {
    let mut data = Vec::new();
    for (level, (colour, _)) in LEVELS.iter().enumerate() {
        let blocks = if level == 0 { 4 } else { 1 };
        for _ in 0..blocks {
            data.extend_from_slice(&colour.to_le_bytes());
            data.extend_from_slice(&[0; 6]);
        }
    }
    let mut header = DdsHeader {
        flags: header_flags::CAPS
            | header_flags::HEIGHT
            | header_flags::WIDTH
            | header_flags::PIXELFORMAT
            | header_flags::MIPMAPCOUNT,
        width: 8,
        height: 8,
        mip_map_count: LEVELS.len() as u32,
        caps: caps::TEXTURE | caps::COMPLEX | caps::MIPMAP,
        ..DdsHeader::default()
    };
    header.pixel_format.flags = pixel_format_flags::FOURCC;
    header.pixel_format.four_cc = u32::from_le_bytes(*b"DXT1");
    Dds {
        header,
        dxt10: None,
        data,
    }
}

fn open(state: &mut AppState, path: PathBuf) -> DocumentId {
    state.load_file(path.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.load_receiver.is_some() && Instant::now() < deadline {
        state.poll_load_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(state.loading, LoadingState::Idle),
        "{}: {}",
        path.display(),
        state.error_dialog_message
    );
    state
        .active_document
        .expect("the file opened as a document")
}

fn saved_as(state: &mut AppState, path: &Path) -> Vec<u8> {
    assert!(state.save_active_document_as(path.to_path_buf()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.save_receiver.is_some() && Instant::now() < deadline {
        state.poll_save_result();
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::read(path).expect("the save wrote the file")
}

fn first_pixel(image: &RgbaImage) -> [u8; 4] {
    image.data[..4].try_into().expect("a pixel")
}

#[test]
fn a_dds_opens_as_a_texture_document_whose_mips_match() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("paint.dds");
    eight_by_eight_dxt1().save(&path).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);

    let kind = state.document(doc).map(|d| d.kind);
    assert_eq!(kind, Some(DocumentKind::Dds));
    assert_eq!(state.active_workspace, Workspace::Texture);

    let payload = &state.document(doc).expect("open").payload;
    let name = textures::sole_texture(payload).expect("a .dds is one texture");
    assert_eq!(name, "paint");
    let texture =
        textures::texture_at(payload, TextureSource::Document, &name).expect("the texture");
    assert_eq!((texture.width(), texture.height()), (8, 8));
    assert_eq!(texture.format(), GrcTextureFormat::DXT1);
    assert_eq!(texture.mip_count(), LEVELS.len() as u8);
    for (level, (_, rgba)) in LEVELS.iter().enumerate() {
        let image = decode_level(&texture, level as u32).expect("each stored mip decodes");
        let side = 8 >> level;
        assert_eq!((image.width, image.height), (side, side), "mip {level}");
        assert_eq!(first_pixel(&image), *rgba, "mip {level}");
    }

    let png = dir.path().join("mip1.png");
    textures::export_texture(&texture, 1, &png).expect("a mip exports to PNG");
    let exported = or_io::codec::texture::load_png(&png).expect("the export is a PNG");
    assert_eq!((exported.width, exported.height), (4, 4));
    assert_eq!(first_pixel(&exported), LEVELS[1].1);

    let copy = dir.path().join("copy.dds");
    textures::export_texture(&texture, 0, &copy).expect("the texture exports to DDS");
    let copy = Dds::open(&copy).expect("the export is a DDS");
    assert_eq!(copy.header.mip_map_count, LEVELS.len() as u32);
    assert_eq!(copy.data, eight_by_eight_dxt1().data);
}

#[test]
fn a_replaced_dds_saves_back_as_dds() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("paint.dds");
    eight_by_eight_dxt1().save(&path).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);

    let mut image = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            image.set_pixel(x, y, [0, 0, 255, 128]);
        }
    }
    assert!(state.texture_dictionary_is_editable(doc, TextureSource::Document));
    state
        .import_texture_over(
            doc,
            TextureSource::Document,
            "paint",
            &image,
            FormatChoice::Fixed(GrcTextureFormat::DXT5),
            &TextureBuildOptions {
                srgb: false,
                ..TextureBuildOptions::default()
            },
        )
        .expect("the replace path takes a .dds");

    let saved = Dds::from_bytes(&saved_as(&mut state, &dir.path().join("saved.dds")))
        .expect("the save is a DDS");
    assert_eq!((saved.header.width, saved.header.height), (16, 16));
    assert_eq!(
        format_of_dds(&saved).expect("a format"),
        (GrcTextureFormat::DXT5, false)
    );
    assert_eq!(saved.header.mip_map_count, 5);
    let reopened =
        DocPayload::Dds(super::dds_document::open("saved", &saved).expect("the save reopens"));
    let texture = textures::texture_at(&reopened, TextureSource::Document, "saved")
        .expect("the reopened texture");
    let top = decode_level(&texture, 0).expect("decodes");
    assert_eq!(first_pixel(&top)[2], 255);
}

#[test]
fn a_replaced_dds_undoes_to_the_texture_it_opened_with() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let path = dir.path().join("paint.dds");
    eight_by_eight_dxt1().save(&path).expect("written");
    let mut state = AppState::new();
    let doc = open(&mut state, path);
    let shown = |state: &AppState| match &state.document(doc).expect("open").payload {
        DocPayload::Dds(texture) => format!("{texture:?}"),
        _ => panic!("not a .dds"),
    };
    let opened = shown(&state);

    let image = RgbaImage::new(16, 16);
    let dxt5 = FormatChoice::Fixed(GrcTextureFormat::DXT5);
    let options = TextureBuildOptions::default();
    state
        .import_texture_over(
            doc,
            TextureSource::Document,
            "paint",
            &image,
            dxt5,
            &options,
        )
        .expect("the replace path takes a .dds");
    let replaced = shown(&state);
    assert_ne!(replaced, opened);

    assert!(state.undo().is_some(), "the replace is not a step");
    assert_eq!(
        shown(&state),
        opened,
        "undone, it is not the texture it opened as"
    );
    assert!(!state.document(doc).expect("open").dirty);
    state.redo();
    assert_eq!(shown(&state), replaced);
}

#[test]
fn a_cube_map_is_refused_rather_than_cut_to_one_face() {
    let mut cube = eight_by_eight_dxt1();
    cube.header.caps2 = caps2::CUBEMAP;
    let error = super::dds_document::open("sky", &cube).unwrap_err();
    assert!(error.contains("cube"), "unhelpful: {error}");
}
