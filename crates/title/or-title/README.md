# or-title

A game install: which title and edition it is, where its archives are, what order they stack in and how a path resolves through them.

| Module | Answers |
|---|---|
| `game` | which title this is: the profiles of GTA V Legacy, GTA V Enhanced, Red Dead Redemption 2 and GTA VI, and the registry |
| `vfs` | which bytes a path names once the archives are mounted |
| `dlc` | which packs exist, their override order, and writing one |
| `mods` | which mods an install holds and which of two wins a path |
| `install` | tables fetched from an install: shaders, materials, names, the timecycle |

Adding a title is one `GameProfile` impl and one registry line, which [tests/adding_a_title.rs](tests/adding_a_title.rs) proves with a title it invents.

⚠ An edition is not a generation: an Enhanced install ships Legacy resources, so a file's layout comes from its own RSC7 version.

- Layer: title, above io. Code that needs only the bytes belongs in `or-io`.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-title`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#title-crate).
