//! What a scanned GTA VI drawable's physical section holds beside its geometries, read when it
//! opened: the object the section starts with, its tint palettes, and the texture its shader group
//! embeds, which a shader slot binding that texture's key draws before any pack.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::gta6::headerless::{read_drawable_head, BufferKind, DrawableHead, Resource};

use super::document::DocPayload;
use super::gta6_resource::{drawable_of, texture_name};
use super::textures::Texture;

type Head = Arc<Result<DrawableHead, String>>;

/// The most heads kept of bodies no open document shows: each may hold the untiled pixels of its
/// embedded textures.
const MOST_HEADS: usize = 64;

/// A head, the path of its body and where that body's resource lies in memory.
// ⚠ An address and not a `Weak`: any `Weak` to the resource keeps `Arc::get_mut`, which every
// edit of the document uses, from ever succeeding.
struct Kept {
    resource: usize,
    path: PathBuf,
    head: Head,
}

/// The head of each scanned drawable or fragment opened last, read on the load's worker, oldest
/// first.
static HEADS: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

fn heads() -> MutexGuard<'static, Vec<Kept>> {
    (HEADS.lock()).unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read the head of the drawable, or of the fragment's drawable where `fragment`, that the scanned
/// body at `path` holds, and keep it for `resource`, the body read.
pub(super) fn keep(
    path: &Path,
    resource: &Arc<Resource>,
    virtual_section: &[u8],
    physical: &[u8],
    fragment: bool,
) {
    let head = read_drawable_head(virtual_section, Some(physical), fragment).map_err(|e| {
        tr!(
            "GTA VI's reader of the physical section's objects does not read this body: {e}",
            e = e
        )
    });
    hold(path, resource, head);
}

fn hold(path: &Path, resource: &Arc<Resource>, head: Result<DrawableHead, String>) {
    let at = Arc::as_ptr(resource) as usize;
    let mut heads = heads();
    // A body read again replaces its head, and a resource where a gone one lay replaces that one's.
    heads.retain(|k| k.path != path && k.resource != at);
    heads.push(Kept {
        resource: at,
        path: path.to_path_buf(),
        head: Arc::new(head),
    });
}

/// Drop the oldest heads past [`MOST_HEADS`] of the bodies none of `documents` shows.
pub(super) fn trim(documents: &[super::Document]) {
    let open = |path: &Path| documents.iter().any(|d| d.path.as_deref() == Some(path));
    trim_kept(&mut heads(), open);
}

fn trim_kept(heads: &mut Vec<Kept>, open: impl Fn(&Path) -> bool) {
    let closed = heads.iter().filter(|k| !open(&k.path)).count();
    let mut past = closed.saturating_sub(MOST_HEADS);
    heads.retain(|k| {
        let gone = past > 0 && !open(&k.path);
        past -= usize::from(gone);
        !gone
    });
}

/// The head the scanned body at `path` was last read with; `None` for any other document.
pub(crate) fn head_of(path: &Path) -> Option<Head> {
    let heads = heads();
    let found = heads.iter().rev().find(|k| k.path == path);
    found.map(|k| Arc::clone(&k.head))
}

/// The texture `name` that the drawable `selection` shows binds by the key of a texture its own
/// shader group embeds.
pub(super) fn texture_named<'p>(
    payload: &'p DocPayload,
    selection: Option<u64>,
    name: &str,
) -> Option<Texture<'p>> {
    let DocPayload::Gta6Drawable(resource) = payload else {
        return None;
    };
    let drawable = drawable_of(resource, selection)?;
    let key = (drawable.shaders.iter())
        .flat_map(|s| s.texture_keys.iter().flatten())
        .copied()
        .find(|key| texture_name(*key).eq_ignore_ascii_case(name))?;
    let heads = heads();
    // Only the head read with this very body: another of the same name may embed other pixels.
    let (path, head) = (heads.iter())
        .filter(|k| k.resource == Arc::as_ptr(resource) as usize)
        .filter_map(|k| Some((&k.path, (*k.head).as_ref().ok()?)))
        .find(|(_, head)| head.name == drawable.name)?;
    // What the document holds, a texture in any shape or none, is drawn in place of the body's.
    use super::gta6_embedded_edit::Held;
    let pixels = match super::gta6_embedded_edit::edited(path, key) {
        Some(Held::Texture(texture)) => texture,
        Some(Held::Removed) => return None,
        None => {
            let found = head.textures.iter().flatten().find(|t| t.key == key)?;
            found.texture.texture.as_ref().ok()?.clone()
        }
    };
    let texture = super::ttpf_document::grc(name.to_string(), pixels).ok()?;
    Some(Texture::Owned(Cow::Owned(texture)))
}

