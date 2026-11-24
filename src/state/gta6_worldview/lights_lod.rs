//! The lists of a GTA VI map body that the map reader passes over: its level-of-detail lights
//! (`LODLightsSOA`), its distant lights (`DistantLODLightsSOA`) and its reflection probes.
//!
//! ⚠ A map of level-of-detail lights states how far each light reaches and no place: the lights
//! stand, one for one, at the positions of the distant lights of the map its `parent` names.
// ponytail: this reads bytes and belongs beside the map reader in or-io; it is here because the
// track that wrote it owned no file there.

use or_io::gta6::map::C_MAP_DATA;
use or_io::hash::literal_string_hash as lit;
use or_io::meta::binary::rbm::schema::RbmModel;

const LOD_LIGHT: u32 = lit(b"CLODLight");
const DISTANT_LIGHT: u32 = lit(b"CDistantLODLight");
/// Where a `CMapData` holds each list.
const PROBES_AT: usize = 0x190;
const LOD_AT: usize = 0x1F0;
const DISTANT_AT: usize = 0x290;
const MAP_DATA_SIZE: usize = 0x460;
const PROBE_SIZE: usize = 0x60;
const STRUCT: u8 = 0x05;
const ARRAY: u8 = 0x52;

/// Each member this reads, where its class holds it and its type code, which a body's own table must agree with.
const LAYOUT: &[(u32, &str, &[u8], usize, u8)] = &[
    (
        C_MAP_DATA,
        "CMapData",
        b"reflectionProbeData",
        PROBES_AT,
        ARRAY,
    ),
    (C_MAP_DATA, "CMapData", b"LODLightsSOA", LOD_AT, STRUCT),
    (
        C_MAP_DATA,
        "CMapData",
        b"DistantLODLightsSOA",
        DISTANT_AT,
        STRUCT,
    ),
    (LOD_LIGHT, "CLODLight", b"falloff", 0x10, ARRAY),
    (LOD_LIGHT, "CLODLight", b"falloffExponent", 0x20, ARRAY),
    (DISTANT_LIGHT, "CDistantLODLight", b"position", 0x00, ARRAY),
    (DISTANT_LIGHT, "CDistantLODLight", b"RGB", 0x10, ARRAY),
    (DISTANT_LIGHT, "CDistantLODLight", b"intensity", 0x20, ARRAY),
];

/// Why the lists of a map body do not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LodError {
    #[error("the body is no CMapData")]
    NotAMap,
    #[error("the body declares {class} with `{member}` elsewhere than at {offset:#X} with type {code:#04X}")]
    Layout {
        class: &'static str,
        member: String,
        offset: usize,
        code: u8,
    },
    #[error("the map's {what} is broken: {why}")]
    Broken { what: &'static str, why: String },
}

/// One distant light: where it stands, its colour and how strong it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distant {
    pub position: [f32; 3],
    /// Red, green and blue.
    // ⚠ The order of the bytes is GTA V's (blue lowest), not measured on this build.
    pub rgb: [u8; 3],
    pub intensity: f32,
}

/// The lists of one map body.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapLod {
    /// How far each level-of-detail light reaches, in metres.
    pub falloff: Vec<f32>,
    pub falloff_exponent: Vec<f32>,
    pub distant: Vec<Distant>,
    /// How many reflection probes the map holds; nothing draws them.
    pub probes: usize,
}

fn broken(what: &'static str, why: impl Into<String>) -> LodError {
    LodError::Broken {
        what,
        why: why.into(),
    }
}

/// The `stride`-byte elements of the array whose 16-byte header is at `at` of `bytes`.
fn elements<'m>(
    model: &'m RbmModel,
    bytes: &[u8],
    at: usize,
    stride: usize,
    what: &'static str,
) -> Result<std::slice::ChunksExact<'m, u8>, LodError> {
    let header =
        (bytes.get(at..at + 12)).ok_or_else(|| broken(what, "its header runs past its block"))?;
    let raw = u64::from_le_bytes(std::array::from_fn(|k| header[k]));
    let count = usize::from(u16::from_le_bytes([header[8], header[9]]));
    if count == 0 {
        return Ok((&[] as &[u8]).chunks_exact(stride));
    }
    if (raw >> 12) & 0xFF != 0 {
        return Err(broken(
            what,
            format!("its pointer {raw:#X} sets bits 12 to 19"),
        ));
    }
    let start = usize::try_from(raw >> 20).unwrap_or(usize::MAX);
    (model.block((raw & 0xFFF) as i32))
        .and_then(|block| block.data.get(start..)?.get(..count * stride))
        .map(|run| run.chunks_exact(stride))
        .ok_or_else(|| broken(what, format!("its {count} elements run past their block")))
}

fn floats<const N: usize>(element: &[u8]) -> [f32; N] {
    std::array::from_fn(|k| f32::from_le_bytes(std::array::from_fn(|b| element[4 * k + b])))
}

