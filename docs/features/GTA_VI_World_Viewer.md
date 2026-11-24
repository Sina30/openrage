# GTA VI World Viewer

The World screen flies a free camera through GTA VI's world as the game streams it, at an hour
and in the game's modes the World panel sets. It reads the install the GTA VI folder of
Settings ▸ Games names, and writes nothing into it.

## Opening it

The rail shows **World**, a globe with a small gamepad, once a GTA VI folder is set. It opens the
World workspace: the view, and beside it the World panel, which says what the mount found, what is
streaming and where the camera is.

The first visit builds an index of the install as a job with progress and Stop. The index is kept
in the configuration folder under `gta6_world`, and is built again only when an archive's size or
modification time changes, or from **Build the index again**.

## Which maps it loads

The viewer mounts the install by what its archives state (`crates/title/or-title/src/gta6_mount.rs`).
How it finds a file depends on whether a key snapshot is at hand: `gtavi_magic.dat` in the
configuration folder, or the file `OPENRAGE_GTA6_KEYS` names.

- **With a snapshot**, each top-level archive's table is deciphered, and the registration documents
  (each pack's setup, its content definitions and their change sets) and the archives they register
  are found by name. Every map body is then bound to the registered archive that holds it.
- **Without one**, the small archives are scanned for the documents in the clear, and a map is bound
  through its block, a map whose name is the registered archive's.

The World panel says which of the two bound the maps. A change set whose file is ciphered is not
read.

Every pack states its mode: the base game, the story pack or the shared minigames. A map of the base
game streams when it is not script managed and holds something to draw. **Time and modes** in the
World panel switches the story pack's and the shared minigames' maps on.

A script-managed map belongs to a group of maps a script switches together. No data the viewer reads
says which groups are on at rest, so none is; the same section lists the groups, and a group ticked
there streams its maps in.

An archetype is found by its 64-bit word among every map types body of the indexed archives, and a
model by the word its drawable or fragment states, or by the one drawable dictionary that holds a
drawable under every name of a dictionary's archetypes. A time archetype draws only inside the
hours it states.

## Streaming and level of detail

`crates/domain/or-world/src/streaming.rs` holds the rules, with no file or device in them:

- A map is wanted while the camera is within 50 m of the box it streams by. Its level-of-detail
  parent maps are wanted with it. A map's body is read when it is wanted, on a worker.
- A map whose content flags put it in the high tier is not streamed in while the camera moves at
  40 m/s or more.
- A node with a parent is judged by the distance to its **parent**, so the children of one parent
  appear together. A parent and its children cross-fade over 20 m either side of the distance it
  states for them: the children fade in while the parent is whole, then the parent fades out. That
  needs every child map loaded and every child's model on the device; until then the parent draws
  alone, so no hole opens while models arrive.
- A node fades out over 20 m past the distance it draws to, and a model that arrives in view fades in
  over a quarter of a second.
- A list of instances draws by the distance to its box, less each instance whose own entity draws.
  A grass list draws each of its instances within the distance the list states for one instance,
  fading out towards it.
- An interior a map places draws its contents, and the entity sets its instance names, while the
  camera is near it.
- The **Level-of-detail scale** multiplies every distance; the rules take one scale a level.

Streaming and selection run on a thread of the world's own, which selects again when the camera
has moved, a map came or went, a model did, or a fade is still under way. A frame reads the last
selection and never waits for one.

## Drawing

A selection's instances stand in chunks, a map's entities or one list each, with the sphere that
holds them. A frame culls the chunks and then the instances to what the camera sees, on every
core, gives each instance the level of detail its distance draws at, and lays the instances out a
model and a level; when neither the camera, the selection nor what is on the device changed, it
does nothing.

- A model keeps every level of detail its drawable holds. Its meshes are written into shared pages
  of vertices and indices, and draws of one page and one material go out together where the device
  can draw several at once (`crates/runtime/or-renderer/src/world.rs`).
- A mesh is drawn with its material's colour, normal and specular maps, in the bucket its shader
  states: opaque, cut out, emissive, decal and blended surfaces draw in that order.
- A fading instance is drawn through an ordered dither, the same in every pass.
- In Shaded the instances are written into the frame the game's own lighting passes light, where a
  game shader pack is read, and the sun casts their shadow in four cascades about what the camera
  sees; with no pack they are lit by the plain pass. The World panel says which.

Models are read on workers, the nearest first, and textures as the models that draw ask for
them; both are put on the device under a budget a frame, and the ones drawn longest ago go once the
device holds more than its budget. A texture is read at the level the screen needs and every level
under it, from the rows of its pack those levels lie in; block-compressed rows go to the device as
they are stored. A texture none of whose rows decode is a mid grey.

## The day

The World screen is lit by GTA VI's own day, read from the install's timecycle, weather and visual
settings: the sun's place at the panel's hour, its light and tint by the weather the Weather bar
names, and the tone curve. Lights around the camera, those the maps' entities place and the maps'
level-of-detail lights, are the frame's point lights. The level's water is drawn as a blended
surface over its quads.

## The camera

| Input | Does |
|---|---|
| Drag in the view | turns the camera where it stands |
| W A S D, or Z Q S D | forward, left, back, right |
| E or Space, C | up, down |
| Shift | four times as fast |
| Mouse wheel | multiplies the speed, from 1 to 2,000 m/s |

The keys are read while the pointer is over the view or the view holds the focus.

## Time and modes

- **Hour** sets the hour of the day: what is placed for some hours draws inside them, and the world
  is lit by its day at that hour. It stops a running day, so the Time bar shows the same hour.
- **Story pack** and **Shared minigames** stream the maps those packs add.
- **Script-managed groups** lists every group of the index with its maps; a group ticked streams in.

## Capture

The **Capture** section of the World panel makes a frame that can be drawn again.

- **Clean frame** draws the world alone over the whole window, with no panel, line or toast over
  it. Escape brings the panels back.
- **Copy the camera** copies where the camera stands and looks as one line,
  `world camera X Y Z YAW PITCH FOV`: metres, then the yaw, the pitch and the vertical field of
  view in degrees. A yaw of zero looks along +Y and a growing yaw turns towards −X.
- The field under it takes such a line and puts the camera there on Enter.

A shots script reads the same line, sets the hour, waits for the world to settle and measures
the frame against a reference image ([screenshots](../guides/Development_Workflow.md#screenshots)).

## Limits

- The far stand-ins of vegetation, the lists of distant impostors, are not drawn.
- A grass instance's colour and a prop instance's tint are read and not drawn.
- An interior's contents come and go at their distance with no fade, and draw at every hour.
- The sky's colour, the fog and the clouds are stand-ins, and a weather is its base cycle alone:
  no modifier is laid over it.
- The level-of-detail lights are lit while the night's emissives are; a distant light with no
  partner is not drawn.
- The water is its quads in one colour: the rivers, lakes and shore of its height map are not drawn.
- A map's occluders are read and cull nothing.
- An emissive surface glows by its colour map at one strength.
- A blended surface casts and receives no shadow.
- A change set's conditions are not evaluated: every change set read counts.
