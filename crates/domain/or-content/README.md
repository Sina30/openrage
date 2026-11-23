# or-content

The kinds of content this build knows, listed once.

A kind of content depends on `or-project` and not the other way round, so something above them has to say which kinds exist. That is this crate and nothing else: the editor and `openrage-cli` plan, adopt and start a project's content from its one list, so they cannot disagree. Adding a kind is a `Content` impl in its own crate and one line in `kinds`.

- Layer: domain.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-content`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
