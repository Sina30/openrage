# Wardrobe parity

What each of the three clothing tools does that the Wardrobe does too, will do in a named wave, or leaves out by a ruling, row by row, and the costs of making clothes in a project that the Wardrobe answers.

Status: the matrix of 2026-10-09, written by wave P18 of [Workbench_Plan.md](Workbench_Plan.md), and read again against the owner's words by wave P31 on 2026-10-20: every row has one of the three statuses below, and the requirements that were still open after the re-verification are [at the end](#requirements-still-open-after-the-re-verification-of-2026-10-20) with their own status and the questions for the owner.

## How to read it

The rows are the rows of the [feature matrix](Clothing_Tool_Design.md#feature-matrix) of [Clothing_Tool_Design.md](Clothing_Tool_Design.md) where Durty Cloth Tool, grzyClothTool or Vestium answers yes. A row where none of them does is left out, and so is a row whose figure is only a number (the drawables of a slot, the interface languages, the licence). Each row has one status:

- **Present** names the screen or the engine that has it, in the tree now.
- **Planned** names the wave of [the second plan](Workbench_Plan.md#the-second-plan-from-the-audit-of-2026-10-08) that builds it. A wave that waits for a ruling says which.
- **Excluded** names the ruling or the held-back line that leaves it out. The rulings are the ones the plan takes by default, which a sentence from the owner reverses.

Nothing here says a pack loads in game: nobody can run GTA V here, and a build says only that every file it wrote reloads through the readers.

## Project

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Project file | all three | Present | The project file, and a pack's `wardrobe.json`, both plain text |
| Diffable, Git-friendly | Durty, Vestium | Present | Text sources and a `wardrobe.json` that is written the same way every time; a save writes the bytes the file holds already as they are, and the workspace it states changes only when the person says so (P16) |
| Autosave and rotating backups | grzy, Vestium | Present | A pack keeps a rotating set of its versions beside it, and one that does not read is a card that offers its last good version and keeps the text that did not read (P19) |
| History and rollback | Durty, Vestium | Present | One stack of Undo for the Wardrobe, with a step for every edit, and a Redo; the project rolls back to any step of its History, kept beside it and out of Git (P16); a pack, one garment or one texture goes back to any version it kept (P19) |
| Pristine originals kept | Vestium | Present | A replacement writes over no file: the garment names a new source and the file it replaced stays where it was, with Undo to name it again. Nothing is written over, so nothing keeps a second copy of a file (P19, P33, P24) |
| Several packs in one project | Durty, Vestium | Present | Several Clothing contents, each its own pack, chosen by the pack's name in the Wardrobe header |
| Reopen a built resource as a project | all three | Present | A FiveM resource is adopted as content in place; a singleplayer DLC, an `.oiv` and a grzyClothTool project are imported as a project of sources with everything the metadata states, and what could not be placed is listed and kept under `files/`. In the app it is Open Built Pack… (File ▸ Project and the palette) or a drop of an `.oiv`, a `.gctproject`, or a `dlc.rpf` on the Project workspace, which asks *Reopen this as a project?*, runs as a job with its count of files and a Stop that makes nothing, and ends in a *Pack reopened* report that names each file not placed and where it was kept; the result is a new folder and the pack is never written, so it has no Undo. Headless it is `openrage-cli wardrobe reopen` (P26) |
| A garment as one package | none | Present | A garment's sources and settings in one folder that another pack adds as an equal garment: Package garment… on the garment and Add package… on the garment and, with none selected, on the pack, in the Wardrobe, where Add package… is one step Undo takes out, and `wardrobe package` and `wardrobe add --package` headless (P26) |
| Headless or CI build | none known | Present | `wardrobe validate`, `plan`, `build` and `verify` over a project, `just wardrobe-check` runs them in order, a split output is read back resource by resource, and `serve` has routes that add, set, fix and waive (P26) |

## Import

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Drag and drop, folders, name resolution | all three | Present | A drop on the Wardrobe or Add clothes…, of a folder of any size with its progress and a Stop; a folder dropped twice adds its garments once; a name behind `dlc^` is read, a protected file is listed by name with its reason, and Say what they are… sets the ped and the slot of the selection in one step (P22) |
| Gender and slot detection, a dialog for the rest | all three | Present | The names say the ped and the slot, and what they do not say is a row marked `?` with a chip, never a dialog that stops the work; the dialog is there when a person asks for it (P22) |
| Image to texture dictionary | all three | Present | An image dropped on a texture's tile makes that texture, as a textures source of the variation that holds the image; an image named as a texture and dropped with its model makes the dictionary of a new variation (P22, P33) |
| Duplicate detection | Durty, Vestium | Present | A duplicate is found by what the files hold, the same ped and slot, model and textures, with its own chip and a Fix… that removes the later copy (P22) |
| grzyClothTool project file | grzy | Present | The one foreign project format read (R16): a `.gctproject`, a zip XORed with `0x2B` that holds a resource, is imported as a project, from Import built pack… or a drop in the app and `wardrobe reopen` headless; not Durty's sealed formats. No file of the tool's is staged, so the reader is held to that description (P26) |
| Menyoo and CodeWalker XML import | Durty | Excluded | CodeWalker XML for drawables is held back, which would reopen the ruling of 2026-09-22 on XML (R10); Menyoo is not read (R16) |

## Slots and model

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Every component and prop slot, both freemode genders | all three | Present | The slot and ped lists of the engine; Tops come first in the list |
| Addon peds | Durty | Present | The addon ped content, which has no workspace of its own |
| Replace modes (R* DLC, base game) | Durty | Present | P29, by R20: a ped's files replace a collection of the game or its base drawables, with the install's DLC list to choose from; marked unverified in game in the plan, the Problems list and the Rules window |
| Reserved slots and dummies | all three | Present | A garment with no model holds a place; P29 adds replace on add, leave on delete (the Rules window), and fill and sort a slot (its heading's menu, `wardrobe dummies`). P38 gives a texture the same: Add an empty texture and Move the empty textures to the end in the Garment panel, Fill on the row of an empty texture, and replace on add takes the first empty texture a garment holds, each a step of Undo, and a build counts a place as a texture of the drawable and ships no file for it |
| Automatic split above the cap | all three | Present | P29, by R7: the cap per slot is 128 unless the project says another up to 255, and a slot past it is written as `<collection>`, `<collection>_1` and on; a build over a ceiling still divides between contents, and a content is not cut into resources |
| Frozen positions | none of the three | Present | P29: a build that goes through writes `wardrobe.lock.json`, and a garment that would take another drawable stops the next build unless a reason is waived |

## Garment attributes

| Feature | Who has it | Status | Where |
|---|---|---|---|
| High heels | all three | Present | The Heels flag of shoes, and the preview raised by it |
| Hat hair squash and hide hair | Durty, Vestium | Present | The hair scale of a hat and the height of a hair, as the staged creature metadata drives them. Hide hair is a preset that writes the hair scale -1.0, the round number staged hats state at the least, and the preview takes the ped's own hair off for a hat that states it or less; it is measured on staged hats and not verified in game. (P28) |
| Hide ears and a hat cut | Durty, Vestium | Excluded | No flag or value of the engine's tables or of the staged documents states either, so no control writes one (D15, which takes names and values from the engine through the corpus); a sample that states one decides (question Q11 below) |
| Behaviour flags | all three | Present | The 17 bits of `ePedCompFlags` some staged document states, by the engine's names, with the rest kept as the number; the drawable's `propMask` bits, a prop's render flags and how firmly it stays on |
| Audio presets | all three | Present | The ids the staged drawables of a slot state most, as the hashes they are, with their use counts |
| First-person alternates | Durty, grzy | Present | The alternate model a component streams in the first person view, written as the engine's table states it; no staged file states one. The dressed ped's First person view wears it (P38) |
| A time cycle modifier per texture | Durty, grzy | Excluded | No staged document states one, so no control writes one (D15); question Q11 below |
| Hair alternation models | Durty, Vestium | Present | The alternate variations of a pack, as sets: the typed `pedalternatevariations.meta`, imported from a server's pack, written, read back and edited in the Garment panel. The dressed ped wears the sets in force: Alternates chooses those a worn garment causes, none or one set, and Model says which set drives a garment's model (P38) |
| Shop metadata with price and tags | Durty | Present | A shop's own metadata is imported, its entry ids and costs kept and the fields it states carried, and the Shop section of Garment edits the entry of each texture: its id, its price, its restriction tags and its forced components, and any other element it states (a lock hash, variant components, outfits: no staged shop file states them) as the XML it is written in, each an Undo step (P34); a garment also carries tags and a group a person writes, which a build reads only through the inclusion rule of a build (P37) |
| Tattoos and overlays | Durty | Excluded | Held back: tattoo and overlay editing |
| Server framework configs | Durty | Present | A build's clothes dump option carries the dump of the pack for server scripts, as JSON and as a Lua table, in the shape of the dump a server's resource ships ([the clothes dump](Clothing_Tool_Design.md#what-the-engine-builds)) |
| Tags, groups, search | Durty, grzy | Present | A fuzzy filter over a garment's place, slot, name, the name the game gives its model, group and tags, a ped filter and the chips; tags and a group are set on every garment picked; a query line over them (tag, group, ped, slot, polygon count, a missing level of detail, a copy of another garment, a problem code) selects the same garments in the list and on the command line, and a saved query is a chip of the list (P37) |

## Beyond the three tools

What none of the three tools has, from the power-user catalogue of [Clothing_Tool_Design.md](Clothing_Tool_Design.md#power-user-catalogue).

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Query bar and saved queries | none of the three | Present | The filter of the Wardrobe reads terms as well as a name: `tag:`, `group:`, `ped:`, `slot:`, `polys>`, `lod:missing`, `is:duplicate`, `duplicate-of:`, `problem:` and `is:problem`, each turned round with a `-` (`wardrobe query` on the command line). Save query… keeps a line in `wardrobe.json` under a name and the Saved queries menu under the filter lists it, and it is read again each time, so it is a smart group; saving and forgetting are steps of Undo (P37) |
| Bulk edit on a query with a dry run | none of the three | Present | Edit the 12 listed… makes one change (add or take off a tag, put in a group or out of it, say which ped, take out of the pack) to every garment the list shows, says first what it would change in how many garments and which refuse, and is one step of Undo (`wardrobe bulk` is a dry run until `--apply`) (P37) |
| Review mode | none of the three | Present | Review versions… compares two versions of the pack read-only, by the garment's stable id: the garments, textures and settings added, changed and removed, and the pictures of both sides of each garment that differs (P37). Before, beside A and B of the dressed ped, wears the pack as it was before its latest change, from the versions the Wardrobe keeps, to set against B in the viewport (P31); two versions the person chooses are not compared in 3D (question Q14 below) |
| Per-asset provenance and licence | none of the three | Present | A source states `credit` (`author`, `url`, `licence`) in its `asset.json`, Credit garment… writes it into the sources a garment names, and a build with Credits file on writes `credits.md` in a FiveM resource, with the sources that state none listed apart (P37) |
| A whole pack as one package | none of the three | Present | Export pack as a package… writes a zip of the pack's sources, settings, cloth and carried shop files and never a compiled game file; Unpack a pack package… makes it a clothing content of the project, and `wardrobe pack` and `wardrobe unpack` do the same from the command line. A garment still travels as a folder (P37) |
| Inclusion rule by tag | none of the three | Present | A build's Only garments tagged ships the garments filed under one of its words; the rest keep their place as a drawable with no model so the other numbers do not move (P37) |
| Dependency graph and size map | none of the three | Present | The Size map section of the Build tab ranks the files of a build by content, by garment and by slot, and lists what the garments name with the garments that share it and what leads outside the pack (`wardrobe sizes`) (P37) |
| Memory-budget solver | none of the three | Excluded | Ruling R18 of the second plan leaves it out until the owner decides the power-user catalogue after P28; Budget textures… fits the textures to a target and the strip shows the figures against the ceilings |
| A file for each garment, so that Git merges two edits to one pack | none of the three | Excluded | `wardrobe.json` is one canonical file, so two people who edit different garments of one pack meet in one file; splitting it into a file for each garment would let Git merge them, and R18 leaves it to the owner with the rest of the catalogue ([Clothing_Tool_Design.md](Clothing_Tool_Design.md#the-power-features-of-wave-p37)) |

## Textures

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Optimizer (format, mips, power of two, resize) | all three | Present | Optimise textures… in the header, the garment panel and the Project menu, for the garments picked or the whole pack, with a Stop: BC1 or BC3 by alpha, mips, the nearest power of two and a size cut by a ratio or to a longest side. A source is copied and the garments name the copy; only the DDS of a texture that changes is written, a mip chain is added under the blocks that were there, and a halving drops the top levels and keeps the rest as they were; a compiled .ytd, a picture and a reference are listed with why they were left (P24) |
| UV crop to the mesh | Durty, Vestium | Present | Crop texture coordinates in the Garment panel, the Project menu and the palette (and `wardrobe mesh --crop-uvs`) moves the coordinates of the garment's models into the smallest aligned power-of-two part of the texture that holds them and cuts every texture they read to it, the pack's colour textures and the normal and specular maps embedded in the models, by copying whole blocks of every mip, so no block is encoded again; a crop that would keep more than half is not made, and a texture made from pictures is refused by name (P30) |
| Normal and specular generation | Durty | Present | Make normal and specular maps… in the Garment panel's Fix menu, the Project menu and the palette (and `wardrobe maps`) lists the maps of each diffuse texture, a name with the word `diff`, and makes them as new pictures in a copy of the texture's source, which the garments name; the source it was is untouched and Undo takes the copy away. They are estimates from the brightness of the diffuse (P35, [the maps](Source_Format_Design.md#maps-made-from-a-diffuse)) |
| Quality-aware texture budgeting | all three | Present | Budget textures… picks the size and the format of each texture that fit a memory target, with the least PSNR lost per byte saved, and shows the plan with the PSNR of every choice and the texture as it is beside the texture as the plan would store it, before anything is written; Accept names lighter copies and Undo takes them back (and `wardrobe budget`, a dry run until `--apply`) (P35, [the budget](Source_Format_Design.md#budgeting-a-textures-source)) |
| Watch folder bound to a slot | all three | Present | Watched folders in the Garment panel binds a folder an external editor saves into to the diffuse, specular or normal slot of a texture of the garment; a picture saved there lands in the source and the live recook draws it (and `wardrobe slot`). The bindings are kept in the pack's private folder (P35) |
| Auto-fix of shader parameters | Durty | Present | Complete materials states the constants a shader declares that a material of the garment's models lacks, at the values the shader declares, in bulk over the garments picked or the whole pack (and `wardrobe mesh --materials`); the finding `material-parameters` offers it as a Fix. It needs the shader database of an install (P35) |
| Auto-fix of a missing sampler and of an Enhanced material | Durty | Excluded | A sampler a material lacks is left, since an entry would state an empty binding and not the shader's default, and an Enhanced material is not written (D15). P38 looked again: the material editor's record says the game falls back to the shader's default only for a slot that is removed, so an entry added empty is a slot cleared, and the shader database names no default texture to put in it; question Q19 below |
| Live external editor round trip | all three | Present | An image saved in the textures source of a variation is cooked again by the live recook and drawn again, with no convert button |
| Material and embedded texture editor | Durty | Present | The Model and Texture workspaces |

## Mesh

| Feature | Who has it | Status | Where |
|---|---|---|---|
| LOD generation and decimation | Durty | Present | Add levels of detail in the Garment panel, the Project menu and the palette (and `wardrobe mesh --levels`) gives a model the Medium and Low levels it lacks, made from the High level by a decimator that keeps every vertex it leaves exactly as it was and keeps the High level's shaders and vertex declarations (ruling R17 of 2026-10-19: no new material, layout or preset, and no Blender); the shares are the person's, a geometry that cannot be cut as far as asked says so, and a strip of hair cards that cannot be cut at all makes no level (P30) |
| Freemode rig validation and repair | Durty | Present | Check mesh lists a group no skeleton knows, a bone spelt otherwise, weights that are noise or do not add up and the heel of a pair of shoes; Repair rig gives the weight on such a group to the bone above it and cleans the weights, and Clean weights does the weights alone, each a new source and one step of Undo (P30) |
| Lossless repair of a model's counts, on request | Durty, Vestium partly | Present | Repair model… and the Repair step of Fix all safe rewrite the counts a compiled .ydd states against the runs they describe, where that loses nothing, into a source made of the repaired file; the Log keeps what was rewritten, and a protected asset (FXAP) is refused by its name (P24) |
| Mirror left and right | Vestium | Present | Mirror model turns left and right over, with the skin given to the bone on the other side; mirroring twice gives back what was, and a ped component needs the freemode skeleton from the game folder (P30) |
| Cloth physics | Durty | Present | A cloth is linked to its garment, a cloth painter extends a build, and the Cloth section of Garment sets the wind scale the build states in the cloth it ships and reads back (P34); the physics themselves are the game's |

## Preview

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Dockable 3D preview | all three | Present | The dressed ped, between the list and Garment |
| Whole dressed ped driven by the metadata | all three | Present | Outfit, Tint, Skin, Heels and Show (Without the pack and With the pack, which other tools call A and B), the props worn at the bones of their anchors, Light (a direction, a height, a strength, or the sun at the hour of the viewport's time of day) and the alternate model a garment is worn with (P34) |
| Level of detail | none recorded | Present | Level in the tools of the dressed ped draws the high, medium, low or very low models of the garments worn, with the triangles and vertices each costs and how many garments hold a model at it; a garment without one is drawn at its first and the preview says so (P28) |
| Wireframe and vertex views | none recorded | Present | Draw in the tools: textures, shape, edges, the normal of each vertex, its first texture coordinates, its first tangent and its first vertex colour as colours (P28, P34) |
| Skeleton and grid | none recorded | Present | Bones draws the skeleton of the ped read from the install, posed by the clip while one plays, and Grid is the viewport's floor grid (P28) |
| Alternation preview | Durty | Present | Alternates chooses which sets of alternate variations are in force on the dressed ped (those a worn garment causes, none, or one set), and the garments and the ped's own drawables they name wear the alternate model; Model says which set drives a garment (P38) |
| Floor for heels | Durty | Present | A ped raised by heels stands on a floor of warm lines at the height they raise it to, beside the grey grid at the ground (P38) |
| First-person view | Durty | Present | First person puts the camera at the head bone of the ped, raised by the heels, with the engine's default field of view of 45 degrees (the first person camera's own value is metadata of the install, which is not read), turns the head with a drag, leaves out the garments flagged hidden in the first person view and wears the first person alternate; it needs the ped's skeleton, which the install gives (P38) |
| Shadows and time of day in Shaded | Durty | Present | The Game daylight of the Light menu: Shadows casts the sun's shadow of the game's passes or takes it away while Draw is Shaded, and Time of day is the viewport's own hour (P38) |
| Mesh physics preview | Durty | Excluded | The cloth is the game's simulation, which would need its cloth and wind driven here and not the shape of the file; question Q14 below |
| Hot swap after a replace | none recorded | Present | A model or texture replaced, taken back, or recooked is drawn again without reopening anything (P28; the replace and its Undo are `a_replaced_model_is_drawn_without_reopening_and_its_undo_draws_the_old_one_again`) |
| Editable base components | none recorded | Present | Ped's own parts takes a slot of the ped off or wears another of its drawables there; a garment worn in the slot comes first and Ped without hair is the one switch for a bare head (P28) |
| Animations | Durty, grzy | Present | Motion plays Idle, Walk, Run or Sprint from the install's default movement of the ped, or the first clip of a `.ycd` file, on the ped's skeleton, with play, pause, loop, a speed of a quarter to twice and a choice among the clips of the dictionary; a clip that drives few of the bones is refused with the figures. Presets loop by choice and a clip file follows its own flag (P28) |
| Catalogue thumbnails | Durty | Present | Each row draws its first texture and resting on it shows every texture of the garment, each decoded once from a small mip. Pictures draws each garment of the pack alone, one picture or a turntable of 4, 8 or 12 turned round it, as WebP files on a clear ground into a folder picked, on a worker with a device of its own (P28) |

## Validation

| Feature | Who has it | Status | Where |
|---|---|---|---|
| Rule list with severities and waivers | Durty, Vestium | Present | The rules of the engine as data, each with a severity, whether a reason can set it aside and, for twelve that read what a file states, a limit; a project sets which rules are on, how serious each is and where its limit stands in the `rules` of its `wardrobe.json`, from Rules… in Problems or by hand, and the list is written out by Export the list… and `openrage-cli wardrobe rules`; the Problems list filters, orders, groups and exports its rows (P23) |
| One-click fixes and a batch pipeline | Durty, Vestium | Present | Fix…, Locate… and Remove this texture or Remove the cloth (or Remove from project for a file) for one finding, each a step of Undo; Fix all safe runs repair, textures, order and mask as one step of Undo with a report for each, and lists what it leaves and why (P24) |
| Reload and check every written file | Vestium | Present | A build reads back every file it wrote and checks the pack against it |

## Build

| Feature | Who has it | Status | Where |
|---|---|---|---|
| FiveM, Legacy | all three | Present | A FiveM resource build |
| FiveM, Enhanced | Durty | Present | A FiveM build for Enhanced, written from the same plan |
| Singleplayer `dlc.rpf` | Durty, grzy | Present | A singleplayer DLC build ships a clothing pack, hats and props included, laid out as the game's own packs are and counted against the most entries an archive holds ([a clothing pack in a pack](../features/Dlc_Pack_Creation.md#a-clothing-pack-in-a-pack)); an `.oiv` installs the same pack |
| alt:V and RageMP | grzy | Excluded | Held back: alt:V and RageMP output |

## What the project approach costs

Making clothes in a project has costs that Durty Cloth Tool, which has none, does not. Seven remain, each with what answers it. The first five are the plan's own list of what stays a disadvantage, and the last two are risks of [Project_Model_Design.md](Project_Model_Design.md#risks) that reach the clothing creator.

| Cost | Answer |
|---|---|
| More concepts than Durty Cloth Tool | The Clothing pack template opens on the Wardrobe and hides Project and Builds from a beginner, who meets them only by pressing the Project button; the screen says Content and Builds |
| Two copies on disk, the sources and the build | The exact DDS keeps a source compact, and Clean empties the build |
| Nobody here can prove that a pack loads in game | A build says that every file it wrote reloads through the readers, and the line that says more stays unticked until a tester records it (R9) |
| A strip that can overflow | The buttons that fit are shown and More catches the rest, and Ctrl+1 to Ctrl+9 reach every workspace |
| Two windows are two processes | They share one preferences file, which a save reads again and writes only the keys it changed |
| A matrix of builds that confuses | It appears only from two contents or two builds, and names are plain |
| Two projects can write one content through a link | One project writes it and the other reads it with a banner, and an edit through a link is refused with a sentence that names the project that writes it |

The five of the plan are in [Workbench_UX_Design.md](Workbench_UX_Design.md#what-stays-a-disadvantage); the seven here are the ones a clothing creator meets, and an owner who counts differently changes this table and not the plan.

## Requirements still open after the re-verification of 2026-10-20

Wave P31 read the 638 requirements recovered from the owner's words against the tree. The verification of 2026-10-19 and a run after P37 left 82 open; they are in [workbench-open-2026-10-20.json](../audit/workbench-open-2026-10-20.json), each with what P31 did about it in its `p31` key. The wave closed what was smaller than a wave, the nine marked Present below that say P31, and each of the others has a status:

- **Present** names what was built, in the tree now.
- **Planned** names the wave that builds it.
- **Excluded** names the held-back line, the ruling or the rule of the repository that leaves it out.
- **Question** is a requirement that no wave builds and no ruling excludes, so a sentence from the owner decides; the question is under the table, and nothing builds it until he answers.

| Requirement | What the owner asked | Status | Where, or why |
|---|---|---|---|
| WAR-17 | A workspace is a layout independent of any content kind: Open in offers any other workspace | Present | P31: Open in lists every workspace and none is greyed; one that cannot show the asset or the content says why in a line under its name, opens when pressed and says the same in a toast. The Map workspace is the row of the Map view below |
| WAR-23 | Each drawback of the project approach has a stated resolution in the design | Present | P31: seven drawback-and-resolution pairs in [Workbench_UX_Design.md](Workbench_UX_Design.md#each-drawback-of-the-project-approach-and-what-answers-it), next to the five costs that remain |
| WAR-144 | An A/B control shows the original against the edit in the preview | Present | P31: Show has Before the last change, the ped wearing the pack as it was before its latest change, read from the versions the Wardrobe keeps, to set against B, the pack as it is. The original of a replaced texture is not kept, because nothing is written over a file, so one texture is compared through Earlier versions and Review versions |
| WAR-155, WAR-101 | Fix all safe runs repair, levels of detail, textures and order; the mesh doctor offers levels, UV crop, weights and the heel check as steps | Present | P31: a box in the Fix menu, Also add missing levels of detail, adds a Levels step to the same one step of Undo; it is off to begin with because a level is a change to a model's geometry. The Mask step stays, added by P27. The doctor's other operations make a new source the person asks for and stay on-demand steps of Undo, not steps of the pipeline |
| WAR-237 | Onboarding: a sample project, a tour and tooltips that link to the documentation | Present | P31: a command the guide explains, and each tool of the dressed ped, ends its tooltip with the guide section that explains it, and Shift+F1 with the pointer on the control opens the guide there |
| PRO-129 | An import only adds, and a toast says what was done with an Undo | Present | P31: an import ends with a toast that says what it made and has Undo import, besides the report window |
| PRO-173 | The project verbs of openrage-cli do headless what the Project workspace does | Present | P31: openrage-cli project apply-template, check-sharing and check-original |
| WAR-182 | Review mode shows a read-only diff of two revisions with a 3D A/B comparison | Present | P31: Review versions lists the difference of two versions and Before the last change wears the previous kept version in the viewport against B. Two versions chosen by the person are not compared in 3D: question Q14 |
| PRO-128, IMP-157 | The app warns before a push that would include a source taken from the game or the build | Question (Q7) | Check before sharing warns when the person runs it, and openrage-cli project check-sharing makes the same check with no window and exits 1 for a path of this machine or a source taken from the game, so a pre-push hook of the person's own can run it; the private folders it lists do not fail it, since Git never sees them. The editor installs no hook and cannot see a push, so the warning at push time is the person's to set up until the owner answers Q7 |
| WAR-220, WAR-221, WAR-222, WAR-223, PRO-180 | A design draft exists for the weapon, particle, map and other project views, after the re-verification | Present | P32: [Workbench_Views_Design.md](Workbench_Views_Design.md) drafts the weapon, particle and map views, ranks the others and asks its rulings. Nothing in it is built |
| WAR-213 | Tattoos and overlays, and graphics packs (timecycle, weather, texture replacement), are named content kinds | Excluded | Tattoo and overlay editing is on the held-back list of [Workbench_Plan.md](Workbench_Plan.md#held-back). Graphics packs are a parked label with no design: question Q1 |
| IMP-057 | For models asset.json states compile.mode edit or author | Excluded | Author mode is on the held-back list, and R10 keeps it so. Edit mode and compile.base are built |
| IMP-121 | drawable.json holds the vertex layout as a named preset and a shader can be chosen | Excluded | The shipped shader and vertex-layout presets are on the held-back list (R10). A material's parameters are editable; its shader is not changed |
| WAR-55, WAR-56 | Menyoo characters import as garments; Durty, grzyClothTool and Vestium projects import | Excluded | R16: the one foreign project format read is grzyClothTool's .gctproject. Durty's formats are sealed and Menyoo is not read. R16 is a default the owner may reverse |
| IMP-064 | Inside a project an asset is identified by its id and not by its name; contents, builds and links point at the id | Excluded | R6 (a default): paths are kept, as the owner ruled for links, and the id detects duplicates, moves and the build diff |
| PRO-117, IMP-147 | A source that did not change is copied to the output exactly and not re-encoded | Excluded | A save writes OpenRage's deterministic canonical form and byte identity with shipped files is not the goal (AGENTS.md). Only the DDS blocks of a texture and files kept whole are exact, which is the texture ruling |
| WAR-160 | A FiveM game-build selector adjusts warning thresholds, such as the ped-metadata slots left free | Excluded | No staged file and no table of the engine states a figure per game build, and a figure is not invented (D15). The Legacy and Enhanced edition setting exists |
| WAR-166 | Game-limit rules: more than 160 alternates per item, a texture dictionary over 16 MB, BC7 where the build is DX11 only, the shared texture pool | Excluded | The shared texture pool is the game's own configuration, which no file of a pack states. The two figures are one limit, as read on 2026-10-20: 160 alternates count alternate 0, which is the drawable itself, so the rule counts 159 switches. The reference pack states at most 103 switches for one asset (question Q16) |
| WAR-132, WAR-133, WAR-135, WAR-137, WAR-127 | Hide ears and hat cut checkboxes, named audio presets, a time cycle modifier per texture, shop lock hash, variant components and outfits | Excluded | D15: names and values come from the engine through the corpus, and no staged document states a flag or value for them, so no control writes one. Hide hair is a preset that writes the round number staged hats state, and audio presets are the ids by slot. A sample that states one decides: question Q11 |
| WAR-86 | Auto-fix of shader parameters, including a sampler and an Enhanced material | Question (Q19) | Complete materials states the constants a shader declares. A missing sampler is left, since an entry would state an empty binding and not the shader's default: the game falls back to the shader's default only for a removed slot (the material editor's record), so adding one clears it, and the shader database names no default texture for a sampler. An Enhanced material is not written. It needs the shader database of an install |
| IMP-036, IMP-124 | A .ybn becomes the kind bounds (bounds.glb with named nodes and bounds.json); collision is carried by nodes named by prefix | Question (Q1) | Source_Format_Design.md records it as planned, later. A .ybn is kept whole under files/ and shipped as it is (R3) |
| IMP-037, WAR-116 | A .yld becomes the kind cloth (cloth.json), so cloth round-trips through a source folder | Question (Q1) | Planned, later. A cloth is a compiled .yld the garment links, and the cloth painter and wind scale edit what a build ships (L, P34) |
| IMP-038, IMP-126 | A .ycd becomes the kind clips (glb bone tracks and clips.json); animation tracks travel through glTF | Question (Q1) | Planned, later. A clip dictionary is kept whole and played in the Animation preview |
| IMP-039 | A .ypt becomes the kind particles (JSON rules and DDS) | Question (Q1) | Planned, later. The Particle workspace edits a .ypt file in place |
| IMP-040 | An .awc becomes the kind audio (WAV files and streams.json) | Question (Q1) | Planned, later. The audio container is edited in place |
| IMP-042, PRO-120 | Every kind of game file the app edits has its own decompile target | Question (Q1) | Seven extensions have a source form (texture dictionaries, drawables, drawable dictionaries, fragments and the metadata files). The rest are kept whole under files/ and shipped as they are (R3), and the mapping of each format to its target is documented |
| PRO-17, PRO-19, WAR-16, WAR-18, WAR-204, WAR-205 | The set of content kinds is open-ended (Map, Weapons, Vehicles); another content's workspace shows the same project; the Map view of a project with no map offers to add one | Question (Q1) | The mechanism is open-ended and three of the six named kinds exist (Files, Clothing, Addon ped). Map documents ride as loose files of the Files content with a type the person states. P32 drafted the Map view: [Workbench_Views_Design.md](Workbench_Views_Design.md#maps) |
| WAR-207, WAR-208, WAR-209, WAR-210, WAR-211, WAR-212, WAR-214, WAR-215, WAR-216 | The project system holds weapons, props, vehicles, interiors, animations, sounds, particles, interface and data patches as content kinds, built in the order clothing, addon peds, vehicles, each with its own metadata, rules and screen | Question (Q1) | None of them is a kind of content. The Clothing_Tool_Design module table lists props, maps and weapons before vehicles, which is not the owner's order. P32 drafted the weapon, particle, map and other views, with its recommendation of an order: [Workbench_Views_Design.md](Workbench_Views_Design.md) |
| WAR-129 | Config builders for server frameworks and tattoo shops (illenium-appearance, qb, ESX, rcore, xnTattoos) | Question (Q12) | The clothes dump (JSON and Lua table) is built, and a plugin of the emitter kind can write a framework's config. No framework-specific emitter ships, and tattoo output is on the held-back list |
| IMP-151, WAR-100 | A build ships no vanilla file: a source with origin.from_game true that the person did not change is left out | Present | P39 (owner, 2026-10-20, Q6): a FiveM resource, a singleplayer DLC and an .oiv leave out an unchanged source of the game, and a build's ship-unchanged key ships it; a pack registers and a package names only what remains, the plan says in a sentence how many it may leave out, and a pack or a package left with nothing is refused with the way out |
| IMP-152, IMP-068 | An output retargets a source's layout and writes the extension of its platform (a PS4 .otd) | Question (Q4) | Legacy and Enhanced retarget with a named stop. Only win64 is written; a source of another platform is refused. OpenRage is Windows first |
| IMP-127 | A model's textures are DDS files beside the glb, referenced by name | Question (Q3) | The textures stay inside the drawable the source cooks from; no DDS is placed beside the glb |
| IMP-041 | Metadata files stay single flat canonical XML files such as name.ymap.xml | Question (Q2) | Built as a .metadata.src folder (id, hash, from_game and the canonical XML), a deliberate divergence of wave P10 with no owner ruling found |
| PRO-98 | A build runs as a job named Project build, and Cancel lets the build being written finish | Question (Q5) | Cancel stops the build being written, removes its folder, keeps the last good build and starts no later build of the run |
| WAR-238 | The interface is translatable from the first release | Question (Q8) | R13 deferred it until after P31 so that strings are extracted once; the interface is English only |
| PRO-188 | The project system is polished to the maximum and left clean | Question (Q9) | The screens were drawn again at five sizes by P31 and the defects seen were fixed. What remains is recorded: no multi-window shell (a second project is a second process), and a read-only window stays read-only until the project is opened again |
| WAR-200, WAR-159 | A plugin write is a transaction over several files; rule packs come from plugins and are rules with a Fix | Present | P39 (owner, 2026-10-20, Q10): plugin ABI version 1 adds `write_files`, files of the project written together, all or none, as one step of the project's History; a validator granted the pack reads the clothing as data; and its finding may carry a fix that Fix… lists and applies like a rule's, as one step of Undo. A plugin built for version 0 still loads and Settings says so. A plugin's finding is not yet in the Rules window or the exported list of rules |
| WAR-146 | The preview offers wireframe, skeleton, grid, polygon and vertex counts per LOD, LOD switching and alternation preview, and shows the floor for live heels | Present | P38: Alternates and Model show a set of alternate variations in force, and a ped raised by heels stands on a floor at the height they raise it to |
| WAR-148 | Debug render modes, time of day, HDR and shadows, first-person view and mesh physics are available in the preview | Question (Q14) | P38: Light has the Game daylight, Shadows and Time of day, for the Shaded view, and First person puts the camera at the head with the engine's default field of view. Mesh physics is the one part not built: the cloth is the game's simulation |
| WAR-176 | A singleplayer pack past the archive entry limit divides into several packs | Question (Q15) | As of 2026-10-20, past 65,535 entries a build divides between contents when Split is on and refuses one content over the limit; cutting inside a content would renumber drawables, which frozen positions forbid |
| WAR-230 | A mod manager with load order, since built packs have to be installed and ordered | Question (Q17) | The Mods panel and the overlay exist. The load order is not written into the overlay the game reads, and a build cannot be installed into it. Installing is the copy of the guide's Install on a server |
| WAR-38 | Reserved positions and dummies exist for drawables and for textures | Present | P38: Add an empty texture, Fill on the row of an empty texture, Move the empty textures to the end, and replace on add (the Rules option of the dummies) take a texture the way they take a drawable, each a step of Undo; a build counts a place as a texture of the drawable and ships no file for it |
| IMP-185 | A cloth whose garment's flag is off is linked and the flag is written on, after a check on a real pack | Question (Q18) | The flag follows the link. A cloth with the flag off stays loose with a hint, and the check on a real pack was not run |
| IMP-063 | Saving a source that did not change rewrites every JSON file byte-identically, so a save produces no diff | Question (Q18) | Source::save does it and is tested; no screen or verb calls it, because the app edits a source by writing the one file that changed |
| IMP-076 | The source-format design records the advantages and drawbacks of OpenIV's open formats | Question (Q18) | It needs sample exports from the owner; CodeWalker's are recorded |
| WAR-20, WAR-24, PRO-148, PRO-182, PRO-184 | The Wardrobe has at least the features of the three tools, every button has a place, and the project system has no avoidable disadvantage | Question (Q13) | Every row of the matrix above has a status. The Excluded rows are defaults of the plan and not sentences of the owner, and the five costs that remain are by design. These close when the owner confirms them as rulings |

### Answers of 2026-10-20

The owner answered the decision sheet of 2026-10-20. Accepted as recommended: Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10, Q13, Q17, Q18 and Q19. Left for later, standing as built: Q14, Q15 and Q16. Not answered: Q11 (a sample or a statement of the flags is still needed) and Q12. The rulings of the views draft that the owner answered are in [Workbench_Views_Design.md](Workbench_Views_Design.md#rulings-the-draft-asks-of-the-owner); they are carried out by waves P39 to P44 of [Workbench_Plan.md](Workbench_Plan.md).

Second part, 2026-10-21: N5 (the French vocabulary), N6 (a bound of 100,000 m on a typed position) and N7 (Clear sky) were accepted; N4 was answered otherwise (split the particle source by rule, like the game's own particle tool keeps its files), carried out by wave P45; N1, N2 and N3, with Q11 and Q12, were not answered and stand as built.

### Questions for the owner

A sentence from the owner on any of these decides it; each stands at the default the table says until then.

- **Q1.** Content kinds and source forms beyond clothing, addon peds and files: which is next (bounds, cloth, clips, particles, audio; maps, weapons, props, vehicles, interiors, sounds, graphics packs), in what order, and does the draft of P32 stand as the step before any of them? The design records each as planned, later. The draft is [Workbench_Views_Design.md](Workbench_Views_Design.md#rulings-the-draft-asks-of-the-owner), and recommends the owner's order of the views, with the draft as the step before any kind.
- **Q2.** Is a metadata source a .metadata.src folder, as built, or a single flat canonical XML file as the design said?
- **Q3.** Do a model's textures stay inside the drawable the source cooks from, as built, or become DDS files beside the glb?
- **Q4.** Does a non-Windows platform ever get a build (a PS4 .otd, a console extension), or is win64 the only platform?
- **Q5.** Does Cancel stop the build being written and keep the last good build, as built, or let the build being written finish?
- **Q6.** May a singleplayer DLC and an .oiv leave out an unchanged source taken from the game, as a FiveM resource does, and ship it only when the build says so?
- **Q7.** Should OpenRage write a pre-push hook that runs check-sharing, or is the verb for the person's own hook enough?
- **Q8.** R13 deferred interface languages until after P31. Start them now, and which languages?
- **Q9.** Does a real multi-window shell replace a second process per project later?
- **Q10.** May the plugin ABI gain a host function that writes several files as one transaction, and may a validator plugin read the pack and not only the build plan?
- **Q11.** Hide ears, a hat cut, a time cycle modifier per texture, named audio presets and a shop entry's lock hash, variant components and outfits: is there a sample file or a statement of the flag that states one?
- **Q12.** Which server frameworks' configs does the owner want first (illenium-appearance, qb, ESX, rcore)?
- **Q13.** Are the Excluded rows of the matrix (R10, R16, R18) the owner's rulings, so that WAR-20, WAR-24, PRO-148, PRO-182 and PRO-184 can close?
- **Q14.** The preview of alternate sets, a floor for heels, a first-person view and the shadows and time of day of Shaded are built (P38). Build a mesh-physics preview, which needs the game's cloth simulation driven here, and a 3D comparison of two versions the person chooses?
- **Q15.** As of 2026-10-20 a build refuses one content that is over the 65,535 entries of an archive. May it cut inside the content, though that renumbers drawables?
- **Q16.** Read on 2026-10-20, the reference pack states at most 103 switches for one asset. Is 159 switches (160 alternates with the drawable itself) the limit to keep?
- **Q17.** Should the Mods overlay take a build installed into it and write the load order the game reads, or is the copy of the guide's Install on a server the install?
- **Q18.** Build or leave the smaller gaps: a flag-off cloth linked on its own, the no-diff rewrite of a source, a note on OpenIV's open formats? (Reserved texture places are built, P38.)
- **Q19.** May Complete materials add the samplers a material lacks? An entry added is empty, and the game falls back to the shader's default only for a slot that is removed, so each added sampler would turn off the default texture the shader uses; the shader database names no default texture to put in it. Or should the fix leave them, as it does, and the Model workspace's editor add one the person chooses?
