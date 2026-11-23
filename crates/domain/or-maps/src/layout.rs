//! Where the file of a map source lands inside a resource.
//!
//! A map file, a type file, a model and a texture dictionary cook to `stream/<name>.<extension>`, as every streamed file does: that is
//! the one place a FiveM resource has for them. Where a singleplayer DLC would put them is measured and not built: the install keeps
//! the map files of its packs in nested archives that `content.xml` registers as `RPF_FILE` with `CONTENTS_DLC_MAP_DATA` (312 entries
//! of 123 documents of the owner's Legacy install, `where_the_install_keeps_its_map_files`), and registers no `.ymap` itself, so a cell
//! of the Builds window says *Yes* for a DLC only when a wave builds that archive.

/// The folder FiveM streams from.
pub const STREAM: &str = "stream";

/// Where a cooked file called `file` (`harbour.ymap`) lands inside the resource.
pub fn stream_path(file: &str) -> String {
    format!("{STREAM}/{file}")
}

/// The `data_file` type a manifest registers a type file under, the one the retail packs register theirs under.
pub const TYPE_FILE: &str = "DLC_ITYP_REQUEST";
