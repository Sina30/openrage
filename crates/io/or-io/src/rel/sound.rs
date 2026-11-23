//! A sound file's items, typed: `rel_type` 54, such as `sounds.dat54.rel`.
//! Each item opens with its class id byte; every sound type but the four
//! lists (class ids 32 to 35) then states a [`Sound`] header. Every type is
//! packed.

use super::layout::{rel_items, rel_struct, Field, ItemReader, ItemWriter};
use super::refuse;
use crate::error::Result;

macro_rules! sound_header {
    ($($field:ident: $ty:ty),* $(,)?) => {
        /// The header a sound type opens with: a field is stated only when its bit, in field order, is set in a mask before them.
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        pub struct Sound {
            $(pub $field: Option<$ty>,)*
        }

        impl Field for Sound {
            const ALIGN: usize = 1;

            fn read_fields(r: &mut ItemReader<'_>) -> Result<Self> {
                let compression = u32::read_fields(r)?;
                let mut bit = 0;
                $(
                    let $field = (compression >> bit & 1 == 1)
                        .then(|| <$ty>::read_fields(r))
                        .transpose()?;
                    bit += 1;
                )*
                if compression >> bit != 0 {
                    return Err(refuse(format!(
                        "a sound header's compression mask {compression:#x} sets a bit past its {bit} fields"
                    )));
                }
                Ok(Self { $($field,)* })
            }

            fn write_fields(&self, w: &mut ItemWriter) -> Result<()> {
                let mut compression = 0u32;
                let mut bit = 0;
                $(
                    compression |= u32::from(self.$field.is_some()) << bit;
                    bit += 1;
                )*
                let _ = bit;
                compression.write_fields(w)?;
                $(
                    if let Some(value) = &self.$field {
                        value.write_fields(w)?;
                    }
                )*
                Ok(())
            }
        }
    };
}

sound_header! {
    flags: u32,
    parent_overrides: u16,
    volume: i16,
    volume_variance: u16,
    pitch: i16,
    pitch_variance: u16,
    pan: i16,
    pan_variance: u16,
    pre_delay: u16,
    pre_delay_variance: u16,
    start_offset: u32,
    start_offset_variance: u32,
    attack_time: u16,
    release_time: i16,
    doppler_factor: u16,
    category: u32,
    lpf_cutoff: u16,
    lpf_cutoff_variance: u16,
    hpf_cutoff: u16,
    hpf_cutoff_variance: u16,
    volume_curve: u32,
    volume_curve_scale: u16,
    volume_curve_plateau: u8,
    speaker_mask: u8,
    effect_route: u8,
    pre_delay_variable: u32,
    start_offset_variable: u32,
    small_reverb_send: u16,
    medium_reverb_send: u16,
    large_reverb_send: u16,
}

rel_struct! {
    pub struct LoopingSound: Sound {
        loop_count: i16,
        loop_count_variance: u16,
        loop_point: u16,
        sound_ref: u32,
        loop_count_variable: u32,
    }
}

rel_struct! {
    pub struct EnvelopeSound: Sound {
        envelope: Envelope,
        attack_variable: u32,
        decay_variable: u32,
        sustain_variable: u32,
        hold_variable: u32,
        release_variable: u32,
        sound_ref: u32,
        mode: u8,
        padding0: u8,
        padding1: u16,
        output_variable: u32,
        output_range: EnvelopeSoundOutputRange,
    }
}

rel_struct! {
    pub struct TwinLoopSound: Sound {
        min_swap_time: u16,
        max_swap_time: u16,
        min_crossfade_time: u16,
        max_crossfade_time: u16,
        crossfade_curve: u32,
        min_swap_time_variable: u32,
        max_swap_time_variable: u32,
        min_crossfade_time_variable: u32,
        max_crossfade_time_variable: u32,
        sound_ref: Vec<SoundRef> = u8,
    }
}

rel_struct! {
    pub struct SpeechSound: Sound {
        last_variation: u32,
        dynamic_field_name: u32,
        voice_name: u32,
        context_name: Vec<u8> = u8,
    }
}

rel_struct! {
    pub struct OnStopSound: Sound {
        child_sound_ref: u32,
        stop_sound_ref: u32,
        finished_sound_ref: u32,
    }
}

rel_struct! {
    pub struct WrapperSound: Sound {
        sound_ref: u32,
        last_play_time: u32,
        fallback_sound_ref: u32,
        min_repeat_time: u16,
        variable_refs: Vec<VariableRefs> = u8,
    }
}

rel_struct! {
    pub struct SequentialSound: Sound {
        sound_ref: Vec<SoundRef> = u8,
    }
}

rel_struct! {
    pub struct StreamingSound: Sound {
        duration: u32,
        sound_ref: Vec<SoundRef> = u8,
    }
}

