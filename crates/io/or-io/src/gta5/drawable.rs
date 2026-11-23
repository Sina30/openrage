//! `.ydr`: a drawable's levels of detail holding models, models holding
//! geometry, the shaders the geometry is drawn with, the rig it deforms with,
//! and the lights and collision bound the game adds.
//!
//! ⚠ **The spine is one declaration over both targets.** Enhanced changed
//! the vertex and index buffers, the shader's instance data and the texture,
//! and nothing that points at them; [`DrawableTarget`] names those four per
//! target and every record here takes it as a parameter.

use std::fmt::Debug;
use std::path::Path;

use super::bounds::PhBoundAny;
use super::drawable_codecs::gen8::EntryData;
use super::drawable_codecs::TextureSlot;
use super::shared::{DatBase, PgBase, PgDictionary, SpdAABB, SpdSphere};
use super::skeleton::{CrJointData, CrSkeletonData};
use super::texture_dict::gen8::GrcTexture;
use crate::codec::mesh::{Mesh, VertexReader, VertexSource};
use crate::decl::*;
use crate::desc::Value;
use crate::Generation;

/// What a target's drawables point at that the other target lays out differently.
pub trait DrawableTarget: TargetMarker {
    type VertexBuffer: Owned<Raw: Record> + Default + Debug + Clone + PartialEq + VertexSource;
    type IndexBuffer: Owned<Raw: Record> + Default + Debug + Clone + PartialEq;
    type InstanceData: Owned<Raw: Record> + Default + Debug + Clone + PartialEq;
    type Texture: Owned<Raw: Record> + Default + Debug + Clone + PartialEq;

    /// The indices a buffer holds.
    fn indices(buffer: &Self::IndexBuffer) -> &[u16];

    /// The dictionary codes a shader binds its embedded textures by.
    fn embedded(shader: &Self::InstanceData) -> Vec<u32>;

    /// How many vertices a stored buffer states, and how many bytes each takes.
    fn stated_vertices(buffer: &Raw<Self::VertexBuffer>) -> (usize, usize);

    /// How many indices a stored buffer states.
    fn stated_indices(buffer: &Raw<Self::IndexBuffer>) -> usize;

    /// The indices a stored buffer holds.
    fn stated_index_run<'a>(
        buffer: &View<'a, Raw<Self::IndexBuffer>>,
    ) -> Result<Run<'a, u16>, ViewError>;

    /// How many vertices an opened buffer's file stated; `None` for one no file stated.
    fn opened_vertices(buffer: &Self::VertexBuffer) -> Option<usize>;

    /// How many indices an opened buffer's file stated; `None` for one no file stated.
    fn opened_indices(buffer: &Self::IndexBuffer) -> Option<usize>;
}

fn embedded_keys<'a, T: 'a, R: 'a>(slots: impl Iterator<Item = &'a TextureSlot<T, R>>) -> Vec<u32> {
    slots
        .filter_map(|slot| match slot {
            TextureSlot::Embedded(k) => k.key,
            _ => None,
        })
        .collect()
}

impl DrawableTarget for Gen8 {
    type VertexBuffer = gen8::GrcVertexBuffer;
    type IndexBuffer = gen8::GrcIndexBuffer;
    type InstanceData = gen8::GrcInstanceData;
    type Texture = super::texture_dict::gen8::GrcTexture;

    fn indices(buffer: &gen8::GrcIndexBuffer) -> &[u16] {
        &buffer.indices
    }

    fn embedded(shader: &gen8::GrcInstanceData) -> Vec<u32> {
        embedded_keys(shader.entries.iter().filter_map(|e| match &e.data {
            EntryData::Texture(slot) => Some(slot),
            EntryData::Float(_) => None,
        }))
    }

    fn stated_vertices(buffer: &Raw<gen8::GrcVertexBuffer>) -> (usize, usize) {
        (buffer.vertex_count as usize, usize::from(buffer.stride))
    }

    fn stated_indices(buffer: &Raw<gen8::GrcIndexBuffer>) -> usize {
        // The count is the low 24 bits, as `GrcIndexBuffer.indices` states.
        (buffer.index_count & 0x00FF_FFFF) as usize
    }

    fn stated_index_run<'a>(
        buffer: &View<'a, Raw<gen8::GrcIndexBuffer>>,
    ) -> Result<Run<'a, u16>, ViewError> {
        buffer.indices()
    }

    fn opened_vertices(buffer: &gen8::GrcVertexBuffer) -> Option<usize> {
        let at = std::mem::offset_of!(Raw<gen8::GrcVertexBuffer>, vertex_count);
        buffer.__carry.hidden::<u32>(at).map(|c| *c as usize)
    }

    fn opened_indices(buffer: &gen8::GrcIndexBuffer) -> Option<usize> {
        let at = std::mem::offset_of!(Raw<gen8::GrcIndexBuffer>, index_count);
        buffer
            .__carry
            .hidden::<u32>(at)
            .map(|c| (c & 0x00FF_FFFF) as usize)
    }
}

#[cfg(feature = "gen9")]
impl DrawableTarget for Gen9 {
    type VertexBuffer = gen9::GrcVertexBufferG9;
    type IndexBuffer = gen9::GrcIndexBufferG9;
    type InstanceData = gen9::GrcInstanceDataG9;
    type Texture = super::texture_dict::gen9::GrcTextureG9;

    fn indices(buffer: &gen9::GrcIndexBufferG9) -> &[u16] {
        &buffer.indices
    }

    fn embedded(shader: &gen9::GrcInstanceDataG9) -> Vec<u32> {
        embedded_keys(shader.params.textures.iter())
    }

    fn stated_vertices(buffer: &Raw<gen9::GrcVertexBufferG9>) -> (usize, usize) {
        (buffer.vertex_count as usize, usize::from(buffer.stride))
    }

    fn stated_indices(buffer: &Raw<gen9::GrcIndexBufferG9>) -> usize {
        buffer.index_count as usize
    }

    fn stated_index_run<'a>(
        buffer: &View<'a, Raw<gen9::GrcIndexBufferG9>>,
    ) -> Result<Run<'a, u16>, ViewError> {
        buffer.indices()
    }

    fn opened_vertices(buffer: &gen9::GrcVertexBufferG9) -> Option<usize> {
        let at = std::mem::offset_of!(Raw<gen9::GrcVertexBufferG9>, vertex_count);
        buffer.__carry.hidden::<u32>(at).map(|c| *c as usize)
    }

    fn opened_indices(buffer: &gen9::GrcIndexBufferG9) -> Option<usize> {
        let at = std::mem::offset_of!(Raw<gen9::GrcIndexBufferG9>, index_count);
        buffer.__carry.hidden::<u32>(at).map(|c| *c as usize)
    }
}

/// The buffers, the instance data and the texture reference as Legacy lays them out.
pub mod gen8 {
    use super::super::drawable_codecs::gen8::{layout, EntryData, GrcInstanceDataEntry};
    use super::super::drawable_codecs::TextureSlot;
    use super::super::shaders::shader_defs::ShaderParamDef;
    use super::super::shared::{DatBase, PgBase};
    use super::super::texture_dict::gen8::{GrcCellGcmTextureWrapper, GrcTexture};
    use crate::codec::mesh::{VertexLayout, VertexSource};
    use crate::decl::*;

    /// `grcFvf`: which channels a vertex carries and how wide each is.
    #[record(size = 0x10, target = "gen8")]
    pub struct GrcFvf {
        /// Bit N set means channel N is present.
        pub fvf: u32,
        pub fvf_size: u8,
        pub flags: u8,
        pub dynamic_order: u8,
        pub channel_count: u8,
        /// Four bits per channel, indexed by channel number and not by position among those present.
        pub channel_sizes: u64,
    }

    /// `grcVertexBuffer`: one geometry's vertices, in the graphics half.
    #[record(size = 0x38, target = "gen8")]
    pub struct GrcVertexBuffer {
        pub base: DatBase,
        #[derived(Self::fvf_stride)]
        stride: u16,
        #[note("zero in every buffer shipped")]
        reserved0: u8,
        #[pad_after(4)]
        pub flags: u8,
        /// The vertices again, in every buffer shipped.
        #[phys]
        #[shares(data)]
        #[len(product = "vertex_count * stride")]
        pub lock_ptr: Shared<Vec<u8>>,
        #[pad_after(4)]
        vertex_count: u32,
        #[phys]
        #[len(product = "vertex_count * stride")]
        pub data: Vec<u8>,
        #[note("zero in every buffer shipped")]
        #[pad_after(4)]
        reserved: u32,
        pub fvf: Option<Box<GrcFvf>>,
    }

    impl GrcVertexBuffer {
        /// How many vertices `data` holds, at the carried stride a save divides its length by.
        pub fn vertex_count(&self) -> usize {
            self.data.len().checked_div(self.stride()).unwrap_or(0)
        }

