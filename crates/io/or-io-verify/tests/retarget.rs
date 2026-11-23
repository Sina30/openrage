//! Each format both targets declare a drawable or a texture in, retargeted
//! between Gen8 and Gen9 over the corpus: each staged `.ydr`, `.ydd`, `.yft`
//! and `.ytd` converts to the other target and back, or is refused by name,
//! saving and reopening owned at each step, with the same geometry, parameter
//! values and pixels, and the loss report names every value the round trip
//! did not bring back.

use std::collections::BTreeMap;
use std::path::Path;

use or_io::codec::mesh::VertexReader;
use or_io::codec::texture::{GrcTextureFormat, TextureSource};
use or_io::decl::{File, Gen8, Gen9, Owned, Raw, Record};
use or_io::gta5::drawable::{DrawableTarget, GrmShaderGroup};
use or_io::gta5::gen8::{EntryData, GrcTexture};
use or_io::gta5::gen9::GrcTextureG9;
use or_io::gta5::retarget::{
    drawable_dictionary_to_gen8, drawable_dictionary_to_gen9, fragment_to_gen8, fragment_to_gen9,
    texture_dictionary_to_gen8, texture_dictionary_to_gen9, to_gen8, to_gen9, LossReport,
};
use or_io::gta5::shaders::fxc::GrcEffect;
use or_io::gta5::shaders::shader_defs::{ParamRemap, ShaderDefinitions};
use or_io::gta5::{
    FragDrawable, FragType, GtaDrawable, GtaFragType, PgDictionary, PhArchetypeDamp,
    PhArticulatedBodyType, RmcDrawable,
};
use or_io::{Error, Generation};
use or_io_verify::gates::{files, name};

/// The remap the corpus's own `.fxc` effects state, each named as its file after the staging prefix.
fn remap(what: &str) -> Option<ParamRemap> {
    let mut effects = Vec::new();
    for path in or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "fxc").find() {
        let stem = path.file_stem()?.to_string_lossy().into_owned();
        let effect = stem.rsplit("__").next()?.to_string();
        effects.push((effect, GrcEffect::parse(&std::fs::read(&path).ok()?).ok()?));
    }
    if effects.is_empty() {
        or_test_support::skip_corpus(what, "no .fxc effect is staged to build the remap from");
        return None;
    }
    let (defs, _) = ShaderDefinitions::assemble("gta5", "gen8", &effects, &[]);
    Some(ParamRemap::of(&defs))
}

/// Each geometry's every channel as raw bytes per vertex, its indices, and its model's shader index.
type Geometry = BTreeMap<String, (Vec<(u8, Vec<Vec<u8>>)>, Vec<u16>, u16)>;

fn geometry<T: DrawableTarget>(d: &RmcDrawable<T>) -> Geometry {
    let mut out = Geometry::new();
    for (l, lod) in d.lod_group.lods.iter().enumerate() {
        for (m, model) in lod.models.iter().enumerate() {
            for (g, geometry) in model.geometries.iter().enumerate() {
                let reader = VertexReader::new(&*geometry.vertex_buffer).expect("a layout");
                let channels = (reader.layout().present())
                    .map(|(c, _)| {
                        let raw = (0..reader.count())
                            .map(|v| reader.raw(c, v).expect("in the run").to_vec())
                            .collect();
                        (c as u8, raw)
                    })
                    .collect();
                let indices = geometry.mesh().indices.iter().map(|&i| i as u16).collect();
                out.insert(
                    format!("level{l}/model{m}/geometry{g}"),
                    (channels, indices, model.shader_indices[g]),
                );
            }
        }
    }
    out
}

/// A parameter's value: a binding by texture name, or its bytes padded to whole registers.
#[derive(Debug, Clone, PartialEq)]
enum Param {
    Texture(Option<String>),
    Constant(Vec<u8>),
}

fn registers(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.resize(bytes.len().next_multiple_of(16), 0);
    out
}

