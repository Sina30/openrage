//! The name scan against a real install, which is the only place it can be
//! shown to produce anything at all.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, ExtractionProgress, Gta5RetailCryptoProvider};
use or_io::container::RpfCryptoProvider;
use or_title::vfs::{build_mounted_game, DlcCutoff};
use or_title::{scan_install_names, GtaInstall, InstalledNameKind};

const WHAT: &str = "an_install_supplies_a_name_for_every_category_the_scan_offers";
const INSTALL_ENV: &str = "GTA_PTFX_GTA_INSTALL";

fn install_dir_from_env() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(INSTALL_ENV).ok()?);
    path.is_dir().then_some(path)
}

#[test]
fn an_install_supplies_a_name_for_every_category_the_scan_offers() {
    let Some(install) = install_dir_from_env() else {
        or_test_support::skip_corpus(
            WHAT,
            format_args!("set {INSTALL_ENV} to a GTA V install dir to run this test"),
        );
        return;
    };
    let exe = install.join("GTA5.exe");
    if !exe.exists() {
        or_test_support::skip_corpus(
            WHAT,
            format_args!("{} not present, so no keys", exe.display()),
        );
        return;
    }

    let progress = move |_: ExtractionProgress| {};
    let Ok(keys) = extract_keys(&exe, &progress) else {
        or_test_support::skip_corpus(WHAT, "key extraction failed");
        return;
    };
    let crypto: Arc<dyn RpfCryptoProvider> =
        Arc::new(Gta5RetailCryptoProvider::new(Arc::new(keys)));

    let game = build_mounted_game(
        &GtaInstall {
            root: install.clone(),
        },
        crypto,
        DlcCutoff::BaseGame,
    )
    .expect("a retail install mounts");

    let cancelled = AtomicBool::new(false);
    let mut counts: Vec<(InstalledNameKind, usize)> = InstalledNameKind::ALL
        .iter()
        .map(|kind| (*kind, 0usize))
        .collect();
    let mut total = 0usize;
    scan_install_names(&game, &cancelled, 512, |batch| {
        total += batch.len();
        for (kind, _) in batch {
            if let Some(slot) = counts.iter_mut().find(|(k, _)| *k == kind) {
                slot.1 += 1;
            }
        }
    });

    assert!(
        total > 1000,
        "{} produced only {total} names, which is not a retail install — the \
         scan is wrong, not the game",
        install.display()
    );
    for (kind, count) in &counts {
        assert!(
            *count > 0,
            "{kind:?} is a category the scan offers and this install produced \
             none of it: {counts:?}"
        );
    }
}
