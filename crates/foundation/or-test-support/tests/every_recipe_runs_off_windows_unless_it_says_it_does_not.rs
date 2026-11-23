//! A recipe that is not marked `[windows]` runs under `sh` as well as PowerShell.
//!
//! ⚠ A recipe body is parsed by whichever shell the platform gives `just`, so a
//! PowerShell cmdlet or `$env:` in one that is not marked fails on Linux and
//! macOS, with a message about a missing command and nothing about the justfile.
//! The logic belongs in `xtask`; what stays is a call to `cargo` or `cargo xtask`.

use std::path::PathBuf;

/// What only PowerShell understands, or only a Windows machine has.
const POWERSHELL_ONLY: &[&str] = &[
    "powershell",
    "pwsh",
    "$env:",
    "$LASTEXITCODE",
    "Write-Host",
    "Join-Path",
    "Get-Command",
    "Set-Content",
    "New-Item",
    "ForEach-Object",
    "-ExecutionPolicy",
];

struct Recipe {
    name: String,
    windows_only: bool,
    body: String,
}

fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while !dir.join("justfile").is_file() {
        assert!(
            dir.pop(),
            "no justfile above {}",
            env!("CARGO_MANIFEST_DIR")
        );
    }
    dir
}

/// just's grammar, to the depth this needs: attributes on the lines directly
/// above a header, a header in column one ending in `:`, and its indented body.
fn recipes(text: &str) -> Vec<Recipe> {
    let mut out: Vec<Recipe> = Vec::new();
    let mut windows_pending = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            windows_pending = false;
        } else if trimmed == "[windows]" {
            windows_pending = true;
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') && !line.starts_with(' ') {
            // another attribute, such as `[private]`, stacked on the same recipe
        } else if line.starts_with(char::is_whitespace) {
            if let Some(last) = out.last_mut() {
                last.body.push_str(line);
                last.body.push('\n');
            }
        } else if !line.starts_with('#') && line.contains(':') && !line.contains(":=") {
            let head = line.split(':').next().unwrap_or_default().trim();
            let name = head.split_whitespace().next().unwrap_or_default();
            if !name.is_empty() && !name.starts_with("set") {
                out.push(Recipe {
                    name: name.to_string(),
                    windows_only: windows_pending,
                    body: String::new(),
                });
            }
            windows_pending = false;
        } else {
            windows_pending = false;
        }
    }
    out
}

fn powershell_in(body: &str) -> Option<&'static str> {
    POWERSHELL_ONLY
        .iter()
        .copied()
        .find(|word| body.contains(word))
}

#[test]
fn a_recipe_that_is_not_windows_only_names_no_powershell() {
    let text = std::fs::read_to_string(workspace_root().join("justfile")).expect("read justfile");
    let recipes = recipes(&text);

    // ⚠ The parse is the thing most likely to break silently: one that found
    // nothing would leave every assertion below vacuously true.
    assert!(
        recipes.len() > 30,
        "only {} recipe(s) parsed out of the justfile",
        recipes.len()
    );
    let windows: Vec<&str> = recipes
        .iter()
        .filter(|r| r.windows_only)
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        windows.contains(&"cw-roundtrip") && windows.contains(&"limits-start"),
        "the `[windows]` recipes were not recognised: {windows:?}"
    );

    let offending: Vec<String> = recipes
        .iter()
        .filter(|r| !r.windows_only)
        .filter_map(|r| powershell_in(&r.body).map(|word| format!("{} ({word})", r.name)))
        .collect();
    assert!(
        offending.is_empty(),
        "these recipes are not marked `[windows]` and run PowerShell, so they fail on \
         Linux and macOS: {offending:?}\n\
         Move the logic into `crates/tools/xtask`, or mark the recipe `[windows]`."
    );
}

#[test]
fn the_justfile_names_no_shell_of_its_own_off_windows() {
    let text = std::fs::read_to_string(workspace_root().join("justfile")).expect("read justfile");
    let setting = text
        .lines()
        .find(|l| l.trim_start().starts_with("set shell"));
    assert!(
        setting.is_none(),
        "`set shell` makes every recipe need that shell on Linux and macOS: {setting:?}"
    );
}
