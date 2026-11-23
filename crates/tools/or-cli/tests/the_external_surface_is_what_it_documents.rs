//! The NDJSON protocol implements exactly the routes it documents.
//!
//! A plugin outside the app is a subprocess speaking this protocol, so the
//! document is the contract: a route only `serve.rs` knows is undiscoverable,
//! and a route only the document names fails the client written against it.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-cli")
        .to_path_buf()
}

/// The route names `serve.rs` dispatches on.
///
/// Read from the dispatch `match` rather than from a list, so a route is found
/// whether or not it was registered anywhere else.
fn implemented() -> Vec<String> {
    let source =
        std::fs::read_to_string(root().join("crates/tools/or-cli/src/serve.rs")).expect("serve.rs");
    let body = source
        .split("fn dispatch")
        .nth(1)
        .expect("serve.rs must have a dispatch function");

    let mut out = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        // ⚠ Stop at the catch-all, spelled `other =>` or `_ =>`. Without this
        // the scrape runs on into `serve.rs`'s own argument parsing and
        // reports `--keys` and `-h` as routes.
        if trimmed.starts_with("other =>") || trimmed.starts_with("_ =>") {
            break;
        }
        // `"name" => ...` and `"a" | "b" => ...`, and nothing else.
        let Some((head, _)) = trimmed.split_once("=>") else {
            continue;
        };
        if !head.trim_start().starts_with('"') {
            continue;
        }
        for piece in head.split('|') {
            let name = piece.trim().trim_matches('"').trim();
            if name.is_empty() {
                continue;
            }
            // ⚠ A `#[cfg(test)]` hook, not a route. It exists so a request's
            // panic isolation can be exercised, it is absent from a release
            // build, and documenting it would invite somebody to call it.
            if name == "__panic" {
                continue;
            }
            out.push(name.to_string());
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Route names with the underscore spellings folded into the hyphenated ones.
///
/// ⚠ `extract-many` is also dispatched as `extract_many`, an alias rather than a
/// second route. `an_alias_is_not_a_second_route` asserts the pair exists so this
/// folding cannot hide a real route whose only spelling is an underscore.
fn canonical() -> Vec<String> {
    let mut out: Vec<String> = implemented()
        .into_iter()
        .filter(|name| !name.contains('_'))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The route names the protocol document describes.
fn documented() -> Vec<String> {
    let doc = std::fs::read_to_string(root().join("docs/reference/CLI_Server_Protocol.md"))
        .expect("CLI_Server_Protocol.md");
    let implemented = canonical();

    // Matched against the implemented names rather than parsed out of prose: the
    // question is only whether the document mentions the route.
    let mut out: Vec<String> = implemented
        .iter()
        .filter(|name| {
            doc.contains(&format!("`{name}`"))
                || doc.contains(&format!("\"{name}\""))
                || doc.contains(&format!("| {name} "))
        })
        .cloned()
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn nothing_is_reachable_that_the_protocol_does_not_describe() {
    let implemented = canonical();
    let documented = documented();

    assert!(
        implemented.len() >= 10,
        "found {} route(s), which is not this protocol — the dispatch scrape has \
         broken rather than the server having shrunk: {implemented:?}",
        implemented.len()
    );

    let undocumented: Vec<&String> = implemented
        .iter()
        .filter(|name| !documented.contains(name))
        .collect();
    assert!(
        undocumented.is_empty(),
        "these routes are reachable and undocumented: {undocumented:?}\n\n\
         `Project_Context.md` commits the external plugin surface to being this \
         protocol, which makes the document the contract. A route only the source \
         knows about cannot be written against, so it is either documented or it \
         is not a route."
    );

    eprintln!(
        "\n=== {} route(s), all documented: {implemented:?} ===",
        implemented.len()
    );
}

/// Every route reads but the four that change a clothing pack, as `Project_Context.md`
/// records; any other route that writes fails this until that record is updated with it.
#[test]
fn the_protocol_is_read_only_but_for_the_four_routes_that_change_a_pack_and_says_so() {
    let implemented = canonical();
    let (changing, reading): (Vec<&String>, Vec<&String>) = implemented
        .iter()
        .partition(|name| name.starts_with("wardrobe-"));

    // The names that would be writes. Extraction writes to a directory the caller
    // named, which is the caller's own filesystem and not the game's.
    let writes: Vec<&&String> = reading
        .iter()
        .filter(|name| {
            let n = name.as_str();
            n.contains("write")
                || n.contains("save")
                || n.contains("import")
                || n.contains("delete")
                || n.contains("patch")
                || n.contains("build")
                || n.starts_with("create")
        })
        .collect();

    assert!(
        writes.is_empty(),
        "these routes look like writes: {writes:?}\n\n\
         That is not a failure of the protocol — it is the point at which \
         `Project_Context.md`'s claim that every route but four reads stops being true. \
         Update that record in the same change, because an external tool's author \
         needs to know whether this protocol can modify their install."
    );

    // The routes that change a pack are these four, and the document says they write a project's pack and never the install.
    assert_eq!(
        changing,
        ["wardrobe-add", "wardrobe-fix", "wardrobe-set", "wardrobe-waive"],
        "a route that changes a pack is named for what it changes, and listed here and in the document"
    );
    let doc = std::fs::read_to_string(root().join("docs/reference/CLI_Server_Protocol.md"))
        .expect("CLI_Server_Protocol.md");
    let section = doc
        .split("## Routes that change a pack")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("the document has a section for the routes that change a pack");
    assert!(
        section.replace('\n', " ").contains("never the install"),
        "the section says what the routes may not touch"
    );

    // And the read routes are all there, so this is not vacuous.
    for expected in ["mount", "search", "list", "locate", "extract"] {
        assert!(
            reading.iter().any(|n| *n == expected),
            "`{expected}` is missing, so the read surface has changed shape and \
             this assertion is measuring the wrong thing"
        );
    }
}

/// ⚠ [`canonical`] drops names containing an underscore, which is only safe while
/// every such name has a hyphenated twin; otherwise a real route spelled with an
/// underscore would be silently excused from being documented.
#[test]
fn an_alias_is_not_a_second_route() {
    let all = implemented();
    let canonical = canonical();

    let aliases: Vec<&String> = all.iter().filter(|n| n.contains('_')).collect();
    for alias in &aliases {
        let hyphenated = alias.replace('_', "-");
        assert!(
            canonical.contains(&hyphenated),
            "`{alias}` has no hyphenated twin, so it is a route rather than an \
             alias — and `canonical` is excusing it from the documentation check. \
             Either document it under this spelling or give it one."
        );
    }
    assert!(
        !aliases.is_empty(),
        "no underscore alias found. If they have all gone, delete `canonical`'s \
         filter and this test together rather than leaving a filter nothing needs."
    );
}

#[test]
fn the_document_does_not_promise_a_route_the_server_lacks() {
    let doc = std::fs::read_to_string(root().join("docs/reference/CLI_Server_Protocol.md"))
        .expect("CLI_Server_Protocol.md");
    let implemented = canonical();

    // Route-shaped names the document uses in a `"cmd": "..."` position.
    let mut promised: Vec<String> = Vec::new();
    for line in doc.lines() {
        let Some(after) = line.split("\"cmd\":").nth(1) else {
            continue;
        };
        let name: String = after
            .trim_start()
            .trim_start_matches('"')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() {
            promised.push(name);
        }
    }
    promised.sort_unstable();
    promised.dedup();

    assert!(
        !promised.is_empty(),
        "the document shows no `\"cmd\": \"...\"` example, so an author has \
         nothing to copy"
    );

    let missing: Vec<&String> = promised
        .iter()
        .filter(|name| !implemented.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "the document shows these routes and the server does not answer them: \
         {missing:?}. A client written from the document would fail at run time."
    );
}