        /// The stride a save writes: what the file stated, else what the FVF derives.
        fn stride(&self) -> usize {
            let stated = self
                .__carry
                .hidden::<u16>(std::mem::offset_of!(Raw<Self>, stride));
            stated
                .copied()
                .or_else(|| self.fvf_stride())
                .map_or(0, usize::from)
        }

        fn fvf_stride(&self) -> Option<u16> {
            self.fvf.as_deref().map(|f| u16::from(f.fvf_size))
        }
    }

    impl VertexSource for GrcVertexBuffer {
        fn vertex_data(&self) -> &[u8] {
            &self.data
        }
        fn vertex_layout(&self) -> Option<VertexLayout> {
            let fvf = self.fvf.as_deref()?;
            Some(VertexLayout::of(fvf.fvf, fvf.channel_sizes))
        }
        fn vertex_stride(&self) -> usize {
            self.stride()
        }
        fn vertex_declared(&self) -> usize {
            self.vertex_count()
        }
    }

    /// `grcIndexBuffer`: one geometry's indices, in the graphics half.
    #[record(size = 0x18, target = "gen8")]
    pub struct GrcIndexBuffer {
        pub base: DatBase,
        #[note("the count in the low 24 bits; the top byte is carried")]
        #[pad_after(4)]
        index_count: u32,
        #[phys]
        #[len(masked = "index_count", mask = 0x00FF_FFFF)]
        pub indices: Vec<u16>,
    }

    /// `grcInstanceData`: a Legacy shader's parameters and the textures bound into it.
    ///
    /// ⚠ **`entries` leads to one allocation of three regions**: the entry
    /// table, each parameter's rows and a table of name hashes. Only the first
    /// is at a constant place, so the codec lays the allocation out and writes
    /// `count`, `texture_count`, `spu_size` and `total_size` from it.
    #[record(size = 0x30, target = "gen8")]
    pub struct GrcInstanceData {
        #[codec(Ptr<[Raw<layout::GrcInstanceDataEntry>]>)]
        #[len(field = "count")]
        pub entries: Vec<GrcInstanceDataEntry<TextureSlot<GrcTexture, GrcTextureReference>>>,
        /// The hash of the shader's name.
        pub basis_hash: Hash32,
        pub basis_hash_code: u32,
        count: u8,
        pub draw_bucket: u8,
        pub phys_mtl_deprecated: u8,
        pub flags: u8,
        spu_size: u16,
        total_size: u16,
        /// The hash of the `.sps` preset the shader was made from.
        pub material_hash: Hash32,
        pub material_hash_code: u32,
        pub draw_bucket_mask: u32,
        pub is_instanced: u8,
        #[pad_after(1)]
        pub user_flags: u8,
        texture_count: u8,
        #[pad_after(4)]
        pub sort_key_deprecated: u32,
    }

    /// `grcTextureReference`: a texture a shader binds by name and does not embed, the `grcTexture` header alone.
    #[record(size = 0x50, target = "gen8")]
    pub struct GrcTextureReference {
        pub base: PgBase,
        pub texture: GrcCellGcmTextureWrapper,
        pub name: String,
        #[init(1)]
        ref_count: u16,
        /// Two, a reference, in every one shipped.
        pub resource_type_and_conversion_flags: u8,
        #[pad_after(4)]
        pub layer_count: u8,
        cached_texture_ptr: Ptr<Opaque>,
        pub physical_size_and_template_type: u32,
        pub handle_index: u32,
        #[pad_after(4)]
        pub extra_flags: u32,
    }

    /// The `resource_type_and_conversion_flags` a texture reference states.
    pub const REFERENCE: u8 = 2;

    impl GrcTextureReference {
        /// A reference by name alone, as a material binds a texture the drawable does not carry.
        pub fn named(name: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                resource_type_and_conversion_flags: REFERENCE,
                ..Default::default()
            }
        }

        /// A reference to `texture` by its name, with its `grcTexture` header.
        pub fn of(texture: &GrcTexture) -> Self {
            Self {
                base: texture.base.clone(),
                texture: texture.texture.clone(),
                layer_count: texture.layer_count,
                ..Self::named(texture.name.clone())
            }
        }
    }

    impl GrcInstanceData {
        /// The entry for the variable whose name hashes to `name_hash`.
        pub fn entry(
            &self,
            name_hash: u32,
        ) -> Option<&GrcInstanceDataEntry<TextureSlot<GrcTexture, GrcTextureReference>>> {
            self.entries.iter().find(|e| e.name_hash.get() == name_hash)
        }

        /// The entry for the variable whose name hashes to `name_hash`, to edit.
        pub fn entry_mut(
            &mut self,
            name_hash: u32,
        ) -> Option<&mut GrcInstanceDataEntry<TextureSlot<GrcTexture, GrcTextureReference>>>
        {
            self.entries
                .iter_mut()
                .find(|e| e.name_hash.get() == name_hash)
        }

        /// Inserts `entry` where the engine reads its kind, a texture after the leading textures and rows last; its index.
        pub fn insert_entry(
            &mut self,
            entry: GrcInstanceDataEntry<TextureSlot<GrcTexture, GrcTextureReference>>,
        ) -> usize {
            let at = match entry.data {
                EntryData::Texture(_) => self
                    .entries
                    .iter()
                    .take_while(|e| matches!(e.data, EntryData::Texture(_)))
                    .count(),
                EntryData::Float(_) => self.entries.len(),
            };
            self.entries.insert(at, entry);
            at
        }

        /// Every texture slot the material binds, in entry order.
        pub fn textures(
            &self,
        ) -> impl Iterator<Item = &TextureSlot<GrcTexture, GrcTextureReference>> {
            self.entries.iter().filter_map(|e| match &e.data {
                EntryData::Texture(slot) => Some(slot),
                EntryData::Float(_) => None,
            })
        }

        /// Every texture slot the material binds, in entry order, to edit.
        pub fn textures_mut(
            &mut self,
        ) -> impl Iterator<Item = &mut TextureSlot<GrcTexture, GrcTextureReference>> {
            self.entries.iter_mut().filter_map(|e| match &mut e.data {
                EntryData::Texture(slot) => Some(slot),
                EntryData::Float(_) => None,
            })
        }
    }

    impl<T: Default> GrcInstanceDataEntry<T> {
        /// A fresh entry for a parameter the database declares: unbound, or at its initial registers.
        ///
        /// ⚠ Keyed by the declared name, never the semantic: every resolvable
        /// Legacy drawable entry in the corpus matches the name
        /// (`a_drawable_hashes_a_parameters_declared_name_and_not_its_semantic`).
        pub fn declared(def: &ShaderParamDef) -> Self {
            Self {
                register: def.register,
                name_hash: Hash32::from_raw(def.name_hash),
                data: if def.is_binding() {
                    EntryData::Texture(T::default())
                } else {
                    EntryData::Float(
                        (def.initial_registers().into_iter())
                            .map(|[x, y, z, w]| Vec4 { x, y, z, w })
                            .collect(),
                    )
                },
            }
        }
    }
}

/// The buffers, the instance data and the texture reference as Enhanced lays them out.
#[cfg(feature = "gen9")]
pub mod gen9 {
    use super::super::drawable_codecs::gen9::Params;
    use super::super::drawable_codecs::TextureSlot;
    use super::super::shared::DatBase;
    use super::super::texture_dict::gen9::{
        GrcTextureG9, PixelFormat, ShaderResourceViewG9, TextureDimension, TileMode,
    };
    use crate::codec::mesh::{elements_g9, VertexLayout, VertexSource};
    use crate::decl::*;

    /// `VertexDeclaration`: where each of 52 channels sits in an Enhanced vertex, how wide it is, and its format.
    #[record(size = 0x140, target = "gen9")]
    pub struct VertexDeclaration {
        pub offsets: [u32; 52],
        pub sizes: [u8; 52],
        pub types: [u8; 52],
        pub data: u64,
    }

    /// `grcVertexBuffer` as Enhanced lays it out: one geometry's vertices, in the graphics half.
    #[record(size = 0x40, target = "gen9")]
    pub struct GrcVertexBufferG9 {
        pub base: DatBase,
        vertex_count: u32,
        #[derived(Self::declared_stride)]
        stride: u16,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_stride: u16,
        pub bind_flags: u32,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_bind_flags: u32,
        #[phys]
        #[len(product = "vertex_count * stride")]
        pub data: Vec<u8>,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_data: u64,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        second_after_data: u64,
        /// The view the renderer binds the buffer through.
        pub srv: Option<Box<ShaderResourceViewG9>>,
        pub info: Option<Box<VertexDeclaration>>,
    }

    impl GrcVertexBufferG9 {
        /// How many vertices `data` holds, at the carried stride a save divides its length by.
        pub fn vertex_count(&self) -> usize {
            self.data.len().checked_div(self.stride()).unwrap_or(0)
        }

        /// The stride a save writes: what the file stated, else what the declaration derives.
        fn stride(&self) -> usize {
            let stated = self
                .__carry
                .hidden::<u16>(std::mem::offset_of!(Raw<Self>, stride));
            stated
                .copied()
                .or_else(|| self.declared_stride())
                .map_or(0, usize::from)
        }

