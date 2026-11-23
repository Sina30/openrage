//! The part of a texture a mesh uses, and moving its texture coordinates into a smaller one.
//!
//! The first texture coordinates (`Tex0`) are the ones every sampler of a ped component's shader reads, the diffuse map and
//! the normal and specular maps alike, so one box serves them all.

use or_io::codec::mesh::GrcFvfChannel;

use crate::scene::Scene;

/// A rectangle of the texture, in the 0 to 1 of its coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvBox {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl UvBox {
    pub const UNIT: Self = Self {
        min: [0.0, 0.0],
        max: [1.0, 1.0],
    };

    pub fn size(&self) -> [f32; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }

    /// The share of the texture the box covers.
    pub fn covers(&self) -> f32 {
        let [w, h] = self.size();
        w * h
    }

    /// Whether the box lies inside the texture's own 0 to 1, which a texture that repeats does not.
    pub fn inside_unit(&self) -> bool {
        self.min.iter().all(|c| *c >= 0.0) && self.max.iter().all(|c| *c <= 1.0)
    }
}

/// Where the `Tex0` set sits among a primitive's, if it has one.
fn first_set(uv_channels: &[u8]) -> Option<usize> {
    uv_channels
        .iter()
        .position(|c| *c == GrcFvfChannel::Tex0 as u8)
}

/// The box the first texture coordinates of every mesh of `scene`, at every level of detail, fill; none when no mesh has any.
pub fn used(scene: &Scene) -> Option<UvBox> {
    let mut found: Option<UvBox> = None;
    for primitive in scene.primitives() {
        let Some(set) = first_set(&primitive.uv_channels) else {
            continue;
        };
        for uv in primitive.uvs.get(set).into_iter().flatten() {
            let b = found.get_or_insert(UvBox { min: *uv, max: *uv });
            for k in 0..2 {
                b.min[k] = b.min[k].min(uv[k]);
                b.max[k] = b.max[k].max(uv[k]);
            }
        }
    }
    found
}

/// Move the first texture coordinates of every mesh of `scene` from the box `from` into the box `to`: the corner of one lands on the corner of the other.
pub fn remap(scene: &mut Scene, from: UvBox, to: UvBox) {
    let (fs, ts) = (from.size(), to.size());
    for primitive in scene
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        let Some(set) = first_set(&primitive.uv_channels) else {
            continue;
        };
        for uv in &mut primitive.uvs[set] {
            for k in 0..2 {
                // A box with no width keeps what it has, rather than dividing by it.
                let along = if fs[k] > 0.0 {
                    (uv[k] - from.min[k]) / fs[k]
                } else {
                    0.0
                };
                uv[k] = to.min[k] + along * ts[k];
            }
        }
    }
}

/// How small a part of a texture a window may be: 1 over 2 to this power, since a texture of a hundred pixels has little left past that.
pub const SMALLEST_POWER: u32 = 7;

/// A part of a texture's width or height that is a power-of-two fraction of it, `index` of the `2^power` parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis {
    pub index: u32,
    pub power: u32,
}

impl Axis {
    /// The part that holds `lo` to `hi`, the smallest there is; none past the whole of the texture or smaller than [`SMALLEST_POWER`].
    fn holding(lo: f32, hi: f32) -> Option<Self> {
        if !(lo >= 0.0 && hi <= 1.0 && lo <= hi) {
            return None;
        }
        (0..=SMALLEST_POWER).rev().find_map(|power| {
            let parts = f64::from(1u32 << power);
            let index = (f64::from(lo) * parts).floor();
            (f64::from(hi) <= (index + 1.0) / parts).then_some(Self {
                index: index as u32,
                power,
            })
        })
    }

    /// Where the part begins, as a share of the whole.
    pub fn origin(&self) -> f32 {
        self.index as f32 / (1u32 << self.power) as f32
    }

    /// How much of the whole the part is.
    pub fn size(&self) -> f32 {
        1.0 / (1u32 << self.power) as f32
    }
}

/// A part of a texture, a power-of-two fraction of it on each axis and aligned to it: a crop to it is a copy of whole blocks at every level of the texture's mips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub x: Axis,
    pub y: Axis,
}

impl Window {
    /// The smallest window that holds `used`, or none where it is the whole texture or a part past what a crop keeps ([`SMALLEST_POWER`]).
    pub fn holding(used: &UvBox) -> Option<Self> {
        let window = Self {
            x: Axis::holding(used.min[0], used.max[0])?,
            y: Axis::holding(used.min[1], used.max[1])?,
        };
        (window.x.power + window.y.power > 0).then_some(window)
    }

    /// The share of the texture the window is.
    pub fn share(&self) -> f32 {
        self.x.size() * self.y.size()
    }

    pub fn bounds(&self) -> UvBox {
        UvBox {
            min: [self.x.origin(), self.y.origin()],
            max: [
                self.x.origin() + self.x.size(),
                self.y.origin() + self.y.size(),
            ],
        }
    }

    /// Move the first texture coordinates of every mesh of `scene` into the window's own 0 to 1.
    ///
    /// A power-of-two scale and an offset that is a multiple of it move a coordinate exactly or to within a last bit, so what lands
    /// a bit outside the unit square is held to its edge.
    pub fn apply(&self, scene: &mut Scene) {
        remap(scene, self.bounds(), UvBox::UNIT);
        for primitive in scene
            .objects
            .iter_mut()
            .flat_map(|o| o.meshes.iter_mut())
            .flat_map(|m| m.primitives.iter_mut())
        {
            if let Some(set) = first_set(&primitive.uv_channels) {
                for uv in &mut primitive.uvs[set] {
                    *uv = uv.map(|c| c.clamp(0.0, 1.0));
                }
            }
        }
    }
}
