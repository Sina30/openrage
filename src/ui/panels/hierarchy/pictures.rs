//! The pictures the Files view draws rows by: a texture's or an image's as an
//! egui texture, a model's as the thumbnail renderer's render of it.

use std::collections::{HashMap, VecDeque};

use crate::render::thumbnails::ThumbKey;
use crate::render::RenderBridge;
use crate::state::files_view::pictures::{Made, PictureKey, Pictures};
use crate::state::models::{self, ModelListing};
use crate::state::{AppState, DocPayload, Document, DocumentId, DocumentKind, RpfNodeId};

/// A row's picture, as a view draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    /// On its way.
    Waiting,
    Ready {
        id: egui::TextureId,
        size: egui::Vec2,
    },
    /// It has none, and its glyph stands for it.
    None,
}

enum Held {
    Waiting,
    Image(egui::TextureHandle),
    /// A model's render, under the key the renderer keeps it by.
    Render(ThumbKey),
    Failed,
}

/// Pictures already made, the most recently drawn last.
#[derive(Default)]
pub struct PictureCache {
    jobs: Pictures,
    held: HashMap<PictureKey, Held>,
    order: VecDeque<PictureKey>,
}

impl crate::ui::panel_state::PanelState for PictureCache {
    const KEY: &'static str = "files-pictures";
}

/// How many pictures stay made; a 96-point tile is about 36 KiB.
const KEPT: usize = 512;

/// The document id a model is read into to build its geometry; no document has it.
const PROBE: DocumentId = DocumentId::MAX - 1;

/// Row `node`'s picture, at most `side` pixels on its longer side, asked for on the first look.
pub fn picture(
    ctx: &egui::Context,
    state: &mut AppState,
    mut bridge: Option<&mut RenderBridge>,
    doc: DocumentId,
    node: RpfNodeId,
    side: u32,
) -> Shown {
    let Some(size) = state.document(doc).and_then(|d| match &d.payload {
        DocPayload::Rpf(rpf) => rpf.session.node(node).map(|_| rpf.session.size_for(node)),
        _ => None,
    }) else {
        return Shown::None;
    };
    let key = PictureKey {
        doc,
        node,
        side,
        size: size.unwrap_or(0),
    };
    let mut cache = state.panels.take::<PictureCache>();
    cache.collect(ctx, state, bridge.as_deref_mut());
    if !cache.held.contains_key(&key) {
        let crypto = state.keys.provider();
        let asked = match state.document(doc).map(|d| &d.payload) {
            Some(DocPayload::Rpf(rpf)) => cache.jobs.ask(&rpf.session, key, crypto),
            _ => false,
        };
        cache.hold(key, if asked { Held::Waiting } else { Held::Failed });
    }
    let shown = match cache.held.get(&key) {
        Some(Held::Image(handle)) => Shown::Ready {
            id: handle.id(),
            size: handle.size_vec2(),
        },
        Some(Held::Render(thumb)) => match bridge {
            Some(bridge) => match bridge.thumbnail(*thumb).map(|t| t.id) {
                Some(Some(id)) => {
                    let side = crate::render::thumbnails::TILE_PX as f32;
                    Shown::Ready {
                        id,
                        size: egui::vec2(side, side),
                    }
                }
                Some(None) => Shown::None,
                None if bridge.thumbnail_pending(*thumb) => Shown::Waiting,
                // ⚠ The renderer let it go to make room, so it is read again.
                None => {
                    cache.held.remove(&key);
                    Shown::Waiting
                }
            },
            None => Shown::None,
        },
        Some(Held::Waiting) => Shown::Waiting,
        Some(Held::Failed) | None => Shown::None,
    };
    if shown == Shown::Waiting {
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }
    state.panels.put(cache);
    shown
}

impl PictureCache {
    fn hold(&mut self, key: PictureKey, held: Held) {
        self.order.retain(|k| *k != key);
        self.order.push_back(key);
        self.held.insert(key, held);
        while self.order.len() > KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.held.remove(&oldest);
            }
        }
    }

    /// Take in what the jobs finished.
    fn collect(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
        mut bridge: Option<&mut RenderBridge>,
    ) {
        for (key, made) in self.jobs.finished() {
            let held = match made {
                Made::Image(rgba) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width as usize, rgba.height as usize],
                        &rgba.data,
                    );
                    let label = format!("files-picture:{}:{}:{}", key.doc, key.node, key.side);
                    Held::Image(ctx.load_texture(label, image, egui::TextureOptions::LINEAR))
                }
                Made::Model { payload, kind } => {
                    match bridge.as_deref_mut().and_then(|bridge| {
                        render_model(state, bridge, key.doc, key.node, *payload, kind)
                    }) {
                        Some(thumb) => Held::Render(thumb),
                        None => Held::Failed,
                    }
                }
                Made::Failed(why) => {
                    tracing::debug!(node = key.node, why, "no picture for a Files view row");
                    Held::Failed
                }
            };
            self.hold(key, held);
        }
    }
}

/// Hand a model's geometry to the renderer for a thumbnail of row `node`; `None` when it has none.
fn render_model(
    state: &AppState,
    bridge: &mut RenderBridge,
    doc: DocumentId,
    node: RpfNodeId,
    payload: DocPayload,
    kind: DocumentKind,
) -> Option<ThumbKey> {
    let uploads = model_geometry(state, payload, kind);
    if uploads.is_empty() {
        return None;
    }
    let key = bridge.thumbnail_key(state, state.document(doc)?, node);
    bridge.request_thumbnail_with(key, uploads);
    Some(key)
}

/// The geometry the first model of `payload` draws, as its gallery tile would.
pub fn model_geometry(
    state: &AppState,
    payload: DocPayload,
    kind: DocumentKind,
) -> Vec<crate::render::meshes::MeshUpload> {
    let mut probe = Document::new(PROBE, None, payload);
    probe.kind = kind;
    let ModelListing::Models(listed) = models::models_in(&probe.payload) else {
        return Vec::new();
    };
    match listed.first() {
        Some(first) => crate::render::thumbnails::geometry_for(state, &probe, first.selection_id),
        None => Vec::new(),
    }
}
