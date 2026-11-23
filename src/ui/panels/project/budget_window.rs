//! The budget window: a size and a format for each texture that fit a memory target, planned off the UI thread and shown, with the PSNR of every choice and the
//! texture as it is beside the texture as the plan would store it, before anything is written.
//!
//! Plan measures and writes nothing; Accept carries out the plan on show as one step of Undo. Nothing here chooses: `or-source::budget` does.

use crate::state::workbench::Question;
use crate::state::AppState;
use crate::ui::panels::{question_escaped, question_within};
use crate::ui::theme;

const TITLE: &str = tr_key!("Budget textures");

enum Answer {
    Plan,
    Accept,
    Close,
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut AppState,
    module: String,
    garments: Option<Vec<String>>,
) {
    let mut answer: Option<Answer> = None;
    question_within(ctx, crate::i18n::tr(TITLE), 720.0, |ui| {
        ui.label(match &garments {
            Some(ids) => tr!(
                "For the textures of the {count} garment(s) picked.",
                count = ids.len()
            ),
            None => tr!("For the textures of every garment in the pack.").to_string(),
        });
        ui.label(
            egui::RichText::new(
                tr!("Pick the memory the textures are to fit and Plan: each texture is given the size and format that lose the least picture per byte saved, instead of one cut for all. Nothing is written until Accept; a source is never changed in place, the garments name a lighter copy and Undo takes it back. A texture made from a picture is left, and the Log says why."),
            )
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_SNUG);
        let planning = state.workbench.wardrobe.textures.is_planning();
        let busy = state.is_planning_a_budget();
        ui.horizontal(|ui| {
            ui.label(tr!("Memory"));
            let desk = &mut state.workbench.wardrobe.textures;
            let field = ui.add(
                egui::DragValue::new(&mut desk.target_mb)
                    .range(0.01..=4096.0)
                    .speed(0.5)
                    .max_decimals(2)
                    .suffix(tr!(" MB")),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::DragValue,
                    true,
                    tr!("Texture memory target"),
                )
            });
            field.on_hover_text(
                tr!("The bytes of the mips of the textures the budget can change: those that are a DDS in a source"),
            );
            if ui
                .add_enabled(!planning, egui::Button::new(tr!("Plan")))
                .clicked()
            {
                answer = Some(Answer::Plan);
            }
            if busy {
                ui.spinner();
            }
        });
        plan_body(ui, ctx, state);
        ui.separator();
        ui.horizontal(|ui| {
            let can = state
                .workbench
                .wardrobe
                .textures
                .plan
                .as_ref()
                .is_some_and(|p| p.plan.textures() > 0);
            if ui
                .add_enabled(can, egui::Button::new(tr!("Accept")))
                .clicked()
            {
                answer = Some(Answer::Accept);
            }
            if ui.button(tr!("Close")).clicked() {
                answer = Some(Answer::Close);
            }
        });
    });
    if question_escaped(ctx, crate::i18n::tr(TITLE)) {
        answer = Some(Answer::Close);
    }
    match answer {
        Some(Answer::Plan) => {
            state.plan_budget(module.clone(), garments.clone());
            state.workbench.asking = Some(Question::Budget { module, garments });
        }
        Some(Answer::Accept) => {
            state.workbench.asking = None;
            state.accept_budget();
        }
        Some(Answer::Close) => state.workbench.asking = None,
        None => state.workbench.asking = Some(Question::Budget { module, garments }),
    }
}

/// The plan on show: what it comes to, each choice with its PSNR, and the chosen one side by side.
fn plan_body(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut AppState) {
    let desk = &state.workbench.wardrobe.textures;
    if let Some(why) = &desk.failed {
        ui.label(egui::RichText::new(why).small().color(theme::ERROR));
        return;
    }
    let Some(shown) = &desk.plan else {
        return;
    };
    let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
    let plan = &shown.plan;
    ui.add_space(theme::SPACING_TIGHT);
    let against = tr!(
        "{textures} texture(s) change: {before} now, {after} after, against a target of {target}",
        textures = plan.textures(),
        before = mb(plan.before),
        after = mb(plan.after),
        target = mb(plan.target),
    );
    ui.label(if plan.met {
        against
    } else {
        tr!("{against}, which is as low as they go", against = against)
    });
    let mut pick = None;
    let chosen = desk.chosen;
    egui::ScrollArea::vertical()
        .id_salt("budget_choices")
        .max_height(170.0)
        .show(ui, |ui| {
            for (i, (source, choice)) in shown.choices().enumerate() {
                let (was, now) = (choice.was(), choice.now());
                let db = now.psnr.map_or_else(
                    || tr!("same picture").to_string(),
                    |db| format!("{db:.1} dB"),
                );
                let text = format!(
                    "{}  {}x{} {} -> {}x{} {}  ({} -> {} KB, {db})",
                    choice.ladder.texture,
                    was.width,
                    was.height,
                    was.format,
                    now.width,
                    now.height,
                    now.format,
                    was.bytes / 1024,
                    now.bytes / 1024,
                );
                if ui
                    .selectable_label(i == chosen, egui::RichText::new(text).small().monospace())
                    .on_hover_text(tr!("In {source}", source = source))
                    .clicked()
                {
                    pick = Some(i);
                }
            }
        });
    if let Some(i) = pick {
        state.look_at_budget_choice(i);
    }
    let desk = &state.workbench.wardrobe.textures;
    if let Some((index, both)) = &desk.pictures {
        ui.add_space(theme::SPACING_TIGHT);
        ui.horizontal(|ui| {
            for (side, label) in [(0usize, tr!("As it is")), (1, tr!("As the plan stores it"))] {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(label).small());
                    let image = &both[side];
                    let pair = std::sync::Arc::as_ptr(both) as usize;
                    let handle = picture_handle(ctx, *index, side, pair, image);
                    let longest = image.width.max(image.height).max(1) as f32;
                    let scale = 220.0 / longest;
                    ui.image((
                        handle.id(),
                        egui::vec2(image.width as f32 * scale, image.height as f32 * scale),
                    ));
                });
            }
        });
    }
}

/// The picture as a texture egui draws, made once for the pair of pictures on show.
fn picture_handle(
    ctx: &egui::Context,
    index: usize,
    side: usize,
    pair: usize,
    image: &or_io::codec::texture::RgbaImage,
) -> egui::TextureHandle {
    let id = egui::Id::new(("budget_picture", index, side, pair));
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }
    let colour = egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.data,
    );
    let handle = ctx.load_texture(
        format!("budget_picture_{index}_{side}"),
        colour,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}