/// Each shader's parameters, by the hash its target spells each with.
fn legacy_params(d: &RmcDrawable<Gen8>) -> Vec<BTreeMap<u32, Param>> {
    let Some(group) = d.shader_group.as_deref() else {
        return Vec::new();
    };
    (group.shaders.iter())
        .map(|s| {
            (s.entries.iter())
                .map(|e| {
                    let value = match &e.data {
                        EntryData::Texture(slot) => {
                            Param::Texture(group.slot_name(slot).map(str::to_string))
                        }
                        EntryData::Float(rows) => Param::Constant(
                            (rows.iter())
                                .flat_map(|r| [r.x, r.y, r.z, r.w])
                                .flat_map(f32::to_le_bytes)
                                .collect(),
                        ),
                    };
                    (e.name_hash.get(), value)
                })
                .collect()
        })
        .collect()
}

fn enhanced_params(d: &RmcDrawable<Gen9>) -> Vec<BTreeMap<u32, Param>> {
    let Some(group): Option<&GrmShaderGroup<Gen9>> = d.shader_group.as_deref() else {
        return Vec::new();
    };
    (group.shaders.iter())
        .map(|s| {
            (s.param_infos.params.iter())
                .filter_map(|p| {
                    let value = if let Some(slot) = p.texture() {
                        Param::Texture(
                            (s.params.textures.get(slot))
                                .and_then(|t| group.slot_name(t))
                                .map(str::to_string),
                        )
                    } else {
                        let (buffer, range) = p.constant()?;
                        Param::Constant(registers(&s.params.buffers[buffer][range]))
                    };
                    Some((p.name.get(), value))
                })
                .collect()
        })
        .collect()
}

/// Every parameter `before` holds that `after` does not is named in `report` under `at`, and every one both hold is equal.
fn same_values(
    path: &Path,
    at: &str,
    before: &[BTreeMap<u32, Param>],
    after: &[BTreeMap<u32, Param>],
    report: &LossReport,
) -> usize {
    assert_eq!(before.len(), after.len(), "{} {at}: shaders", name(path));
    let mut carried = 0;
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        let shader = format!("{at}shader {i}");
        for (hash, value) in b {
            match a.get(hash) {
                Some(back) => {
                    assert_eq!(back, value, "{} {shader} {hash:#010x}", name(path));
                    carried += 1;
                }
                None => assert!(
                    report.names(&shader, &format!("{hash:#010x}")),
                    "{} {shader}: {hash:#010x} is gone and the report does not name it: {report}",
                    name(path)
                ),
            }
        }
    }
    carried
}

/// A texture's name, width, height, pixel format, sRGB flag and mip chain.
type Texture = (String, u16, u16, GrcTextureFormat, bool, Vec<u8>);

fn texture(t: &impl TextureSource, name: &str, width: u16, height: u16) -> Texture {
    let (w, h) = t.stored_size();
    let format = t.pixel_format();
    let size = format.total_size(w, h, t.stored_levels()) as usize;
    let chain = t.payload()[..size].to_vec();
    (name.to_string(), width, height, format, t.is_srgb(), chain)
}

fn dictionary<T: Owned>(dict: &PgDictionary<T>, each: impl Fn(&T) -> Texture) -> Vec<(u32, Texture)>
where
    Raw<T>: Record,
{
    (dict.codes.iter().copied())
        .zip(
            dict.entries
                .iter()
                .map(|t| each(t.as_deref().expect("an entry"))),
        )
        .collect()
}

fn legacy_dict(dict: &PgDictionary<GrcTexture>) -> Vec<(u32, Texture)> {
    dictionary(dict, |t| texture(t, &t.name, t.width, t.height))
}

fn enhanced_dict(dict: &PgDictionary<GrcTextureG9>) -> Vec<(u32, Texture)> {
    dictionary(dict, |t| texture(t, &t.name, t.width, t.height))
}

fn legacy_textures(d: &RmcDrawable<Gen8>) -> Vec<(u32, Texture)> {
    (d.shader_group.as_deref())
        .and_then(|g| g.texture_dictionary.as_deref())
        .map_or_else(Vec::new, legacy_dict)
}

fn enhanced_textures(d: &RmcDrawable<Gen9>) -> Vec<(u32, Texture)> {
    (d.shader_group.as_deref())
        .and_then(|g| g.texture_dictionary.as_deref())
        .map_or_else(Vec::new, enhanced_dict)
}

fn reopened<T>(value: &T) -> T
where
    T: File,
{
    T::from_bytes(&value.to_bytes().expect("saves")).expect("reopens owned")
}

