//! A query over a pack selects exactly the garments the pack states: by tag, slot, ped, polygon count, a missing level of detail,
//! a copy of another garment, a problem code and a name. A saved query is kept in the pack and selects the same garments after
//! the pack is built and read again, wherever they now stand in the list.

mod common;

use std::path::Path;

use common::{garment, project, put_bytes, ytd, JBIB, LOWR};
use or_source::sample::drawable_dictionary_with_levels as drawables;
use or_wardrobe::measure::{measure, Measure};
use or_wardrobe::query::{forget, save, saved, Query, Subject};
use or_wardrobe::{inspect, plan, plans, Depth, Garment, Ped, Wardrobe};

/// The model of a garment: `vertices` vertices at `levels` levels of detail.
struct Model {
    file: &'static str,
    vertices: usize,
    levels: usize,
}

const MODELS: [Model; 4] = [
    Model {
        file: "models/a.ydd",
        vertices: 90,
        levels: 3,
    },
    Model {
        file: "models/b.ydd",
        vertices: 300,
        levels: 1,
    },
    Model {
        file: "models/a_copy.ydd",
        vertices: 90,
        levels: 3,
    },
    Model {
        file: "models/jeans.ydd",
        vertices: 600,
        levels: 2,
    },
];

fn named(mut g: Garment, label: &str, tags: &[&str]) -> Garment {
    g.label = Some(label.into());
    g.tags = tags.iter().map(|t| t.to_string()).collect();
    g
}

/// Five garments: two wool jackets, a copy of the first, jeans for the female ped, and a jacket whose model is not there.
fn pack(folder: &Path) -> Wardrobe {
    for m in &MODELS {
        put_bytes(folder, m.file, &drawables("x", m.vertices, 1.0, m.levels));
    }
    put_bytes(folder, "textures/a.ytd", &ytd("a"));
    put_bytes(folder, "textures/a_copy.ytd", &ytd("a"));
    put_bytes(folder, "textures/b.ytd", &ytd("b"));
    put_bytes(folder, "textures/jeans.ytd", &ytd("jeans"));
    let mut w = Wardrobe::new("queried");
    w.garments = vec![
        named(
            garment(Ped::Male, JBIB, None, MODELS[0].file, &["textures/a.ytd"]),
            "Wool jacket",
            &["wool", "SinglePlayer"],
        ),
        named(
            garment(Ped::Male, JBIB, None, MODELS[1].file, &["textures/b.ytd"]),
            "Plain jacket",
            &["wool"],
        ),
        named(
            garment(
                Ped::Male,
                JBIB,
                None,
                MODELS[2].file,
                &["textures/a_copy.ytd"],
            ),
            "Wool jacket copy",
            &["singleplayer"],
        ),
        named(
            garment(
                Ped::Female,
                LOWR,
                None,
                MODELS[3].file,
                &["textures/jeans.ytd"],
            ),
            "Jeans",
            &[],
        ),
        named(
            garment(
                Ped::Male,
                JBIB,
                None,
                "models/gone.ydd",
                &["textures/a.ytd"],
            ),
            "Lost jacket",
            &[],
        ),
    ];
    w.write(folder).unwrap();
    w
}

/// The ids selected by `text`, in the pack's order.
fn ids(folder: &Path, text: &str) -> Vec<String> {
    let w = Wardrobe::read(folder).unwrap();
    let facts = inspect(folder, &w, Depth::Measure);
    let planned = plan(&w, &facts, "wardrobe", folder);
    let subject = Subject::new(&w, &planned.allocation, &planned.findings, &planned.counts);
    Query::parse(text)
        .unwrap_or_else(|why| panic!("{text}: {why}"))
        .ids(&subject)
}

/// The ids of the garments of `w` for which `holds` is true.
fn those(w: &Wardrobe, holds: impl Fn(&Garment) -> bool) -> Vec<String> {
    (w.garments.iter().filter(|g| holds(g)))
        .map(|g| g.id.clone())
        .collect()
}

/// The triangles of the high level of detail of the model at `rel`, counted by the reader that opens the file.
fn triangles(folder: &Path, rel: &str) -> Option<u64> {
    let bytes = std::fs::read(folder.join(rel)).ok()?;
    let counted = Measure::of(&measure(rel, &bytes)?)?;
    (counted.drawables.iter())
        .filter_map(|d| d.lods[0])
        .map(|l| l.triangles)
        .reduce(|a, b| a + b)
}

