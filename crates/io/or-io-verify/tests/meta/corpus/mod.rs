#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use or_io::meta::BinaryFormat;

pub struct Fixture {
    pub name: String,
    pub path: PathBuf,
    pub format: BinaryFormat,
    pub bytes: Vec<u8>,
}

pub struct Corpus {
    fixtures: Vec<Fixture>,
}

pub const CORPUS_ENV: &str = "OPENRAGE_META_CORPUS";

pub fn corpus_dir() -> PathBuf {
    corpus_origin().path().to_path_buf()
}

pub fn corpus_origin() -> or_test_support::CorpusOrigin {
    or_test_support::owned_corpus_origin(
        "or-meta",
        env!("CARGO_MANIFEST_DIR"),
        CORPUS_ENV,
        "binary",
    )
}

pub fn format_label(format: BinaryFormat) -> &'static str {
    match format {
        BinaryFormat::Pso => "PSO",
        BinaryFormat::Rbm => "RBM",
        BinaryFormat::Rbf => "RBF",
    }
}

impl Corpus {
    pub fn load() -> Self {
        let dir = corpus_dir();
        let mut paths: Vec<PathBuf> = match fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect(),
            Err(_) => Vec::new(),
        };
        paths.sort();

        let mut fixtures = Vec::new();
        for path in paths {
            let Ok(bytes) = fs::read(&path) else { continue };
            let Some(format) = or_io::meta::detect_binary_format(&bytes) else {
                continue;
            };
            fixtures.push(Fixture {
                name: path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path,
                format,
                bytes,
            });
        }
        Self { fixtures }
    }

    pub fn of_format(format: BinaryFormat) -> Vec<Fixture> {
        Self::load()
            .fixtures
            .into_iter()
            .filter(|f| f.format == format)
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Fixture> {
        self.fixtures.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.fixtures.is_empty()
    }

    pub fn len(&self) -> usize {
        self.fixtures.len()
    }
}

#[macro_export]
macro_rules! require_corpus {
    ($corpus:expr) => {
        if $corpus.is_empty() {
            or_test_support::skip_corpus(
                "binary metadata corpus",
                "no binary fixtures staged; run crates/io/or-io-verify/scripts/stage-meta-corpus.ps1",
            );
            return;
        }
    };
}
