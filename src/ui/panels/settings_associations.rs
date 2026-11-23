//! The File associations page: register OpenRage as a handler for the formats
//! it opens, and take the registration back.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::platform::associations::{self, Candidate, Journal, Registry};
use crate::state::AppState;
use crate::ui::theme;

const JOURNAL_FILE: &str = "file-associations.json";

/// Where the page reads and writes, so a test can hand it a registry in
/// memory and a scratch journal.
pub trait Backend {
    fn registry(&mut self) -> Result<&mut dyn Registry, String>;
    fn exe(&self) -> Option<PathBuf>;
    fn journal_path(&self) -> Option<PathBuf>;
}

/// The current user's classes, opened only when the page needs them.
#[derive(Default)]
struct UserRegistry {
    #[cfg(windows)]
    opened: Option<associations::WindowsRegistry>,
}

impl UserRegistry {
    #[cfg(windows)]
    fn open(&mut self) -> Result<&mut dyn Registry, String> {
        if self.opened.is_none() {
            self.opened = Some(associations::WindowsRegistry::under_current_user(
                associations::USER_CLASSES,
            )?);
        }
        Ok(self.opened.as_mut().expect("opened above") as &mut dyn Registry)
    }

    #[cfg(not(windows))]
    fn open(&mut self) -> Result<&mut dyn Registry, String> {
        Err(tr!("File associations are managed by the desktop on this platform").to_string())
    }
}

impl Backend for UserRegistry {
    fn registry(&mut self) -> Result<&mut dyn Registry, String> {
        if crate::platform::standing_in() {
            tracing::info!("the registry's stand-in was asked for the user's file associations");
            return Err(
                tr!("this run stands in for the registry, so it changes no association").into(),
            );
        }
        self.open()
    }

    fn exe(&self) -> Option<PathBuf> {
        std::env::current_exe().ok()
    }

    fn journal_path(&self) -> Option<PathBuf> {
        crate::state::prefs::config_root().map(|root| root.join(JOURNAL_FILE))
    }
}

/// What the page remembers between frames.
#[derive(Default)]
pub struct AssociationsPage {
    checked: Option<BTreeSet<String>>,
    make_default: bool,
    registered: Option<Vec<String>>,
    message: Option<(bool, String)>,
}

impl crate::ui::panel_state::PanelState for AssociationsPage {
    const KEY: &'static str = "settings-associations";
}

pub fn draw_associations_page(ui: &mut egui::Ui, state: &mut AppState) {
    draw_with(ui, state, &mut UserRegistry::default());
}

