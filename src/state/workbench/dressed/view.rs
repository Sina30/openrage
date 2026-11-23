//! How the dressed ped is seen: the view mode of its 3D view and the words that go with each.

use or_renderer::ShadingMode;

use crate::render::debug_channel::DebugChannel;

/// What the 3D view of the dressed ped draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// The shape, lit, without textures.
    Solid,
    /// The textures of the garments: where a dressed ped starts.
    #[default]
    Material,
    /// The game's own shaders and passes, lit as it lights.
    Shaded,
    /// The edges of every triangle, no surfaces.
    Wireframe,
    /// The normal of each vertex as a colour.
    Normals,
    /// The first texture coordinates of each vertex as a colour.
    Texcoords,
    /// The first tangent of each vertex as a colour.
    Tangents,
    /// The first vertex colour of each vertex, unlit.
    VertexColours,
}

impl ViewMode {
    pub const ALL: [ViewMode; 8] = [
        ViewMode::Solid,
        ViewMode::Material,
        ViewMode::Shaded,
        ViewMode::Wireframe,
        ViewMode::Normals,
        ViewMode::Texcoords,
        ViewMode::Tangents,
        ViewMode::VertexColours,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ViewMode::Solid => tr!("Solid"),
            ViewMode::Material => tr!("Material"),
            ViewMode::Shaded => tr!("Shaded"),
            ViewMode::Wireframe => tr!("Wireframe"),
            ViewMode::Normals => tr!("Normals"),
            ViewMode::Texcoords => tr!("UVs"),
            ViewMode::Tangents => tr!("Tangents"),
            ViewMode::VertexColours => tr!("Vertex colours"),
        }
    }

    /// What the mode draws, in a few words for under its name in the Draw menu.
    pub fn note(self) -> &'static str {
        match self {
            ViewMode::Solid => tr!("The shape, lit, with no textures"),
            ViewMode::Material => tr!("With the textures of the garments"),
            ViewMode::Shaded => tr!("As the game lights it, with the game's shaders"),
            ViewMode::Wireframe => tr!("The edges of every triangle"),
            ViewMode::Normals => tr!("Which way each surface faces, as colours"),
            ViewMode::Texcoords => tr!("How each texture is laid on, as colours"),
            ViewMode::Tangents => tr!("The tangent of each vertex, as colours"),
            ViewMode::VertexColours => tr!("The colour each vertex carries"),
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            ViewMode::Solid => tr!("The shape, lit, without textures"),
            ViewMode::Material => tr!("With the textures of the garments"),
            ViewMode::Shaded => tr!("Drawn with the game's own shaders, as the game lights it; needs the baked shaders"),
            ViewMode::Wireframe => tr!("The edges of every triangle, no surfaces"),
            ViewMode::Normals => tr!("The normal of each vertex as a colour: x, y and z as red, green and blue, a zero axis mid-grey"),
            ViewMode::Texcoords => tr!("The first texture coordinates of each vertex as a colour: u in red, v in green; a coordinate past 1 saturates"),
            ViewMode::Tangents => tr!("The first tangent of each vertex as a colour: x, y and z as red, green and blue; a mesh that has none is grey"),
            ViewMode::VertexColours => tr!("The first colour each vertex carries, unlit and not tinted; a mesh that has none is grey"),
        }
    }

    /// The mode of the viewport that draws it.
    pub fn shading(self) -> ShadingMode {
        match self {
            ViewMode::Solid => ShadingMode::Solid,
            ViewMode::Material => ShadingMode::Material,
            ViewMode::Shaded => ShadingMode::Shaded,
            ViewMode::Wireframe => ShadingMode::Wireframe,
            ViewMode::Normals
            | ViewMode::Texcoords
            | ViewMode::Tangents
            | ViewMode::VertexColours => ShadingMode::Channel,
        }
    }

    /// The channel whose colours the vertices carry in this mode.
    pub fn channel(self) -> Option<DebugChannel> {
        match self {
            ViewMode::Normals => Some(DebugChannel::Normal),
            ViewMode::Texcoords => Some(DebugChannel::Texcoord(0)),
            ViewMode::Tangents => Some(DebugChannel::Tangent(0)),
            ViewMode::VertexColours => Some(DebugChannel::Colour(0)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_is_a_mode_of_the_viewport_and_only_the_four_vertex_data_views_carry_a_channel() {
        let shading: Vec<ShadingMode> = ViewMode::ALL.iter().map(|m| m.shading()).collect();
        assert_eq!(
            shading,
            [
                ShadingMode::Solid,
                ShadingMode::Material,
                ShadingMode::Shaded,
                ShadingMode::Wireframe,
                ShadingMode::Channel,
                ShadingMode::Channel,
                ShadingMode::Channel,
                ShadingMode::Channel
            ]
        );
        let channels: Vec<bool> = ViewMode::ALL
            .iter()
            .map(|m| m.channel().is_some())
            .collect();
        assert_eq!(
            channels,
            [false, false, false, false, true, true, true, true]
        );
        let mut shown: Vec<_> = ViewMode::ALL.iter().filter_map(|m| m.channel()).collect();
        shown.dedup();
        assert_eq!(shown.len(), 4, "each shows a channel of its own");
        assert_eq!(ViewMode::default(), ViewMode::Material);
        let mut names: Vec<&str> = ViewMode::ALL.iter().map(|m| m.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            ViewMode::ALL.len(),
            "each mode has a name of its own"
        );
    }
}
