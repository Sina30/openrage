# or-cli

The subcommands of `openrage-cli`: convert, inspect, search, build and serve RAGE files with no window.

This crate is a library, `or_cli::run`. The `openrage` package builds it into the `openrage-cli` binary, so run it from the repository root:

```bash
cargo run -p openrage --bin openrage-cli -- help
cargo run -p openrage --bin openrage-cli -- rpf list x64a.rpf
```

| To | Commands |
|---|---|
| Look inside | `inspect` `describe` `search` `depends` `names` `hash` `formats` `versions` |
| Convert | `convert` `meta` `texture` `mesh` `awc` `gfx` `script` `deflate` |
| Work on archives | `rpf` `bulk` `dlc` `dlc-list` |
| Build content | `project` `wardrobe` `addonped` `weapons` `maps` `particles` `source` `world` |
| Automate | `serve` `photo` |
| Census an install | `analyse` `archives` `channels` `bounds` `navmesh` `gaps` |

`openrage-cli <command> --help` gives a command's usage. An exit code of 0 is success, 1 a runtime error and 2 a usage error.

- Layer: tools, beside the chain; only the app depends on it.
- Start at [src/lib.rs](src/lib.rs), where `run` dispatches every command.
- Tests: `just test-crate or-cli`.
- The `serve` protocol: [CLI_Server_Protocol.md](../../../docs/reference/CLI_Server_Protocol.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#tool-crates).
