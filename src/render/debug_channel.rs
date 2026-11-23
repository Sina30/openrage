//! Which vertex channel the viewport is showing, and how it becomes a colour.
//!
//! ⚠ **The list is built from the file, not from the format.** An FVF can
//! name 8 texcoord sets and 2 colour sets; a census of 1,075,524 shipped
//! declarations found `Tex6`, `Tex7` and `Tangent1` in **zero** buffers and
//! `Tex4`/`Tex5` in 89 each. A selector offering sixteen entries where the
//! asset has three is a worse surface than one offering three, so
//! [`offered_by`] asks the geometry.

use or_io::codec::mesh::{GrcFvfChannel, VertexLayout};

/// A channel the viewport can show, named as a user would say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DebugChannel {
    Normal,
    Tangent(u8),
    Colour(u8),
    Texcoord(u8),
    /// A GTA VI vertex's reference position, which no GTA V channel holds.
    Reference,
}

impl DebugChannel {
    pub fn channel(self) -> GrcFvfChannel {
        match self {
            Self::Normal => GrcFvfChannel::Normal,
            Self::Tangent(0) => GrcFvfChannel::Tangent0,
            Self::Tangent(_) => GrcFvfChannel::Tangent1,
            Self::Colour(0) => GrcFvfChannel::Diffuse,
            Self::Colour(_) => GrcFvfChannel::Specular,
            Self::Texcoord(n) => {
                GrcFvfChannel::ALL[GrcFvfChannel::Tex0 as usize + n.min(7) as usize]
            }
            Self::Reference => GrcFvfChannel::Position,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Normal => "Normals".to_string(),
            Self::Tangent(n) => format!("Tangent {n}"),
            Self::Colour(n) => format!("Colour {n}"),
            Self::Texcoord(n) => format!("Texcoord {n}"),
            Self::Reference => tr!("Reference position").to_string(),
        }
    }

    /// What the colour slot means for this channel, for the legend.
    pub fn legend(self) -> &'static str {
        match self {
            Self::Normal | Self::Tangent(_) => {
                "XYZ mapped to RGB through v * 0.5 + 0.5, so a zero axis reads mid-grey"
            }
            Self::Colour(_) => "the channel's own RGB, unlit",
            Self::Texcoord(_) => "U in red, V in green, blue zero; values outside 0..1 clamp",
            Self::Reference => tr!(
                "XYZ over the geometry's reference positions as RGB; black where a vertex holds none"
            ),
        }
    }

    /// Turn a channel's value into something visible.
    ///
    /// ⚠ Signed channels are remapped rather than clamped, because a normal
    /// pointing down the negative axis would otherwise be black and
    /// indistinguishable from one pointing nowhere.
    pub fn visualise(self, value: [f32; 4]) -> [f32; 4] {
        match self {
            Self::Normal | Self::Tangent(_) => [
                value[0] * 0.5 + 0.5,
                value[1] * 0.5 + 0.5,
                value[2] * 0.5 + 0.5,
                1.0,
            ],
            Self::Colour(_) | Self::Reference => [value[0], value[1], value[2], 1.0],
            Self::Texcoord(_) => [value[0].clamp(0.0, 1.0), value[1].clamp(0.0, 1.0), 0.0, 1.0],
        }
    }
}

/// Every channel present in at least one of these layouts, in menu order.
///
/// ⚠ The union across geometries rather than the intersection. A drawable
/// whose second geometry carries `Tex1` can still be asked about it, and a
/// geometry without the channel renders as absent rather than removing the
/// entry for the whole model.
pub fn offered_by<'a>(layouts: impl IntoIterator<Item = &'a VertexLayout>) -> Vec<DebugChannel> {
    let mut present = [false; GrcFvfChannel::COUNT];
    for layout in layouts {
        for (channel, _) in layout.present() {
            present[channel as usize] = true;
        }
    }

    let mut out = Vec::new();
    if present[GrcFvfChannel::Normal as usize] {
        out.push(DebugChannel::Normal);
    }
    for n in 0..2u8 {
        let c = DebugChannel::Colour(n);
        if present[c.channel() as usize] {
            out.push(c);
        }
    }
    for n in 0..8u8 {
        let c = DebugChannel::Texcoord(n);
        if present[c.channel() as usize] {
            out.push(c);
        }
    }
    for n in 0..2u8 {
        let c = DebugChannel::Tangent(n);
        if present[c.channel() as usize] {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::codec::mesh::GrcDataSize;

    fn layout(channels: &[(GrcFvfChannel, GrcDataSize)]) -> VertexLayout {
        let (mut fvf, mut channel_sizes) = (0u32, 0u64);
        for (channel, size) in channels {
            fvf |= channel.mask();
            channel_sizes |= (*size as u64) << (*channel as u64 * 4);
        }
        VertexLayout::of(fvf, channel_sizes)
    }

    #[test]
    fn the_selector_offers_what_the_file_has_and_nothing_else() {
        let typical = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
            (GrcFvfChannel::Diffuse, GrcDataSize::Color),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);
        assert_eq!(
            offered_by([&typical]),
            vec![
                DebugChannel::Normal,
                DebugChannel::Colour(0),
                DebugChannel::Texcoord(0),
            ]
        );

        // Position alone offers nothing, rather than offering everything.
        let bare = layout(&[(GrcFvfChannel::Position, GrcDataSize::Float3)]);
        assert!(offered_by([&bare]).is_empty());
    }

    #[test]
    fn a_channel_only_one_geometry_carries_is_still_offered() {
        let first = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);
        let second = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Tex1, GrcDataSize::Float2),
            (GrcFvfChannel::Tangent0, GrcDataSize::Float4),
        ]);
        assert_eq!(
            offered_by([&first, &second]),
            vec![
                DebugChannel::Texcoord(0),
                DebugChannel::Texcoord(1),
                DebugChannel::Tangent(0),
            ]
        );
    }

    #[test]
    fn no_two_entries_name_the_same_channel() {
        let every = (0..8u8)
            .map(DebugChannel::Texcoord)
            .chain((0..2).map(DebugChannel::Colour))
            .chain((0..2).map(DebugChannel::Tangent))
            .chain([DebugChannel::Normal]);
        let mut seen = std::collections::BTreeSet::new();
        for entry in every {
            assert!(
                seen.insert(entry.channel() as u8),
                "{} repeats a channel",
                entry.label()
            );
        }
        assert_eq!(seen.len(), 13);
    }

    /// ⚠ A normal pointing down an axis must not render black, or it is
    /// indistinguishable from a normal that is not there.
    #[test]
    fn a_negative_axis_is_visible_rather_than_black() {
        let down = DebugChannel::Normal.visualise([0.0, 0.0, -1.0, 1.0]);
        assert_eq!(down, [0.5, 0.5, 0.0, 1.0]);
        let zero = DebugChannel::Normal.visualise([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(zero, [0.5, 0.5, 0.5, 1.0]);
        assert_ne!(down, zero);

        // A texcoord outside the unit square clamps rather than wrapping,
        // so tiled UVs read as saturated instead of as a false gradient.
        let tiled = DebugChannel::Texcoord(0).visualise([3.5, -2.0, 0.0, 1.0]);
        assert_eq!(tiled, [1.0, 0.0, 0.0, 1.0]);
    }
}
