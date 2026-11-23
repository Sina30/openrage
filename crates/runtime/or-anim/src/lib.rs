// Skinning order is `world * inverse_bind`; quat blends use PrepareSlerp hemisphere flip.
pub mod binding;
pub mod clip;
pub mod edit;
#[cfg(feature = "gta6")]
pub mod gta6;
pub mod player;
pub mod pose;
pub mod resolve;
pub mod retarget;
pub mod skeleton;
pub mod skin;
#[cfg(test)]
mod testing;
pub use binding::{BindingReport, BoneBinding, ClipBinding};
pub use edit::{bake, bake_into, BakedAnimation, EditKey, EditTrack, EditValue, PoseEdits};
pub use or_transport::{LoopMode, TimeRange, Transport};
pub use player::{ClipPlayer, CombineMode};
pub use pose::{skin_matrices_from_world, BoneTransform, Pose};
pub use resolve::{
    best_skeleton, bind, clip_tags, coverage_score, rank_skeletons, BoundClip, SkeletonRanking,
};
pub use retarget::{
    decode_skinning_against, palette_bones, retarget_mesh, retarget_palette,
    retarget_palette_or_identity, PaletteBone, PaletteRetarget,
};
pub use skeleton::{mat4_of, Bone, Skeleton, NO_PARENT};
pub use skin::{
    decode_skinning, skin_channel_offsets, SkinChannelOffsets, SkinnedMesh, VertexInfluences,
    MAX_INFLUENCES,
};
