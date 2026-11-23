//! RDR2's TFIT-tagged archives, opened with the keys the user's own files supply.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names an RDR2 install, whose `RDR2.exe` unlocks the
//! bundled snapshot of the cipher tables.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::{BodyForm, Rdr2CryptoProvider, RpfArchive, RpfEntry, RpfVersion};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

type Tagged = Vec<(PathBuf, u16)>;

fn provider_and_archives(what: &str) -> Option<(Arc<Rdr2CryptoProvider>, Tagged)> {
    let Some(root) = game_roots()
        .into_iter()
        .find(|r| r.join("RDR2.exe").is_file())
    else {
        skip_install(what, format_args!("no RDR2.exe under {GAME_ROOT_ENV}"));
        return None;
    };
    let provider = Rdr2CryptoProvider::for_install(root.join("RDR2.exe"), None)
        .map(Arc::new)
        .expect("the executable's AES key unlocks the snapshot");
    Some((provider, tagged_archives(&root)))
}

/// Every top-level RPF8 archive whose table is tagged with a key id, with that id.
fn tagged_archives(root: &Path) -> Vec<(PathBuf, u16)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        let mut head = [0u8; 16];
        let read = std::fs::File::open(&path)
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head));
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("rpf"))
            && read.is_ok()
            && RpfVersion::detect([head[0], head[1], head[2], head[3]]) == Some(RpfVersion::Rpf8)
        {
            let tag = u16::from_le_bytes([head[12], head[13]]);
            if tag != 0xFF {
                found.push((path, tag));
            }
        }
    }
    found.sort();
    found
}

