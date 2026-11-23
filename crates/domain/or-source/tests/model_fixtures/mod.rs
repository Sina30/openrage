//! Drawables the suites build, so a check does not depend on a staged corpus, and what a drawable's meshes are made of.

#![allow(dead_code)]

use or_interchange::extract::channel_lanes;
use or_interchange::Bone;
use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout, VertexReader};
use or_io::decl::{File, Gen8, Hash32, Shared};
use or_io::gta5::drawable::gen8;
use or_io::gta5::{
    DrawableTarget, FragDrawable, FragType, GrmGeometry, GrmModel, GrmShaderGroup, GtaDrawable,
    GtaFragType, PgDictionary, RmcDrawable, RmcLod, SpdAABB,
};

/// An FVF declaring `channels`, each at its encoding.
fn fvf(channels: &[(GrcFvfChannel, GrcDataSize)]) -> gen8::GrcFvf {
    let mut fvf = gen8::GrcFvf::default();
    for &(channel, size) in channels {
        fvf.fvf |= 1 << channel as u32;
        fvf.channel_sizes |= (size as u64) << (channel as u64 * 4);
    }
    fvf.channel_count = channels.len() as u8;
    fvf.fvf_size = VertexLayout::of(fvf.fvf, fvf.channel_sizes).packed_stride() as u8;
    fvf
}

/// A geometry of position, normal, one UV set and, when `skinned`, the two skin channels.
///
/// `shift` moves every vertex, so two geometries of one drawable are not the same one.
pub fn geometry(vertices: usize, skinned: bool, shift: f32) -> GrmGeometry<Gen8> {
    // In the order a vertex lays them out, which is the order of the channels' numbers.
    let mut channels = vec![(GrcFvfChannel::Position, GrcDataSize::Float3)];
    if skinned {
        channels.push((GrcFvfChannel::Weight, GrcDataSize::UByte4));
        channels.push((GrcFvfChannel::Binding, GrcDataSize::UByte4));
    }
    channels.push((GrcFvfChannel::Normal, GrcDataSize::Float3));
    channels.push((GrcFvfChannel::Tex0, GrcDataSize::Float2));

    let mut data = Vec::new();
    for index in 0..vertices {
        let along = index as f32 + shift;
        for axis in [along, 0.0, along * 0.5] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        if skinned {
            // Two influences, 200 and 55 of 255, on palette slots 0 and 1, which are bones 5 and 2.
            data.extend_from_slice(&[200u8, 55, 0, 0]);
            data.extend_from_slice(&[0u8, 1, 0, 0]);
        }
        for axis in [0.0f32, 1.0, 0.0] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
        for axis in [along * 0.25, 0.5f32] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
    }
    let triangles = vertices / 3;
    let mut geometry = GrmGeometry::<Gen8> {
        primitive_type: 3,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data,
            fvf: Some(Box::new(fvf(&channels))),
            lock_ptr: Shared::Same,
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices: (0..triangles * 3).map(|i| i as u16).collect(),
            ..Default::default()
        }),
        vertex_data: Shared::Same,
        matrix_palette: if skinned { vec![5, 2] } else { Vec::new() },
        ..Default::default()
    };
    // The counts a file states are its runs' lengths, which an import states the same way.
    geometry.take_counts_from_runs();
    geometry
}

/// A drawable of one model of `geometries`, one shader, and `bones` bones where it has a skeleton.
pub fn drawable(
    name: &str,
    geometries: Vec<GrmGeometry<Gen8>>,
    bones: Option<usize>,
) -> RmcDrawable<Gen8> {
    let count = geometries.len();
    let mut out = RmcDrawable::<Gen8> {
        name: name.into(),
        shader_group: Some(Box::new(GrmShaderGroup {
            shaders: vec![Box::new(gen8::GrcInstanceData {
                basis_hash: Hash32::of("default"),
                ..Default::default()
            })],
            ..Default::default()
        })),
        ..Default::default()
    };
    out.lod_group.lods[0] = RmcLod {
        models: vec![Box::new(GrmModel {
            aabbs: vec![SpdAABB::default(); or_io::codec::mesh::aabb_slots(count)],
            shader_indices: vec![0; count],
            geometries: geometries.into_iter().map(Box::new).collect(),
            ..Default::default()
        })],
        ..Default::default()
    }
    .into();
    if let Some(bones) = bones {
        out.skeleton = Some(Box::new(skeleton(bones)));
    }
    out
}

