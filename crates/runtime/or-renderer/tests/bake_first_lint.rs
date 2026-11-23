use std::path::{Path, PathBuf};
const ALLOWLIST_PREFIXES: &[&str] = &["stencil/", "gizmos/", "viewport_grid/", "debug_overlay/"];
fn is_allowed_wgsl_path(rel: &Path) -> bool {
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.contains("/fallback/") {
        return true;
    }
    for p in ALLOWLIST_PREFIXES {
        if s.starts_with(p) {
            return true;
        }
    }
    false
}

fn collect_wgsl_files() -> Vec<PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = crate_dir.join("src");
    let mut out = Vec::new();
    walk(&src, &src, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
            continue;
        }
        if path.extension().is_some_and(|e| e == "wgsl") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
}

#[test]
fn bake_first_policy_holds_for_current_tree() {
    let files = collect_wgsl_files();
    let mut violations = Vec::new();
    for f in &files {
        if !is_allowed_wgsl_path(f) {
            violations.push(f.clone());
        }
    }
    assert!(
        violations.is_empty(),
        "Phase 6 WS4 bake-first policy violation — these WGSL files \
         live outside a `fallback/` subfolder and outside the editor-only \
         allowlist (stencil/, gizmos/, viewport_grid/, debug_overlay/):\n{:#?}\n\
         Move the shader into `<module>/fallback/` or extend the allowlist \
         in `tests/bake_first_lint.rs`.",
        violations,
    );
}

#[test]
fn bake_first_lint_blocks_new_root_wgsl() {
    assert!(is_allowed_wgsl_path(Path::new(
        "fog/fallback/fog_inject_fallback.wgsl"
    )));
    assert!(is_allowed_wgsl_path(Path::new(
        "shaded/fallback/lighting_directional_fallback.wgsl"
    )));
    assert!(is_allowed_wgsl_path(Path::new("stencil/water_plane.wgsl")));
    assert!(is_allowed_wgsl_path(Path::new("gizmos/light_cone.wgsl")));
    assert!(is_allowed_wgsl_path(Path::new("viewport_grid/grid.wgsl")));
    assert!(is_allowed_wgsl_path(Path::new(
        "debug_overlay/depth_vis.wgsl"
    )));

    assert!(!is_allowed_wgsl_path(Path::new("foo/foo.wgsl")));
    assert!(!is_allowed_wgsl_path(Path::new("fog/fog_inject.wgsl")));
    assert!(!is_allowed_wgsl_path(Path::new(
        "shaded/some_new_thing.wgsl"
    )));
    assert!(!is_allowed_wgsl_path(Path::new("rogue.wgsl")));
}
