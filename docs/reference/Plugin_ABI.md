# Plugin ABI

The contract between an in-app plugin, a WebAssembly guest, and the OpenRage
host: calls, memory, and how names, values and refusals cross.

**ABI version 1.** The host half is `crates/domain/or-plugin`, held to this
document by `crates/domain/or-plugin/tests/how_anything_crosses_the_boundary.rs`.
The app embeds the host and runs the export `go`, after consent, with the
editor's edit mode as the write gate and the format's read-back and canonical
save as the `SaveCheck`, and shows every refusal
([R44e](../direction/Roadmap.md#r44e-consent-and-refusal-in-the-app)); a file's bytes, read
and replaced, are [R44f](../direction/Roadmap.md#r44f-payload-and-gpu-access) and a texture
or a buffer apart from its file and GPU access are not built. A plugin that
adds to the editor stays loaded and registers panels, commands and settings and
draws over the viewport
([R45](../direction/Roadmap.md#r45-the-plugin-sdk-and-registration-surface), [A guest that
stays loaded](#a-guest-that-stays-loaded)). A plugin can write several files of the
project together, all or none ([Writing several files
together](#writing-several-files-together)). A plugin that extends a build is not run on a
document but over each plan the app makes: a validator reports findings, reads the clothing
pack as data and suggests a fix a person accepts, and an emitter
adds files, data in and data out ([A plugin that extends a
build](#a-plugin-that-extends-a-build)). The guest library, how to build a
guest, how to install one, and the registries a plugin extends are in
[plugins/README.md](../../plugins/README.md).

## Versions: an older plugin still loads

The host implements every version from `OLDEST_ABI` (0) to `ABI_VERSION` (1), and a manifest names the
one it was built for in `abi`. The rules are four, and `a_plugin_of_an_older_abi_still_loads` holds them:

- **A plugin built for an older version loads and runs as it was built.** Everything version 0
  defined keeps its name, its signature and its capability key, and a grant of version 0's capabilities
  buys none of version 1's functions. Settings → Plugins says of such a plugin that it is built for an
  older interface, that it still loads, and that rebuilding it for the newest gives it what is newer.
- **A plugin built for a newer version than the host knows is refused with a sentence** that says to
  update OpenRage, never run in part. The host of version 0 refused every other version the same way, so
  a plugin built for 1 is refused there too (`Manifest::accept_up_to`).
- **A plugin may not want what its version does not have.** A manifest that declares version 0 and
  wants `cap.write_files`, `cap.read_pack` or `cap.suggest_fixes` is refused with a sentence that says to
  rebuild it: its code was not built against the functions those buy.
- **A new status, tag or signature is a new version** with its own number, and what an older version
  had is not changed.

| Version | Adds |
|---|---|
| 0 | the described readers and `patch_field`, the file's bytes, panels, commands, settings and overlays, the plan of a build, findings and added files |
| 1 | `write_files`, `pack_size` and `read_pack`, `report_with_fix`: [files written together](#writing-several-files-together), [the pack read as data](#a-plugin-that-extends-a-build) and [a finding with a fix](#a-plugin-that-extends-a-build) |

A manifest names an id and the capabilities it
wants, and is refused without either; the wants are shown for consent
(`Manifest::requested`) and are not a grant.

## Calls

Every host function is in the module namespace `openrage`, takes `i32`
arguments and returns an `i32` status, never an answer; answers go into the
guest's memory at addresses the guest supplies.

- The host runs one export the embedder names, taking and returning nothing
  (`go` below); a trap or running out of fuel ends the run, not the host.
- The guest exports its memory as `memory`. The host never calls a guest
  allocator or frees guest memory, which would run guest code inside a host
  call. No `memory`, or a pointer or length outside it, is `abi.bad_argument`.
- A string goes in as `(ptr, len)`: UTF-8, not NUL-terminated, or else
  `abi.bad_argument`.
- Bytes come out through `(out_ptr, out_cap, out_len_ptr)`. The length, a
  little-endian `u32`, is written at `out_len_ptr` whether or not the answer
  fits; one longer than `out_cap` is `abi.buffer_too_small` and not a byte of
  the buffer is written, so a guest that ignores the status reads its own old
  bytes rather than half an answer. Grow to the reported length and call
  again. An answer over 1 MiB, which bounds what the host builds, is
  `abi.too_large`.
- There is no `last_error`: the guest gets the status, and the host keeps the
  sentence for the user, a `Failure` for each call that did not answer `abi.ok`.
  Those include `abi.buffer_too_small` and `abi.null_pointer`, which are not
  faults: a host showing failures to a user drops them.

## Statuses

| Code | Key | Means |
|---|---|---|
| 0 | `abi.ok` | The call succeeded. |
| 1 | `abi.buffer_too_small` | The needed length is in the length slot and the buffer is untouched: retry. |
| 2 | `abi.not_granted` | The plugin was not granted the capability this call costs. |
| 3 | `abi.not_writable` | Granted, but the embedder's write gate refuses or the document was opened read-only. |
| 4 | `abi.no_such_type` | The document's description has no type by that name. |
| 5 | `abi.no_such_field` | The type has no field by that name, or none at that index. |
| 6 | `abi.refused` | Understood and refused: a field that is not a value, a structural field, a patch or a file the format refuses. |
| 7 | `abi.bad_argument` | A pointer, a length or the bytes at one were invalid: a bug in the guest. |
| 8 | `abi.too_large` | The answer exceeds what one call carries. Not retryable. |
| 9 | `abi.truncated` | The description places the field past the end of the document: the file and its description disagree. |
| 10 | `abi.null_pointer` | The pointer is null in this document, which is not a fault. |

## Value encoding

A value crosses as a tag byte and a body, little-endian and unpadded, which a
guest decodes without the description.

```text
value  := tag:u8 body
str    := len:u32 bytes:u8[len]        ; UTF-8

0x01 Int             i128
0x02 F32             f32
0x03 F64             f64
0x04 Bool            u8                ; 0 or 1; anything else is refused
0x05 Enum            str enum_name, i64 value
0x06 Struct          str type_name, u32 n, n × (str field_name, value)
0x07 Array           u32 n, u8 has_capacity, [u64 capacity], n × value
0x08 Bytes           u32 len, u8[len]
0x09 Ptr             u64 source, u8 has_target, [value]
0x0A Alias           u64 address
0x0B Opaque          u32 len, u8[len]
0x0C UndescribedPtr  u64 address
0x0D NotAnAddress    u64 raw
0x0E Blob            str codec, u32 n, n × (str part_name, value)
0x0F Member          str field, u8 has_index, [u64 index]
```

`Int` is `i128` because `u64` fields above `i64::MAX` occur; a guest that wants
64 bits reads the low eight bytes and checks the high eight are a sign
extension. `Bytes`, a payload
the description named, and `Opaque`, a run nothing describes, keep separate
tags. An optional part carries a flag, never a sentinel. `Alias`,
`UndescribedPtr`, `NotAnAddress`, `Blob` and `Member` cross as the reader found
them, never as null. A `Struct` lists each of its type's fields once, in
declaration order. `field_value` answers tags `0x01` to `0x05` and `0x07`, and
`patch_field` takes a scalar or an `Enum`.

⚠ Never allocate from a declared count: four billion elements in nine bytes is
one call and an out-of-memory for the side that trusted it. The host reserves
against the bytes left, refuses nesting deeper than 32 (`MAX_DEPTH`), since its
stack overflow would happen outside WebAssembly's isolation, and refuses bytes
left over after a complete value.

## Host functions

Types and fields go by the description's names, the engine's
(`GrcVertexBuffer`, `vertices_count`).

| Function | Signature and answer | Costs |
|---|---|---|
| `root_type` | `(out_ptr, out_cap, out_len_ptr)`: UTF-8 | read the open document |
| `field_count` | `(type_ptr, type_len, out_ptr)`: a `u32` | read the open document |
| `field_name` | `(type_ptr, type_len, index, out_ptr, out_cap, out_len_ptr)`: UTF-8 | read the open document |
| `type_size` | `(type_ptr, type_len, out_ptr)`: a `u32`, a run's stride | read the open document |
| `field_value` | `(type_ptr, type_len, at, name_ptr, name_len, out_ptr, out_cap, out_len_ptr)`: an encoded value | read the open document |
| `follow` | `(type_ptr, type_len, at, name_ptr, name_len, out_ptr)`: the offset a pointer lands at | read the open document |
| `element_count` | `(type_ptr, type_len, at, name_ptr, name_len, out_ptr)`: a `u32` | read the open document |
| `patch_field` | `(type_ptr, type_len, at, name_ptr, name_len, value_ptr, value_len)` | change the open document |
| `file_size` | `(out_ptr)`: a `u32`, the length of the file's bytes | read the file's bytes |
| `read_file` | `(offset, out_ptr, len)`: `len` bytes of the file from `offset`, copied to `out_ptr` | read the file's bytes |
| `write_file` | `(ptr, len, why_ptr, why_cap, why_len_ptr)` | replace the file's bytes |
| `write_files` | `(set_ptr, set_len, why_ptr, why_cap, why_len_ptr)`: files of the project, kept all together or not at all (ABI 1) | write several files together |

`at` is the offset of the type's instance; the root is at 0. Only the functions
the grants pay for are linked, so importing an ungranted one fails
instantiation with an error naming the capability. The default grant is *read
the open document*. A grant is a capability, named in a manifest by its key:

| Capability | Key | Buys |
|---|---|---|
| read the open document | `cap.read_open_document` | the described readers |
| change the open document | `cap.edit_open_document` | `patch_field` |
| read the file's bytes | `cap.read_file_bytes` | `file_size`, `read_file` |
| replace the file's bytes | `cap.write_file_bytes` | `write_file` |
| add panels | `cap.add_panels` | `register_panel`, `draw_widget` |
| add commands | `cap.add_commands` | `register_command` |
| add settings | `cap.add_settings` | `declare_setting`, `setting_value` |
| draw overlays | `cap.draw_overlays` | `set_overlay`, `overlay_picking` |
| read the plan of a build | `cap.read_plan` | `plan_size`, `read_plan` |
| report findings about a build | `cap.report_findings` | `report` |
| add files to a build | `cap.add_files` | `add_file` |
| write several files together (ABI 1) | `cap.write_files` | `write_files` |
| read the pack of a build (ABI 1) | `cap.read_pack` | `pack_size`, `read_pack` |
| suggest a fix with a finding (ABI 1) | `cap.suggest_fixes` | `report_with_fix` |

The four after the file's bytes are the functions of [a guest that stays
loaded](#a-guest-that-stays-loaded), which also reaches the file's bytes with
the two capabilities above them and nothing else of the document. The next
three are [a plugin that extends a build](#a-plugin-that-extends-a-build)'s, and the
three of version 1 are [files written together](#writing-several-files-together)'s and
the pack's and the fix's of the same plugin.

### Walking a document

No call reads a whole document: the guest takes one hop at a time, so no call
costs the host more than one field. `field_value` answers a scalar, an enum or
a fixed run of scalars, and refuses anything indirect or bit-packed. `follow`
answers where a pointer or an `atArray` lands, or `abi.null_pointer`.
`element_count` answers a fixed array's length, an `atArray`'s count or the
count its sibling fields state. A run of structs is `follow`, `element_count`,
`type_size` of the element as the stride, and `field_value` at
`start + index * stride`.

⚠ The walk reaches neither an embedded struct, whose offset no call answers,
nor the entries of a table of pointers: `follow` lands on the table and
`type_size` gives the pointee's size rather than eight, so the recipe reads
wrong bytes as plausible values.

### Writing a field

`patch_field` changes one scalar or enum in place, checking in this order:

1. The grant: without *change the open document* the import does not resolve.
2. The embedder's write gate, before anything is read: `abi.not_writable`.
3. The arguments (`abi.bad_argument`) and the value, which must be a scalar or
   an enum (`abi.refused`).
4. A document the host opened read-only: `abi.not_writable`. A run that already
   replaced the file with `write_file`: `abi.refused`, because the patch would
   land in the document as it was before and be lost.
5. The description: a count, a capacity, a selector, a bit-packed or non-scalar field,
   an undeclared enum value or a value too wide is `abi.refused`, because a
   plugin patches and never rebuilds (`the_host_patches_and_never_builds`).
6. The embedder's `SaveCheck` on the patched bytes. A refusal is `abi.refused`
   and puts the bytes back exactly
   (`a_refused_patch_leaves_the_document_byte_identical`).

The last catches what the reader tolerates and the engine does not, none of it
visible in an offset or a width: a texture slot list read as a prefix with a
separately stated count, a binary-searched table that must stay sorted, an
index never checked against a shorter companion table, a scratch-buffer size
the runtime allocates from, an allocation short while its count is right, and
paired arrays sharing one subscript. `AcceptsAnything` is for tests and for a
tool with no format knowledge, never for the application. Both refusals answer
`abi.refused`, and the failure's sentence says which check refused.

### The file's bytes

A guest that links `or-io` parses the file itself, so the host hands it the
bytes and takes bytes back. The guest names no path: the file is the one the
host holds for the document the plugin was opened on, as the RSC7 file the
document would save, stored rather than deflated. The embedder builds it once
per run, and only for a plugin granted either capability.

- `file_size` writes the length, a little-endian `u32`; more than 4 GiB is
  `abi.too_large`.
- `read_file` copies `len` bytes from `offset` into the guest's memory at
  `out_ptr`, which the guest has made room for: the host builds nothing and the
  copy is the only cost it adds. A range past the end of the file, or a buffer
  past the end of the guest's memory, is `abi.bad_argument` and writes nothing.
  What the copy costs is measured in [R44f](../direction/Roadmap.md#r44f-payload-and-gpu-access).
- `write_file` replaces the file with the guest's bytes, checking in this order:
  1. The grant: without *replace the file's bytes* the import does not resolve.
  2. The embedder's write gate, before the bytes are read: `abi.not_writable`.
  3. The arguments: `abi.bad_argument`.
  4. A document the host opened read-only: `abi.not_writable`.
  5. A host that holds no file for the document, or a run that already patched
     a field: `abi.refused`.
  6. The embedder's `SaveCheck` of the file, which the app answers by reading it
     back at the document's own target and saving it in canonical form:
     `abi.refused`, and nothing changes.
  7. The bytes are kept: the next `file_size` and `read_file` serve them, and the
     embedder is handed them when the run ends.

  A kept file becomes the embedder's open document, not a file on disk: the app
  reads it back as the document's bytes, marks the document changed, and leaves
  saving, which asks the edit gate again, to the user. A run that ends in a trap
  hands the embedder nothing, so a kept write a later trap follows is not applied.
- **The sentence.** `(why_ptr, why_cap, why_len_ptr)` is a slot the host fills
  with the sentence a user is shown, whatever the refusal was: the length at
  `why_len_ptr`, always, and the longest prefix of the sentence that fits in
  `why_cap` bytes, cut at a character. The status stays the refusal's, never
  `abi.buffer_too_small`, and the length is 0 when the write was kept. The slot
  is checked and zeroed first, so a slot outside the guest's memory is
  `abi.bad_argument` before anything is kept.
- ⚠ The described tree and the file are two views of one document. A run
  patches fields or replaces the file, not both, and after a kept write the
  described reads keep answering for the document as the run began.
- ⚠ A guest that parses spends instructions before it reads a byte: `or-io`
  builds the format's description first, and the host's fuel limit is the
  embedder's, which for the app is `Limits::default` and does not cover it.

## A guest that stays loaded

A plugin that wants `cap.add_panels`, `cap.add_commands`, `cap.add_settings` or
`cap.draw_overlays` is not run once on a document. The host instantiates it
once, after the user has answered for every capability it declared, and calls it
again for as long as the editor runs, so a tool keeps its model in the guest's
memory between frames. It has the module namespace, statuses, strings and out
buffers above, and other functions: no described-tree readers and no
`patch_field`, and the document only as [the file's
bytes](#what-a-session-reaches-of-the-document). Tools → Run plugin loads it
(again, if it is loaded); a plugin already answered for loads when the editor
starts.

The guest exports these, which the host calls with `i32` arguments:

| Export | Called | Does |
|---|---|---|
| `register` | once, when the plugin loads; required | calls the registering functions below |
| `draw_panel` | `(index)`: every frame the panel registered at `index` shows | describes the panel's widgets |
| `run_command` | `(index)`: when the user runs the command registered at `index` | whatever the command does |
| `picked` | `(lod, model, geometry, polygon, vertex)`: a viewport click the guest asked for | takes the pick |

Each call has `Limits::fuel` to itself, and a call that traps or runs out ends
the session: the panels say why and Tools → Run plugin loads the plugin again.
Fuel is the longest a plugin stalls a frame, since a panel is called on the
thread that draws it.

| Function | Signature and answer | Costs |
|---|---|---|
| `register_panel` | `(spec_ptr, spec_len)` | add panels |
| `draw_widget` | `(widget_ptr, widget_len, out_ptr, out_cap, out_len_ptr)`: what the user did to the widget | add panels |
| `register_command` | `(spec_ptr, spec_len)` | add commands |
| `declare_setting` | `(spec_ptr, spec_len)` | add settings |
| `setting_value` | `(id_ptr, id_len, out_ptr, out_cap, out_len_ptr)`: an encoded setting value | add settings |
| `set_overlay` | `(ptr, len)` | draw overlays |
| `overlay_picking` | `(on)`: non-zero sends the viewport's clicks to `picked`, zero stops | draw overlays |

`file_size`, `read_file` and `write_file` are the functions above, with the
capabilities above.

```text
str       := len:u32 bytes:u8[len]       ; UTF-8; at most `MAX_NAME` bytes, and `MAX_TEXT` for a label widget's text, a text field's and a setting's
panel     := str id, str title
command   := str id, str label, str shortcut     ; an empty shortcut is none
setting   := str id, str label, u8 kind, body
             0x01 Bool    u8 default
             0x02 Number  f64 min, f64 max, f64 default
             0x03 Text    str default
value     := u8 kind, body                ; what `setting_value` answers: 0x01 u8, 0x02 f64, 0x03 str
widget    := u8 tag, body
             0x01 Label      str text
             0x02 Button     str id, str text
             0x03 Slider     str id, str label, f32 min, f32 max, f32 value
             0x04 Checkbox   str id, str label, u8 checked
             0x05 TextField  str id, str label, str text
             0x06 List       str id, u32 selected, u32 n, n × str item   ; no selection is 0xFFFFFFFF
overlay   := u32 n, n × primitive
primitive := 0x01 f32 x, f32 y, f32 z, f32 size, f32 r, f32 g, f32 b, f32 a   ; a point
           | 0x02 vertex from, vertex to                                       ; a line
vertex    := f32 x, f32 y, f32 z, f32 r, f32 g, f32 b, f32 a
```

The limits named here are the constants of
`crates/domain/or-plugin/src/surface.rs`, and the guest library's encoders are
held to the host's (`the_guest_library_encodes_the_surface_the_host_decodes`). Counts are not
allocated from: a list of four billion items in nine bytes is refused as
`abi.too_large`.

### Registering

`register_panel`, `register_command` and `declare_setting` are called from
`register` and nowhere else; later they are `abi.refused`. An id is letters,
digits, `_`, `-` and `.`, and a title or a label is not blank. A second
registration of an id is `abi.refused` and the first stands: a registration
never replaces another, and a plugin registers at most `MAX_REGISTERED` of each.
A registration the host refuses is kept with its sentence in the plugin's report in
Settings → Plugins, since the panel it would have added is one a user looks for. A panel
joins the panel registry as `plugin.<plugin id>.<panel id>`, so it is in View →
Panels and a saved layout keeps it; while the plugin is not loaded it draws a
sentence and a button that loads it.

A command's shortcut is a binding such as `Ctrl+Alt+P`. It needs Ctrl or Alt, so
that a plugin never takes a key a field is typed with, and it is dropped, the
command staying registered, where it names no key or the app or another plugin
holds it; the plugin's report in Settings → Plugins says so. It is not rebound
in Settings → Keyboard shortcuts.

A setting is shown on a page of its own for the plugin in Settings, and the
preferences keep what the user chose by plugin and setting id. The guest reads
it with `setting_value` when it needs it; the value is the default until the
user changes it, and a stored value of another kind than the setting's is the
default. A number is kept to its range, and a declaration whose default lies
outside it is `abi.refused`.

### Panels

The host calls `draw_panel`, the guest describes each widget in order with
`draw_widget`, and the host draws the frame when the call returns. egui never
crosses: the six widgets above are the whole vocabulary, drawn by the host with
whatever it draws with, and the guest cannot ask for another.

Each `draw_widget` answers what the user did to that widget since the last
frame, or the value the guest gave where nothing changed it, so a guest keeps
its value by assigning the answer: a label answers nothing, a button one byte
that is 1 once after a click, a slider its `f32` kept to its range, a checkbox
one byte, a text field its UTF-8 text and a list its selected index as a
`u32`. An event belongs to the panel it was sent from, which `draw_panel`'s index
names, so two panels may use one widget id and a frame of one panel leaves
another's events waiting. An event answers one call and is dropped at the end of
the panel's frame if no widget of that id took it; one of another kind than its
widget's is not taken.
A call answered `abi.buffer_too_small` took nothing, so a retry sees the same
event. A frame holds at most `MAX_WIDGETS` widgets, `draw_widget` outside `draw_panel`
is `abi.refused`, and two widgets of one id in a frame are the guest's bug.

### Overlays and picks

`set_overlay` replaces what the guest draws, in the model space of the open
document, and an overlay the guest sets again as it was is not a change. It is
drawn while the document that was active when the guest set it is active and over
no other, as lines in front of the meshes: a point is a cross whose arms reach its
size on each axis, and a line runs from the colour of one vertex to the other's.
A line is the renderer's one pixel wide. An overlay holds at most `MAX_PRIMITIVES`
primitives, and one with a position that is not finite draws nothing of it.

While `overlay_picking` is on, a click in the viewport goes to `picked` and
selects nothing. `(lod, model, geometry)` name the geometry of the level the
viewport draws, `polygon` is the triangle under the cursor among the geometry's
and `vertex` the index in its vertex buffer of the triangle's corner nearest
the hit. A click on nothing is not delivered. ⚠ The polygon index counts the
triangles the viewport draws, so a corner with a vertex the buffer does not hold,
which the viewport drops, shifts the later ones.

### What a session reaches of the document

Only the file's bytes, and only for a plugin granted `cap.read_file_bytes` or
`cap.write_file_bytes`. A call is handed the file the open document would save
(the RSC7 file, stored rather than deflated), which the app builds again only
once the document has changed; a call made while no document is open has no
file, and `file_size`, `read_file` and `write_file` answer `abi.refused`.
`write_file` passes the grant, the editor's write gate and the format's save
check in the order above, and the bytes it keeps become the open document when
the call returns, which marks it changed and leaves saving to the user.

## Writing several files together

*Version 1.* `write_file` replaces the one file the plugin was opened on. `write_files` hands the host a
set of files of the **project**, each by its path in the project folder, and the host keeps all of them or
none: a refusal of any file is a refusal of the set, and the sentence says that no file was written.

```text
set  := u32 n, n × (str path, u32 len, u8[len] bytes)     ; str := u32 len, UTF-8, at most MAX_NAME bytes
```

`write_files` checks, in this order, and answers the first that refuses:

1. The grant: without *write several files together* the import does not resolve.
2. The embedder's write gate, before the set is read: `abi.not_writable`. For the app that is the
   project's edit mode, so a project opened read-only refuses.
3. The pointer and the length are inside the guest's memory, or `abi.bad_argument`.
4. A call after the run already kept a set: `abi.refused`; a run writes its files in one call.
5. The bytes are this encoding, or `abi.bad_argument`. Nothing is reserved from a count.
6. The set as data: at most `MAX_WRITTEN_FILES` files, `MAX_WRITTEN_FILE` bytes for one and `MAX_WRITTEN` for
   all, no file twice (two paths that differ only in case are one on Windows), and every path inside the
   project: relative, with `/`, no empty part, no `.` or `..`, no drive or backslash, no part ending in a
   space or a dot. Otherwise `abi.refused`.
7. `SaveCheck::why_the_files_would_be_invalid`, **once, for the whole set**: `abi.refused`. A host that has
   not implemented it refuses every set, so forgetting is not a way to write.

The host writes nothing itself. A kept set is handed to the embedder as `Outcome::transaction` when the run
ends, and a run that traps hands over nothing. The app applies it as **one step of the project's History**:
it reads each file as it is now, writes the set, and if any write fails puts back what it had written and
records no step, so Undo takes all the files back together and Redo puts them all back
(`src/state/plugins/transaction.rs`), and the project is scanned again. Undo and Redo move nothing when a
file is not as the step left it, and say which. The app checks the set before it writes (`ProjectFiles`): a
project holds sources, so a game file is refused, and so are the project's own places (a folder named
`.openrage`, `build` or `.git` at any depth, in any letters, and by the 8.3 short name NTFS gives the dotted
two, such as `GIT~1`; and a project file), a `wardrobe.json` that does not read as a pack and a `.json` that does
not read. ⚠ A step keeps the bytes of each file before and after, so a project holds the copies of the files a
plugin wrote for as long as the step is kept. The step's record of the paths is a file in the project's private
folder, so Undo and Redo check each path in it as a plugin's own is checked, and a record that names a place a
plugin may not write moves nothing and says so (`a_step_whose_record_names_a_place_a_plugin_may_not_write_is_not_moved`).

The guest library's `write_files` takes `&[(&str, &[u8])]` and gives the refusal's sentence back. The limits of
a set are the constants of `crates/domain/or-plugin/src/transaction.rs`, and
`crates/domain/or-plugin/tests/several_files_are_written_together_or_not_at_all.rs` holds each check above.

## A plugin that extends a build

A validator reads the plan of a build and reports what is wrong with it. An emitter reads the same plan
and adds text files to it. Data goes in and data comes out, and nothing else crosses: the guest has no
path, no folder, no document and no clock, and each call runs a fresh instance, so what a rule says is a
function of the plan and, for a validator granted `cap.read_pack`, of the pack. A plugin extends a build and never hosts one.

A plugin whose manifest wants `cap.report_findings` or `cap.add_files` is not run on a document. The app
runs it over each plan its scan and a build make, once the user has answered for every capability it
declared (Tools → Run plugin records the answer and Settings → Plugins revokes it). The guest exports
`validate` for a validator and `emit` for an emitter, each taking and returning nothing, with a fresh
`Limits::fuel` for each call.

| Function | Signature and answer | Costs |
|---|---|---|
| `plan_size` | `(out_ptr)`: a `u32`, the length of the plan document in bytes | read the plan |
| `read_plan` | `(offset, out_ptr, len)`: `len` bytes of the document from `offset`, copied to `out_ptr` | read the plan |
| `report` | `(code_ptr, code_len, level, subject_ptr, subject_len, message_ptr, message_len)` | report findings |
| `add_file` | `(path_ptr, path_len, text_ptr, text_len)` | add files |
| `pack_size` | `(out_ptr)`: a `u32`, the length of the pack document in bytes (ABI 1) | read the pack |
| `read_pack` | `(offset, out_ptr, len)`: `len` bytes of the pack document from `offset`, copied to `out_ptr` (ABI 1) | read the pack |
| `report_with_fix` | `(code_ptr, code_len, level, subject_ptr, subject_len, message_ptr, message_len, label_ptr, label_len, files_ptr, files_len)` (ABI 1) | suggest a fix |

```text
plan    := the JSON document `openrage-cli wardrobe plan` prints for one resource (Plan_Document.md), UTF-8:
           format, schema, output, resource, kind, target, files, totals, budgets, items, problems
           (each file: path, how, bytes, and `from`, which is always `null` for a plugin)
pack    := the JSON document of the clothing the build takes, UTF-8 (ABI 1):
           { "format": "openrage-pack", "schema": 1,
             "packs": [ { "content": <the content's id>,
                          "folder": <the content's folder, by its path in the project>,
                          "wardrobe": <the content's wardrobe.json, as the file holds it> } ] }
level   := 0 note | 1 warning | 2 error
code    := letters, digits, `_`, `-` and `.`        ; the app shows it as `plugin.<plugin id>.<code>`
fix     := label (str), files (the set of Writing several files together)
```

- **The plan.** It is the document a script reads, so a plugin sees what a CI job sees, except that a file's
  `from`, where on the user's disk it is copied from, is `null`: a guest has no path, and an emitter writes
  into a resource the user publishes (`Plan::guest_document`). A key is added to
  it without a new schema and never removed or renamed (`crates/domain/or-project/src/extension.rs`). The
  host refuses to hand over one longer than `MAX_PLAN` bytes, and a read past its end is
  `abi.bad_argument` and writes nothing.
- **The pack** is the clothing the build takes, as data. A validator granted `cap.read_pack` reads it with
  `pack_size` and `read_pack`, as it reads the plan, and a pack document longer than `MAX_PLAN` is not handed
  over. It holds the wardrobe of each clothing content the build takes, with the content's id and its folder
  in the project, and no path of the user's disk. A build that takes no clothing hands an empty list, and
  `pack_size` answers the length of that. The pack is what a content says of its garments; what the files
  hold is the plan's and the engine's.
- **A finding** crosses one call at a time. The host refuses an empty or malformed code, a blank message,
  and every finding past `MAX_FINDINGS`, as `abi.refused` with the sentence the user is shown; a level
  that is none of the three is `abi.bad_argument`. An error is also a problem of the plan, so the
  build is refused; a warning is listed in the Problems list; a note is not.
- **A fix** is a finding with `report_with_fix`: the same checks, a label that says in a few words what
  accepting it does, and a set of files as in [Writing several files together](#writing-several-files-together).
  The host refuses a blank label, a set that is not one, and more than `MAX_EMITTED` bytes of files in one call,
  as `abi.refused`, and keeps **neither the fix nor its finding**. A fix writes nothing: the app shows the
  finding with Fix beside it, as it shows a rule's, and Fix lists the files it would write and asks. Accepting
  applies the set as one step of the project's History, checked as a set before any file is written, so a set
  one file of which the editor's check refuses is refused with a sentence and changes nothing.
  `cap.suggest_fixes` asks for no write: it is the person who writes.
- **A file** crosses as a path and its text. The host refuses an empty path, a text over `MAX_FILE`
  bytes, more than `MAX_FILES` files and more than `MAX_EMITTED` bytes of text in one call. What a path may
  be is the plan's to decide, since the plan is what writes it: a path that leaves the resource, a path in
  `stream/` and one that is another file's in all but case are problems of the plan naming the plugin
  (`with_emitted`), and only a FiveM resource takes an emitted file, since a singleplayer DLC registers
  each file with a type and a package names its files in a manifest, and an emitter states neither. An
  emitted file is not listed in the manifest's `files`, which a plugin cannot edit.
- **A call that traps or runs out of fuel produces nothing**: what it reported before is dropped, and the
  plugin is said as a warning of the build, never as a reason the plan is not made.
- ⚠ A guest is linked once, whichever export is called, so a guest that has both exports needs the
  grants of both. `cap.add_files` is not a write in the sense of the edit gate: an added file lands in the
  plan, which the user builds, and nothing is written before they do.

The limits are the constants of `crates/domain/or-plugin/src/extension.rs`, and
`crates/domain/or-plugin/tests/what_a_plugin_that_extends_a_build_may_and_may_not_do.rs` holds each of
them, and the sample guest built with `--features extend` is the validator and the emitter that runs
under it. The pack and the fix are held by
`crates/domain/or-plugin/tests/a_validator_reads_the_pack_and_suggests_a_fix.rs`, and the sample built with
`--features fixes` is the validator that runs under it.

## A guest, end to end

The guest `a_name_crosses_in_and_a_described_value_crosses_out` runs, reading a
`Root.ratio` that holds 2.5:

```wat
(module
  (import "openrage" "field_value"
    (func $field_value (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "Root")
  (data (i32.const 8) "ratio")
  (func (export "go")
    ;; type "Root" at 0, field "ratio" at 8, the root at offset 0;
    ;; answer into 40 with 64 bytes of room, its length written to 32
    (if (i32.ne (call $field_value (i32.const 0) (i32.const 4) (i32.const 0)
                  (i32.const 8) (i32.const 5) (i32.const 40) (i32.const 64) (i32.const 32))
                (i32.const 0))
      (then unreachable))
    ;; tag 0x02 is F32, and the four bytes after it are the value
    (if (i32.ne (i32.load8_u (i32.const 40)) (i32.const 0x02)) (then unreachable))
    (if (f32.ne (f32.load (i32.const 41)) (f32.const 2.5)) (then unreachable))))
```
