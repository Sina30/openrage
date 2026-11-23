//! A build in the app keeps what its verification found as data the Build tab draws: every file a
//! reader reopened and how many, the metadata read back, the names checked, the streaming memory
//! against the build's ceiling, and on request whether a second build writes the same bytes.
//!
//! ⚠ Run on the worker a build uses, with a project in a scratch folder: no install, no keys.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::container::resource::{ResourceEnvelope, Rsc7};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::PgDictionary;
use or_project::{Container, MetadataCount, Output, Reproducible};

use super::workbench::{Builds, Job, LastBuild};

fn write(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A Legacy texture dictionary of one block-compressed texture.
fn legacy_ytd() -> Vec<u8> {
    let format = or_io::codec::texture::GrcTextureFormat::DXT1;
    let texture = GrcTexture {
        name: "road".into(),
        width: 16,
        height: 16,
        depth: 1,
        mip_count: 1,
        mip_stride: u16::try_from(format.legacy_mip_stride(16)).unwrap(),
        format: format.to_raw_format(),
        backing_store: vec![0x5a; format.total_size(16, 16, 1) as usize],
        ..Default::default()
    };
    PgDictionary {
        codes: vec![or_io::string_hash(b"road")],
        entries: vec![Some(Box::new(texture))],
        ..Default::default()
    }
    .to_bytes()
    .unwrap()
}

fn metadata(root: &str, entries: usize) -> Vec<u8> {
    let items = "<Item><Flag>1</Flag></Item>".repeat(entries);
    format!("<?xml version=\"1.0\"?>\n<{root}><Entries>{items}</Entries></{root}>\n").into_bytes()
}

fn job(folder: &Path, files: &[(String, Vec<u8>)], reproducible: bool) -> Job {
    let members: Vec<PathBuf> = files
        .iter()
        .map(|(rel, body)| {
            write(&folder.join(rel), body);
            folder.join(rel)
        })
        .collect();
    Job {
        container: Container {
            outputs: vec![Output::fivem_resource("fivem", "pack", &["files"])],
            ..Container::default()
        },
        folder: folder.to_path_buf(),
        root: folder.join("build"),
        members,
        outputs: vec!["fivem".into()],
        remap: Arc::new(Default::default()),
        extensions: Vec::new(),
        reproducible,
    }
}

fn built(job: Job) -> LastBuild {
    let mut builds = Builds::default();
    assert!(builds.start(job));
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        if let Some((_, last)) = builds.poll().into_iter().find(|(id, _)| id == "fivem") {
            return last;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the build did not end");
}

#[test]
fn a_build_keeps_every_reloaded_file_the_metadata_the_names_the_memory_and_the_second_build() {
    let dir = tempfile::tempdir().unwrap();
    let ytd = legacy_ytd();
    let mut files: Vec<(String, Vec<u8>)> = (0..25)
        .map(|n| (format!("stream/road{n:02}.ytd"), ytd.clone()))
        .collect();
    files.push(("stream/a.ymt".into(), metadata("PackInfo", 2)));
    files.push(("stream/b.ymt".into(), metadata("PackInfo", 3)));
    files.push(("client.lua".into(), b"print('hi')\n".to_vec()));

    let LastBuild::Built(done) = built(job(&dir.path().join("pack"), &files, true)) else {
        panic!("the build stopped");
    };

    let part = &done.parts[0];
    assert_eq!(
        part.reloaded.len(),
        27,
        "25 dictionaries and 2 documents, none cut off"
    );
    assert_eq!(part.checked, 29, "and the script and the manifest");
    assert_eq!(
        part.metadata,
        [MetadataCount {
            reader: "PackInfo (XML)".into(),
            documents: 2,
            entries: 5
        }]
    );
    assert_eq!(
        part.names.line(),
        "No content holds a name the game resolves once"
    );
    let one = u64::from(Rsc7::peek(&ytd).unwrap().total_size());
    assert_eq!((part.memory.files, part.memory.bytes), (25, 25 * one));
    assert_eq!(part.memory.ceiling, None);
    assert_eq!(part.reproducible, Reproducible::Yes { files: 29 });
}

#[test]
fn a_second_build_is_made_only_when_it_is_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let files = vec![("client.lua".to_string(), b"x".to_vec())];

    let LastBuild::Built(done) = built(job(&dir.path().join("pack"), &files, false)) else {
        panic!("the build stopped");
    };

    assert_eq!(done.parts[0].reproducible, Reproducible::NotRun);
    let scratch: Vec<String> = std::fs::read_dir(dir.path().join("pack/build"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".again"))
        .collect();
    assert!(scratch.is_empty(), "{scratch:?}");
}