/// A refusal names the texture or the place it refuses, and why.
fn refused_by_name(path: &Path, e: &Error) {
    let Error::Invalid { what, why } = e else {
        panic!("{}: refused without a name: {e}", name(path));
    };
    assert!(!what.is_empty() && !why.is_empty(), "{}: {e}", name(path));
    println!("{} refused: {e}", name(path));
}

/// A Legacy drawable's trip under `at`: the same geometry on both sides, and its values and textures back; geometries and values carried.
fn legacy_trip(
    path: &Path,
    at: &str,
    before: &RmcDrawable<Gen8>,
    there: &RmcDrawable<Gen9>,
    back: &RmcDrawable<Gen8>,
    lost: &LossReport,
) -> (usize, usize) {
    let geometries = geometry(before);
    assert_eq!(
        geometries,
        geometry(there),
        "{} {at}: as Enhanced",
        name(path)
    );
    assert_eq!(
        geometries,
        geometry(back),
        "{} {at}: back as Legacy",
        name(path)
    );
    let carried = same_values(path, at, &legacy_params(before), &legacy_params(back), lost);
    assert_eq!(
        legacy_textures(before),
        legacy_textures(back),
        "{} {at}: embedded textures",
        name(path)
    );
    (geometries.len(), carried)
}

fn enhanced_trip(
    path: &Path,
    at: &str,
    before: &RmcDrawable<Gen9>,
    there: &RmcDrawable<Gen8>,
    back: &RmcDrawable<Gen9>,
    lost: &LossReport,
) -> (usize, usize) {
    let geometries = geometry(before);
    assert_eq!(
        geometries,
        geometry(there),
        "{} {at}: as Legacy",
        name(path)
    );
    assert_eq!(
        geometries,
        geometry(back),
        "{} {at}: back as Enhanced",
        name(path)
    );
    let carried = same_values(
        path,
        at,
        &enhanced_params(before),
        &enhanced_params(back),
        lost,
    );
    assert_eq!(
        enhanced_textures(before),
        enhanced_textures(back),
        "{} {at}: embedded textures",
        name(path)
    );
    (geometries.len(), carried)
}

#[test]
fn every_staged_legacy_drawable_converts_to_enhanced_and_back_with_its_geometry_and_values() {
    let what = "a Legacy drawable retargeted and back";
    let Some(remap) = remap(what) else {
        return;
    };
    let Some(paths) = files("ydr", Some(Generation::Gen8), what) else {
        return;
    };
    let (mut geometries, mut carried) = (0usize, 0usize);
    for path in &paths {
        let legacy = GtaDrawable::<Gen8>::open(path).unwrap();
        let (enhanced, lost) =
            to_gen9(&legacy, &remap).unwrap_or_else(|e| panic!("{}: to Enhanced: {e}", name(path)));
        let enhanced = reopened(&enhanced);
        let (back, _) = to_gen8(&enhanced, &remap)
            .unwrap_or_else(|e| panic!("{}: back to Legacy: {e}", name(path)));
        let back = reopened(&back);
        let (g, c) = legacy_trip(path, "", &legacy, &enhanced, &back, &lost);
        geometries += g;
        carried += c;
    }
    println!(
        "{} Legacy drawables, {geometries} geometries, {carried} parameter values carried both ways",
        paths.len()
    );
    assert!(carried > 0, "no parameter value was carried");
}

#[test]
fn every_staged_enhanced_drawable_converts_to_legacy_and_back_with_its_geometry_and_values() {
    let what = "an Enhanced drawable retargeted and back";
    let Some(remap) = remap(what) else {
        return;
    };
    let Some(paths) = files("ydr", Some(Generation::Gen9), what) else {
        return;
    };
    let (mut geometries, mut carried) = (0usize, 0usize);
    for path in &paths {
        let enhanced = GtaDrawable::<Gen9>::open(path).unwrap();
        let (legacy, lost) =
            to_gen8(&enhanced, &remap).unwrap_or_else(|e| panic!("{}: to Legacy: {e}", name(path)));
        let legacy = reopened(&legacy);
        let (back, _) = to_gen9(&legacy, &remap)
            .unwrap_or_else(|e| panic!("{}: back to Enhanced: {e}", name(path)));
        let back = reopened(&back);
        let (g, c) = enhanced_trip(path, "", &enhanced, &legacy, &back, &lost);
        geometries += g;
        carried += c;
    }
    println!(
        "{} Enhanced drawables, {geometries} geometries, {carried} parameter values carried both ways",
        paths.len()
    );
    assert!(carried > 0, "no parameter value was carried");
}