        /// Bits 2 to 9 of the declaration's `data` state the vertex size.
        fn declared_stride(&self) -> Option<u16> {
            self.info.as_deref().map(|d| ((d.data >> 2) & 0xFF) as u16)
        }
    }

    impl VertexSource for GrcVertexBufferG9 {
        fn vertex_data(&self) -> &[u8] {
            &self.data
        }
        fn vertex_layout(&self) -> Option<VertexLayout> {
            let d = self.info.as_deref()?;
            Some(VertexLayout::of_g9(&elements_g9(
                &d.offsets, &d.sizes, &d.types,
            )))
        }
        fn vertex_stride(&self) -> usize {
            self.stride()
        }
        fn vertex_declared(&self) -> usize {
            self.vertex_count()
        }
    }

    /// `grcIndexBuffer` as Enhanced lays it out: one geometry's indices, in the graphics half.
    #[record(size = 0x40, target = "gen9")]
    pub struct GrcIndexBufferG9 {
        pub base: DatBase,
        index_count: u32,
        /// Bytes per index: two in every buffer shipped.
        pub index_size: u16,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_index_size: u16,
        pub bind_flags: u32,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_bind_flags: u32,
        #[phys]
        #[len(field = "index_count")]
        pub indices: Vec<u16>,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_indices: u64,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        second_after_indices: u64,
        /// The view the renderer binds the buffer through.
        pub srv: Option<Box<ShaderResourceViewG9>>,
        #[note("zero in every buffer shipped: no other value to name it by, so carried")]
        after_srv: u64,
    }

    /// `ShaderParamInfo`: one named parameter of an Enhanced shader and where its value is.
    #[record(size = 0x08, target = "gen9")]
    pub struct ShaderParamInfo {
        pub name: Hash32,
        /// The kind and slot, or a constant's buffer and byte range, packed as the accessors below read them.
        pub data: u32,
    }

    impl ShaderParamInfo {
        const TEXTURE: u32 = 0;
        const OTHER: u32 = 1;
        const SAMPLER: u32 = 2;
        const CONSTANT: u32 = 3;

        fn kind(&self) -> u32 {
            self.data & 0x3
        }

        fn slot(&self) -> usize {
            ((self.data >> 2) & 0xFF) as usize
        }

        /// The texture slot a texture parameter binds.
        pub fn texture(&self) -> Option<usize> {
            (self.kind() == Self::TEXTURE).then(|| self.slot())
        }

        /// The other slot a kind-1 parameter names.
        pub fn other(&self) -> Option<usize> {
            (self.kind() == Self::OTHER).then(|| self.slot())
        }

        /// The sampler a sampler parameter names.
        pub fn sampler(&self) -> Option<usize> {
            (self.kind() == Self::SAMPLER).then(|| self.slot())
        }

        /// The constant buffer a constant reads, and the byte range in it.
        pub fn constant(&self) -> Option<(usize, std::ops::Range<usize>)> {
            (self.kind() == Self::CONSTANT).then(|| {
                let offset = ((self.data >> 8) & 0xFFF) as usize;
                let length = ((self.data >> 20) & 0xFFF) as usize;
                (((self.data >> 2) & 0x3F) as usize, offset..offset + length)
            })
        }
    }

    /// `ShaderParamInfos`: how many of each kind of slot an Enhanced shader's block holds, then every parameter's info.
    ///
    /// ⚠ The slot tables are repeated `buffer_copies` times, one per worker
    /// thread, and each copy addresses its own payload.
    #[record(size = 0x08, target = "gen9")]
    pub struct ShaderParamInfos {
        num_buffers: u8,
        num_textures: u8,
        num_unknowns: u8,
        num_samplers: u8,
        num_params: u8,
        #[note("zero in every shader shipped: no other value to name it by, so carried")]
        after_num_params: u8,
        #[note("zero in every shader shipped: no other value to name it by, so carried")]
        second_after_num_params: u8,
        pub buffer_copies: u8,
        #[trailing]
        #[len(field = "num_params")]
        pub params: Vec<ShaderParamInfo>,
    }

    /// `grcInstanceData` as Enhanced lays it out: a shader's parameters and the textures bound into it.
    ///
    /// ⚠ **`params`, `texture_refs` and `other_refs` address one allocation**,
    /// whose parts the counts in `param_infos` locate, so the codec lays it
    /// out and writes those counts and `param_data_size` from it.
    #[record(size = 0x40, target = "gen9", recompute = Self::slots)]
    pub struct GrcInstanceDataG9 {
        /// The hash of the shader's name.
        pub name: Hash32,
        /// The hash of the `.sps` preset the shader was made from.
        pub preset: Hash32,
        #[codec(Ptr<Opaque>)]
        pub params: Params<TextureSlot<GrcTextureG9, GrcTextureReferenceG9>>,
        texture_refs: Ptr<Opaque>,
        #[note("null unless `params` holds other slots")]
        other_refs: Ptr<Opaque>,
        pub param_infos: Box<ShaderParamInfos>,
        #[note("zero in every shader shipped: no other value to name it by, so carried")]
        after_param_infos: u64,
        #[note("zero in every shader shipped: no other value to name it by, so carried")]
        second_after_param_infos: u64,
        #[note("zero in every shader shipped: no other value to name it by, so carried")]
        third_after_param_infos: u8,
        pub draw_bucket: u8,
        param_data_size: u16,
        pub draw_bucket_mask: u32,
    }

    impl GrcInstanceDataG9 {
        /// The info for the parameter whose name hashes to `name_hash`.
        pub fn param(&self, name_hash: u32) -> Option<&ShaderParamInfo> {
            (self.param_infos.params.iter()).find(|p| p.name.get() == name_hash)
        }

        /// Every texture slot a texture parameter binds, in parameter order.
        pub fn textures(
            &self,
        ) -> impl Iterator<Item = &TextureSlot<GrcTextureG9, GrcTextureReferenceG9>> {
            (self.param_infos.params.iter()).filter_map(|p| self.params.textures.get(p.texture()?))
        }

        /// The bytes a constant reads, to read or write in place; `None` for another kind or a range outside its buffer.
        pub fn constant_mut(&mut self, name_hash: u32) -> Option<&mut [u8]> {
            let (buffer, range) = self.param(name_hash)?.constant()?;
            self.params.buffers.get_mut(buffer)?.get_mut(range)
        }

        /// Refuses a parameter whose slot or range lies outside the block, and kinds the block does not count.
        ///
        /// ⚠ The block's counts are its lists' lengths, but each parameter
        /// reaches its region by an index of its own, and an index past its
        /// region reads the next one. Two parameters naming one slot, or a
        /// slot no parameter names, are not refused: each addresses a real
        /// slot, and the regions are placed from the lists' lengths.
        fn slots(
            &self,
            _: &mut Raw<Self>,
            _: &mut [(u32, crate::desc::Value)],
            _: usize,
            _: &Cx,
        ) -> Result<(), OwnedError> {
            let p = &self.params;
            let refuse = |why: String| {
                Err(OwnedError::Invalid {
                    what: format!("GrcInstanceDataG9 {:#010x}", self.name.get()),
                    why,
                })
            };
            for info in &self.param_infos.params {
                let name = info.name.get();
                if let Some(slot) = info.texture() {
                    if slot >= p.textures.len() {
                        return refuse(format!(
                            "{name:#010x} binds texture slot {slot} of {}",
                            p.textures.len()
                        ));
                    }
                } else if let Some(slot) = info.sampler() {
                    if slot >= p.samplers.len() {
                        return refuse(format!(
                            "{name:#010x} names sampler {slot} of {}",
                            p.samplers.len()
                        ));
                    }
                } else if let Some((buffer, range)) = info.constant() {
                    match p.buffers.get(buffer) {
                        None => {
                            return refuse(format!(
                                "{name:#010x} reads constant buffer {buffer} of {}",
                                p.buffers.len()
                            ))
                        }
                        Some(b) if range.end > b.len() => {
                            return refuse(format!(
                                "{name:#010x} reads bytes {range:?} of a {}-byte constant buffer",
                                b.len()
                            ))
                        }
                        Some(_) => {}
                    }
                } else if let Some(slot) = info.other() {
                    if slot >= p.others {
                        return refuse(format!(
                            "{name:#010x} names other slot {slot} of {}",
                            p.others
                        ));
                    }
                }
            }
            Ok(())
        }
    }