/// The level-of-detail lights, distant lights and reflection probes of a map body's class table and data blocks.
pub fn read(model: &RbmModel) -> Result<MapLod, LodError> {
    let root = (model.block(model.root_block_id as i32))
        .filter(|root| root.data_type == C_MAP_DATA && root.data.len() >= MAP_DATA_SIZE)
        .ok_or(LodError::NotAMap)?;
    for &(class, name, member, offset, code) in LAYOUT {
        // A class the body does not declare has no record in it to misread.
        let Some(declared) = model.class_for(class) else {
            continue;
        };
        let fits = (declared.entries.iter()).any(|e| {
            e.name_hash == lit(member)
                && e.offset as usize == offset
                && e.data_type.to_byte() == code
        });
        if !fits {
            return Err(LodError::Layout {
                class: name,
                member: String::from_utf8_lossy(member).into_owned(),
                offset,
                code,
            });
        }
    }
    let root = &root.data;
    let float = |at: usize, what| -> Result<Vec<f32>, LodError> {
        Ok(elements(model, root, LOD_AT + at, 4, what)?
            .map(|e| floats::<1>(e)[0])
            .collect())
    };
    let (falloff, falloff_exponent) = (float(0x10, "falloff")?, float(0x20, "falloffExponent")?);
    if falloff.len() != falloff_exponent.len() {
        return Err(broken(
            "level-of-detail lights",
            "its lists are not as long",
        ));
    }
    let at = |member: usize, stride: usize, what| {
        elements(model, root, DISTANT_AT + member, stride, what)
    };
    let (position, rgb) = (at(0x00, 12, "position")?, at(0x10, 4, "RGB")?);
    let intensity = at(0x20, 4, "intensity")?;
    if [rgb.len(), intensity.len()] != [position.len(); 2] {
        return Err(broken("distant lights", "its lists are not as long"));
    }
    let distant: Vec<Distant> = (position.zip(rgb).zip(intensity))
        .map(|((position, rgb), intensity)| Distant {
            position: floats(position),
            rgb: [rgb[2], rgb[1], rgb[0]],
            intensity: floats::<1>(intensity)[0],
        })
        .collect();
    let finite = |d: &Distant| d.position.iter().all(|x| x.is_finite());
    if !distant.iter().all(finite) {
        return Err(broken("distant lights", "a position is no number"));
    }
    let probes = elements(model, root, PROBES_AT, PROBE_SIZE, "reflection probes")?.len();
    Ok(MapLod {
        falloff,
        falloff_exponent,
        distant,
        probes,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use or_io::meta::binary::rbm::schema::RbmDataBlock;

    fn header(root: &mut [u8], at: usize, block: u64, count: u16) {
        root[at..at + 8].copy_from_slice(&block.to_le_bytes());
        root[at + 8..at + 10].copy_from_slice(&count.to_le_bytes());
        root[at + 10..at + 12].copy_from_slice(&count.to_le_bytes());
    }

    fn bytes(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// A map body of `falloff` level-of-detail lights and of distant lights (position, RGB word, intensity).
    // ⚠ The model declares no class: an entry of a class table cannot be made outside or-io, so
    // the refusal of a body that declares a list elsewhere is not tested here.
    pub(in super::super) fn model(falloff: &[f32], distant: &[([f32; 3], u32, f32)]) -> RbmModel {
        let mut root = vec![0u8; MAP_DATA_SIZE];
        let n = falloff.len() as u16;
        header(&mut root, LOD_AT + 0x10, 2, n);
        header(&mut root, LOD_AT + 0x20, 3, n);
        let m = distant.len() as u16;
        for (k, member) in [0x00, 0x10, 0x20].into_iter().enumerate() {
            header(&mut root, DISTANT_AT + member, 4 + k as u64, m);
        }
        let blocks: Vec<(u32, Vec<u8>)> = vec![
            (C_MAP_DATA, root),
            (0x21, bytes(falloff)),
            (0x21, bytes(&vec![1.0; falloff.len()])),
            (0, distant.iter().flat_map(|d| bytes(&d.0)).collect()),
            (
                0x15,
                distant.iter().flat_map(|d| d.1.to_le_bytes()).collect(),
            ),
            (
                0x21,
                distant.iter().flat_map(|d| d.2.to_le_bytes()).collect(),
            ),
        ];
        RbmModel {
            classes: Default::default(),
            enums: Default::default(),
            data_blocks: (blocks.into_iter())
                .map(|(data_type, data)| RbmDataBlock {
                    data_type,
                    data,
                    data_addr: 0,
                })
                .collect(),
            root_block_id: 1,
            class_order: Vec::new(),
            enum_order: Vec::new(),
        }
    }

    #[test]
    fn the_lights_of_a_map_are_read_list_by_list() {
        let lists = read(&model(
            &[3.0, 10.0],
            &[([1.0, 2.0, 3.0], 0xFF10_2030, 80.0)],
        ))
        .expect("a map body");
        assert_eq!(lists.falloff, [3.0, 10.0]);
        assert_eq!(lists.falloff_exponent, [1.0, 1.0]);
        assert_eq!(lists.distant.len(), 1);
        let light = lists.distant[0];
        assert_eq!(light.position, [1.0, 2.0, 3.0]);
        assert_eq!((light.rgb, light.intensity), ([0x10, 0x20, 0x30], 80.0));
        assert_eq!(lists.probes, 0);
        assert_eq!(read(&model(&[], &[])), Ok(MapLod::default()));
    }

    #[test]
    fn a_list_past_its_block_lists_of_two_lengths_and_another_root_are_refused() {
        let mut long = model(&[3.0], &[]);
        header(&mut long.data_blocks[0].data, LOD_AT + 0x10, 2, 9);
        let past = read(&long);
        assert!(
            matches!(
                &past,
                Err(LodError::Broken {
                    what: "falloff",
                    ..
                })
            ),
            "{past:?}"
        );
        let mut uneven = model(&[3.0, 4.0], &[]);
        header(&mut uneven.data_blocks[0].data, LOD_AT + 0x20, 3, 1);
        let lists = read(&uneven);
        assert!(matches!(&lists, Err(LodError::Broken { .. })), "{lists:?}");
        let mut other = model(&[], &[]);
        other.data_blocks[0].data_type = LOD_LIGHT;
        assert_eq!(read(&other), Err(LodError::NotAMap));
    }
}
