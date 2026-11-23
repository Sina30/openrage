//! `openrage-cli deflate`: an RSC7 resource's decompressed pages, virtual then
//! physical, with the RSC7 header stripped.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use or_io::container::resource::{ResourceEnvelope, Rsc7};

fn cmd_info(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    if data.len() < 16 {
        return Err("File too small to be a valid RSC7 resource".to_string());
    }

    let seal = Rsc7::peek(&data).map_err(|e| format!("Failed to parse RSC7 header: {}", e))?;

    println!("RSC7 Resource Header");
    println!("====================");
    println!("File:           {}", path.display());
    println!("File size:      {} bytes", data.len());
    println!();
    for (label, value) in seal.describe() {
        println!("{label:<15} {value}");
    }
    println!();

    let virtual_size = seal.virtual_size();
    let physical_size = seal.physical_size();
    let total_size = seal.total_size();

    println!("Decoded sizes:");
    println!(
        "  Virtual pages:  {} bytes ({:.2} KB)",
        virtual_size,
        virtual_size as f64 / 1024.0
    );
    println!(
        "  Physical pages: {} bytes ({:.2} KB)",
        physical_size,
        physical_size as f64 / 1024.0
    );
    println!(
        "  Total:          {} bytes ({:.2} KB)",
        total_size,
        total_size as f64 / 1024.0
    );
    println!();

    let compressed_size = data.len() - 16;
    let ratio = (compressed_size as f64 / total_size as f64) * 100.0;
    println!("Compression:");
    println!("  Compressed:     {} bytes", compressed_size);
    println!("  Decompressed:   {} bytes", total_size);
    println!("  Ratio:          {:.1}%", ratio);

    Ok(())
}

fn cmd_deflate(input: &Path, output: &Path) -> Result<(), String> {
    println!("Deflating: {} -> {}", input.display(), output.display());

    let data = fs::read(input).map_err(|e| format!("Failed to read input: {}", e))?;
    println!("Input size: {} bytes", data.len());

    let resource = Rsc7::open(&data).map_err(|e| format!("Failed to decompress: {}", e))?;

    println!("RSC7 version: {}", resource.version);
    println!("Virtual size: {} bytes", resource.virtual_size);
    println!("Physical size: {} bytes", resource.physical_size);

    println!("Decompressed size: {} bytes", resource.data.len());

    fs::write(output, &resource.data).map_err(|e| format!("Failed to write output: {}", e))?;

    println!();
    println!(
        "Success! Decompressed data written to: {}",
        output.display()
    );
    println!();
    println!("Data layout:");
    println!(
        "  Bytes 0x{:08X} - 0x{:08X}: Virtual pages (CPU data)",
        0, resource.virtual_size as usize
    );
    println!(
        "  Bytes 0x{:08X} - 0x{:08X}: Physical pages (GPU data)",
        resource.virtual_size as usize,
        resource.virtual_size as usize + resource.physical_size as usize
    );

    Ok(())
}

fn cmd_split(input: &Path, output_base: &Path) -> Result<(), String> {
    println!("Splitting: {}", input.display());

    let data = fs::read(input).map_err(|e| format!("Failed to read input: {}", e))?;

    let resource = Rsc7::open(&data).map_err(|e| format!("Failed to decompress: {}", e))?;

    let virtual_size = resource.virtual_size as usize;
    let _physical_size = resource.physical_size as usize;

    let virtual_path = output_base.with_extension("virtual.bin");
    let physical_path = output_base.with_extension("physical.bin");

    let virtual_data = &resource.data[..virtual_size];
    let physical_data = &resource.data[virtual_size..];

    fs::write(&virtual_path, virtual_data)
        .map_err(|e| format!("Failed to write virtual data: {}", e))?;
    println!(
        "Virtual pages:  {} -> {} bytes",
        virtual_path.display(),
        virtual_data.len()
    );

    fs::write(&physical_path, physical_data)
        .map_err(|e| format!("Failed to write physical data: {}", e))?;
    println!(
        "Physical pages: {} -> {} bytes",
        physical_path.display(),
        physical_data.len()
    );

    println!();
    println!(
        "Success! Split into {} and {}",
        virtual_path.display(),
        physical_path.display()
    );

    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("RAGE Resource Deflate Tool - Extract raw data from RSC7 files");
    eprintln!();
    eprintln!("Usage:");
    eprintln!(
        "  {} <input> <output>          Decompress to single file",
        program
    );
    eprintln!(
        "  {} --info <file>             Show header information",
        program
    );
    eprintln!(
        "  {} --split <input> <base>    Split into virtual/physical files",
        program
    );
    eprintln!();
    eprintln!("Output format:");
    eprintln!("  The output is raw decompressed RAGE resource data without the RSC7 header.");
    eprintln!("  - First N bytes: Virtual (CPU/system) pages");
    eprintln!("  - Remaining:     Physical (GPU/graphics) pages");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} textures.ytd textures.bin", program);
    eprintln!("  {} --info particles.ypt", program);
    eprintln!("  {} --split model.ydr model", program);
    eprintln!("    -> Creates model.virtual.bin and model.physical.bin");
}

pub fn run(args: &[String]) -> ExitCode {
    let program = "openrage-cli deflate";

    if args.is_empty() {
        print_usage(program);
        return ExitCode::from(2);
    }

    let result = match args[0].as_str() {
        "--info" | "-i" => {
            if args.len() < 2 {
                eprintln!("Error: Missing file argument");
                eprintln!("Usage: {} --info <file>", program);
                return ExitCode::from(2);
            }
            cmd_info(Path::new(&args[1]))
        }
        "--split" | "-s" => {
            if args.len() < 3 {
                eprintln!("Error: Missing input/output arguments");
                eprintln!("Usage: {} --split <input> <output_base>", program);
                return ExitCode::from(2);
            }
            cmd_split(Path::new(&args[1]), Path::new(&args[2]))
        }
        "-h" | "--help" | "help" => {
            print_usage(program);
            return ExitCode::SUCCESS;
        }
        input => {
            if args.len() < 2 {
                eprintln!("Error: Missing output argument");
                eprintln!("Usage: {} <input> <output>", program);
                return ExitCode::from(2);
            }
            cmd_deflate(Path::new(input), Path::new(&args[1]))
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(1)
        }
    }
}