    /// `grcTextureReference` as Enhanced lays it out: a texture a shader binds by name, the texture's first 0x50 bytes.
    #[record(size = 0x50, target = "gen9")]
    pub struct GrcTextureReferenceG9 {
        #[note("a runtime address, carried")]
        vft: u64,
        #[note("zero in every reference shipped, which has no payload")]
        block_count: u32,
        #[note("zero in every reference shipped, which has no payload")]
        block_stride: u32,
        pub flags: u32,
        #[note("zero in every reference shipped: no other value to name it by, so carried")]
        after_flags: u32,
        pub width: u16,
        pub height: u16,
        pub depth: u16,
        pub dimension: Open<TextureDimension>,
        pub format: Open<PixelFormat>,
        pub tile_mode: Open<TileMode>,
        pub anti_alias_type: u8,
        pub mip_levels: u8,
        #[note("zero in every reference shipped: no other value to name it by, so carried")]
        after_mip_levels: u8,
        #[note("zero in every reference shipped: no other value to name it by, so carried")]
        second_after_mip_levels: u8,
        #[note("zero in every reference shipped: no other value to name it by, so carried")]
        third_after_mip_levels: u8,
        #[init(1)]
        ref_count: u16,
        pub name: String,
        #[note("null in every reference shipped, which embeds no view")]
        view_ptr: Ptr<Opaque>,
        #[note("null in every reference shipped, which has no payload")]
        pixels: Ptr<Opaque>,
        pub usage_data: u32,
        #[note("zero in every reference shipped: no other value to name it by, so carried")]
        after_usage_data: u32,
        #[note("zero in every reference shipped: no other value to name it by, so carried")]
        second_after_usage_data: u64,
    }
}

/// `grmGeometryQB`: one submesh, a vertex buffer and an index buffer and how to draw them.
#[record(size = 0x98, recompute = Self::counts)]
pub struct GrmGeometry<T: DrawableTarget> {
    pub base: DatBase,
    #[note("null in every geometry shipped")]
    vtx_decl: Ptr<Opaque>,
    #[pad_after(4)]
    pub geom_type: i32,
    pub vertex_buffer: Box<T::VertexBuffer>,
    #[note("null in every geometry shipped")]
    vb_slot1: Ptr<Opaque>,
    #[note("null in every geometry shipped")]
    vb_slot2: Ptr<Opaque>,
    #[note("null in every geometry shipped")]
    vb_slot3: Ptr<Opaque>,
    pub index_buffer: Box<T::IndexBuffer>,
    #[note("null in every geometry shipped")]
    ib_slot1: Ptr<Opaque>,
    #[note("null in every geometry shipped")]
    ib_slot2: Ptr<Opaque>,
    #[note("null in every geometry shipped")]
    ib_slot3: Ptr<Opaque>,
    #[note("the index buffer's length; written at save")]
    indices_count: u32,
    #[note("a triangle list's indices over three, written at save; carried for another type")]
    primitive_count: u32,
    vertices_count: u16,
    pub primitive_type: u8,
    #[pad_after(4)]
    pub double_buffered: u8,
    /// The bones this geometry skins against, as skeleton indices.
    #[len(field = "matrix_count")]
    pub matrix_palette: Vec<u16>,
    #[derived(Self::buffer_stride)]
    vertex_stride: u8,
    pub extra_vertices: u8,
    #[pad_after(4)]
    matrix_count: u16,
    /// The vertex buffer's vertices again, in every geometry shipped.
    #[phys]
    #[shares(vertex_buffer.data)]
    #[len(product = "vertices_count * vertex_stride")]
    pub vertex_data: Shared<Vec<u8>>,
    #[note("null in every geometry shipped")]
    vtx_decl_offset: Ptr<Opaque>,
    #[note("null in every geometry shipped")]
    offset_buffer: Ptr<Opaque>,
    #[pad_after(4)]
    pub index_offset: u32,
}

impl<T: DrawableTarget> GrmGeometry<T> {
    /// `grmPrimitiveType`'s triangle list, the type every shipped geometry states.
    const TRIS: u8 = 3;

    /// Writes the index buffer's length and, for a triangle list, the primitives it draws.
    fn counts(
        &self,
        raw: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        let indices = T::indices(&self.index_buffer).len();
        let invalid = |why: String| OwnedError::Invalid {
            what: "GrmGeometry.index_buffer".into(),
            why,
        };
        raw.indices_count = u32::try_from(indices).map_err(|_| {
            invalid(format!(
                "{indices} indices, and the count is thirty-two bits"
            ))
        })?;
        if self.primitive_type == Self::TRIS && !indices.is_multiple_of(3) {
            return Err(invalid(format!(
                "{indices} indices, and a triangle list takes three a primitive"
            )));
        }
        raw.primitive_count = self.primitive_count();
        Ok(())
    }

    /// Each count the file stated against the run it describes that a save rewrites, as `(count, stated, written, lossless)`.
    ///
    /// ⚠ A count the file stated in agreement with its run is rewritten by an
    /// edit to the run, which is not a repair. An index count disagreeing with
    /// the index buffer is two statements of one run, and rewriting it changes
    /// which indices the game draws, so it is never lossless.
    fn restated(&self) -> Vec<(&'static str, usize, usize, bool)> {
        let stated = |at: usize| self.__carry.hidden::<u32>(at).map(|c| *c as usize);
        let stated_indices = stated(std::mem::offset_of!(Raw<Self>, indices_count));
        let indices = T::indices(&self.index_buffer).len();
        let mut out = Vec::new();
        if let (Some(s), Some(run)) = (stated_indices, T::opened_indices(&self.index_buffer)) {
            if s != run && s != indices {
                out.push(("indices_count", s, indices, false));
            }
        }
        let stated_primitives = stated(std::mem::offset_of!(Raw<Self>, primitive_count));
        if let (true, Some(p), Some(i)) = (
            self.primitive_type == Self::TRIS,
            stated_primitives,
            stated_indices,
        ) {
            let written = self.primitive_count() as usize;
            if p * 3 != i && p != written {
                out.push(("primitive_count", p, written, true));
            }
        }
        // ⚠ Only a geometry whose vertices are its buffer's: one carrying its own was read at its stated count.
        if matches!(self.vertex_data, Shared::Same) {
            let at = std::mem::offset_of!(Raw<Self>, vertices_count);
            let s = self.__carry.hidden::<u16>(at).map(|c| usize::from(*c));
            if let (Some(s), Some(run)) = (s, T::opened_vertices(&self.vertex_buffer)) {
                let written = self.vertex_buffer.vertex_declared();
                if s != run && s != written {
                    out.push(("vertices_count", s, written, true));
                }
            }
        }
        out
    }

    /// Drops the counts the file stated, so a save states the runs' lengths: for runs replaced whole.
    pub fn take_counts_from_runs(&mut self) {
        self.__carry
            .forget(std::mem::offset_of!(Raw<Self>, indices_count));
        self.__carry
            .forget(std::mem::offset_of!(Raw<Self>, vertices_count));
        // ⚠ Another primitive type's count is carried, and is all a save has to state.
        if self.primitive_type == Self::TRIS {
            self.__carry
                .forget(std::mem::offset_of!(Raw<Self>, primitive_count));
        }
    }

    /// Drops the stride and the palette count the file stated, so a save states what the buffer and the palette are now: for a buffer replaced by one of another declaration.
    pub fn take_layout_from_buffer(&mut self) {
        self.__carry
            .forget(std::mem::offset_of!(Raw<Self>, vertex_stride));
        self.__carry
            .forget(std::mem::offset_of!(Raw<Self>, matrix_count));
    }

    /// The vertex buffer's stride, where a byte holds it.
    fn buffer_stride(&self) -> Option<u8> {
        u8::try_from(self.vertex_buffer.vertex_stride()).ok()
    }

    /// How many primitives a save states: a triangle list's indices over three, else the count carried.
    pub fn primitive_count(&self) -> u32 {
        if self.primitive_type == Self::TRIS {
            return u32::try_from(T::indices(&self.index_buffer).len() / 3).unwrap_or(u32::MAX);
        }
        self.__carry
            .hidden::<u32>(std::mem::offset_of!(Raw<Self>, primitive_count))
            .copied()
            .unwrap_or(0)
    }

    /// The geometry's positions, normals, lowest uvs and indices, whatever target laid its vertices out.
    pub fn mesh(&self) -> Mesh {
        Mesh::decode(
            VertexReader::new(&*self.vertex_buffer),
            T::indices(&self.index_buffer),
        )
    }
}

/// `grmModel`: geometries drawn together at one level of detail, a box each, and the shader each uses.
#[record(size = 0x30)]
pub struct GrmModel<T: DrawableTarget> {
    pub base: DatBase,
    pub geometries: Vec<Box<GrmGeometry<T>>>,
    /// ⚠ One box per geometry, and the union of them in front once there is
    /// more than one: box `i` is at `i + 1` above one geometry and at `0` at one.
    #[len(prefixed_above_one = "geom_count")]
    pub aabbs: Vec<SpdAABB>,
    /// Which shader each geometry draws with, as an index into the shader group.
    #[len(field = "geom_count")]
    pub shader_indices: Vec<u16>,
    pub matrix_count: u8,
    pub flags: u8,
    pub geom_type: u8,
    pub matrix_index: u8,
    pub mask: u8,
    pub skin_flag_and_tess: u8,
    geom_count: u16,
}

