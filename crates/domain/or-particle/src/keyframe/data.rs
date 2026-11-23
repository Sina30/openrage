use or_io::gta5::particle::{self as ptx, RawKeyframeEntry};
use or_io::gta5::Vec4V;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyframeProp {
    PositionX,
    PositionY,
    PositionZ,
    ColourR,
    ColourG,
    ColourB,
    ColourA,
    Size,
    Rotation,
    SpawnRate,
    Custom(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Interpolation {
    Step,
    #[default]
    Linear,
    Smooth,
    Bezier,
}

impl Interpolation {
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Step,
            1 => Self::Linear,
            2 => Self::Smooth,
            3 => Self::Bezier,
            _ => Self::Linear,
        }
    }

    pub fn to_id(self) -> u32 {
        match self {
            Self::Step => 0,
            Self::Linear => 1,
            Self::Smooth => 2,
            Self::Bezier => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: f32,

    pub value: f32,

    pub interpolation: Interpolation,

    pub tangent_in: Option<f32>,
    pub tangent_out: Option<f32>,
}

impl Keyframe {
    pub fn new(time: f32, value: f32) -> Self {
        Self {
            time,
            value,
            interpolation: Interpolation::Linear,
            tangent_in: None,
            tangent_out: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyframePropData {
    pub entries: Vec<RawKeyframeEntry>,
    pub property_id: u32,
    pub rand_index: i8,
    pub invert_bias_link: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyframeData {
    pub property: Option<KeyframeProp>,

    pub keyframes: Vec<Keyframe>,

    pub entries: Vec<RawKeyframeEntry>,

    pub looping: bool,
}

impl KeyframeData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn constant(value: f32) -> Self {
        Self {
            property: None,
            keyframes: vec![Keyframe::new(0.0, value)],
            entries: Vec::new(),
            looping: false,
        }
    }

    pub fn from_raw_entries(entries: Vec<RawKeyframeEntry>) -> Self {
        Self {
            property: None,
            keyframes: Vec::new(),
            entries,
            looping: false,
        }
    }

    pub fn evaluate(&self, time: f32) -> f32 {
        if self.keyframes.is_empty() {
            return 0.0;
        }

        if self.keyframes.len() == 1 {
            return self.keyframes[0].value;
        }

        let time = if self.looping {
            time.rem_euclid(1.0)
        } else {
            time.clamp(0.0, 1.0)
        };

        let mut prev = &self.keyframes[0];
        let mut next = &self.keyframes[0];

        for kf in &self.keyframes {
            if kf.time <= time {
                prev = kf;
            }
            if kf.time >= time && next.time < time {
                next = kf;
                break;
            }
            next = kf;
        }

        if prev.time >= next.time {
            return prev.value;
        }

        let t = (time - prev.time) / (next.time - prev.time);

        match prev.interpolation {
            Interpolation::Step => prev.value,
            Interpolation::Linear => lerp(prev.value, next.value, t),
            Interpolation::Smooth => lerp(prev.value, next.value, smoothstep(t)),
            Interpolation::Bezier => lerp(prev.value, next.value, smoothstep(t)),
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl KeyframePropData {
    /// A keyframed value's keys, and what it states besides them; `None` for one with no key.
    pub fn of(k: &ptx::PtxKeyframeProp) -> Option<Self> {
        (!k.keyframe.entries.is_empty()).then(|| Self {
            entries: k.keyframe.entries.iter().map(raw_entry).collect(),
            property_id: k.property_id.get(),
            rand_index: k.rand_index,
            invert_bias_link: k.invert_bias_link.raw(),
        })
    }
}

/// One key as the editor holds it.
pub(crate) fn raw_entry(e: &ptx::PtxKeyframeEntry) -> RawKeyframeEntry {
    let lanes = |v: &Vec4V| [v.x, v.y, v.z, v.w];
    RawKeyframeEntry {
        time: lanes(&e.time),
        value: lanes(&e.value),
    }
}

/// `slot`'s keys set to `raw`, each entry kept where one was.
pub(crate) fn set_entries(slot: &mut ptx::PtxKeyframe, raw: &[RawKeyframeEntry]) {
    let set = |v: &mut Vec4V, [x, y, z, w]: [f32; 4]| (v.x, v.y, v.z, v.w) = (x, y, z, w);
    slot.entries.resize_with(raw.len(), Default::default);
    for (e, r) in slot.entries.iter_mut().zip(raw) {
        set(&mut e.time, r.time);
        set(&mut e.value, r.value);
    }
}

/// A keyframed value that holds `value` at every time.
pub fn constant([x, y, z, w]: [f32; 4]) -> ptx::PtxKeyframeProp {
    let mut k = ptx::PtxKeyframeProp::default();
    let mut e = ptx::PtxKeyframeEntry::default();
    (e.value.x, e.value.y, e.value.z, e.value.w) = (x, y, z, w);
    k.keyframe.entries.push(e);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_keyframe() {
        let data = KeyframeData::constant(5.0);
        assert_eq!(data.evaluate(0.0), 5.0);
        assert_eq!(data.evaluate(0.5), 5.0);
        assert_eq!(data.evaluate(1.0), 5.0);
    }

    #[test]
    fn test_linear_interpolation() {
        let data = KeyframeData {
            property: None,
            keyframes: vec![Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 10.0)],
            entries: Vec::new(),
            looping: false,
        };

        assert_eq!(data.evaluate(0.0), 0.0);
        assert_eq!(data.evaluate(0.5), 5.0);
        assert_eq!(data.evaluate(1.0), 10.0);
    }
}
