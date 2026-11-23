//! A colour written as a literal is the same in both themes, so each file's
//! count of them is held here: a new one is a decision, made by changing the
//! ledger, and not an accident.
//!
//! ⚠ A literal that is drawn as text or as a mark that carries meaning goes
//! through `theme::legible` or `theme::graphic` where it is drawn; the ledger
//! says where literals are, not that they read.

use std::collections::BTreeMap;
use std::path::Path;

/// Every way a `Color32` is built from numbers.
const LITERALS: [&str; 6] = [
    "Color32::from_rgb(",
    "Color32::from_rgba_unmultiplied(",
    "Color32::from_rgba_premultiplied(",
    "Color32::from_gray(",
    "Color32::from_black_alpha(",
    "Color32::from_white_alpha(",
];

/// The theme, which is where a colour is written, and this file, which names the ways.
const EXEMPT: [&str; 2] = [
    "src/ui/theme.rs",
    "src/ui/no_new_colour_is_written_outside_the_theme.rs",
];

/// How many literals each file under `src/ui` and `src/app` holds.
const LEDGER: [(&str, usize); 14] = [
    ("src/ui/focus_ring.rs", 2),
    ("src/ui/no_text_the_shell_paints_reads_under_4_5_to_1.rs", 1),
    ("src/ui/panels/game_picker.rs", 1),
    ("src/ui/panels/inspector/gta6_drawable/head.rs", 1),
    ("src/ui/panels/maps/plan.rs", 3),
    ("src/ui/panels/maps/scanned.rs", 1),
    ("src/ui/panels/maps/world.rs", 6),
    ("src/ui/panels/settings_appearance.rs", 1),
    ("src/ui/panels/timeline.rs", 4),
    ("src/ui/panels/timeline_ycd.rs", 9),
    ("src/ui/panels/viewport.rs", 13),
    ("src/ui/timeline/channels.rs", 1),
    ("src/ui/timeline/provider.rs", 1),
    ("src/ui/timeline/ruler.rs", 2),
];

fn count(dir: &Path, root: &Path, into: &mut BTreeMap<String, usize>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            count(&path, root, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let name = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&path).expect("a source file reads");
            let found: usize = LITERALS.iter().map(|way| source.matches(way).count()).sum();
            if found > 0 && !EXEMPT.contains(&name.as_str()) {
                into.insert(name, found);
            }
        }
    }
}

#[test]
fn every_file_holds_the_colour_literals_the_ledger_says() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = BTreeMap::new();
    for dir in ["src/ui", "src/app"] {
        count(&root.join(dir), root, &mut found);
    }
    assert!(found.len() > 5, "the walk found {} files", found.len());
    let ledger: BTreeMap<String, usize> = LEDGER
        .iter()
        .map(|(name, held)| ((*name).to_string(), *held))
        .collect();
    let mut wrong = Vec::new();
    for name in found.keys().chain(ledger.keys()) {
        let (has, says) = (
            found.get(name).copied().unwrap_or(0),
            ledger.get(name).copied().unwrap_or(0),
        );
        if has != says {
            wrong.push(format!("{name}: {has} literals, the ledger says {says}"));
        }
    }
    wrong.sort();
    wrong.dedup();
    assert!(
        wrong.is_empty(),
        "take the colour from the theme, or change the ledger:\n{}",
        wrong.join("\n")
    );
}
