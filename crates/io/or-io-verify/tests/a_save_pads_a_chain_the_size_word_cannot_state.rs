//! A Legacy texture whose mip chain is not a whole number of the size word's units is padded by a
//! save, so no staged file is refused for it: the measurement over every staged `.ytd`, `.ydr`,
//! `.ydd` and `.yft` and the mod `.ydd` files.

use std::path::PathBuf;

use or_io::decl::{Gen8, Resource};
use or_io::gta5::drawable::GtaDrawable;
use or_io::gta5::fragment::GtaFragType;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;
use or_test_support::{mods_corpus_origin, shared_corpus_root, Corpus, TARGET_GEN8};

/// What saving every file of one kind came to.
#[derive(Default)]
struct Saved {
    opened: usize,
    saved: usize,
    /// Files whose save is refused because a run's length cannot be stated through its mask.
    refused_for_the_mask: Vec<String>,
}

fn save_each<R: Resource>(paths: &[PathBuf], into: &mut Saved)
where
    or_io::decl::Raw<R>: or_io::decl::Record,
{
    for path in paths {
        let file = std::fs::read(path).expect("a corpus file reads");
        let Ok(owned) = R::from_bytes(&file) else {
            continue;
        };
        into.opened += 1;
        match owned.to_bytes() {
            Ok(_) => into.saved += 1,
            Err(e)
                if e.to_string()
                    .contains("states a length only through the mask") =>
            {
                into.refused_for_the_mask
                    .push(format!("{}: {e}", path.display()));
            }
            // Refused for another reason, which other suites own.
            Err(_) => {}
        }
    }
}

#[test]
fn no_staged_legacy_file_is_refused_at_save_for_a_chain_its_size_word_cannot_state() {
    let what = "a save pads a chain the size word cannot state";
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = |ext: &str| Corpus::with_root(&root, ext).target(TARGET_GEN8).find();
    let (ytd, ydr, ydd, yft) = (files("ytd"), files("ydr"), files("ydd"), files("yft"));
    if ytd.is_empty() && ydr.is_empty() && ydd.is_empty() && yft.is_empty() {
        or_test_support::skip(what, &root, "ytd");
        return;
    }
    let mods = mods_corpus_origin(env!("CARGO_MANIFEST_DIR"));
    let mod_ydd = Corpus::with_root(mods.path(), "ydd").all_targets().find();

    let mut saved = Saved::default();
    save_each::<PgDictionary<GrcTexture>>(&ytd, &mut saved);
    save_each::<GtaDrawable<Gen8>>(&ydr, &mut saved);
    save_each::<PgDictionary<GtaDrawable<Gen8>>>(&ydd, &mut saved);
    save_each::<GtaFragType<Gen8>>(&yft, &mut saved);
    save_each::<PgDictionary<GtaDrawable<Gen8>>>(&mod_ydd, &mut saved);

    println!(
        "{} staged Legacy files opened owned and {} saved; {} were refused for a length the size \
         word cannot state ({} of them the staged mod files)",
        saved.opened,
        saved.saved,
        saved.refused_for_the_mask.len(),
        mod_ydd.len(),
    );
    assert!(saved.opened > 0);
    assert!(
        saved.refused_for_the_mask.is_empty(),
        "{:#?}",
        saved.refused_for_the_mask
    );
}
