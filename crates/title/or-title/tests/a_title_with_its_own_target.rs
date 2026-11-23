use or_io::generation::{Generation, TargetDescriptor};
use or_title::game::{registry, ContainerFamily, DlcPolicy, GameProfile, InstallLayout};
use or_title::Game;

const NEWLAND_V1: Generation = Generation::from_id("newland-v1");

#[derive(Debug, Clone, Copy, Default)]
struct Newland;

impl GameProfile for Newland {
    fn id(&self) -> &'static str {
        "newland"
    }

    fn label(&self) -> &'static str {
        "Newland"
    }

    fn description(&self) -> &'static str {
        "A title invented by a test, whose asset layouts are its own."
    }

    fn container(&self) -> ContainerFamily {
        ContainerFamily::Rpf7
    }

    fn native_target(&self) -> Generation {
        NEWLAND_V1
    }

    fn asset_targets(&self) -> &'static [Generation] {
        &[NEWLAND_V1]
    }

    fn target_labels(&self) -> &'static [TargetDescriptor] {
        &[TargetDescriptor {
            id: "newland-v1",
            label: "Newland",
        }]
    }

    fn executable_candidates(&self) -> &'static [&'static str] {
        &["Newland.exe"]
    }

    fn install_layout(&self) -> InstallLayout {
        InstallLayout {
            base_archive: "base.rpf",
            update_archive: None,
            dlc_directories: &[],
            platform_archive_prefix: "x64",
        }
    }

    fn dlc_policy(&self) -> DlcPolicy {
        DlcPolicy::MountAll
    }
}

fn newland() -> Game {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        static PROFILE: Newland = Newland;
        registry::register(&PROFILE);
    });
    Game::by_id("newland").expect("registered above")
}

#[test]
fn a_title_can_declare_a_target_that_is_not_gta_vs() {
    let profile = newland().profile();

    assert_eq!(profile.native_target(), NEWLAND_V1);
    assert_eq!(profile.asset_targets(), &[NEWLAND_V1]);

    assert_ne!(NEWLAND_V1, Generation::Gen8);
    assert_ne!(NEWLAND_V1, Generation::Gen9);
    assert!(
        !NEWLAND_V1.is_gen9(),
        "a foreign target is not Enhanced either; `is_gen9` must be a test for \
         one target rather than a test for `!= Gen8`"
    );
    assert!(
        !profile.asset_targets().contains(&Generation::Gen8),
        "declaring a real target must not leave GTA V's in the list"
    );
}

#[test]
fn the_user_facing_name_of_the_target_is_this_titles() {
    let target = newland().profile().native_target();

    assert_eq!(target.slug(), "newland-v1");
    assert_eq!(target.edition_label(), "Newland");
    assert_ne!(
        target.edition_label(),
        "Legacy",
        "the whole point: a second title's user must not be shown GTA V's \
         edition name"
    );
}

#[test]
fn no_declared_format_reads_a_target_nothing_declared() {
    let target = newland().profile().native_target();

    for format in or_io::gta5::formats::FORMATS {
        assert!(
            format.targets().all(|t| t != target),
            "`.{}` claims a target only this title names",
            format.ext
        );
        assert!(
            format.root_name(target).is_err(),
            "`.{}` must refuse a target it does not declare rather than read it \
             with GTA V's layout",
            format.ext
        );
    }
}

#[test]
fn an_unnamed_target_labels_itself_rather_than_borrowing_a_builtin() {
    const UNNAMED: Generation = Generation::from_id("nothing-names-this-one");

    assert_eq!(UNNAMED.slug(), "nothing-names-this-one");
    assert_eq!(UNNAMED.edition_label(), "nothing-names-this-one");
    assert_ne!(UNNAMED.edition_label(), "Legacy");
    assert_ne!(UNNAMED.edition_label(), "Enhanced");
}

#[test]
fn declaring_a_new_target_leaves_the_builtins_alone() {
    let _ = newland();

    assert_eq!(Generation::Gen8.edition_label(), "Legacy");
    assert_eq!(Generation::Gen9.edition_label(), "Enhanced");
    assert_eq!(Game::GtaVLegacy.generation(), Generation::Gen8);
    assert_eq!(Game::GtaVEnhanced.generation(), Generation::Gen9);
}

#[test]
fn declaring_an_own_target_did_not_add_a_third_step() {
    let profile = newland().profile();

    assert_eq!(profile.id(), "newland");
    assert_eq!(profile.native_target(), NEWLAND_V1);
    assert_eq!(profile.target_labels().len(), 1);

    assert!(registry::by_id("newland").is_some());
}
