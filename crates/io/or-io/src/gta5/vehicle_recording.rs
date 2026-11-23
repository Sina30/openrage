//! `.yvr`: one vehicle's recorded drive, a state per sampled frame.

use super::path_region::PgBasePageMap;
use crate::decl::*;
use crate::Generation;

impl Resource for CVehicleRecording {
    const EXT: &'static str = "yvr";
    const TARGET: Generation = Generation::Gen8;
}

/// `CVehicleRecording`: a `.yvr`'s root, with its `pgBase` members first.
#[record(size = 0x20)]
pub struct CVehicleRecording {
    vft: u64,
    pub first_node: Option<Box<PgBasePageMap>>,
    pub states: Vec<CVehicleStateEachFrame>,
}

/// One sampled frame of a recording.
///
/// ⚠ Each speed is metres per second times `32767 / 120`, and each matrix
/// component an axis of the vehicle's rotation times 127.
#[record(size = 0x20)]
pub struct CVehicleStateEachFrame {
    /// Milliseconds from the recording's start.
    pub time_in_recording: u32,
    pub speed_x: i16,
    pub speed_y: i16,
    pub speed_z: i16,
    pub matrix_a_x: i8,
    pub matrix_a_y: i8,
    pub matrix_a_z: i8,
    pub matrix_b_x: i8,
    pub matrix_b_y: i8,
    pub matrix_b_z: i8,
    pub steer_angle: i8,
    pub gas: i8,
    pub brake: i8,
    pub hand_brake: i8,
    pub coors_x: f32,
    pub coors_y: f32,
    pub coors_z: f32,
}

/// Every invariant the engine assumes of a `.yvr` and never checks: none beyond reading.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), ViewError> {
    View::<Raw<CVehicleRecording>>::at_offset(image, root)?.states()?;
    Ok(())
}