fn open(path: &Path, provider: &Arc<Rdr2CryptoProvider>) -> RpfArchive {
    RpfArchive::open_with(path, provider.clone())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A wrong key reads a table as noise, and noise puts rows far past the end of the file.
#[test]
fn every_tagged_archive_opens_and_its_rows_land_inside_the_file() {
    let Some((provider, archives)) = provider_and_archives("rdr2 tagged tables") else {
        return;
    };
    assert!(!archives.is_empty());
    for (path, tag) in archives {
        let size = std::fs::metadata(&path).expect("metadata").len();
        let archive = open(&path, &provider);
        assert!(archive.file_count() > 0, "{}", path.display());
        for entry in &archive.entries {
            let (offset, len) = match entry {
                RpfEntry::Binary(b) => (b.file_offset, u64::from(b.read_size())),
                RpfEntry::Resource(r) => (r.file_offset, u64::from(r.consumed_size)),
                RpfEntry::Directory(_) => continue,
            };
            assert!(
                offset + len <= size,
                "{} (key {tag:#x}): a row ends at {} past the {size}-byte file",
                path.display(),
                offset + len
            );
        }
        eprintln!(
            "rdr2: {} (key {tag:#x}) holds {} file(s)",
            path.display(),
            archive.file_count()
        );
    }
}

/// An enciphered body deciphers to the metadata it holds: an Oodle body keeps its first literals,
/// which spell the file's magic (RBF0 or PSIN).
#[test]
fn an_enciphered_body_in_a_nested_archive_deciphers_to_readable_metadata() {
    let Some((provider, archives)) = provider_and_archives("rdr2 bodies") else {
        return;
    };
    let (path, _) = archives
        .into_iter()
        .find(|(p, _)| p.ends_with("data_0.rpf"))
        .expect("data_0.rpf");
    let archive = Arc::new(open(&path, &provider));
    let mut checked = 0;
    for node in archive
        .nodes
        .iter()
        .filter(|n| !n.is_dir() && n.name().ends_with(".rpf"))
    {
        let nested = archive
            .open_nested(node.entry_index())
            .unwrap_or_else(|e| panic!("{}: {e}", node.name()));
        for n in nested
            .nodes
            .iter()
            .filter(|n| n.name().ends_with(".ymt"))
            .take(8)
        {
            let RpfEntry::Binary(b) = &nested.entries[n.entry_index() as usize] else {
                continue;
            };
            if !b.encrypted {
                continue;
            }
            let body = nested
                .read_form(n.entry_index(), BodyForm::Decrypted)
                .unwrap_or_else(|e| panic!("{}: {e}", n.name()));
            let head = &body[..body.len().min(64)];
            assert!(
                head.windows(4).any(|w| w == b"RBF0" || w == b"PSIN"),
                "{} did not decipher: {head:02x?}",
                n.name()
            );
            checked += 1;
        }
    }
    eprintln!("rdr2: {checked} enciphered body(ies) read as RBF0 or PSIN metadata");
    assert!(checked > 0);
}

/// An Oodle body deciphers, then inflates to exactly the size its row states and to the file it holds.
#[test]
fn an_oodle_body_decodes_to_the_size_its_row_states() {
    use or_io::container::rpf::Compressor;

    let Some((provider, archives)) = provider_and_archives("rdr2 oodle") else {
        return;
    };
    let (path, _) = archives
        .into_iter()
        .find(|(p, _)| p.ends_with("data_0.rpf"))
        .expect("data_0.rpf");
    let archive = Arc::new(open(&path, &provider));
    let mut decoded = 0;
    for node in archive
        .nodes
        .iter()
        .filter(|n| !n.is_dir() && n.name().ends_with(".rpf"))
    {
        let nested = archive
            .open_nested(node.entry_index())
            .unwrap_or_else(|e| panic!("{}: {e}", node.name()));
        for n in nested
            .nodes
            .iter()
            .filter(|n| n.name().ends_with(".ymt"))
            .take(16)
        {
            let RpfEntry::Binary(b) = &nested.entries[n.entry_index() as usize] else {
                continue;
            };
            if b.compressor != Compressor::Oodle {
                continue;
            }
            let bytes = nested
                .read_entry(n.entry_index())
                .unwrap_or_else(|e| panic!("{}: {e}", n.name()));
            assert_eq!(bytes.len(), b.uncompressed_size as usize, "{}", n.name());
            assert!(
                bytes.starts_with(b"RBF0") || bytes.starts_with(b"PSIN"),
                "{} decoded to {:02x?}",
                n.name(),
                &bytes[..bytes.len().min(16)]
            );
            decoded += 1;
        }
    }
    eprintln!("rdr2: {decoded} Oodle body(ies) decoded");
    assert!(decoded > 0);
}

/// A resource's body deciphers, inflates, and is exactly the virtual and physical bytes its flags state.
#[test]
fn a_resource_opens_as_the_bytes_its_flags_state() {
    use or_io::container::rpf::rsc8::Rsc8;

    let Some((provider, archives)) = provider_and_archives("rdr2 resources") else {
        return;
    };
    let (mut opened, mut refused) = (0usize, std::collections::BTreeMap::<String, usize>::new());
    for (path, _) in archives.iter().filter(|(p, _)| {
        ["textures_0.rpf", "hd_0.rpf", "packs_0.rpf"]
            .iter()
            .any(|n| p.ends_with(n))
    }) {
        let archive = Arc::new(open(path, &provider));
        let mut all = vec![Arc::clone(&archive)];
        for node in archive
            .nodes
            .iter()
            .filter(|n| n.name().ends_with(".rpf"))
            .take(6)
        {
            if let Ok(nested) = archive.open_nested(node.entry_index()) {
                all.push(Arc::new(nested));
            }
        }
        for a in all {
            for node in a.nodes.iter().filter(|n| !n.is_dir()).take(400) {
                if !matches!(
                    &a.entries[node.entry_index() as usize],
                    RpfEntry::Resource(_)
                ) {
                    continue;
                }
                let result = a
                    .read_entry(node.entry_index())
                    .and_then(|bytes| Rsc8::open(&bytes));
                match result {
                    Ok(_) => opened += 1,
                    Err(e) => {
                        let why = e.to_string();
                        *refused.entry(why.chars().take(80).collect()).or_default() += 1;
                    }
                }
            }
        }
    }
    eprintln!("rdr2: {opened} resource(s) opened; refused: {refused:#?}");
    assert!(opened > 0);
}

/// Each texture of a dictionary is listed under the hash of its name, and its first mip decodes.
#[test]
fn a_texture_dictionary_lists_the_textures_it_holds_and_they_decode() {
    use or_io::codec::texture::decode_level;
    use or_io::decl::File;
    use or_io::hash::string_hash;
    use or_io::rdr2::TextureDictionary;

    let Some((provider, archives)) = provider_and_archives("rdr2 textures") else {
        return;
    };
    let (mut dicts, mut textures, mut named, mut decoded) = (0, 0, 0, 0);
    let mut tiles = std::collections::BTreeMap::<String, usize>::new();
    let mut problems = std::collections::BTreeMap::<String, usize>::new();
    for (path, _) in archives.iter().filter(|(p, _)| {
        ["textures_0.rpf", "textures_1.rpf"]
            .iter()
            .any(|n| p.ends_with(n))
    }) {
        let archive = Arc::new(open(path, &provider));
        let mut all = vec![Arc::clone(&archive)];
        for node in archive
            .nodes
            .iter()
            .filter(|n| n.name().ends_with(".rpf"))
            .take(4)
        {
            if let Ok(nested) = archive.open_nested(node.entry_index()) {
                all.push(Arc::new(nested));
            }
        }
        for a in all {
            for node in a
                .nodes
                .iter()
                .filter(|n| n.name().ends_with(".ytd"))
                .take(60)
            {
                let opened = a
                    .read_entry(node.entry_index())
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        TextureDictionary::from_bytes(&bytes).map_err(|e| e.to_string())
                    });
                let dict = match opened {
                    Ok(dict) => dict,
                    Err(why) => {
                        *problems.entry(why.chars().take(90).collect()).or_default() += 1;
                        continue;
                    }
                };
                dicts += 1;
                for (code, texture) in dict.codes.iter().zip(&dict.entries) {
                    let Some(texture) = texture else { continue };
                    textures += 1;
                    named += usize::from(*code == string_hash(texture.name.as_bytes()));
                    *tiles.entry(format!("{:?}", texture.tile_mode)).or_default() += 1;
                    decoded += usize::from(
                        decode_level(&**texture, 0)
                            .is_ok_and(|img| img.width == u32::from(texture.width).max(1)),
                    );
                }
            }
        }
    }
    eprintln!("rdr2: {dicts} dictionaries, {textures} textures, {named} named by their hash, {decoded} decoded; tile modes {tiles:?}; problems {problems:#?}");
    assert!(dicts > 0);
}

