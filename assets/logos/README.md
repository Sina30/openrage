# Game logos

Where the start page looks for each title's logo; none is committed, because Rockstar's logos are trademarked artwork.

The start page (`draw_game_row` in `src/ui/panels/game_picker.rs`) draws a
logo at the left of each install's row, square and small (`LOGO_SIZE` there),
keeping the image's aspect ratio. A logo is a PNG named after the title's id:

| File | Title |
|---|---|
| `gta-v-legacy.png` | GTA V (Legacy) |
| `gta-v-enhanced.png` | GTA V (Enhanced) |
| `rdr2.png` | Red Dead Redemption 2 |

For the official look, drop your own copies here under those names; a square
PNG works best.

⚠ This folder is not gitignored, so `git add -A` stages a logo dropped here
and the next commit carries trademarked artwork. List the files in
`.git/info/exclude`, or put them beside the executable instead.

## Lookup order

The first file that exists wins:

1. `assets/logos/<id>.png` beside the executable, for portable and release
   builds.
2. This folder, by the source path compiled into the binary, which a build
   run from this checkout finds.
3. The icon cache, `openrage/logos/<id>.png` under the local data directory
   (`%LOCALAPPDATA%` on Windows), where `src/state/game_discovery.rs` saves the
   icon it extracts from each discovered install's executable. A run whose
   configuration is redirected (`OPENRAGE_CONFIG_DIR`, a self-check or
   `just shots`) keeps its cache in `logos/` beside that configuration instead.

So a file dropped here beats the extracted icon. With none of the three, the
start page paints a badge in the title's brand colours.
