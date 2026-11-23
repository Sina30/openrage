---
name: OpenRage
description: The desktop editor's incumbent look, a flat graphite workbench with one blue accent.
colors:
  accent: "#0078d4"
  accent-hover: "#1c86e0"
  accent-text: "#68afe5"
  on-accent: "#ffffff"
  selection: "#183c59"
  bg-l0: "#1a1c20"
  bg-l1: "#212429"
  bg-l2: "#2a2d33"
  bg-l3: "#383c44"
  border: "#383c44"
  text-primary: "#cccccc"
  text-secondary: "#aaaaaa"
  text-strong: "#ffffff"
  success: "#4ec469"
  warning: "#cca700"
  error: "#f68787"
  info: "#4ec4ff"
  search-match: "#363012"
  search-match-active: "#3f3614"
  search-match-edge: "#b8962e"
  light-bg-l0: "#ffffff"
  light-bg-l1: "#f3f3f3"
  light-bg-l2: "#e8e8e8"
  light-bg-l3: "#d6d6d6"
  light-border: "#c8c8c8"
  light-text-primary: "#1f1f1f"
  light-text-secondary: "#5c5c5c"
  light-accent-text: "#005ea6"
  light-selection: "#c2daed"
  kind-model: "#6ac86a"
  kind-texture: "#c586c0"
  kind-collision: "#e69b4a"
  kind-animation: "#a88be0"
  kind-particle: "#4ec9b0"
  kind-archive: "#e6c64a"
  kind-metadata: "#c4a7e7"
  kind-folder: "#739cd7"
  kind-unknown: "#5a5a5a"
typography:
  heading:
    fontFamily: "Inter, Ubuntu Light, Hack"
    fontSize: "18px"
    fontWeight: 400
  body:
    fontFamily: "Inter, Ubuntu Light, Hack"
    fontSize: "12.5px"
    fontWeight: 400
  small:
    fontFamily: "Inter, Ubuntu Light, Hack"
    fontSize: "10.5px"
    fontWeight: 400
  mono:
    fontFamily: "Hack, monospace"
    fontSize: "12px"
    fontWeight: 400
rounded:
  xs: "3px"
  sm: "4px"
  md: "6px"
  lg: "8px"
spacing:
  hair: "2px"
  tight: "4px"
  snug: "6px"
  default: "8px"
  loose: "10px"
  relaxed: "12px"
  wide: "16px"
  section: "24px"
components:
  button:
    backgroundColor: "{colors.bg-l2}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
    padding: "4px 8px"
  button-hover:
    backgroundColor: "{colors.bg-l3}"
    textColor: "{colors.text-primary}"
  button-active:
    backgroundColor: "{colors.selection}"
    textColor: "{colors.text-strong}"
  workspace-button:
    backgroundColor: "transparent"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
    width: "88px"
    height: "22px"
  workspace-button-active:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
  document-tab:
    backgroundColor: "{colors.bg-l1}"
    textColor: "{colors.text-secondary}"
    height: "28px"
  document-tab-active:
    backgroundColor: "{colors.bg-l2}"
    textColor: "{colors.text-primary}"
  input:
    backgroundColor: "{colors.bg-l0}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
  card:
    backgroundColor: "{colors.bg-l1}"
    rounded: "{rounded.md}"
  card-hover:
    backgroundColor: "{colors.bg-l2}"
  window:
    backgroundColor: "{colors.bg-l1}"
    rounded: "{rounded.lg}"
  menu:
    backgroundColor: "{colors.bg-l1}"
    rounded: "{rounded.md}"
    padding: "6px"
---

# Design System: OpenRage

This records the look the editor has today, extracted from `src/ui/theme.rs`,
`src/ui/focus_ring.rs` and `src/app/gpu.rs`. The code is the authority: where
this file and the code disagree, the code is right and this file is stale.
The frontmatter holds the dark theme, which is the default; the `light-`
entries are the light theme's counterparts.

## Overview

**Creative North Star: "The Workbench"**

A neutral dark bench where the asset is the only colourful thing. Panels are
flat graphite in four close steps, text is grey on grey at a ratio that has
been measured, and one blue marks where the user is. A model, a texture or a
particle effect sits in the middle of that, lit and coloured by the game's own
shaders, and nothing around it competes.

The bench is dense on purpose. Sessions are long, a dozen documents are open
at once, and the inspector shows offsets, hashes and sizes that have to be
read exactly, so values sit in monospace and labels stay small. The interface
is the familiar shape of a desktop editor: a menu bar, a row of tabs, docked
panels, a status bar.

