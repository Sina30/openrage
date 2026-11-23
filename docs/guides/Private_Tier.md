# The private tier

What a clone of OpenRage does not get, why each piece stays off GitHub, where it
lives on the owner's machine, and how to rebuild it.

## Where the line is

Layouts are public, and so are the engine's names: `or-io` declares GTA V's
structures in the tracked tree, named from the GTA source (D1, D15). What
stays out of every commit is keys other than the key snapshot `or-io` bundles
(D18), game content (the corpus, the testimony, the metadata schema snapshot),
and citations of the leaked source and the notes that lean on it. The legal
reasoning and what may never enter the
published crate are in
[legal and provenance](../direction/OR_IO_Specification.md#legal-and-provenance); what the
tree may derive from is [Licence_And_Provenance_Policy.md](Licence_And_Provenance_Policy.md).

## What is held back

| Item | Where | Rebuild |
|---|---|---|
| Format specifications, engine summaries and leak-derived notes ([docs/extra/](../extra/README.md)) | the checkout's docs/extra/, gitignored | none: written analysis cannot be regenerated |
| The testimony | `../openrage-corpus/testimony` | none: `openrage-cli testimony record` is deleted (P45, #296) and nothing new is recorded ([Oracle_Testimony.md](Oracle_Testimony.md)) |
| The game-file corpus | `../openrage-corpus/or-asset` and `or-meta/binary`, an unversioned directory | re-staged from an install with `crates/io/or-io-verify/scripts/stage-corpus.ps1` and `crates/io/or-io-verify/scripts/stage-meta-corpus.ps1`; the layout is in `crates/io/or-io-verify/tests/fixtures/README.md` |
| Mod files | `../openrage-corpus/mods`, read only by the `.ydd` repair tests and never by the shared corpus's suites | copied from the mod's archive by hand; the layout is in `crates/io/or-io-verify/tests/fixtures/README.md` |
| The metadata schema snapshot | `../openrage-corpus/or-meta/v3/schema_snapshot.bin`, or `schema_snapshot.bin` beside the keys cache in the user's configuration directory | `just snapshot <psc-root>` over a PSC tree ([the snapshot README](../../crates/io/or-io/src/meta/README.md)) |
| Probe output and one-off scripts | `tmp/`, gitignored | disposable |

⚠ Private does not mean backed up. None of this is on GitHub, so losing the
machine loses docs/extra/ and, since R66f deleted the compiled readers, the testimony
for good unless someone has copied them elsewhere; the corpus and the snapshot
come back from an install and a PSC tree.

## Where it lives

Discovery finds each piece from the checkout or any lane, and a variable
overrides it, except `OPENRAGE_SCHEMA`, which a build reads after a worktree's
own snapshot; docs/extra/ is read only by people and has none.

| Variable | Names | Default |
|---|---|---|
| `OPENRAGE_FIXTURES` | the shared corpus every crate borrows, and each crate's own; it must be a directory | `crates/io/or-io-verify/tests/fixtures/` for the shared corpus, or a crate's own `tests/fixtures/`, if it holds a file, else the nearest `openrage-corpus/<crate>` or `openrage-corpus/or-asset` walking up |
| `OPENRAGE_META_CORPUS` | the metadata suites' corpus, which ignores `OPENRAGE_FIXTURES` | `or-io-verify`'s `tests/fixtures/binary/` if staged, else `openrage-corpus/or-meta/binary` |
| `OPENRAGE_SCHEMA` | the schema snapshot the editor, the CLI and a suite install at run time, or `none` | `schema_snapshot.bin` beside the keys cache (not for a suite), then, from a source tree, the nearest `openrage-corpus/or-meta/v3/schema_snapshot.bin` walking up |
| `OPENRAGE_TESTIMONY` | the testimony records | `testimony/` beside the discovered corpus |
| `OPENRAGE_KEY_SNAPSHOT` | a key snapshot for the default extraction path, which the install-gated suites and the shader tool use | the bundled snapshot (D18); the app and `openrage-cli` read `magic.dat` beside the keys cache first, and `openrage-cli rpf extract-keys --magic` takes one |

`none` is a word because PowerShell deletes a variable assigned an empty
string, and unsetting a variable does not hide a sibling the walk finds. A
clean checkout is the two variables `just test-no-pack` sets
([the clean-checkout state](Development_Workflow.md#the-clean-checkout-state)).

## Without it

A build with none of the private tier starts, reads what it can and says what
it cannot. A check that fails only because the tier is absent is a bug in the
check.

- **No schema snapshot.** Binary metadata reads, writes and round-trips (RBM on
  its decompressed payload), and a name outside `or_io::meta::binary::public_names`
  shows as `hash_XXXXXXXX`. PSO and RBM XML import lays the document out from
  the type tables a donor or the install's metadata files carry, and refuses by
  name a structure no table states.
  `or_io::meta::has_default_library()` reports the state.
- **No corpus or testimony.** Synthetic tests run; a corpus suite and
  `the_recorded_testimony_holds` skip, name where they looked and record the
  skip in the ledger ([the skip ledger](Development_Workflow.md#the-skip-ledger)).
- **No docs/extra/.** Nothing at build time reads it.

A configuration that cannot be honest fails instead of skipping: an
`OPENRAGE_FIXTURES` that is not a directory panics.

## What keeps it out of a commit

- `.gitignore` names the fixture roots, the corpus junction names `/corpus/`
  and `/openrage-corpus/`, `/packs/`, `*.orpack`, `/tmp/`, the docs/extra/
  tier, `schema_snapshot.bin` by name, and every RAGE extension a corpus has
  been seen to carry.
- ⚠ The extension rules are not redundant with the directory rules. The fixture
  rules once named `crates/format/or-io/` and `crates/format/or-meta/`, the V2
  split moved both crates, and the next `git add -A` swept 414 MB of game
  resources and 6,000 metadata files into 24 commits unnoticed. A path-shaped
  rule stops working when a crate moves; an extension does not.
- Git descends into a Windows junction as though it were a directory, so a
  corpus junctioned into the checkout stays out only because a rule names it.
- `scripts/lint_provenance.ps1` fails on committed game content with no
  exceptions ([what enforces it](Licence_And_Provenance_Policy.md#what-enforces-it)),
  and `the_key_snapshot_is_in_the_tree_once` on any key snapshot, by name,
  content or embed, other than the one `or-io` bundles (D18).

## The retracted files

A file git already tracks is not ignored, which is how the metadata schema
snapshot (last tracked at `crates/asset/or-meta/data/schema_snapshot.bin`) and
a `.ydr` drawable fixture were committed before the rules existed. Both were
removed from `feat/v2-overhaul` with
`git filter-repo --invert-paths --path-regex`, matching by basename, and a
force-push. They are still in `main`'s history, because #1 landed that branch
by merging the unrewritten old `main`, which carries both.
[Rewriting the history](../direction/OR_IO_Specification.md#rewriting-the-history) (R66g)
removes them from every commit.
