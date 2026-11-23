//! A replacement writes over no file and no image beside a compiled file makes a texture, so the two documents that once said
//! otherwise say what happens now.

use std::path::Path;

fn doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_guide_does_not_send_a_person_to_save_an_image_beside_a_texture_file() {
    let guide = doc("guides/Workbench_Guide.md");
    assert!(
        !guide.contains("save an image beside the texture file"),
        "a saved image is cooked from the textures source's own folder now"
    );
    assert!(
        guide.contains(".textures.src"),
        "the guide names the folder"
    );
}

#[test]
fn the_parity_table_does_not_say_a_replacement_keeps_the_original_bytes() {
    let parity = doc("direction/Wardrobe_Parity.md");
    let row = parity
        .lines()
        .find(|line| line.starts_with("| Pristine originals kept"))
        .expect("the row is in the table");
    assert!(
        !row.contains("The first replacement of a texture keeps the original"),
        "a replacement is a new source and keeps nothing: {row}"
    );
    assert!(row.contains("stays where it was"), "{row}");
}
