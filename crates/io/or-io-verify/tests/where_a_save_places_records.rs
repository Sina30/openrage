//! The alignment census over every declared format's canonical saves: a save
//! places every record at its natural alignment, so a view of a saved file
//! needs no `packed` declaration a shipped file does not (OD-ALIGN).

use std::collections::BTreeSet;

use or_io::gta5::formats::FORMATS;
use or_io_verify::census;
use or_io_verify::gates::{self, name, Opened};

#[test]
fn a_canonical_save_places_every_record_at_its_natural_alignment() {
    let mut off = Vec::new();
    for fmt in FORMATS {
        let ext = fmt.ext;
        for target in fmt.targets().collect::<BTreeSet<_>>() {
            let Some(paths) = gates::files(ext, Some(target), &format!("the saved .{ext} census"))
            else {
                continue;
            };
            let pack = gates::pack(ext, target);
            let root = fmt.root_name(target).expect("declared");
            let mut saved = census::Census::default();
            for path in &paths {
                let opened = Opened::read(path);
                let bytes = fmt
                    .read(target, opened.image())
                    .and_then(|t| fmt.save(target, &t))
                    .unwrap_or_else(|e| panic!(".{ext} save refused: {}: {e}", name(path)));
                saved.fold(census::take(&pack, Opened::of(&bytes).image(), &root, 0));
            }
            assert!(saved.unreached.is_empty(), "{:?}", saved.unreached);
            let instances: usize = saved.types.values().map(|t| t.instances).sum();
            println!(
                ".{ext} {}: {} file(s) saved, {instances} instance(s) of {} type(s)",
                target.slug(),
                paths.len(),
                saved.types.len()
            );
            for (ty, tally) in saved.misaligned() {
                let line = format!(
                    ".{ext} {}: {ty} {} of {} off alignment {}, first at {:#x?}",
                    target.slug(),
                    tally.misaligned,
                    tally.instances,
                    tally.alignment,
                    tally.examples
                );
                println!("    {line}");
                off.push(line);
            }
        }
    }
    assert!(off.is_empty(), "{}", off.join("\n"));
}