/// `rmcLod`: the models at one level of detail.
#[record(size = 0x10)]
pub struct RmcLod<T: DrawableTarget> {
    pub models: Vec<Box<GrmModel<T>>>,
}

/// `rmcLodGroup`: four levels of detail, the distances they switch at, and the bounds of the whole.
#[record(size = 0x70)]
pub struct RmcLodGroup<T: DrawableTarget> {
    pub bounds_sphere: SpdSphere,
    pub bounds_box: SpdAABB,
    /// High, medium, low and very low; a slot the game leaves null reads as an empty level.
    pub lods: [NullIsEmpty<RmcLod<T>>; 4],
    pub lod_distances: [f32; 4],
    pub bucket_masks: [u32; 4],
}

/// `grmShaderGroup`: the shaders a drawable's geometry names, and the textures they sample.
#[record(size = 0x40, recompute = Self::bindings)]
pub struct GrmShaderGroup<T: DrawableTarget> {
    pub base: DatBase,
    /// Embedded in the drawable, and not shared with a `.ytd`.
    pub texture_dictionary: Option<Box<PgDictionary<T::Texture>>>,
    pub shaders: Vec<Box<T::InstanceData>>,
    #[note("empty in every shader group shipped")]
    shader_group_vars: Vec<u8>,
    pub container_size_qw: u16,
    #[pad_after(5)]
    pub has_instanced: u8,
    #[note("a runtime address in 2 of 18 shader groups shipped, carried")]
    shader_datas: Ptr<Opaque>,
}

impl<T: DrawableTarget> GrmShaderGroup<T> {
    /// Refuses a shader binding a code the group's own dictionary does not hold.
    fn bindings(
        &self,
        _: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        _: &Cx,
    ) -> Result<(), OwnedError> {
        let codes: &[u32] = self.texture_dictionary.as_deref().map_or(&[], |d| &d.codes);
        for (i, shader) in self.shaders.iter().enumerate() {
            if let Some(code) = T::embedded(shader).into_iter().find(|c| !codes.contains(c)) {
                return Err(OwnedError::Invalid {
                    what: "GrmShaderGroup.shaders".into(),
                    why: format!(
                        "shader {i} binds the embedded texture {code:#010x}, and the group's \
                         dictionary holds no texture under that code"
                    ),
                });
            }
        }
        Ok(())
    }
}

/// A Legacy group's textures by name, as its materials bind them.
impl GrmShaderGroup<Gen8> {
    /// The slot a material binds `name` by: the dictionary's entry where it holds one, else a reference by name.
    pub fn binding(&self, name: &str) -> TextureSlot<GrcTexture, gen8::GrcTextureReference> {
        let code = crate::hash::string_hash_str(name);
        match self.texture_dictionary.as_deref() {
            Some(d) if d.codes.contains(&code) => TextureSlot::Embedded(Keyed::to(code)),
            _ => TextureSlot::Reference(Box::new(gen8::GrcTextureReference::named(name))),
        }
    }

    /// The name `slot` binds: its entry's in the group's dictionary where embedded, else its reference's; `None` when unbound.
    pub fn slot_name<'a>(
        &'a self,
        slot: &'a TextureSlot<GrcTexture, gen8::GrcTextureReference>,
    ) -> Option<&'a str> {
        match slot {
            TextureSlot::Unbound => None,
            TextureSlot::Reference(r) => Some(&r.name),
            TextureSlot::Embedded(k) => {
                let d = self.texture_dictionary.as_deref()?;
                let at = d.codes.iter().position(|c| Some(*c) == k.key)?;
                d.entries.get(at)?.as_deref().map(|t| t.name.as_str())
            }
        }
    }

    /// The shaders binding `name`, whatever its case, embedded or by reference.
    pub fn users(&self, name: &str) -> Vec<usize> {
        let code = crate::hash::string_hash_str(name);
        (self.shaders.iter().enumerate())
            .filter(|(_, s)| s.textures().any(|slot| binds(slot, name, code)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Carries `texture` in the group's dictionary and rebinds every reference to its name there; the texture it replaces.
    pub fn embed(&mut self, texture: GrcTexture) -> Option<GrcTexture> {
        let code = crate::hash::string_hash_str(&texture.name);
        let name = texture.name.clone();
        let replaced = self
            .texture_dictionary
            .get_or_insert_with(Default::default)
            .insert(texture);
        for slot in self.shaders.iter_mut().flat_map(|s| s.textures_mut()) {
            if binds(slot, &name, code) {
                *slot = TextureSlot::Embedded(Keyed::to(code));
            }
        }
        replaced
    }

    /// Stops carrying `name` and rebinds each material that bound it to a reference by that name.
    pub fn unembed(&mut self, name: &str) -> Option<GrcTexture> {
        let code = crate::hash::string_hash_str(name);
        let dictionary = self.texture_dictionary.as_deref_mut()?;
        let at = dictionary.codes.iter().position(|c| *c == code)?;
        dictionary.entries.get(at)?.as_ref()?;
        dictionary.codes.remove(at);
        let texture = *dictionary.entries.remove(at)?;
        // ⚠ A drawable that carries nothing and one with an empty dictionary
        // are different files, and the former is what an unembedded one is.
        if dictionary.codes.is_empty() {
            self.texture_dictionary = None;
        }
        let reference = gen8::GrcTextureReference::of(&texture);
        for slot in self.shaders.iter_mut().flat_map(|s| s.textures_mut()) {
            if binds(slot, name, code) {
                *slot = TextureSlot::Reference(Box::new(reference.clone()));
            }
        }
        Some(texture)
    }
}

#[cfg(feature = "gen9")]
impl GrmShaderGroup<Gen9> {
    /// The name `slot` binds: its entry's in the group's dictionary where embedded, else its reference's; `None` when unbound.
    pub fn slot_name<'a>(
        &'a self,
        slot: &'a TextureSlot<super::texture_dict::gen9::GrcTextureG9, gen9::GrcTextureReferenceG9>,
    ) -> Option<&'a str> {
        match slot {
            TextureSlot::Unbound => None,
            TextureSlot::Reference(r) => Some(&r.name),
            TextureSlot::Embedded(k) => {
                let d = self.texture_dictionary.as_deref()?;
                let at = d.codes.iter().position(|c| Some(*c) == k.key)?;
                d.entries.get(at)?.as_deref().map(|t| t.name.as_str())
            }
        }
    }
}

fn binds(slot: &TextureSlot<GrcTexture, gen8::GrcTextureReference>, name: &str, code: u32) -> bool {
    match slot {
        TextureSlot::Embedded(k) => k.key == Some(code),
        TextureSlot::Reference(r) => r.name.eq_ignore_ascii_case(name),
        TextureSlot::Unbound => false,
    }
}

/// `rmcDrawable`: levels of detail, their shaders and their rig.
#[record(size = 0xb0, recompute = Self::repair)]
pub struct RmcDrawable<T: DrawableTarget> {
    pub base: PgBase,
    pub shader_group: Option<Box<GrmShaderGroup<T>>>,
    pub skeleton: Option<Box<CrSkeletonData>>,
    pub lod_group: RmcLodGroup<T>,
    pub joint_data: Option<Box<CrJointData>>,
    pub handle_index: u16,
    #[pad_after(4)]
    pub container_size_qw: u16,
    /// The first level of detail again where set: in 12 of 13 Legacy drawables shipped, and in no Enhanced one.
    #[shares(lod_group.lods[0])]
    pub container: Shared<Option<Box<RmcLod<T>>>>,
    /// ⚠ In 12 of 13 shipped Legacy `.ydr` this word is not an address: it
    /// reads empty and is written back while the name stays empty.
    pub name: String,
}

impl<T: DrawableTarget> RmcDrawable<T> {
    /// Refuses a save rewriting a count a geometry's file stated against its run, unless `cx` repairs and every rewrite is lossless.
    fn repair(
        &self,
        _: &mut Raw<Self>,
        _: &mut [(u32, Value)],
        _: usize,
        cx: &Cx,
    ) -> Result<(), OwnedError> {
        let container = match &self.container {
            Shared::Own(Some(level)) => Some((LodSlot::Container, &**level)),
            _ => None,
        };
        let levels = (self.lod_group.lods.iter().enumerate())
            .map(|(i, level)| (LodSlot::Level(i), &**level))
            .chain(container);
        let mut found = Vec::new();
        for (lod, level) in levels {
            for (m, model) in level.models.iter().enumerate() {
                for (g, geometry) in model.geometries.iter().enumerate() {
                    for (count, stated, written, lossless) in geometry.restated() {
                        let at = format!("{lod}/model{m}/geometry{g}");
                        found.push((at, count, stated, written, lossless));
                    }
                }
            }
        }
        // Spells the first eight, as a mesh refusal does.
        let spell = |lossless: Option<bool>| {
            let named: Vec<String> = (found.iter())
                .filter(|f| lossless.is_none_or(|l| f.4 == l))
                .map(|(at, count, stated, written, _)| {
                    format!("{at} states {count} {stated} and its run makes {written}")
                })
                .collect();
            let more = named.len().saturating_sub(8);
            let mut out = named.into_iter().take(8).collect::<Vec<_>>().join("; ");
            if more > 0 {
                out.push_str(&format!(" and {more} more"));
            }
            out
        };
        let refuse = |why: String| {
            Err(OwnedError::Invalid {
                what: "the drawable's mesh counts".into(),
                why,
            })
        };
        match cx.repair() {
            _ if found.is_empty() => Ok(()),
            Repair::Refuse => refuse(format!(
                "{}; saving rewrites them from their runs, which OpenRage does only when asked \
                 to apply its fixes",
                spell(None)
            )),
            Repair::Lossless if found.iter().any(|f| !f.4) => refuse(format!(
                "{}; rewriting these would change which bytes the game reads, so they are not \
                 repaired",
                spell(Some(false))
            )),
            Repair::Lossless => {
                for (at, count, stated, written, _) in found {
                    cx.note_repair(format!("{at}: {count} {stated} rewritten as {written}"));
                }
                Ok(())
            }
        }
    }

