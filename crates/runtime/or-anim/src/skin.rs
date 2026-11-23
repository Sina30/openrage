// Binding → matrix_palette slot → skeleton bone → skin matrix (empty palette = rigid).
use glam::{Mat4, Vec3};
use or_io::codec::mesh::{
    decode, GrcDataSize, GrcFvfChannel, VertexLayout, VertexReader, VertexSource,
};
pub const MAX_INFLUENCES: usize = 8;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexInfluences {
    pub bones: [u16; MAX_INFLUENCES],
    pub weights: [f32; MAX_INFLUENCES],
    pub count: u8,
}

impl Default for VertexInfluences {
    fn default() -> Self {
        Self::UNBOUND
    }
}

impl VertexInfluences {
    pub const UNBOUND: Self = Self {
        bones: [0; MAX_INFLUENCES],
        weights: [0.0; MAX_INFLUENCES],
        count: 0,
    };

    pub fn rigid(bone: u16) -> Self {
        let mut out = Self::UNBOUND;
        out.bones[0] = bone;
        out.weights[0] = 1.0;
        out.count = 1;
        out
    }

    pub fn is_bound(&self) -> bool {
        self.count > 0
    }

    pub fn blended_matrix(&self, skin: &[Mat4]) -> Mat4 {
        if self.count == 0 {
            return Mat4::IDENTITY;
        }
        let mut acc = Mat4::ZERO;
        let mut total = 0.0f32;
        for i in 0..self.count as usize {
            let weight = self.weights[i];
            if weight == 0.0 {
                continue;
            }
            let Some(matrix) = skin.get(self.bones[i] as usize) else {
                continue;
            };
            acc += *matrix * weight;
            total += weight;
        }
        if total <= 1e-6 {
            Mat4::IDENTITY
        } else if (total - 1.0).abs() > 1e-4 {
            acc * (1.0 / total)
        } else {
            acc
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkinnedMesh {
    pub influences: Vec<VertexInfluences>,
    pub bind_positions: Vec<[f32; 3]>,
    pub bind_normals: Vec<[f32; 3]>,
    pub bones_used: Vec<u16>,
    pub out_of_range_influences: usize,
    pub degenerate_weight_vertices: usize,
}

impl SkinnedMesh {
    pub fn vertex_count(&self) -> usize {
        self.influences.len()
    }

    pub fn is_empty(&self) -> bool {
        self.influences.is_empty()
    }

    pub fn skin_into(
        &self,
        skin: &[Mat4],
        positions: &mut Vec<[f32; 3]>,
        normals: &mut Vec<[f32; 3]>,
    ) {
        let count = self.influences.len();
        positions.clear();
        positions.reserve(count);
        normals.clear();
        normals.reserve(count);

        for i in 0..count {
            let influences = self.influences[i];
            let bind_pos = self.bind_positions.get(i).copied().unwrap_or([0.0; 3]);
            let bind_normal = self.bind_normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);

            if !influences.is_bound() {
                positions.push(bind_pos);
                normals.push(bind_normal);
                continue;
            }

            let matrix = influences.blended_matrix(skin);
            let p = matrix.transform_point3(Vec3::from(bind_pos));
            let n = matrix.transform_vector3(Vec3::from(bind_normal));
            positions.push(p.to_array());
            normals.push(n.to_array());
        }
    }

    pub fn has_anomalies(&self) -> bool {
        self.out_of_range_influences > 0 || self.degenerate_weight_vertices > 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkinChannelOffsets {
    pub position: Option<usize>,
    pub normal: Option<usize>,
    pub weight: Option<usize>,
    pub binding: Option<usize>,
}

impl SkinChannelOffsets {
    pub fn has_influences(&self) -> bool {
        self.weight.is_some() && self.binding.is_some()
    }
}

pub fn skin_channel_offsets(layout: &VertexLayout) -> SkinChannelOffsets {
    let at = |channel| layout.slot(channel).map(|s| s.offset);
    SkinChannelOffsets {
        position: at(GrcFvfChannel::Position),
        normal: at(GrcFvfChannel::Normal),
        weight: at(GrcFvfChannel::Weight),
        binding: at(GrcFvfChannel::Binding),
    }
}

/// A geometry's influences through its matrix palette; `None` where the palette is empty.
pub fn decode_skinning<S: VertexSource + ?Sized>(
    vertices: &S,
    palette: &[u16],
) -> Option<SkinnedMesh> {
    if palette.is_empty() {
        return None;
    }
    let reader = VertexReader::new(vertices)?;
    if reader.stride() == 0 {
        return None;
    }
    let layout = reader.layout();
    if !skin_channel_offsets(layout).has_influences() {
        return None;
    }
    let size = |channel| layout.slot(channel).map(|s| s.size);
    if size(GrcFvfChannel::Position)? != GrcDataSize::Float3 {
        return None;
    }
    let weight_size = size(GrcFvfChannel::Weight)?;
    let binding_size = size(GrcFvfChannel::Binding)?;
    let normal_size = size(GrcFvfChannel::Normal);

    let vertex_count = reader.count();

    let mut mesh = SkinnedMesh {
        influences: Vec::with_capacity(vertex_count),
        bind_positions: Vec::with_capacity(vertex_count),
        bind_normals: Vec::with_capacity(vertex_count),
        ..SkinnedMesh::default()
    };
    let mut used = std::collections::BTreeSet::new();
    let channel = |channel, i| reader.raw(channel, i).unwrap_or(&[]);
    let xyz = |size, bytes| decode(size, bytes).map(|v| [v[0], v[1], v[2]]);

    for i in 0..vertex_count {
        let position = channel(GrcFvfChannel::Position, i);
        mesh.bind_positions
            .push(xyz(GrcDataSize::Float3, position).unwrap_or([0.0; 3]));
        let normal = channel(GrcFvfChannel::Normal, i);
        mesh.bind_normals.push(match normal_size {
            Some(GrcDataSize::Float3) => xyz(GrcDataSize::Float3, normal).unwrap_or([0.0; 3]),
            Some(GrcDataSize::PackedNormal) => {
                xyz(GrcDataSize::PackedNormal, normal).unwrap_or([0.0, 1.0, 0.0])
            }
            _ => [0.0, 1.0, 0.0],
        });

        let raw_weights = read_four_bytes(channel(GrcFvfChannel::Weight, i), 0, weight_size);
        let raw_slots = read_four_bytes(channel(GrcFvfChannel::Binding, i), 0, binding_size);

        let mut influences = VertexInfluences::UNBOUND;
        let mut slot_out = 0usize;
        let mut weight_total = 0u32;

        for lane in 0..4 {
            let weight = raw_weights[lane];
            if weight == 0 {
                continue;
            }
            weight_total += u32::from(weight);
            let palette_slot = raw_slots[lane] as usize;
            let Some(&bone) = palette.get(palette_slot) else {
                mesh.out_of_range_influences += 1;
                continue;
            };
            influences.bones[slot_out] = bone;
            influences.weights[slot_out] = f32::from(weight);
            used.insert(bone);
            slot_out += 1;
        }
        influences.count = slot_out as u8;

        if slot_out == 0 {
            mesh.degenerate_weight_vertices += 1;
            if let Some(&bone) = palette.get(raw_slots[0] as usize) {
                influences = VertexInfluences::rigid(bone);
                used.insert(bone);
            }
        } else if weight_total > 0 {
            let inv = 1.0 / weight_total as f32;
            for weight in influences.weights.iter_mut().take(slot_out) {
                *weight *= inv;
            }
        }

        mesh.influences.push(influences);
    }

    mesh.bones_used = used.into_iter().collect();
    Some(mesh)
}

fn read_four_bytes(data: &[u8], offset: usize, size: GrcDataSize) -> [u8; 4] {
    match size {
        GrcDataSize::UByte4 | GrcDataSize::Color => {
            let mut out = [0u8; 4];
            for (lane, slot) in out.iter_mut().enumerate() {
                *slot = data.get(offset + lane).copied().unwrap_or(0);
            }
            out
        }
        GrcDataSize::Short4 => {
            let mut out = [0u8; 4];
            for (lane, slot) in out.iter_mut().enumerate() {
                let base = offset + lane * 2;
                let raw = u16::from_le_bytes([
                    data.get(base).copied().unwrap_or(0),
                    data.get(base + 1).copied().unwrap_or(0),
                ]);
                *slot = (raw >> 8) as u8;
            }
            out
        }
        _ => [0u8; 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(channels: &[(GrcFvfChannel, GrcDataSize)]) -> VertexLayout {
        let (mut fvf, mut sizes) = (0u32, 0u64);
        for &(channel, size) in channels {
            fvf |= channel.mask();
            sizes |= (size as u64) << (channel as u64 * 4);
        }
        VertexLayout::of(fvf, sizes)
    }

    fn skin_matrices(count: usize) -> Vec<Mat4> {
        (0..count)
            .map(|i| Mat4::from_translation(Vec3::new(i as f32 * 10.0, 0.0, 0.0)))
            .collect()
    }

    #[test]
    fn rigid_binding_takes_its_bone_matrix_whole() {
        let influences = VertexInfluences::rigid(2);
        let matrix = influences.blended_matrix(&skin_matrices(4));
        assert_eq!(
            matrix.transform_point3(Vec3::ZERO),
            Vec3::new(20.0, 0.0, 0.0)
        );
    }

    #[test]
    fn two_equal_influences_blend_to_the_midpoint() {
        let influences = VertexInfluences {
            bones: [0, 2, 0, 0, 0, 0, 0, 0],
            weights: [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            count: 2,
        };
        let matrix = influences.blended_matrix(&skin_matrices(4));
        assert_eq!(
            matrix.transform_point3(Vec3::ZERO),
            Vec3::new(10.0, 0.0, 0.0)
        );
    }

    #[test]
    fn unbound_vertices_get_the_identity() {
        let matrix = VertexInfluences::UNBOUND.blended_matrix(&skin_matrices(4));
        assert_eq!(matrix, Mat4::IDENTITY);
    }

    #[test]
    fn a_missing_bone_matrix_is_renormalised_away() {
        let influences = VertexInfluences {
            bones: [2, 9, 0, 0, 0, 0, 0, 0],
            weights: [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            count: 2,
        };
        let matrix = influences.blended_matrix(&skin_matrices(4));
        assert_eq!(
            matrix.transform_point3(Vec3::ZERO),
            Vec3::new(20.0, 0.0, 0.0)
        );
    }

    #[test]
    fn all_weights_zero_yields_the_identity_not_the_origin() {
        let influences = VertexInfluences {
            bones: [1, 2, 0, 0, 0, 0, 0, 0],
            weights: [0.0; MAX_INFLUENCES],
            count: 2,
        };
        assert_eq!(influences.blended_matrix(&skin_matrices(4)), Mat4::IDENTITY);
    }

    #[test]
    fn skinning_transforms_positions_and_rotates_normals_without_translating_them() {
        let mesh = SkinnedMesh {
            influences: vec![VertexInfluences::rigid(0)],
            bind_positions: vec![[1.0, 0.0, 0.0]],
            bind_normals: vec![[0.0, 1.0, 0.0]],
            ..SkinnedMesh::default()
        };
        let skin = vec![
            Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0))
                * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2),
        ];
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        mesh.skin_into(&skin, &mut positions, &mut normals);

        let p = Vec3::from(positions[0]);
        assert!((p - Vec3::new(5.0, 1.0, 0.0)).length() < 1e-5, "p = {p}");
        let n = Vec3::from(normals[0]);
        assert!((n - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5, "n = {n}");
    }

    #[test]
    fn skinning_reuses_output_buffers() {
        let mesh = SkinnedMesh {
            influences: vec![VertexInfluences::rigid(0); 3],
            bind_positions: vec![[0.0; 3]; 3],
            bind_normals: vec![[0.0, 1.0, 0.0]; 3],
            ..SkinnedMesh::default()
        };
        let skin = skin_matrices(1);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        mesh.skin_into(&skin, &mut positions, &mut normals);
        mesh.skin_into(&skin, &mut positions, &mut normals);
        assert_eq!(positions.len(), 3);
        assert_eq!(normals.len(), 3);
    }

    #[test]
    fn skinning_from_bind_is_idempotent() {
        let mesh = SkinnedMesh {
            influences: vec![VertexInfluences::rigid(1)],
            bind_positions: vec![[1.0, 2.0, 3.0]],
            bind_normals: vec![[0.0, 1.0, 0.0]],
            ..SkinnedMesh::default()
        };
        let skin = skin_matrices(2);
        let mut a = Vec::new();
        let mut an = Vec::new();
        mesh.skin_into(&skin, &mut a, &mut an);
        let mut b = Vec::new();
        let mut bn = Vec::new();
        mesh.skin_into(&skin, &mut b, &mut bn);
        assert_eq!(a, b);
    }

    #[test]
    fn channel_offsets_follow_declaration_order() {
        let fvf = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Weight, GrcDataSize::Color),
            (GrcFvfChannel::Binding, GrcDataSize::UByte4),
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
            (GrcFvfChannel::Tex0, GrcDataSize::Half2),
        ]);

        let offsets = skin_channel_offsets(&fvf);
        assert_eq!(offsets.position, Some(0));
        assert_eq!(offsets.weight, Some(12));
        assert_eq!(offsets.binding, Some(16));
        assert_eq!(offsets.normal, Some(20));
        assert!(offsets.has_influences());
    }

    #[test]
    fn missing_influence_channels_are_reported() {
        let fvf = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
        ]);
        let offsets = skin_channel_offsets(&fvf);
        assert!(!offsets.has_influences());
        assert_eq!(offsets.normal, Some(12));
    }

    #[test]
    fn ubyte4_and_color_read_identically() {
        let data = [10u8, 20, 30, 40];
        assert_eq!(
            read_four_bytes(&data, 0, GrcDataSize::UByte4),
            read_four_bytes(&data, 0, GrcDataSize::Color)
        );
        assert_eq!(read_four_bytes(&data, 0, GrcDataSize::UByte4), data);
    }

    #[test]
    fn short4_influences_are_narrowed_to_bytes() {
        let data = [0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04];
        assert_eq!(read_four_bytes(&data, 0, GrcDataSize::Short4), [1, 2, 3, 4]);
    }

    #[test]
    fn reads_past_the_buffer_end_yield_zero_rather_than_panicking() {
        let data = [1u8, 2];
        assert_eq!(read_four_bytes(&data, 0, GrcDataSize::UByte4), [1, 2, 0, 0]);
    }

    #[test]
    fn anomaly_reporting_flags_both_kinds() {
        let clean = SkinnedMesh::default();
        assert!(!clean.has_anomalies());
        assert!(clean.is_empty());

        let noisy = SkinnedMesh {
            out_of_range_influences: 1,
            ..SkinnedMesh::default()
        };
        assert!(noisy.has_anomalies());
    }

    #[test]
    fn an_unskinned_geometry_decodes_to_none() {
        let vertices = or_io::gta5::gen8::GrcVertexBuffer::default();
        assert!(decode_skinning(&vertices, &[]).is_none());
    }

    /// ⚠ Skinning cares about four channels and a vertex may carry any of
    /// sixteen, so the offsets must account for channels this crate never
    /// reads.
    #[test]
    fn the_channels_skinning_ignores_still_move_the_ones_it_reads() {
        let fvf = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Weight, GrcDataSize::UByte4),
            (GrcFvfChannel::Binding, GrcDataSize::UByte4),
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
            // Neither of these is a skinning channel, and both sit before Tex0.
            (GrcFvfChannel::Diffuse, GrcDataSize::Color),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);

        let offsets = skin_channel_offsets(&fvf);
        assert_eq!(offsets.position, Some(0));
        assert_eq!(offsets.weight, Some(12));
        assert_eq!(offsets.binding, Some(16));
        assert_eq!(offsets.normal, Some(20));
        assert!(offsets.has_influences());

        let bare = layout(&[]);
        assert!(!skin_channel_offsets(&bare).has_influences());
    }
}
