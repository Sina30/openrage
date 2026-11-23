use std::path::PathBuf;
use std::sync::Arc;

use or_io::container::{Gta5RetailCryptoProvider, KeysCache, RpfCryptoProvider};
use or_title::{build_vfs_for_install, load_clip_regions, GtaInstall};

fn install_dir() -> Option<PathBuf> {
    let raw = std::env::var("GTA_PTFX_GTA_INSTALL").ok()?;
    let p = PathBuf::from(raw);
    p.is_dir().then_some(p)
}

fn keys_cache_path() -> Option<PathBuf> {
    let raw = std::env::var("GTA_PTFX_KEYS_CACHE").ok()?;
    let p = PathBuf::from(raw);
    p.is_file().then_some(p)
}

#[test]
fn editor_cached_path_loads_clip_regions() {
    let Some(install) = install_dir() else {
        or_test_support::skip_corpus(
            "editor_cached_path_loads_clip_regions",
            "set GTA_PTFX_GTA_INSTALL",
        );
        return;
    };
    let Some(cache_path) = keys_cache_path() else {
        or_test_support::skip_corpus(
            "editor_cached_path_loads_clip_regions",
            "set GTA_PTFX_KEYS_CACHE to your gta5_keys.bin",
        );
        return;
    };

    let cache = match KeysCache::read_from(&cache_path) {
        Ok(Some(c)) => c,
        Ok(None) => panic!("Cache file `{}` empty", cache_path.display()),
        Err(e) => panic!("Cache read failed: {e}"),
    };
    eprintln!(
        "Cache loaded: gta5_path={}, sha1={}",
        cache.metadata.gta5_path.display(),
        cache.metadata.gta5_sha1_hex
    );
    let keys = cache.into_keys().expect("cache decode succeeds");
    let provider: Arc<dyn RpfCryptoProvider> =
        Arc::new(Gta5RetailCryptoProvider::new(Arc::new(keys)));

    let vfs = build_vfs_for_install(&GtaInstall { root: install }, provider)
        .expect("VFS build succeeds with cached keys");
    let table = load_clip_regions(&vfs).expect("clip-regions parse succeeds with cached keys");

    eprintln!(
        "Loaded {} clip-region entries via the cached-keys path",
        table.len()
    );
    assert!(table.len() > 100);
    assert!(table.get_by_name("ptfx_glow_rgb").is_some());
}