fn skeleton(bones: usize) -> or_io::gta5::CrSkeletonData {
    use or_io::gta5::{CrBoneData, Vec4V};
    or_io::gta5::CrSkeletonData {
        bones: (0..bones)
            .map(|i| CrBoneData {
                name: format!("bone{i}"),
                bone_id: i as u16,
                index: i as u16,
                parent_index: if i == 0 { -1 } else { 0 },
                rotation: Vec4V {
                    w: 1.0,
                    ..Default::default()
                },
                scale: Vec4V {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect(),
        parent_indices: (0..bones).map(|i| if i == 0 { -1 } else { 0 }).collect(),
        ..Default::default()
    }
}

/// A reference skeleton of `count` bones, a chain off bone 0, as a ped's `.yft` would hand over.
pub fn reference(count: usize) -> Vec<Bone> {
    (0..count)
        .map(|i| Bone {
            name: format!("ref{i}"),
            tag: 100 + i as u16,
            parent: (i > 0).then_some(0),
            translation: [0.0, 0.1 * i as f32, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        })
        .collect()
}

/// A `.ydr` that saves.
pub fn ydr(drawable: RmcDrawable<Gen8>) -> Vec<u8> {
    GtaDrawable::<Gen8> {
        base: drawable,
        ..Default::default()
    }
    .to_bytes()
    .unwrap_or_else(|e| panic!("the fixture drawable would not save: {e}"))
}

/// A `.yft` whose common drawable is `drawable`, and that saves.
pub fn yft(drawable: RmcDrawable<Gen8>) -> Vec<u8> {
    GtaFragType::<Gen8> {
        base: FragType {
            common_drawable: Some(Box::new(FragDrawable {
                base: drawable,
                ..Default::default()
            })),
            ..Default::default()
        },
        ..Default::default()
    }
    .to_bytes()
    .unwrap_or_else(|e| panic!("the fixture fragment would not save: {e}"))
}

/// A `.ydd` of `entries`, each under the hash of its name.
pub fn ydd(entries: Vec<(&str, RmcDrawable<Gen8>)>) -> Vec<u8> {
    let mut keyed: Vec<(u32, GtaDrawable<Gen8>)> = entries
        .into_iter()
        .map(|(name, base)| {
            (
                or_io::string_hash_str(name),
                GtaDrawable {
                    base,
                    ..Default::default()
                },
            )
        })
        .collect();
    keyed.sort_by_key(|(code, _)| *code);
    PgDictionary {
        codes: keyed.iter().map(|(code, _)| *code).collect(),
        entries: keyed.into_iter().map(|(_, d)| Some(Box::new(d))).collect(),
        ..Default::default()
    }
    .to_bytes()
    .unwrap_or_else(|e| panic!("the fixture dictionary would not save: {e}"))
}

/// What one geometry holds that a round trip has to give back.
#[derive(Debug, Clone, PartialEq)]
pub struct Meshes {
    pub positions: Vec<Vec<[f32; 3]>>,
    pub indices: Vec<Vec<u16>>,
    /// Each vertex's influences as (bone it resolves to, weight byte), the weighted lanes only, bone order.
    pub skin: Vec<Vec<Vec<(u16, u8)>>>,
}

pub fn meshes<T: DrawableTarget>(drawable: &RmcDrawable<T>) -> Meshes {
    let geometries: Vec<_> = drawable
        .lod_group
        .lods
        .iter()
        .flat_map(|lod| lod.models.iter())
        .flat_map(|model| model.geometries.iter().map(|g| &**g))
        .collect();
    let mut out = Meshes {
        positions: Vec::new(),
        indices: Vec::new(),
        skin: Vec::new(),
    };
    for geometry in geometries {
        let reader = VertexReader::new(&*geometry.vertex_buffer);
        out.positions.push(
            reader
                .as_ref()
                .map(|r| r.positions().collect())
                .unwrap_or_default(),
        );
        out.indices
            .push(T::indices(&geometry.index_buffer).to_vec());
        let mut skin = Vec::new();
        if let Some(reader) = reader.filter(|r| r.layout().has(GrcFvfChannel::Binding)) {
            for vertex in 0..reader.count() {
                let weights = channel_lanes(&reader, GrcFvfChannel::Weight, vertex);
                let slots = channel_lanes(&reader, GrcFvfChannel::Binding, vertex);
                let mut lanes: Vec<(u16, u8)> = (0..4)
                    .filter(|lane| weights[*lane] > 0)
                    .map(|lane| {
                        let bone = geometry.matrix_palette[usize::from(slots[lane])];
                        (bone, weights[lane])
                    })
                    .collect();
                lanes.sort_unstable();
                skin.push(lanes);
            }
        }
        out.skin.push(skin);
    }
    out
}

/// The first drawable of a `.ydr`.
pub fn read_ydr(bytes: &[u8]) -> GtaDrawable<Gen8> {
    GtaDrawable::<Gen8>::from_bytes(bytes).unwrap()
}

/// A `.ydd`'s drawables as (code, drawable), in file order.
pub fn read_ydd(bytes: &[u8]) -> Vec<(u32, RmcDrawable<Gen8>)> {
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).unwrap();
    dictionary
        .codes
        .iter()
        .copied()
        .zip(dictionary.entries)
        .filter_map(|(code, entry)| Some((code, entry?.base)))
        .collect()
}

/// Whether two position runs are the same to the tolerance a quantised channel allows.
pub fn same_positions(a: &[Vec<[f32; 3]>], b: &[Vec<[f32; 3]>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.len() == b.len()
                && a.iter().zip(b).all(|(p, q)| {
                    (0..3).all(|axis| (p[axis] - q[axis]).abs() <= 1e-3 * p[axis].abs().max(1.0))
                })
        })
}

/// Four floats in the order the engine lays them out.
pub fn vec4(x: f32, y: f32, z: f32, w: f32) -> or_io::gta5::Vec4V {
    or_io::gta5::Vec4V {
        x,
        y,
        z,
        w,
        ..Default::default()
    }
}
