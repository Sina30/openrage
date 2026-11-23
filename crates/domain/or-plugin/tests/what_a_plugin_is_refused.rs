//! The capability model refuses things, and the refusals are the point.
//!
//! A runtime delivers isolation and no capability control: a store with every
//! host function registered is a sandbox around a plugin that can already do
//! everything the editor can. Every test here is a refusal, or the composition
//! of two permissions that must both say yes.

use or_plugin::{
    AlwaysWritable, Because, Capability, Grants, NeverWritable, Permissions, WriteGate,
};

#[test]
fn the_default_grant_can_do_almost_nothing() {
    let grants = Grants::default();

    assert_eq!(
        grants.len(),
        1,
        "the default is one capability. A generous default is a capability model in \
         name only, and the threat this exists for is a plugin somebody found on a \
         forum. Got {:?}",
        grants.iter().collect::<Vec<_>>()
    );
    assert!(
        grants.has(Capability::ReadOpenDocument),
        "and it is reading the document the plugin was opened on, because opening \
         it there is what consent to that meant"
    );

    let mut permissions = Permissions::new(grants, AlwaysWritable);
    for capability in Capability::ALL {
        let outcome = permissions.permits(capability);
        if capability == Capability::ReadOpenDocument {
            assert!(outcome.is_ok(), "{capability:?} is the default");
            continue;
        }
        let refused = outcome.expect_err(&format!(
            "{capability:?} must be refused by default. If it is ever granted \
             without being asked for, the default has grown and this test is the \
             place to argue for it."
        ));
        assert_eq!(refused.because, Because::NotGranted);
    }

    assert_eq!(
        permissions.refusals().len(),
        Capability::ALL.len() - 1,
        "a refusal nothing records produces a plugin that appears to work and does \
         nothing, which is the hardest kind to diagnose"
    );
}

/// A user who set the editor to read-only and then installed a plugin must not
/// become writable again without saying so.
#[test]
fn a_granted_edit_is_still_refused_when_the_embedder_says_read_only() {
    let grants = Grants::default().granting(Capability::EditOpenDocument);

    let mut open = Permissions::new(grants.clone(), AlwaysWritable);
    assert!(open.permits(Capability::EditOpenDocument).is_ok());

    // Refused where it does not — with the *same* grant.
    let mut shut = Permissions::new(
        grants,
        NeverWritable("the editor is in read-only mode".into()),
    );
    let refused = shut
        .permits(Capability::EditOpenDocument)
        .expect_err("granted and still refused");

    assert_eq!(
        refused.because,
        Because::EmbedderRefusesWrites("the editor is in read-only mode".into()),
        "⚠ and it must be a *distinct* reason from `NotGranted`. They need different \
         answers — grant it, against change the edit mode — and a host that \
         conflated them would send a user to the wrong setting."
    );
}

/// The write gate is derived from `Capability::writes()`; this holds the two in
/// step.
#[test]
fn every_writing_capability_passes_the_gate_and_no_reading_one_does() {
    let all = Grants::trusted_with_everything();
    let gate = NeverWritable("no".into());

    let mut writes = Vec::new();
    let mut reads = Vec::new();
    for capability in Capability::ALL {
        let mut permissions = Permissions::new(all.clone(), NeverWritable("no".into()));
        match permissions.permits(capability) {
            Ok(()) => reads.push(capability),
            Err(_) => writes.push(capability),
        }
    }

    assert_eq!(
        writes,
        vec![
            Capability::EditOpenDocument,
            Capability::WriteFileBytes,
            Capability::WriteInstall,
            Capability::WriteFiles
        ],
        "exactly the four writing capabilities must be stopped by a refusing gate"
    );
    for capability in &writes {
        assert!(
            capability.writes(),
            "{capability:?} was stopped by the write gate and does not report \
             `writes()` — the gate and the predicate have diverged"
        );
    }
    for capability in &reads {
        assert!(
            !capability.writes(),
            "{capability:?} reports `writes()` and passed a refusing gate, which \
             means a write is not being gated"
        );
    }
    assert!(gate.why_not_writable().is_some(), "the gate really refuses");
}

#[test]
fn a_refusal_names_the_capability_by_a_stable_key() {
    let mut permissions = Permissions::new(Grants::none(), AlwaysWritable);
    let refused = permissions
        .permits(Capability::Network)
        .expect_err("nothing is granted");

    let diagnostic = refused.diagnostic();
    assert_eq!(diagnostic.key, "plugin.refused");
    assert_eq!(
        diagnostic.arg("capability").map(ToString::to_string),
        Some("cap.network".to_string()),
        "the capability travels as a typed argument, so a host grouping refusals \
         groups by capability rather than by sentence shape"
    );
    assert!(
        diagnostic.message.contains("connect to the internet"),
        "and the message is the sentence a consent prompt would have shown, so the \
         user reads the same words in both places: {:?}",
        diagnostic.message
    );

    // Every capability has both, because both reach a user.
    for capability in Capability::ALL {
        assert!(capability.key().starts_with("cap."));
        assert!(
            capability.asks_for().len() > 10,
            "{capability:?} needs a sentence a consent prompt can show"
        );
    }
}

/// A payload reaches a guest as its file's bytes, copied into its memory
/// (`what_a_files_bytes_cost_a_guest` measures what that costs); a texture or a
/// buffer apart from its file, and a GPU resource, have no capability, and
/// previewing a texture is the host renderer's job.
#[test]
fn a_payload_capability_does_not_exist_yet() {
    let names: Vec<&str> = Capability::ALL.iter().map(|c| c.key()).collect();
    for landed in ["cap.read_file_bytes", "cap.write_file_bytes"] {
        assert!(
            names.contains(&landed),
            "`{landed}` landed with R44f: a guest reads the file's bytes and hands \
             bytes back, the second through the write gate. If it is gone, the \
             `not_yet` notes are wrong with it."
        );
    }
    for absent in ["cap.payload", "cap.buffer", "cap.gpu", "cap.texture"] {
        assert!(
            !names.contains(&absent),
            "`{absent}` exists. A texture or a buffer apart from its file, or a GPU \
             resource, is a capability nothing has asked for: a guest parses the \
             file's bytes, and rendering stays the host's. The `not_yet` notes and \
             this test are what has to be replaced, with what asked for it."
        );
    }
    assert!(
        or_plugin::not_yet::PAYLOAD_ACCESS.contains("file's bytes"),
        "the note must say how a payload does reach a guest, and what does not"
    );
    assert!(or_plugin::not_yet::GPU_ACCESS.contains("host"));
}

#[test]
fn nothing_in_the_tree_grants_everything() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the workspace root is three above or-plugin");

    let mut callers = Vec::new();
    for dir in ["src", "crates"] {
        walk(&root.join(dir), &mut |path| {
            // Tests are where the decision is legitimately made.
            if path.components().any(|c| c.as_os_str() == "tests") {
                return;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                return;
            };
            if text.contains("trusted_with_everything") && !text.contains("pub fn trusted_with") {
                callers.push(path.to_string_lossy().to_string());
            }
        });
    }

    assert!(
        callers.is_empty(),
        "these call `Grants::trusted_with_everything` outside a test: {callers:?}\n\n\
         A host that hands over everything is the outcome choosing WASM was meant \
         to avoid. If a call site genuinely needs it, it needs a row here saying \
         which and why."
    );
}

fn walk(dir: &std::path::Path, f: &mut impl FnMut(&std::path::Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            f(&path);
        }
    }
}
