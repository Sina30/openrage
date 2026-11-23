// The container layer cannot see a title: an RSC7 header is a magic, a version
// and two flag words, none of which names a game, so versions are per title.

use std::collections::BTreeSet;

use or_io::container::resource::{ResourceType, DEFAULT_REGISTRY, GTA5};
use or_title::game::registry;

#[test]
fn every_openable_title_has_a_version_table() {
    let profiles = registry::all();
    assert!(
        profiles.len() >= 3,
        "expected the two GTA V editions and RDR2 at least, found {}",
        profiles.len()
    );

    let tables: BTreeSet<&str> = DEFAULT_REGISTRY.titles().into_iter().collect();
    assert_eq!(
        tables,
        BTreeSet::from([GTA5]),
        "the shipped registry is one title"
    );

    let supported: BTreeSet<&str> = profiles
        .iter()
        .filter(|p| p.is_supported())
        .map(|p| p.id())
        .collect();
    // A supported title opens its archives; only a title with a version table decodes its resources.
    assert_eq!(
        supported,
        BTreeSet::from(["gta-v-enhanced", "gta-v-legacy", "rdr2"]),
    );
    assert!(
        !tables.contains("rdr2"),
        "rdr2 has no '{GTA5}'-style table, so its resources are refused by name rather than read as GTA V's"
    );
}

#[test]
fn both_gta_v_editions_share_one_table() {
    for id in ["gta-v-enhanced", "gta-v-legacy"] {
        assert!(registry::by_id(id).is_some(), "{id} is registered");
    }
    assert!(DEFAULT_REGISTRY.is_supported(GTA5, ResourceType::Ydr, 165));
    assert!(DEFAULT_REGISTRY.is_supported(GTA5, ResourceType::Ydr, 159));
}
