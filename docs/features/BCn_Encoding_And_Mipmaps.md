# BCn encoding and mipmaps

How OpenRage turns an RGBA image into a Gen8 texture: the mip chain, block
compression, the `GrcTexture` fields that move together, and interchange.

The code is `or_io::codec::texture` (the format tables, decode, `mipmap` and
`png`) and `or_io::authoring` (`encode`, `fit` and `texture_import`, behind the
`authoring` feature, D11), and the owned `GrcTexture`'s `set_pixels`. The owned Legacy
`GrcTexture` and Enhanced `GrcTextureG9` take `to_png` and `from_png`
(`a_known_image_survives_from_png_save_open_to_png_per_channel`). The on-disk
layout is in the local
[GrcTexture specification](../extra/GrcTexture_Structure_Specification.md), and
decoding in the [texture decoding specification](../extra/Texture_Decoding_Specification.md).

## Pipeline

| Stage | Entry point |
|---|---|
| Read a source image | `texture_import::load` |
| Build the mip chain, compress a level, pack the chain | `generate_mip_chain`, `encode_level`, `encode_texture_with_options` |
| Build a new texture | `GrcTexture::from_png`, or `set_pixels` on a new `GrcTexture` |
| Replace an entry's pixels | `set_pixels`, which encodes with the three above |
| Write the dictionary | the owned dictionary's `save`, with no pack |

- `set_pixels` keeps the entry's name and usage classification, so an import
  cannot turn a texture into a render target or back
  (`importing_pixels_leaves_the_usage_classification_alone`).
- Gen8 only: a Gen9 dictionary holds `GrcTextureG9`, and the app refuses to
  re-encode or import into one (`src/state/texture_edit.rs`).

## Formats and backends

| Format | Backend | Notes |
|---|---|---|
| `DXT1`, `DXT3`, `DXT5` (BC1–3) | texpresso | 1-bit, explicit and interpolated alpha |
| `DXT5A` (BC4) | texpresso | compresses R |
| `DXN` (BC5) | texpresso | R in a block's first 8 bytes, G in its second 8 |
| `BC7` | intel_tex_2 | needs the `bc7-encode` feature |
| `A8R8G8B8`, `A8B8G8R8` | built-in | stored BGRA and RGBA |
| `L8`, `A8` | built-in | mean of R, G and B; alpha |

`is_encodable` says what a build can emit; BC6, ETC2/EAC and the float
formats decode but cannot be produced. texpresso is pure Rust. BC7 uses
Intel's ISPC compressor through `intel_tex_2`, which ships prebuilt kernels
(no ISPC or libclang install) but is the workspace's one native link, so
`bc7-encode` on `or-io` is off by default, a BC7 request without it fails
naming the feature, and CI tests it in a step of its own on every tip of
`main`. `or-source` turns it on in its own manifest, so a build that holds that crate
compiles BC7.
`EncodeQuality` `Fast`, `Normal` and `High` are texpresso's range, cluster and
iterative cluster fit, and ISPC's ultra-fast, basic and slow settings.

## Channel order

⚠ A red/blue swap, or BC5's halves swapped, survives any round trip that makes
the same mistake both ways and ships textures in the wrong colours, so the
tests assert absolute values on asymmetric input
(`channel_order_survives_encode_decode`, `bc5_keeps_red_and_green_distinct`).
`RgbaImage` is always R, G, B, A. `A8R8G8B8` is the one format swizzled on
disk, by `encode_texture_data` and the matching arm of `decode_texture_data`.
The BC4 decoder writes R and leaves G and B at zero.

## Mip chains

- sRGB colour is filtered in linear space. Averaging code values darkens every
  level, compounding down the chain: a black-and-white checkerboard's level 1
  is about 188, not 128 (`srgb_average_is_brighter_than_naive_average`). Alpha
  is coverage and never gamma-decoded.
- Colour is premultiplied before filtering, so transparent texels do not bleed
  into visible ones (`transparent_texels_do_not_bleed_colour`).
- `MipFilter::Box` (default) is an exact 2×2 mean; `Lanczos3` is separable and
  clamps each pass, so overshoot cannot un-premultiply into a bright fringe.
  Both are hand-rolled to stay reproducible in linear premultiplied space.
- Non-power-of-two sizes are refused, because the game's streaming and mip
  selection expect power-of-two, unless `TextureBuildOptions::allow_npot`
  (`--allow-npot`), which re-encoding some shipped textures needs.
- The colour space is `TextureBuildOptions::srgb`, default `true`, not the
  image's own flag. Set it `false` for normal maps and packed masks.

## `GrcTexture` fields

`set_pixels` sets `width` and `height` to level 0, `format`, `depth` (1),
`mip_count`, `cut_mip_levels` (0), `mip_stride` (`GrcTextureFormat::legacy_mip_stride`,
which is what every shipped Legacy texture states: a quarter of a compressed texture's
row of blocks, a row of texels), the sRGB bit of `info_bits` and `backing_store` (every level, level 0 first, padded
to the unit the payload word states).

