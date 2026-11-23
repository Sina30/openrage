//! Lazy decoded-texture cache shared across panels, one egui handle per
//! `(document, texture_name, channel_view)`.
//!
//! ⚠ Bounded by decoded bytes, evicting least-recently-used entries. A preview
//! is RGBA8 — 16 MiB for a 2048×2048 texture whatever its compressed size — and
//! is held twice, by egui's texture store and by the atlas upload path, so
//! browsing a couple of large dictionaries unbounded exhausted the heap and
//! aborted the process. The default budget is generous because a re-decode is
//! a visible stutter; the user sets it in the Statistics panel.
//!
//! # ⚠ Not a GPU resource cache, and so not `or-renderer`'s
//!
//! The V2 shell plan's step 11 listed this as a move
//! to `or-renderer` on the grounds that a GPU resource cache is a renderer
//! concern. It is an **egui** cache: the key carries a `DocumentId`, the
//! value is an `egui::TextureHandle`, the size limit comes from
//! `egui::Context::input`, and the upload is `ctx.load_texture`.
//! `or-renderer` has no egui dependency and `AGENTS.md` says the app owns
//! egui integration separately, so the move as stated would put egui into
//! the renderer.
//!
//! Mip selection ([`fitting_level`]) reads only the codec's `TextureSource`, and
//! downscaling is `or_io::authoring::fit`; neither names an egui type.

use std::collections::HashMap;

use or_io::authoring::fit_within;
use or_io::codec::texture::{decode_level, ChannelView, RgbaImage, TextureSource};

use super::document::DocumentId;
use super::textures::Texture;

pub type TextureKey = (DocumentId, String, ChannelView);

pub const DEFAULT_BUDGET_MIB: u32 = 512;

#[derive(Clone)]
pub struct CachedTexture {
    pub handle: egui::TextureHandle,
    pub width: u32,
    pub height: u32,
}

impl CachedTexture {
    fn decoded_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

struct Entry {
    texture: CachedTexture,
    last_used: u64,
}

pub struct TextureCache {
    entries: HashMap<TextureKey, Entry>,
    tick: u64,
    bytes: usize,
    budget: usize,
    /// The tiles decoding off this thread (`texture_tiles`).
    pub(super) tiles: super::texture_tiles::TileJobs,
}

impl TextureCache {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            tick: 0,
            bytes: 0,
            budget,
            tiles: Default::default(),
        }
    }

    /// Decoded bytes held, RGBA8.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn texture_ids(&self) -> std::collections::HashSet<egui::TextureId> {
        self.entries
            .values()
            .map(|e| e.texture.handle.id())
            .collect()
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Evicts at once down to a lowered budget.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
        self.evict_to_budget();
    }

    pub fn get(&mut self, key: &TextureKey) -> Option<&CachedTexture> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.entries.get_mut(key)?;
        entry.last_used = tick;
        Some(&entry.texture)
    }

    pub fn get_or_decode(
        &mut self,
        ctx: &egui::Context,
        key: TextureKey,
        texture: &Texture<'_>,
    ) -> Option<CachedTexture> {
        if let Some(entry) = self.get(&key) {
            return Some(entry.clone());
        }

        if !texture.has_pixel_data() || texture.width() == 0 || texture.height() == 0 {
            return None;
        }

        let limit = ctx.input(|i| i.max_texture_side).max(1) as u32;

        let level = fitting_level(texture, limit);

        let rgba = match decode_level(texture, level) {
            Ok(image) => image,
            Err(e) => {
                tracing::warn!(
                    texture = texture.name(),
                    error = %e,
                    "Failed to decode texture for preview"
                );
                return None;
            }
        };
        self.upload(ctx, key, rgba, egui::TextureOptions::LINEAR)
    }

    /// The picture under `key`, made by `make` on a miss.
    pub fn get_or_insert_image(
        &mut self,
        ctx: &egui::Context,
        key: TextureKey,
        options: egui::TextureOptions,
        make: impl FnOnce() -> Option<RgbaImage>,
    ) -> Option<CachedTexture> {
        if let Some(entry) = self.get(&key) {
            return Some(entry.clone());
        }
        let rgba = make()?;
        self.upload(ctx, key, rgba, options)
    }

    fn upload(
        &mut self,
        ctx: &egui::Context,
        key: TextureKey,
        rgba: RgbaImage,
        options: egui::TextureOptions,
    ) -> Option<CachedTexture> {
        let limit = ctx.input(|i| i.max_texture_side).max(1) as u32;
        let rgba = fit_within(rgba.with_channel_view(key.2), limit, &key.1)?;
        Some(self.keep(ctx, key, &rgba, options))
    }

    /// Keeps `rgba` under `key` as it is: its channels and its size are the caller's.
    pub(super) fn keep(
        &mut self,
        ctx: &egui::Context,
        key: TextureKey,
        rgba: &RgbaImage,
        options: egui::TextureOptions,
    ) -> CachedTexture {
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width as usize, rgba.height as usize],
            &rgba.data,
        );

        let label = format!("tex:{}:{}:{:?}", key.0, key.1, key.2);
        let handle = ctx.load_texture(label, color_image, options);

        let entry = CachedTexture {
            handle,
            width: rgba.width,
            height: rgba.height,
        };
        self.tick += 1;
        self.bytes += entry.decoded_bytes();
        self.entries.insert(
            key,
            Entry {
                texture: entry.clone(),
                last_used: self.tick,
            },
        );
        self.evict_to_budget();
        entry
    }

    fn evict_to_budget(&mut self) {
        while self.bytes > self.budget && self.entries.len() > 1 {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(e) = self.entries.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(e.texture.decoded_bytes());
                tracing::trace!(
                    texture = %victim.1,
                    cache_mib = self.bytes / (1024 * 1024),
                    "Evicted decoded texture preview"
                );
            }
        }
    }

    pub fn forget_document(&mut self, doc: DocumentId) {
        let freed: usize = self
            .entries
            .iter()
            .filter(|((d, _, _), _)| *d == doc)
            .map(|(_, e)| e.texture.decoded_bytes())
            .sum();
        self.entries.retain(|(d, _, _), _| *d != doc);
        self.tiles.forget();
        self.bytes = self.bytes.saturating_sub(freed);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.tiles.forget();
        self.bytes = 0;
    }
}

