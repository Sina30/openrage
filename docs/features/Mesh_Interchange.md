# Mesh interchange

How `or-interchange` takes a drawable of either target out to glTF and puts edited geometry back into an existing drawable: what the mapping commits to, what it refuses, and what it does not do.

The crate has six modules. `scene` is the model (meshes, materials, a skeleton) with no RAGE binary concern and no glTF concern; `extract` fills it from a `rmcDrawable`; `gltf` writes it and `gltf::read` reads a glTF file back into it; `fbx` reads an FBX into it; `import` puts a `scene` into a drawable (`replace_geometry`, `replace_in_file`), and `doctor` changes one (levels of detail, a mirror, the rig, the weights and the texture a mesh uses; [the mesh doctor](#the-mesh-doctor)). A second interchange format is a second writer and reader over `scene`, not a second walk of the drawable. The work is [R38](../direction/Roadmap.md#r38-mesh-interchange).

```bash
openrage-cli mesh export prop.ydr prop.glb             # one self-contained file
openrage-cli mesh export prop.ydr prop.gltf            # JSON beside a .bin and one .png per texture
openrage-cli mesh export ped.ydd ped.glb --lods all    # every LOD, not only the highest
openrage-cli mesh export ped.ydd part.gltf --entry 3   # one drawable of a .ydd or .yft, to edit
openrage-cli mesh channels ped.ydd                     # the channels an import must supply
openrage-cli mesh import ped.ydd part.gltf out.ydd --entry 3   # or part.glb, or part.fbx
openrage-cli mesh census --dir path/to/corpus          # or --game DIR; --limit 0 walks every file
```

## Where a write is refused

`or_io::gta5::drawable::MeshDefect` holds ten defects in which a geometry's stated counts disagree with the runs they describe. Every drawable-bearing format's `validate` (`.ydr`, `.ydd`, `.yft` and `.ypt`, both targets) runs them over the image the canonical save built, so every owned save of an incoherent drawable is refused, whatever built it (`every_mesh_defect_is_refused_at_save`, `a_save_refuses_an_index_past_the_vertex_run`).

`replace_in_file` opens the file owned (`GtaDrawable<T>`, `PgDictionary<GtaDrawable<T>>` or `GtaFragType<T>`, for the target `T` the CLI reads off the file's RSC7 version), edits one drawable's `Vec` fields on a clone and calls `save()`, which runs the format's own `validate` over the image it built; there is no separate read-back and no pack parameter (P48, #310). Nothing is mutated in place, so a refusal leaves the input file as it was.

| Refused | Because |
|---|---|
| `vertices_count` past the buffer | the engine walks the count, not the bytes |
| `vertex_stride` not the buffer's stride | every vertex after the first lands off alignment |
| `indices_count` past the buffer | the same, one run over |
| `indices_count` not `primitive_count * 3` | the run is described twice and must agree |
| an index past the vertex run | it reads a vertex that is not there |
| `matrix_count` not the palette's length | the palette is addressed by the count |
| a palette slot naming no bone | it skins to a bone the skeleton does not have |
| `aabbs` the wrong length | above one geometry it is `n + 1` long (`or_io::codec::mesh::aabb_slots`) |
| `shader_indices` not one per geometry | they share a subscript with `geometries` |
| a shader index past the group | it draws with a material that is not there |

Each was counted at zero over shipped files before it became a refusal; `openrage-cli mesh census` reports all ten, for Gen8 and Gen9 drawables. The same `validate` refuses a vertex or index buffer more than one geometry of the file points at (`or_io::gta5::drawable::Buffers`), which the game's fixup walk would resolve twice; `openrage-cli convert repair` removes a level that draws only through buffers a more detailed level owns (`a_buffer_two_levels_point_at_is_refused_and_a_level_of_aliases_is_removed`). `openrage-cli convert repair --drop-copies` also removes a level that draws 90 percent or more of the high level's vertices (`levels::copies`, `a_level_that_copies_the_high_one_is_found_and_removed`): such a level is the high one again, and three of them made a hair of 225,104 triangles a heap of more than 64 MiB, which the writer lays out as one page of 128 MiB (`DatResourceSizes::single_chunk`) and the game refuses over 100 MB (`ERR_STR_FAILURE_3: page 0 in paged resource ... is over 100 MB`, a client crash). `convert verify` and `inspect fixups` (a file, `--batch`, `--game`) report a heap whose one page is over the limit, as an oversized page (`GAME_PAGE_LIMIT`, `DatResourceSizes::largest_page`).

⚠ The stride, the index and primitive counts and the palette's length are derived at save, so an owned value cannot state them wrong; their refusals catch a tree that does.

## Export

Export is a read path: it writes no resource.

### What the corpus settled

Measured by `openrage-cli mesh census --dir` over 124 staged files, 416 drawables, 1,590 geometries and 706,778 triangles (at `f2df0e6f`, #111):

- Winding needs no flip. The right-hand-rule facing agrees with the stored normals on 701,473 of 706,778 triangles, and glTF uses the same rule for front faces; the rest are individual triangles whose authored normals oppose their winding, which is ordinary in real content. Reversing indices would turn every model inside out.
- `is_triangle_list` tests the file's two counts, `indices_count == primitive_count * 3`, rather than the type byte: a geometry states its index run twice and its type once. All 1,590 geometries state type 3 and an index run exactly three times their primitive count. A triangle list states type 3, in RAGE's zero-based numbering. Why only the bytes can catch a wrong variant number is in [OR_IO_Specification.md](../direction/OR_IO_Specification.md#tagged-bases-and-enums).
- 16-bit indices constrain nothing on the way out: the largest vertex run is 5,954. They constrain the import.

All 416 drawables also go out to glTF and back in with no refusal, their 813,007 vertices returning in place with the same indices and the same heaviest bone (`crates/domain/or-interchange/tests/every_corpus_drawable_comes_back.rs`, at `26171b55`), and every exported document is held to the glTF rules by `crates/domain/or-interchange/tests/every_corpus_drawable_exports.rs`.

### The axis change is one root node

RAGE is Z-up and glTF Y-up. The writer carries the quarter turn about X as one rotation on the scene's root node, `[-1/√2, 0, 0, 1/√2]` as xyzw, and leaves every position, normal, tangent and bind matrix in the drawable's own frame; an import has nothing to undo. `the_root_basis_takes_z_up_to_y_up_without_mirroring` holds that RAGE +Z reaches glTF +Y, RAGE +Y reaches glTF −Z, and the determinant is +1: a determinant of −1 mirrors the mesh and inverts every triangle's facing at once.

⚠ Rotating the data instead is wrong below the root. `local' = R·L` ignores that the parent's frame turned too, which gives a skeleton that is right in its bind pose and shears as soon as it moves. A skinned mesh node sits at the scene root rather than under the basis, because glTF requires a reader to ignore a skinned node's transform; its vertices are placed by the joints, which are under the basis.

### Materials

| RAGE sampler | glTF |
|---|---|
| `DiffuseSampler` and its aliases | `pbrMetallicRoughness.baseColorTexture` |
| `BumpSampler`, `NormalSampler` | `normalTexture` |
| everything else | `extras.rage.textures`, by parameter name |

A specular map is not a metallic-roughness map: RAGE packs intensity, falloff and fresnel into three channels, and glTF's green channel is roughness. `SpecSampler` therefore stays in `extras`, and the Khronos validator reports those textures as `UNUSED_OBJECT`, which is accurate. Both PBR factors are stated, metallic 0.0 and roughness 1.0, because glTF's defaults of 1.0 render as dark, rough metal (`a_material_states_the_pbr_factors_rather_than_taking_gltfs_defaults`). `alphaMode` comes from the draw bucket, a mapping choice rather than a fact of the file: `ALPHA`, `DECAL` and `DISPLACEMENT_ALPHA` become `BLEND`, `CUTOUT` becomes `MASK` at 0.5, and the rest `OPAQUE`. Samplers are written as repeat and linear, not from the shader's sampler state.

### What goes in extras

What glTF has no slot for is kept under `extras.rage`, so a diff of two exports shows it and an import can find it.

| Where | Carries |
|---|---|
| document | the source name, `upAxis: "Z"`, and the drop report |
| material | the shader preset, the draw bucket, every texture binding by name, every vector parameter |
| primitive | its LOD, its model and geometry index, and which `GrcFvfChannel` each `TEXCOORD_n` and `COLOR_n` came from |
| bone node | its bone tag |

The UV channel list is load-bearing: glTF's `TEXCOORD_n` must start at 0 and be contiguous while a drawable may skip `Tex0`, so without `extras.rage.uvChannels` an import cannot put a UV set back on its channel.

### What the format forces

- Normals and tangents are scaled to unit length, since a `PackedNormal` channel quantises each axis to ten bits.
- A tangent's fourth component becomes exactly ±1; RAGE stores the bitangent sign there, and a `PackedNormal` tangent with none decodes to 1.0.
- Skin weights are renormalised to sum to one, and a lane with no weight names joint 0.
- An inverse bind matrix keeps an exact `(0, 0, 0, 1)` bottom row. A 4×4 inversion lands on 1.0000001 there, which the validator rejects as `ACCESSOR_INVALID_IBM` (`every_inverse_bind_matrix_is_still_affine`).
- A normal or tangent with no direction is replaced and counted in the report.

### Skinning without a skeleton

A ped-component `.ydd` states `matrix_palette` indices into a skeleton that lives in another file: 28 of the census's 416 drawables (at `f2df0e6f`). glTF cannot say that, and `JOINTS_0` without a `skin` is data readers discard, so the writer withholds `JOINTS_0` and `WEIGHTS_0` and the report counts them (`skinning_without_a_skeleton_is_withheld_and_reported`). The interchange model keeps the influences either way. `SceneBuilder::with_reference_skeleton` is the other answer: given the ped's skeleton, which is `bones_of` over the `.yft` it belongs to, it reads such a drawable against it, and the writer then states the skin, with each joint the bone the palette named. A palette that outgrows the skeleton is a different ped's, and a source refuses it (`a_reference_skeleton_the_palette_outgrows_is_refused_naming_the_bone`).

## Import

An import replaces geometry in a drawable that exists; it does not author one. The channel set, the materials, the skeleton and the drawable box all come from the file being edited.

- The geometry's channel set is kept exactly, because a material's vertex declaration names the channels it reads. `mesh channels` prints it.
- Primitives are paired per LOD against only the LODs the mesh brings; a LOD it does not mention is left as it was. Names come first: a mesh answers to the model its name ends in (`…_lod1_model0`, as an export names it) and a primitive to the geometry that draws with a shader of its material's name, so a file whose nodes or primitives a tool reordered pairs right (`meshes_a_file_reorders_are_paired_to_their_models_by_name`, `primitives_are_paired_to_geometries_by_the_name_of_their_material`); a source states the names its people use in `drawable.json` ([Source_Format_Design.md](../direction/Source_Format_Design.md#what-wave-p12-built)). What no name settles is paired positionally, in the exporter's walk order. A primitive's `stated_address`, which our exports write into `extras.rage`, is checked against where it landed whenever the pairing was positional and a disagreement is refused (`a_reordered_mesh_is_refused_rather_than_paired_positionally`, `two_primitives_of_one_material_keep_the_order_their_export_stated`); `None` means the file never said and is paired positionally, where `Some((0, 0))` claims geometry 0.
- A mesh past what a geometry holds is cut and not refused: more than 65,535 vertices, since `vertices_count` is a `u16` beside a 16-bit index payload, or more than 256 bones, since a `Binding` lane addresses the palette with a byte. It is cut by triangle into geometries that keep the shader and the vertex declaration of the one it replaces, and `ImportReport::splits` says where (`a_mesh_past_the_sixteen_bit_ceiling_is_split_into_geometries_and_reported`, `a_mesh_weighted_past_the_palette_ceiling_is_split_by_bones_and_reported`). This is not adding a geometry: the slot and its shader are the base's, and a mesh with more or fewer primitives than the model has geometries is refused as before.
- The five counts are rebuilt from the mesh, the bone palette from the bones the influences use, and the geometry and model boxes through `or_io::codec::mesh`. The drawable box is left as it was, because it is authored rather than derived ([R41b](../direction/Roadmap.md#r41b-bounding-volumes-and-bvh); `an_import_reboxes_the_model_and_leaves_the_drawable_box_alone`).
- Weights are quantised so the four lanes sum to exactly 255. The engine blends by the stored bytes, so lanes rounded separately to 254 scale the vertex down (`quantised_weights_sum_to_the_whole`).
- `--keep-skinning` carries a ped component's own influences across, since glTF could not; it refuses if the vertex count moved, because then vertex *i* is not the old vertex *i* (`kept_skinning_refuses_when_the_vertex_count_moved`).

It refuses, naming what it found: a geometry count different from the model's, since adding a geometry means choosing its material and resizing the box array; more than 65,535 vertices in a mesh with no triangle to cut it by (`a_mesh_past_the_sixteen_bit_ceiling_with_no_triangle_is_still_refused`); a channel the geometry declares and the mesh omits, unless `--fill-missing` accepts a stated default and the report names each filled channel; a bone the rebuilt palette does not hold; an embedded `data:` buffer; an accessor with no `bufferView`, which reads as zeroes; a primitive with no indices; a mode other than a triangle list; and a component type glTF does not define. `crates/domain/or-interchange/tests/a_mesh_comes_back.rs` reaches the geometry count, the vertex ceiling, the omitted channel and the two buffer refusals; the palette, index, mode and component-type refusals have no test.

### A skin's mesh space comes from the skin

⚠ glTF requires a reader to ignore a skinned node's own transform, and the vertices are not in world space either: at the bind pose a skinned vertex lands at `joint_world · IBM · v`, and that product is one constant matrix, the map from the mesh's space to the world. For this exporter it is the basis, because the vertices stay in RAGE space and the turn is on the joints; for a Blender file it is the identity. `gltf::read` derives it, and refuses a skin whose joints disagree about it; assuming either value turns the other's files a quarter turn about X.

### Weight and Binding are lane payloads

⚠ Whatever width a `Weight` or `Binding` channel is declared at, its lanes are payloads, not normalised values. A `Binding` declared `Color` and put through the normalised encoder clamps a bone slot of 5 to 1 and writes 255, and a `Weight` declared `UByte4` writes 1 for a full weight (`a_binding_declared_as_a_colour_keeps_its_bone_slots`, from `des_bridge_root.ydr`, whose two skin channels are both `Color`).

### What a file held that a drawable has no place for

An import takes a file's geometry and keeps the drawable's own materials, textures, skeleton and
boxes, so a file's own are left behind. `Scene::dropped` counts them where each reader meets them,
and `openrage-cli mesh import` prints each as `not carried:`. The glTF reader counts materials,
textures, animations, cameras, lights, skins after the first, primitives with morph targets or with
more than four influences, and any attribute it makes no channel of
(`a_file_from_a_modelling_tool_says_everything_an_edit_has_no_place_for`); the FBX reader's are in
the next section.

## FBX in

`fbx::load` reads a binary FBX into the same `scene`, so `replace_geometry` takes it as it takes a
glTF. The parser is `fbxcel`, which reads 7.4 and later; a 7.1 to 7.3 file lays its nodes out the
same way and is read as 7.4, which holds on the one 7.3 file this was tried on. An ASCII file, a
version before 7.1 and a document that places no mesh are refused by name, a layer mapped by edge
is left out and named in the report, and a corrupt file is an error and not a panic
(`a_file_that_is_not_a_binary_fbx_of_a_version_it_reads_is_refused_by_name`).

- **Space.** The file's own axes and unit become the drawable's, Z up and metres, in one matrix: the
  coordinate axis goes to X, up to Z and the front axis, which points at the viewer, to -Y, so a
  Blender file's axes are the identity and a Maya file's turn a quarter about X. The unit is the
  file's `UnitScaleFactor`, centimetres per unit, scaled to the metre. A left-handed file is mirrored
  by the same matrix, and the winding is turned with it
  (`a_left_handed_file_is_mirrored_and_its_winding_is_turned_with_it`). V is
  turned over, because FBX measures it from the bottom of the image and a drawable from the top.
- **Placement.** A model's transform is the one the FBX SDK states, pivots and offsets included, and
  its parents'. A skinned mesh sits where its bones were bound (the cluster's `Transform`), and a
  bone's rest pose is its `TransformLink` where the file states one.
- **Vertices.** An FBX states normals, coordinates and colours per corner of a polygon or per
  control point, so corners that state the same values are one vertex, and a polygon of more than
  three corners is cut into a fan, which is wrong for a concave one and is counted. Polygons split by
  material become one primitive each, in material order. A model's name says its level of detail
  where it has `lod` and a digit in it.
- **Skin.** Bones are the models a cluster names, in the order the vertices first use them; a
  vertex keeps its four heaviest influences, renormalised, and one with none is bound whole to the
  first joint. Both are counted.
- ⚠ Counted and not carried: materials, textures, animation stacks, cameras, lights, blend shapes, and
  the sets of coordinates after the eighth and of colours after the second. A mesh pairs with a
  drawable's geometries in file order within a level of detail, so a file that lists them in another
  order than the drawable has to be reordered in the tool.
- ⚠ The parser decodes every array it meets, so a hostile file costs memory in proportion to what its
  compression claims. It is for files a person chose.

## What Gen9 adds

`extract` and `import` take `RmcDrawable<T>` for either target, and each target is read and written as itself: `Extracted` reads a target's materials and textures, and `Imported` writes its vertex and index buffers. `openrage-cli mesh export`, `mesh import` and `mesh channels` pick the target from the file's RSC7 version.

⚠ Routing a Gen9 drawable through Gen8 would put Gen8 vertex declarations on a Gen9 file's data and describe the result as that file.

A Gen9 vertex declaration is 52 element slots, each with an offset, a step and a DXGI-style format, where Gen8 has a 16-channel FVF. `or_io::codec::mesh::VertexLayout::of_g9` reads the elements that name a Gen8 channel into the same `GrcFvfChannel` and `GrcDataSize` the Gen8 path uses, so the glTF mapping above is unchanged; these rows are the whole of what Gen9 adds.

| Gen9 element slot | Channel |
|---|---|
| 0 | `Position` |
| 4 | `Normal` |
| 8 | `Tangent0` |
| 16 | `Weight` |
| 20 | `Binding` |
| 24, 25 | `Diffuse`, `Specular` |
| 28 to 33 | `Tex0` to `Tex5` |

| Gen9 format | Encoded as |
|---|---|
| `R32G32B32A32Float` | `Float4` |
| `R32G32B32Float` | `Float3` |
| `R32G32Float` | `Float2` |
| `R16G16B16A16Float` | `Half4` |
| `R16G16Float` | `Half2` |
| `R8G8B8A8Unorm` | `Color` |
| `R8G8B8A8Uint` | `UByte4` |
| `R10G10B10A2` | `PackedNormal` |

The staged Enhanced drawables carry only the first three, `R8G8B8A8Unorm` and `R8G8B8A8Uint`: a skinned one declares `Weight` as `R8G8B8A8Unorm` and `Binding` as `R8G8B8A8Uint`, and both go through the lane path above. Every one is interleaved, each element stepping by the buffer's stride.

An import into a Gen9 geometry also refuses an element that names no Gen8 channel or has a format with no row above, because rebuilding the buffer would zero it, and an element stored as its own run rather than inside the interleaved vertex, because that run's offset depends on the vertex count. No staged drawable has either. A Gen9 vertex buffer keeps no second copy of its vertices, so there is no `lock_ptr` to rejoin.

All 14 staged Enhanced drawables (5 `.ydr`, 2 `.ydd` entries and 7 `.yft` drawables; 41 geometries and 15,868 vertices) go out to glTF and back in with no refusal, and their files save and reopen with the same meshes (`every_enhanced_ydr_survives_the_round_trip` and its `.ydd` and `.yft` siblings); `an_enhanced_edit_lands_in_the_saved_file` moves every vertex and finds the move in the saved file. The exported documents are held to the glTF rules by `every_enhanced_ydr_exports_a_document_gltf_accepts` and its siblings. The tree does not run the Khronos validator itself.

## Memory

An export holds one resource: one drawable set's decoded geometry, one texture's RGBA while it is encoded, and the output buffer; 4.5 MB resident on the largest corpus drawable with `--lods all` (at `f2df0e6f`). An import holds two, the drawable as read and the clone being built, so a refusal can leave the first untouched. The census keeps counters and no per-file result, 17.1 MB resident over the 124 files (at `f2df0e6f`), and its `--limit` has a bounded default so that walking a whole install is asked for. There is no batch mode for export or import.

## The mesh doctor

What a person does to the mesh of a drawable that the game states no number for lives in `or_interchange::doctor`, and the Wardrobe runs it over a garment ([Clothing_Tool_Design.md](../direction/Clothing_Tool_Design.md#what-the-engine-builds)). Each operation works on a scene, or on a drawable through one (`through_scene`: read every level, change it, put it back through the import above), keeps the shader and the vertex declaration of the geometry it works from, and makes no material, no layout and no bone.

- **A level of detail the drawable lacks.** `levels::add_level` makes the Medium, Low or Very low level from the High one: it copies the High level's models, geometries, shader indices and vertex declarations into the empty level and replaces the vertices and triangles of each geometry with the decimator's, through the import. The High level is not touched, and the new level has the same models and geometries, drawing with the same shaders (`the_ped_components_of_the_corpus_get_the_levels_they_lack_and_keep_their_skin`, over every staged `.ydd` of both targets). It is the one geometry the interchange adds, and it is no authoring: nothing is chosen. A level that is there is made again only when asked; a drawable that switches its levels at distances of its own is refused, since where the new level takes over is then a choice (every ped component states 9998 for all four, and the game picks its level by the ped).
- **The decimator** (`decimate`) collapses edges, and every vertex it keeps is a vertex of the source with all its channels untouched (normal, tangent, texture coordinates, colours, bones and weights), so the channel set a vertex declaration names is the source's and a skin is never blended. Vertices are welded by position only to find the surface. A collapse is allowed when every vertex the removed position has is given a vertex of the kept one by a triangle they share, so a texture seam or a hard edge never opens (`a_texture_seam_is_never_torn`), and a border only slides along itself; one that would flip a triangle, make the surface non-manifold or leave a position more than sixteen neighbours is refused. It is priced by the quadric error of the faces plus how far the channels of the vertex that goes are from what the triangle that replaces it states at the same place, so a surface whose texture runs linearly loses nothing however long its edges are. **Tolerance:** a geometry of eight triangles or fewer is copied, none is cut below eight, and each ends at its target (its triangles times the ratio, rounded up) or one under it, or the report says it could not and why. Every body geometry of the staged garments of both targets (jackets, trousers, tops and heads) reaches 35 and 15 percent, and together they come out to the ratio asked to within a hundredth (`every_body_geometry_of_both_targets_reaches_35_and_15_percent_of_its_triangles`), and at 8 percent some stop short and say so; a strip of hair cards has no collapse that is safe, since every position of a one-quad strip is on its border, and where the decimator takes out less than a quarter of what was asked no level is made and the report says why (`a_strip_of_hair_cards_that_has_no_safe_collapse_makes_no_level_and_says_why`).
- **A mirror** (`mirror::mirror_drawable`) negates x of every position, normal and tangent, turns a tangent's sign, swaps the last two corners of every triangle and gives the skin to the bone on the other side, found by the `L` or `R` between underscores of its name; the freemode skeleton pairs every left bone with a right one and leaves none over (`the_freemode_skeleton_pairs_its_left_bones_with_its_right_ones`), and a weight on a bone with a side and no partner is refused by name. The drawable's boxes and bounding sphere are mirrored with it. Mirroring twice is the identity: exactly on a scene (`mirroring_twice_is_the_identity`) and, over a drawable of both targets, positions, triangles and skin exactly and the unit normals and tangents to within the last bit that their scaling to unit length may move (`the_ped_components_of_the_corpus_mirror_twice_to_themselves_and_their_renamed_groups_are_mended`).
- **The rig and the weights** (`rig`). `check` lists a group no skeleton knows (a joint with no name, or the one a tool gives what it made: `bone3`, `Group.001`), a bone spelt otherwise than the skeleton spells it, a vertex with no weight, weights that do not add up and weights under one percent; `repair` renames the misspelt, gives the weight on an unnamed or unknown group to the nearest bone above it that the skeleton has, drops it where there is none, and cleans the weights (`a_group_with_no_name_of_a_bone_is_found_and_its_weight_goes_to_the_bone_above_it`). `clean_weights` drops the faint weights, makes the rest add up and gives a vertex with none the skin of the nearest vertex that has, and a second cleaning finds nothing to do. `heel_bones` looks at the lowest fifth of each foot of a pair of shoes, at the back, and says how many of those vertices the foot bone holds most: a heel the toe bone holds bends with the toe.
- **The texture a mesh uses** (`uv`). `used` is the part of the texture the first texture coordinates fill (the ones the diffuse, normal and specular samplers all read), and `Window::holding` the smallest aligned power-of-two part of the texture that holds it, which is a copy of whole blocks at every level of the mips ([Source_Format_Design.md](../direction/Source_Format_Design.md#cropping-a-textures-source)); `Window::apply` moves the coordinates into it.

## What this does not do

- Author a drawable from scratch, or add or remove a geometry of a level that is there: both mean choosing a shader, a vertex declaration or a box array, which is done in the source tool. The one geometry it adds is a whole level of detail made from the High one, which chooses none of them (above).
- Write an FBX: there is no permissive Rust FBX writer, and CodeWalker's is GPL ([Licence_And_Provenance_Policy.md](../guides/Licence_And_Provenance_Policy.md)). An FBX is read, above, and never written.
- A text format of its own: `.gltf` is already an open, documented, text-based one, and a bespoke format would have to say something glTF cannot.
- Animation: a `.ycd` clip has no route into a glTF `animation`.
- Rigid bone attachment: nothing places a model at the bone its `GrmModel::matrix_index` names, so the exporter does not either.
- Sampler state, as above.
