use or_io::container::resource::{
    Authoring, Compression, DatResourceFileType, ResourceEnvelope, ResourceType, Rsc7, TitleId,
    VersionError, VersionRegistry, VersionSupport, GTA5,
};

const SYNTHETIC: TitleId = "synthetic";

const SYNTHETIC_YPT_VERSIONS: [u32; 2] = [900, 901];

const SYNTHETIC_YDR_VERSIONS: [u32; 1] = [910];

const SHARED_YBN_VERSION: u32 = 43;

fn two_titles() -> VersionRegistry {
    let mut registry = VersionRegistry::default();
    registry.register(VersionSupport::multiple(
        SYNTHETIC,
        ResourceType::Ypt,
        SYNTHETIC_YPT_VERSIONS,
        SYNTHETIC_YPT_VERSIONS[0],
    ));
    registry.register(VersionSupport::multiple(
        SYNTHETIC,
        ResourceType::Ydr,
        SYNTHETIC_YDR_VERSIONS,
        SYNTHETIC_YDR_VERSIONS[0],
    ));
    registry.register(VersionSupport::single(
        SYNTHETIC,
        ResourceType::Ybn,
        SHARED_YBN_VERSION,
    ));
    registry
}

#[test]
fn the_shipped_registry_holds_exactly_one_title() {
    let registry = VersionRegistry::default();
    assert_eq!(
        registry.titles(),
        vec![GTA5],
        "the default registry is GTA V's table and nothing else"
    );
    // Compared with the list rather than pinned to a literal, which goes red
    // every time a format is added.
    let registered = ResourceType::ALL
        .iter()
        .filter(|rt| registry.get(GTA5, **rt).is_some())
        .count();
    assert_eq!(
        registered,
        ResourceType::ALL.len(),
        "every GTA V resource type has a table"
    );
    // The floor is what stops the comparison above from passing vacuously if
    // the walk breaks and the list reads as empty.
    assert!(
        ResourceType::ALL.len() >= 10,
        "{} resource types is too few to be the list — the walk is broken, not the table",
        ResourceType::ALL.len()
    );
}

#[test]
fn a_second_title_is_a_register_call() {
    let registry = two_titles();
    assert_eq!(registry.titles(), vec![GTA5, SYNTHETIC]);

    let declared = ResourceType::ALL
        .iter()
        .filter(|rt| registry.get(SYNTHETIC, **rt).is_some())
        .count();
    assert_eq!(
        declared,
        3,
        "the synthetic title declares Ypt, Ydr and Ybn out of {} types",
        ResourceType::ALL.len()
    );
    assert!(registry.get(SYNTHETIC, ResourceType::Ytd).is_none());
    assert!(registry.get(GTA5, ResourceType::Ytd).is_some());
}

#[test]
fn neither_titles_versions_are_visible_through_the_other() {
    let registry = two_titles();

    for version in [68, 71] {
        assert!(
            registry.is_supported(GTA5, ResourceType::Ypt, version),
            "gta5 reads its own YPT {version}"
        );
        assert!(
            !registry.is_supported(SYNTHETIC, ResourceType::Ypt, version),
            "the synthetic title must not inherit gta5's YPT {version}"
        );
    }
    for version in [165, 159, 154] {
        assert!(registry.is_supported(GTA5, ResourceType::Ydr, version));
        assert!(!registry.is_supported(SYNTHETIC, ResourceType::Ydr, version));
    }

    for version in SYNTHETIC_YPT_VERSIONS {
        assert!(registry.is_supported(SYNTHETIC, ResourceType::Ypt, version));
        assert!(
            !registry.is_supported(GTA5, ResourceType::Ypt, version),
            "gta5 must not gain YPT {version} from a second title's table"
        );
    }
    for version in SYNTHETIC_YDR_VERSIONS {
        assert!(registry.is_supported(SYNTHETIC, ResourceType::Ydr, version));
        assert!(!registry.is_supported(GTA5, ResourceType::Ydr, version));
    }
}

#[test]
fn a_version_both_titles_claim_answers_for_both() {
    let registry = two_titles();
    assert!(registry.is_supported(GTA5, ResourceType::Ybn, SHARED_YBN_VERSION));
    assert!(registry.is_supported(SYNTHETIC, ResourceType::Ybn, SHARED_YBN_VERSION));
    assert_eq!(
        registry.write_version(GTA5, ResourceType::Ybn),
        registry.write_version(SYNTHETIC, ResourceType::Ybn),
        "the two tables agree where they were made to agree"
    );
}

#[test]
fn the_write_version_is_per_title() {
    let registry = two_titles();
    assert_eq!(registry.write_version(GTA5, ResourceType::Ypt), Some(68));
    assert_eq!(
        registry.write_version(SYNTHETIC, ResourceType::Ypt),
        Some(SYNTHETIC_YPT_VERSIONS[0])
    );
    assert_eq!(registry.write_version(SYNTHETIC, ResourceType::Ytd), None);
    assert_eq!(registry.write_version(GTA5, ResourceType::Ytd), Some(13));
}

#[test]
fn the_error_names_the_titles_own_versions() {
    let registry = two_titles();

    let err = registry
        .validate(SYNTHETIC, ResourceType::Ypt, 68)
        .expect_err("68 is not a synthetic YPT version");
    match err {
        VersionError::UnsupportedVersion {
            title,
            resource_type,
            found_version,
            ref expected_versions,
        } => {
            assert_eq!(title, SYNTHETIC);
            assert_eq!(resource_type, ResourceType::Ypt);
            assert_eq!(found_version, 68);
            assert_eq!(expected_versions, "900 or 901");
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
    assert!(!format!("{err}").contains("71"), "{err}");

    let err = registry
        .validate(SYNTHETIC, ResourceType::Ytd, 13)
        .expect_err("the synthetic title has no YTD table");
    assert!(matches!(
        err,
        VersionError::UnknownResourceType {
            title: SYNTHETIC,
            resource_type: ResourceType::Ytd
        }
    ));
    assert!(registry.validate(GTA5, ResourceType::Ytd, 13).is_ok());
}

#[test]
fn registering_a_second_title_does_not_disturb_the_first() {
    let before = VersionRegistry::default();
    let after = two_titles();

    for rt in ResourceType::ALL {
        let a = before.get(GTA5, rt).expect("registered before");
        let b = after.get(GTA5, rt).expect("still registered after");
        assert_eq!(
            a.supported_read_versions, b.supported_read_versions,
            "{rt}'s gta5 read versions changed when a second title was added"
        );
        assert_eq!(
            a.write_version, b.write_version,
            "{rt}'s gta5 write version"
        );
    }
}

#[test]
fn a_bare_header_validates_against_gta_v_by_name() {
    let bytes = Rsc7::author(
        Authoring::new(DatResourceFileType::Custom(68), Compression::fast()),
        Vec::new(),
        Vec::new(),
    )
    .expect("authors a file at version 68");

    let seal = Rsc7::peek(&bytes).expect("parses");
    assert_eq!(seal.version(), 68);

    assert!(seal.validate(ResourceType::Ypt).is_ok());
    assert!(seal.validate_for(GTA5, ResourceType::Ypt).is_ok());

    assert!(seal.validate_for(SYNTHETIC, ResourceType::Ypt).is_err());
}
