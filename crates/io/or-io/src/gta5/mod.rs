//! GTA V's paged formats: one module per format, the declarations they share,
//! and a prelude per target. A format takes the engine's names when it moves
//! to the owned form; the rest keep OpenRage's earlier ones.

pub mod bounds;
pub mod bounds_dict;
pub mod clip_dict;
pub mod cloth_dict;
pub mod data;
pub mod drawable;
pub mod drawable_codecs;
pub mod drawable_dict;
pub mod expression_dict;
pub mod formats;
pub mod fragment;
pub mod frame_filter_dict;
pub mod hdri;
pub mod navmesh;
pub mod particle;
pub mod path_region;
pub mod pose_db;
#[cfg(feature = "gen9")]
pub mod retarget;
pub mod script;
pub mod shaders;
pub mod shared;
pub mod skeleton;
pub mod text;
pub mod texture_dict;
pub mod vehicle_recording;
pub mod waypoint_recording;

pub use crate::decl::{File, HasOrigin, Member, Owned, Peek, Resource};
pub use crate::Error;
pub use bounds::{
    BoundType, Color32, CompressedVertex, LocalBoxMinMax, OctantMap, PhBVHSubtreeInfo, PhBound,
    PhBoundAny, PhBoundAnyExt, PhBoundBVH, PhBoundBox, PhBoundCapsule, PhBoundComposite,
    PhBoundCylinder, PhBoundDisc, PhBoundGeometry, PhBoundPlane, PhBoundSphere, PhMaterialMgrId,
    PhOptimizedBvh, PhOptimizedBvhNode, PhPolygon, PhPrimBox, PhPrimCapsule, PhPrimCylinder,
    PhPrimSphere, TypeAndIncludeFlags,
};
pub use clip_dict::{
    Animation, AtMap, AtMapEntry, CrAnimDictionary, CrAnimation, CrBlockStream, CrClip,
    CrClipAnimation, CrClipAnimations, CrClipAny, CrClipAnyExt, CrClipDictionary, CrProperties,
    CrProperty, CrPropertyAttribute, CrPropertyAttributeAny, CrPropertyAttributeAnyExt,
    CrPropertyAttributeBool, CrPropertyAttributeFloat, CrPropertyAttributeHashString,
    CrPropertyAttributeInt, CrPropertyAttributeQuaternion, CrPropertyAttributeVector3, CrTag,
    CrTags, Dof, EClipType, EType, StringKey,
};
pub use cloth_dict::{
    BindingInfo, CharacterCloth, CharacterClothController, ClothBridgeSimGfx, ClothController,
    ClothVertexTags, PhClothData, PhEdgeData, PhMapData, PhMorphController, PhMorphData,
    PhVerletCloth,
};
pub use drawable::{
    CLightAttr, ColourData, DrawableTarget, GrmGeometry, GrmModel, GrmShaderGroup, GtaDrawable,
    RmcDrawable, RmcLod, RmcLodGroup, VectorData,
};
pub use drawable_codecs::TextureSlot;
pub use expression_dict::{
    CrExpressionStream, CrExpressions, CrExpressionsDictionary, CrMotionDescription, Dof3, Track,
    Variable,
};
pub use fragment::{
    BgPaneModelInfoBase, ClothInstanceTuning, EnvironmentCloth, EvtSet, FragDrawable,
    FragPhysicsLOD, FragPhysicsLODGroup, FragType, FragTypeChild, FragTypeGroup,
    FwVehicleGlassWindowData, GrmMatrixSet, GtaFragType, Matrix43, PgArray, PhArchetypeDamp,
    PhArticulatedBodyType, PhJoint1DofType, PhJoint3DofType, PhJointType, PhJointTypeAny,
    PhJointTypeAnyExt, ShatterMap, ShatterMapRow, VehicleGlassWindow,
};
pub use frame_filter_dict::{
    CrFrameFilter, CrFrameFilterAny, CrFrameFilterAnyExt, CrFrameFilterDictionary,
    CrFrameFilterMultiWeight, EFrameFilterType, TrackIdIndex,
};
pub use navmesh::{
    AiSplitArray, CAdjacentNavmeshLookups, CNavMesh, CNavMeshCompressedVertex, CNavMeshCoverPoint,
    CNavMeshQuadTree, CSpecialLinkInfo, Matrix34, SubArray, TAdjPoly, TCoverPointFlags,
    TNavMeshPoly, TNavMeshQuadTreeLeafData, TShortMinMax,
};
pub use particle::{
    AtStringMap, PtxBehaviour, PtxBehaviourAny, PtxBehaviourAnyExt, PtxBiasLink, PtxDomain,
    PtxDomainObj, PtxDrawable, PtxEffectRule, PtxEffectSpawner, PtxEmitterRule, PtxEvent,
    PtxEventEmitter, PtxEvolution, PtxEvolutionList, PtxEvolvedKeyframe, PtxEvolvedKeyframeProp,
    PtxFxList, PtxKeyframe, PtxKeyframeEntry, PtxKeyframeProp, PtxParticleRule, PtxRenderState,
    PtxShaderInst, PtxShaderVar, PtxShaderVarAny, PtxShaderVarAnyExt, PtxShaderVarKeyframe,
    PtxShaderVarTexture, PtxShaderVarVector, PtxSpawnedEffectScalars, PtxTechniqueDesc,
    PtxTimeLine, PtxdModel, PtxdSprite, PtxdTrail, PtxuAcceleration, PtxuAge, PtxuAnimateTexture,
    PtxuAttractor, PtxuCameraEvolution, PtxuCollision, PtxuColour, PtxuDampening, PtxuDecal,
    PtxuDecalPool, PtxuFogVolume, PtxuLight, PtxuLiquid, PtxuMatrixWeight, PtxuNoise, PtxuRiver,
    PtxuRotation, PtxuSize, PtxuVelocity, PtxuWind, PtxuZCull,
};
pub use path_region::{
    AtBinaryMap, AtBinaryMapDataPair, CNodeAddress, CPathNode, CPathNodeLink, CPathRegion,
    CPathVirtualJunction, JunctionMapContainer, PgBaseNode, PgBasePageMap,
};
pub use shared::{
    AtBitSet, AtString, DatBase, Mat33V, Mat34V, PgBase, PgBaseRefCounted, PgDictionary, QuatV,
    SpdAABB, SpdSphere, Vec3V, Vec4V, Vector3,
};
pub use skeleton::{
    CrBoneData, CrJointControlPoint, CrJointData, CrJointRotationLimit, CrJointScaleLimit,
    CrJointTranslationLimit, CrSkeletonData,
};
pub use text::{CGxt2Entry, CGxt2Header, Gxt2, Gxt2Entry, GXT2_MAGIC};
pub use vehicle_recording::{CVehicleRecording, CVehicleStateEachFrame};
pub use waypoint_recording::{FwWaypointRecordingRoute, RouteData};

/// A prelude of Legacy-target declarations.
pub mod gen8 {
    pub use super::drawable::gen8::{
        GrcFvf, GrcIndexBuffer, GrcInstanceData, GrcTextureReference, GrcVertexBuffer,
    };
    pub use super::drawable_codecs::gen8::{EntryData, GrcInstanceDataEntry};
    pub use super::texture_dict::gen8::{GrcCellGcmTextureWrapper, GrcTexture};
}

/// A prelude of Enhanced-target declarations.
#[cfg(feature = "gen9")]
pub mod gen9 {
    pub use super::drawable::gen9::{
        GrcIndexBufferG9, GrcInstanceDataG9, GrcTextureReferenceG9, GrcVertexBufferG9,
        ShaderParamInfo, ShaderParamInfos, VertexDeclaration,
    };
    pub use super::drawable_codecs::gen9::Params;
    pub use super::texture_dict::gen9::{
        GrcTextureG9, PixelFormat, ShaderResourceViewG9, TextureDimension, TileMode,
    };
}
