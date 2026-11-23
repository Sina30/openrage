//! A scanned GTA VI anim scene's timeline, above its metadata view: the scene's own events as the
//! top row, then a row per entity, each event a span from its start to its end. Read only.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::gta6::anim_scene::{self, AnimSceneBody, Event, Scene};
use or_io::meta::binary::rbm::schema::RbmModel;
use or_io::meta::binary::ContainerLayout;
use or_io::meta::MetaInstance;
use or_transport::{TimeRange, Transport};

use crate::state::{AppState, DocPayload, Document, DocumentId};
use crate::ui::theme;
use crate::ui::timeline::{
    self, ChannelRow, RowEvents, RowId, RowKind, SpanEvent, TimelineSource, TimelineState,
};

/// The most the timeline takes of the height above the metadata view.
const HEIGHT: f32 = 240.0;
const KIND_SCENE: u8 = 0;
const KIND_ENTITY: u8 = 1;

/// The class table, value tree and tree revision of a GTA VI headerless metadata document whose
/// root block is a scene class.
fn scene_model(doc: &Document) -> Option<(&RbmModel, &MetaInstance, u64)> {
    let DocPayload::Meta(meta) = &doc.payload else {
        return None;
    };
    let Some(ContainerLayout::Rbm(rbm)) = meta.layout.as_ref().map(|l| &l.container) else {
        return None;
    };
    let parsed = meta.parsed.as_ref()?;
    let scene = rbm.seal.is_none() && anim_scene::scene_root(&rbm.model).is_some();
    scene.then_some((&rbm.model, &parsed.instance, parsed.revision))
}

/// Whether `doc` is the metadata view of a GTA VI anim scene.
pub(crate) fn holds_one(doc: &Document) -> bool {
    scene_model(doc).is_some()
}

type Typed = BTreeMap<DocumentId, (u64, Result<Arc<AnimSceneBody>, String>)>;

/// What each document's tree was last typed to, at the tree's revision.
// ponytail: one map for the whole app, which no close empties, typed on the UI thread once per revision; a worker if a scene's size makes that frame hitch.
fn typed() -> MutexGuard<'static, Typed> {
    static TYPED: Mutex<Typed> = Mutex::new(BTreeMap::new());
    TYPED.lock().unwrap_or_else(|p| p.into_inner())
}

/// The anim scene the value tree of `doc` states, or why it is refused; `None` for any other document.
pub(crate) fn of(doc: &Document) -> Option<Result<Arc<AnimSceneBody>, String>> {
    let (model, tree, revision) = scene_model(doc)?;
    let mut typed = typed();
    if let Some((_, read)) = typed.get(&doc.id).filter(|(at, _)| *at == revision) {
        return Some(read.clone());
    }
    let read = (anim_scene::read_tree(model, tree))
        .map(Arc::new)
        .map_err(|e| e.to_string());
    typed.insert(doc.id, (revision, read.clone()));
    Some(read)
}

/// The timeline's own state, and the rows built for the scene it last showed.
#[derive(Default)]
struct SceneTimelineState {
    rows: TimelineState,
    transport: Transport,
    /// The document shown, and which scene of it.
    shown: Option<DocumentId>,
    scene: usize,
    built: Option<(Arc<AnimSceneBody>, usize, Arc<SceneTimeline>)>,
}

impl crate::ui::panel_state::PanelState for SceneTimelineState {
    const KEY: &'static str = "gta6_anim_scene_timeline";
}

/// An event's span from its earlier time to its later, and whether it starts after its end.
struct Span {
    range: TimeRange,
    label: String,
    reversed: bool,
}

/// The rows of one scene, built once when it is first shown.
struct SceneTimeline {
    rows: Vec<ChannelRow>,
    spans: Vec<Vec<Span>>,
    bounds: TimeRange,
    summary: String,
}

fn span(event: &Event) -> Option<Span> {
    let (start, end) = event.span()?;
    let label = match &event.clip {
        Some(clip) => format!("{} {}", event.class, clip.name),
        None => event.class.clone(),
    };
    let reversed = start > end;
    let label = match reversed {
        true => tr!("{label}, which starts after its end", label = label),
        false => label,
    };
    Some(Span {
        range: TimeRange::new(start.min(end), start.max(end)),
        label,
        reversed,
    })
}

impl SceneTimeline {
    fn of(scene: &Scene) -> Self {
        let own = ChannelRow::group(RowId::new(KIND_SCENE, 0, 0), tr!("Scene events"));
        let mut rows = vec![own.with_kind(RowKind::Track)];
        let mut spans = vec![scene.events.iter().filter_map(span).collect::<Vec<_>>()];
        for (k, entity) in scene.entities.iter().enumerate() {
            let id = RowId::new(KIND_ENTITY, u32::try_from(k).unwrap_or(u32::MAX), 0);
            let label = format!("{} {}", entity.key, entity.class);
            rows.push(ChannelRow::group(id, label).with_kind(RowKind::Track));
            spans.push(entity.events.iter().filter_map(span).collect());
        }
        let (first, last) = (scene.all_events().filter_map(Event::span))
            .fold((0.0f32, 0.0f32), |(lo, hi), (start, end)| {
                (lo.min(start.min(end)), hi.max(start.max(end)))
            });
        let events = scene.all_events().count();
        let summary = format!(
            "{} · {}",
            tr_n!("{count} entity", "{count} entities", scene.entities.len()),
            tr_n!("{count} event", "{count} events", events)
        );
        Self {
            rows,
            spans,
            bounds: TimeRange::new(first, last.max(first + 1.0)),
            summary,
        }
    }
}

