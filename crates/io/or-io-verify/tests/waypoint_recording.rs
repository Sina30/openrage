//! `.ywr` through its declarations: the gates `declared_gates!` writes, over
//! the corpus; then every file both installs ship, opened owned and saved to
//! its fixed point, with its waypoints' stride held to the bytes.
//!
//! ⚠ No testimony was ever recorded for `.ywr`, so the two testimony gates
//! skip; both installs ship it at version 1, the Legacy layout.

mod install;

use std::mem::size_of;

use or_io::decl::{Described, File, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::waypoint_recording::*;
use or_io::Generation;
use or_io_verify::gates::{offset_of_run, payload, Filed, Gates, Opened};

const EXT: &str = "ywr";

/// The alignment a file pads a waypoint run's end up to before its page map.
const PAGE_MAP_ALIGN: usize = 16;

fn put<R: Record + Described>(out: &mut Filed, v: &View<'_, R>) {
    out.structs.insert(
        (View::offset(v), R::name()),
        Plain::as_bytes(View::get(v)).to_vec(),
    );
}

fn walk(image: Image<'_>) -> Result<Filed, ViewError> {
    let mut out = Filed::default();
    let root = View::<Raw<FwWaypointRecordingRoute>>::at_offset(image, 0)?;
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
    for waypoint in root.waypoints()?.views() {
        put(&mut out, &waypoint);
    }
    Ok(out)
}

fn typed(image: Image<'_>) -> Result<Filed, String> {
    walk(image).map_err(|e| e.to_string())
}

const GATES: Gates = Gates::new(EXT, Generation::Gen8, ("RouteData", "xyz"));

or_io_verify::declared_gates! {
    gates: GATES,
    typed: typed,
    reads_typed: every_ywr_reads_typed_as_the_tree_reads,
    owned: FwWaypointRecordingRoute => every_ywr_opens_owned_and_saves_as_its_tree_saves,
    fixed_point: every_ywr_saves_to_a_fixed_point,
    census: every_ywr_instance_is_aligned_for_its_view,
    testimony: the_ywr_testimony_holds_through_its_declarations,
    one_byte_late: a_ywr_field_one_byte_late_is_named_by_the_testimony,
}

/// Where a shipped file's waypoints end, by the declared stride, against where its page map starts.
fn waypoints_end_before_the_page_map(file: &[u8]) -> Result<(usize, usize), String> {
    let opened = Opened::of(file);
    let image = opened.image();
    let root =
        View::<Raw<FwWaypointRecordingRoute>>::at_offset(image, 0).map_err(|e| e.to_string())?;
    let waypoints = root.waypoints().map_err(|e| e.to_string())?;
    let pages = root
        .first_node()
        .map_err(|e| e.to_string())?
        .ok_or("no page map")?;
    let start = offset_of_run(image, &waypoints).ok_or("no waypoints")?;
    let end = start + waypoints.len() * size_of::<Raw<RouteData>>();
    Ok((end, View::offset(&pages)))
}

/// Every shipped `.ywr` opens owned, saves, reopens equal and saves again to
/// the same bytes; and its waypoints, at the declared stride, end in the
/// padding before the page map the file allocates next, never past it.
#[test]
fn every_shipped_ywr_opens_owned_and_reaches_its_fixed_point() {
    let Some(roots) = install::roots("every shipped .ywr to its fixed point") else {
        return;
    };
    for root in roots {
        let (mut seen, mut fixed, mut waypoints) = (0usize, 0usize, 0usize);
        let mut failed: Vec<String> = Vec::new();
        install::every(&root, EXT, &mut |label, bytes| {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            seen += 1;
            match waypoints_end_before_the_page_map(&bytes) {
                Ok((end, pages)) if end.next_multiple_of(PAGE_MAP_ALIGN) == pages => {}
                Ok((end, pages)) => {
                    return failed.push(format!(
                        "{label}: the waypoints end at {end:#x}, the page map at {pages:#x}"
                    ))
                }
                Err(e) => return failed.push(format!("{label}: {e}")),
            }
            let owned = match FwWaypointRecordingRoute::from_bytes(&bytes) {
                Ok(owned) => owned,
                Err(e) => return failed.push(format!("{label}: open: {e}")),
            };
            waypoints += owned.waypoints.len();
            let saved = match owned.to_bytes() {
                Ok(saved) => saved,
                Err(e) => return failed.push(format!("{label}: save: {e}")),
            };
            let again = match FwWaypointRecordingRoute::from_bytes(&saved) {
                Ok(again) => again,
                Err(e) => return failed.push(format!("{label}: reopen: {e}")),
            };
            if again.waypoints != owned.waypoints {
                return failed.push(format!("{label}: the waypoints reopen differently"));
            }
            match again.to_bytes() {
                Ok(resaved) if payload(&resaved) == payload(&saved) => fixed += 1,
                Ok(_) => failed.push(format!("{label}: the second save differs from the first")),
                Err(e) => failed.push(format!("{label}: save again: {e}")),
            }
        });
        println!(
            "{}: {seen} .ywr, {waypoints} waypoints; {fixed} at their fixed point",
            root.display()
        );
        failed.truncate(20);
        assert!(failed.is_empty(), "{}", failed.join("\n"));
        assert!(seen > 0, "{} ships no .ywr", root.display());
        assert_eq!(fixed, seen);
    }
}