    /// Moves shader `from` to `to` and renumbers every model naming one; an index past the group panics, as `Vec::remove` does.
    ///
    /// ⚠ A model names its shaders by position, so a `Vec` move alone
    /// repaints every geometry naming a shader between the two.
    pub fn move_shader(&mut self, from: usize, to: usize) {
        let Some(group) = self.shader_group.as_deref_mut() else {
            return;
        };
        let shader = group.shaders.remove(from);
        group.shaders.insert(to, shader);
        let renumber = |i: &mut u16| {
            let at = usize::from(*i);
            let moved = if at == from {
                to
            } else if from < to && (from + 1..=to).contains(&at) {
                at - 1
            } else if to < from && (to..from).contains(&at) {
                at + 1
            } else {
                at
            };
            *i = u16::try_from(moved).unwrap_or(*i);
        };
        let container = match &mut self.container {
            Shared::Own(Some(lod)) => Some(&mut **lod),
            _ => None,
        };
        let lods = self.lod_group.lods.iter_mut().map(|l| &mut **l);
        for lod in lods.chain(container) {
            for model in &mut lod.models {
                model.shader_indices.iter_mut().for_each(&renumber);
            }
        }
    }
}

/// `VectorData`: three floats, unaligned.
#[record(size = 0x0c)]
pub struct VectorData {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// `ColourData`: three bytes of colour, no alpha.
#[record(size = 0x03)]
pub struct ColourData {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// `CLightAttr`: one light a drawable emits.
///
/// ⚠ `light_type` says which fields apply, and the others are present and
/// unzeroed, so a light's type is never inferred from which look set.
#[record(size = 0xa8)]
pub struct CLightAttr {
    #[note("zero in every light shipped")]
    vft: u64,
    #[pad_after(4)]
    pub position: VectorData,
    pub colour: ColourData,
    pub flashiness: u8,
    pub intensity: f32,
    pub flags: u32,
    /// The bone the light rides on, or -1.
    pub bone_tag: i16,
    /// 1 point, 2 spot, 4 capsule.
    pub light_type: u8,
    pub group_id: u8,
    pub time_flags: u32,
    pub falloff: f32,
    pub falloff_exponent: f32,
    pub culling_plane: [f32; 4],
    pub shadow_blur: u8,
    #[note("zero in every light shipped")]
    padding1: u8,
    #[note("zero in every light shipped")]
    padding2: i16,
    #[note("zero in every light shipped")]
    padding3: u32,
    pub vol_intensity: f32,
    pub vol_size_scale: f32,
    pub vol_outer_colour: ColourData,
    pub light_hash: u8,
    pub vol_outer_intensity: f32,
    pub corona_size: f32,
    pub vol_outer_exponent: f32,
    pub light_fade_distance: u8,
    pub shadow_fade_distance: u8,
    pub specular_fade_distance: u8,
    pub volumetric_fade_distance: u8,
    pub shadow_near_clip: f32,
    pub corona_intensity: f32,
    pub corona_z_bias: f32,
    pub direction: VectorData,
    pub tangent: VectorData,
    pub cone_inner_angle: f32,
    pub cone_outer_angle: f32,
    pub extents: VectorData,
    #[pad_after(4)]
    pub projected_texture_key: u32,
}

/// `gtaDrawable`: the engine's drawable, then the lights and the collision bound the game adds.
#[record(size = 0xd0)]
pub struct GtaDrawable<T: DrawableTarget = Gen8> {
    pub base: RmcDrawable<T>,
    pub lights: Vec<CLightAttr>,
    #[note("null in every drawable shipped")]
    tint_data: Ptr<Opaque>,
    pub bound: Option<Box<PhBoundAny>>,
}

impl Resource for GtaDrawable<Gen8> {
    const EXT: &'static str = "ydr";
    const TARGET: Generation = Generation::Gen8;
}

#[cfg(feature = "gen9")]
impl Resource for GtaDrawable<Gen9> {
    const EXT: &'static str = "ydr";
    const TARGET: Generation = Generation::Gen9;
}

impl GtaDrawable {
    /// The Legacy drawable at `path`; an Enhanced one is refused by name, pointing at `or_io::open`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, crate::Error> {
        <Self as File>::open(path)
    }
}

/// A `gtaDrawable` is an `rmcDrawable`, and its fields read as the base's.
impl<T: DrawableTarget> std::ops::Deref for GtaDrawable<T> {
    type Target = RmcDrawable<T>;
    fn deref(&self) -> &RmcDrawable<T> {
        &self.base
    }
}

impl<T: DrawableTarget> std::ops::DerefMut for GtaDrawable<T> {
    fn deref_mut(&mut self) -> &mut RmcDrawable<T> {
        &mut self.base
    }
}

/// A count a drawable's mesh states about a run stored elsewhere that disagrees with the run.
///
/// ⚠ A reader addresses by whichever of the count and the run is smaller;
/// the game trusts the count and reads off the end of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeshDefect {
    /// `vertices_count` exceeds the vertices the buffer states.
    VertexCountPastBuffer,
    /// `vertex_stride` is not the buffer's own stride.
    VertexStrideDisagrees,
    /// `indices_count` exceeds the indices the buffer states.
    IndexCountPastBuffer,
    /// A triangle list's `indices_count` is not `primitive_count * 3`.
    PrimitiveCountDisagrees,
    /// An index names a vertex the run does not have.
    IndexPastVertexRun,
    /// `matrix_count` states a palette the geometry does not point at.
    PaletteCountDisagrees,
    /// A palette slot names a bone the skeleton does not have.
    PaletteNamesNoBone,
    /// `aabbs` is not as long as the geometry count implies.
    AabbSlotsDisagree,
    /// `shader_indices` is not one per geometry.
    ShaderIndexCountDisagrees,
    /// A shader index names a material the group does not have.
    ShaderIndexPastGroup,
}

impl MeshDefect {
    pub const ALL: [Self; 10] = [
        Self::VertexCountPastBuffer,
        Self::VertexStrideDisagrees,
        Self::IndexCountPastBuffer,
        Self::PrimitiveCountDisagrees,
        Self::IndexPastVertexRun,
        Self::PaletteCountDisagrees,
        Self::PaletteNamesNoBone,
        Self::AabbSlotsDisagree,
        Self::ShaderIndexCountDisagrees,
        Self::ShaderIndexPastGroup,
    ];

    /// How a refusal names it.
    pub fn label(self) -> &'static str {
        match self {
            Self::VertexCountPastBuffer => "vertices_count past the buffer",
            Self::VertexStrideDisagrees => "vertex_stride is not the buffer's",
            Self::IndexCountPastBuffer => "indices_count past the buffer",
            Self::PrimitiveCountDisagrees => "indices_count is not primitive_count * 3",
            Self::IndexPastVertexRun => "an index past the vertex run",
            Self::PaletteCountDisagrees => "matrix_count with no palette",
            Self::PaletteNamesNoBone => "a palette slot names no bone",
            Self::AabbSlotsDisagree => "aabbs is the wrong length",
            Self::ShaderIndexCountDisagrees => "shader_indices is not one per geometry",
            Self::ShaderIndexPastGroup => "a shader index past the group",
        }
    }
}

/// One mesh defect, where it is, and what the counts say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub defect: MeshDefect,
    /// `level0/model2/geometry1`, or as much of it as applies.
    pub at: String,
    pub detail: String,
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({})", self.at, self.defect.label(), self.detail)
    }
}

/// Every mesh defect a drawable holds; a refusal spells the first eight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defects(pub Vec<Found>);

impl std::fmt::Display for Defects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const SPELLED: usize = 8;
        let spelled: Vec<String> = self.0.iter().take(SPELLED).map(|d| d.to_string()).collect();
        write!(
            f,
            "the mesh counts contradict the runs they describe, so the game would read off \
             the end of a buffer: {}",
            spelled.join("; ")
        )?;
        match self.0.len().saturating_sub(SPELLED) {
            0 => Ok(()),
            more => write!(f, " and {more} more"),
        }
    }
}

