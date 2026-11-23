use super::head::embedded;
use super::*;

/// The pixels of the built drawable's embedded texture, as the head reads them.
fn pixels(img: &Image, phys: &[u8]) -> Vec<u8> {
    let head = read_drawable_head(&img.bytes, Some(phys), false).expect("the head reads");
    let [Ok(t)] = head.textures.as_slice() else {
        panic!("one texture, not {:?}", head.textures);
    };
    let read = t.texture.texture.as_ref().expect("its pixels read");
    read.pixels.clone()
}

#[test]
fn an_embedded_texture_is_written_into_its_own_tiles_and_reads_back_as_given() {
    let (img, mut phys, _) = embedded(TEXTURE_KEY);
    let before = phys.clone();
    let read = pixels(&img, &phys);
    let new: Vec<u8> = (0..read.len()).map(|i| (i * 7 + 3) as u8).collect();
    write_embedded_texture(&img.bytes, &mut phys, false, TEXTURE_KEY, &new).expect("written");
    assert_eq!(pixels(&img, &phys), new);
    assert_eq!(phys.len(), before.len());
    // The texture's 0x2000 bytes are its own: the geometry's data after them is as it was.
    assert!(phys[0x2000..] == before[0x2000..]);
    let d = read_drawable(&img.bytes, Some(&phys)).expect("the drawable reads");
    let g = d.levels[0].as_ref().expect("level 0")[0].geometries[0]
        .as_ref()
        .expect("the geometry reads");
    assert_eq!(g.triangles, [[0, 1, 2]]);

    // The same pixels write nothing; another key or another length is refused and writes nothing.
    let written = phys.clone();
    write_embedded_texture(&img.bytes, &mut phys, false, TEXTURE_KEY, &new).expect("unchanged");
    assert!(phys == written);
    let refused = [
        write_embedded_texture(&img.bytes, &mut phys, false, TEXTURE_KEY ^ 1, &read),
        write_embedded_texture(&img.bytes, &mut phys, false, TEXTURE_KEY, &read[1..]),
    ];
    assert!(refused.iter().all(|r| r.is_err()));
    assert!(phys == written);
}

#[test]
fn a_drawable_that_embeds_no_texture_has_none_written() {
    let (img, mut phys, _) = drawable([1.0, 1.0, 0.0]);
    let before = phys.clone();
    assert!(write_embedded_texture(&img.bytes, &mut phys, false, TEXTURE_KEY, &[0; 64]).is_err());
    assert!(phys == before);
}