Copy is part of the look. A panel with nothing to show says so in one grey
sentence in its middle; a refused write names what refused it. No surface
sells anything.

**Key Characteristics:**
- Flat graphite panels, separated by a one-pixel border and never by shadow.
- One accent, blue, kept for place, focus and selection.
- Colour otherwise belongs to file kinds, states and the asset itself.
- Small type, tight spacing, monospace for every value read from a file.
- Every text colour reaches 4.5 to 1 on every panel, in both themes, by test.
- Two themes and a user-chosen accent, scale and font: nothing is a constant.

## Colors

Graphite neutrals with a cool cast, one blue accent, four status colours and a
family of mid-saturation kind colours.

### Primary
- **Bench Blue** (`accent`): the fill of the active workspace button, the
  two-pixel line along the top of the tab on show, the stroke of a hovered
  control, the slider's trailing fill.
- **Bench Blue, lifted** (`accent-hover`): the accent under the pointer.
- **Readable Blue** (`accent-text`): the accent's hue lightened until it
  reads as text on every panel and on a selection. Links, the title of the
  focused panel, the focus ring, the text cursor.
- **Selection** (`selection`): the panel with a tint of the accent in it. A
  selected row, a pressed control. Opaque, so the colour text sits on is known.

### Neutral
- **Well** (`bg-l0`): the darkest step. The tab strip, text fields, code.
- **Panel** (`bg-l1`): every panel and window.
- **Raised** (`bg-l2`): a resting control, the active tab, a hovered card.
- **Lifted** (`bg-l3`): a hovered or open control.
- **Divider** (`border`): what separates panels. It is 1.3 to 1 on the panel,
  and is meant to be: a control that is nothing but its outline uses a
  stronger outline, derived to reach 3 to 1.
- **Text** (`text-primary`), **Quiet text** (`text-secondary`) and **Strong
  text** (`text-strong`). Disabled and dim text are the same colour as quiet
  text, because anything fainter fails the contrast floor.

### Tertiary
- **States:** `success`, `warning`, `error`, `info`. A dirty document is
  warning yellow, a loading one info blue, an error red, a new or recovered
  one green.
- **Kinds:** each file kind has its own colour (`kind-model`,
  `kind-texture`, `kind-collision`, `kind-animation`, `kind-particle`,
  `kind-archive`, `kind-metadata`, `kind-folder` and more in the code). It
  tints the kind's icon in the tree, the outline and the tab, and is adjusted
  per theme until it reaches 3 to 1.
- **Find matches:** a dark amber fill with a brighter amber edge, so a match
  is seen while the text on it still reads.

### Named Rules
**The Place Rule.** Blue says where you are and what is selected: the active
workspace, the tab on show, the focused widget, the selection, a link. This is
guidance, not a prohibition; a new use of the accent should be able to say
which of those it is.

**The Measured Text Rule.** A text colour is judged by its contrast ratio on
every panel and on a selection, 4.5 to 1 for text and 3 to 1 for a mark that
carries meaning. A colour that is not in the palette goes through the theme's
legibility helpers before it is drawn.

**The Resolved Colour Rule.** A themed colour is read at draw time, never
stored as a constant, because the user picks the theme and the accent.

## Typography

**Interface Font:** Inter Regular, bundled (with egui's Ubuntu Light and Hack
behind it for what Inter lacks)
**Mono Font:** Hack
**Icon Font:** Phosphor, regular weight only
**CJK:** a font per language, the language in use first

**Character:** One weight of one humanist sans, small, with a plain monospace
for anything read out of a file. Hierarchy comes from size and from the three
text colours, not from weight.

