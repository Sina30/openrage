//! A glb, a glTF or an FBX imported into a model source: stored for the compile, with a report of what the drawable had no place for.

mod model_fixtures;

use std::io::Cursor;
use std::path::{Path, PathBuf};

use fbxcel::low::FbxVersion;
use fbxcel::writer::v7400::binary::{FbxFooter, Writer};
use model_fixtures::{drawable, geometry, meshes, read_ydd, read_ydr, same_positions, ydd, ydr};
use or_interchange::gltf::{self, Container};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection};
use or_source::asset::Source;
use or_source::models::{self, Extraction, Imported};
use or_source::Problem;

fn how() -> Extraction<'static> {
    Extraction {
        from_game: false,
        virtual_path: None,
        textures: false,
        skeleton: None,
    }
}

fn said(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(Problem::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn source(dir: &Path) -> Source {
    Source::open(dir).unwrap()
}

fn import(dir: &Path, entry: Option<&str>, file: &Path) -> Imported {
    models::import(&source(dir), entry, file)
        .unwrap_or_else(|problems| panic!("import: {problems:#?}"))
}

fn scene_of(drawable: &or_io::gta5::RmcDrawable<or_io::decl::Gen8>) -> or_interchange::Scene {
    drawable_scene(
        "subject",
        drawable,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    )
}

fn two_geometries() -> Vec<u8> {
    ydr(drawable(
        "subject",
        vec![geometry(6, false, 0.0), geometry(9, false, 40.0)],
        None,
    ))
}

#[test]
fn a_glb_is_stored_as_it_came_and_what_a_drawable_has_no_place_for_is_reported() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = models::extract_drawable(&bytes, "subject", scratch.path(), &how()).unwrap();

    // Another mesh of the same shape, lifted, as a tool would hand back; its material is the
    // drawable's own and the file says so.
    let mut scene = scene_of(&read_ydr(&bytes).base);
    for p in scene
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        for position in &mut p.positions {
            position[2] += 3.0;
        }
    }
    let external = scratch.path().join("edited.glb");
    std::fs::write(
        &external,
        gltf::write(&scene, "edited", Container::Glb).primary,
    )
    .unwrap();

    let imported = import(&folder, None, &external);
    assert_eq!(
        imported.dropped,
        vec![or_interchange::Dropped::new("materials", 1)]
    );
    assert!(imported.report.iter().any(|l| l.contains("2 geometries")));
    // Stored exactly as given, so the artist's own file is the source.
    assert_eq!(
        std::fs::read(folder.join("model.glb")).unwrap(),
        std::fs::read(&external).unwrap()
    );
    let compiled = models::compile(&source(&folder)).unwrap();
    let before = meshes(&read_ydr(&bytes).base);
    let after = meshes(&read_ydr(&compiled.bytes).base);
    let expected: Vec<Vec<[f32; 3]>> = before
        .positions
        .iter()
        .map(|run| run.iter().map(|p| [p[0], p[1], p[2] + 3.0]).collect())
        .collect();
    assert!(same_positions(&expected, &after.positions));
}

#[test]
fn a_gltf_with_its_files_beside_it_is_stored_as_one_self_contained_glb() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = models::extract_drawable(&bytes, "subject", scratch.path(), &how()).unwrap();

    let files = gltf::write(
        &scene_of(&read_ydr(&bytes).base),
        "outside",
        Container::Separate,
    );
    let outside = scratch.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("outside.gltf"), &files.primary).unwrap();
    for (name, data) in &files.sidecars {
        std::fs::write(outside.join(name), data).unwrap();
    }

    import(&folder, None, &outside.join("outside.gltf"));
    let stored = std::fs::read(folder.join("model.glb")).unwrap();
    assert_eq!(&stored[..4], b"glTF", "a .gltf is stored as a glb");
    // Two geometries went in under their own addresses: the compile accepts them.
    let compiled = models::compile(&source(&folder)).unwrap();
    let (before, after) = (
        meshes(&read_ydr(&bytes).base),
        meshes(&read_ydr(&compiled.bytes).base),
    );
    assert!(same_positions(&before.positions, &after.positions));
}

