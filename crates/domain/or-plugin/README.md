# or-plugin

The plugin host: what a plugin may ask for, and what the host refuses.

A plugin is a WebAssembly module. WebAssembly isolates it and does not limit what it may do, so the capability model here is the control: a manifest names the capabilities a plugin wants, the user answers, and the default grant is reading the open document alone. A Rust type cannot cross into a guest, so the host's surface is a described tree of names, offsets and widths.

- Layer: domain; the app embeds it.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-plugin`.
- The contract: [Plugin_ABI.md](../../../docs/reference/Plugin_ABI.md). Writing a guest: [plugins/README.md](../../../plugins/README.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
