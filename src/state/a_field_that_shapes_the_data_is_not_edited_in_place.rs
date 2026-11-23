//! A field whose value counts, sizes or lays out data elsewhere in the file, or
//! says what kind of thing its owner is, draws read-only in the inspector's
//! grid, saying why instead, and the texture view's own re-encode still changes
//! what it describes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_io::desc::{FieldRepr, Pack, PackType};
use or_io::gta5::formats::Format;
use or_io::gta5::{gen8::GrcTexture, File as _, PgDictionary};

use super::document::{DocPayload, Document};
use super::opening_a_ydd::finish_load;
use super::prefs::Game;
use super::textures::TextureSource;
use super::{AppState, CloseAnswer};

const WHAT: &str =
    "every_field_that_shapes_data_draws_read_only_with_a_reason_in_every_corpus_kind";

/// How many staged files of one kind and target are tried, smallest first, for one whose grid is described.
const TRIES: usize = 4;

/// How many nodes of one type a walk reads: every node of a type draws that type's fields.
const PER_TYPE: usize = 4;

/// The grid's label for each field of `ty` the list holds, with its reason, walking embedded structs as the grid does.
fn shaping(
    pack: &Pack,
    ty: &PackType,
    prefix: &str,
    depth: usize,
    out: &mut Vec<(String, &'static str)>,
) {
    for field in &ty.fields {
        let label = match prefix {
            "" => field.display_name(),
            _ => format!("{prefix} > {}", field.display_name()),
        };
        match &field.repr {
            FieldRepr::Inline { type_name } if depth < 4 => {
                if let Some(inner) = pack.type_by_name(type_name) {
                    shaping(pack, inner, &label, depth + 1, out);
                }
            }
            _ => out.extend(or_editor::shapes_data(&ty.name, &field.name).map(|why| (label, why))),
        }
    }
}

/// Every node of `node`'s outline with a type, depth first.
fn typed(node: &or_editor::Node, out: &mut Vec<(String, usize)>) {
    if let Some(name) = &node.type_name {
        out.push((name.clone(), node.at));
    }
    for child in &node.children {
        typed(child, out);
    }
}

/// What a file's grid draws of the fields the list holds.
#[derive(Default)]
struct Walked {
    /// The target the file's RSC7 version names.
    target: &'static str,
    /// `target Type: Label` of each field drawn read-only with its reason.
    held: BTreeSet<String>,
    /// Each field drawn otherwise, and how.
    wrong: Vec<String>,
}

/// What `doc`'s grid draws of each field the list holds, at every node of its outline; `None` where nothing is described.
fn walked(doc: &Document, path: &Path) -> Option<Walked> {
    let (pack, _) = super::described_root(doc)?;
    let image = doc.resource.as_ref()?;
    let outline = crate::ui::panels::hierarchy::described_outline(doc)?;
    let format = or_io::gta5::formats::format(doc.kind.extension())?;
    let target = format.target_of_version(image.seal.version()).ok()?.slug();
    let mut nodes = Vec::new();
    typed(&outline.root, &mut nodes);
    let mut out = Walked {
        target,
        ..Walked::default()
    };
    let mut read: BTreeMap<String, usize> = BTreeMap::new();
    for (name, at) in nodes {
        let times = read.entry(name.clone()).or_default();
        *times += 1;
        if *times > PER_TYPE {
            continue;
        }
        let Some(grid) = crate::ui::panels::inspector::summary_at(doc, &name, at) else {
            continue;
        };
        let Some(ty) = pack.type_by_name(&grid.title) else {
            continue;
        };
        let mut expected = Vec::new();
        shaping(&pack, ty, "", 0, &mut expected);
        for (label, why) in expected {
            let field = format!("{target} {}: {label}", grid.title);
            match grid.rows.iter().find(|r| r.label == label) {
                Some(r) if r.edit.is_none() && r.fixed == Some(why) => {
                    out.held.insert(field);
                }
                Some(r) if r.edit.is_some() => out
                    .wrong
                    .push(format!("{field} is written in place in {}", path.display())),
                Some(r) => out.wrong.push(format!(
                    "{field} gives another reason in {}: {:?}",
                    path.display(),
                    r.fixed
                )),
                None => out
                    .wrong
                    .push(format!("{field} has no row in {}", path.display())),
            }
        }
    }
    Some(out)
}

/// What the grid of the file at `path` draws of the fields the list holds, opened as the app opens it.
fn walk(state: &mut AppState, path: &Path) -> Result<Option<Walked>, String> {
    state.clear_error();
    state.load_file(path.to_path_buf());
    let id = finish_load(state)?;
    let walked = state.document(id).and_then(|doc| walked(doc, path));
    state.close_document(id);
    state.answer_close(CloseAnswer::DontSave);
    Ok(walked)
}

/// The kinds walked, as extension and target, each listed field drawn read-only with its reason, and each drawn otherwise.
type Walks = (
    BTreeSet<(&'static str, &'static str)>,
    BTreeSet<String>,
    Vec<String>,
);

/// The grid of the smallest described staged file of each declared kind per target, and of every staged file of a kind `every_file` names.
fn walk_every_kind(every_file: impl Fn(&Format) -> bool) -> Walks {
    let mut state = AppState::default();
    // ⚠ An Enhanced file otherwise waits on the question whether to switch editions.
    state.prefs.active_game = Some(Game::GtaVEnhanced);

    let (mut kinds, mut held, mut wrong) = (BTreeSet::new(), BTreeSet::new(), Vec::new());
    for format in or_io::gta5::formats::FORMATS {
        let every = every_file(format);
        for target in or_test_support::KNOWN_TARGETS {
            let mut files = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), format.ext)
                .target(target)
                .find();
            files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
            let tries = if every { files.len() } else { TRIES };
            for path in files.iter().take(tries) {
                match walk(&mut state, path) {
                    Ok(Some(walked)) => {
                        kinds.insert((format.ext, walked.target));
                        held.extend(walked.held);
                        wrong.extend(walked.wrong);
                        if !every {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(why) => eprintln!("{} did not open: {why}", path.display()),
                }
            }
        }
    }
    (kinds, held, wrong)
}

#[test]
fn every_field_that_shapes_data_draws_read_only_with_a_reason_in_every_corpus_kind() {
    let (kinds, held, wrong) = walk_every_kind(|_| false);
    if kinds.is_empty() {
        or_test_support::skip_corpus(WHAT, "no staged file of a declared kind opens");
        return;
    }
    eprintln!("walked {kinds:?}; drawn read-only with a reason:");
    for field in &held {
        eprintln!("  {field}");
    }
    assert!(
        wrong.is_empty(),
        "a field that shapes data is not drawn read-only with its reason:\n  {}",
        wrong.join("\n  ")
    );
    for (ext, target) in &kinds {
        assert!(
            held.iter().any(|f| f.starts_with(target)),
            "no {target} file reached a field that shapes data, so its walk proved nothing"
        );
        let texture_width = |f: &String| {
            f.starts_with(target) && f.contains(" GrcTexture") && f.ends_with(": Width")
        };
        assert!(
            *ext != "ytd" || held.iter().any(texture_width),
            "a {target} .ytd was walked and none of its textures' widths was checked"
        );
    }
}

/// A texture's usage at each target and each collision primitive's type, as the walk names a field it found read-only with its reason.
const RETYPING: &[&str] = &[
    "gen8 GrcTexture: ResourceTypeAndConversionFlags",
    "gen9 GrcTextureG9: UsageData",
    "gen8 PhPrimSphere: PrimType",
    "gen8 PhPrimCapsule: PrimType",
    "gen8 PhPrimBox: PrimType",
    "gen8 PhPrimCylinder: PrimType",
];

#[test]
fn a_textures_usage_and_every_primitives_type_draw_read_only_with_a_reason_in_every_corpus_kind() {
    // ⚠ Every staged file of a kind that can hold a collision primitive: few hold a sphere or a cylinder.
    let (kinds, held, wrong) = walk_every_kind(|format| {
        (format.targets())
            .any(|t| (format.description(t)).is_ok_and(|p| p.type_by_name("PhPrimBox").is_some()))
    });
    if kinds.is_empty() {
        or_test_support::skip_corpus(
            "a_textures_usage_and_every_primitives_type_draw_read_only_with_a_reason_in_every_corpus_kind",
            "no staged file of a declared kind opens",
        );
        return;
    }
    assert!(
        wrong.is_empty(),
        "a listed field is not drawn read-only with its reason:\n  {}",
        wrong.join("\n  ")
    );
    let walked = |field: &str| (kinds.iter()).any(|(_, target)| field.starts_with(target));
    let missing: Vec<&str> = (RETYPING.iter().copied())
        .filter(|field| walked(field) && !held.contains(*field))
        .collect();
    assert!(
        missing.is_empty(),
        "the walk found these nowhere drawn read-only with their reason: {missing:?}"
    );
}

/// A Legacy dictionary holding one 64-by-64 A8R8G8B8 texture, `paint`, open as document 1.
fn holding_paint() -> AppState {
    let mut dictionary = PgDictionary::default();
    dictionary.insert(GrcTexture {
        name: "paint".to_string(),
        width: 64,
        height: 64,
        depth: 1,
        format: GrcTextureFormat::A8R8G8B8.to_raw_format(),
        mip_count: 1,
        backing_store: vec![0x80; 64 * 64 * 4],
        ..Default::default()
    });
    let mut state = AppState::new();
    state.documents.clear();
    state.documents.push(Document::new(
        1,
        Some("C:/a/paint.ytd".into()),
        DocPayload::Resource(or_io::Asset::TextureDict(dictionary)),
    ));
    state.active_document = Some(1);
    state
}

/// `paint` as a save of document 1 writes it, read back.
fn saved_paint(state: &AppState) -> GrcTexture {
    let bytes = super::document_io::serialize_payload(
        &state.document(1).expect("open").payload,
        super::prefs::RpfOutputMode::Open,
        std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new()),
    )
    .expect("a Legacy .ytd saves");
    let reopened = PgDictionary::<GrcTexture>::from_bytes(&bytes).expect("the saved file reopens");
    reopened
        .get("paint")
        .expect("paint survives the save")
        .clone()
}

#[test]
fn the_texture_views_own_reencode_still_changes_a_textures_format_and_size() {
    let mut state = holding_paint();
    let options = TextureBuildOptions::default();
    let opened = saved_paint(&state);

    let dxt1 = FormatChoice::Fixed(GrcTextureFormat::DXT1);
    state
        .reencode_texture_in_place(1, TextureSource::Document, "paint", dxt1, &options)
        .expect("a 64-by-64 texture re-encodes");
    let reencoded = saved_paint(&state);
    assert_eq!(
        reencoded.format,
        GrcTextureFormat::DXT1.to_raw_format(),
        "the re-encode left the format as it was"
    );
    assert_ne!(
        (reencoded.mip_count, reencoded.backing_store.len()),
        (opened.mip_count, opened.backing_store.len()),
        "the re-encode left the mip levels and the payload's size as they were"
    );

    let image = RgbaImage::new(32, 16);
    state
        .import_texture_over(1, TextureSource::Document, "paint", &image, dxt1, &options)
        .expect("a 32-by-16 image imports over it");
    let imported = saved_paint(&state);
    assert_eq!((imported.width, imported.height), (32, 16));
}
