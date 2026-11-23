# xtask

The repository's own tasks, run as `cargo xtask <task>`, so one implementation runs on Windows, Linux and macOS.

The `justfile` is a list of one-line calls into it: `fmt`, `lint-encoding`, `lint-provenance`, `gate` (what `just ready` runs), `install-hooks`, `lane-new`, `janitor` and `dist` among them.

It depends on no workspace crate, because the pre-commit hook runs this binary and a commit must never wait on the workspace compiling. Run `just install-hooks` again whenever this crate changes.

- Layer: tools; it is run, and nothing depends on it.
- Start at [src/main.rs](src/main.rs).
- Tests: `cargo nextest run -p xtask`.
- How the tasks are used: [Development_Workflow.md](../../../docs/guides/Development_Workflow.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#tool-crates).
