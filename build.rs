use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

type IconFrame = (u32, image::ImageBuffer<image::Rgba<u8>, Vec<u8>>);

use image::imageops::FilterType;
use image::{ImageBuffer, Rgba};

const LOGO_SOURCE: &str = "assets/brand/openrage-symbol.png";
const MASTER_SIZE: u32 = 256;
const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
    let png_path = out_dir.join("app_logo_256.png");
    let ico_path = out_dir.join("app_logo.ico");

    let source = image::ImageReader::open(LOGO_SOURCE)
        .unwrap_or_else(|err| panic!("open {LOGO_SOURCE}: {err}"))
        .with_guessed_format()
        .unwrap_or_else(|err| panic!("guess format for {LOGO_SOURCE}: {err}"))
        .decode()
        .unwrap_or_else(|err| panic!("decode {LOGO_SOURCE}: {err}"))
        .into_rgba8();
    let source = pad_to_square(&source);

    let master = resize_square(&source, MASTER_SIZE);
    master
        .save(&png_path)
        .unwrap_or_else(|err| panic!("write {}: {err}", png_path.display()));

    let frames: Vec<IconFrame> = ICO_SIZES
        .iter()
        .map(|&size| {
            let img = if size == MASTER_SIZE {
                master.clone()
            } else {
                resize_square(&source, size)
            };
            (size, img)
        })
        .collect();
    write_ico(&frames, &ico_path).expect("write app_logo.ico");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={LOGO_SOURCE}");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("ico path is utf8"));
        res.set("ProductName", "OpenRage");
        res.set("FileDescription", "OpenRage");
        res.set("CompanyName", "OpenRage contributors");
        res.set(
            "LegalCopyright",
            "Licensed under MIT. Third-party licenses: LICENSE-third-party.md",
        );
        if let Err(err) = res.compile() {
            println!("cargo:warning=winresource compile failed: {err}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = ico_path;
    }
}

// The symbol is not square; resizing it straight to a square would squash it.
fn pad_to_square(source: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let (w, h) = source.dimensions();
    if w == h {
        return source.clone();
    }
    let side = w.max(h);
    let mut canvas = ImageBuffer::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    image::imageops::overlay(
        &mut canvas,
        source,
        i64::from((side - w) / 2),
        i64::from((side - h) / 2),
    );
    canvas
}

fn resize_square(
    source: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    size: u32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    if source.width() == size && source.height() == size {
        return source.clone();
    }
    image::imageops::resize(source, size, size, FilterType::Lanczos3)
}

fn write_ico(frames: &[IconFrame], path: &PathBuf) -> std::io::Result<()> {
    use image::codecs::png::PngEncoder;
    use image::ExtendedColorType;
    use image::ImageEncoder;

    let mut entries: Vec<(u32, Vec<u8>)> = Vec::with_capacity(frames.len());
    for (size, img) in frames {
        let mut png_bytes: Vec<u8> = Vec::new();
        PngEncoder::new(&mut png_bytes)
            .write_image(img.as_raw(), *size, *size, ExtendedColorType::Rgba8)
            .map_err(std::io::Error::other)?;
        entries.push((*size, png_bytes));
    }

    let f = File::create(path)?;
    let mut w = BufWriter::new(f);

    // ICONDIR header
    w.write_all(&[0u8, 0])?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&(entries.len() as u16).to_le_bytes())?;

    let dir_size = 6 + entries.len() * 16;
    let mut offset = dir_size as u32;
    for (size, bytes) in &entries {
        // ICO byte 0 represents 256px
        let w_byte = if *size >= 256 { 0u8 } else { *size as u8 };
        let h_byte = w_byte;
        w.write_all(&[w_byte, h_byte])?;
        w.write_all(&[0u8])?;
        w.write_all(&[0u8])?;
        w.write_all(&1u16.to_le_bytes())?;
        w.write_all(&32u16.to_le_bytes())?;
        w.write_all(&(bytes.len() as u32).to_le_bytes())?;
        w.write_all(&offset.to_le_bytes())?;
        offset += bytes.len() as u32;
    }
    for (_, bytes) in &entries {
        w.write_all(bytes)?;
    }
    w.flush()?;
    Ok(())
}
