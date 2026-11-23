//! The file half of the D16 target in the engine's names, holding only the
//! lines built today; `src/doc_target_sample.rs` names the package owing each of the rest.

use std::path::Path;

/// Runs the built lines on `path`, a Legacy `.ytd`, and saves it in place; `out/` and `decal.png` are relative to the working directory.
pub fn run(path: &Path) -> Result<(), or_io::Error> {
    use or_io::gta5::gen8::GrcTexture;
    use or_io::gta5::*;
    use or_io::Asset;

    match or_io::open(path)? {
        Asset::Drawable(d) => println!("{}", d.name),
        Asset::TextureDict(t) => println!("{} textures", t.entries.len()),
        other => println!("{:?}", other.origin().path()),
    }

    let mut ytd = PgDictionary::<GrcTexture>::open(path)?;
    for tex in ytd.entries.iter().flatten() {
        tex.to_png(format!("out/{}.png", tex.name))?;
    }
    ytd.insert(GrcTexture::from_png("decal.png")?);
    ytd.save()?;
    Ok(())
}

/// Runs the built drawable lines on `police.ydr`, a Legacy `.ydr` in the working directory whose first level holds two models or more, and saves it in place.
pub fn drawable() -> Result<(), or_io::Error> {
    use or_io::gta5::*;

    let mut car = GtaDrawable::open("police.ydr")?;
    println!("{}", car.name);
    println!("{:?}", car.lod_group.lod_distances);
    if let Some(skel) = &car.skeleton {
        for bone in &skel.bones {
            println!("{} {:?}", bone.name, bone.translation);
        }
    }
    for model in &car.lod_group.lods[0].models {
        for geom in &model.geometries {
            println!("{} verts", geom.vertex_buffer.vertex_count());
        }
    }
    let mesh = car.lod_group.lods[0].models[0].geometries[0].mesh();
    println!("{} triangles", mesh.indices.len() / 3);
    car.lod_group.lod_distances[0] = 45.0;
    car.name = "police2".into();
    car.lod_group.lods[0].models.remove(1);
    car.save()?;
    Ok(())
}