pub fn draw_with(ui: &mut egui::Ui, state: &mut AppState, backend: &mut dyn Backend) {
    ui.heading(tr!("File associations"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "Adds OpenRage to Open With for the formats you tick, for your Windows account \
             only. Nothing is registered until you press Register, and Undo takes back \
             exactly what it wrote."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_DEFAULT);

    let candidates = associations::candidates();
    let mut page = state.panels.take::<AssociationsPage>();
    let checked = page.checked.get_or_insert_with(|| {
        candidates
            .iter()
            .filter(|c| c.suggested)
            .map(|c| c.ext.clone())
            .collect()
    });
    if page.registered.is_none() {
        page.registered = Some(read_registered(backend, &candidates));
    }
    let registered = page.registered.clone().unwrap_or_default();

    ui.horizontal(|ui| {
        if ui.small_button(tr!("Game formats")).clicked() {
            *checked = candidates
                .iter()
                .filter(|c| c.suggested)
                .map(|c| c.ext.clone())
                .collect();
        }
        if ui.small_button(tr!("None")).clicked() {
            checked.clear();
        }
    });
    egui::Grid::new("associations_grid")
        .num_columns(3)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for c in &candidates {
                let mut on = checked.contains(&c.ext);
                if ui.checkbox(&mut on, format!(".{}", c.ext)).changed() {
                    if on {
                        checked.insert(c.ext.clone());
                    } else {
                        checked.remove(&c.ext);
                    }
                }
                ui.label(
                    egui::RichText::new(&c.noun)
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
                if registered.contains(&c.ext) {
                    ui.label(
                        egui::RichText::new(tr!("Opens in OpenRage"))
                            .small()
                            .color(theme::SUCCESS),
                    );
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    ui.add_space(theme::SPACING_SNUG);
    ui.checkbox(
        &mut page.make_default,
        tr!("Also make OpenRage the default for these (double-click opens them here)"),
    )
    .on_hover_text(tr!(
        "Without this, OpenRage is only added to Open With and whatever opens the file \
         today keeps doing so. Windows may still ask once which program to use."
    ));
    ui.add_space(theme::SPACING_SNUG);

    let journal_path = backend.journal_path();
    let journal = journal_path
        .as_deref()
        .map(associations::load_journal)
        .unwrap_or_default();
    ui.horizontal(|ui| {
        let chosen: Vec<(String, String)> = candidates
            .iter()
            .filter(|c| checked.contains(&c.ext))
            .map(|c| (c.ext.clone(), c.noun.clone()))
            .collect();
        if ui
            .add_enabled(!chosen.is_empty(), egui::Button::new(tr!("Register")))
            .on_hover_text(tr!("Write the associations for the ticked formats"))
            .clicked()
        {
            page.message = Some(register(
                backend,
                &chosen,
                page.make_default,
                journal.clone(),
            ));
            page.registered = None;
        }
        let undo_label = match journal.extensions().len() {
            0 => tr!("Undo").to_string(),
            n => tr!("Undo ({n} format(s))", n = n),
        };
        if ui
            .add_enabled(!journal.is_empty(), egui::Button::new(undo_label))
            .on_hover_text(tr!("Take back everything OpenRage registered"))
            .on_disabled_hover_text(tr!("OpenRage has registered nothing to take back"))
            .clicked()
        {
            page.message = Some(undo(backend, &journal));
            page.registered = None;
        }
    });
    if let Some((ok, text)) = &page.message {
        let color = if *ok { theme::SUCCESS } else { theme::ERROR };
        ui.label(egui::RichText::new(text).small().color(color));
    }
    state.panels.put(page);
}

fn read_registered(backend: &mut dyn Backend, candidates: &[Candidate]) -> Vec<String> {
    let Some(exe) = backend.exe() else {
        return Vec::new();
    };
    let exts: Vec<String> = candidates.iter().map(|c| c.ext.clone()).collect();
    match backend.registry() {
        Ok(reg) => associations::registered(reg, &exe, &exts),
        Err(_) => Vec::new(),
    }
}

fn register(
    backend: &mut dyn Backend,
    chosen: &[(String, String)],
    make_default: bool,
    earlier: Journal,
) -> (bool, String) {
    let (Some(exe), Some(path)) = (backend.exe(), backend.journal_path()) else {
        return (
            false,
            tr!("No executable path or settings folder to record this in").into(),
        );
    };
    let wanted = associations::desired(&exe, chosen, make_default);
    let reg = match backend.registry() {
        Ok(reg) => reg,
        Err(e) => return (false, e),
    };
    match associations::apply(reg, &wanted) {
        Ok(journal) => {
            let wrote = journal.changes.len();
            if let Err(e) =
                associations::save_journal(&path, &associations::merged(earlier, journal))
            {
                return (
                    false,
                    tr!("Registered, but the undo record was not saved: {e}", e = e),
                );
            }
            associations::notify_shell();
            (
                true,
                tr!(
                    "Registered {count} format(s); {wrote} value(s) written",
                    wrote = wrote,
                    count = chosen.len()
                ),
            )
        }
        Err(e) => (false, tr!("Nothing was registered: {e}", e = e)),
    }
}

fn undo(backend: &mut dyn Backend, journal: &Journal) -> (bool, String) {
    let Some(path) = backend.journal_path() else {
        return (
            false,
            tr!("No settings folder holds the undo record").into(),
        );
    };
    let reg = match backend.registry() {
        Ok(reg) => reg,
        Err(e) => return (false, e),
    };
    match associations::undo(reg, journal) {
        Ok(report) => {
            let _ = associations::save_journal(&path, &Journal::default());
            associations::notify_shell();
            if report.left_alone.is_empty() {
                (
                    true,
                    tr!(
                        "Undone: {restored} value(s) restored",
                        restored = report.restored
                    ),
                )
            } else {
                (
                    true,
                    tr!("Undone, except {count} value(s) something else has changed since: {left_alone}", count = report.left_alone.len(), left_alone = report.left_alone.join(", ")),
                )
            }
        }
        Err(e) => (false, tr!("Undo stopped: {e}", e = e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::associations::MapRegistry;

    struct Fake {
        reg: MapRegistry,
        dir: tempfile::TempDir,
    }

    impl Backend for Fake {
        fn registry(&mut self) -> Result<&mut dyn Registry, String> {
            Ok(&mut self.reg)
        }
        fn exe(&self) -> Option<PathBuf> {
            Some(PathBuf::from(r"C:\Tools\openrage.exe"))
        }
        fn journal_path(&self) -> Option<PathBuf> {
            Some(self.dir.path().join(JOURNAL_FILE))
        }
    }

    fn frame(
        ctx: &egui::Context,
        state: &mut AppState,
        fake: &mut Fake,
        n: usize,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Pos2)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 3000.0),
            )),
            time: Some(n as f64 * 0.1),
            events,
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| draw_with(ui, state, fake));
        out.platform_output
            .accesskit_update
            .map(|u| {
                u.nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let label = node.label()?.to_string();
                        let b = node.bounds()?;
                        Some((
                            label,
                            egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn click(
        ctx: &egui::Context,
        state: &mut AppState,
        fake: &mut Fake,
        n: &mut usize,
        label: &str,
    ) {
        let found = frame(ctx, state, fake, *n, vec![]);
        *n += 1;
        let at = found
            .iter()
            .find(|(l, _)| l.starts_with(label))
            .unwrap_or_else(|| panic!("no `{label}` on the page"))
            .1;
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        frame(
            ctx,
            state,
            fake,
            *n,
            vec![egui::Event::PointerMoved(at), press(true)],
        );
        frame(ctx, state, fake, *n + 1, vec![press(false)]);
        *n += 2;
    }

    /// A test build stands in as a self-check does, so the page's own registry is never the user's.
    #[test]
    fn the_page_leaves_the_users_registry_to_a_stand_in() {
        assert!(
            UserRegistry::default().registry().is_err(),
            "the page opened the user's own classes"
        );
    }

    /// ⚠ **Presses the page's own buttons.** Drawing the page writes nothing,
    /// Register writes and journals, and Undo leaves the registry byte for
    /// byte as it found it and removes the journal.
    #[test]
    fn register_then_undo_through_the_page() {
        let mut state = AppState::default();
        let mut fake = Fake {
            reg: MapRegistry::default(),
            dir: tempfile::tempdir().unwrap(),
        };
        fake.reg.write(".ytd", "", "OtherTool.ytd").unwrap();
        let before = fake.reg.clone();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut n = 0;
        for _ in 0..2 {
            frame(&ctx, &mut state, &mut fake, n, vec![]);
            n += 1;
        }
        assert_eq!(fake.reg, before, "opening the page writes nothing");

        click(
            &ctx,
            &mut state,
            &mut fake,
            &mut n,
            "Also make OpenRage the default",
        );
        click(&ctx, &mut state, &mut fake, &mut n, "Register");
        assert_eq!(
            fake.reg.read(".ytd", "").unwrap().as_deref(),
            Some("OpenRage.ytd")
        );
        assert!(fake
            .reg
            .read(r".ydr\OpenWithProgids", "OpenRage.ydr")
            .unwrap()
            .is_some());
        let journal_path = fake.journal_path().unwrap();
        assert!(journal_path.is_file(), "the undo record was kept");

        click(&ctx, &mut state, &mut fake, &mut n, "Undo");
        assert_eq!(fake.reg, before, "undo is exact");
        assert!(!journal_path.exists(), "and leaves nothing to undo");
    }
}