/// Which level of detail of a drawable a geometry is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LodSlot {
    /// One of the four, highest detail first.
    Level(usize),
    /// The level the drawable's `container` points at where it is not a level again.
    Container,
}

impl std::fmt::Display for LodSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Level(i) => write!(f, "level{i}"),
            Self::Container => f.write_str("container"),
        }
    }
}

/// A level of detail, read in place.
type LevelView<'a, T> = View<'a, Raw<RmcLod<T>>>;

/// Each level of detail a drawable holds, and its container where that is not a level again.
fn levels<'a, T: DrawableTarget>(
    drawable: &View<'a, Raw<RmcDrawable<T>>>,
) -> Result<Vec<(LodSlot, LevelView<'a, T>)>, ViewError> {
    let image = View::image(drawable);
    let mut out = Vec::new();
    let mut at = Vec::new();
    for (i, slot) in drawable.lod_group.lods.iter().enumerate() {
        if !slot.is_null() {
            at.push(slot.raw());
            out.push((LodSlot::Level(i), View::at_address(image, slot.raw())?));
        }
    }
    if !at.contains(&{ drawable.container }.raw()) {
        if let Some(container) = drawable.container()? {
            out.push((LodSlot::Container, container));
        }
    }
    Ok(out)
}

/// Every mesh defect a drawable holds, each as `visit` sees it.
pub fn each_defect<T: DrawableTarget>(
    drawable: &View<'_, Raw<RmcDrawable<T>>>,
    visit: &mut dyn FnMut(Found),
) -> Result<(), ViewError> {
    let bones = match drawable.skeleton()? {
        Some(s) => s.bones()?.len(),
        None => 0,
    };
    let group = drawable.shader_group()?;
    let materials = match &group {
        Some(g) => Some(g.shaders()?.len()),
        None => None,
    };
    for (lod, level) in levels(drawable)? {
        let models = level.models()?;
        for m in 0..models.len() {
            let Some(model) = models.follow(m)? else {
                continue;
            };
            let at = format!("{lod}/model{m}");
            let geometries = model.geometries()?;
            let shader_indices = model.shader_indices()?;
            let mut found = |defect: MeshDefect, at: String, detail: String| {
                visit(Found { defect, at, detail })
            };
            if shader_indices.len() != geometries.len() {
                found(
                    MeshDefect::ShaderIndexCountDisagrees,
                    at.clone(),
                    format!(
                        "{} shader indices for {} geometries",
                        shader_indices.len(),
                        geometries.len()
                    ),
                );
            }
            let slots = crate::codec::mesh::aabb_slots(geometries.len());
            if model.aabbs()?.len() != slots {
                found(
                    MeshDefect::AabbSlotsDisagree,
                    at.clone(),
                    format!(
                        "{} boxes for {} geometries, where the rule implies {slots}",
                        model.aabbs()?.len(),
                        geometries.len()
                    ),
                );
            }
            // ⚠ A drawable with no shader group states no materials, and every index would be out of range of none.
            if let Some(materials) = materials {
                for (g, index) in shader_indices.iter().enumerate() {
                    if usize::from(*index) >= materials {
                        found(
                            MeshDefect::ShaderIndexPastGroup,
                            format!("{at}/geometry{g}"),
                            format!("shader index {index} of {materials} materials"),
                        );
                    }
                }
            }
            for g in 0..geometries.len() {
                let Some(geometry) = geometries.follow(g)? else {
                    continue;
                };
                geometry_defects(&format!("{at}/geometry{g}"), &geometry, bones, &mut found)?;
            }
        }
    }
    Ok(())
}

fn geometry_defects<T: DrawableTarget>(
    at: &str,
    geometry: &View<'_, Raw<GrmGeometry<T>>>,
    bones: usize,
    found: &mut dyn FnMut(MeshDefect, String, String),
) -> Result<(), ViewError> {
    let vertices = usize::from(geometry.vertices_count);
    if let Some(vb) = geometry.vertex_buffer()? {
        let (holds, stride) = T::stated_vertices(View::get(&vb));
        if vertices > holds {
            found(
                MeshDefect::VertexCountPastBuffer,
                at.to_string(),
                format!("states {vertices} vertices and the buffer {holds}"),
            );
        }
        // ⚠ `vertex_stride` is a byte, so a buffer wider than 255 cannot state its stride here.
        let states = usize::from(geometry.vertex_stride);
        if stride <= 255 && states != stride {
            found(
                MeshDefect::VertexStrideDisagrees,
                at.to_string(),
                format!("the geometry states {states} and the buffer {stride}"),
            );
        }
    }
    let indices = geometry.indices_count as usize;
    if let Some(ib) = geometry.index_buffer()? {
        let holds = T::stated_indices(View::get(&ib));
        if indices > holds {
            found(
                MeshDefect::IndexCountPastBuffer,
                at.to_string(),
                format!("states {indices} indices and the buffer {holds}"),
            );
        }
        // ⚠ Bounded by the stated count, which is what the game walks.
        let run = T::stated_index_run(&ib)?;
        let walked = &run[..indices.min(holds).min(run.len())];
        if let Some((i, index)) =
            (walked.iter().enumerate()).find(|(_, v)| usize::from(**v) >= vertices)
        {
            found(
                MeshDefect::IndexPastVertexRun,
                at.to_string(),
                format!("index {i} is {index} of {vertices} vertices"),
            );
        }
    }
    let primitives = geometry.primitive_count as usize;
    if geometry.primitive_type == GrmGeometry::<T>::TRIS && indices != primitives * 3 {
        found(
            MeshDefect::PrimitiveCountDisagrees,
            at.to_string(),
            format!("{indices} indices and {primitives} primitives"),
        );
    }
    let stated = usize::from(geometry.matrix_count);
    if stated != 0 && { geometry.matrix_palette }.is_null() {
        found(
            MeshDefect::PaletteCountDisagrees,
            at.to_string(),
            format!("matrix_count {stated} and no palette"),
        );
    }
    // ⚠ Only against a skeleton this drawable carries: a ped component's palette names bones in another file.
    if bones > 0 {
        let palette = geometry.matrix_palette()?;
        if let Some((slot, bone)) =
            (palette.iter().enumerate()).find(|(_, b)| usize::from(**b) >= bones)
        {
            found(
                MeshDefect::PaletteNamesNoBone,
                at.to_string(),
                format!("palette slot {slot} names bone {bone} of {bones}"),
            );
        }
    }
    Ok(())
}

/// Which buffer of a geometry's two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BufferKind {
    Vertex,
    Index,
}

impl std::fmt::Display for BufferKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Vertex => "vertex buffer",
            Self::Index => "index buffer",
        })
    }
}

/// Where a geometry is: its drawable in the file, its level of detail, its model and its place there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The drawable's index among those its file holds, in the order its validation visits them.
    pub drawable: usize,
    pub lod: LodSlot,
    pub model: usize,
    pub geometry: usize,
}

impl std::fmt::Display for Site {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "drawable {} {}/model{}/geometry{}",
            self.drawable, self.lod, self.model, self.geometry
        )
    }
}

/// A buffer more than one geometry points at.
///
/// ⚠ The game's fixup walk has no visited set: it rebases a buffer once per
/// geometry pointing at it, and the second finds an address that is neither
/// virtual nor physical and fails the load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aliased {
    /// Where the file holds the buffer.
    pub source_address: u64,
    pub kind: BufferKind,
    pub sites: Vec<Site>,
}

impl std::fmt::Display for Aliased {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sites: Vec<String> = self.sites.iter().map(Site::to_string).collect();
        write!(
            f,
            "{} at {:#X} is pointed at from {} geometries ({})",
            self.kind,
            self.source_address,
            self.sites.len(),
            sites.join("; ")
        )
    }
}

/// Every buffer more than one geometry in a file points at.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{} buffer(s) are pointed at by more than one geometry, which the game's fixup walk would \
     resolve twice and fail on: {}",
    .0.len(),
    .0.iter().map(Aliased::to_string).collect::<Vec<_>>().join("; ")
)]
pub struct Aliasing(pub Vec<Aliased>);

/// The buffers every geometry of a file's drawables points at, by address.
#[derive(Debug, Default)]
pub struct Buffers {
    seen: std::collections::BTreeMap<(u64, BufferKind), Vec<Site>>,
    /// Where each drawable visited so far is, in visiting order.
    drawables: Vec<usize>,
}