/// Each word of `before` that `back` does not hold again is named in `report` under `at`.
fn words_named(at: &str, words: &[(&str, u64, u64)], report: &LossReport) {
    for (field, before, back) in words {
        assert!(
            before == back || report.names(at, field),
            "{at}: {field} was {before:#x} and came back {back:#x}, and the report does not name it: {report}"
        );
    }
}

#[test]
fn every_staged_legacy_texture_dictionary_converts_to_enhanced_and_back_with_its_pixels() {
    let what = "a Legacy texture dictionary retargeted and back";
    let Some(paths) = files("ytd", Some(Generation::Gen8), what) else {
        return;
    };
    let (mut converted, mut textures) = (0usize, 0usize);
    for path in &paths {
        let legacy = PgDictionary::<GrcTexture>::open(path).unwrap();
        let (enhanced, lost) = match texture_dictionary_to_gen9(&legacy) {
            Ok(out) => out,
            Err(e) => {
                refused_by_name(path, &e);
                continue;
            }
        };
        let enhanced = reopened(&enhanced);
        let (back, _) = texture_dictionary_to_gen8(&enhanced)
            .unwrap_or_else(|e| panic!("{}: back to Legacy: {e}", name(path)));
        let back = reopened(&back);

        let before = legacy_dict(&legacy);
        assert_eq!(
            before,
            enhanced_dict(&enhanced),
            "{}: as Enhanced",
            name(path)
        );
        assert_eq!(before, legacy_dict(&back), "{}: back as Legacy", name(path));
        for (b, a) in legacy.entries.iter().zip(&back.entries) {
            let (b, a) = (b.as_deref().unwrap(), a.as_deref().unwrap());
            let at = format!("texture {}", b.name);
            let w = |f: fn(&GrcTexture) -> u64| (f(b), f(a));
            #[rustfmt::skip]
            let words = [
                ("handle_index", w(|t| t.handle_index.into())),
                ("extra_flags", w(|t| t.extra_flags.into())),
                ("stereo_rt_mode", w(|t| t.stereo_rt_mode as u64)),
                ("resource_type_and_conversion_flags", w(|t| t.resource_type_and_conversion_flags.into())),
                ("info_bits", w(|t| t.info_bits.into())),
                ("mip_stride", w(|t| t.mip_stride.into())),
            ];
            let words: Vec<_> = words.iter().map(|(f, (x, y))| (*f, *x, *y)).collect();
            words_named(&at, &words, &lost);
        }
        converted += 1;
        textures += before.len();
    }
    println!(
        "{converted} of {} Legacy texture dictionaries, {textures} textures, carried both ways",
        paths.len()
    );
    assert!(converted > 0, "no dictionary converted");
}

#[test]
fn every_staged_enhanced_texture_dictionary_converts_to_legacy_and_back_with_its_pixels() {
    let what = "an Enhanced texture dictionary retargeted and back";
    let Some(paths) = files("ytd", Some(Generation::Gen9), what) else {
        return;
    };
    let (mut converted, mut textures) = (0usize, 0usize);
    for path in &paths {
        let enhanced = PgDictionary::<GrcTextureG9>::open(path).unwrap();
        let (legacy, lost) = match texture_dictionary_to_gen8(&enhanced) {
            Ok(out) => out,
            Err(e) => {
                refused_by_name(path, &e);
                continue;
            }
        };
        let legacy = reopened(&legacy);
        let (back, _) = texture_dictionary_to_gen9(&legacy)
            .unwrap_or_else(|e| panic!("{}: back to Enhanced: {e}", name(path)));
        let back = reopened(&back);

        let before = enhanced_dict(&enhanced);
        assert_eq!(before, legacy_dict(&legacy), "{}: as Legacy", name(path));
        assert_eq!(
            before,
            enhanced_dict(&back),
            "{}: back as Enhanced",
            name(path)
        );
        for (b, a) in enhanced.entries.iter().zip(&back.entries) {
            let (b, a) = (b.as_deref().unwrap(), a.as_deref().unwrap());
            let at = format!("texture {}", b.name);
            let w = |f: fn(&GrcTextureG9) -> u64| (f(b), f(a));
            let words = [
                ("flags", w(|t| t.flags.into())),
                ("anti_alias_type", w(|t| t.anti_alias_type.into())),
                ("usage_data", w(|t| t.usage_data.into())),
            ];
            let words: Vec<_> = words.iter().map(|(f, (x, y))| (*f, *x, *y)).collect();
            words_named(&at, &words, &lost);
        }
        converted += 1;
        textures += before.len();
    }
    println!(
        "{converted} of {} Enhanced texture dictionaries, {textures} textures, carried both ways",
        paths.len()
    );
    assert!(converted > 0, "no dictionary converted");
}

