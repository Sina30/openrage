//! Animation channels: the nine bit-packed channel formats of a block stream,
//! decoded per DOF and written back, over any animation that implements
//! [`AnimationSource`].

pub mod channel;
pub mod encode;
pub mod packed;
pub mod sample;

pub use channel::{
    decode_dof, ChannelFormat, ChannelWalk, DecodedChannel, DecodedDof, DofDecoder, DofSamples,
    MoverChannelRef,
};
pub use encode::{
    encode_block, format_of_track, reencode_animation, BitWriter, DofSampleTable, EncodePolicy,
    EncodedBlock,
};
pub use packed::{PackedArray, PackedSequence};
pub use sample::{sample_bone, BonePose};

/// The bytes a block stream's header precedes, which its offsets count.
pub const BLOCK_HEADER_SIZE: usize = 0x20;

/// An animation's flag that its blocks are compressed (`kCompact`).
pub const COMPACT: u16 = 1 << 10;

/// What a DOF drives (`kTrack*`); stored in files, so never renumbered.
pub mod track {
    pub const BONE_TRANSLATION: u8 = 0;
    pub const BONE_ROTATION: u8 = 1;
    pub const BONE_SCALE: u8 = 2;
    pub const VISIBILITY: u8 = 4;
    pub const MOVER_TRANSLATION: u8 = 5;
    pub const MOVER_ROTATION: u8 = 6;
}

/// The value a DOF holds (`kFormatType*`).
pub mod format_type {
    pub const VECTOR3: u8 = 0;
    pub const QUATERNION: u8 = 1;
    pub const FLOAT: u8 = 2;

    /// The floats a value of `format` holds; zero for a format the engine does not define.
    pub fn component_count(format: u8) -> usize {
        match format {
            VECTOR3 => 3,
            QUATERNION => 4,
            FLOAT => 1,
            _ => 0,
        }
    }
}

/// What the codec reads of one block stream.
pub trait BlockSource {
    /// The bytes after the header: constants, frames, then the channel directory.
    fn data(&self) -> &[u8];
    fn compact_size(&self) -> u32;
    fn constant_size(&self) -> u32;
    fn num_frames(&self) -> u16;
    fn frame_size(&self) -> u16;
    fn segment_size(&self) -> u8;

    fn is_compact(&self) -> bool {
        self.compact_size() != 0
    }

    /// Where the channel directory starts in [`BlockSource::data`].
    ///
    /// ⚠ Derived, never the header's `channel_offset`, which locates the
    /// root-motion reference table counted from the start of the header.
    fn channel_directory_offset(&self) -> usize {
        (self.constant_size() as usize).saturating_add(
            usize::from(self.frame_size()).saturating_mul(usize::from(self.num_frames())),
        )
    }
}

/// What the codec reads of one animation: its timing, its DOFs and its blocks.
pub trait AnimationSource {
    type Block: BlockSource;

    fn name(&self) -> &str;
    fn num_frames(&self) -> u16;
    fn frames_per_chunk(&self) -> u16;
    fn duration(&self) -> f32;
    fn dof_count(&self) -> usize;
    /// DOF `index`'s format type.
    fn dof_format(&self, index: usize) -> Option<u8>;
    /// The first DOF driving `track` of the bone tagged `id`.
    fn find_dof(&self, id: u16, track: u8) -> Option<usize>;
    fn block_count(&self) -> usize;
    /// Block `index`; `None` past the end or where the file holds none.
    fn block(&self, index: usize) -> Option<&Self::Block>;

    fn time_to_internal_frame(&self, time: f32) -> f32 {
        if self.duration() <= 0.0 || self.num_frames() < 2 {
            return 0.0;
        }
        time * (self.num_frames() - 1) as f32 / self.duration()
    }

    fn internal_frame_to_time(&self, internal_frame: f32) -> f32 {
        if self.num_frames() < 2 {
            return 0.0;
        }
        internal_frame * self.duration() / (self.num_frames() - 1) as f32
    }

    fn internal_frame_to_block_index(&self, internal_frame: f32) -> usize {
        if self.frames_per_chunk() == 0 {
            return 0;
        }
        let frame = internal_frame.max(0.0) as usize;
        (frame / usize::from(self.frames_per_chunk())).min(self.block_count().saturating_sub(1))
    }

    fn internal_frame_to_block_time(&self, internal_frame: f32, block_index: usize) -> f32 {
        let chunk = f32::from(self.frames_per_chunk());
        let base = (block_index * usize::from(self.frames_per_chunk())) as f32;
        (internal_frame - base).clamp(0.0, chunk)
    }

    /// The blocks an animation of this length and chunk size holds.
    fn calc_num_blocks(&self) -> usize {
        if self.num_frames() <= 1 || self.frames_per_chunk() == 0 {
            return 1;
        }
        (usize::from(self.num_frames() - 2) / usize::from(self.frames_per_chunk())) + 1
    }

    /// The frames block `block_index` holds, the last shared with the next block.
    fn frames_in_block(&self, block_index: usize) -> usize {
        let chunk = usize::from(self.frames_per_chunk());
        if chunk == 0 {
            return 0;
        }
        if block_index + 1 < self.calc_num_blocks() {
            return chunk + 1;
        }
        match usize::from(self.num_frames().saturating_sub(1)) % chunk {
            0 => chunk + 1,
            remainder => remainder + 1,
        }
    }
}

/// An animation [`reencode_animation`] can write its blocks back into.
pub trait AnimationSink: AnimationSource {
    /// Replaces every block, states the largest one's payload size, and clears [`COMPACT`].
    fn store_blocks(&mut self, blocks: Vec<EncodedBlock>, max_block_size: u32);
}