rel_struct! {
    pub struct RetriggeredOverlappedSound: Sound {
        loop_count: i16,
        loop_count_variance: u16,
        delay_time: u16,
        delay_time_variance: u16,
        loop_count_variable: u32,
        delay_time_variable: u32,
        start_sound: u32,
        sound_ref: u32,
        stop_sound: u32,
    }
}

rel_struct! {
    pub struct CrossfadeSound: Sound {
        near_sound_ref: u32,
        far_sound_ref: u32,
        mode: u8,
        min_distance: f32,
        max_distance: f32,
        hysteresis: f32,
        crossfade_curve: u32,
        distance_variable: u32,
        min_distance_variable: u32,
        max_distance_variable: u32,
        hysteresis_variable: u32,
        crossfade_variable: u32,
    }
}

rel_struct! {
    pub struct CollapsingStereoSound: Sound {
        left_sound_ref: u32,
        right_sound_ref: u32,
        min_distance: f32,
        max_distance: f32,
        min_distance_variable: u32,
        max_distance_variable: u32,
        crossfade_override_variable: u32,
        frontend_left_pan: u32,
        frontend_right_pan: u32,
        position_relative_pan_damping: f32,
        position_relative_pan_damping_variable: u32,
        mode: u8,
    }
}

rel_struct! {
    pub struct SimpleSound: Sound {
        wave_ref: WaveRef,
        wave_slot_index: u8,
    }
}

rel_struct! {
    pub struct MultitrackSound: Sound {
        sound_ref: Vec<SoundRef> = u8,
    }
}

rel_struct! {
    pub struct RandomizedSound: Sound {
        history_index: u8,
        history_space: Vec<i8> = u8,
        variations: Vec<Variations> = u8,
    }
}

rel_struct! {
    pub struct EnvironmentSound: Sound {
        channel_id: u8,
    }
}

rel_struct! {
    pub struct DynamicEntitySound: Sound {
        object_refs: Vec<ObjectRefs> = u8,
    }
}

rel_struct! {
    pub struct SequentialOverlapSound: Sound {
        delay_time: u16,
        delay_time_variable: u32,
        sequence_direction: u32,
        child_sound: Vec<ChildSound> = u8,
    }
}

rel_struct! {
    pub struct ModularSynthSound: Sound {
        synth_def: u32,
        preset: u32,
        play_back_time_limit: f32,
        virtualisation_mode: u8,
        pad1: u8,
        pad0: u16,
        num_environment_sounds: u32,
        environment_sound: [ModularSynthSoundEnvironmentSound; 4],
        exposed_variable: Vec<ExposedVariable> = u32,
    }
}

rel_struct! {
    pub struct GranularSound: Sound {
        wave_slot_ptr: u32,
        channel0: Channel0,
        channel1: Channel1,
        channel2: Channel2,
        channel3: Channel3,
        channel4: Channel4,
        channel5: Channel5,
        channel_settings0: ChannelSettings0,
        channel_settings1: ChannelSettings1,
        channel_settings2: ChannelSettings2,
        channel_settings3: ChannelSettings3,
        channel_settings4: ChannelSettings4,
        channel_settings5: ChannelSettings5,
        loop_randomisation_change_rate: f32,
        loop_randomisation_pitch_fraction: f32,
        channel0_volume: i16,
        channel1_volume: i16,
        channel2_volume: i16,
        channel3_volume: i16,
        channel4_volume: i16,
        channel5_volume: i16,
        parent_sound: u32,
        granular_clock: Vec<GranularClock> = u8,
    }
}

rel_struct! {
    pub struct DirectionalSound: Sound {
        sound_ref: u32,
        inner_angle: f32,
        outer_angle: f32,
        rear_attenuation: f32,
        yaw_angle: f32,
        pitch_angle: f32,
    }
}

rel_struct! {
    pub struct KineticSound: Sound {
        sound_ref: u32,
        mass: f32,
        yaw_angle: f32,
        pitch_angle: f32,
    }
}

rel_struct! {
    pub struct SwitchSound: Sound {
        variable: u32,
        sound_ref: Vec<SoundRef> = u8,
    }
}

rel_struct! {
    pub struct VariableCurveSound: Sound {
        sound_ref: u32,
        input_variable: u32,
        output_variable: u32,
        curve: u32,
    }
}

rel_struct! {
    pub struct VariablePrintValueSound: Sound {
        variable: u32,
        message: Vec<u8> = u8,
    }
}

rel_struct! {
    pub struct VariableBlockSound: Sound {
        sound_ref: u32,
        variable: Vec<Variable> = u8,
    }
}

rel_struct! {
    pub struct IfSound: Sound {
        true_sound_ref: u32,
        false_sound_ref: u32,
        left_variable: u32,
        condition_type: u8,
        rhs: RHS,
    }
}

