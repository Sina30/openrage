//! Particle sprites drawn with a pass of the pack's `ptfx_sprite`, whose vertex stage reads which corner it is once per vertex and everything else once per sprite, so a sprite is one instance of a six-vertex quad.

use glam::Vec3;
use or_io::gta5::shaders::spirv_pack::SignatureElement;

use super::effect_pass::{EffectCache, EffectError, EffectKind, EffectPass, Layout, Shape};
use super::engine::EngineConstants;
use super::pack::GamePack;
use super::pipeline::Target;

/// The effect every sprite draws with.
pub const EFFECT: &str = "ptfx_sprite";

/// The one input the vertex stage reads once per vertex: which corner of the sprite it is.
const CORNER: (&str, u32) = ("TEXCOORD", 3);

/// The two triangles of a sprite, each vertex two indices into the vertex stage's table of corner parameters, whose ends are 0 and 2: bottom left, bottom right, top right, then bottom left, top right, top left.
const CORNERS: [[f32; 2]; 6] = [
    [0.0, 0.0],
    [2.0, 0.0],
    [2.0, 2.0],
    [0.0, 0.0],
    [2.0, 2.0],
    [0.0, 2.0],
];

/// The whole of a texture, as the cell of an atlas of one.
pub const WHOLE_TEXTURE: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

/// One sprite, as the vertex stage reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    pub centre: Vec3,
    /// From the centre to the middle of the right side.
    pub right: Vec3,
    /// From the centre to the middle of the top side.
    pub up: Vec3,
    pub colour: [f32; 4],
    /// The cell of the atlas each of the two frames is, as `[u_min, u_max, v_min, v_max]` with `v_min` the top.
    pub frame_a: [f32; 4],
    pub frame_b: [f32; 4],
    /// How much of the second frame is mixed over the first, from nought to one.
    pub frame_blend: f32,
    /// How much the colour is brightened by its own square.
    pub glow: f32,
}

impl Sprite {
    /// A sprite of the whole texture, with no second frame and no glow.
    pub fn new(centre: Vec3, right: Vec3, up: Vec3, colour: [f32; 4]) -> Self {
        Self {
            centre,
            right,
            up,
            colour,
            frame_a: WHOLE_TEXTURE,
            frame_b: WHOLE_TEXTURE,
            frame_blend: 0.0,
            glow: 0.0,
        }
    }
}

/// What the vertex stage reads once per vertex, by the semantic and index of the input.
pub fn corner_value(semantic: &str, index: u32, corner: [f32; 2]) -> [f32; 4] {
    if (semantic, index) == CORNER {
        [corner[0], corner[1], 0.0, 0.0]
    } else {
        [0.0; 4]
    }
}

/// What a sprite input the vertex stage reads once per sprite is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Position,
    Colour,
    BlendGlow,
    Ones,
    FirstEdge,
    SecondEdge,
    ThirdEdge,
    Corners,
    FrameA,
    FrameB,
}

/// The field a sprite input of this semantic and index is made of; none for an input the stage does not use.
fn field_of(semantic: &str, index: u32) -> Option<Field> {
    Some(match (semantic, index) {
        ("POSITION", 0) => Field::Position,
        ("COLOR", 0) => Field::Colour,
        ("TEXCOORD", 0) => Field::BlendGlow,
        ("TEXCOORD", 1) => Field::Ones,
        ("TEXCOORD", 6) => Field::FirstEdge,
        ("TEXCOORD", 7) => Field::SecondEdge,
        ("TEXCOORD", 8) => Field::ThirdEdge,
        ("TEXCOORD", 9) => Field::Corners,
        ("TEXCOORD", 10) => Field::FrameA,
        ("TEXCOORD", 11) => Field::FrameB,
        _ => return None,
    })
}

