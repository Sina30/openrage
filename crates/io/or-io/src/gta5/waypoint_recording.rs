//! `.ywr`: one recorded route a ped or vehicle follows, waypoint by waypoint.

use super::path_region::PgBasePageMap;
use crate::decl::*;
use crate::Generation;

impl Resource for FwWaypointRecordingRoute {
    const EXT: &'static str = "ywr";
    const TARGET: Generation = Generation::Gen8;
}

/// `fwWaypointRecordingRoute`: a `.ywr`'s root, with its `pgBase` members first.
#[record(size = 0x28)]
pub struct FwWaypointRecordingRoute {
    vft: u64,
    pub first_node: Option<Box<PgBasePageMap>>,
    #[note("0 in every shipped route")]
    pub flags1: u32,
    #[note("0 in every shipped route")]
    pub flags2: u32,
    #[len(field = "num_waypoints")]
    pub waypoints: Vec<RouteData>,
    #[pad_after(4)]
    num_waypoints: i32,
}

/// One waypoint: a position and two packed words.
///
/// ⚠ Twenty bytes, so a run of them ends off sixteen-byte alignment and a
/// file pads the next allocation up to it.
#[record(size = 0x14)]
pub struct RouteData {
    pub xyz: [f32; 3],
    #[note("flags in bits 0 to 15, the heading in 16 to 23, the speed in 24 to 31")]
    pub packed_struct1_as_integer: u32,
    #[note("the free space on the left in bits 0 to 7, on the right in 8 to 15")]
    pub packed_struct2_as_integer: u32,
}

/// Every invariant the engine assumes of a `.ywr` and never checks: none beyond reading.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), ViewError> {
    View::<Raw<FwWaypointRecordingRoute>>::at_offset(image, root)?.waypoints()?;
    Ok(())
}
