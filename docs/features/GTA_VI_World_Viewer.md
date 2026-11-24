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

A script-managed map belongs to a group of maps a script switches together. With a key snapshot, the
world-state selectors of each mode's game-shape archive pick, hour by hour, which script-managed
groups are on, as a new game shows them; without one, none is. The same section lists the groups,
and a group ticked there streams its maps in; from then on the hour no longer sets them, until
**The game's own groups** puts them back.

An archetype is found by its 64-bit word among every map types body of the indexed archives, and a
model by the word its drawable or fragment states, or by the one drawable dictionary that holds a
drawable under every name of a dictionary's archetypes. A time archetype draws only inside the
hours it states.

## Streaming and level of detail

`crates/domain/or-world/src/streaming.rs` holds the rules, with no file or device in them:

- A map is wanted while the camera is within 50 m of the box it streams by. Its level-of-detail
  parent maps are wanted with it. A map's body is read when it is wanted, on a worker.
- Every tier of map streams in at any speed.
- A node with a parent is judged by the distance to its **parent**, so the children of one parent
  appear together. A parent gives way to its children once every child map is loaded and every
  child's model is on the device; until then it draws alone, so no hole opens while models arrive.
  It stays whole until its children are a band short of the distance it states for them, and fades
  out over the next 20 m.
- A node is whole within the distance it draws to and fades out over a band past it: 20 m for the
  high, orphan, first and second levels of detail, 25 m for the three farthest, 5 m for an orphan that
  draws to 20 m or less.
- A far node with no parent that arrives in view fades in over five selections. A node of the high
  level, an orphan and any node with a parent are there at once, and so is everything after the camera
  jumps more than 5 m, moves faster than 80 m/s or the scale changes.
- A distance is scaled by what the archetype is: terrain and environment draw shorter at the near
  levels, a grass list at 0.8, keeping the part of the distance inside the model's own radius whole.
  A prop list is never drawn shorter than it states.
- A list of instances draws by the distance to its box, less each instance whose own entity draws.
  A grass list draws each of its instances within the distance the list states for one instance,
  scaled with the list, fading out towards it.
- An interior a map places draws its contents, and the entity sets its instance names, while the
  camera is near it.
- The **Level-of-detail scale** multiplies every distance, and a lens narrower than 45° draws farther.

The maps of occluders stream by a tier of their own, on workers of their own, beside the maps the
forest draws.

Streaming and selection run on a thread of the world's own, which selects again when the camera
has moved, a map came or went, a model did, or a fade is still under way. A frame reads the last
selection and never waits for one.

## Drawing

A selection's instances stand in chunks, a map's entities or one list each, with the sphere that
holds them. A frame culls the chunks and then the instances to what the camera sees, on every
core, gives each instance the level of detail its distance draws at, and lays the instances out a
model and a level; when neither the camera, the selection nor what is on the device changed, it
does nothing.

- The occluders of the occluder maps around the camera, the largest on screen first, are drawn
  into a small depth image of their own, and a chunk whose sphere they hide is not laid out.
- A model keeps every level of detail its drawable holds. Its meshes are written into shared pages
  of vertices and indices, and draws of one page and one material go out together where the device
  can draw several at once (`crates/runtime/or-renderer/src/world.rs`).
- A mesh is drawn with its whole material: its family (one, two or four layers, decal, glass, grass,
  terrain LOD, emissive), each layer's colour, normal and material maps, its lookup, tint and palette
  maps and its constants, in the bucket its shader states: opaque, cut out, emissive, decal and
  blended surfaces draw in that order. A surface that only marks water for the game is not drawn.
  Metalness and roughness come from the material maps, and the two vertex colours carry the palette
  column, the layer weights, the tint and the glow.
- An emissive surface glows by its material's strength, its map and its second vertex colour, going
  to its night value by the day's own blend, at the day's exposure.
- The far stand-ins of vegetation draw as cards: one quad a stand-in, facing the camera, in the
  passes and buckets the cut-out meshes draw in and into the sun's shadow. Each species is baked
  once, from a fixed set of directions, into an atlas from the least detailed level of its own model, once that
  model and its colour maps are on the device; until then its cards draw nothing. Which species each
  stand-in names is worked out at index build from the far stand-in lists of every map.
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
settings: the sun's and the moon's place at the panel's hour, and the weather the Weather bar names,
its cycle with its modifiers laid over it. The sun's colour, the sky's dome and the fog's colours
come from a model of the atmosphere with the install's own values; the frame is exposed by the
install's exposure curve, metered on the light of the moment, and the tone curves are the visual
settings'.

The lights around the camera are those the archetypes of its entities hold, those the entities
place themselves and the maps' level-of-detail lights where no placed light stands, each lit in the
hours it states and scaled by the day's exposure; the brightest are the frame's point and spot
lights. A level-of-detail light fades out at the reach its size class has in the visual settings.

The level's water is drawn as a blended surface over its quads, coloured by its depth over the
ground the level's height map gives and by the day's water.

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
  The line above it says whether the groups on are the game's own at this hour or the ones chosen.

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

- A far stand-in's card is baked from its species' own model, not read from the game's own atlas;
  a species seen nowhere near the camera has no model on the device and draws no card. A stand-in
  and the prop that takes its place near the camera both draw at the edge between them.
- A grass instance's colour and a prop instance's tint are read and not drawn.
- An interior's contents come and go at their distance with no fade, and draw at every hour.
- The atmosphere scatters light once, so the sky and its light are dimmer than the game's; the
  clouds are not drawn. The exposure is metered on the moment's light, not on the frame drawn.
- A light falls off by the frame's own curve; capsule and area lights draw as points, a spot's inner
  cone is the frame's, and coronas and light volumes are not drawn. A distant light with no partner
  is not drawn.
- The water has no waves, reflection or refraction, and its colour by place (the level's fog atlas)
  is not read: one medium colours all of it by depth.
- The occluders hide whole chunks, never an entity alone.
- A blended surface casts and receives no shadow.
- A change set's conditions are not evaluated: every change set read counts.
