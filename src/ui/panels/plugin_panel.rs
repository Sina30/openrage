//! A panel a plugin registered: the host draws the widgets its guest describes each frame, and
//! what the user does to them goes back to the guest for its next frame.
//!
//! ⚠ egui never crosses the plugin boundary: the guest describes a widget in `or_plugin`'s small
//! vocabulary and this file is the only place that turns it into egui.

use or_plugin::{Event, Widget};

use crate::state::AppState;
use crate::ui::panel_registry::{self, Panel, PanelContext, PanelId};
use crate::ui::theme;

/// The id a plugin's panel has in the panel registry, and so in a saved layout.
pub fn panel_id(plugin: &str, panel: &str) -> String {
    format!("plugin.{plugin}.{panel}")
}

/// A panel of a plugin, drawn from the guest's own description of it.
pub struct GuestPanel {
    id: PanelId,
    plugin: String,
    panel: String,
}

/// Add a plugin's panel to the panel registry, once.
///
/// ⚠ One small object per distinct panel is leaked for the life of the process, as an id is
/// (`panel_registry::intern`): a registry holds `&'static` values. A second registration of the id
/// is refused and a builtin is never displaced.
pub fn register(plugin: &str, panel: &str) {
    let id = panel_registry::intern(&panel_id(plugin, panel));
    if panel_registry::get(id).is_some() {
        return;
    }
    let guest: &'static GuestPanel = Box::leak(Box::new(GuestPanel {
        id,
        plugin: plugin.to_string(),
        panel: panel.to_string(),
    }));
    panel_registry::register(guest);
}

impl Panel for GuestPanel {
    fn id(&self) -> PanelId {
        self.id
    }

    fn title(&self, state: &AppState) -> String {
        state
            .plugin_panel_title(&self.plugin, &self.panel)
            .unwrap_or_else(|| self.panel.clone())
    }

    fn draw(&self, ui: &mut egui::Ui, ctx: &mut PanelContext<'_>) {
        match ctx.state.plugin_panel(&self.plugin, &self.panel) {
            Ok(widgets) => {
                let events = draw_widgets(ui, &widgets);
                if !events.is_empty() {
                    ctx.state
                        .deliver_plugin_events(&self.plugin, &self.panel, events);
                    ui.ctx().request_repaint();
                }
            }
            Err(why) => draw_stopped(ui, ctx.state, &self.plugin, &why),
        }
    }
}

fn draw_stopped(ui: &mut egui::Ui, state: &mut AppState, plugin: &str, why: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme::SPACING_SECTION);
        ui.label(egui::RichText::new("This plugin's panel is not running").strong());
        ui.label(egui::RichText::new(why).small().color(theme::WARNING.get()));
        ui.add_space(theme::SPACING_TIGHT);
        if ui.button("Load the plugin").clicked() {
            state.request_plugin_run(plugin);
        }
    });
}

/// Draw one frame of a panel and return what the user did to it.
pub fn draw_widgets(ui: &mut egui::Ui, widgets: &[Widget]) -> Vec<Event> {
    let mut events = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (at, widget) in widgets.iter().enumerate() {
                ui.push_id(at, |ui| draw_widget(ui, widget, &mut events));
            }
        });
    events
}

