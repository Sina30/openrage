use serde::{Deserialize, Serialize};

use or_io::gta5::particle::{Axis, RawKeyframeEntry};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    pub fn unit_x() -> Self {
        Self::new(1.0, 0.0, 0.0)
    }

    pub fn unit_y() -> Self {
        Self::new(0.0, 1.0, 0.0)
    }

    pub fn unit_z() -> Self {
        Self::new(0.0, 0.0, 1.0)
    }
}

pub type DomainKeyframes = Vec<RawKeyframeEntry>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DomainKeyframeData {
    pub position: Option<DomainKeyframes>,
    pub rotation: Option<DomainKeyframes>,
    pub size_outer: Option<DomainKeyframes>,
    pub size_inner: Option<DomainKeyframes>,
}

impl DomainKeyframeData {
    pub fn has_any(&self) -> bool {
        self.position.is_some()
            || self.rotation.is_some()
            || self.size_outer.is_some()
            || self.size_inner.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Domain {
    Point(Vec3),

    Box {
        min: Vec3,
        max: Vec3,
    },

    Sphere {
        center: Vec3,
        radius_inner: f32,
        radius_outer: f32,
    },

    Cylinder {
        center: Vec3,
        radius: f32,
        height: f32,
        axis: Axis,
    },

    Disc {
        center: Vec3,
        radius_inner: f32,
        radius_outer: f32,
        normal: Vec3,
    },

    Line {
        start: Vec3,
        end: Vec3,
    },

    Ring {
        center: Vec3,
        radius_inner: f32,
        radius_outer: f32,
        normal: Vec3,
    },

    Attractor {
        position: Vec3,
        strength: f32,
    },
}

impl Default for Domain {
    fn default() -> Self {
        Self::Point(Vec3::zero())
    }
}

impl Domain {
    pub fn type_id(&self) -> u32 {
        match self {
            Self::Point(_) => 0,
            Self::Box { .. } => 1,
            Self::Sphere { .. } => 2,
            Self::Cylinder { .. } => 3,
            Self::Disc { .. } => 4,
            Self::Line { .. } => 5,
            Self::Ring { .. } => 6,
            Self::Attractor { .. } => 7,
        }
    }
}