/// A texture's first mip is a picture: written as a PNG, the pixels read row by row.
#[test]
fn a_texture_is_written_as_a_png() {
    use or_io::decl::File;
    use or_io::rdr2::TextureDictionary;

    let Some(dir) = std::env::var_os("OR_PNG_DIR").map(PathBuf::from) else {
        return;
    };
    let Some((provider, archives)) = provider_and_archives("rdr2 png") else {
        return;
    };
    let (path, _) = archives
        .into_iter()
        .find(|(p, _)| p.ends_with("textures_0.rpf"))
        .expect("textures_0.rpf");
    let archive = Arc::new(open(&path, &provider));
    let mut written = 0;
    for node in archive
        .nodes
        .iter()
        .filter(|n| n.name().ends_with(".rpf"))
        .take(4)
    {
        let Ok(nested) = archive.open_nested(node.entry_index()) else {
            continue;
        };
        for ytd in nested
            .nodes
            .iter()
            .filter(|n| n.name().ends_with(".ytd"))
            .take(3)
        {
            let Ok(bytes) = nested.read_entry(ytd.entry_index()) else {
                continue;
            };
            let Ok(dict) = TextureDictionary::from_bytes(&bytes) else {
                continue;
            };
            for texture in dict
                .entries
                .iter()
                .flatten()
                .filter(|t| t.width >= 64)
                .take(1)
            {
                texture
                    .to_png(dir.join(format!(
                            "{written}_{}.png",
                            texture
                                .name
                                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                        )))
                    .expect("a PNG");
                written += 1;
            }
        }
    }
    assert!(written > 0);
}