/// Each drawable of a dictionary by its hash, as the loss report names it.
fn by_hash<T: DrawableTarget>(
    dict: &PgDictionary<GtaDrawable<T>>,
) -> Vec<(String, &RmcDrawable<T>)> {
    (dict.codes.iter())
        .zip(&dict.entries)
        .map(|(hash, d)| {
            let d = d.as_deref().expect("an entry");
            (format!("drawable {hash:#010x}/"), &d.base)
        })
        .collect()
}

#[test]
fn every_staged_legacy_drawable_dictionary_converts_to_enhanced_and_back_with_its_geometry_and_values(
) {
    let what = "a Legacy drawable dictionary retargeted and back";
    let Some(remap) = remap(what) else {
        return;
    };
    let Some(paths) = files("ydd", Some(Generation::Gen8), what) else {
        return;
    };
    let (mut converted, mut drawables, mut geometries, mut carried) = (0, 0, 0, 0);
    for path in &paths {
        let legacy = PgDictionary::<GtaDrawable<Gen8>>::open(path).unwrap();
        let (enhanced, lost) = match drawable_dictionary_to_gen9(&legacy, &remap) {
            Ok(out) => out,
            Err(e) => {
                refused_by_name(path, &e);
                continue;
            }
        };
        let enhanced = reopened(&enhanced);
        let (back, _) = drawable_dictionary_to_gen8(&enhanced, &remap)
            .unwrap_or_else(|e| panic!("{}: back to Legacy: {e}", name(path)));
        let back = reopened(&back);
        let (before, there, again) = (by_hash(&legacy), by_hash(&enhanced), by_hash(&back));
        assert_eq!(before.len(), there.len(), "{}: drawables", name(path));
        assert_eq!(before.len(), again.len(), "{}: drawables", name(path));
        for ((at, b), ((_, t), (_, a))) in before.iter().zip(there.iter().zip(&again)) {
            let (g, c) = legacy_trip(path, at, b, t, a, &lost);
            geometries += g;
            carried += c;
        }
        converted += 1;
        drawables += before.len();
    }
    println!(
        "{converted} of {} Legacy drawable dictionaries, {drawables} drawables, {geometries} \
         geometries, {carried} parameter values carried both ways",
        paths.len()
    );
    assert!(carried > 0, "no parameter value was carried");
}

#[test]
fn every_staged_enhanced_drawable_dictionary_converts_to_legacy_and_back_with_its_geometry_and_values(
) {
    let what = "an Enhanced drawable dictionary retargeted and back";
    let Some(remap) = remap(what) else {
        return;
    };
    let Some(paths) = files("ydd", Some(Generation::Gen9), what) else {
        return;
    };
    let (mut converted, mut drawables, mut geometries, mut carried) = (0, 0, 0, 0);
    for path in &paths {
        let enhanced = PgDictionary::<GtaDrawable<Gen9>>::open(path).unwrap();
        let (legacy, lost) = match drawable_dictionary_to_gen8(&enhanced, &remap) {
            Ok(out) => out,
            Err(e) => {
                refused_by_name(path, &e);
                continue;
            }
        };
        let legacy = reopened(&legacy);
        let (back, _) = drawable_dictionary_to_gen9(&legacy, &remap)
            .unwrap_or_else(|e| panic!("{}: back to Enhanced: {e}", name(path)));
        let back = reopened(&back);
        let (before, there, again) = (by_hash(&enhanced), by_hash(&legacy), by_hash(&back));
        assert_eq!(before.len(), there.len(), "{}: drawables", name(path));
        assert_eq!(before.len(), again.len(), "{}: drawables", name(path));
        for ((at, b), ((_, t), (_, a))) in before.iter().zip(there.iter().zip(&again)) {
            let (g, c) = enhanced_trip(path, at, b, t, a, &lost);
            geometries += g;
            carried += c;
        }
        converted += 1;
        drawables += before.len();
    }
    println!(
        "{converted} of {} Enhanced drawable dictionaries, {drawables} drawables, {geometries} \
         geometries, {carried} parameter values carried both ways",
        paths.len()
    );
    assert!(carried > 0, "no parameter value was carried");
}

