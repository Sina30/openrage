# or-source

The folders an asset is edited as and compiled back from.

A game file becomes a source folder a person can edit with ordinary tools and keep in Git: `asset.json`, pictures for a `.ytd`, models for a `.ydr` or a `.ydd`, a fragment for a `.yft`, and metadata for a `.ymt`, `.ymap` or `.ytyp`. `cook` compiles one saved source back into its game file, and `openrage-cli source` and `openrage-cli project import` run it with no window. Nothing here draws or watches a folder.

- Layer: domain.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-source`.
- Design: [Source_Format_Design.md](../../../docs/direction/Source_Format_Design.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
