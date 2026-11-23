# or-io

Reads, edits and writes the file formats of Rockstar's RAGE games: GTA V's resources and archives first, declared under the engine's names.

A file opens as an ordinary Rust value, with errors only at open and at save:

```rust
use or_io::gta5::*;

let mut car = GtaDrawable::open("police.ydr")?;   // the layout comes from the file's RSC7 version
println!("{} {:?}", car.name, car.lod_group.lod_distances);
car.lod_group.lod_distances[0] = 45.0;            // plain assignment
car.lod_group.lods[0].models.remove(1);           // structural edits are Vec operations
car.save()?;                                      // validated, canonical form
```

A save writes a deterministic canonical form, which is not the shipped bytes, and saving that again is byte-identical.

## What is in it

| Module | Holds |
|---|---|
| `gta5` | GTA V's structures as `#[record]` declarations; [formats.rs](src/gta5/formats.rs) lists every format and its targets |
| `container` | the RSC7 resource and the RPF archives, every generation from RPF0 to RPF8 and IMG1 to IMG3 |
| `meta` | PSO, RBM, RBF0 and XML metadata in one tree ([meta/README.md](src/meta/README.md)) |
| `decl`, `desc`, `write` | the declaration machinery, the description of a layout and the canonical writer |
| `dds`, `awc`, `rel`, `gfx`, `mrf` | formats that are ordinary code, most behind a feature of their name |
| `gta6` | GTA VI's bodies, read by scanning an archive, behind the `gta6` feature |

## Features

The default set reads and writes both editions of GTA V and opens its archives with keys taken from the user's own executable. `meta`, `awc`, `gfx`, `rel`, `fxc`, `authoring`, `bc7-encode` and `gta6` add formats, and [Cargo.toml](Cargo.toml) says what each one costs.

- Layer: io, above foundation and below everything that knows where a file lives.
- Tests: `just test-crate or-io`; a format's corpus suite is `just test-format MODULE`, in [or-io-verify](../or-io-verify).
- Design: [OR_IO_Specification.md](../../../docs/direction/OR_IO_Specification.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#io-crates).
