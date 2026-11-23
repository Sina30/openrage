# or-kraken

Decodes Oodle Kraken, the compressor of most of Red Dead Redemption 2's larger bodies, through a pure-Rust port of ooz.

It is a crate of its own for two reasons. The port panics on a stream it cannot read, and the catch has to live in a crate compiled to unwind, which the development profile's Cranelift is not. And the port's licence stays out of `or-io`, which reaches this crate only through its `oodle` feature.

- Layer: io.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-kraken`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#io-crates).
