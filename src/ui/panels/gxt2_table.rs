use egui::TextStyle;

use crate::state::{AppState, DocPayload};
use crate::ui::theme;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(doc_id) = state.active_document else {
        empty_state(ui);
        return;
    };
    if !matches!(
        state.document(doc_id).map(|d| &d.payload),
        Some(DocPayload::Gxt2(_) | DocPayload::Gta6Text(_))
    ) {
        empty_state(ui);
        return;
    }

    let query_id = egui::Id::new(("gxt2_search", doc_id));
    let mut query: String = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(query_id))
        .unwrap_or_default();

    let total = match state.document(doc_id).map(|d| &d.payload) {
        Some(DocPayload::Gxt2(t)) => t.len(),
        Some(DocPayload::Gta6Text(t)) => t.rows.len(),
        _ => 0,
    };
    // A GTA VI key is shown by the label the search holds where it hashes to it.
    let typed = or_io::hash::string_hash(query.trim().as_bytes());

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{total} entries")).strong());
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label("🔎");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Filter by hash (hex) or value…")
                .desired_width(240.0),
        );
        if resp.changed() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(query_id, query.clone()));
        }
        if !query.is_empty()
            && ui
                .button(egui_phosphor::regular::X)
                .on_hover_text("Clear filter")
                .clicked()
        {
            query.clear();
            ui.ctx()
                .data_mut(|d| d.insert_temp(query_id, query.clone()));
        }
    });
    ui.separator();

    // `None` shows every entry; `Some(indices)` is the filtered subset in the
    // table's native (hash-sorted) order.
    //
    // Cached on `(query, edit generation)`, so a steady query costs one lookup.
    // An uncached pass with a predicate allocating two `String`s per entry costs
    // 1.69 ms on a 23,136-entry table (`what_a_gxt2_filter_pass_costs`, release,
    // minimum over 20 passes), 10% of a 60 fps frame; the allocation-free
    // predicate below takes the miss, one per keystroke, to 0.48 ms (3.5x).
    //
    // ⚠ **That 3.5x is a release figure and it inverts in debug**, where
    // the hand-rolled predicate reads 0.29x — std's `contains` stays
    // optimised in a debug build of this crate and a `windows().any()`
    // does not. The cache wins in both.
    let generation = state.document(doc_id).map(|d| d.edit_epoch()).unwrap_or(0);
    let cache_id = egui::Id::new(("gxt2_filter", doc_id));
    let filtered: Option<Vec<usize>> = {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            None
        } else {
            let key = (needle.clone(), generation);
            let cached = ui
                .ctx()
                .data_mut(|d| d.get_temp::<Gxt2Filter>(cache_id))
                .filter(|c| c.key == key)
                .map(|c| c.indices);
            match cached {
                Some(indices) => Some(indices),
                None => {
                    let hash_needle = needle.strip_prefix("0x").unwrap_or(&needle);
                    let indices: Vec<usize> = match state.document(doc_id).map(|d| &d.payload) {
                        Some(DocPayload::Gxt2(t)) => t
                            .entries
                            .iter()
                            .enumerate()
                            .filter(|(_, e)| {
                                hex_contains(e.hash, hash_needle)
                                    || contains_ascii_lowercase(&e.value, &needle)
                            })
                            .map(|(i, _)| i)
                            .collect(),
                        Some(DocPayload::Gta6Text(t)) => (0..t.rows.len())
                            .filter(|i| {
                                t.row(*i).is_some_and(|(key, _, value)| {
                                    key == typed
                                        || hex_contains(key, hash_needle)
                                        || contains_ascii_lowercase(value, &needle)
                                        || (state.name_index.name_of(key))
                                            .is_some_and(|n| contains_ascii_lowercase(n, &needle))
                                })
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            cache_id,
                            Gxt2Filter {
                                key,
                                indices: indices.clone(),
                            },
                        )
                    });
                    Some(indices)
                }
            }
        }
    };

    let shown = filtered.as_ref().map(|f| f.len()).unwrap_or(total);
    if shown == 0 {
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(
            egui::RichText::new(if total == 0 {
                "This table is empty."
            } else {
                "No entries match the filter."
            })
            .color(theme::TEXT_SECONDARY),
        );
        return;
    }

    if filtered.is_some() {
        ui.label(
            egui::RichText::new(format!("Showing {shown} of {total}"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }

    let row_height = ui.text_style_height(&TextStyle::Body) + 8.0;
    // A GTA VI table has a column for the audio stream that speaks a row, where one is known.
    let spoken = matches!(
        state.document(doc_id).map(|d| &d.payload),
        Some(DocPayload::Gta6Text(_))
    );
    let mut edit: Option<(usize, String)> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, shown, |ui, range| {
            egui::Grid::new("gxt2_rows")
                .num_columns(if spoken { 3 } else { 2 })
                .striped(true)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for view_idx in range {
                        let entry_idx = filtered.as_ref().map(|f| f[view_idx]).unwrap_or(view_idx);

                        let (key, mut value, wave) = match state.document(doc_id).map(|d| &d.payload) {
                            Some(DocPayload::Gxt2(t)) => match t.entries.get(entry_idx) {
                                Some(e) => (format!("0x{:08X}", e.hash), e.value.clone(), None),
                                None => continue,
                            },
                            Some(DocPayload::Gta6Text(t)) => match t.row(entry_idx) {
                                Some((hash, form, text)) => {
                                    let line = crate::state::gta6_dialogue::line_of(hash);
                                    let name = match state.name_index.name_of(hash) {
                                        Some(name) => name.to_string(),
                                        None if hash == typed => query.trim().to_string(),
                                        // A dialogue line names its label.
                                        None => match &line {
                                            Some((_, label)) => label.clone(),
                                            None => format!("0x{hash:08X}"),
                                        },
                                    };
                                    let key = match form {
                                        Some(f) => tr!("{key}, form {n}", key = name, n = f + 1),
                                        None => name,
                                    };
                                    (key, text.to_string(), line.map(|l| l.0))
                                }
                                None => continue,
                            },
                            _ => continue,
                        };

                        ui.label(
                            egui::RichText::new(key)
                                .monospace()
                                .color(theme::legible(theme::KIND_GXT2, theme::AA)),
                        );
                        if spoken {
                            ui.label(egui::RichText::new(wave.unwrap_or_default()).monospace())
                                .on_hover_text(tr!(
                                    "The audio stream that speaks this line, where a conversation body opened from the scan states it"
                                ));
                        }

                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut value)
                                .desired_width(f32::INFINITY)
                                .font(TextStyle::Monospace),
                        );
                        if resp.changed() {
                            edit = Some((entry_idx, value));
                        }
                        ui.end_row();
                    }
                });
        });

    let Some((entry_idx, new_value)) = edit else {
        return;
    };
    let value = state.document_mut(doc_id);
    let Some(value) = value.and_then(|d| entry_value(&mut d.payload, entry_idx)) else {
        return;
    };
    if *value != new_value {
        let before = std::mem::replace(value, new_value);
        let change = crate::state::Change::part(format!("entry {entry_idx}"), before, move |p| {
            entry_value(p, entry_idx)
        });
        state.record(doc_id, format!("Edit entry {entry_idx}"), change);
    }
}