#[test]
fn a_query_selects_exactly_the_garments_the_pack_states() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);

    // The state of the pack, as the pack's own data states it.
    assert_eq!(
        ids(at, "tag:wool"),
        those(&w, |g| g.tags.iter().any(|t| t == "wool"))
    );
    assert_eq!(
        ids(at, "tag:SINGLEPLAYER"),
        those(&w, |g| g
            .tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case("singleplayer"))),
        "a tag is the same tag in any case"
    );
    assert_eq!(
        ids(at, "tag:wool -tag:singleplayer"),
        those(&w, |g| g.tags.contains(&"wool".to_string())
            && !g
                .tags
                .iter()
                .any(|t| t.eq_ignore_ascii_case("singleplayer")))
    );
    assert_eq!(ids(at, "slot:lowr"), those(&w, |g| g.slot == Some(LOWR)));
    assert_eq!(
        ids(at, "ped:female"),
        those(&w, |g| g.ped == Some(Ped::Female))
    );
    assert_eq!(
        ids(at, "slot:jbib ped:male"),
        those(&w, |g| g.slot == Some(JBIB) && g.ped == Some(Ped::Male))
    );

    // What the files state, counted by the reader that opens them.
    let wanted: u64 = 40;
    assert_eq!(
        ids(at, &format!("polys>{wanted}")),
        those(&w, |g| g
            .model
            .as_deref()
            .and_then(|m| triangles(at, m))
            .is_some_and(|t| t > wanted)),
        "the lost jacket has no file to count, so no count term selects it"
    );
    assert_eq!(
        ids(at, "polys<=40"),
        those(&w, |g| g
            .model
            .as_deref()
            .and_then(|m| triangles(at, m))
            .is_some_and(|t| t <= 40))
    );
    let missing = |rel: &str| {
        MODELS
            .iter()
            .find(|m| m.file == rel)
            .is_some_and(|m| m.levels < 3)
    };
    assert_eq!(
        ids(at, "lod:missing"),
        those(&w, |g| g.model.as_deref().is_some_and(missing)),
        "a model with no medium or no low level of detail lacks one"
    );
    assert_eq!(
        ids(at, "-lod:missing slot:jbib"),
        those(&w, |g| g.slot == Some(JBIB)
            && !g.model.as_deref().is_some_and(missing)),
        "a term is turned round with a minus"
    );

    // A copy is the garment that ships what an earlier one ships.
    assert_eq!(ids(at, "is:duplicate"), vec![w.garments[2].id.clone()]);
    assert_eq!(
        ids(at, "duplicate-of:\"wool jacket\""),
        vec![w.garments[2].id.clone()]
    );
    assert!(ids(at, "duplicate-of:jeans").is_empty());

    // A rule's code: the jacket whose model is not there.
    assert_eq!(
        ids(at, "problem:model-missing"),
        vec![w.garments[4].id.clone()]
    );
    assert!(ids(at, "is:problem").contains(&w.garments[4].id));

    // A name, its letters in order.
    assert_eq!(ids(at, "jea"), vec![w.garments[3].id.clone()]);
    assert_eq!(
        ids(at, "jacket tag:wool"),
        those(&w, |g| g
            .model
            .as_deref()
            .is_some_and(|m| m != "models/gone.ydd")
            && g.label.as_deref().is_some_and(|l| l.contains("jacket"))
            && g.tags.contains(&"wool".to_string())),
        "a word and a term both hold"
    );

    // The empty query selects every garment.
    assert_eq!(ids(at, "  "), those(&w, |_| true));
}

#[test]
fn a_term_that_cannot_be_read_is_said_and_never_dropped() {
    for (text, says) in [
        ("colour:red", "not a term"),
        ("polys>lots", "number"),
        ("tag:", "needs a tag"),
        ("slot:hat", "no slot"),
        ("ped:child", "no ped"),
        ("lod:high", "lod:missing"),
        ("tag:\"open", "quote"),
        ("polys:ten", "number"),
    ] {
        let why = Query::parse(text).expect_err(text);
        assert!(why.contains(says), "{text}: {why}");
    }
}

