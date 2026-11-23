//! Where each title is installed, as the editor's preferences store it.
//!
//! Keyed by [`Game::asset_slug`], so a title registered at runtime keeps its
//! path like a builtin does and nothing here names a title.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Game;

#[derive(Debug, Clone, Default)]
pub struct GamePaths {
    roots: BTreeMap<String, PathBuf>,
}

impl GamePaths {
    pub fn get(&self, game: Game) -> Option<&PathBuf> {
        self.roots.get(game.asset_slug())
    }

    pub fn set(&mut self, game: Game, path: Option<PathBuf>) {
        match path {
            Some(p) => {
                self.roots.insert(game.asset_slug().to_string(), p);
            }
            None => {
                self.roots.remove(game.asset_slug());
            }
        }
    }
}

impl Serialize for GamePaths {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.roots.serialize(s)
    }
}

impl<'de> Deserialize<'de> for GamePaths {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<String, Option<PathBuf>>::deserialize(d)?;
        let roots = raw
            .into_iter()
            .filter_map(|(key, path)| path.map(|p| (normalize_game_key(key), p)))
            .collect();
        Ok(Self { roots })
    }
}

/// ⚠ A `prefs.json` written before the slug settled spells the key with
/// underscores, so a reader that does not fold them loses the user's install
/// path silently and sends them back to the picker.
fn normalize_game_key(key: String) -> String {
    if key.contains('_') {
        key.replace('_', "-")
    } else {
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const OLD_SHAPE: &str = r#"{
        "gta_v_legacy": "C:/Games/GTAV",
        "gta_v_enhanced": "C:/Games/GTAV Enhanced",
        "rdr2": null
    }"#;

    #[test]
    fn the_old_field_names_still_load() {
        let paths: GamePaths = serde_json::from_str(OLD_SHAPE).expect("parses");
        assert_eq!(
            paths.get(Game::GtaVLegacy).map(|p| p.to_string_lossy()),
            Some("C:/Games/GTAV".into())
        );
        assert_eq!(
            paths.get(Game::GtaVEnhanced).map(|p| p.to_string_lossy()),
            Some("C:/Games/GTAV Enhanced".into())
        );
        assert_eq!(
            paths.get(Game::Rdr2),
            None,
            "an explicit null is not a path"
        );
    }

    #[test]
    fn loading_the_old_shape_rewrites_it_as_ids() {
        let paths: GamePaths = serde_json::from_str(OLD_SHAPE).expect("parses");
        let json = serde_json::to_string(&paths).expect("serializes");
        assert!(
            json.contains("gta-v-legacy") && !json.contains("gta_v_legacy"),
            "should have been written back under the profile id: {json}"
        );
    }

    #[test]
    fn the_new_shape_round_trips() {
        let mut paths = GamePaths::default();
        paths.set(Game::GtaVEnhanced, Some(PathBuf::from("D:/Enhanced")));
        let json = serde_json::to_string(&paths).expect("serializes");
        let back: GamePaths = serde_json::from_str(&json).expect("parses");
        assert_eq!(back.get(Game::GtaVEnhanced), paths.get(Game::GtaVEnhanced));
        assert_eq!(back.get(Game::GtaVLegacy), None);
    }

    #[test]
    fn clearing_a_root_removes_it() {
        let mut paths = GamePaths::default();
        paths.set(Game::Rdr2, Some(PathBuf::from("E:/RDR2")));
        paths.set(Game::Rdr2, None);
        assert_eq!(paths.get(Game::Rdr2), None);
        assert_eq!(paths.roots.len(), 0);
    }

    #[test]
    fn a_root_for_an_unknown_title_is_kept() {
        let json = r#"{"some-future-title": "F:/Games/Later"}"#;
        let paths: GamePaths = serde_json::from_str(json).expect("parses");
        let back = serde_json::to_string(&paths).expect("serializes");
        assert!(back.contains("some-future-title"), "dropped it: {back}");
    }

    #[test]
    fn the_keys_are_the_slugs_the_cli_looks_up() {
        let mut paths = GamePaths::default();
        for game in Game::all() {
            paths.set(game, Some(PathBuf::from("D:/anywhere")));
        }
        let value = serde_json::to_value(&paths).expect("serializes");
        let map = value.as_object().expect("an object");
        for game in Game::all() {
            assert!(
                map.contains_key(game.asset_slug()),
                "{:?} should be keyed by {:?}, got {:?}",
                game,
                game.asset_slug(),
                map.keys().collect::<Vec<_>>()
            );
        }
    }
}