impl Buffers {
    /// Notes the buffers of each geometry of the file's next drawable.
    pub fn visit<T: DrawableTarget>(
        &mut self,
        drawable: &View<'_, Raw<RmcDrawable<T>>>,
    ) -> Result<(), ViewError> {
        let index = self.drawables.len();
        let at = View::offset(drawable);
        // ⚠ A fragment's cloth points at its common drawable, which is then visited twice and is one drawable.
        let again = self.drawables.contains(&at);
        self.drawables.push(at);
        if again {
            return Ok(());
        }
        for (lod, level) in levels(drawable)? {
            let models = level.models()?;
            for m in 0..models.len() {
                let Some(model) = models.follow(m)? else {
                    continue;
                };
                let geometries = model.geometries()?;
                for g in 0..geometries.len() {
                    let Some(geometry) = geometries.follow(g)? else {
                        continue;
                    };
                    let site = Site {
                        drawable: index,
                        lod,
                        model: m,
                        geometry: g,
                    };
                    for (kind, address) in [
                        (BufferKind::Vertex, { geometry.vertex_buffer }.raw()),
                        (BufferKind::Index, { geometry.index_buffer }.raw()),
                    ] {
                        if address != 0 {
                            let sites = self.seen.entry((address, kind)).or_default();
                            sites.push(site.clone());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Every buffer more than one geometry points at, by address.
    pub fn aliased(&self) -> Vec<Aliased> {
        (self.seen.iter())
            .filter(|(_, sites)| sites.len() > 1)
            .map(|((address, kind), sites)| Aliased {
                source_address: *address,
                kind: *kind,
                sites: sites.clone(),
            })
            .collect()
    }

    /// Refuses a file any of whose buffers more than one geometry points at.
    pub fn refuse(&self) -> Result<(), Aliasing> {
        match self.aliased() {
            aliased if aliased.is_empty() => Ok(()),
            aliased => Err(Aliasing(aliased)),
        }
    }
}

/// The levels of detail a drawable draws only through buffers a more detailed level owns, which removing un-aliases.
///
/// ⚠ A level holding one buffer of its own is kept, even though the alias
/// beside it stays: removing it would lose geometry.
pub fn aliased_lods<T: DrawableTarget>(
    drawable: &View<'_, Raw<RmcDrawable<T>>>,
) -> Result<Vec<LodSlot>, ViewError> {
    let mut buffers = Buffers::default();
    buffers.visit(drawable)?;
    let owner = |sites: &[Site]| sites.iter().map(|s| s.lod).min();
    let mut removable: std::collections::BTreeMap<LodSlot, bool> = Default::default();
    for sites in buffers.seen.values() {
        let winner = owner(sites);
        for site in sites {
            let keep = sites.len() < 2 || Some(site.lod) == winner;
            *removable.entry(site.lod).or_insert(true) &= !keep;
        }
    }
    Ok((removable.into_iter())
        .filter_map(|(lod, removable)| removable.then_some(lod))
        .collect())
}

impl<T: DrawableTarget> RmcDrawable<T> {
    /// Empties each level named; the container, where named, stops being carried.
    pub fn remove_lods(&mut self, slots: &[LodSlot]) {
        for slot in slots {
            match *slot {
                LodSlot::Level(i) => {
                    if let Some(level) = self.lod_group.lods.get_mut(i) {
                        *level = RmcLod::default().into();
                    }
                }
                LodSlot::Container => self.container = Shared::Own(None),
            }
        }
    }
}

/// A drawable the game would misread, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error("{0}")]
    Mesh(Defects),
    #[error("{0}")]
    Aliased(#[from] Aliasing),
    #[error("an embedded texture: {0}")]
    Texture(#[from] super::texture_dict::Broken),
    #[error("the collision bound: {0}")]
    Bounds(#[from] super::bounds::Broken),
    #[error("{0}")]
    View(#[from] ViewError),
}

/// Every count a drawable's mesh states about a run stored elsewhere, and every embedded texture.
pub fn check_drawable<T: DrawableTarget>(
    drawable: &View<'_, Raw<RmcDrawable<T>>>,
    target: Generation,
) -> Result<(), Broken> {
    if let Some(g) = drawable.shader_group()? {
        if let Some(dict) = g.texture_dictionary()? {
            super::texture_dict::validate(View::image(&dict), View::offset(&dict), target)?;
        }
    }
    let mut found = Vec::new();
    each_defect(drawable, &mut |f| found.push(f))?;
    if found.is_empty() {
        return Ok(());
    }
    Err(Broken::Mesh(Defects(found)))
}

/// A game drawable: its drawable, and its collision bound.
fn check_game_drawable<T: DrawableTarget>(
    drawable: &View<'_, Raw<GtaDrawable<T>>>,
    target: Generation,
) -> Result<(), Broken> {
    check_drawable(&drawable.base(), target)?;
    if let Some(bound) = drawable.bound()? {
        let bound = View::<Raw<super::bounds::PhBound>>::at_offset(
            View::image(&bound),
            View::offset(&bound),
        )?;
        super::bounds::check_bound(bound, "")?;
    }
    Ok(())
}

fn check_ydr<T: DrawableTarget>(
    image: Image<'_>,
    root: usize,
    target: Generation,
    buffers: &mut Buffers,
) -> Result<(), Broken> {
    let drawable = View::<Raw<GtaDrawable<T>>>::at_offset(image, root)?;
    check_game_drawable(&drawable, target)?;
    Ok(buffers.visit(&drawable.base())?)
}

/// A `.ydr` drawable's root, checked for what the game assumes and never checks.
pub fn validate_game_drawable(
    image: Image<'_>,
    root: usize,
    target: Generation,
) -> Result<(), Broken> {
    #[cfg(feature = "gen9")]
    if target == Generation::Gen9 {
        let drawable = View::<Raw<GtaDrawable<Gen9>>>::at_offset(image, root)?;
        return check_game_drawable(&drawable, target);
    }
    let drawable = View::<Raw<GtaDrawable<Gen8>>>::at_offset(image, root)?;
    check_game_drawable(&drawable, target)
}

/// A `.ydr`'s root, checked for what the game assumes and never checks, and its buffers.
pub fn validate(image: Image<'_>, root: usize, target: Generation) -> Result<(), Broken> {
    Ok(buffers(image, root, target)?.refuse()?)
}

/// A `.ydr`'s buffers, once its drawable passes its checks.
pub fn buffers(image: Image<'_>, root: usize, target: Generation) -> Result<Buffers, Broken> {
    let mut buffers = Buffers::default();
    #[cfg(feature = "gen9")]
    if target == Generation::Gen9 {
        check_ydr::<Gen9>(image, root, target, &mut buffers)?;
        return Ok(buffers);
    }
    check_ydr::<Gen8>(image, root, target, &mut buffers)?;
    Ok(buffers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gta5::gen8::GrcInstanceDataEntry;

    fn named(hash: u32) -> Box<gen8::GrcInstanceData> {
        Box::new(gen8::GrcInstanceData {
            basis_hash: Hash32::from_raw(hash),
            ..Default::default()
        })
    }

    /// ⚠ A model names its shaders by position, so a move renumbers every model.
    #[test]
    fn moving_a_shader_follows_every_model_that_named_one() {
        let mut d = RmcDrawable::<Gen8> {
            shader_group: Some(Box::new(GrmShaderGroup {
                shaders: vec![named(0xa), named(0xb), named(0xc)],
                ..Default::default()
            })),
            ..Default::default()
        };
        let model = GrmModel {
            shader_indices: vec![0, 1, 2, 2],
            ..Default::default()
        };
        d.lod_group.lods[0] = NullIsEmpty::new(RmcLod {
            models: vec![Box::new(model)],
            ..Default::default()
        });
        d.move_shader(0, 2);
        let group = d.shader_group.as_deref().unwrap();
        let hashes: Vec<u32> = group.shaders.iter().map(|s| s.basis_hash.get()).collect();
        assert_eq!(hashes, [0xb, 0xc, 0xa]);
        assert_eq!(d.lod_group.lods[0].models[0].shader_indices, [2, 0, 1, 1]);
        d.move_shader(2, 0);
        assert_eq!(d.lod_group.lods[0].models[0].shader_indices, [0, 1, 2, 2]);
    }

    /// ⚠ A sampler appended after the constants would be read by the engine as a constant.
    #[test]
    fn a_texture_joins_the_leading_textures_and_rows_the_tail() {
        let mut m = gen8::GrcInstanceData::default();
        let rows = |hash| GrcInstanceDataEntry {
            name_hash: Hash32::from_raw(hash),
            data: EntryData::Float(vec![Vec4::default()]),
            ..Default::default()
        };
        let texture = |hash| GrcInstanceDataEntry {
            name_hash: Hash32::from_raw(hash),
            ..Default::default()
        };
        assert_eq!(m.insert_entry(rows(1)), 0);
        assert_eq!(m.insert_entry(texture(2)), 0);
        assert_eq!(m.insert_entry(rows(3)), 2);
        assert_eq!(m.insert_entry(texture(4)), 1);
        let order: Vec<u32> = m.entries.iter().map(|e| e.name_hash.get()).collect();
        assert_eq!(order, [2, 4, 1, 3]);
        assert!(m.entry(4).is_some() && m.entry(5).is_none());
    }
}
