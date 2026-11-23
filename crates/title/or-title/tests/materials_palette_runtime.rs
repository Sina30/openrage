use std::path::PathBuf;
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, ExtractionProgress, Gta5RetailCryptoProvider};
use or_io::container::RpfCryptoProvider;
use or_io::gta5::data::materials_palette::MaterialPaletteSource;
use or_title::{load_material_palette_from_gta_install, GtaInstall};

fn install_dir_from_env() -> Option<PathBuf> {
    let raw = std::env::var("GTA_PTFX_GTA_INSTALL").ok()?;
    let path = PathBuf::from(raw);
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

#[test]
fn loads_palette_from_local_install() {
    let Some(install) = install_dir_from_env() else {
        or_test_support::skip_corpus(
            "loads_palette_from_local_install",
            "set GTA_PTFX_GTA_INSTALL to a GTA V install dir to run this test",
        );
        return;
    };
    let exe = install.join("GTA5.exe");
    if !exe.exists() {
        or_test_support::skip_corpus(
            "loads_palette_from_local_install",
            format_args!(
                "Skipping: {} not present, can't extract keys",
                exe.display()
            ),
        );
        return;
    }

    let progress = move |_: ExtractionProgress| {};
    let keys = match extract_keys(&exe, &progress) {
        Ok(k) => Arc::new(k),
        Err(e) => {
            or_test_support::skip_corpus(
                "loads_palette_from_local_install",
                format_args!("key extraction failed ({e})"),
            );
            return;
        }
    };
    let provider: Arc<dyn RpfCryptoProvider> = Arc::new(Gta5RetailCryptoProvider::new(keys));

    let gta_install = GtaInstall { root: install };
    let palette = load_material_palette_from_gta_install(&gta_install, provider)
        .expect("load_material_palette_from_gta_install succeeded");

    assert!(
        palette.len() >= 200,
        "expected at least 200 palette entries, got {}",
        palette.len()
    );
    assert!(matches!(
        palette.source(),
        MaterialPaletteSource::GtaInstall { .. }
    ));

    assert_eq!(palette.entries()[0].name, "DEFAULT");
    assert_eq!(palette.entries()[1].name, "CONCRETE");

    let concrete = palette.rgb_for(1);
    assert!(
        concrete[0] >= 120 && concrete[0] <= 200,
        "concrete R={} out of expected 120..200 range",
        concrete[0]
    );
    assert_eq!(
        concrete[0], concrete[1],
        "concrete should be a neutral grey (R==G), got {concrete:?}"
    );
    assert_eq!(
        concrete[1], concrete[2],
        "concrete should be a neutral grey (G==B), got {concrete:?}"
    );
}
