# or-io-verify

Checks `or-io` against evidence the published crate does not carry: the recorded testimony, a second description and a corpus of real files.

It is never published and nothing depends on it. A format's gates are written by `declared_gates!` from [src/gates.rs](src/gates.rs): typed reads, the testimony, the canonical fixed point and the alignment census.

⚠ These suites are the slow tier and skip on a clean checkout, and a skip is not a pass. Stage a corpus first ([fixtures/README.md](tests/fixtures/README.md)).

- Layer: io.
- One format's suite: `just test-format MODULE`.
- What each check can and cannot see: [Oracle_Testimony.md](../../../docs/guides/Oracle_Testimony.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#io-crates).