/// Each drawable a fragment holds, as the loss report names it.
fn named<T: DrawableTarget>(f: &FragType<T>) -> Vec<(String, &RmcDrawable<T>)> {
    let mut out: Vec<(String, &FragDrawable<T>)> = Vec::new();
    out.extend(
        f.common_drawable
            .as_deref()
            .map(|d| ("drawable/".into(), d)),
    );
    for (i, d) in f.extra_drawables.iter().enumerate() {
        out.push((format!("extra drawable {i}/"), d));
    }
    for (i, cloth) in f.env_cloth.iter().enumerate() {
        let d = cloth.referenced_drawable.as_deref();
        out.extend(d.map(|d| (format!("environment cloth {i}/"), d)));
    }
    out.extend((f.common_cloth_drawable.as_deref()).map(|d| ("cloth drawable/".into(), d)));
    if let Some(group) = f.physics_lod_group.as_deref() {
        let lods = [&group.high_lod, &group.medium_lod, &group.low_lod];
        for (l, lod) in lods.into_iter().enumerate() {
            for (c, child) in lod.iter().flat_map(|lod| &lod.children).enumerate() {
                let at = format!("physics level {l}, child {c}");
                out.extend((child.undamaged_entity.as_deref()).map(|d| (format!("{at}/"), d)));
                out.extend(
                    (child.damaged_entity.as_deref()).map(|d| (format!("{at} damaged/"), d)),
                );
            }
        }
    }
    out.into_iter().map(|(at, d)| (at, &d.base)).collect()
}

/// What a fragment states apart from its drawables, which neither target lays out differently.
fn same_physics<A: DrawableTarget, B: DrawableTarget>(
    path: &Path,
    a: &GtaFragType<A>,
    b: &GtaFragType<B>,
) {
    let at = name(path);
    assert_eq!(a.lights, b.lights, "{at}: lights");
    assert_eq!(a.vehicle_window_data, b.vehicle_window_data, "{at}: glass");
    assert_eq!(a.tune_name, b.tune_name, "{at}: tune name");
    assert_eq!(a.child_count(), b.child_count(), "{at}: children");
    let (la, lb) = (
        a.physics_lod_group
            .as_deref()
            .into_iter()
            .flat_map(|g| g.lods()),
        b.physics_lod_group
            .as_deref()
            .into_iter()
            .flat_map(|g| g.lods()),
    );
    for (x, y) in la.zip(lb) {
        assert_eq!(x.groups, y.groups, "{at}: groups");
        // ⚠ A joint's, an archetype's and a bound's vectors hold NaN in their fourth lanes, so only scalars compare.
        let body = |b: &Option<Box<PhArticulatedBodyType>>| {
            (b.as_deref()).map(|b| (b.joint_parent_index, b.joint_types, b.ph_joint_types.len()))
        };
        assert_eq!(
            body(&x.body_type),
            body(&y.body_type),
            "{at}: articulated body"
        );
        let archetype = |a: &Option<Box<PhArchetypeDamp>>| {
            a.as_deref().map(|a| {
                let words = [a.type_flags, a.include_flags, a.property_flags.into()];
                let masses = [a.mass, a.inv_mass, a.gravity_factor, a.max_speed];
                (a.filename.clone(), words, masses.map(f32::to_bits))
            })
        };
        assert_eq!(
            archetype(&x.phys_damp_undamaged),
            archetype(&y.phys_damp_undamaged),
            "{at}: archetype"
        );
        assert_eq!(
            archetype(&x.phys_damp_damaged),
            archetype(&y.phys_damp_damaged),
            "{at}: damaged archetype"
        );
        for (p, q) in x.children.iter().zip(&y.children) {
            assert_eq!(
                (p.undamaged_mass, p.damaged_mass, p.bone_id),
                (q.undamaged_mass, q.damaged_mass, q.bone_id),
                "{at}: a child"
            );
        }
    }
}

