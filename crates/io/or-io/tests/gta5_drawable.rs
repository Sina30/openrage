//! A `.ydr` built as an owned value with invented content, for each target:
//! a shader group whose dictionary holds one texture, a shader binding it,
//! a reference and a row of floats, a two-bone skeleton, a light and one level
//! of detail of two models. Saved, reopened equal, edited as the D16 target
//! edits a drawable, and saved to a fixed point; the typed open names the
//! target a file holds. Needs no game file.

use std::path::{Path, PathBuf};

use or_io::decl::{File, Gen8, Gen9, Hash32, Keyed, Raw, Shared, Vec4, View};
use or_io::gta5::drawable::{gen8, gen9};
use or_io::gta5::drawable_codecs::gen8::{EntryData, GrcInstanceDataEntry};
use or_io::gta5::drawable_codecs::gen9::Params;
use or_io::gta5::texture_dict::gen8::GrcTexture;
use or_io::gta5::texture_dict::gen9::GrcTextureG9;
use or_io::gta5::*;
use or_io::hash::string_hash_str;
use or_io::{Asset, Error};

fn dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gta5_drawable");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn skeleton() -> CrSkeletonData {
    let bone = |name: &str, id, parent| CrBoneData {
        name: name.into(),
        bone_id: id,
        parent_index: parent,
        next_index: -1,
        ..Default::default()
    };
    CrSkeletonData {
        bones: vec![bone("root", 0, -1), bone("door", 0x4321, 0)],
        inverse_transforms: vec![Mat34V::default(); 2],
        default_transforms: vec![Mat34V::default(); 2],
        parent_indices: vec![-1, 0],
        bone_tag_slot_count: 3,
        ..Default::default()
    }
}

fn lods<T: DrawableTarget>() -> RmcLodGroup<T> {
    let model = |mask| {
        Box::new(GrmModel::<T> {
            mask,
            ..Default::default()
        })
    };
    RmcLodGroup {
        lods: [
            RmcLod {
                models: vec![model(1), model(2)],
                ..Default::default()
            }
            .into(),
            Default::default(),
            Default::default(),
            Default::default(),
        ],
        lod_distances: [30.0, 60.0, 120.0, 240.0],
        ..Default::default()
    }
}