fn entry_value(payload: &mut DocPayload, entry: usize) -> Option<&mut String> {
    match payload {
        DocPayload::Gxt2(t) => t.entries.get_mut(entry).map(|e| &mut e.value),
        DocPayload::Gta6Text(t) => t.text_mut(entry),
        _ => None,
    }
}

#[derive(Clone)]
struct Gxt2Filter {
    key: (String, u64),
    indices: Vec<usize>,
}

fn hex_contains(hash: u32, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > 8 {
        return false;
    }
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0u8; 8];
    for (i, slot) in buf.iter_mut().enumerate() {
        *slot = DIGITS[((hash >> (28 - i * 4)) & 0xF) as usize];
    }
    buf.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn contains_ascii_lowercase(haystack: &str, needle: &str) -> bool {
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    let h = haystack.as_bytes();
    if h.len() < n.len() {
        return false;
    }
    h.windows(n.len())
        .any(|w| w.iter().zip(n).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

fn empty_state(ui: &mut egui::Ui) {
    theme::empty_message(ui, "No global text table open");
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_TABLE_ENTRIES: usize = 23_136;

    fn allocating_predicate(hash: u32, value: &str, needle: &str, hash_needle: &str) -> bool {
        let hash_hex = format!("{:08x}", hash);
        hash_hex.contains(hash_needle) || value.to_ascii_lowercase().contains(needle)
    }

    fn synthetic_table(n: usize) -> Vec<(u32, String)> {
        (0..n)
            .map(|i| {
                let hash = (i as u32).wrapping_mul(2_654_435_761);
                (
                    hash,
                    format!("Press ~INPUT_{i}~ to Continue, Agent {hash:X}"),
                )
            })
            .collect()
    }

    #[test]
    fn the_allocation_free_filter_matches_the_one_it_replaced() {
        let table = synthetic_table(2_000);
        let needles = [
            "a",
            "0",
            "ff",
            "00",
            "abcd",
            "agent",
            "AGENT",
            "continue",
            "press ~input_7",
            "zzzzzzzz",
            "123456789",
            "~",
            ",",
            " to ",
        ];
        let mut agreed = 0usize;
        let mut matched = 0usize;
        for raw in needles {
            let needle = raw.trim().to_ascii_lowercase();
            let hash_needle = needle.strip_prefix("0x").unwrap_or(&needle);
            for (hash, value) in &table {
                let old = allocating_predicate(*hash, value, &needle, hash_needle);
                let new =
                    hex_contains(*hash, hash_needle) || contains_ascii_lowercase(value, &needle);
                assert_eq!(
                    old, new,
                    "predicates disagree on needle {raw:?} / hash {hash:08x} / value {value:?}"
                );
                agreed += 1;
                if old {
                    matched += 1;
                }
            }
        }
        println!("{agreed} comparisons agreed, {matched} of them positive");
        assert!(
            matched > 1_000,
            "only {matched} positive matches — the needles are not exercising the filter"
        );
    }

    #[test]
    fn what_a_gxt2_filter_pass_costs() {
        use std::time::Instant;

        let table = synthetic_table(REAL_TABLE_ENTRIES);
        let needle = "input_17".to_string();
        let hash_needle = needle.as_str();
        const PASSES: usize = 20;

        for _ in 0..3 {
            let _ = table
                .iter()
                .filter(|(h, v)| allocating_predicate(*h, v, &needle, hash_needle))
                .count();
            let _ = table
                .iter()
                .filter(|(h, v)| {
                    hex_contains(*h, hash_needle) || contains_ascii_lowercase(v, &needle)
                })
                .count();
        }

        let (mut best_old, mut best_new) = (f64::MAX, f64::MAX);
        let (mut hits_old, mut hits_new) = (0usize, 0usize);
        for _ in 0..PASSES {
            let t = Instant::now();
            let n: usize = table
                .iter()
                .filter(|(h, v)| allocating_predicate(*h, v, &needle, hash_needle))
                .count();
            best_old = best_old.min(t.elapsed().as_secs_f64());
            hits_old = n;

            let t = Instant::now();
            let n: usize = table
                .iter()
                .filter(|(h, v)| {
                    hex_contains(*h, hash_needle) || contains_ascii_lowercase(v, &needle)
                })
                .count();
            best_new = best_new.min(t.elapsed().as_secs_f64());
            hits_new = n;
        }

        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        println!("--- one filter pass over {REAL_TABLE_ENTRIES} entries ({profile}) ---");
        println!(
            "  allocating (was)  {:>8.2} ms   ({} matches)",
            best_old * 1e3,
            hits_old
        );
        println!(
            "  allocation-free   {:>8.2} ms   ({} matches)",
            best_new * 1e3,
            hits_new
        );
        println!("  ratio             {:>8.2}x", best_old / best_new);
        println!(
            "  the old one ran every frame, so at 60 fps it spent {:.0}% of a \
             16.7 ms budget re-deriving an unchanged answer",
            100.0 * best_old * 1e3 / 16.7
        );

        assert_eq!(
            hits_old, hits_new,
            "the two passes disagree on how many rows match"
        );
        assert!(
            hits_new > 0 && hits_new < REAL_TABLE_ENTRIES / 4,
            "needle matched {hits_new} of {REAL_TABLE_ENTRIES} — pick one that \
             selects a minority so the miss path is what is being timed"
        );
        assert!(best_old > 0.0 && best_new > 0.0);

        #[cfg(not(debug_assertions))]
        assert!(
            best_new <= best_old,
            "release build: the allocation-free predicate is slower — \
             {:.2} ms vs {:.2} ms",
            best_new * 1e3,
            best_old * 1e3
        );
    }
}