rel_struct! {
    pub struct MathOperationSound: Sound {
        sound_ref: u32,
        operation: Vec<Operation> = u8,
    }
}

rel_struct! {
    pub struct ParameterTransformSound: Sound {
        sound_ref: u32,
        parameter_transform: Vec<ParameterTransform> = u32,
    }
}

rel_struct! {
    pub struct FluctuatorSound: Sound {
        sound_ref: u32,
        fluctuator: Vec<Fluctuator> = u32,
    }
}

rel_struct! {
    pub struct AutomationSound: Sound {
        sound_ref: u32,
        playback_rate: f32,
        playback_rate_variance: f32,
        playback_rate_variable: u32,
        note_map: u32,
        data_ref: DataRef,
        variable_outputs: Vec<VariableOutputs> = u32,
    }
}

rel_struct! {
    pub struct ExternalStreamSound: Sound {
        num_environment_sounds: u8,
        environment_sound: [ExternalStreamSoundEnvironmentSound; 4],
    }
}

rel_struct! {
    pub struct SoundSet {
        flags: u32,
        sounds: Vec<Sounds> = u32,
    }
}

rel_struct! {
    pub struct NoteMap {
        flags: u32,
        range: Vec<Range> = u8,
    }
}

rel_struct! {
    pub struct SoundSetList {
        flags: u32,
        sound_set: Vec<SoundSetListSoundSet> = u32,
    }
}

rel_struct! {
    pub struct SoundHashList {
        flags: u32,
        current_sound_idx: u16,
        sound_hashes: Vec<SoundHashes> = u32,
    }
}

rel_struct! {
    pub struct Envelope {
        attack: u16,
        attack_variance: u16,
        decay: u16,
        decay_variance: u16,
        sustain: u8,
        sustain_variance: u8,
        hold: i32,
        hold_variance: u16,
        release: i32,
        release_variance: u32,
        attack_curve: u32,
        decay_curve: u32,
        release_curve: u32,
    }
}

rel_struct! {
    pub struct EnvelopeSoundOutputRange {
        min: f32,
        max: f32,
    }
}

rel_struct! {
    pub struct SoundRef {
        sound_id: u32,
    }
}

rel_struct! {
    pub struct VariableRefs {
        vr_ref: u32,
        destination: u8,
    }
}

