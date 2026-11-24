//! GTA VI's water as one blended mesh about the camera, built again once the camera has gone far
//! from where it was built.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use or_io::gta6::water::WaterData;
use or_renderer::{ViewportRenderer, WorldBucket, WorldDraw, WorldInstance, WorldMesh};

use crate::state::gta6_world::worldview::water::mesh;

/// How far from the camera the water's quads are drawn, in metres.
const REACH: f32 = 3_000.0;
/// How far the camera goes before the mesh is built again, in metres.
const REBUILD: f32 = 500.0;
/// The colour the water's surface is drawn in; the opacity is each quad's own.
// ponytail: one colour for all water; the game's water shading reads maps a GTA VI install does not give the frame.
const TINT: [f32; 4] = [0.05, 0.18, 0.22, 1.0];

#[derive(Default)]
pub(super) struct Water {
    /// The mesh, the water it was built of and where the camera stood.
    built: Option<(Option<Arc<WorldMesh>>, usize, Vec3)>,
}

impl Water {
    /// Build the mesh again where the water arrived or the camera left where it was built; whether it was.
    pub(super) fn refresh(
        &mut self,
        renderer: &ViewportRenderer,
        device: &wgpu::Device,
        water: Option<&Arc<WaterData>>,
        eye: Vec3,
    ) -> bool {
        let Some(water) = water else {
            return self.built.take().is_some();
        };
        let of = Arc::as_ptr(water) as usize;
        if let Some((_, was, at)) = &self.built {
            if *was == of && at.distance(eye) < REBUILD {
                return false;
            }
        }
        let (vertices, indices) = mesh(&water.quads, eye, REACH);
        let drawn = (!indices.is_empty()).then(|| {
            let material = renderer.world_material(device, None, None, None, WorldBucket::Blend);
            Arc::new(WorldMesh::new(device, &vertices, &indices, material))
        });
        self.built = Some((drawn, of, eye));
        true
    }

    /// Add the water to a frame's instances and draws, after the `laid` ones.
    pub(super) fn draw(&self, laid: &mut Vec<WorldInstance>, draws: &mut Vec<WorldDraw>) {
        let Some((Some(mesh), _, _)) = &self.built else {
            return;
        };
        let at = laid.len() as u32;
        laid.push(WorldInstance::of(Mat4::IDENTITY, TINT));
        draws.push(WorldDraw {
            mesh: Arc::clone(mesh),
            instances: at..at + 1,
        });
    }
}
