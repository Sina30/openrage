# Test fixtures

How the shared game-file corpus that every crate's suites borrow is laid out, found and staged; none is ever committed, and this directory holds only this README.

Game files are copyrighted and this directory is gitignored apart from this
README, so a clean checkout has no corpus and hosted CI runs without one. A suite
that finds nothing skips and names where it looked, and a skipped suite has
not passed: `just test` ends by listing what it did not check, and
`just test-strict` sets `OPENRAGE_REQUIRE_FIXTURES=1` so that a skip fails.

## Where a corpus lives

Stage it in a sibling `openrage-corpus` directory, which the suites find with
no configuration:

```text
openrage-corpus/
  or-asset/         .ydr, .ydd, .ybn, ... flat, and a gen9/ subdirectory
  or-meta/binary/   .ymt, .ymf, .pso
  mods/             mod files, read only by the .ydd repair tests
```

⚠ Never stage a mod file in `or-asset/`: its suites assume every file is one
the game ships, and a mod whose counts contradict its runs fails them.
`mods/` is found only by `or_test_support::mods_corpus_origin`, beside or
above the workspace, and not under `OPENRAGE_FIXTURES`, so the clean-checkout
run sees none.

`or-asset/` is the shared corpus, named for the crate that first owned it;
this directory, in `or-io-verify`, is its in-tree shadow.

Discovery takes the first of three routes that answers, and
`or_test_support::corpus_origin` reports which:

1. `OPENRAGE_FIXTURES`, if set and not empty. It serves the shared corpus and
   every crate's own queries, and is for a corpus that is not beside the
   workspace; the sibling layout needs no variable. A value that is not a
   directory fails the run rather than letting every suite skip.
2. The in-tree `tests/fixtures/`, once it holds a file that is neither a
   dotfile nor Markdown: this directory for `shared_corpus_root`, and a
   crate's own for `Corpus::new`.
3. The nearest `openrage-corpus` beside the workspace root or any of its
   ancestors, which is how a lane under `openrage-lanes/` finds it:
   `openrage-corpus/<crate>` if that exists, else `openrage-corpus/or-asset`.

The metadata suites (`or-io-verify`'s `meta` binary) ignore `OPENRAGE_FIXTURES`:
they read `OPENRAGE_META_CORPUS`, then this directory's `binary/`, then
`openrage-corpus/or-meta/binary`. ⚠ Staging in `binary/` here shadows the
shared corpus too, as below.
[docs/guides/Private_Tier.md](../../../../../docs/guides/Private_Tier.md) lists every
variable.

⚠ Do not stage in this directory. One file here makes it the corpus for every
format, so each suite that reads drawables or textures from the sibling finds
none and skips, which reports green. A crate's own `tests/fixtures/` shadows
the sibling for that crate's queries the same way, so
`crates/io/or-io/tests/awc_roundtrip.rs` stages its `.awc` files in the
sibling.

## Layout and targets

A `Corpus` query takes the target it files a file under from its path: a
directory named `gen9` anywhere under the root puts it in Gen9 queries, and
anything else, a flat file included, in Gen8 ones. A query returns one target,
Gen8 unless it asks. The file's own target is still its RSC7 version: `gen9`
holds what the Enhanced install ships, whose `.yed`, `.yfd`, `.ynd` and `.ynv`
are Gen8 versions, so a suite that needs Gen9 bytes checks the version, as
`or-io-verify`'s suites do. ⚠ A Gen9 file handed to a Gen8
reader does not fail: it reads as plausible nonsense, such as a 0×0 texture,
and the suite passes. So `all_targets()` is only for code that does not depend
on the generation. A file staged for both targets keeps its name in each, so a
cross-generation test pairs files by name. A `<title>/<target>/` nesting also
works, and `matched_targets_in()` needs it: it pairs files under a title
directory, so over the flat layout above it finds nothing.

## Staging

`openrage-cli search` and `openrage-cli rpf extract` pull files out of an
install. `crates/io/or-io-verify/scripts/stage-corpus.ps1` stages a list of
extensions into the sibling `openrage-corpus/or-asset` by default.
`crates/io/or-io-verify/scripts/stage-meta-corpus.ps1`
stages the metadata corpus into the sibling `openrage-corpus/or-meta/binary`
unless given `-OutDir`.

## Writing a suite

Find files with `Corpus`, never a hand-written `read_dir` over a fixture
directory:

```rust
use or_test_support::{Corpus, TARGET_GEN9};

// Gen8 files, the default.
let files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr").find();

// Gen9 files, named explicitly.
let enhanced = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
    .target(TARGET_GEN9)
    .find();

// Both, for something generation-agnostic such as a skeleton.
let all = Corpus::new(env!("CARGO_MANIFEST_DIR"), "yft").all_targets().find();
```
