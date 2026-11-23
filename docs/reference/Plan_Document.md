# The plan document

The JSON that `openrage-cli wardrobe plan` prints for a build, which a script, a CI job and a plugin read.
It is the plan of one resource written as data: what a build would write, how much, what it cannot, and the
things in it that keep an identity when they are renamed. Printing it writes nothing, and `--out <file>` puts
it in a file instead of on the screen. `crates/domain/or-project/tests/the_plan_document_is_what_its_document_says.rs`
holds this document to `Plan::document` in `crates/domain/or-project/src/extension.rs`.

`wardrobe plan` prints a list with one document for each resource the build divides into, one when it does not
divide. A plugin is handed one document at a time, with every `from` set to `null`, because a guest has no path
([Plugin_ABI.md](Plugin_ABI.md)).

```json
{
  "format": "openrage-wardrobe-plan",
  "schema": 1,
  "output": "fivem",
  "resource": "demo",
  "kind": "fivem-resource",
  "target": null,
  "files": [
    { "path": "stream/demo/jbib_000_u.ydd", "how": "cook", "bytes": 41210, "from": "C:\\packs\\demo\\models\\jacket.drawables.src" }
  ],
  "totals": { "files": 1, "bytes": 41210 },
  "budgets": [
    { "name": "entries", "used": 3, "ceiling": null }
  ],
  "items": [
    { "id": "a1b2c3d4e5f6", "name": "mp_m_freemode_01_mp_m_demo^jbib_000_u", "digest": "5c0d" }
  ],
  "problems": []
}
```

## Keys

| Key | Holds |
|---|---|
| `format` | Always `openrage-wardrobe-plan`: what a reader checks first. |
| `schema` | A whole number, 1 today. It rises when a key is removed or changes meaning, and never for a key added. |
| `output` | The id of the build in the project. |
| `resource` | The folder the build writes and the resource's name; a build that divides names its parts `<name>_1`, `<name>_2`. |
| `kind` | What the build writes: `fivem-resource`, `singleplayer-dlc` or `oiv-package`. A kind a newer build wrote is carried as it was spelled. |
| `target` | `legacy` or `enhanced` when the build moves every drawable and texture dictionary to that edition's layout, else `null` for as authored. |
| `files` | Every file the build writes, ordered by path: `path` inside the resource with `/`, `how`, `bytes`, and `from`. |
| `totals` | `files` and `bytes` over `files`. |
| `budgets` | Each named figure the contents contribute (the archive entries of a pack, the shop entries, the size of the metadata, the memory the game streams), as `name`, `used` and `ceiling`, which is `null` when the build declares none. |
| `items` | The things with a stable identity, ordered by `id`: `id` never changes and is never reused, `name` is what the game sees and moves with the garment's place, `digest` is equal for equal content. |
| `problems` | One sentence each for what stops the build. A plan with any is refused. |

A file's `how` is how the build makes it: `copy` (a file of the project, copied exactly), `document` (text the
build writes), `generated` (a file the build writes whose bytes are not text, such as binary metadata), `retarget` (a file of
the project moved to the build's edition) or `cook` (a source folder cooked into the file it ships, then moved).
`from` is where on the disk a `copy`, `retarget` or `cook` reads, and `null` for the others and for a plugin. `bytes` of a
`cook` is the payload the file is made from, since the cooked file's size is not known before it is made.

## What may change

A key is added without a new schema and is never removed or renamed. A reader ignores a key it does not know,
and a script that reads `kind` handles a word it has not seen. A plan says what a build would write; that the
files then reload is what `openrage-cli wardrobe verify` reads back, and that the game loads them is not said by
anything here.
