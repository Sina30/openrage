use std::path::PathBuf;
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, ExtractionProgress, Gta5RetailCryptoProvider};
use or_io::container::RpfCryptoProvider;
use or_io::gta5::data::ptx_clip_regions::ClipRegionTable;
use or_title::{build_vfs_for_install, load_clip_regions, GtaInstall};

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
fn loads_clip_regions_from_local_install() {
    let Some(install) = install_dir_from_env() else {
        or_test_support::skip_corpus(
            "loads_clip_regions_from_local_install",
            "set GTA_PTFX_GTA_INSTALL to a GTA V install dir to run this test",
        );
        return;
    };
    let exe = install.join("GTA5.exe");
    if !exe.exists() {
        or_test_support::skip_corpus(
            "loads_clip_regions_from_local_install",
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
                "loads_clip_regions_from_local_install",
                format_args!("key extraction failed ({e})"),
            );
            return;
        }
    };
    let provider: Arc<dyn RpfCryptoProvider> = Arc::new(Gta5RetailCryptoProvider::new(keys));

    let gta_install = GtaInstall {
        root: install.clone(),
    };
    let vfs = build_vfs_for_install(&gta_install, provider).expect("VFS build succeeded");
    let table = load_clip_regions(&vfs).expect("ptxclipregions.dat parsed");

    assert!(
        table.len() >= 100,
        "expected at least 100 clip-region entries, got {}",
        table.len()
    );

    let must_be_present: &[&str] = &[
        "ptfx_glow_rgb",
        "ptfx_smoke_wispy_anim",
        "ptfx_embers_rgb",
        "ptfx_muzzle_core_rgb",
    ];

    let mut report = String::new();
    let mut missing: Vec<&str> = Vec::new();
    for name in must_be_present {
        match table.get_by_name(name) {
            Some(entry) => {
                report.push_str(&format!(
                    "  {name:30} -> {} cols x {} rows ({} frames)\n",
                    entry.num_columns,
                    entry.num_rows,
                    entry.frames.len(),
                ));
            }
            None => {
                report.push_str(&format!("  {name:30} -> MISSING\n"));
                missing.push(name);
            }
        }
    }
    eprintln!("clip-region lookup results:\n{report}");

    assert!(
        missing.is_empty(),
        "expected entries for {missing:?}; missing from {} (table.len() = {})",
        install.display(),
        table.len()
    );

    let mut multi_row_count = 0usize;
    for entry in table.entries() {
        if entry.num_rows > 1 && entry.num_columns > 1 {
            multi_row_count += 1;
        }
    }
    assert!(
        multi_row_count >= 10,
        "expected at least 10 multi-row atlases, got {multi_row_count} (parser regression?)"
    );
}

#[test]
fn resolve_atlas_dims_matches_engine_table() {
    let Some(install) = install_dir_from_env() else {
        or_test_support::skip_corpus(
            "resolve_atlas_dims_matches_engine_table",
            "set GTA_PTFX_GTA_INSTALL to a GTA V install dir to run this test",
        );
        return;
    };
    let exe = install.join("GTA5.exe");
    if !exe.exists() {
        or_test_support::skip_corpus(
            "resolve_atlas_dims_matches_engine_table",
            format_args!("{} not present", exe.display()),
        );
        return;
    }

    let progress = move |_: ExtractionProgress| {};
    let keys = match extract_keys(&exe, &progress) {
        Ok(k) => Arc::new(k),
        Err(e) => {
            or_test_support::skip_corpus(
                "resolve_atlas_dims_matches_engine_table",
                format_args!("key extraction failed ({e})"),
            );
            return;
        }
    };
    let provider: Arc<dyn RpfCryptoProvider> = Arc::new(Gta5RetailCryptoProvider::new(keys));
    let vfs = build_vfs_for_install(&GtaInstall { root: install }, provider).unwrap();
    let table = load_clip_regions(&vfs).unwrap();

    fn lookup(table: &ClipRegionTable, name: &str) -> Option<(u32, u32)> {
        let hash = or_io::string_hash(name.as_bytes());
        table.get_by_hash(hash).map(|e| (e.num_rows, e.num_columns))
    }

    let glow = lookup(&table, "ptfx_glow_rgb").expect("ptfx_glow_rgb in table");
    let wispy = lookup(&table, "ptfx_smoke_wispy_anim").expect("ptfx_smoke_wispy_anim in table");
    eprintln!("ptfx_glow_rgb         dims = {glow:?}");
    eprintln!("ptfx_smoke_wispy_anim dims = {wispy:?}");

    let upper = lookup(&table, "PTFX_GLOW_RGB").expect("uppercase still hits table");
    assert_eq!(upper, glow);
}
