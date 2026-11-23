//! A file of names the user picks, read as a job: each line names a key of an open GTA VI pack its
//! hash keys, or a name the texture naming rules give from it as a drawable's stem does.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use super::gta6_texture_naming::{NamingEnded, TextureNaming};
use super::{AppState, DocPayload};

/// The most bytes of a names file read.
const MOST_BYTES: u64 = 256 << 20;

impl AppState {
    /// Name the textures of every open GTA VI pack from the lines of the file at `path`, on a worker.
    pub fn load_texture_names(&mut self, path: &Path) {
        let keys: HashSet<u64> = (self.documents.iter())
            .filter_map(|d| match &d.payload {
                DocPayload::Ttpf(ttpf) => Some(ttpf.pack.entries.iter()),
                _ => None,
            })
            .flatten()
            .filter(|e| e.on_disk && e.key != 0)
            .map(|e| e.key)
            .collect();
        if keys.is_empty() {
            self.show_warning_toast(tr!(
                "Open a GTA VI texture pack first: a names file names the keys of the packs open."
            ));
            return;
        }
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() <= MOST_BYTES => {}
            Ok(_) => {
                self.show_error_toast(tr!(
                    "{path} is longer than the {count} MiB a names file may be",
                    path = path.display(),
                    count = MOST_BYTES >> 20
                ));
                return;
            }
            Err(e) => {
                self.show_error_toast(format!("{}: {e}", path.display()));
                return;
            }
        }
        let path = path.to_path_buf();
        let doing = tr!("Naming GTA VI textures from {file}", file = path.display());
        let run = TextureNaming::run(doing, move |cancel, progress| {
            let mut bytes = Vec::new();
            let read = std::fs::File::open(&path)
                .and_then(|file| file.take(MOST_BYTES).read_to_end(&mut bytes));
            if let Err(error) = read {
                tracing::warn!(%error, path = %path.display(), "the names file was not read");
                return NamingEnded::Loaded(0);
            }
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = (text.lines().map(str::trim))
                .filter(|line| !line.is_empty())
                .collect();
            let mut named = 0;
            for (done, line) in lines.iter().enumerate() {
                if cancel.is_set() {
                    return NamingEnded::Stopped;
                }
                progress.reached(done as u64, lines.len() as u64);
                named += super::ttpf_document::learn_rule_names(line, &keys);
            }
            NamingEnded::Loaded(named)
        });
        self.texture_naming.push(run);
    }
}

#[cfg(test)]
#[path = "a_pack_s_textures_are_named_from_a_names_file.rs"]
mod tests;
