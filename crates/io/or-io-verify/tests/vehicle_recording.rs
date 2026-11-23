//! `.yvr` through its declarations: the gates `declared_gates!` writes, over
//! the corpus; then every file both installs ship, opened owned and saved to
//! its fixed point, with its frames' stride held to the bytes.
//!
//! ⚠ No testimony was ever recorded for `.yvr`, so the two testimony gates
//! skip; both installs ship it at version 1, the Legacy layout.

mod install;

use std::mem::size_of;

use or_io::decl::{Described, File, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::vehicle_recording::*;
use or_io::Generation;
use or_io_verify::gates::{offset_of_run, payload, Filed, Gates, Opened};

const EXT: &str = "yvr";

fn put<R: Record + Described>(out: &mut Filed, v: &View<'_, R>) {
    out.structs.insert(
        (View::offset(v), R::name()),
        Plain::as_bytes(View::get(v)).to_vec(),
    );
}

fn walk(image: Image<'_>) -> Result<Filed, ViewError> {
    let mut out = Filed::default();
    let root = View::<Raw<CVehicleRecording>>::at_offset(image, 0)?;
    put(&mut out, &root);
    if let Some(pages) = root.first_node()? {
        put(&mut out, &pages);
        let slots = pages.page_info()?;
        if let Some(at) = offset_of_run(image, &slots) {
            out.runs.insert(
                at,
                slots.iter().flat_map(|v| v.as_bytes().to_vec()).collect(),
            );
        }
    }
    for state in root.states()?.views() {
        put(&mut out, &state);
    }
    Ok(out)
}

fn typed(image: Image<'_>) -> Result<Filed, String> {
    walk(image).map_err(|e| e.to_string())
}

const GATES: Gates = Gates::new(EXT, Generation::Gen8, ("CVehicleStateEachFrame", "coors_x"));

or_io_verify::declared_gates! {
    gates: GATES,
    typed: typed,
    reads_typed: every_yvr_reads_typed_as_the_tree_reads,
    owned: CVehicleRecording => every_yvr_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_yvr_saves_to_a_fixed_point,
    census: every_yvr_instance_is_aligned_for_its_view,
    testimony: the_yvr_testimony_holds_through_its_declarations,
    one_byte_late: a_yvr_field_one_byte_late_is_named_by_the_testimony,
}

/// Where a shipped file's frames end, by the declared stride, against where its page map starts.
fn frames_end_at_the_page_map(file: &[u8]) -> Result<(usize, usize), String> {
    let opened = Opened::of(file);
    let image = opened.image();
    let root = View::<Raw<CVehicleRecording>>::at_offset(image, 0).map_err(|e| e.to_string())?;
    let states = root.states().map_err(|e| e.to_string())?;
    let pages = root
        .first_node()
        .map_err(|e| e.to_string())?
        .ok_or("no page map")?;
    let start = offset_of_run(image, &states).unwrap_or(size_of::<Raw<CVehicleRecording>>());
    let end = start + states.len() * size_of::<Raw<CVehicleStateEachFrame>>();
    Ok((end, View::offset(&pages)))
}

/// Every shipped `.yvr` opens owned, saves, reopens equal and saves again to
/// the same bytes; and its frames, at the declared stride, end exactly where
/// the page map the file allocates next begins.
#[test]
fn every_shipped_yvr_opens_owned_and_reaches_its_fixed_point() {
    let Some(roots) = install::roots("every shipped .yvr to its fixed point") else {
        return;
    };
    for root in roots {
        let (mut seen, mut fixed, mut frames) = (0usize, 0usize, 0usize);
        let mut failed: Vec<String> = Vec::new();
        install::every(&root, EXT, &mut |label, bytes| {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            seen += 1;
            match frames_end_at_the_page_map(&bytes) {
                Ok((end, pages)) if end == pages => {}
                Ok((end, pages)) => {
                    return failed.push(format!(
                        "{label}: the frames end at {end:#x}, the page map at {pages:#x}"
                    ))
                }
                Err(e) => return failed.push(format!("{label}: {e}")),
            }
            let owned = match CVehicleRecording::from_bytes(&bytes) {
                Ok(owned) => owned,
                Err(e) => return failed.push(format!("{label}: open: {e}")),
            };
            frames += owned.states.len();
            let saved = match owned.to_bytes() {
                Ok(saved) => saved,
                Err(e) => return failed.push(format!("{label}: save: {e}")),
            };
            let again = match CVehicleRecording::from_bytes(&saved) {
                Ok(again) => again,
                Err(e) => return failed.push(format!("{label}: reopen: {e}")),
            };
            if again.states != owned.states {
                return failed.push(format!("{label}: the frames reopen differently"));
            }
            match again.to_bytes() {
                Ok(resaved) if payload(&resaved) == payload(&saved) => fixed += 1,
                Ok(_) => failed.push(format!("{label}: the second save differs from the first")),
                Err(e) => failed.push(format!("{label}: save again: {e}")),
            }
        });
        println!(
            "{}: {seen} .yvr, {frames} frames; {fixed} at their fixed point",
            root.display()
        );
        failed.truncate(20);
        assert!(failed.is_empty(), "{}", failed.join("\n"));
        assert!(seen > 0, "{} ships no .yvr", root.display());
        assert_eq!(fixed, seen);
    }
}
