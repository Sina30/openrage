use std::path::{Path, PathBuf};
const OPENINGS: [&str; 2] = ["struct CameraUniform {", "struct Camera {"];
const RUST_MARKER: &str = "[[f32;";
fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn block_body(text: &str, open: usize) -> &str {
    let body = &text[open..];
    let end = body.find("\n}").unwrap_or(body.len());
    &body[..end]
}

fn field_names(body: &str) -> Vec<String> {
    body.lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with('#'))
        .filter_map(|line| {
            let (name, _) = line.split_once(':')?;
            let name = name.trim().trim_start_matches("pub ").trim();
            (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .then(|| name.to_string())
        })
        .collect()
}

struct Decl {
    where_: String,
    fields: Vec<String>,
    rust: bool,
}

fn camera_declarations() -> Vec<Decl> {
    let mut files = Vec::new();
    rust_sources(&src_root(), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no sources under {:?}", src_root());

    let mut out = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for opening in OPENINGS {
            let mut from = 0usize;
            while let Some(rel) = text[from..].find(opening) {
                let at = from + rel;
                from = at + opening.len();
                // A `pub struct Camera` is a viewpoint of the renderer's Rust API, not a uniform.
                if text[..at].ends_with("pub ") {
                    continue;
                }
                let body = block_body(&text, at);
                let line = text[..at].matches('\n').count() + 1;
                out.push(Decl {
                    where_: format!(
                        "{}:{line}",
                        path.strip_prefix(src_root()).unwrap_or(path).display()
                    ),
                    fields: field_names(body),
                    rust: body.contains(RUST_MARKER),
                });
            }
        }
    }
    out
}

#[test]
fn every_wgsl_camera_declares_the_host_struct_in_full() {
    let decls = camera_declarations();

    let hosts: Vec<&Decl> = decls.iter().filter(|d| d.rust).collect();
    assert_eq!(
        hosts.len(),
        1,
        "expected exactly one Rust `CameraUniform`; found {} at {:?}. This test \
         derives its expectation from that declaration, so it cannot choose \
         between two.",
        hosts.len(),
        hosts.iter().map(|d| &d.where_).collect::<Vec<_>>()
    );
    let expected = &hosts[0].fields;
    assert!(
        expected.len() >= 4,
        "the host struct at {} parsed as {expected:?}, which is too short to be right",
        hosts[0].where_
    );

    let shaders: Vec<&Decl> = decls.iter().filter(|d| !d.rust).collect();
    assert!(
        shaders.len() >= 2,
        "found only {} WGSL camera declarations — either the shaders moved or \
         the scan is broken, and both make this test silently vacuous",
        shaders.len()
    );

    let wrong: Vec<String> = shaders
        .iter()
        .filter(|d| &d.fields != expected)
        .map(|d| format!("  {}: {:?}", d.where_, d.fields))
        .collect();

    assert!(
        wrong.is_empty(),
        "a WGSL camera struct does not match the host's `CameraUniform`.\n\n\
         expected, from the Rust declaration at {}:\n  {expected:?}\n\n\
         mismatched:\n{}\n\n\
         A uniform struct is addressed by byte offset, so a missing or \
         reordered field does not fail to compile — it shifts every field after \
         it and reads plausible nonsense. Declare every field in order, \
         including the ones the shader never reads.",
        hosts[0].where_,
        wrong.join("\n")
    );
}

#[test]
fn the_field_that_caused_this_is_still_declared_on_both_sides() {
    let decls = camera_declarations();
    for decl in &decls {
        assert!(
            decl.fields.iter().any(|f| f == "inv_view_proj"),
            "{} no longer declares `inv_view_proj`. If that removal is \
             deliberate it has to happen in the Rust struct and every WGSL \
             view of it in one change — which is exactly what did not happen \
             when it was added.",
            decl.where_
        );
    }
}