#[test]
fn every_staged_legacy_fragment_converts_to_enhanced_and_back_with_its_geometry_and_values() {
    let what = "a Legacy fragment retargeted and back";
    let Some(remap) = remap(what) else {
        return;
    };
    let Some(paths) = files("yft", Some(Generation::Gen8), what) else {
        return;
    };
    let (mut converted, mut drawables, mut geometries, mut carried) = (0, 0, 0, 0);
    for path in &paths {
        let legacy = GtaFragType::<Gen8>::open(path).unwrap();
        let (enhanced, lost) = match fragment_to_gen9(&legacy, &remap) {
            Ok(out) => out,
            Err(e) => {
                refused_by_name(path, &e);
                continue;
            }
        };
        let enhanced = reopened(&enhanced);
        let (back, _) = fragment_to_gen8(&enhanced, &remap)
            .unwrap_or_else(|e| panic!("{}: back to Legacy: {e}", name(path)));
        let back = reopened(&back);
        same_physics(path, &legacy, &enhanced);
        same_physics(path, &legacy, &back);
        let (before, there, again) = (named(&legacy), named(&enhanced), named(&back));
        assert_eq!(before.len(), there.len(), "{}: drawables", name(path));
        assert_eq!(before.len(), again.len(), "{}: drawables", name(path));
        for ((at, b), ((_, t), (_, a))) in before.iter().zip(there.iter().zip(&again)) {
            let (g, c) = legacy_trip(path, at, b, t, a, &lost);
            geometries += g;
            carried += c;
        }
        converted += 1;
        drawables += before.len();
    }
    println!(
        "{converted} of {} Legacy fragments, {drawables} drawables, {geometries} geometries, \
         {carried} parameter values carried both ways",
        paths.len()
    );
    assert!(carried > 0, "no parameter value was carried");
}

#[test]
fn every_staged_enhanced_fragment_converts_to_legacy_and_back_with_its_geometry_and_values() {
    let what = "an Enhanced fragment retargeted and back";
    let Some(remap) = remap(what) else {
        return;
    };
    let Some(paths) = files("yft", Some(Generation::Gen9), what) else {
        return;
    };
    let (mut converted, mut drawables, mut geometries, mut carried) = (0, 0, 0, 0);
    for path in &paths {
        let enhanced = <GtaFragType<Gen9> as File>::open(path).unwrap();
        let (legacy, lost) = match fragment_to_gen8(&enhanced, &remap) {
            Ok(out) => out,
            Err(e) => {
                refused_by_name(path, &e);
                continue;
            }
        };
        let legacy = reopened(&legacy);
        let (back, _) = fragment_to_gen9(&legacy, &remap)
            .unwrap_or_else(|e| panic!("{}: back to Enhanced: {e}", name(path)));
        let back = reopened(&back);
        same_physics(path, &enhanced, &legacy);
        same_physics(path, &enhanced, &back);
        let (before, there, again) = (named(&enhanced), named(&legacy), named(&back));
        assert_eq!(before.len(), there.len(), "{}: drawables", name(path));
        assert_eq!(before.len(), again.len(), "{}: drawables", name(path));
        for ((at, b), ((_, t), (_, a))) in before.iter().zip(there.iter().zip(&again)) {
            let (g, c) = enhanced_trip(path, at, b, t, a, &lost);
            geometries += g;
            carried += c;
        }
        converted += 1;
        drawables += before.len();
    }
    println!(
        "{converted} of {} Enhanced fragments, {drawables} drawables, {geometries} geometries, \
         {carried} parameter values carried both ways",
        paths.len()
    );
    assert!(carried > 0, "no parameter value was carried");
}
