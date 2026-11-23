use super::*;

use or_io::gta6::anim_scene::Scene;

/// A row for a scanned anim scene's own events and one per entity, a map's scenes under headers; whether `doc_id` holds one.
pub(super) fn draw_gta6_anim_scene(
    ui: &mut egui::Ui,
    state: &AppState,
    doc_id: DocumentId,
) -> bool {
    let read = state
        .document(doc_id)
        .and_then(crate::ui::panels::gta6_anim_scene::of);
    let Some(read) = read else {
        return false;
    };
    // A refusal is said above the metadata view, where the timeline would be.
    let Ok(body) = read else {
        return true;
    };
    if !body.map {
        body.scenes.iter().for_each(|scene| scene_rows(ui, scene));
        return true;
    }
    for (k, scene) in body.scenes.iter().enumerate() {
        let entities = tr_n!("{count} entity", "{count} entities", scene.entities.len());
        let key = scene.key.as_deref().unwrap_or_default();
        egui::CollapsingHeader::new(format!("{key} — {entities}"))
            .id_salt(("gta6_anim_scene", doc_id, k))
            .show(ui, |ui| scene_rows(ui, scene));
    }
    true
}

fn scene_rows(ui: &mut egui::Ui, scene: &Scene) {
    row(ui, tr!("Scene events"), scene.events.len());
    for entity in &scene.entities {
        row(
            ui,
            &format!("{} — {}", entity.key, entity.class),
            entity.events.len(),
        );
    }
}

fn row(ui: &mut egui::Ui, label: &str, events: usize) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(detail_label(tr_n!(
            "{count} event",
            "{count} events",
            events
        )));
    });
}