### Hierarchy
- **Heading** (regular, about 18 px): a document's or an inspector's title.
- **Body** (regular, egui's default body size): labels, rows, menus, prose.
- **Small** (regular, 10.5 pt): field labels in the inspector, counts,
  secondary facts. Raised from egui's 9 pt, which was too small to stand alone.
- **Mono** (Hack): offsets, hashes, sizes, paths, code, the viewport's hints.

### Named Rules
**The One Weight Rule.** There is no bold face in the bundle. Emphasis is the
strong text colour, which is white on dark and black on light.

**The User's Font Rule.** The user may pick a system font, the monospace, a
scale and a language. A layout that depends on Inter's exact widths is broken.

## Layout

A docked desktop shell, top to bottom: a menu bar that also carries the
workspace buttons (88 px wide, 22 px tall); a tab strip 28 px tall, one tab per
open document; the dock, whose panels the user arranges and which a workspace
presets (files and outline to the left, the document or viewport in the
middle, the inspector to the right, a timeline below where one applies); and a
one-line status bar with the game, the path, the tab count, the log and jobs.
A rail 38 px wide runs down the left edge of the window, in the tab strip's
colour: the rooms Home, Game, Project and Automate at its head, Help and
Settings at its foot.

Spacing is eight tokens from 2 to 24 px, and a gap in a panel is one of them. Items sit 8 px apart across and 4 px
down, and a button pads 8 by 4, which is what makes the shell dense. An icon
keeps a 4 px gutter from its label. The window resizes freely and panels
scroll; nothing reflows by breakpoint. Every string is translated into ten
languages, so a label is given room to grow or is cut with an ellipsis, never
sized to its English.

## Elevation & Depth

Flat. Depth inside the window is tonal: the four background steps say what is
recessed (well), level (panel) and raised (control). Shadow is kept for the
two things that float above the dock: a window (offset 0 by 6, blur 24) and a
popup or menu (offset 0 by 6, blur 14), both black at low alpha and lighter in
the light theme. A box drawn over the viewport is the panel colour at about 92
percent opacity, so what it holds reads as it does on a panel.

### Named Rules
**The Flat Dock Rule.** A docked panel never casts a shadow, and a card lifts
on hover by changing its fill and outline, not by rising.

## Shapes

Gently squared, in four radius tokens. Controls take 4 px, cards and menus 6 px, windows
8 px; the focus ring is 3 px. Lines are one pixel, two for a pressed control's
stroke, the text cursor, the active tab's top line and the focus ring. Slider
handles are circles. Icons are Phosphor outlines at text size.

## Components

### Buttons
- **Shape:** 4 px radius, 8 by 4 padding.
- **Resting:** raised fill with a one-pixel outline strong enough to be seen.
- **Hover / Open:** lifted fill, accent stroke.
- **Pressed / Keyboard-focused:** selection fill, two-pixel readable-blue
  stroke, strong text.
- **Workspace button:** transparent at rest with primary text; accent fill
  with on-accent text when it is the workspace on show.

### Document tabs
- **Style:** a kind icon tinted by its kind, the name, a dirty dot in warning
  yellow, a close mark. Panel fill and quiet text at rest; raised fill,
  primary text and a two-pixel accent line along the top when on show.

### Cards / Containers
- **Corner Style:** 6 px.
- **Background:** the group frame at rest; raised fill with the strong outline
  under the pointer.
- **Shadow Strategy:** none.

### Inputs / Fields
- **Style:** the well as fill, one-pixel outline, 4 px radius, grey
  placeholder text that says what the field accepts.
- **Focus:** a two-pixel readable-blue text cursor; the keyboard ring below.

### Navigation
- **Rail:** a 30 px square button for each room, holding one 18 px icon and
  no label; its name is its hover and what a screen reader says. Quiet at rest,
  lifted fill under the pointer. The room on show takes the raised fill, the
  icon in readable blue and a short accent bar at its left edge.
- **Menus:** 6 px radius, 6 px margin, a popup shadow; commands carry an icon.
- **Trees and outlines:** a disclosure triangle, a kind-tinted icon, the name,
  then small quiet facts on the same row.

### Focus ring
A two-pixel readable-blue ring, 2 px outside the widget, 3 px radius. It
appears only after a key that moves focus has been pressed more recently than
the pointer, is clipped to its scroll area, and is covered by a menu opened
over it.

### Empty state
One sentence of quiet text centred in the panel, with a 34 px outline icon
above it where the panel has room. Never justified.

## Do's and Don'ts

### Do:
- **Do** take every colour from the theme, read at draw time.
- **Do** pass a colour that is not a palette slot through the theme's
  legibility helper before drawing text or a meaningful icon with it.
- **Do** separate panels with the divider and controls with the stronger
  outline.
- **Do** set file values (offsets, hashes, sizes, paths) in monospace.
- **Do** check a screen in both themes and in a long-string and a CJK language.

### Don't:
- **Don't** hard-code a hex in a panel; a constant draws dark-theme colours on
  the light theme's white.
- **Don't** put a shadow on a docked panel or a card.
- **Don't** dim text below quiet text to show it is disabled.
- **Don't** add a second weight or a second interface font.
- **Don't** centre-and-justify a message that can wrap.