`physical_size_and_template` cannot be derived from a shipped file. Over 290
Gen8 textures it is the chain rounded up to 4096 in 258, follows no stated rule
in 32, and a recompute from the chain matches 7 (measured at `c2c44c22` by a
suite deleted with `or-asset`). A save states the payload's length in the
word's masked bits, and pads a mip chain that is not a whole number of that word's units
with zeros up to the next one (`a_mip_chain_that_is_not_a_whole_number_of_units_is_padded_at_save`)
([OD-IMPLIED-PIXEL-LEN](../direction/Project_Context.md#a-textures-pixel-length-is-implied-from-its-mip-chain-before-the-packs-retire-2026-09-29)).

With `cut_mip_levels` non-zero, the payload begins at that level, and
`decode_level` indexes it from the stored size and level count.

## Interchange

- `texture_import::SourceFormat` is the one list an import accepts (PNG, JPEG,
  BMP, TGA, TIFF, DDS); the CLI, the app and `or-source` all read through it. Export writes PNG,
  one level per file, and DDS.
- DDS export copies `pixel_data` at the stored dimensions, under a FourCC
  header for linear DXT1, DXT3, DXT5, ATI1 and ATI2, the DX10 header for BC7,
  other compressed formats and anything sRGB, and pixel masks otherwise.
- ⚠ Importing a `.dds` decodes mip 0 and re-encodes: copying its payload would
  carry a format, mip count and pitch the entry's header does not state.
  `or_source::textures` is the exact path: it builds the entry's header from the
  DDS's own format, mip count and sRGB flag and copies the payload block for block.
- Every raster import reports sRGB, since none states a colour space this
  reader trusts, so a normal map needs `--linear` whatever format it came in.
- `FormatChoice::Automatic`, offered by the app, picks BC1 when every pixel is
  opaque and BC3 otherwise, keyed on the alpha values, not the source format.

## Optimising a textures source

`or_source::optimise` plans and writes what a pack's textures cost: a missing chain of mips is added under the blocks of the level that is there, a texture halved to a power of two it already holds mips for loses its top levels and keeps the rest, and only a change of size (`resize_rgba` with the Lanczos filter, whether a side grows or shrinks, and the mips under it made by the box filter) or of format (`automatic_format` for BC1 or BC3) is encoded afresh. What is written is the exact DDS the source holds, so the compile copies it block for block; a texture that needs nothing is not rewritten. A textures source compiles for either edition, which is how a picture reaches an Enhanced pack; the Texture workspace still refuses to put a new texture into an Enhanced dictionary, since that edit is over the owned Legacy `GrcTexture`.

## Determinism and quality

Output is byte-reproducible across runs and machines. The sRGB transfer is a
baked 256-entry table searched for its inverse, because platform `powf` is not
correctly rounded (`srgb_transfer_round_trips_every_code_value`); resampling is
single-threaded in a fixed order; texpresso's `rayon` writes disjoint blocks.
`encoding_is_reproducible` in `authoring::encode` holds it.

`rgba_psnr` reports PSNR overall and per channel. The floors, on a smooth
gradient: BC1 30 dB, BC3 32 dB, BC5 40 dB on R and G, and BC7 40 dB under
`bc7-encode`, held by `psnr_meets_the_workplan_thresholds`.

## CLI

```bash
openrage-cli texture info     <file.ytd>
openrage-cli texture export   <file.ytd> <outdir> [--format png|dds] [--all-mips] [--name <texture>]
openrage-cli texture import   <file.ytd> <texture-name> <image> -o <out.ytd> [encode options]
openrage-cli texture reencode <in.ytd> <out.ytd> --format <fmt> [encode options]
openrage-cli texture psnr     <a> <b>
openrage-cli texture census   <dir> [--verify] [--json] [-v]
openrage-cli texture census   --game <install-root> [--keys <file>] [--verify] [--json]
openrage-cli texture import bumper.ytd bumper_n normal.png -o new.ytd -f DXN --linear
```

`<image>` and both `psnr` arguments take any `SourceFormat`. `census --verify`
decodes every texture rather than only tallying formats.

Encode options are `-f`/`--format` (RAGE or D3D spelling: `DXN`, `BC5`,
`ATI2`), `-m`/`--mips`, `--filter box|lanczos3`, `-q`/`--quality`, `--srgb`,
`--linear` and `--allow-npot`. `import` without `--format` keeps the replaced
texture's format and sRGB flag and with one assumes sRGB; `reencode` keeps each
texture's flag; `--srgb` and `--linear` override both.

## Tests

The tests are unit tests in `authoring::encode`, `codec::texture::mipmap`,
`dds` and the app's `state::texture_edit`, and
`every_corpus_legacy_texture_decodes_as_recorded` holds every corpus Legacy
texture's decode to its recorded digest; run `just test` with a corpus staged
before claiming the path works.
