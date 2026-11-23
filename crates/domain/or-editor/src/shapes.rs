//! The declared fields a grid does not write in place: one whose value counts,
//! sizes or lays out data elsewhere in the file, which an edit of it alone
//! leaves out of step, and one whose value says what kind of thing its owner
//! is, which an edit makes another kind. A grid draws each read-only with why.

const LEGACY_PIXELS: &str = "It lays out the texture's pixels, so it changes with them: \
                             re-encode the texture, or import an image over it, in the Texture \
                             workspace.";
const ENHANCED_PIXELS: &str = "It lays out the texture's pixels, so it changes only with them, \
                               and OpenRage re-encodes only Legacy textures.";
const MESH: &str = "It lays out the geometry's vertices or indices, which only a mesh import \
                    (openrage-cli mesh import) rewrites, keeping the two in step.";
const CLIP: &str = "It says how the animation's channels are packed, which a save re-encodes \
                    to match once the clip is edited in the Animation workspace.";
const MATERIAL: &str = "It says how many of a material's values there are and where they lie: \
                        the material editor writes the values and leaves it as the file has it.";
const ELSEWHERE: &str = "It counts or sizes data elsewhere in the file, and nothing in OpenRage \
                         changes how much of that data there is, so it stays as the file has it.";
const USAGE: &str = "It says what kind of texture this is, a render target or a reference among \
                     them, and the game takes it for that kind, so it stays as the file has it.";
const PRIMITIVE: &str = "It says which primitive the slot holds, and the slot's other bytes are \
                         read as that primitive, so another value would turn it into another.";

/// Each type's fields not written in place, by the engine's names, and why.
const SHAPES: &[(&str, &[&str], &str)] = &[
    (
        "GrcTexture",
        &[
            "width",
            "height",
            "depth",
            "mip_stride",
            "format",
            "image_type",
            "mip_count",
            "cut_mip_levels",
            "layer_count",
        ],
        LEGACY_PIXELS,
    ),
    (
        "GrcTextureG9",
        &[
            "width",
            "height",
            "depth",
            "dimension",
            "format",
            "tile_mode",
            "mip_levels",
        ],
        ENHANCED_PIXELS,
    ),
    (
        "GrcFvf",
        &[
            "fvf",
            "fvf_size",
            "flags",
            "dynamic_order",
            "channel_count",
            "channel_sizes",
        ],
        MESH,
    ),
    ("VertexDeclaration", &["data"], MESH),
    ("GrcIndexBufferG9", &["index_size"], MESH),
    ("GrmGeometry", &["primitive_type", "extra_vertices"], MESH),
    (
        "CrBlockStream",
        &[
            "compact_size",
            "constant_size",
            "channel_offset",
            "slop_size",
            "num_frames",
            "frame_size",
            "indirect_size",
            "quantize_size",
            "segment_size",
            "num_mover_channels",
        ],
        CLIP,
    ),
    (
        "CrAnimation",
        &["num_frames", "frames_per_chunk", "max_block_size"],
        CLIP,
    ),
    ("GrcInstanceDataEntry", &["count"], MATERIAL),
    ("ShaderParamInfo", &["data"], MATERIAL),
    ("ShaderParamInfos", &["buffer_copies"], MATERIAL),
    ("RmcDrawable", &["container_size_qw"], ELSEWHERE),
    ("GrmShaderGroup", &["container_size_qw"], ELSEWHERE),
    ("AtBitSet", &["bit_size"], ELSEWHERE),
    (
        "CPathRegion",
        &["num_nodes_car_nodes", "num_nodes_ped_nodes"],
        ELSEWHERE,
    ),
    (
        "CPathVirtualJunction",
        &["x_samples", "y_samples"],
        ELSEWHERE,
    ),
    ("FragDrawable", &["num_extra_bounds"], ELSEWHERE),
    (
        "FragPhysicsLOD",
        &["root_group_count", "num_bony_children"],
        ELSEWHERE,
    ),
    (
        "FragTypeGroup",
        &["num_children", "num_child_groups"],
        ELSEWHERE,
    ),
    ("PhOptimizedBvhNode", &["polygon_count"], ELSEWHERE),
    ("PhVerletCloth", &["num_pinned_verts"], ELSEWHERE),
    ("CrSkeletonData", &["bone_tag_slot_count"], ELSEWHERE),
    ("CrJointRotationLimit", &["num_control_points"], ELSEWHERE),
    ("CrExpressions", &["max_packed_size"], ELSEWHERE),
    ("GrcTexture", &["resource_type_and_conversion_flags"], USAGE),
    (
        "GrcTextureReference",
        &["resource_type_and_conversion_flags"],
        USAGE,
    ),
    ("GrcTextureG9", &["usage_data"], USAGE),
    ("GrcTextureReferenceG9", &["usage_data"], USAGE),
    ("PhPrimSphere", &["prim_type"], PRIMITIVE),
    ("PhPrimCapsule", &["prim_type"], PRIMITIVE),
    ("PhPrimBox", &["prim_type"], PRIMITIVE),
    ("PhPrimCylinder", &["prim_type"], PRIMITIVE),
];

/// Why `field` of `type_name` is not written in place, where its value shapes data elsewhere in the file or says what its owner is.
pub fn shapes_data(type_name: &str, field: &str) -> Option<&'static str> {
    SHAPES
        .iter()
        .find(|(ty, fields, _)| *ty == type_name && fields.contains(&field))
        .map(|(_, _, why)| *why)
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::desc::FieldRepr;

    /// Whether `field` of `type_name` is declared by some format, at some target, as one value the grid would write.
    fn written_somewhere(type_name: &str, field: &str) -> bool {
        or_io::gta5::formats::FORMATS.iter().any(|format| {
            format.targets().any(|target| {
                let (Ok(pack), Ok(reach)) = (format.description(target), format.reach(target))
                else {
                    return false;
                };
                let Some(ty) = pack.type_by_name(type_name) else {
                    return false;
                };
                let shown = reach
                    .owned_public(type_name)
                    .is_none_or(|public| public.iter().any(|(name, _)| *name == field));
                ty.field_by_name(field).is_some_and(|f| {
                    matches!(f.repr, FieldRepr::Scalar(_) | FieldRepr::Enum { .. })
                        && shown
                        && !ty.is_structural(field)
                })
            })
        })
    }

    #[test]
    fn every_listed_field_is_one_the_grid_would_otherwise_write() {
        let stale: Vec<String> = (SHAPES.iter())
            .flat_map(|(ty, fields, _)| fields.iter().map(move |field| (*ty, *field)))
            .filter(|(ty, field)| !written_somewhere(ty, field))
            .map(|(ty, field)| format!("{ty}.{field}"))
            .collect();
        assert!(
            stale.is_empty(),
            "listed, but no declared format shows it as a value the grid writes: {stale:?}"
        );
    }
}
