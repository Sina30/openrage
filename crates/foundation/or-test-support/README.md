# or-test-support

Fixture discovery and skip handling shared by OpenRage's test suites.

No game file is committed, so a suite that reads real files asks `or_test_support::Corpus` for them and records a skip through the `skip_*` helpers when none is staged, which is how the skip ledger counts it. A suite never walks a fixture folder by hand.

⚠ A query without a target returns Gen8 files only, because a Gen9 file read with a Gen8 layout returns plausible nonsense and no error.

- Layer: foundation; a dev-dependency of the suites and nothing else.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-test-support`.
- Staging a corpus: [fixtures/README.md](../../io/or-io-verify/tests/fixtures/README.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#foundation-crates).