fn legacy() -> GtaDrawable {
    let code = string_hash_str("diffuse");
    let texture = GrcTexture {
        name: "diffuse".into(),
        width: 4,
        height: 4,
        mip_count: 1,
        ..Default::default()
    };
    let entry = |name: &str, data| GrcInstanceDataEntry {
        name_hash: Hash32::of(name),
        data,
        ..Default::default()
    };
    let shader = gen8::GrcInstanceData {
        entries: vec![
            entry(
                "DiffuseSampler",
                EntryData::Texture(TextureSlot::Embedded(Keyed::to(code))),
            ),
            entry(
                "BumpSampler",
                EntryData::Texture(TextureSlot::Reference(Box::new(
                    gen8::GrcTextureReference {
                        name: "shared_bump".into(),
                        resource_type_and_conversion_flags: 2,
                        ..Default::default()
                    },
                ))),
            ),
            entry("specularIntensity", EntryData::Float(vec![Vec4::default()])),
        ],
        basis_hash: Hash32::of("default"),
        ..Default::default()
    };
    GtaDrawable {
        base: RmcDrawable {
            shader_group: Some(Box::new(GrmShaderGroup {
                texture_dictionary: Some(Box::new(PgDictionary {
                    codes: vec![code],
                    entries: vec![Some(Box::new(texture))],
                    ..Default::default()
                })),
                shaders: vec![Box::new(shader)],
                ..Default::default()
            })),
            skeleton: Some(Box::new(skeleton())),
            lod_group: lods(),
            name: "police".into(),
            ..Default::default()
        },
        lights: vec![CLightAttr {
            light_type: 1,
            intensity: 2.0,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn enhanced() -> GtaDrawable<Gen9> {
    let shader = gen9::GrcInstanceDataG9 {
        name: Hash32::of("default"),
        params: Params {
            buffers: vec![vec![0; 16]],
            textures: vec![TextureSlot::Reference(Box::new(
                gen9::GrcTextureReferenceG9 {
                    name: "shared_diffuse".into(),
                    ..Default::default()
                },
            ))],
            samplers: vec![1],
            ..Default::default()
        },
        param_infos: Box::new(gen9::ShaderParamInfos {
            buffer_copies: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    GtaDrawable {
        base: RmcDrawable {
            shader_group: Some(Box::new(GrmShaderGroup {
                texture_dictionary: Some(Box::new(PgDictionary::<GrcTextureG9>::default())),
                shaders: vec![Box::new(shader)],
                ..Default::default()
            })),
            skeleton: Some(Box::new(skeleton())),
            lod_group: lods(),
            name: "police".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// `value` saved to `path`, reopened as the same type and equal to it, and saved again to the same bytes.
fn round_trip<D>(value: &D, path: &Path) -> D
where
    D: File + PartialEq + std::fmt::Debug,
{
    value.save_as(path).unwrap();
    let reopened = D::open(path).unwrap();
    assert_eq!(&reopened, value);
    let first = std::fs::read(path).unwrap();
    reopened.save().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), first, "a fixed point");
    reopened
}

#[test]
fn a_legacy_drawable_built_owned_saves_reopens_and_edits_as_the_target_does() {
    let path = dir().join("police.ydr");
    round_trip(&legacy(), &path);

    let mut car = GtaDrawable::open(&path).unwrap();
    assert_eq!(car.name, "police");
    let entries = &car.shader_group.as_deref().unwrap().shaders[0].entries;
    assert!(matches!(
        &entries[0].data,
        EntryData::Texture(TextureSlot::Embedded(k)) if k.key == Some(string_hash_str("diffuse"))
    ));
    assert!(matches!(
        &entries[1].data,
        EntryData::Texture(TextureSlot::Reference(r)) if r.name == "shared_bump"
    ));
    car.lod_group.lod_distances[0] = 45.0;
    car.name = "police2".into();
    car.lod_group.lods[0].models.remove(1);
    car.save().unwrap();

    let again = GtaDrawable::open(&path).unwrap();
    assert_eq!(again.name, "police2");
    assert_eq!(again.lod_group.lod_distances, [45.0, 60.0, 120.0, 240.0]);
    let models = &again.lod_group.lods[0].models;
    assert_eq!(models.iter().map(|m| m.mask).collect::<Vec<_>>(), [1]);
    assert_eq!(again.skeleton.as_deref().unwrap().bones[1].name, "door");
    round_trip(&again, &path);
}

#[test]
fn an_enhanced_drawable_built_owned_saves_reopens_and_opens_as_its_target() {
    let path = dir().join("police_enhanced.ydr");
    let saved = round_trip(&enhanced(), &path);
    let textures = &saved.shader_group.as_deref().unwrap().shaders[0]
        .params
        .textures;
    assert!(matches!(&textures[0], TextureSlot::Reference(r) if r.name == "shared_diffuse"));

    let why = GtaDrawable::open(&path).unwrap_err();
    assert!(
        matches!(
            why,
            Error::WrongTarget {
                found: "Enhanced",
                ..
            }
        ),
        "{why:?}"
    );
    assert!(why.to_string().contains("`or_io::open`"), "{why}");
    match or_io::open(&path).unwrap() {
        Asset::DrawableGen9(d) => assert_eq!(d.name, "police"),
        other => panic!("{:?} opened as another asset", other.origin().path()),
    }
}

#[test]
fn a_legacy_drawable_opens_through_or_io_open() {
    let path = dir().join("asset.ydr");
    legacy().save_as(&path).unwrap();
    match or_io::open(&path).unwrap() {
        Asset::Drawable(d) => assert_eq!(d.name, "police"),
        other => panic!("{:?} opened as another asset", other.origin().path()),
    }
}

#[test]
fn a_save_states_the_index_and_primitive_counts_its_index_buffer_holds() {
    let path = dir().join("indices.ydr");
    let mut car = legacy();
    let model = &mut car.lod_group.lods[0].models[0];
    model.geometries.push(Box::new(GrmGeometry {
        primitive_type: 3,
        vertex_data: Shared::Same,
        vertex_buffer: Box::new(gen8::GrcVertexBuffer {
            data: vec![0; 36],
            lock_ptr: Shared::Same,
            fvf: Some(Box::new(gen8::GrcFvf {
                fvf_size: 12,
                ..Default::default()
            })),
            ..Default::default()
        }),
        index_buffer: Box::new(gen8::GrcIndexBuffer {
            indices: vec![0, 1, 2],
            ..Default::default()
        }),
        ..Default::default()
    }));
    model.aabbs.push(Default::default());
    model.shader_indices.push(0);
    car.save_as(&path).unwrap();

    let mut car = GtaDrawable::open(&path).unwrap();
    let geometry = &mut car.lod_group.lods[0].models[0].geometries[0];
    geometry.index_buffer.indices.extend([2, 1, 0]);
    assert_eq!(geometry.primitive_count(), 2);
    car.save().unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let peeked = GtaDrawable::<Gen8>::peek(&bytes).unwrap();
    let image = peeked.image();
    let slot = peeked.root().unwrap().base().lod_group.lods[0];
    let lod = View::<Raw<RmcLod<Gen8>>>::at_address(image, slot.raw()).unwrap();
    let model = lod.models().unwrap().follow(0).unwrap().unwrap();
    let geometry = model.geometries().unwrap().follow(0).unwrap().unwrap();
    assert_eq!(
        (geometry.indices_count, geometry.primitive_count),
        (6, 2),
        "the counts the saved geometry states"
    );
}

/// Why saving `drawable` fails once its first model draws with shader `index`, if it does.
fn drawing_with<T: DrawableTarget>(mut drawable: GtaDrawable<T>, index: u16) -> Option<String>
where
    GtaDrawable<T>: File,
{
    let model = &mut drawable.lod_group.lods[0].models[0];
    model.geometries.push(Default::default());
    model.aabbs.push(Default::default());
    model.shader_indices.push(index);
    drawable.to_bytes().err().map(|e| e.to_string())
}

#[test]
fn a_model_drawing_with_a_shader_past_its_group_is_refused_at_save_on_either_target() {
    assert_eq!(drawing_with(legacy(), 0), None);
    assert_eq!(drawing_with(enhanced(), 0), None);
    for why in [drawing_with(legacy(), 1), drawing_with(enhanced(), 1)] {
        let why = why.expect("a shader past the group of one");
        assert!(why.contains("shader index 1 of 1 materials"), "{why}");
    }
}

/// Each record of `v`, outermost first.
fn each_record(v: &mut or_io::desc::Value, f: &mut dyn FnMut(&mut or_io::desc::Record)) {
    use or_io::desc::Value;
    match v {
        Value::Struct(record) => {
            f(record);
            for (_, child) in record.children_mut() {
                each_record(child, f);
            }
        }
        Value::Ptr {
            target: Some(t), ..
        } => each_record(t, f),
        Value::Array { elements, .. } => elements.iter_mut().for_each(|e| each_record(e, f)),
        Value::Blob { parts, .. } => parts.iter_mut().for_each(|(_, p)| each_record(p, f)),
        _ => {}
    }
}

/// `bytes` with its material's second texture pointer aimed at the first's reference, as a ped component binds one by-name texture twice.
fn sharing_a_reference(bytes: &[u8]) -> Vec<u8> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    use or_io::decl::Aligned;
    use or_io::desc::{Image, Value};
    use or_io::Generation;

    let opened = Rsc7::open(bytes).unwrap();
    let pages = Aligned::from_bytes(&opened.data);
    let format = or_io::gta5::formats::format("ydr").unwrap();
    let mut tree = format
        .read(
            Generation::Gen8,
            Image::new(pages.bytes(), opened.virtual_size as usize),
        )
        .unwrap();
    let mut shared = false;
    each_record(&mut tree, &mut |record| {
        let Some(Value::Ptr {
            target: Some(blob), ..
        }) = record.child_mut("entries")
        else {
            return;
        };
        let Value::Blob { parts, .. } = blob.as_mut() else {
            return;
        };
        let Some((_, Value::Array { elements, .. })) = parts.iter_mut().find(|(n, _)| n == "data")
        else {
            return;
        };
        let Some(Value::Ptr { source, .. }) = elements.first() else {
            return;
        };
        let source = *source;
        elements[1] = Value::Alias(source);
        shared = true;
    });
    assert!(shared, "the material was not found");
    format.save(Generation::Gen8, &tree).unwrap()
}

#[test]
fn two_slots_that_share_one_texture_reference_open_with_one_each() {
    let reference = || TextureSlot::Reference(Box::new(gen8::GrcTextureReference::named("shared")));
    let mut value = legacy();
    let shader = &mut value.shader_group.as_deref_mut().unwrap().shaders[0];
    shader.entries[0].data = EntryData::Texture(reference());
    shader.entries[1].data = EntryData::Texture(reference());
    let saved = value.to_bytes().unwrap();

    let opened = GtaDrawable::<Gen8>::from_bytes(&sharing_a_reference(&saved)).unwrap();
    let shader = &opened.shader_group.as_deref().unwrap().shaders[0];
    for entry in &shader.entries[..2] {
        assert!(
            matches!(&entry.data, EntryData::Texture(TextureSlot::Reference(r)) if r.name == "shared"),
            "{:?}",
            entry.data
        );
    }

    let path = dir().join("shared_reference.ydr");
    let again = round_trip(&opened, &path);
    assert_eq!(again, opened, "each slot owns a reference once it is saved");
}
