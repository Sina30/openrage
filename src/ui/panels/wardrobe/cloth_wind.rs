//! The wind of a garment's cloth, in the Cloth section of the Garment panel: what the linked file states, and the scale the build states in its place.
//!
//! ⚠ Thin by design. The scale is the garment's own statement in `wardrobe.json` (`Edit::Wind`), the build writes it into the
//! cloth it ships and reads the shipped file back (`or_wardrobe::wind`); the linked file is never touched.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_wardrobe::edit::Edit;
use or_wardrobe::{wind, Garment};

use crate::ui::theme;

/// How long what a cloth file states is kept before the file is looked at again.
const KEPT: Duration = Duration::from_secs(2);

/// What a cloth file stated when it was looked at: the pack's folder, the path of the cloth, when, and what it said.
type Looked = (PathBuf, String, Instant, Result<Vec<f32>, String>);

/// What the control keeps between frames.
#[derive(Default)]
pub(super) struct Ui {
    /// The scale being set: the garment and the number, kept until the field is let go.
    draft: Option<(String, f32)>,
    /// What the linked file stated when it was last looked at: the pack's folder, the path of the cloth, when, and what it said.
    file: Option<Looked>,
}

impl Ui {
    /// The wind scale each cloth of `cloth`, a path inside `folder`, states, looked at again once it has been kept a while.
    fn stated(&mut self, folder: &Path, cloth: &str) -> &Result<Vec<f32>, String> {
        let fresh = self.file.as_ref().is_some_and(|(of, named, at, _)| {
            of == folder && named == cloth && at.elapsed() < KEPT
        });
        if !fresh {
            let read = wind::stated_by(folder, cloth);
            self.file = Some((
                folder.to_path_buf(),
                cloth.to_string(),
                Instant::now(),
                read,
            ));
        }
        &self.file.as_ref().expect("just read").3
    }
}

/// The scales a file states as one phrase: the one number when they agree.
fn phrase(winds: &[f32]) -> String {
    match winds {
        [] => tr!("no cloth with a controller").to_string(),
        [first, rest @ ..] if rest.iter().all(|w| w == first) => format!("{first}"),
        many => many
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The wind row of the garment's cloth, below the link; it asks for `Edit::Wind` in `asked` once a number is let go.
pub(super) fn draw(
    ui: &mut egui::Ui,
    shown: &mut Ui,
    folder: &Path,
    g: &Garment,
    asked: &mut Option<(String, Edit)>,
) {
    let small = |ui: &mut egui::Ui, text: String| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .small()
                    .color(theme::TEXT_SECONDARY),
            )
            .wrap(),
        );
    };
    let Some(cloth) = g.cloth.as_deref() else {
        if let Some(scale) = g.wind_scale {
            small(
                ui,
                tr!("A wind scale of {scale} is stated and no cloth is linked, so it is written nowhere", scale = scale),
            );
            if ui.small_button(tr!("Forget the wind scale")).clicked() {
                *asked = Some((g.id.clone(), Edit::Wind(None)));
            }
        }
        return;
    };
    if g.slot.is_some_and(or_wardrobe::Slot::is_prop) {
        return;
    }
    let file = shown.stated(folder, cloth).clone();
    let from_file = file.as_ref().ok().and_then(|w| w.first().copied());
    let now = g.wind_scale.or(from_file).unwrap_or(wind::DEFAULT);
    ui.horizontal(|ui| {
        ui.label(tr!("Wind scale"));
        let mut value = match &shown.draft {
            Some((who, v)) if *who == g.id => *v,
            _ => now,
        };
        let field = ui.add(
            egui::DragValue::new(&mut value)
                .speed(0.01)
                .range(-wind::LARGEST..=wind::LARGEST)
                .fixed_decimals(3),
        );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, tr!("Wind scale of the cloth"))
        });
        field.clone().on_hover_text(
            tr!("The scale the build states in the cloth it ships; the linked file is not changed. A build reads the shipped cloth back and says when it does not state it"),
        );
        if field.changed() {
            shown.draft = Some((g.id.clone(), value));
        }
        if (field.drag_stopped() || field.lost_focus()) && shown.draft.is_some() {
            shown.draft = None;
            if value != now {
                *asked = Some((g.id.clone(), Edit::Wind(Some(value))));
            }
        }
        if g.wind_scale.is_some() {
            let back = ui.small_button(tr!("Use the file's"));
            back.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr!("Use the wind scale the cloth file states"),
                )
            });
            if back
                .on_hover_text(tr!("Ship the cloth as its file is"))
                .clicked()
            {
                shown.draft = None;
                *asked = Some((g.id.clone(), Edit::Wind(None)));
            }
        }
    });
    let said = match (&file, g.wind_scale) {
        (Ok(winds), Some(scale)) => {
            tr!(
                "The build states {scale} in the cloth; the file states {winds}",
                scale = scale,
                winds = phrase(winds)
            )
        }
        (Ok(winds), None) => tr!(
            "The file states {winds}; the build ships it as it is",
            winds = phrase(winds)
        ),
        (Err(why), _) => tr!(
            "What the file states of its wind cannot be read: {why}",
            why = why
        ),
    };
    small(ui, said);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_that_agree_are_one_number_and_ones_that_differ_are_listed() {
        assert_eq!(phrase(&[1.0]), "1");
        assert_eq!(phrase(&[2.5, 2.5]), "2.5");
        assert_eq!(phrase(&[1.0, 2.0]), "1, 2");
        assert_eq!(phrase(&[]), "no cloth with a controller");
    }
}