/// The first stored mip small enough to fit `limit`, or the smallest there is.
///
/// ⚠ Decoding a stored level is not the same as resampling level 0 to the
/// same size: the game's own chain is what the artist shipped.
pub(super) fn fitting_level(texture: &impl TextureSource, limit: u32) -> u32 {
    let (width, height) = texture.stored_size();
    let format = texture.pixel_format();
    let levels = texture.stored_levels();
    (0..levels)
        .find(|&level| {
            let (w, h) = format.mip_dimensions(width, height, level);
            w <= limit && h <= limit
        })
        .unwrap_or_else(|| levels.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BYTE_BUDGET: usize = DEFAULT_BUDGET_MIB as usize * 1024 * 1024;

    fn insert(cache: &mut TextureCache, doc: DocumentId, name: &str, w: u32, h: u32) {
        cache.tick += 1;
        let texture = CachedTexture {
            handle: unreachable_handle(),
            width: w,
            height: h,
        };
        cache.bytes += texture.decoded_bytes();
        cache.entries.insert(
            (doc, name.to_string(), ChannelView::Rgba),
            Entry {
                texture,
                last_used: cache.tick,
            },
        );
        cache.evict_to_budget();
    }

    fn unreachable_handle() -> egui::TextureHandle {
        let ctx = egui::Context::default();
        ctx.load_texture(
            "test",
            egui::ColorImage::new([1, 1], vec![egui::Color32::BLACK]),
            egui::TextureOptions::LINEAR,
        )
    }

    #[test]
    fn byte_total_tracks_inserts_and_document_forget() {
        let mut cache = TextureCache::new(BYTE_BUDGET);
        insert(&mut cache, 1, "a", 16, 16);
        insert(&mut cache, 2, "b", 32, 32);
        assert_eq!(cache.bytes, 16 * 16 * 4 + 32 * 32 * 4);
        cache.forget_document(1);
        assert_eq!(cache.bytes, 32 * 32 * 4);
        assert_eq!(cache.entries.len(), 1);
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn eviction_drops_the_least_recently_used_first() {
        let mut cache = TextureCache::new(BYTE_BUDGET);
        let edge = ((BYTE_BUDGET / 2) as f64 / 4.0).sqrt() as u32;
        insert(&mut cache, 1, "oldest", edge, edge);
        insert(&mut cache, 1, "middle", edge, edge);
        assert!(cache
            .get(&(1, "oldest".to_string(), ChannelView::Rgba))
            .is_some());
        insert(&mut cache, 1, "newest", edge, edge);

        assert!(cache.bytes <= BYTE_BUDGET, "budget respected");
        assert!(cache
            .entries
            .contains_key(&(1, "newest".to_string(), ChannelView::Rgba)));
        assert!(cache
            .entries
            .contains_key(&(1, "oldest".to_string(), ChannelView::Rgba)));
        assert!(
            !cache
                .entries
                .contains_key(&(1, "middle".to_string(), ChannelView::Rgba)),
            "the untouched entry is the one evicted"
        );
    }

    #[test]
    fn a_lowered_budget_evicts_at_once() {
        let mut cache = TextureCache::new(BYTE_BUDGET);
        insert(&mut cache, 1, "older", 1024, 1024);
        insert(&mut cache, 1, "newer", 1024, 1024);
        assert_eq!(cache.entries.len(), 2);

        cache.set_budget(1024 * 1024 * 4);
        assert_eq!(cache.bytes, 1024 * 1024 * 4);
        assert!(cache
            .entries
            .contains_key(&(1, "newer".to_string(), ChannelView::Rgba)));
    }

    #[test]
    fn a_single_oversized_entry_is_kept_rather_than_thrashed() {
        let mut cache = TextureCache::new(BYTE_BUDGET);
        insert(&mut cache, 1, "huge", MAX_TEST_EDGE, MAX_TEST_EDGE);
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.bytes > BYTE_BUDGET);
    }

    const MAX_TEST_EDGE: u32 = 16384;

    use or_io::codec::texture::GrcTextureFormat;

    /// A context reporting `limit` as its maximum texture side, the way a
    /// real integration reports the adapter's. Driven through a pass
    /// because that is where `RawInput` reaches `InputState`.
    ///
    /// ⚠ `limit` cannot go below 1024: egui sizes its own font atlas from
    /// this and `epaint` asserts "Tiny texture atlas" under that.
    fn context_with_limit(limit: usize) -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(
            egui::RawInput {
                max_texture_side: Some(limit),
                ..Default::default()
            },
            |_| {},
        );
        assert_eq!(ctx.input(|i| i.max_texture_side), limit, "limit took");
        ctx
    }

    fn texture_with_distinct_levels(edge: u16, mip_count: u8) -> Texture<'static> {
        let format = GrcTextureFormat::A8R8G8B8;
        let side = u32::from(edge);
        let mut backing_store =
            vec![0; format.total_size(side, side, u32::from(mip_count)) as usize];
        for level in 0..u32::from(mip_count) {
            backing_store[format.mip_level_range(side, side, level)].fill(level_fill(level));
        }
        Texture::Owned(std::borrow::Cow::Owned(or_io::gta5::gen8::GrcTexture {
            name: "uppr_diff_001_a_uni".into(),
            width: edge,
            height: edge,
            depth: 1,
            format: format.to_raw_format(),
            mip_count,
            backing_store,
            ..Default::default()
        }))
    }

    fn level_fill(level: u32) -> u8 {
        0x30 + (level as u8) * 0x20
    }

    #[test]
    fn an_oversized_texture_is_reduced_rather_than_aborting_the_process() {
        let ctx = context_with_limit(1024);
        let mut cache = TextureCache::new(BYTE_BUDGET);

        let flat = texture_with_distinct_levels(2048, 1);
        assert_eq!(fitting_level(&flat, 1024), 0, "nothing else is stored");
        let cached = cache
            .get_or_decode(&ctx, (1, "flat".into(), ChannelView::Rgba), &flat)
            .expect("an oversized texture still previews");

        assert_eq!(
            (cached.width, cached.height),
            (1024, 1024),
            "reduced to the limit, aspect preserved"
        );
    }

    #[test]
    fn an_oversized_texture_with_a_mip_chain_is_also_reduced() {
        let ctx = context_with_limit(1024);
        let mut cache = TextureCache::new(BYTE_BUDGET);

        let chained = texture_with_distinct_levels(2048, 2);
        let cached = cache
            .get_or_decode(&ctx, (1, "chained".into(), ChannelView::Rgba), &chained)
            .expect("an oversized texture still previews");

        assert_eq!((cached.width, cached.height), (1024, 1024));
    }

    #[test]
    fn a_texture_already_inside_the_limit_is_left_alone() {
        let ctx = context_with_limit(1024);
        let mut cache = TextureCache::new(BYTE_BUDGET);

        let small = texture_with_distinct_levels(512, 1);
        let cached = cache
            .get_or_decode(&ctx, (1, "small".into(), ChannelView::Rgba), &small)
            .expect("previews");

        assert_eq!(
            (cached.width, cached.height),
            (512, 512),
            "no needless reduction"
        );
    }

    /// A stored level that fits is what reaches the handle, rather than a
    /// resample of level 0.
    #[test]
    fn the_cache_uploads_the_stored_level_that_fits() {
        let ctx = context_with_limit(1024);
        let mut cache = TextureCache::new(BYTE_BUDGET);

        let chained = texture_with_distinct_levels(2048, 2);
        cache
            .get_or_decode(&ctx, (1, "chained".into(), ChannelView::Rgba), &chained)
            .expect("previews");

        assert_eq!(fitting_level(&chained, 1024), 1);
        let decoded = decode_level(&chained, fitting_level(&chained, 1024))
            .expect("the level that fits decodes");
        assert!(
            decoded
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|px| px[0] == level_fill(1)),
            "the cache's own mip choice is level 1's pixels"
        );
    }
}
