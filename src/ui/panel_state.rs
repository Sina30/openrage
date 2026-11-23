use std::any::Any;
use std::collections::HashMap;

pub trait PanelState: Any + Default + Send + Sync {
    const KEY: &'static str;
}

#[derive(Default)]
pub struct PanelStore {
    slots: HashMap<&'static str, Box<dyn Any + Send + Sync>>,
}

impl std::fmt::Debug for PanelStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<&str> = self.slots.keys().copied().collect();
        keys.sort();
        f.debug_struct("PanelStore").field("slots", &keys).finish()
    }
}

impl PanelStore {
    pub fn get<T: PanelState>(&mut self) -> &mut T {
        let slot = self
            .slots
            .entry(T::KEY)
            .or_insert_with(|| Box::new(T::default()));
        if !slot.is::<T>() {
            tracing::error!(
                key = T::KEY,
                "two panel states share a key with different types; resetting the slot"
            );
            *slot = Box::new(T::default());
        }
        slot.downcast_mut::<T>()
            .expect("just ensured the slot holds T")
    }

    pub fn peek<T: PanelState>(&self) -> Option<&T> {
        self.slots.get(T::KEY)?.downcast_ref::<T>()
    }

    pub fn take<T: PanelState>(&mut self) -> T {
        match self.slots.remove(T::KEY) {
            Some(slot) => match slot.downcast::<T>() {
                Ok(value) => *value,
                Err(_) => T::default(),
            },
            None => T::default(),
        }
    }

    pub fn put<T: PanelState>(&mut self, value: T) {
        self.slots.insert(T::KEY, Box::new(value));
    }

    // Half of the store's contract and reached only by its own tests: a
    // cleared slot must read as `Default` rather than as whatever the last
    // panel left, which is what `take` relies on.
    #[allow(dead_code)]
    pub fn clear(&mut self, key: &'static str) {
        self.slots.remove(key);
    }

    // The store's only way to say what it holds, and the test that a
    // registered panel keeps state across frames has nothing else to assert
    // against.
    #[allow(dead_code)]
    pub fn occupied(&self) -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = self.slots.keys().copied().collect();
        keys.sort();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, PartialEq, Debug)]
    struct Counter {
        hits: u32,
    }
    impl PanelState for Counter {
        const KEY: &'static str = "test-counter";
    }

    #[derive(Default)]
    struct Other {
        flag: bool,
    }
    impl PanelState for Other {
        const KEY: &'static str = "test-other";
    }

    #[test]
    fn a_slot_starts_at_default_and_then_persists() {
        let mut store = PanelStore::default();
        assert_eq!(store.peek::<Counter>(), None, "nothing before first use");
        assert_eq!(store.get::<Counter>().hits, 0);
        store.get::<Counter>().hits += 1;
        store.get::<Counter>().hits += 1;
        assert_eq!(store.get::<Counter>().hits, 2);
    }

    #[test]
    fn two_surfaces_do_not_share_a_slot() {
        let mut store = PanelStore::default();
        store.get::<Counter>().hits = 7;
        store.get::<Other>().flag = true;
        assert_eq!(store.get::<Counter>().hits, 7);
        assert!(store.get::<Other>().flag);
    }

    #[test]
    fn a_taken_slot_reads_as_default_and_the_put_back_wins() {
        let mut store = PanelStore::default();
        store.get::<Counter>().hits = 9;

        let mine = store.take::<Counter>();
        assert_eq!(mine.hits, 9);
        assert_eq!(
            store.get::<Counter>().hits,
            0,
            "a taken slot must read as Default — this is the hazard, not a bug"
        );
        store.get::<Counter>().hits = 4;

        store.put(mine);
        assert_eq!(
            store.get::<Counter>().hits,
            9,
            "the put-back discards what was written while the slot was taken"
        );
    }

    #[test]
    fn take_then_put_round_trips() {
        let mut store = PanelStore::default();
        store.get::<Counter>().hits = 3;
        let mut mine = store.take::<Counter>();
        assert_eq!(mine.hits, 3);
        assert_eq!(store.peek::<Counter>(), None, "taken means taken");
        mine.hits += 1;
        store.put(mine);
        assert_eq!(store.get::<Counter>().hits, 4);
    }

    #[test]
    fn clearing_a_slot_resets_it_rather_than_corrupting_it() {
        let mut store = PanelStore::default();
        store.get::<Counter>().hits = 5;
        assert_eq!(store.occupied(), vec!["test-counter"]);
        store.clear(Counter::KEY);
        assert_eq!(store.occupied(), Vec::<&str>::new());
        assert_eq!(store.get::<Counter>().hits, 0);
    }

    #[test]
    fn a_panel_registered_at_runtime_keeps_state_across_frames() {
        use crate::state::AppState;
        use crate::ui::panel_registry::{self, Panel, PanelContext, PanelId};

        const ID: &str = "acceptance-plugin-panel";

        #[derive(Default)]
        struct Scratch {
            draws: u32,
            last_seen_selection: Option<u64>,
        }
        impl PanelState for Scratch {
            const KEY: &'static str = ID;
        }

        struct PluginPanel;
        impl Panel for PluginPanel {
            fn id(&self) -> PanelId {
                ID
            }
            fn title(&self, _state: &AppState) -> String {
                "Plugin".to_string()
            }
            fn draw(&self, ui: &mut egui::Ui, ctx: &mut PanelContext<'_>) {
                let selected = ctx.state.selection.current().map(|(_, id)| id);
                let scratch = ctx.state.panels.get::<Scratch>();
                scratch.draws += 1;
                scratch.last_seen_selection = selected;
                ui.label(format!("drawn {} time(s)", scratch.draws));
            }
        }
        static PLUGIN: PluginPanel = PluginPanel;

        assert!(
            panel_registry::register(&PLUGIN),
            "nothing else should own this id"
        );

        let mut state = AppState::default();
        state.selection.select(7, 42);

        for _ in 0..3 {
            egui::__run_test_ui(|ui| {
                let mut panel_ctx = PanelContext {
                    state: &mut state,
                    bridge: None,
                };
                panel_registry::draw_panel(ID, ui, &mut panel_ctx);
            });
        }

        let scratch = state.panels.get::<Scratch>();
        assert_eq!(
            scratch.draws, 3,
            "the panel's own state did not survive between frames"
        );
        assert_eq!(
            scratch.last_seen_selection,
            Some(42),
            "the panel drew but could not see the application's selection"
        );
    }

    #[test]
    fn a_key_collision_resets_instead_of_panicking() {
        #[derive(Default)]
        struct Impostor;
        impl PanelState for Impostor {
            const KEY: &'static str = "test-counter";
        }

        let mut store = PanelStore::default();
        store.get::<Counter>().hits = 9;
        let _ = store.get::<Impostor>();
        assert_eq!(store.get::<Counter>().hits, 0);
    }
}
