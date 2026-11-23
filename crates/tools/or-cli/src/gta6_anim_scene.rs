//! GTA VI's anim scenes on the command line: `rpf scan-open` on one prints its scenes, their
//! entities and their events, each as its body states it. Nothing is written.

use or_io::gta6::anim_scene::{self, AnimSceneBody, Event};

/// The scenes, events and entities `scan-open` lists of an anim scene before it counts the rest.
const ROWS: usize = 8;

/// Print the anim scene a headerless resource's sections hold, or say why its reader refuses it.
pub(crate) fn print_body(virtual_section: &[u8], physical: &[u8]) -> Result<(), String> {
    if !anim_scene::is_anim_scene(virtual_section) {
        return Ok(());
    }
    let body = anim_scene::open(virtual_section, physical)
        .map_err(|e| format!("GTA VI's anim-scene reader refuses this body: {e}"))?;
    lines(&body, ROWS).iter().for_each(|l| println!("{l}"));
    Ok(())
}

fn time(t: Option<f32>) -> String {
    match t {
        Some(t) => format!("{t:.3}"),
        None => "none".to_string(),
    }
}

fn event_line(lead: &str, e: &Event) -> String {
    let mut line = format!(
        "{lead} {} {} .. {} s, preload {}",
        e.class,
        time(e.start),
        time(e.end),
        time(e.preload)
    );
    if let Some(entity) = &e.entity {
        line.push_str(&format!(", entity {entity}"));
    }
    if let Some(clip) = &e.clip {
        line.push_str(&format!(", clip {} {}", clip.dictionary, clip.name));
    }
    line
}

/// What an anim scene states, with at most `rows` scenes, events and entities listed.
fn lines(body: &AnimSceneBody, rows: usize) -> Vec<String> {
    let mut out = vec![match body.map {
        true => format!(
            "  anim scene    a map of {} scenes (hash_233FA41B)",
            body.scenes.len()
        ),
        false => "  anim scene    one scene (hash_1EBDDB5B)".to_string(),
    }];
    for (k, scene) in body.scenes.iter().enumerate().take(rows) {
        let events: Vec<&Event> = scene.all_events().collect();
        let reversed = (events.iter())
            .filter(|e| e.span().is_some_and(|(start, end)| start > end))
            .count();
        let timeless = events.iter().filter(|e| e.span().is_none()).count();
        out.push(format!(
            "  scene {k} {}: {} events of its own, {} entities, {} events in all; {reversed} start after their end, {timeless} state no finite times",
            scene.key.as_deref().unwrap_or("(the body's one scene)"),
            scene.events.len(),
            scene.entities.len(),
            events.len()
        ));
        for event in scene.events.iter().take(rows) {
            out.push(event_line("    event", event));
        }
        for entity in scene.entities.iter().take(rows) {
            out.push(format!(
                "    entity {} {}: {} events",
                entity.key,
                entity.class,
                entity.events.len()
            ));
            for event in entity.events.iter().take(rows) {
                out.push(event_line("      event", event));
            }
        }
    }
    if body.scenes.len() > rows {
        out.push(format!("  ... {} more scenes", body.scenes.len() - rows));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::anim_scene::{Clip, Entity, Scene};

    fn event(start: f32, end: f32) -> Event {
        Event {
            class: "hash_0CB4F193".into(),
            start: Some(start),
            end: Some(end),
            preload: None,
            entity: Some("hash_00000A11".into()),
            clip: Some(Clip {
                dictionary: "hash_00000D1C".into(),
                name: "hash_00000C11".into(),
            }),
        }
    }

    #[test]
    fn a_scene_prints_its_counts_and_each_event_as_stated() {
        let scene = Scene {
            key: Some("hash_000005CE".into()),
            events: vec![event(0.5, 2.25)],
            entities: vec![Entity {
                key: "hash_00000E17".into(),
                class: "hash_64D97283".into(),
                events: vec![event(3.0, 1.0), event(1.0, 1.5)],
            }],
        };
        let body = AnimSceneBody {
            map: true,
            scenes: vec![scene.clone(), scene.clone(), scene],
        };
        let out = lines(&body, 2);
        assert_eq!(out[0], "  anim scene    a map of 3 scenes (hash_233FA41B)");
        assert!(out[1].starts_with("  scene 0 hash_000005CE: 1 events of its own, 1 entities, 3 events in all; 1 start after their end, 0 state"), "{}", out[1]);
        assert_eq!(
            out[2],
            "    event hash_0CB4F193 0.500 .. 2.250 s, preload none, entity hash_00000A11, clip hash_00000D1C hash_00000C11"
        );
        assert_eq!(out[3], "    entity hash_00000E17 hash_64D97283: 2 events");
        assert!(out[4].contains(" 3.000 .. 1.000 s"), "{}", out[4]);
        assert_eq!(out.last().map(String::as_str), Some("  ... 1 more scenes"));
        assert_eq!(out.len(), 1 + 2 * 5 + 1);
    }
}