rel_struct! {
    pub struct WaveRef {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct Variations {
        variation: u32,
        weight: f32,
    }
}

rel_struct! {
    pub struct ObjectRefs {
        game_object_hash: u32,
    }
}

rel_struct! {
    pub struct ChildSound {
        sound_ref: u32,
    }
}

rel_struct! {
    pub struct ModularSynthSoundEnvironmentSound {
        sound_ref: u32,
    }
}

rel_struct! {
    pub struct ExposedVariable {
        synth_key: u32,
        variable: u32,
        value: f32,
    }
}

rel_struct! {
    pub struct Channel0 {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct Channel1 {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct Channel2 {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct Channel3 {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct Channel4 {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct Channel5 {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct ChannelSettings0 {
        output_buffer: u8,
        granular_clock_index: u8,
        stretch_to_min_pitch0: u8,
        stretch_to_max_pitch0: u8,
        max_loop_proportion: f32,
    }
}

rel_struct! {
    pub struct ChannelSettings1 {
        output_buffer: u8,
        granular_clock_index: u8,
        stretch_to_min_pitch1: u8,
        stretch_to_max_pitch1: u8,
        max_loop_proportion: f32,
    }
}

rel_struct! {
    pub struct ChannelSettings2 {
        output_buffer: u8,
        granular_clock_index: u8,
        stretch_to_min_pitch2: u8,
        stretch_to_max_pitch2: u8,
        max_loop_proportion: f32,
    }
}

rel_struct! {
    pub struct ChannelSettings3 {
        output_buffer: u8,
        granular_clock_index: u8,
        stretch_to_min_pitch3: u8,
        stretch_to_max_pitch3: u8,
        max_loop_proportion: f32,
    }
}

rel_struct! {
    pub struct ChannelSettings4 {
        output_buffer: u8,
        granular_clock_index: u8,
        stretch_to_min_pitch4: u8,
        stretch_to_max_pitch4: u8,
        max_loop_proportion: f32,
    }
}

rel_struct! {
    pub struct ChannelSettings5 {
        output_buffer: u8,
        granular_clock_index: u8,
        stretch_to_min_pitch5: u8,
        stretch_to_max_pitch5: u8,
        max_loop_proportion: f32,
    }
}

rel_struct! {
    pub struct GranularClock {
        min_pitch: f32,
        max_pitch: f32,
    }
}

rel_struct! {
    pub struct Variable {
        variable_ref: u32,
        variable_value: f32,
        initial_variance: f32,
        usage: u8,
    }
}

rel_struct! {
    pub struct RHS {
        right_value: f32,
        right_variable: u32,
    }
}

rel_struct! {
    pub struct Operation {
        operation: u8,
        param1: Param1,
        param2: Param2,
        param3: Param3,
        result: u32,
    }
}

rel_struct! {
    pub struct Param1 {
        value: f32,
        variable: u32,
    }
}

rel_struct! {
    pub struct Param2 {
        value: f32,
        variable: u32,
    }
}

rel_struct! {
    pub struct Param3 {
        value: f32,
        variable: u32,
    }
}

rel_struct! {
    pub struct ParameterTransform {
        input_variable: u32,
        input_range: InputRange,
        output: Vec<Output> = u32,
    }
}

rel_struct! {
    pub struct InputRange {
        min: f32,
        max: f32,
    }
}

rel_struct! {
    pub struct Output {
        input_smooth_rate: f32,
        destination: u8,
        padding0: u8,
        padding1: u8,
        padding2: u8,
        output_variable: u32,
        output_range: OutputOutputRange,
        transform_points: Vec<TransformPoints> = u32,
    }
}

rel_struct! {
    pub struct OutputOutputRange {
        min: f32,
        max: f32,
    }
}

rel_struct! {
    pub struct TransformPoints {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct Fluctuator {
        mode: u8,
        destination: u8,
        output_variable: u32,
        increase_rate: f32,
        decrease_rate: f32,
        band_one_minimum: f32,
        band_one_maximum: f32,
        band_two_minimum: f32,
        band_two_maximum: f32,
        intra_band_flip_probabilty: f32,
        inter_band_flip_probabilty: f32,
        min_switch_time: u32,
        max_switch_time: u32,
        initial_value: f32,
    }
}

rel_struct! {
    pub struct DataRef {
        bank_name: u32,
        wave_name: u32,
    }
}

rel_struct! {
    pub struct VariableOutputs {
        channel_id: u32,
        output_variable: u32,
    }
}

rel_struct! {
    pub struct ExternalStreamSoundEnvironmentSound {
        sound_ref: u32,
    }
}

rel_struct! {
    pub struct Sounds {
        name: u32,
        sound: u32,
    }
}

rel_struct! {
    pub struct Range {
        first_note_id: u8,
        last_note_id: u8,
        mode: u8,
        sound_ref: u32,
    }
}

rel_struct! {
    pub struct SoundSetListSoundSet {
        sound_set_hash: u32,
    }
}

rel_struct! {
    pub struct SoundHashes {
        sound_hash: u32,
    }
}

rel_items! {
    pub enum SoundItem, class byte 1 {
        1 => LoopingSound packed,
        2 => EnvelopeSound packed,
        3 => TwinLoopSound packed,
        4 => SpeechSound packed,
        5 => OnStopSound packed,
        6 => WrapperSound packed,
        7 => SequentialSound packed,
        8 => StreamingSound packed,
        9 => RetriggeredOverlappedSound packed,
        10 => CrossfadeSound packed,
        11 => CollapsingStereoSound packed,
        12 => SimpleSound packed,
        13 => MultitrackSound packed,
        14 => RandomizedSound packed,
        15 => EnvironmentSound packed,
        16 => DynamicEntitySound packed,
        17 => SequentialOverlapSound packed,
        18 => ModularSynthSound packed,
        19 => GranularSound packed,
        20 => DirectionalSound packed,
        21 => KineticSound packed,
        22 => SwitchSound packed,
        23 => VariableCurveSound packed,
        24 => VariablePrintValueSound packed,
        25 => VariableBlockSound packed,
        26 => IfSound packed,
        27 => MathOperationSound packed,
        28 => ParameterTransformSound packed,
        29 => FluctuatorSound packed,
        30 => AutomationSound packed,
        31 => ExternalStreamSound packed,
        32 => SoundSet packed,
        33 => NoteMap packed,
        34 => SoundSetList packed,
        35 => SoundHashList packed,
    }
}

impl SoundItem {
    /// Reads an item of a `.dat54` file, which holds three bytes after its class id that its `.dat54.rel` sibling's item lacks.
    pub fn decode_dat54(body: &[u8]) -> Result<([u8; 3], Self)> {
        let (head, fields) = body
            .split_at_checked(4)
            .ok_or_else(|| refuse(format!("a .dat54 item is {} bytes", body.len())))?;
        let mut stripped = Vec::with_capacity(body.len() - 3);
        stripped.push(head[0]);
        stripped.extend_from_slice(fields);
        Ok(([head[1], head[2], head[3]], Self::decode(&stripped)?))
    }
}