/// A triangle of three corners, each with a normal and a coordinate, in an FBX as Blender writes one.
fn triangle_fbx() -> Vec<u8> {
    struct Out(Writer<Cursor<Vec<u8>>>);
    impl Out {
        fn node(
            &mut self,
            name: &str,
            f: impl FnOnce(&mut fbxcel::writer::v7400::binary::AttributesWriter<'_, Cursor<Vec<u8>>>),
            children: impl FnOnce(&mut Self),
        ) {
            {
                let mut attributes = self.0.new_node(name).unwrap();
                f(&mut attributes);
            }
            children(self);
            self.0.close_node().unwrap();
        }
    }
    let mut out = Out(Writer::new(Cursor::new(Vec::new()), FbxVersion::V7_4).unwrap());
    let none = |_: &mut fbxcel::writer::v7400::binary::AttributesWriter<'_, Cursor<Vec<u8>>>| {};
    // Blender's own axes: Z up, a front along -Y, X across, and centimetres.
    out.node("GlobalSettings", none, |g| {
        g.node("Properties70", none, |p| {
            for (name, value) in [
                ("UpAxis", 2),
                ("UpAxisSign", 1),
                ("FrontAxis", 1),
                ("FrontAxisSign", -1),
                ("CoordAxis", 0),
                ("CoordAxisSign", 1),
            ] {
                p.node(
                    "P",
                    |a| {
                        a.append_string_direct(name).unwrap();
                        a.append_string_direct("int").unwrap();
                        a.append_string_direct("Integer").unwrap();
                        a.append_string_direct("").unwrap();
                        a.append_i32(value).unwrap();
                    },
                    |_| {},
                );
            }
        });
    });
    out.node("Objects", none, |o| {
        o.node(
            "Geometry",
            |a| {
                a.append_i64(10).unwrap();
                a.append_string_direct("Triangle\0\u{1}Geometry").unwrap();
                a.append_string_direct("Mesh").unwrap();
            },
            |g| {
                g.node(
                    "Vertices",
                    |a| {
                        a.append_arr_f64_from_iter(
                            None,
                            [0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0],
                        )
                        .unwrap()
                    },
                    |_| {},
                );
                g.node(
                    "PolygonVertexIndex",
                    |a| a.append_arr_i32_from_iter(None, [0, 1, !2]).unwrap(),
                    |_| {},
                );
                g.node(
                    "LayerElementNormal",
                    |a| a.append_i32(0).unwrap(),
                    |l| {
                        l.node(
                            "MappingInformationType",
                            |a| a.append_string_direct("ByPolygonVertex").unwrap(),
                            |_| {},
                        );
                        l.node(
                            "ReferenceInformationType",
                            |a| a.append_string_direct("Direct").unwrap(),
                            |_| {},
                        );
                        l.node(
                            "Normals",
                            |a| {
                                a.append_arr_f64_from_iter(
                                    None,
                                    [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                                )
                                .unwrap()
                            },
                            |_| {},
                        );
                    },
                );
                g.node(
                    "LayerElementUV",
                    |a| a.append_i32(0).unwrap(),
                    |l| {
                        l.node(
                            "MappingInformationType",
                            |a| a.append_string_direct("ByPolygonVertex").unwrap(),
                            |_| {},
                        );
                        l.node(
                            "ReferenceInformationType",
                            |a| a.append_string_direct("Direct").unwrap(),
                            |_| {},
                        );
                        l.node(
                            "UV",
                            |a| {
                                a.append_arr_f64_from_iter(None, [0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
                                    .unwrap()
                            },
                            |_| {},
                        );
                    },
                );
            },
        );
        o.node(
            "Model",
            |a| {
                a.append_i64(20).unwrap();
                a.append_string_direct("Triangle\0\u{1}Model").unwrap();
                a.append_string_direct("Mesh").unwrap();
            },
            |_| {},
        );
        o.node(
            "Material",
            |a| {
                a.append_i64(30).unwrap();
                a.append_string_direct("Skin\0\u{1}Material").unwrap();
                a.append_string_direct("").unwrap();
            },
            |_| {},
        );
    });
    out.node("Connections", none, |c| {
        for (child, parent) in [(10, 20), (20, 0)] {
            c.node(
                "C",
                |a| {
                    a.append_string_direct("OO").unwrap();
                    a.append_i64(child).unwrap();
                    a.append_i64(parent).unwrap();
                },
                |_| {},
            );
        }
    });
    out.0.finalize(&FbxFooter::default()).unwrap().into_inner()
}

#[test]
fn an_fbx_is_converted_to_a_glb_and_compiles_into_the_drawable() {
    let scratch = tempfile::tempdir().unwrap();
    let base = ydr(drawable("subject", vec![geometry(6, false, 0.0)], None));
    let folder = models::extract_drawable(&base, "subject", scratch.path(), &how()).unwrap();

    let external = scratch.path().join("triangle.fbx");
    std::fs::write(&external, triangle_fbx()).unwrap();
    let imported = import(&folder, None, &external);
    assert_eq!(
        imported.dropped,
        vec![or_interchange::Dropped::new("materials", 1)]
    );

    assert_eq!(
        &std::fs::read(folder.join("model.glb")).unwrap()[..4],
        b"glTF"
    );
    let compiled = models::compile(&source(&folder)).unwrap();
    let after = meshes(&read_ydr(&compiled.bytes).base);
    // One geometry of one triangle, in metres: a hundred centimetres is one.
    assert_eq!(after.indices, vec![vec![0u16, 1, 2]]);
    assert!(same_positions(
        &after.positions,
        &[vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]
    ));
}

#[test]
fn an_import_that_does_not_compile_is_refused_and_leaves_the_glb_as_it_was() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = models::extract_drawable(&bytes, "subject", scratch.path(), &how()).unwrap();
    let before = std::fs::read(folder.join("model.glb")).unwrap();

    // One geometry's worth of mesh for a drawable of two.
    let one = ydr(drawable("subject", vec![geometry(6, false, 0.0)], None));
    let external = scratch.path().join("one.glb");
    std::fs::write(
        &external,
        gltf::write(&scene_of(&read_ydr(&one).base), "one", Container::Glb).primary,
    )
    .unwrap();

    let problems = models::import(&source(&folder), None, &external).unwrap_err();
    let why = said(&problems);
    assert!(
        why.contains("2 geometries") && why.contains("brings 1"),
        "{why}"
    );
    assert_eq!(std::fs::read(folder.join("model.glb")).unwrap(), before);
    assert!(!folder.join("model.glb.tmp").exists());
}

#[test]
fn a_format_this_build_does_not_import_is_refused_naming_the_ones_it_does() {
    let scratch = tempfile::tempdir().unwrap();
    let folder =
        models::extract_drawable(&two_geometries(), "subject", scratch.path(), &how()).unwrap();
    let external = scratch.path().join("mesh.obj");
    std::fs::write(&external, "v 0 0 0").unwrap();
    let why = said(&models::import(&source(&folder), None, &external).unwrap_err());
    assert!(
        why.contains("glb") && why.contains("gltf") && why.contains("fbx"),
        "{why}"
    );

    let missing: PathBuf = scratch.path().join("nothing.glb");
    let why = said(&models::import(&source(&folder), None, &missing).unwrap_err());
    assert!(why.contains("nothing.glb"), "{why}");
}

#[test]
fn an_import_into_a_dictionary_names_the_entry_it_goes_to() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = drawable("alpha", vec![geometry(6, false, 0.0)], None);
    let beta = drawable("beta", vec![geometry(9, false, 20.0)], None);
    let bytes = ydd(vec![("alpha", alpha.clone()), ("beta", beta.clone())]);
    let folder = models::extract_drawables(&bytes, "pair", scratch.path(), &how()).unwrap();

    let mut scene = scene_of(&beta);
    for p in scene
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        for position in &mut p.positions {
            position[2] += 4.0;
        }
    }
    let external = scratch.path().join("beta.glb");
    std::fs::write(
        &external,
        gltf::write(&scene, "beta", Container::Glb).primary,
    )
    .unwrap();

    // Which drawable of a dictionary a mesh goes into is the caller's to say.
    let why = said(&models::import(&source(&folder), None, &external).unwrap_err());
    assert!(why.contains("alpha") && why.contains("beta"), "{why}");
    let why = said(&models::import(&source(&folder), Some("gamma"), &external).unwrap_err());
    assert!(why.contains("gamma") && why.contains("alpha"), "{why}");

    let alpha_before = std::fs::read(folder.join("drawables/alpha/model.glb")).unwrap();
    import(&folder, Some("beta"), &external);
    assert_eq!(
        std::fs::read(folder.join("drawables/alpha/model.glb")).unwrap(),
        alpha_before,
        "an entry no import named was changed"
    );
    assert_eq!(
        std::fs::read(folder.join("drawables/beta/model.glb")).unwrap(),
        std::fs::read(&external).unwrap()
    );
    let compiled = models::compile(&source(&folder)).unwrap();
    let after = read_ydd(&compiled.bytes);
    let beta_code = or_io::string_hash_str("beta");
    let (_, beta_after) = after.iter().find(|(c, _)| *c == beta_code).unwrap();
    let expected: Vec<Vec<[f32; 3]>> = model_fixtures::meshes(&beta)
        .positions
        .iter()
        .map(|run| run.iter().map(|p| [p[0], p[1], p[2] + 4.0]).collect())
        .collect();
    assert!(same_positions(&expected, &meshes(beta_after).positions));

    // A drawable source takes no entry.
    let single =
        models::extract_drawable(&two_geometries(), "solo", scratch.path(), &how()).unwrap();
    let why = said(&models::import(&source(&single), Some("beta"), &external).unwrap_err());
    assert!(why.contains("no entries"), "{why}");
}

#[test]
fn the_lines_of_an_import_say_what_the_compile_did_and_what_the_drawable_had_no_place_for() {
    let scratch = tempfile::tempdir().unwrap();
    let bytes = two_geometries();
    let folder = models::extract_drawable(&bytes, "subject", scratch.path(), &how()).unwrap();
    let external = scratch.path().join("same.glb");
    std::fs::write(
        &external,
        gltf::write(&scene_of(&read_ydr(&bytes).base), "same", Container::Glb).primary,
    )
    .unwrap();

    let lines = import(&folder, None, &external).lines();

    assert!(
        lines.iter().any(|l| l.contains("2 geometries")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "not carried: 1 materials"),
        "the words `openrage-cli mesh import` prints for the same loss: {lines:?}"
    );
}

#[test]
fn a_source_lists_the_entries_an_import_can_name() {
    let scratch = tempfile::tempdir().unwrap();
    let alpha = drawable("alpha", vec![geometry(6, false, 0.0)], None);
    let beta = drawable("beta", vec![geometry(9, false, 20.0)], None);
    let pair = ydd(vec![("alpha", alpha), ("beta", beta)]);
    let dictionary = models::extract_drawables(&pair, "pair", scratch.path(), &how()).unwrap();
    let single =
        models::extract_drawable(&two_geometries(), "solo", scratch.path(), &how()).unwrap();

    assert_eq!(
        models::entries(&source(&dictionary)).unwrap(),
        ["alpha", "beta"]
    );
    assert!(models::entries(&source(&single)).unwrap().is_empty());
}