#[test]
fn a_part_of_a_name_is_no_term_and_a_key_a_comparison_or_a_minus_is() {
    for text in ["", "jb03", "red jacket"] {
        assert!(!Query::parse(text).unwrap().has_terms(), "{text}");
    }
    for text in ["tag:wool", "polys>5000", "red slot:jbib", "-wool"] {
        assert!(Query::parse(text).unwrap().has_terms(), "{text}");
    }
}

#[test]
fn a_saved_query_is_data_of_the_pack_and_selects_the_same_garments_after_a_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let mut w = pack(at);
    // Without the jacket whose model is missing, which stops a build.
    w.garments.truncate(4);
    save(&mut w, "Singleplayer wool", "tag:singleplayer   tag:wool").unwrap();
    save(&mut w, "Female", "ped:female").unwrap();
    // The same name in another case is the same query: it is replaced and keeps its place.
    save(&mut w, "FEMALE", "ped:female slot:lowr").unwrap();
    w.write(at).unwrap();

    let text = std::fs::read_to_string(at.join("wardrobe.json")).unwrap();
    assert!(
        text.contains("\"query\": \"tag:singleplayer tag:wool\""),
        "a plain line that diffs well: {text}"
    );
    let before = ids(at, &saved(&w, "singleplayer WOOL").unwrap().query);
    assert_eq!(before, vec![w.garments[0].id.clone()]);

    // Rebuilt: a plan, a build and a read of the pack from the disk.
    let project = project(at, &w, "queried_pack");
    let plan = plans(&project, "fivem").unwrap().remove(0);
    let out = tempfile::tempdir().unwrap();
    or_project::build(&plan, out.path()).expect("it builds");
    let mut again = Wardrobe::read(at).unwrap();
    assert_eq!(again.queries, w.queries, "a build writes no query away");
    let kept = saved(&again, "Singleplayer wool").unwrap().query.clone();
    assert_eq!(ids(at, &kept), before);

    // Reordered: the same garments, wherever they now stand.
    again.garments.reverse();
    again.write(at).unwrap();
    assert_eq!(ids(at, &kept), before);
    assert_eq!(
        ids(at, "ped:female slot:lowr"),
        vec![w.garments[3].id.clone()]
    );

    // A query that does not read is not kept, and a name is needed; a saved query can be forgotten.
    let mut w = again;
    assert!(save(&mut w, "Bad", "colour:red").is_err());
    assert!(save(&mut w, "  ", "tag:wool").is_err());
    assert!(save(&mut w, "Empty", "   ").is_err());
    assert_eq!(w.queries.len(), 2);
    forget(&mut w, "female").unwrap();
    assert!(forget(&mut w, "female").is_err());
    assert_eq!(w.queries.len(), 1);
}

#[test]
fn a_model_that_is_a_source_is_counted_from_its_glb_as_the_file_it_came_from_is() {
    // Models as a project holds them: dropped game files, decompiled into sources, which no cook is needed to count.
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    or_wardrobe::edit::create(at, "sourced").unwrap();
    let drop = at.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let c = "mp_m_freemode_01_mp_m_sourced";
    let shapes = [(90usize, 3usize), (300, 1), (600, 2), (30, 3)];
    let mut counted: Vec<(Option<u64>, bool)> = Vec::new();
    for (n, (vertices, levels)) in shapes.iter().enumerate() {
        let bytes = drawables("x", *vertices, n as f32, *levels);
        // What the reader that opens the compiled file says, before it is a source.
        let said = Measure::of(&measure(&format!("m{n}.ydd"), &bytes).unwrap()).unwrap();
        counted.push((
            said.drawables[0].lods[0].map(|l| l.triangles),
            said.drawables[0].lods[1].is_none() || said.drawables[0].lods[2].is_none(),
        ));
        std::fs::write(drop.join(format!("{c}^jbib_{n:03}_u.ydd")), bytes).unwrap();
        std::fs::write(
            drop.join(format!("{c}^jbib_diff_{n:03}_a_uni.ytd")),
            ytd(&format!("t{n}")),
        )
        .unwrap();
    }
    or_wardrobe::edit::add_clothes(at, &[drop]).unwrap();
    let w = Wardrobe::read(at).unwrap();
    assert!(
        w.garments.iter().all(|g| g
            .model
            .as_deref()
            .is_some_and(|m| m.ends_with(".drawables.src"))),
        "every model is a source: {:?}",
        w.garments.iter().map(|g| &g.model).collect::<Vec<_>>()
    );
    let planned = plan(&w, &inspect(at, &w, Depth::Measure), "wardrobe", at);
    for (n, garment) in w.garments.iter().enumerate() {
        let place = planned.allocation.of(n).unwrap().position as usize;
        assert_eq!(
            (planned.counts[n].triangles, planned.counts[n].lod_missing),
            counted[place],
            "{:?} at place {place}",
            garment.model
        );
    }
    // The terms select what the files state, for a source as for a compiled file.
    let subject = Subject::new(&w, &planned.allocation, &planned.findings, &planned.counts);
    let wanted: Vec<usize> = (0..w.garments.len())
        .filter(|&n| {
            let place = planned.allocation.of(n).unwrap().position as usize;
            counted[place].0.is_some_and(|t| t > 40) && counted[place].1
        })
        .collect();
    assert!(!wanted.is_empty() && wanted.len() < w.garments.len());
    assert_eq!(
        Query::parse("polys>40 lod:missing")
            .unwrap()
            .select(&subject),
        wanted
    );
}

