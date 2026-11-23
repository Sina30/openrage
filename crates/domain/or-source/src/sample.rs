//! Small valid game files, so that the suites of the crates that import, cook or plan them need no staged corpus.
//!
//! Each is a Legacy file the readers open and [`crate::game_import`] decompiles. Nothing outside a test calls this.

use or_io::codec::mesh::{GrcDataSize, GrcFvfChannel, VertexLayout};
use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::{File, Gen8, Hash32, Shared};
use or_io::gta5::drawable::gen8;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{
    GrmGeometry, GrmModel, GrmShaderGroup, GtaDrawable, PgDictionary, RmcDrawable, RmcLod, SpdAABB,
};

fn mark(tag: &str) -> u32 {
    tag.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

/// A drawable dictionary that differs from the one of every other `tag`, so that equal tags are equal bytes.
pub fn drawables_of(tag: &str) -> Vec<u8> {
    let mark = mark(tag);
    drawable_dictionary(&format!("d{mark:08x}"), 6, (mark % 997) as f32)
}

/// A texture dictionary that differs from the one of every other `tag`.
pub fn textures_of(tag: &str) -> Vec<u8> {
    let mark = mark(tag);
    texture_dictionary(&format!("t{mark:08x}"), (mark >> 8) as u8)
}

/// The bytes a test writes for the file called `name` whose content the words `tag` stand for: a small valid game file for a `.ydd` or a `.ytd` (the same words, the same bytes), and the words themselves for any other.
pub fn as_a_game_file(name: &str, tag: &[u8]) -> Vec<u8> {
    let extension = name.rsplit('.').next().map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("ydd") => drawables_of(&String::from_utf8_lossy(tag)),
        Some("ytd") => textures_of(&String::from_utf8_lossy(tag)),
        _ => tag.to_vec(),
    }
}

/// A texture dictionary of one block-compressed 16 by 16 texture called `name`, with its mips down to 4 by 4, whose pixels `shade` decides.
pub fn texture_dictionary(name: &str, shade: u8) -> Vec<u8> {
    let format = GrcTextureFormat::DXT1;
    let texture = GrcTexture {
        name: name.into(),
        width: 16,
        height: 16,
        depth: 1,
        mip_count: 3,
        mip_stride: u16::try_from(format.legacy_mip_stride(16)).unwrap_or_default(),
        format: format.to_raw_format(),
        backing_store: vec![shade; format.total_size(16, 16, 3) as usize],
        ..Default::default()
    };
    PgDictionary {
        codes: vec![or_io::string_hash(name.as_bytes())],
        entries: vec![Some(Box::new(texture))],
        ..Default::default()
    }
    .to_bytes()
    .unwrap_or_default()
}

/// A drawable dictionary of one drawable called `name` of one geometry of `vertices` vertices, which `shift` moves so that two are not equal.
pub fn drawable_dictionary(name: &str, vertices: usize, shift: f32) -> Vec<u8> {
    drawable_dictionary_with_levels(name, vertices, shift, 1)
}

/// The same drawable with its one geometry at the first `levels` levels of detail (1 to 4), high first, so that a file with fewer than three has a medium or a low level missing.
pub fn drawable_dictionary_with_levels(
    name: &str,
    vertices: usize,
    shift: f32,
    levels: usize,
) -> Vec<u8> {
    let channels = [
        (GrcFvfChannel::Position, GrcDataSize::Float3),
        (GrcFvfChannel::Normal, GrcDataSize::Float3),
        (GrcFvfChannel::Tex0, GrcDataSize::Float2),
    ];
    let mut fvf = gen8::GrcFvf::default();
    for (channel, size) in channels {
        fvf.fvf |= 1 << channel as u32;
        fvf.channel_sizes |= (size as u64) << (channel as u64 * 4);
    }
    fvf.channel_count = channels.len() as u8;
    fvf.fvf_size = VertexLayout::of(fvf.fvf, fvf.channel_sizes).packed_stride() as u8;
    let mut data = Vec::new();
    for index in 0..vertices {
        let along = index as f32 + shift;
        for axis in [along, 0.0, along * 0.5, 0.0, 1.0, 0.0, along * 0.25, 0.5] {
            data.extend_from_slice(&axis.to_le_bytes());
        }
    }
    let mut geometry = GrmGeometry::<Gen8> {
        primitive_type: 3,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data,
            fvf: Some(Box::new(fvf)),
            lock_ptr: Shared::Same,
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices: (0..(vertices / 3) * 3).map(|i| i as u16).collect(),
            ..Default::default()
        }),
        vertex_data: Shared::Same,
        ..Default::default()
    };
    geometry.take_counts_from_runs();
    let mut drawable = RmcDrawable::<Gen8> {
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
    for lod in drawable.lod_group.lods.iter_mut().take(levels.clamp(1, 4)) {
        *lod = RmcLod {
            models: vec![Box::new(GrmModel {
                aabbs: vec![SpdAABB::default(); or_io::codec::mesh::aabb_slots(1)],
                shader_indices: vec![0],
                geometries: vec![Box::new(geometry.clone())],
                ..Default::default()
            })],
            ..Default::default()
        }
        .into();
    }
    let code = or_io::string_hash_str(name);
    PgDictionary {
        codes: vec![code],
        entries: vec![Some(Box::new(GtaDrawable {
            base: drawable,
            ..Default::default()
        }))],
        ..Default::default()
    }
    .to_bytes()
    .unwrap_or_default()
}
