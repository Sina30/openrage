//! The install half of the D16 target in the engine's names, holding only the
//! lines built today; `src/doc_target_sample.rs` names the package owing each of the rest.

use or_io::gta5::CrFrameFilterDictionary;
use or_title::Gta5;

/// Runs the built lines against the install; `mods/` is relative to the working directory.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let gta = Gta5::find()?;
    let filters: CrFrameFilterDictionary = gta.open("player.yfd")?;
    gta.save_to_mod(&filters, "mods/player.yfd")?;
    Ok(())
}