#[test]
fn a_minus_in_front_of_a_word_leaves_out_the_garments_that_word_finds() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let w = pack(at);
    let found = ids(at, "wool");
    assert!(
        !found.is_empty() && found.len() < w.garments.len(),
        "the word finds some garments and not all: {found:?}"
    );
    assert_eq!(
        ids(at, "-wool"),
        those(&w, |g| !found.contains(&g.id)),
        "the garments the word does not find"
    );
    assert_eq!(
        ids(at, "-wool jacket"),
        vec![w.garments[4].id.clone()],
        "a word and a word turned round both hold"
    );
    assert_eq!(
        ids(at, "-wool -jea"),
        vec![w.garments[4].id.clone()],
        "each word turned round leaves its own garments out"
    );
}

/// Rewrite the glb at `file` with its JSON changed by `edit`, the binary chunk as it was.
fn rewrite_glb(file: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let bytes = std::fs::read(file).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    edit(&mut json);
    let mut text = serde_json::to_vec(&json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let rest = &bytes[20 + json_len..];
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((20 + text.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(rest);
    std::fs::write(file, out).unwrap();
}

#[test]
fn a_model_a_modelling_tool_exported_again_is_counted_as_the_cook_reads_it() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    or_wardrobe::edit::create(at, "retooled").unwrap();
    let drop = at.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let c = "mp_m_freemode_01_mp_m_retooled";
    // 300 vertices at two levels of detail: a high one and a medium one, and no low one.
    std::fs::write(
        drop.join(format!("{c}^jbib_000_u.ydd")),
        drawables("x", 300, 0.0, 2),
    )
    .unwrap();
    std::fs::write(drop.join(format!("{c}^jbib_diff_000_a_uni.ytd")), ytd("t")).unwrap();
    or_wardrobe::edit::add_clothes(at, &[drop]).unwrap();
    let counted = || {
        let w = Wardrobe::read(at).unwrap();
        let planned = plan(&w, &inspect(at, &w, Depth::Measure), "wardrobe", at);
        (planned.counts[0].triangles, planned.counts[0].lod_missing)
    };
    let made = counted();
    assert!(
        made.0.is_some_and(|t| t > 0) && made.1,
        "OpenRage's own glb: {made:?}"
    );

    let w = Wardrobe::read(at).unwrap();
    let source = at.join(w.garments[0].model.as_deref().unwrap());
    let glbs: Vec<_> = std::fs::read_dir(source.join(or_source::models::DIR))
        .unwrap()
        .flatten()
        .map(|e| e.path().join(or_source::models::MODEL))
        .collect();
    assert_eq!(glbs.len(), 1);

    // A modelling tool drops what a primitive carries and keeps its node's extras and the names of its meshes.
    rewrite_glb(&glbs[0], |json| {
        for mesh in json["meshes"].as_array_mut().unwrap() {
            for primitive in mesh["primitives"].as_array_mut().unwrap() {
                primitive.as_object_mut().unwrap().remove("extras");
            }
        }
    });
    assert_eq!(counted(), made, "the level a node states");

    // Or the extras of its nodes as well, and a mesh named for its level says which it is.
    rewrite_glb(&glbs[0], |json| {
        for node in json["nodes"].as_array_mut().unwrap() {
            node.as_object_mut().unwrap().remove("extras");
        }
    });
    assert_eq!(counted(), made, "the level a mesh's name states");
}