fn draw_widget(ui: &mut egui::Ui, widget: &Widget, events: &mut Vec<Event>) {
    match widget {
        Widget::Label { text } => {
            ui.label(text.as_str());
        }
        Widget::Button { id, text } => {
            if ui.button(text.as_str()).clicked() {
                events.push(Event::Clicked { id: id.clone() });
            }
        }
        Widget::Slider {
            id,
            label,
            min,
            max,
            value,
        } => {
            // ⚠ A range or a value that is not a number is the guest's mistake, and egui is not asked to draw it.
            let (min, max) = match min.is_finite() && max.is_finite() {
                true => (*min, *max),
                false => (0.0, 1.0),
            };
            let mut shown = if value.is_finite() { *value } else { min };
            let response = ui.add(egui::Slider::new(&mut shown, min..=max).text(label.as_str()));
            if response.changed() && shown != *value {
                events.push(Event::Slid {
                    id: id.clone(),
                    value: shown,
                });
            }
        }
        Widget::Checkbox { id, label, checked } => {
            let mut shown = *checked;
            if ui.checkbox(&mut shown, label.as_str()).changed() {
                events.push(Event::Checked {
                    id: id.clone(),
                    checked: shown,
                });
            }
        }
        Widget::TextField { id, label, text } => {
            ui.horizontal(|ui| {
                ui.label(label.as_str());
                let mut shown = text.clone();
                let response = ui.add(egui::TextEdit::singleline(&mut shown));
                if response.changed() {
                    events.push(Event::Typed {
                        id: id.clone(),
                        text: shown,
                    });
                }
            });
        }
        Widget::List {
            id,
            items,
            selected,
        } => {
            egui::ScrollArea::vertical()
                .id_salt("list")
                .max_height(160.0)
                .show(ui, |ui| {
                    for (index, item) in items.iter().enumerate() {
                        let chosen = *selected == Some(index as u32);
                        if ui.selectable_label(chosen, item.as_str()).clicked() {
                            events.push(Event::Chose {
                                id: id.clone(),
                                index: index as u32,
                            });
                        }
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(widgets: &[Widget], input: egui::RawInput) -> (Vec<Event>, egui::FullOutput) {
        let ctx = egui::Context::default();
        let mut events = Vec::new();
        let output = ctx.run_ui(input, |ui| {
            events = draw_widgets(ui, widgets);
        });
        (events, output)
    }

    fn every_widget() -> Vec<Widget> {
        vec![
            Widget::Button {
                id: "go".into(),
                text: "Go".into(),
            },
            Widget::Slider {
                id: "w".into(),
                label: "Weight".into(),
                min: 0.0,
                max: 1.0,
                value: f32::NAN,
            },
            Widget::Slider {
                id: "bad".into(),
                label: "Bad".into(),
                min: f32::NAN,
                max: 1.0,
                value: 0.5,
            },
            Widget::Checkbox {
                id: "c".into(),
                label: "Pin".into(),
                checked: true,
            },
            Widget::TextField {
                id: "t".into(),
                label: "Name".into(),
                text: "left".into(),
            },
            Widget::List {
                id: "l".into(),
                items: vec!["Front".into(), "Back".into()],
                selected: Some(1),
            },
            Widget::Label {
                text: "pressed 0 times".into(),
            },
        ]
    }

    #[test]
    fn every_widget_draws_and_a_frame_nobody_touched_sends_nothing_back() {
        let (events, output) = frame(&every_widget(), egui::RawInput::default());
        assert_eq!(events, Vec::new());
        assert!(!output.shapes.is_empty(), "the widgets drew nothing at all");
    }

    /// The widgets drawn in a frame of their own, and what each node of the accessibility tree is: its role, label and bounds.
    fn drawn(
        ctx: &egui::Context,
        widgets: &[Widget],
        events: Vec<egui::Event>,
    ) -> (Vec<Event>, Vec<(egui::accesskit::Role, String, egui::Rect)>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut sent = Vec::new();
        let output = ctx.run_ui(input, |ui| sent = draw_widgets(ui, widgets));
        let nodes = (output.platform_output.accesskit_update.iter())
            .flat_map(|update| update.nodes.iter())
            .filter_map(|(_, node)| {
                let bounds = node.bounds()?;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                let text = node.label().or_else(|| node.value()).unwrap_or_default();
                Some((node.role(), text.to_string(), rect))
            })
            .collect();
        (sent, nodes)
    }

    /// What drawing `widgets` sends back after a click on the node of `role` that `wanted` accepts, and then `typed` into it.
    fn after_a_click_on(
        widgets: &[Widget],
        role: egui::accesskit::Role,
        wanted: impl Fn(&str) -> bool,
        typed: Option<&str>,
    ) -> Vec<Event> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let (_, nodes) = drawn(&ctx, widgets, Vec::new());
        let node = (nodes.iter())
            .find(|(r, label, _)| *r == role && wanted(label))
            .unwrap_or_else(|| panic!("no {role:?} drawn: {nodes:#?}"));
        let at = node.2.center();
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut steps = vec![
            vec![egui::Event::PointerMoved(at)],
            vec![press(true)],
            vec![press(false)],
        ];
        steps.extend(typed.map(|text| vec![egui::Event::Text(text.to_string())]));
        let mut sent = Vec::new();
        for events in steps {
            sent.extend(drawn(&ctx, widgets, events).0);
        }
        sent
    }

    #[test]
    fn what_the_user_does_to_a_widget_is_what_the_guest_is_told() {
        use egui::accesskit::Role;

        let widgets = every_widget();
        assert_eq!(
            after_a_click_on(&widgets, Role::Button, |l| l == "Go", None),
            [Event::Clicked { id: "go".into() }]
        );
        assert_eq!(
            after_a_click_on(&widgets, Role::CheckBox, |l| l == "Pin", None),
            [Event::Checked {
                id: "c".into(),
                checked: false
            }]
        );
        assert_eq!(
            after_a_click_on(&widgets, Role::Button, |l| l == "Front", None),
            [Event::Chose {
                id: "l".into(),
                index: 0
            }]
        );
        assert_eq!(
            after_a_click_on(&widgets, Role::TextInput, |_| true, Some("!")),
            [Event::Typed {
                id: "t".into(),
                text: "left!".into()
            }]
        );
    }
}
