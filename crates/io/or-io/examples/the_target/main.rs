//! The D16 target's file half: `cargo run -p or-io --example the_target -- <file.ytd>`
//! writes each texture to `out/`, inserts `decal.png` and saves the `.ytd` in place,
//! then edits and saves `police.ydr` where the working directory holds one.

mod sample;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: the_target <path to a Legacy .ytd>, run where decal.png is")?;
    std::fs::create_dir_all("out")?;
    sample::run(std::path::Path::new(&path))?;
    if std::path::Path::new("police.ydr").exists() {
        sample::drawable()?;
    }
    Ok(())
}
