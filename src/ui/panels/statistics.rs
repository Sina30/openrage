//! The Statistics panel: the frame time and its phases, the texture preview cache and the
//! viewport's GPU textures against their budgets, the GPU memory no budget evicts, egui's own
//! textures, each open document's bytes, the app's other long-lived CPU state and wgpu's own
//! objects, read from the counters the app already keeps.

use crate::render::gpu_budget::{DriverObjects, GpuUsage};
use crate::state::frame_stats::FramePhase;
use crate::state::memory::CpuState;
use crate::state::AppState;
use crate::ui::theme;

const MIB: usize = 1024 * 1024;
const BUDGET_CHOICES_MIB: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

/// What the panel shows, separate from drawing it.
pub struct Readings {
    pub fps: f32,
    pub frame_ms: f32,
    pub phase_ms: [f32; FramePhase::ALL.len()],
    pub cache_bytes: usize,
    pub cache_budget: usize,
    pub gpu: GpuUsage,
    pub gpu_budget: usize,
    /// The stages of the last viewport frame the device timed, with the milliseconds each took.
    pub gpu_stages: Vec<(&'static str, f32)>,
    /// egui's textures other than the texture previews: the font atlas, icons and images.
    pub egui_texture_bytes: usize,
    /// Each open document's name and the bytes it holds on the CPU.
    pub documents: Vec<(String, usize)>,
    /// The app's other long-lived CPU state, each structure at what it holds.
    pub cpu: CpuState,
}

/// What a reading walks the app's documents and structures for.
#[derive(Clone)]
struct Walked {
    documents: Vec<(String, usize)>,
    cpu: CpuState,
}

fn walked(state: &AppState) -> Walked {
    Walked {
        documents: (state.documents.iter())
            .map(|doc| (doc.name.clone(), doc.held_bytes()))
            .collect(),
        cpu: state.cpu_state(),
    }
}

/// How long the panel keeps what it walked, which visits every string and record the app holds.
const WALK_EVERY: f64 = 1.0;

/// The walk the last second made, or a new one.
fn walked_lately(ctx: &egui::Context, state: &AppState) -> Walked {
    let id = egui::Id::new("statistics-walked");
    let now = ctx.input(|input| input.time);
    let last = ctx.data(|data| data.get_temp::<(f64, Walked)>(id));
    if let Some((_, kept)) = last.filter(|(at, _)| (0.0..WALK_EVERY).contains(&(now - at))) {
        return kept;
    }
    let fresh = walked(state);
    ctx.data_mut(|data| data.insert_temp(id, (now, fresh.clone())));
    fresh
}

/// The readings with every walk made fresh, which the tests read; `draw` keeps a walk for a second.
#[cfg(test)]
pub fn readings(state: &AppState, ctx: &egui::Context) -> Readings {
    readings_of(state, ctx, walked(state))
}

fn readings_of(state: &AppState, ctx: &egui::Context, walked: Walked) -> Readings {
    let previews = state.texture_cache.texture_ids();
    let egui_texture_bytes = ctx
        .tex_manager()
        .read()
        .allocated()
        .filter(|(id, _)| !previews.contains(id))
        .map(|(_, meta)| meta.bytes_used())
        .sum();
    Readings {
        fps: state.frame_stats.fps(),
        frame_ms: state.frame_stats.frame_ms(),
        phase_ms: state.frame_stats.phase_ms(),
        cache_bytes: state.texture_cache.bytes(),
        cache_budget: state.texture_cache.budget(),
        gpu: state.gpu_usage,
        gpu_budget: state.prefs.gpu_texture_budget(),
        gpu_stages: state.gpu_stages.clone(),
        egui_texture_bytes,
        cpu: walked.cpu,
        documents: walked.documents,
    }
}

fn mib_text(bytes: usize) -> String {
    format!("{:.1} MiB", bytes as f64 / MIB as f64)
}

fn budget_combo(ui: &mut egui::Ui, id: &str, mib: &mut u32) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{mib} MiB"))
        .show_ui(ui, |ui| {
            for choice in BUDGET_CHOICES_MIB {
                ui.selectable_value(mib, choice, format!("{choice} MiB"));
            }
        });
}

