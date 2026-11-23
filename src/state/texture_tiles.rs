//! A tile's small picture of a texture, decoded off the UI thread and kept in the texture cache
//! apart from the full preview.
//!
//! ⚠ A tile that decoded on the UI thread froze the window as a scroll brought tiles into view:
//! a 2048×2048 texture is tens of milliseconds, and one with no mips a second.

use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use or_io::authoring::fit_within;
use or_io::codec::texture::{decode_level, ChannelView, RgbaImage};

use super::texture_cache::{fitting_level, CachedTexture, TextureCache, TextureKey};
use super::textures::Texture;

/// What a tile draws of a texture.
pub enum Tile {
    Ready(CachedTexture),
    /// Being decoded; the worker asks for a repaint when it is done.
    Waiting,
    /// It has no pixels, or they did not decode.
    None,
}

/// How many tiles decode at once, and how many may wait.
const RUNNING: usize = 4;
const WAITING: usize = 256;

struct Job {
    key: TextureKey,
    texture: Texture<'static>,
    side: u32,
}

/// The tiles being decoded, newest asked first.
pub struct TileJobs {
    tx: Sender<(TextureKey, u64, Option<RgbaImage>)>,
    /// ⚠ Behind a lock only because the app's state is shared across threads; one thread reads it.
    rx: Mutex<Receiver<(TextureKey, u64, Option<RgbaImage>)>>,
    waiting: VecDeque<Job>,
    asked: HashSet<TextureKey>,
    failed: HashSet<TextureKey>,
    running: usize,
    /// Counts the times the cache forgot a document: a tile decoded from before is of bytes since replaced.
    epoch: u64,
}

impl Default for TileJobs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx: Mutex::new(rx),
            waiting: VecDeque::new(),
            asked: HashSet::new(),
            failed: HashSet::new(),
            running: 0,
            epoch: 0,
        }
    }
}

impl TileJobs {
    /// Every tile asked for so far is of a document as it was: none is kept when it arrives.
    pub(super) fn forget(&mut self) {
        self.epoch += 1;
        self.waiting.clear();
        self.asked.clear();
        self.failed.clear();
    }

    fn start(&mut self, ctx: &egui::Context) {
        while self.running < RUNNING {
            // ⚠ The tiles asked for last are the ones on screen.
            let Some(job) = self.waiting.pop_back() else {
                return;
            };
            self.running += 1;
            let (tx, ctx, epoch) = (self.tx.clone(), ctx.clone(), self.epoch);
            std::thread::spawn(move || {
                let picture = decode(&job.texture, job.key.2, job.side);
                let _ = tx.send((job.key, epoch, picture));
                ctx.request_repaint();
            });
        }
    }
}

/// The key a tile's picture is kept under, which no texture's name is: a name holds no unit separator.
fn tile_key((doc, name, view): &TextureKey) -> TextureKey {
    (*doc, format!("{name}\u{1f}tile"), *view)
}

/// `texture` as a tile shows it: the stored level that fits `side`, reduced to it when none does.
fn decode(texture: &Texture<'_>, view: ChannelView, side: u32) -> Option<RgbaImage> {
    let level = fitting_level(texture, side);
    match decode_level(texture, level) {
        Ok(image) => fit_within(image.with_channel_view(view), side, texture.name()),
        Err(e) => {
            tracing::warn!(texture = texture.name(), error = %e, "Failed to decode a texture for its tile");
            None
        }
    }
}

impl TextureCache {
    /// The tile of the texture under `key`, or `None` when it is still to be asked for with [`Self::ask_tile`].
    pub fn tile(&mut self, ctx: &egui::Context, key: &TextureKey) -> Option<Tile> {
        self.take_in_tiles(ctx);
        let kept = tile_key(key);
        if let Some(ready) = self.get(&kept) {
            return Some(Tile::Ready(ready.clone()));
        }
        if self.tiles.failed.contains(key) {
            return Some(Tile::None);
        }
        self.tiles.asked.contains(key).then_some(Tile::Waiting)
    }

