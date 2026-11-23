# The app

The `openrage` package: the desktop editor, and the entry point of the `openrage-cli` binary.

| Path | Holds |
|---|---|
| `src/main.rs` | the editor's entry: the window, or `--self-check` and `--photo` with none |
| `src/cli_main.rs` | the `openrage-cli` entry, which is all of `or_cli::run` |
| `src/app/` | the event loop on winit, and the egui and wgpu integration |
| `src/state/` | `AppState` and what the editor does, split by concern: documents, formats, projects, plugins |
| `src/ui/` | panels, docks, workspaces and widgets |
| `src/render/` | the bridge from a document to what `or-renderer` draws |
| `src/i18n/` | the interface languages, as `.po` catalogs |
| `src/platform/` | what differs per operating system, such as file associations |
| `src/self_check.rs` | every panel drawn through real frames with no window |

The files named `doc_*.rs`, `src/shell_budget.rs` and `src/suppressions.rs` are tests that read the documents and the shape of this folder.

⚠ `just dev` opens a window and takes the desktop's focus. To check a change with no window, run `just self-check`, and to see what it draws, `just shots SCRIPT DIR`.

- Layer: app, the top of the chain; it depends on every layer below and on `or-cli` and `or-shader-tool`.
- Tests: `cargo nextest run --bin openrage`, which runs the documentation gates too.
- A format enters the app as three registrations: [what a format costs the app](../docs/reference/Crate_Reference.md#what-a-format-costs-the-app).
- Reference: [Crate_Reference.md](../docs/reference/Crate_Reference.md#the-app).