impl TimelineSource for SceneTimeline {
    fn bounds(&self) -> TimeRange {
        self.bounds
    }

    fn rows(&self) -> &[ChannelRow] {
        &self.rows
    }

    fn row_events(&self, row: usize, view: TimeRange, out: &mut RowEvents) {
        out.clear();
        let shown = (self.spans.get(row).into_iter().flatten())
            .filter(|s| s.range.end >= view.start && s.range.start <= view.end);
        for s in shown {
            let colour = match s.reversed {
                true => theme::WARNING.get(),
                false => theme::ACCENT.get(),
            };
            out.spans.push(SpanEvent {
                range: s.range,
                colour,
                label: Some(s.label.clone()),
            });
        }
    }

    fn read_only_reason(&self) -> Option<&str> {
        Some(tr!(
            "the timeline shows each event as the body states it; its fields edit in the form below"
        ))
    }

    fn playback_unavailable(&self) -> Option<&str> {
        Some(tr!(
            "Nothing plays: the clips a scene names are not posed here."
        ))
    }

    fn summary(&self) -> String {
        self.summary.clone()
    }
}

/// Draw the timeline of the anim scene the document `doc_id` holds; whether it holds one.
pub(crate) fn draw(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) -> bool {
    let read = state.document(doc_id).and_then(of);
    let body = match read {
        None => return false,
        Some(Ok(body)) => body,
        Some(Err(why)) => {
            let why = tr!(
                "GTA VI's anim-scene reader does not read this scene, so no timeline shows it: {why}",
                why = why
            );
            ui.label(egui::RichText::new(why).small().color(theme::WARNING));
            return true;
        }
    };
    let Some(last) = body.scenes.len().checked_sub(1) else {
        let none = egui::RichText::new(tr!("This map of scenes holds none."));
        ui.label(none.color(theme::TEXT_SECONDARY));
        return true;
    };
    let panel = state.panels.get::<SceneTimelineState>();
    if panel.shown != Some(doc_id) {
        panel.shown = Some(doc_id);
        panel.scene = 0;
    }
    panel.scene = panel.scene.min(last);
    if body.map {
        let key = |k: usize| body.scenes[k].key.clone().unwrap_or_default();
        egui::ComboBox::from_label(tr!("Scene"))
            .selected_text(key(panel.scene))
            .show_ui(ui, |ui| {
                for k in 0..=last {
                    ui.selectable_value(&mut panel.scene, k, key(k));
                }
            });
    }
    let cached = (panel.built.as_ref())
        .filter(|(b, scene, _)| Arc::ptr_eq(b, &body) && *scene == panel.scene)
        .map(|(_, _, source)| Arc::clone(source));
    let source = match cached {
        Some(source) => source,
        None => {
            let source = Arc::new(SceneTimeline::of(&body.scenes[panel.scene]));
            panel.built = Some((Arc::clone(&body), panel.scene, Arc::clone(&source)));
            source
        }
    };
    // An edit of the tree keeps the view; another document or scene starts it afresh.
    panel
        .rows
        .ensure_subject(doc_id ^ ((panel.scene as u64) << 40));
    let size = egui::vec2(
        ui.available_width(),
        HEIGHT.min(ui.available_height() * 0.5),
    );
    let SceneTimelineState {
        rows, transport, ..
    } = panel;
    ui.allocate_ui(size, |ui| {
        let output = timeline::draw(ui, &*source, transport, rows);
        if let Some(time) = output.seek_to {
            transport.time = transport.range.clamp(time);
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::anim_scene::{Clip, Entity};

    fn event(start: f32, end: f32) -> Event {
        Event {
            class: "hash_0CB4F193".into(),
            start: Some(start),
            end: Some(end),
            preload: None,
            entity: None,
            clip: Some(Clip {
                dictionary: "hash_00000D1C".into(),
                name: "hash_00000C11".into(),
            }),
        }
    }

    #[test]
    fn each_entity_is_a_row_of_spans_and_a_start_after_its_end_is_drawn_as_stated() {
        let timeless = Event {
            start: Some(f32::NAN),
            ..event(0.0, 0.0)
        };
        let scene = Scene {
            key: None,
            events: vec![event(0.5, 2.0), timeless],
            entities: vec![Entity {
                key: "hash_00000E17".into(),
                class: "hash_64D97283".into(),
                events: vec![event(6.0, 4.5)],
            }],
        };
        let source = SceneTimeline::of(&scene);
        assert_eq!(source.rows().len(), 2);
        assert_eq!(source.rows()[1].label, "hash_00000E17 hash_64D97283");
        assert_eq!(source.bounds(), TimeRange::new(0.0, 6.0));

        let mut out = RowEvents::default();
        source.row_events(0, TimeRange::new(0.0, 10.0), &mut out);
        let ranges: Vec<TimeRange> = out.spans.iter().map(|s| s.range).collect();
        assert_eq!(
            ranges,
            [TimeRange::new(0.5, 2.0)],
            "a time that is not finite is drawn"
        );

        source.row_events(1, TimeRange::new(0.0, 10.0), &mut out);
        assert_eq!(out.spans.len(), 1);
        assert_eq!(out.spans[0].range, TimeRange::new(4.5, 6.0));
        assert_eq!(out.spans[0].colour, theme::WARNING.get());

        source.row_events(1, TimeRange::new(7.0, 9.0), &mut out);
        assert!(out.spans.is_empty(), "a span out of view is given");
    }
}