    /// Decode `texture`'s tile off this thread, no side over `side`; `texture` is `None` when the tile's texture is not held.
    pub fn ask_tile(
        &mut self,
        ctx: &egui::Context,
        key: TextureKey,
        side: u32,
        texture: Option<Texture<'static>>,
    ) -> Tile {
        let texture = texture.filter(|t| t.has_pixel_data() && t.width() != 0 && t.height() != 0);
        let Some(texture) = texture else {
            self.tiles.failed.insert(key);
            return Tile::None;
        };
        let jobs = &mut self.tiles;
        // The oldest asked were scrolled past, and are asked again if they come back.
        if jobs.waiting.len() >= WAITING {
            if let Some(dropped) = jobs.waiting.pop_front() {
                jobs.asked.remove(&dropped.key);
            }
        }
        jobs.asked.insert(key.clone());
        jobs.waiting.push_back(Job { key, texture, side });
        jobs.start(ctx);
        Tile::Waiting
    }

    /// Keep the tiles the workers finished.
    fn take_in_tiles(&mut self, ctx: &egui::Context) {
        let done: Vec<_> = match self.tiles.rx.lock() {
            Ok(rx) => rx.try_iter().collect(),
            Err(_) => Vec::new(),
        };
        for (key, epoch, picture) in done {
            self.tiles.running = self.tiles.running.saturating_sub(1);
            if epoch != self.tiles.epoch {
                continue;
            }
            self.tiles.asked.remove(&key);
            let options = egui::TextureOptions::LINEAR;
            let kept = picture.map(|rgba| self.keep(ctx, tile_key(&key), &rgba, options));
            if kept.is_none() {
                self.tiles.failed.insert(key);
            }
        }
        self.tiles.start(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::codec::texture::GrcTextureFormat;

    fn a_texture(edge: u16, mip_count: u8) -> Texture<'static> {
        let format = GrcTextureFormat::A8R8G8B8;
        let side = u32::from(edge);
        let backing_store =
            vec![0x80; format.total_size(side, side, u32::from(mip_count)) as usize];
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

    /// Frames until the tile under `key` is no longer waiting.
    fn settled(cache: &mut TextureCache, ctx: &egui::Context, key: &TextureKey) -> Tile {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match cache.tile(ctx, key) {
                Some(Tile::Waiting) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Some(tile) => return tile,
                None => panic!("the tile was never asked for"),
            }
        }
    }

    #[test]
    fn a_tile_waits_while_it_is_decoded_and_is_then_no_larger_than_asked() {
        let ctx = egui::Context::default();
        let mut cache = TextureCache::new(1 << 30);
        let key = (1, "uppr_diff_001_a_uni".to_string(), ChannelView::Rgba);
        assert!(cache.tile(&ctx, &key).is_none(), "nothing is asked for yet");

        for (mips, what) in [(4, "a stored level fits"), (1, "level 0 is reduced")] {
            cache.forget_document(1);
            let asked = cache.ask_tile(&ctx, key.clone(), 256, Some(a_texture(1024, mips)));
            assert!(
                matches!(asked, Tile::Waiting),
                "the UI thread decodes nothing"
            );
            let Tile::Ready(tile) = settled(&mut cache, &ctx, &key) else {
                panic!("the tile did not decode: {what}");
            };
            assert_eq!((tile.width, tile.height), (256, 256), "{what}");
        }
        assert!(
            cache.get(&key).is_none(),
            "a tile is not the full preview, which is decoded when it is asked for"
        );
    }

    #[test]
    fn a_tile_of_a_texture_without_pixels_is_none_and_is_not_asked_for_again() {
        let ctx = egui::Context::default();
        let mut cache = TextureCache::new(1 << 30);
        let key = (1, "gone".to_string(), ChannelView::Rgba);
        assert!(matches!(
            cache.ask_tile(&ctx, key.clone(), 256, None),
            Tile::None
        ));
        assert!(matches!(cache.tile(&ctx, &key), Some(Tile::None)));
    }

    #[test]
    fn a_tile_decoded_before_its_document_was_forgotten_is_not_kept() {
        let ctx = egui::Context::default();
        let mut cache = TextureCache::new(1 << 30);
        let key = (1, "uppr_diff_001_a_uni".to_string(), ChannelView::Rgba);
        cache.ask_tile(&ctx, key.clone(), 256, Some(a_texture(1024, 1)));
        cache.forget_document(1);
        // The worker finishes whatever the cache forgot; what it sends is of the bytes before.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while cache.tiles.running > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
            cache.take_in_tiles(&ctx);
        }
        assert!(
            cache.tile(&ctx, &key).is_none(),
            "the tile is asked for again"
        );
    }
}