/// Each long-lived structure on the CPU, with what it holds.
fn cpu_rows(cpu: &CpuState) -> [(&'static str, &'static str, usize); 8] {
    [
        (
            "Name index",
            "The names the pickers offer, the snapshot they read and the hash index over them",
            cpu.name_index,
        ),
        (
            "File index",
            "Every path of the install, folder or archive Files shows, which Go to File and the \
             search of every archive read",
            cpu.file_index,
        ),
        (
            "Search results",
            "The hits of the last search of the install and the matches of each archive's find",
            cpu.search_results,
        ),
        (
            "Archive trees",
            "A row for each entry the open archives list, with its name and where it reads from",
            cpu.archive_tree,
        ),
        (
            "Archive tables of contents",
            "Each archive the trees opened, once: its entries, their names and the tree built over \
             them",
            cpu.archive_tables,
        ),
        (
            "Undo history",
            "What every document's steps keep: the images, bytes, rows and textures they swap back",
            cpu.undo_history,
        ),
        (
            "Simulation",
            "The effect that plays: its particles, the rules it runs from and the snapshots a \
             seek replays from",
            cpu.simulation,
        ),
        (
            "Uses and Placed by",
            "What the first Uses or Placed by read: the install's mount and file catalogue and the \
             placements. The archetype, table and model indexes behind them are not counted",
            cpu.dependency_index,
        ),
    ]
}