/// The four numbers of a field of a sprite.
///
/// The stage puts a corner at `centre + a + w + (c - w) * s + (d - a) * t`, with `a`, `c` and `d` the xyz of `TEXCOORD7`, `TEXCOORD8` and `TEXCOORD6`, `w` their three w, and `s` and `t` the corner's parameters, which `TEXCOORD9` gives for its two ends (nought and one, the first two for `s` and the last two for `t`). So `d = up`, `a = -up`, `c = right` and `w = -right` put the corners at `centre ± right ± up`. A frame's texture coordinate is a corner's parameters mixed over its cell, `TEXCOORD10` and `TEXCOORD11` holding `[u_min, u_max, v_min, v_max]` of the first and second.
fn field_value(field: Field, sprite: &Sprite) -> [f32; 4] {
    let (r, u, c) = (sprite.right, sprite.up, sprite.centre);
    match field {
        Field::Position => [c.x, c.y, c.z, 1.0],
        Field::Colour => sprite.colour,
        Field::BlendGlow => [sprite.frame_blend, sprite.glow, 0.0, 0.0],
        Field::Ones => [1.0; 4],
        Field::FirstEdge => [u.x, u.y, u.z, -r.x],
        Field::SecondEdge => [-u.x, -u.y, -u.z, -r.y],
        Field::ThirdEdge => [r.x, r.y, r.z, -r.z],
        Field::Corners => [0.0, 1.0, 0.0, 1.0],
        Field::FrameA => sprite.frame_a,
        Field::FrameB => sprite.frame_b,
    }
}

/// What the vertex stage reads once per sprite, by the semantic and index of the input.
pub fn sprite_value(semantic: &str, index: u32, sprite: &Sprite) -> [f32; 4] {
    field_of(semantic, index).map_or([0.0; 4], |field| field_value(field, sprite))
}

fn reads_once_per_sprite(input: &SignatureElement) -> bool {
    (input.semantic.as_str(), input.index) != CORNER
}

const SHAPE: Shape = Shape {
    effect: EFFECT,
    topology: wgpu::PrimitiveTopology::TriangleList,
    per_instance: reads_once_per_sprite,
};

/// Sprites of one kind made ready to draw: the pipeline built and the resources bound once, so that a frame costs a write of the constants and the sprites and a draw into a pass its caller owns.
pub struct SpritePass {
    effect: EffectPass,
    layout: Layout<Field>,
    /// The bytes of the last frame's sprites, written over by the next, so that a frame allocates nothing.
    scratch: Vec<u8>,
    count: u32,
}

impl SpritePass {
    /// Set what the particle rule's shader variables hold, by the hash of each one's name.
    pub fn set_rule_values(&mut self, rule: &[(u32, [f32; 4])]) {
        self.effect.set_rule_values(rule);
    }

    /// The GPU bytes this pass holds.
    pub fn resident_bytes(&self) -> crate::viewport::ResidentBytes {
        self.effect.resident_bytes()
    }

    /// Prepare sprites of `kind` to draw into `target`, a pass of one colour attachment and the depth the target says.
    pub fn prepare(
        cache: &mut EffectCache,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &GamePack,
        target: &Target,
        kind: &EffectKind,
    ) -> Result<Self, EffectError> {
        let mut effect = EffectPass::prepare(cache, device, queue, pack, target, &SHAPE, kind)?;
        let streams = effect.streams();
        let corners = Layout::new(
            &streams.vertex,
            streams.vertex_stride,
            effect.inputs(),
            |semantic, index| ((semantic, index) == CORNER).then_some(()),
        );
        let layout = Layout::new(
            &streams.instance,
            streams.instance_stride,
            effect.inputs(),
            field_of,
        );
        let mut bytes = Vec::new();
        corners.write(&mut bytes, CORNERS.len(), |record, ()| {
            [CORNERS[record][0], CORNERS[record][1], 0.0, 0.0]
        });
        effect.set_vertices(device, queue, &bytes);
        Ok(Self {
            effect,
            layout,
            scratch: Vec::new(),
            count: 0,
        })
    }

    /// Write the constants of this frame, what each effect declares and the kind's over it and the scene's over those, and the sprites to draw.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &EngineConstants,
        sprites: &[Sprite],
    ) {
        self.effect.write_constants(queue, scene);
        self.scratch.clear();
        self.layout
            .write(&mut self.scratch, sprites.len(), |record, field| {
                field_value(field, &sprites[record])
            });
        self.effect.set_instances(device, queue, &self.scratch);
        self.count = u32::try_from(sprites.len()).unwrap_or(u32::MAX);
    }

    /// How many sprites the last update left to draw.
    pub fn drawn(&self) -> u32 {
        self.count
    }

    /// Draw the sprites into `pass`, which must have the colour and depth formats this was prepared for.
    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }
        self.effect
            .record(pass, 0..CORNERS.len() as u32, 0..self.count);
    }
}

#[cfg(test)]
mod tests;