/// What the inspector says of a head: the object the physical section starts with, each texture
/// the shader group embeds, each tint palette, and the vertex buffers no model lists.
pub(crate) fn head_lines(head: &Result<DrawableHead, String>) -> Vec<String> {
    let head = match head {
        Ok(head) => head,
        Err(e) => return vec![e.clone()],
    };
    let mut lines = Vec::new();
    let palette_leads = head.palettes.iter().flatten().any(|p| p.at == 0);
    let what = match head.lead {
        Some(BufferKind::Vertex) => Some(tr!("a vertex buffer")),
        Some(BufferKind::Raw) => Some(tr!("a raw buffer")),
        Some(BufferKind::Structured) => Some(tr!("a structured buffer")),
        Some(BufferKind::Texture) => Some(tr!("a texture")),
        Some(BufferKind::CpuTexture) if palette_leads => Some(tr!("a tint palette")),
        Some(BufferKind::CpuTexture) => Some(tr!("a texture the CPU reads")),
        None => None,
    };
    lines.extend(what.map(|what| {
        tr!(
            "The physical section starts with {what}, the largest object of its first block; each geometry's vertex data is found through its own vertex buffer.",
            what = what
        )
    }));
    for texture in &head.textures {
        let t = match texture {
            Ok(t) => t,
            Err(e) => {
                lines.push(tr!(
                    "A texture its shader group embeds is not read: {e}",
                    e = e
                ));
                continue;
            }
        };
        let d = &t.texture;
        lines.push(tr!(
            "Its shader group embeds the texture {name} (hash_{hash} in its dictionary), {what}, of {mips} mips at p+{at}: a shader slot that binds its key draws it before any pack.",
            name = texture_name(t.key),
            hash = format!("{:08X}", t.hash),
            what = format!("{}\u{00d7}{}, DXGI {}", d.width, d.height, d.format),
            mips = d.mip_levels,
            at = format!("{:#X}", d.pixels_at),
        ));
        if let Err(e) = &d.texture {
            lines.push(tr!("Its pixels do not read: {e}", e = e));
        }
    }
    for palette in &head.palettes {
        lines.push(match palette {
            Ok(p) => tr_n!(
                "A tint palette of {count} colour at p+{at}, which row 0 of its shader group's table reaches.",
                "A tint palette of {count} colours at p+{at}, which row 0 of its shader group's table reaches.",
                p.colours.len(),
                at = format!("{:#X}", p.at)
            ),
            Err(e) => tr!(
                "A tint palette of its shader group's table is not read: {e}",
                e = e
            ),
        });
    }
    if head.unlisted_vertex_buffers > 0 {
        lines.push(tr_n!(
            "{count} vertex buffer of the body belongs to no geometry the drawable's models list, so it is not drawn: where the body points at its geometry is not established.",
            "{count} vertex buffers of the body belong to no geometry the drawable's models list, so they are not drawn: where the body points at their geometries is not established.",
            head.unlisted_vertex_buffers
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::headerless::{
        DictionaryTexture, Drawable, EmbeddedTexture, Resource, Shader, TintPalette,
    };

    const KEY: u64 = 0x1357_9BDF_2468_ACE0;

    fn drawable(name: &str) -> DocPayload {
        let drawable = Drawable {
            name: name.to_string(),
            box_min: [0.0; 3],
            box_max: [1.0; 3],
            shaders: vec![Shader {
                name_hash: 0x0BAD_CAFE,
                texture_keys: vec![None, Some(KEY)],
                parameters: Vec::new(),
            }],
            levels: [None, None, None, None],
            lod_distances: [9998.0; 4],
            bound: None,
            embedded: None,
            extra_skeletons: Vec::new(),
        };
        DocPayload::Gta6Drawable(Arc::new(Resource::Drawable(drawable)))
    }

    /// A head named `name` embedding a 4×4 RGBA8 texture under [`KEY`] and a palette of 3 colours.
    fn head(name: &str) -> DrawableHead {
        let pixels = (0..64).map(|i| i as u8 ^ 0x5A).collect();
        let format = or_io::gta6::Format::of_dxgi(28).expect("RGBA8 is measured");
        DrawableHead {
            name: name.to_string(),
            lead: Some(BufferKind::CpuTexture),
            textures: vec![Ok(EmbeddedTexture {
                key: KEY,
                hash: 0x2468_1357,
                texture: DictionaryTexture {
                    name: format!("hash_{KEY:016X}"),
                    width: 4,
                    height: 4,
                    depth: 1,
                    dimension: 1,
                    format: 28,
                    mip_levels: 1,
                    pixels_at: 0x100,
                    pixels_len: 64,
                    texture: Ok(or_io::gta6::Texture {
                        width: 4,
                        height: 4,
                        format,
                        levels: 1,
                        faces: 1,
                        pixels,
                    }),
                },
            })],
            palettes: vec![Ok(TintPalette {
                at: 0,
                colours: vec![[0x33, 0x22, 0x11, 0xFF]; 3],
            })],
            unlisted_vertex_buffers: 2,
        }
    }

    fn resource(payload: &DocPayload) -> &Arc<Resource> {
        let DocPayload::Gta6Drawable(r) = payload else {
            unreachable!("a GTA VI drawable")
        };
        r
    }

    #[test]
    fn an_embedded_texture_draws_for_a_slot_binding_its_key_in_the_drawable_embedding_it() {
        let name = "gta6_head_test_prop.#dr";
        let own = drawable(name);
        hold(
            Path::new("gta6_head_test_prop"),
            resource(&own),
            Ok(head(name)),
        );
        let wanted = texture_name(KEY);
        let Some(Texture::Owned(t)) = texture_named(&own, None, &wanted) else {
            panic!("the embedded texture");
        };
        assert_eq!(
            (t.width, t.height, t.name.as_str()),
            (4, 4, wanted.as_str())
        );
        let pixels: Vec<u8> = (0..64).map(|i| i as u8 ^ 0x5A).collect();
        assert_eq!(t.backing_store, pixels);
        let another = drawable("another.#dr");
        let other = texture_named(&another, None, &wanted);
        assert!(other.is_none(), "another drawable embeds nothing");
        let unbound = texture_named(&own, None, "hash_0000000000000001");
        assert!(unbound.is_none(), "a key no slot binds");
        // Another body of the same name embeds nothing of its own, so it draws none of these pixels.
        let namesake = drawable(name);
        assert!(
            texture_named(&namesake, None, &wanted).is_none(),
            "the head of another body"
        );
    }

    #[test]
    fn a_kept_head_leaves_its_body_editable_and_a_body_read_again_replaces_it() {
        let path = Path::new("gta6_head_test_again");
        let mut opened = drawable("gta6_head_test_again.#dr");
        hold(path, resource(&opened), Ok(head("first")));
        let DocPayload::Gta6Drawable(r) = &mut opened else {
            unreachable!("a GTA VI drawable")
        };
        assert!(
            Arc::get_mut(r).is_some(),
            "an edit takes the body as its own"
        );
        let again = drawable("gta6_head_test_again.#dr");
        hold(path, resource(&again), Ok(head("second")));
        let kept = heads().iter().filter(|k| k.path == path).count();
        assert_eq!(kept, 1);
        let name = head_of(path).and_then(|h| h.as_ref().as_ref().ok().map(|h| h.name.clone()));
        assert_eq!(name.as_deref(), Some("second"));
    }

    #[test]
    fn an_open_documents_head_outlasts_any_number_of_others() {
        let kept = |at: usize| Kept {
            resource: at,
            path: PathBuf::from(format!("gta6_head_test_trim_{at}")),
            head: Arc::new(Err(String::new())),
        };
        let mut heads: Vec<Kept> = (0..MOST_HEADS + 3).map(kept).collect();
        let open = |path: &Path| path == Path::new("gta6_head_test_trim_0");
        trim_kept(&mut heads, open);
        let left: Vec<usize> = heads.iter().map(|k| k.resource).collect();
        let expected: Vec<usize> = std::iter::once(0).chain(3..MOST_HEADS + 3).collect();
        assert_eq!(left, expected, "the open one and the newest of the others");
        trim_kept(&mut heads, open);
        assert_eq!(
            heads.len(),
            MOST_HEADS + 1,
            "none past the most is left to drop"
        );
    }

    #[test]
    fn the_inspector_names_what_the_physical_section_starts_with_and_what_it_holds() {
        let lines = head_lines(&Ok(head("gta6_head_lines.#dr")));
        let has = |line: usize, part: &str| lines.get(line).is_some_and(|l| l.contains(part));
        assert!(
            has(0, "starts with a tint palette, the largest"),
            "{lines:?}"
        );
        assert!(
            has(1, &texture_name(KEY)) && has(1, "hash_24681357"),
            "{lines:?}"
        );
        assert!(
            has(1, "4\u{00d7}4, DXGI 28") && has(1, "p+0x100"),
            "{lines:?}"
        );
        assert!(has(2, "A tint palette of 3 colours at p+0x0,"), "{lines:?}");
        assert!(has(3, "2 vertex buffers of the body belong"), "{lines:?}");
        assert_eq!(lines.len(), 4);
    }
}