/// wgpu's own objects, each a count where wgpu gives no size, and the allocator's two totals.
fn driver_rows(driver: &DriverObjects) -> [(&'static str, &'static str, String); 5] {
    const NO_SIZE: &str = "wgpu reports a count for these and no size";
    let memory = match driver.allocator {
        Some((used, reserved)) => format!(
            "{} of {} reserved",
            mib_text(used as usize),
            mib_text(reserved as usize)
        ),
        None => "not reported by this backend".to_string(),
    };
    [
        (
            "Driver pipelines",
            NO_SIZE,
            format!(
                "{} render, {} compute",
                driver.render_pipelines, driver.compute_pipelines
            ),
        ),
        (
            "Driver shader modules",
            NO_SIZE,
            driver.shader_modules.to_string(),
        ),
        (
            "Driver bind groups",
            NO_SIZE,
            format!(
                "{} in {} layouts, {} pipeline layouts",
                driver.bind_groups, driver.bind_group_layouts, driver.pipeline_layouts
            ),
        ),
        ("Driver samplers", NO_SIZE, driver.samplers.to_string()),
        (
            "Driver memory",
            "What the backend's allocator reports for buffers and textures: the GPU lines above \
             are counted from their formats and extents and are checked against it, not added to it",
            memory,
        ),
    ]
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    let r = readings_of(state, ui.ctx(), walked_lately(ui.ctx(), state));
    egui::Grid::new("statistics")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Frame time");
            if r.frame_ms > 0.0 {
                ui.label(format!("{:.1} ms ({:.0} fps)", r.frame_ms, r.fps));
            } else {
                ui.label(egui::RichText::new("no frame yet").color(theme::TEXT_SECONDARY));
            }
            ui.end_row();

            for (phase, ms) in FramePhase::ALL.into_iter().zip(r.phase_ms) {
                ui.label(format!("    {}", phase.label()));
                ui.label(format!("{ms:.2} ms"));
                ui.end_row();
            }
            ui.label("    Waiting and other")
                .on_hover_text("Frame time outside the timed phases, mostly waiting for input");
            let timed: f32 = r.phase_ms.iter().sum();
            ui.label(format!("{:.2} ms", (r.frame_ms - timed).max(0.0)));
            ui.end_row();

            if !r.gpu_stages.is_empty() {
                ui.label("GPU frame").on_hover_text(
                    "How long the device took to draw the last viewport frame, by stage; the CPU &
                     frame time above runs alongside it",
                );
                ui.label(format!(
                    "{:.3} ms",
                    r.gpu_stages.iter().map(|(_, ms)| ms).sum::<f32>()
                ));
                ui.end_row();
                for (stage, ms) in &r.gpu_stages {
                    ui.label(format!("    {stage}"));
                    ui.label(format!("{ms:.3} ms"));
                    ui.end_row();
                }
            }

            ui.label("Texture previews");
            ui.add(
                egui::ProgressBar::new(r.cache_bytes as f32 / r.cache_budget.max(1) as f32).text(
                    format!("{} of {} MiB", r.cache_bytes / MIB, r.cache_budget / MIB),
                ),
            );
            ui.end_row();

            ui.label("Texture preview budget")
                .on_hover_text("Decoded previews past this are evicted, least recently used first");
            let mut mib = state.prefs.texture_cache_mib;
            budget_combo(ui, "texture_cache_budget", &mut mib);
            if mib != state.prefs.texture_cache_mib {
                state.set_texture_cache_budget_mib(mib);
            }
            ui.end_row();

            ui.label("GPU textures");
            ui.add(
                egui::ProgressBar::new(r.gpu.texture_bytes as f32 / r.gpu_budget.max(1) as f32)
                    .text(format!(
                        "{} of {} MiB",
                        r.gpu.texture_bytes / MIB,
                        r.gpu_budget / MIB
                    )),
            );
            ui.end_row();

            ui.label("GPU mesh buffers");
            ui.label(format!("{} MiB", r.gpu.buffer_bytes / MIB));
            ui.end_row();

            ui.label("GPU render targets").on_hover_text(
                "The viewport renderer's own targets: sized by the viewport and never evicted",
            );
            ui.label(mib_text(r.gpu.target_bytes));
            ui.end_row();

            ui.label("GPU gallery thumbnails")
                .on_hover_text("The gallery's tiles and the renderer that draws them");
            ui.label(mib_text(r.gpu.thumbnail_bytes));
            ui.end_row();

            let resident = [
                (
                    "GPU line buffers",
                    "The grid, gizmo and bounds lines",
                    r.gpu.line_buffer_bytes,
                ),
                (
                    "GPU particle instance buffers",
                    "The particle, trail and model passes' instance and quad buffers",
                    r.gpu.particle_instance_bytes,
                ),
                (
                    "GPU uniform and storage buffers",
                    "The passes' uniforms",
                    r.gpu.uniform_buffer_bytes,
                ),
                (
                    "GPU default textures",
                    "The 1x1 textures bound where nothing else is",
                    r.gpu.default_texture_bytes,
                ),
            ];
            for (label, hover, bytes) in resident {
                ui.label(label).on_hover_text(hover);
                ui.label(mib_text(bytes));
                ui.end_row();
            }

            ui.label("GPU texture budget").on_hover_text(
                "Viewport textures past this are evicted, least recently used first",
            );
            let mut mib = state.prefs.gpu_texture_mib;
            budget_combo(ui, "gpu_texture_budget", &mut mib);
            if mib != state.prefs.gpu_texture_mib {
                state.set_gpu_texture_budget_mib(mib);
            }
            ui.end_row();

            ui.label("egui textures")
                .on_hover_text("The font atlas, icons and images egui holds, besides the previews");
            ui.label(mib_text(r.egui_texture_bytes));
            ui.end_row();

            ui.label("Documents").on_hover_text(
                "What each open document holds on the CPU: the image it was read from and its \
                 contents, a resource's at what its value allocates",
            );
            ui.label(mib_text(r.documents.iter().map(|(_, bytes)| bytes).sum()));
            ui.end_row();
            for (name, bytes) in &r.documents {
                ui.label(format!("    {name}"));
                ui.label(mib_text(*bytes));
                ui.end_row();
            }

            for (label, hover, bytes) in cpu_rows(&r.cpu) {
                ui.label(label).on_hover_text(hover);
                ui.label(mib_text(bytes));
                ui.end_row();
            }

            for (label, hover, text) in driver_rows(&r.gpu.driver) {
                ui.label(label).on_hover_text(hover);
                ui.label(text);
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::codec::texture::{ChannelView, GrcTextureFormat};

    fn texture(edge: u16) -> crate::state::textures::Texture<'static> {
        let format = GrcTextureFormat::A8R8G8B8;
        let side = u32::from(edge);
        crate::state::textures::Texture::Owned(std::borrow::Cow::Owned(
            or_io::gta5::gen8::GrcTexture {
                name: "stats".into(),
                width: edge,
                height: edge,
                depth: 1,
                format: format.to_raw_format(),
                mip_count: 1,
                backing_store: vec![0; format.total_size(side, side, 1) as usize],
                ..Default::default()
            },
        ))
    }

    #[test]
    fn the_panel_reads_the_live_counters() {
        let mut state = AppState::new();
        let ctx = egui::Context::default();
        let before = readings(&state, &ctx);
        assert_eq!(before.cache_bytes, 0);
        assert_eq!(
            before.cache_budget,
            state.prefs.texture_cache_mib as usize * 1024 * 1024
        );

        state
            .texture_cache
            .get_or_decode(&ctx, (1, "stats".into(), ChannelView::Rgba), &texture(64))
            .expect("decodes");
        state.tick_frame_stats();
        std::thread::sleep(std::time::Duration::from_millis(2));
        state.tick_frame_stats();

        let after = readings(&state, &ctx);
        assert_eq!(after.cache_bytes, 64 * 64 * 4);
        assert!(after.fps > 0.0);
        assert_eq!(after.fps, state.frame_stats.fps());
    }

    #[test]
    fn the_panel_breaks_the_frame_time_into_phases_that_sum_to_no_more_than_it() {
        let mut state = AppState::new();
        state.tick_frame_stats();
        for frame in 1..=3u64 {
            for phase in FramePhase::ALL {
                let started = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(frame));
                state.frame_stats.record(phase, started.elapsed());
            }
            state.tick_frame_stats();

            let r = readings(&state, &egui::Context::default());
            assert_eq!(r.phase_ms.len(), FramePhase::ALL.len());
            assert!(r.phase_ms.iter().all(|&ms| ms >= 1.0), "{:?}", r.phase_ms);
            let sum: f32 = r.phase_ms.iter().sum();
            assert!(
                sum <= r.frame_ms,
                "phases {sum} ms > frame {} ms",
                r.frame_ms
            );
        }
    }

    #[test]
    fn the_panel_reads_the_stages_the_device_timed() {
        let mut state = AppState::new();
        assert!(readings(&state, &egui::Context::default())
            .gpu_stages
            .is_empty());
        state.gpu_stages = vec![("g-buffer", 0.25), ("lighting", 0.5)];

        let r = readings(&state, &egui::Context::default());
        assert_eq!(r.gpu_stages, vec![("g-buffer", 0.25), ("lighting", 0.5)]);
    }

    #[test]
    fn the_panel_reads_the_gpu_counters_against_the_preference() {
        let mut state = AppState::new();
        state.gpu_usage = crate::render::gpu_budget::GpuUsage {
            texture_bytes: 3 * MIB,
            buffer_bytes: MIB,
            ..Default::default()
        };
        state.set_gpu_texture_budget_mib(256);

        let r = readings(&state, &egui::Context::default());
        assert_eq!(r.gpu.texture_bytes, 3 * MIB);
        assert_eq!(r.gpu.buffer_bytes, MIB);
        assert_eq!(r.gpu_budget, 256 * MIB);
        assert_eq!(state.prefs.gpu_texture_mib, 256);
    }

    #[test]
    fn the_panel_shows_each_class_it_counts_once_it_is_allocated() {
        let mut state = AppState::new();
        let ctx = egui::Context::default();
        let before = readings(&state, &ctx);
        assert!(before.documents.is_empty());
        assert_eq!(before.egui_texture_bytes, 0);
        assert_eq!(before.gpu, GpuUsage::default());

        state.documents.push(crate::state::Document::new(
            1,
            Some("notes.txt".into()),
            crate::state::DocPayload::Text(crate::state::TextDocument::decode(b"hello")),
        ));
        let _icon = ctx.load_texture(
            "icon",
            egui::ColorImage::filled([8, 8], egui::Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        state
            .texture_cache
            .get_or_decode(&ctx, (1, "stats".into(), ChannelView::Rgba), &texture(64))
            .expect("decodes");
        state.gpu_usage = GpuUsage {
            line_buffer_bytes: 2,
            particle_instance_bytes: 3,
            uniform_buffer_bytes: 4,
            default_texture_bytes: 5,
            ..Default::default()
        };

        let after = readings(&state, &ctx);
        assert_eq!(after.documents, vec![("notes".to_string(), 5)]);
        assert_eq!(
            after.egui_texture_bytes,
            8 * 8 * 4,
            "the texture previews are their own line"
        );
        assert_eq!(after.gpu.line_buffer_bytes, 2);
        assert_eq!(after.gpu.particle_instance_bytes, 3);
        assert_eq!(after.gpu.uniform_buffer_bytes, 4);
        assert_eq!(after.gpu.default_texture_bytes, 5);
    }

    #[test]
    fn the_panel_reads_the_gpu_memory_outside_the_viewport_textures() {
        let mut state = AppState::new();
        state.gpu_usage = GpuUsage {
            target_bytes: 5 * MIB,
            thumbnail_bytes: 7 * MIB,
            ..Default::default()
        };

        let r = readings(&state, &egui::Context::default());
        assert_eq!(r.gpu.target_bytes, 5 * MIB);
        assert_eq!(r.gpu.thumbnail_bytes, 7 * MIB);
    }

    #[test]
    fn the_panel_gives_each_cpu_structure_a_line_of_its_own() {
        let cpu = CpuState {
            name_index: 1,
            file_index: 2,
            search_results: 3,
            archive_tree: 4,
            archive_tables: 5,
            undo_history: 6,
            simulation: 7,
            dependency_index: 8,
        };
        let mut shown: Vec<usize> = cpu_rows(&cpu).iter().map(|(_, _, bytes)| *bytes).collect();
        shown.sort_unstable();
        assert_eq!(shown, (1..=8).collect::<Vec<_>>(), "a figure no line shows");
        let labels: std::collections::BTreeSet<_> =
            cpu_rows(&cpu).iter().map(|(label, ..)| *label).collect();
        assert_eq!(labels.len(), 8, "two figures under one label");
    }

    #[test]
    fn the_panel_draws_every_cpu_structure_and_every_driver_count() {
        let mut state = AppState::new();
        let driver = DriverObjects {
            render_pipelines: 3,
            ..Default::default()
        };
        state.gpu_usage = GpuUsage {
            driver,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut painted = Vec::new();
        // ⚠ A grid draws nothing on its first frame, while it measures its columns.
        for _ in 0..2 {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| draw(ui, &mut state));
            painted = (output.shapes.iter())
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                    _ => None,
                })
                .collect();
        }
        let cpu = cpu_rows(&CpuState::default()).map(|(label, ..)| label);
        let counted = driver_rows(&driver).map(|(label, ..)| label);
        for label in cpu.into_iter().chain(counted) {
            assert!(
                painted.iter().any(|text| text == label),
                "no line {label:?}"
            );
        }
        assert!(painted.iter().any(|text| text == "3 render, 0 compute"));
    }

    #[test]
    fn the_panel_shows_wgpus_objects_as_counts_and_its_allocator_as_bytes() {
        let mut state = AppState::new();
        let driver = DriverObjects {
            render_pipelines: 3,
            compute_pipelines: 1,
            shader_modules: 4,
            bind_groups: 7,
            bind_group_layouts: 2,
            pipeline_layouts: 3,
            samplers: 5,
            allocator: Some((MIB as u64, 4 * MIB as u64)),
        };
        state.gpu_usage = GpuUsage {
            driver,
            ..Default::default()
        };
        let r = readings(&state, &egui::Context::default());
        assert_eq!(r.gpu.driver, driver);
        let rows = driver_rows(&r.gpu.driver);
        let shown: Vec<&str> = rows.iter().map(|(_, _, text)| text.as_str()).collect();
        assert_eq!(
            shown,
            [
                "3 render, 1 compute",
                "4",
                "7 in 2 layouts, 3 pipeline layouts",
                "5",
                "1.0 MiB of 4.0 MiB reserved"
            ]
        );
        let none = driver_rows(&DriverObjects::default());
        assert_eq!(none[4].2, "not reported by this backend");
    }

    #[test]
    fn the_panel_walks_the_apps_structures_once_a_second_not_every_frame() {
        use crate::state::name_index::NameIndexCategory;

        let ctx = egui::Context::default();
        let mut state = AppState::new();
        let at = |time: f64, state: &AppState| {
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut seen = None;
            let _ = ctx.run_ui(input, |ui| seen = Some(walked_lately(ui.ctx(), state)));
            seen.expect("the frame ran").cpu.name_index
        };
        let first = at(10.0, &state);
        state
            .name_index
            .add(NameIndexCategory::Model, "prop_after_the_walk");
        assert_eq!(
            at(10.5, &state),
            first,
            "the second after a walk reads it again"
        );
        assert!(
            at(11.5, &state) > first,
            "a second on, the names are walked again"
        );
    }
}
