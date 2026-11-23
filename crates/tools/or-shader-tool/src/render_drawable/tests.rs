use or_io::gta5::{File as _, PgDictionary};

use or_renderer::game_shaders::frame::Rendered;

use super::*;

#[test]
fn a_loose_file_reads_by_its_path_and_a_missing_one_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let path = dir.path().join("a.ydr");
    std::fs::write(&path, [7, 8, 9]).expect("the file is written");
    let found = read_drawable(&path, None).expect("it reads");
    assert_eq!(
        (found.name.as_str(), found.bytes),
        (path.to_str().unwrap_or(""), vec![7, 8, 9])
    );
    assert!(found.beside.is_empty());
    assert!(read_drawable(&dir.path().join("none.ydr"), None).is_err());
}

#[test]
fn a_name_in_a_folder_that_is_no_install_is_refused() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    assert!(read_drawable(dir.path(), Some("a.ydr")).is_err());
}

fn dictionary_of(name: &str, bgra: [u8; 4]) -> Vec<u8> {
    dictionary_in(name, bgra, 0)
}

fn dictionary_in(name: &str, bgra: [u8; 4], info_bits: u8) -> Vec<u8> {
    use or_io::gta5::gen8::GrcTexture;
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(GrcTexture {
        name: name.into(),
        width: 2,
        height: 2,
        depth: 1,
        mip_stride: 8,
        // Direct3D 9 `A8R8G8B8`, whose bytes lie blue first
        format: 21,
        mip_count: 1,
        info_bits,
        backing_store: bgra.repeat(4),
        ..Default::default()
    });
    dictionary.to_bytes().expect("the dictionary encodes")
}

#[test]
fn a_dictionary_reads_as_its_textures_by_lower_case_name_in_rgba() {
    let textures =
        dictionary_textures(&dictionary_of("Prop_Bench_01a", [1, 2, 3, 255])).expect("it reads");
    assert_eq!(textures.keys().collect::<Vec<_>>(), ["prop_bench_01a"]);
    let image = &textures["prop_bench_01a"];
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(&image.rgba[..4], &[3, 2, 1, 255]);
}

#[test]
fn bytes_that_are_no_dictionary_are_refused() {
    assert!(dictionary_textures(&[1, 2, 3, 4]).is_err());
}

#[test]
fn a_ytd_beside_a_loose_drawable_is_read() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    std::fs::write(dir.path().join("a.ydr"), [7]).expect("the drawable is written");
    std::fs::write(
        dir.path().join("a.ytd"),
        dictionary_of("wall", [1, 2, 3, 255]),
    )
    .expect("the dictionary is written");
    let found = read_drawable(&dir.path().join("a.ydr"), None).expect("it reads");
    assert_eq!(found.beside.keys().collect::<Vec<_>>(), ["wall"]);
}

#[test]
fn a_linear_channel_is_encoded_as_srgb_with_its_ends_fixed() {
    assert_eq!((srgb_of(0), srgb_of(255)), (0, 255));
    // A linear one half is 188 of 255 in sRGB, and the first step above nothing is 13
    assert_eq!(srgb_of(128), 188);
    assert_eq!(srgb_of(1), 13);
    assert!((0..255).all(|c| srgb_of(c) <= srgb_of(c + 1)));
}

#[test]
fn a_render_is_written_with_its_colours_encoded_and_its_alpha_as_it_is() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let path = dir.path().join("out.png");
    let rendered = Rendered {
        size: 1,
        format: wgpu::TextureFormat::Rgba8Unorm,
        bytes: vec![128, 0, 255, 77],
    };
    write_png(rendered, &path).expect("it is written");
    let written = image::open(&path).expect("it reads back").to_rgba8();
    assert_eq!(written.get_pixel(0, 0).0, [188, 0, 255, 77]);
}

#[test]
fn a_texture_states_its_colour_space_and_it_travels_with_the_pixels() {
    let linear = dictionary_textures(&dictionary_in("a", [1, 2, 3, 255], 0)).expect("it reads");
    let srgb = dictionary_textures(&dictionary_in("a", [1, 2, 3, 255], 1)).expect("it reads");
    assert!(!linear["a"].srgb);
    assert!(srgb["a"].srgb);
}

#[test]
fn a_contact_sheet_holds_forty_eight_pictures_row_by_row_and_the_next_starts_a_sheet() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let picture = |n: u8| {
        (
            format!("car_{n}.yft"),
            Rendered {
                size: 2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                bytes: [n, 0, 255 - n, 255].repeat(4),
            },
        )
    };
    let pictures: Vec<_> = (0..50).map(picture).collect();
    assert_eq!(write_sheets(&pictures, true, dir.path()), Ok(2));
    let first = image::open(dir.path().join("sheet_0.png"))
        .expect("the first sheet reads")
        .to_rgba8();
    assert_eq!(first.dimensions(), (16, 12));
    // The tenth picture is the second of the second row, whose cell starts at pixel (2, 2)
    assert_eq!(first.get_pixel(2, 2).0, [9, 0, 246, 255]);
    let second = std::fs::read_to_string(dir.path().join("sheet_1.txt")).expect("the list reads");
    assert_eq!(second, " 0  car_48.yft\n 1  car_49.yft\n");
}
