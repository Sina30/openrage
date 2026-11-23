# or-media

Decodes every audio and video format OpenRage meets, behind one interface, in its own Rust.

A `Source` states what it holds and opens its tracks as an `AudioStream` of interleaved samples or a `VideoStream` of RGBA frames. The codec of a track picks a row of the `Decoders` table: PCM, ADPCM, MP3, XMA, Bink 1, Bink 2 and Bink audio are rows, and a codec with no row is refused by name while its info still reads.

⚠ This crate is AGPL-3.0-only, the strictest licence of any file in it, where the rest of the workspace is MIT. Each ported file opens with a header naming its origin, revision and licence ([LICENSE-third-party.md](../../../LICENSE-third-party.md)).

- Layer: io; it depends on `or-io`, and `or-io` never depends on it.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-media`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#io-crates).
