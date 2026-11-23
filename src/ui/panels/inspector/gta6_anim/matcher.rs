//! A motion dictionary's matchers: each one's clips, entries, frames and database, as GTA VI's
//! animation reader states them.

use or_io::gta6::anim::{Matcher, MatcherClass, MatcherLevel};

use super::values::{halves_text, scrolled};
use super::*;

/// Lanes of a level's node.
const LANES: usize = 8;

pub(super) fn draw_matchers(ui: &mut egui::Ui, doc_id: DocumentId, resource: &AnimResource) {
    for (m, matcher) in resource.matchers.iter().enumerate() {
        egui::CollapsingHeader::new(&matcher.name)
            .id_salt(("gta6_anim_matcher", doc_id, m))
            .show(ui, |ui| draw_matcher(ui, (doc_id, m), matcher));
    }
}

fn draw_matcher(ui: &mut egui::Ui, id: (DocumentId, usize), matcher: &Matcher) {
    let database = &matcher.database;
    grid(ui, &format!("gta6_anim_matcher_{id:?}"), |ui| {
        row(ui, tr!("Key"), &format!("{:#010X}", matcher.key));
        row(ui, tr!("Clips, by key"), &matcher.clips.len().to_string());
        row(ui, tr!("Entries"), &matcher.entries.len().to_string());
        row(ui, tr!("Frames"), &database.frames.to_string());
        row(ui, tr!("Bones"), &database.skeleton.bones.len().to_string());
        row(ui, tr!("Features"), &database.features.len().to_string());
        let class = match database.class {
            MatcherClass::Vectors { .. } => "1",
            MatcherClass::Levels { .. } => "3",
        };
        row(ui, tr!("Database class"), class);
        let weights: Vec<String> = database.weights.iter().map(|w| format!("{w:.4}")).collect();
        row(ui, tr!("Dimension weights"), &weights.join(" "));
    });
    for clip in &matcher.clips {
        let dictionary = clip.dictionary.map_or_else(
            || tr!("by an alias word whose source is not known").to_string(),
            |key| format!("{key:#010X}"),
        );
        ui.monospace(format!(
            "{:#010X}:{:#010X} {dictionary}",
            clip.alias, clip.leaf
        ));
    }
    for feature in &database.features {
        let [a, b, c] = feature.words;
        ui.monospace(format!(
            "{} kind {} dims {}+{} {a:#010X} {b:#010X} {c:#010X}",
            named(feature.name),
            feature.kind,
            feature.offset,
            feature.dims
        ));
    }
    let entries = egui::CollapsingHeader::new(tr!("Entries")).id_salt((id, "entries"));
    entries.show(ui, |ui| {
        ui.label(tr!(
            "Each entry: its clip, its start in seconds, its rate and its frames"
        ));
        scrolled(ui, (id, "entry_lines"), matcher.entries.len(), |i| {
            let e = &matcher.entries[i];
            let (start, end) = (e.frames.start, e.frames.end);
            format!("{}  {}  {}  {start}..{end}", e.clip, e.start, e.rate)
        });
    });
    let (vectors, order) = match &database.class {
        MatcherClass::Vectors { vectors, .. } => (vectors, None),
        MatcherClass::Levels {
            stored,
            dimension_order,
            ..
        } => (stored, Some(dimension_order)),
    };
    let frames = egui::CollapsingHeader::new(tr!("Frames")).id_salt((id, "frames"));
    frames.show(ui, |ui| {
        ui.label(tr!("Each frame's vector of 16-bit floats, as stored"));
        if let Some(order) = order {
            ui.label(tr!(
                "By an inferred reading, stored dimension k holds the feature dimension at place k of this order: {order}",
                order = format!("{order:?}")
            ));
        }
        scrolled(ui, (id, "frame_lines"), vectors.len(), |f| {
            format!("{f}: {}", halves_text(&vectors[f]))
        });
    });
    if let MatcherClass::Levels { pairs, levels, .. } = &database.class {
        for (l, level) in levels.iter().enumerate() {
            draw_level(ui, (id, l), l, level, usize::from(*pairs));
        }
    }
    ui.label(
        egui::RichText::new(tr!("Not read: what a frame's halves mean, where a clip's alias word comes from, the roles of the database's hash at 0x68 and of its frame words, what picks the word a kind-5 feature's hash takes, and which way a class-3 database's dimension order runs."))
        .color(theme::TEXT_SECONDARY),
    );
}

/// Level `l`: for each node and lane, the least and greatest of each stored dimension.
fn draw_level(
    ui: &mut egui::Ui,
    salt: ((DocumentId, usize), usize),
    l: usize,
    level: &MatcherLevel,
    pairs: usize,
) {
    let title = tr!(
        "Level {level}: lanes of {frames} frames, each the least and greatest of every stored dimension over its frames",
        level = l,
        frames = level.lane_frames
    );
    let nodes = level.minima.len().checked_div(pairs).unwrap_or(0);
    let header = egui::CollapsingHeader::new(title).id_salt(("gta6_anim_level", salt));
    header.show(ui, |ui| {
        scrolled(ui, ("gta6_anim_level_lines", salt), nodes * LANES, |i| {
            let (node, lane) = (i / LANES, i % LANES);
            let mut bounds = Vec::with_capacity(2 * pairs);
            for k in 0..2 * pairs {
                let (element, half) = (node * pairs + k / 2, LANES * (k % 2) + lane);
                let low = level.minima[element][half];
                let high = level.maxima[element][half];
                bounds.push(format!("{}..{}", halves_text(&[low]), halves_text(&[high])));
            }
            format!("{node}.{lane}: {}", bounds.join(" "))
        });
    });
}
