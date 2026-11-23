//! Where the file of a particle asset lands inside a resource.
//!
//! A `.ypt` cooks to `stream/<name>.ypt`, as every streamed file does. That is the one place a FiveM resource has for it. Where a
//! singleplayer DLC would put it is a question this build does not answer: the install keeps its `.ypt` files inside archives named
//! `ptfx.rpf` (and `ptfx_hi.rpf`, `ptfx_lo.rpf` and, for an Enhanced install, `ptfx_g9.rpf`) in `data/effects/` of the base game, of its
//! update and of every DLC pack, so a cell of the Builds window says *Yes* for a DLC only when a wave builds that archive.

/// The folder FiveM streams from.
pub const STREAM: &str = "stream";

/// Where a cooked file called `file` (`fx_smoke.ypt`) lands inside the resource.
pub fn stream_path(file: &str) -> String {
    format!("{STREAM}/{file}")
}
