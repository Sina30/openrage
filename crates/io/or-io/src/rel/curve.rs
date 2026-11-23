//! A curve file's items, typed: `rel_type` 16, such as `curves.dat16.rel`.
//! Each item opens with its class id and name table offset in one word, and
//! lays its fields out at natural alignment after [`BaseCurve`]'s.

use super::layout::{rel_items, rel_struct, NameTableOffset};

rel_struct! {
    pub struct BaseCurve {
        name_table_offset: NameTableOffset,
        flags: u32,
        min_input: f32,
        max_input: f32,
    }
}

rel_struct! {
    pub struct Constant: BaseCurve {
        value: f32,
    }
}

rel_struct! {
    pub struct Linear: BaseCurve {
        left_hand_pair: LeftHandPair,
        right_hand_pair: RightHandPair,
    }
}

rel_struct! {
    pub struct LinearDb: BaseCurve {
        left_hand_pair: LeftHandPair,
        right_hand_pair: RightHandPair,
    }
}

rel_struct! {
    pub struct PiecewiseLinear: BaseCurve {
        point: Vec<Point> = u32,
    }
}

rel_struct! {
    pub struct EqualPower: BaseCurve {
        flip: u8,
    }
}

rel_struct! {
    pub struct ValueTable: BaseCurve {
        value: Vec<Value> = u16,
    }
}

rel_struct! {
    pub struct Exponential: BaseCurve {
        flip: u8,
        exponent: f32,
    }
}

rel_struct! {
    pub struct DecayingExponential: BaseCurve {
        horizontal_scaling: f32,
    }
}

rel_struct! {
    pub struct DecayingSquaredExponential: BaseCurve {
        horizontal_scaling: f32,
    }
}

rel_struct! {
    pub struct SineCurve: BaseCurve {
        start_phase: f32,
        end_phase: f32,
        frequency: f32,
        vertical_scaling: f32,
        vertical_offset: f32,
    }
}

rel_struct! {
    pub struct OneOverXSquared: BaseCurve {
        horizontal_scaling: f32,
    }
}

rel_struct! {
    pub struct DefaultDistanceAttenuation: BaseCurve {}
}

rel_struct! {
    pub struct DistanceAttenuationValueTable: BaseCurve {
        value: Vec<Value> = u16,
    }
}

rel_struct! {
    pub struct LeftHandPair {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct RightHandPair {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct Point {
        x: f32,
        y: f32,
    }
}

rel_struct! {
    pub struct Value {
        y: f32,
    }
}

rel_items! {
    pub enum CurveItem, class byte 0 {
        1 => Constant,
        2 => Linear,
        3 => LinearDb,
        4 => PiecewiseLinear,
        // ⚠ Padded to 20 bytes, its base's alignment, where a game object
        // pads only to its own fields'.
        5 => EqualPower padded,
        6 => ValueTable,
        7 => Exponential,
        8 => DecayingExponential,
        9 => DecayingSquaredExponential,
        10 => SineCurve,
        12 => OneOverXSquared,
        13 => DefaultDistanceAttenuation,
        15 => DistanceAttenuationValueTable,
    }
}