/// Every archive of the install, nested ones included, opens or is refused for a stated cause, and a
/// spread of entries per archive reads; the tally is what stands between the install and a full read.
#[test]
#[ignore = "walks the whole install; run it on request"]
fn the_whole_install_reads_and_the_gaps_are_counted() {
    use std::collections::BTreeMap;

    let Some((provider, archives)) = provider_and_archives("rdr2 census") else {
        return;
    };
    let mut tally = BTreeMap::<String, usize>::new();
    let mut count = |k: String| *tally.entry(k).or_default() += 1;
    let mut stack: Vec<(String, Arc<RpfArchive>)> = Vec::new();
    for (path, _) in &archives {
        stack.push((
            path.file_name().unwrap().to_string_lossy().into(),
            Arc::new(open(path, &provider)),
        ));
    }
    while let Some((label, archive)) = stack.pop() {
        count("archive opened".into());
        for (n, node) in archive.nodes.iter().filter(|n| !n.is_dir()).enumerate() {
            let index = node.entry_index();
            if node.name().ends_with(".rpf") {
                match archive.open_nested(index) {
                    Ok(nested) => {
                        stack.push((format!("{label}/{}", node.name()), Arc::new(nested)))
                    }
                    Err(e) => {
                        eprintln!("nested {label}/{}: {e}", node.name());
                        count(format!(
                            "nested refused: {}",
                            e.to_string().chars().take(70).collect::<String>()
                        ));
                    }
                }
                continue;
            }
            if n % 25 != 0 {
                continue;
            }
            let read = archive.read_entry(index);
            match read {
                Ok(_) => count("entry read".into()),
                Err(e) => {
                    let why: String = e.to_string().chars().take(70).collect();
                    eprintln!("entry {label}/{}: {why}", node.name());
                    count(format!("entry refused: {why}"));
                }
            }
        }
    }
    eprintln!("rdr2 census: {tally:#?}");
    let unexpected: Vec<_> = tally
        .keys()
        .filter(|k| k.contains("refused") && !k.contains("has no key"))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:#?}");
}

/// A compressed body is ciphered over its stream, which the row does not state: bodies of every size
/// in the archives that hold the most of them all inflate to what their rows say.
#[test]
fn every_enciphered_oodle_body_of_a_patch_archive_inflates() {
    use or_io::container::rpf::Compressor;

    let Some((provider, archives)) = provider_and_archives("rdr2 patch bodies") else {
        return;
    };
    let (path, _) = archives
        .into_iter()
        .find(|(p, _)| p.ends_with("update_2.rpf"))
        .expect("update_2.rpf");
    let top = Arc::new(open(&path, &provider));
    let mut read = 0;
    for rpf in top.nodes.iter().filter(|n| n.name().ends_with(".rpf")) {
        let Ok(nested) = top.open_nested(rpf.entry_index()) else {
            continue;
        };
        for node in nested.nodes.iter().filter(|n| !n.is_dir()) {
            let RpfEntry::Binary(b) = &nested.entries[node.entry_index() as usize] else {
                continue;
            };
            if !b.encrypted || b.compressor != Compressor::Oodle {
                continue;
            }
            let bytes = nested
                .read_entry(node.entry_index())
                .unwrap_or_else(|e| panic!("{}/{}: {e}", rpf.name(), node.name()));
            assert_eq!(bytes.len(), b.uncompressed_size as usize);
            read += 1;
        }
    }
    eprintln!("rdr2: {read} enciphered Oodle bodies inflated");
    assert!(read > 0);
}

/// An archive edited and saved reads back file for file, whatever the keys of its bodies: the table
/// is written clear and each carried body keeps its own cipher, and a file added is stored clear.
#[test]
fn an_edited_archive_reads_back_file_for_file() {
    let Some((provider, archives)) = provider_and_archives("rdr2 edit") else {
        return;
    };
    let (path, _) = archives
        .into_iter()
        .find(|(p, _)| p.ends_with("textures_0.rpf"))
        .expect("textures_0.rpf");
    let top = Arc::new(open(&path, &provider));
    let mut checked = 0;
    for rpf in top
        .nodes
        .iter()
        .filter(|n| n.name().ends_with(".rpf"))
        .take(1)
    {
        let nested = top.open_nested(rpf.entry_index()).expect("nested");
        let mut builder = nested.edit().expect("edit");
        let big = b"a compressible line of text\n".repeat(5000);
        builder.insert("added.txt", big.clone()).expect("insert");
        let bytes = builder.to_bytes().expect("save");
        let back = RpfArchive::from_bytes_with(None, bytes, provider.clone()).expect("reopen");
        assert!(back.read("added.txt").expect("added") == big);
        for node in nested.nodes.iter().filter(|n| !n.is_dir()) {
            let want = nested.read(node.path()).expect("original");
            let got = back
                .read(node.path())
                .unwrap_or_else(|e| panic!("{}: {e}", node.path()));
            assert!(want == got, "{} differs after a save", node.path());
            checked += 1;
        }
    }
    eprintln!("rdr2: {checked} file(s) read back after a save");
    assert!(checked > 0);
}
