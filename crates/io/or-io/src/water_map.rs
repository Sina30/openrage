//! `waterheight.dat`: a grid of world tiles naming the rivers, lakes and pools
//! that lie under each, and those bodies' shapes and heights, stored big-endian.
//!
//! Every pad the file holds (after the header, between arrays, after a body's
//! run and a region's fourth lanes) is zero in every shipped file, so a save
//! writes zeros there and an open refuses anything else.

use crate::error::{IoError, Result};

const TAG: u32 = u32::from_be_bytes(*b"WMAP");
const HEADER_LEN: usize = 52;
/// The header's own record sizes: header, tile row, tile, reference, river point, river, lake box, lake, pool.
const RECORD_SIZES: [u8; 9] = [HEADER_LEN as u8, 4, 2, 2, 16, 48, 16, 48, 32];

/// The tile value that holds no reference.
pub const NO_REFS: u16 = 0xffff;

/// One water height file: the tile grid, its references and the bodies they name.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterMap {
    /// The game reads any version from 100 to 199.
    pub version: u32,
    /// World X of the grid's west edge.
    pub offset_x: f32,
    /// World Y of the grid's north edge; rows run south from it.
    pub offset_y: f32,
    pub tile_size_x: f32,
    pub tile_size_y: f32,
    pub num_tile_cols: u16,
    /// One per grid row, north first.
    pub tile_rows: Vec<WaterMapTileRow>,
    /// Each stored tile's first reference in [`WaterMap::ref_ids`], or [`NO_REFS`].
    pub tiles: Vec<u16>,
    pub ref_ids: Vec<WaterMapRefId>,
    /// X, Y, height and width.
    pub river_points: Vec<[f32; 4]>,
    pub rivers: Vec<WaterMapBody>,
    /// Centre X and Y, extent X and Y.
    pub lake_boxes: Vec<[f32; 4]>,
    pub lakes: Vec<WaterMapBody>,
    pub pools: Vec<WaterMapRegion>,
    /// An RGBA per river, after the data; only the game's development builds read them.
    pub river_colours: Vec<[u8; 4]>,
    pub lake_colours: Vec<[u8; 4]>,
    pub pool_colours: Vec<[u8; 4]>,
}

impl Default for WaterMap {
    fn default() -> Self {
        Self {
            version: 100,
            offset_x: 0.0,
            offset_y: 0.0,
            tile_size_x: 0.0,
            tile_size_y: 0.0,
            num_tile_cols: 0,
            tile_rows: Vec::new(),
            tiles: Vec::new(),
            ref_ids: Vec::new(),
            river_points: Vec::new(),
            rivers: Vec::new(),
            lake_boxes: Vec::new(),
            lakes: Vec::new(),
            pools: Vec::new(),
            river_colours: Vec::new(),
            lake_colours: Vec::new(),
            pool_colours: Vec::new(),
        }
    }
}

/// The run of a grid row's columns that is stored, and where its tiles start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaterMapTileRow {
    /// The run's first column.
    pub first: u8,
    pub count: u8,
    /// The run's first tile in [`WaterMap::tiles`].
    pub start: u16,
}

/// What a reference points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterMapRefType {
    Ocean,
    River,
    Lake,
    Pool,
}

/// A reference from a tile to a body, bit-packed: last of its tile 15, type 13..=14, then index and segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaterMapRefId(pub u16);

impl WaterMapRefId {
    pub fn kind(self) -> WaterMapRefType {
        match self.0 >> 13 & 3 {
            0 => WaterMapRefType::Ocean,
            1 => WaterMapRefType::River,
            2 => WaterMapRefType::Lake,
            _ => WaterMapRefType::Pool,
        }
    }

    /// The tile's references end with this one.
    pub fn is_last(self) -> bool {
        self.0 & 0x8000 != 0
    }
}

/// A body's box: centre X, Y and height, then extent X, Y and weight.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WaterMapRegion {
    pub centre_and_height: [f32; 3],
    pub extent_and_weight: [f32; 3],
}

/// A river or a lake: its region and its run of points or boxes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WaterMapBody {
    pub region: WaterMapRegion,
    pub count: u8,
    /// The run's first river point or lake box.
    pub start: u16,
}

impl WaterMap {
    /// Reads a whole file, refusing a record size, pad, length or range the bytes do not bear out.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let head = bytes
            .get(..8 + HEADER_LEN)
            .ok_or_else(|| refuse(format!("{} bytes is shorter than the header", bytes.len())))?;
        let u32_at = |at: usize| u32::from_be_bytes(head[at..at + 4].try_into().unwrap());
        let u16_at = |at: usize| u16::from_be_bytes([head[at], head[at + 1]]) as usize;
        let f32_at = |at: usize| f32::from_bits(u32_at(at));
        if u32_at(0) != TAG {
            return Err(refuse(format!(
                "the tag is {:#010x}, not big-endian `WMAP`",
                u32_at(0)
            )));
        }
        let version = u32_at(4);
        if version / 100 != 1 {
            return Err(refuse(format!("version {version} is not 1.xx")));
        }
        if head[0x32..0x3B] != RECORD_SIZES || head[0x3B] != 0 {
            return Err(refuse(format!(
                "the header states record sizes {:?} and pad {}, not {RECORD_SIZES:?} and 0",
                &head[0x32..0x3B],
                head[0x3B]
            )));
        }
        let data_len = u32_at(8) as usize;
        let data = bytes
            .get(8 + HEADER_LEN..)
            .and_then(|rest| rest.get(..data_len))
            .ok_or_else(|| {
                refuse(format!(
                    "the header states {data_len} bytes of data and {} follow it",
                    bytes.len() - 8 - HEADER_LEN
                ))
            })?;

        let mut r = Reader { bytes: data, at: 0 };
        let tile_rows = r.array(4, u16_at(0x1E), |r| {
            Ok(WaterMapTileRow {
                first: r.u8()?,
                count: r.u8()?,
                start: r.u16()?,
            })
        })?;
        let tiles = r.array(4, u32_at(0x20) as usize, Reader::u16)?;
        let ref_ids = r.array(4, u32_at(0x24) as usize, |r| r.u16().map(WaterMapRefId))?;
        let river_points = r.array(16, u16_at(0x28), Reader::vec4)?;
        let rivers = r.array(16, u16_at(0x2A), Reader::body)?;
        let lake_boxes = r.array(16, u16_at(0x2C), Reader::vec4)?;
        let lakes = r.array(16, u16_at(0x2E), Reader::body)?;
        let pools = r.array(16, u16_at(0x30), Reader::region)?;
        if r.at != data_len {
            return Err(refuse(format!(
                "the arrays end at {} of {data_len} bytes of data",
                r.at
            )));
        }

        let tail = &bytes[8 + HEADER_LEN + data_len..];
        let expected = 4 * (rivers.len() + lakes.len() + pools.len());
        if tail.len() != expected {
            return Err(refuse(format!(
                "{} bytes follow the data where one colour per body takes {expected}",
                tail.len()
            )));
        }
        let mut colours = tail
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| [c[0], c[1], c[2], c[3]]);
        let mut take = |n: usize| colours.by_ref().take(n).collect::<Vec<_>>();
        let (river_colours, lake_colours, pool_colours) =
            (take(rivers.len()), take(lakes.len()), take(pools.len()));

        let map = Self {
            version,
            offset_x: f32_at(0x0C),
            offset_y: f32_at(0x10),
            tile_size_x: f32_at(0x14),
            tile_size_y: f32_at(0x18),
            num_tile_cols: u16_at(0x1C) as u16,
            tile_rows,
            tiles,
            ref_ids,
            river_points,
            rivers,
            lake_boxes,
            lakes,
            pools,
            river_colours,
            lake_colours,
            pool_colours,
        };
        map.check()?;
        Ok(map)
    }

    /// Writes the file, refusing a range outside its arrays or a colour count that is not one per body.
    pub fn save(&self) -> Result<Vec<u8>> {
        self.check()?;
        let count = |n: usize, what: &str| {
            u16::try_from(n).map_err(|_| refuse(format!("{n} {what} passes u16")))
        };
        let mut data = Vec::new();
        let align = |data: &mut Vec<u8>, to: usize| data.resize(data.len().next_multiple_of(to), 0);
        for row in &self.tile_rows {
            data.extend_from_slice(&[row.first, row.count]);
            data.extend_from_slice(&row.start.to_be_bytes());
        }
        align(&mut data, 4);
        for tile in &self.tiles {
            data.extend_from_slice(&tile.to_be_bytes());
        }
        align(&mut data, 4);
        for id in &self.ref_ids {
            data.extend_from_slice(&id.0.to_be_bytes());
        }
        align(&mut data, 16);
        for point in &self.river_points {
            put_floats(&mut data, point);
        }
        for (bodies, boxes) in [(&self.rivers, None), (&self.lakes, Some(&self.lake_boxes))] {
            if let Some(boxes) = boxes {
                for b in boxes {
                    put_floats(&mut data, b);
                }
            }
            for body in bodies {
                put_region(&mut data, &body.region);
                data.extend_from_slice(&[body.count, 0]);
                data.extend_from_slice(&body.start.to_be_bytes());
                data.extend_from_slice(&[0; 12]);
            }
        }
        for pool in &self.pools {
            put_region(&mut data, pool);
        }

        let mut out = Vec::with_capacity(8 + HEADER_LEN + data.len());
        out.extend_from_slice(&TAG.to_be_bytes());
        out.extend_from_slice(&self.version.to_be_bytes());
        let data_len = u32::try_from(data.len())
            .map_err(|_| refuse(format!("{} bytes of data passes u32", data.len())))?;
        out.extend_from_slice(&data_len.to_be_bytes());
        put_floats(
            &mut out,
            &[
                self.offset_x,
                self.offset_y,
                self.tile_size_x,
                self.tile_size_y,
            ],
        );
        out.extend_from_slice(&self.num_tile_cols.to_be_bytes());
        out.extend_from_slice(&count(self.tile_rows.len(), "tile rows")?.to_be_bytes());
        for n in [self.tiles.len(), self.ref_ids.len()] {
            let n = u32::try_from(n).map_err(|_| refuse(format!("{n} tiles passes u32")))?;
            out.extend_from_slice(&n.to_be_bytes());
        }
        for (n, what) in [
            (self.river_points.len(), "river points"),
            (self.rivers.len(), "rivers"),
            (self.lake_boxes.len(), "lake boxes"),
            (self.lakes.len(), "lakes"),
            (self.pools.len(), "pools"),
        ] {
            out.extend_from_slice(&count(n, what)?.to_be_bytes());
        }
        out.extend_from_slice(&RECORD_SIZES);
        out.push(0);
        out.extend_from_slice(&data);
        for colour in self
            .river_colours
            .iter()
            .chain(&self.lake_colours)
            .chain(&self.pool_colours)
        {
            out.extend_from_slice(colour);
        }
        Ok(out)
    }

    /// The grid's row count, which is how many tile rows the file stores.
    pub fn num_tile_rows(&self) -> usize {
        self.tile_rows.len()
    }

    /// A tile's references, through the one flagged last; `None` for a tile that stores none.
    pub fn tile_refs(&self, col: u16, row: usize) -> Option<&[WaterMapRefId]> {
        let run = self.tile_rows.get(row)?;
        let i = usize::from(col).checked_sub(usize::from(run.first))?;
        if i >= usize::from(run.count) {
            return None;
        }
        let start = *self.tiles.get(usize::from(run.start) + i)?;
        if start == NO_REFS {
            return None;
        }
        let refs = self.ref_ids.get(usize::from(start)..)?;
        let end = refs
            .iter()
            .position(|r| r.is_last())
            .map_or(refs.len(), |at| at + 1);
        Some(&refs[..end])
    }

    fn check(&self) -> Result<()> {
        for (i, row) in self.tile_rows.iter().enumerate() {
            let (first, count, start) = (
                usize::from(row.first),
                usize::from(row.count),
                usize::from(row.start),
            );
            if count > 0
                && (first + count > usize::from(self.num_tile_cols)
                    || start + count > self.tiles.len())
            {
                return Err(refuse(format!(
                    "tile row {i} ({count} tiles from column {first}, tile {start}) leaves the \
                     {}-column grid or the {} tiles",
                    self.num_tile_cols,
                    self.tiles.len()
                )));
            }
        }
        if let Some(i) = self
            .tiles
            .iter()
            .position(|&t| t != NO_REFS && usize::from(t) >= self.ref_ids.len())
        {
            return Err(refuse(format!(
                "tile {i} starts at reference {} of {}",
                self.tiles[i],
                self.ref_ids.len()
            )));
        }
        for (bodies, runs, what) in [
            (&self.rivers, self.river_points.len(), "river"),
            (&self.lakes, self.lake_boxes.len(), "lake"),
        ] {
            if let Some(i) = bodies
                .iter()
                .position(|b| usize::from(b.start) + usize::from(b.count) > runs)
            {
                return Err(refuse(format!(
                    "{what} {i}'s run of {} from {} passes the {runs} the file holds",
                    bodies[i].count, bodies[i].start
                )));
            }
        }
        for (colours, bodies, what) in [
            (self.river_colours.len(), self.rivers.len(), "river"),
            (self.lake_colours.len(), self.lakes.len(), "lake"),
            (self.pool_colours.len(), self.pools.len(), "pool"),
        ] {
            if colours != bodies {
                return Err(refuse(format!(
                    "{colours} {what} colours for {bodies} {what}s"
                )));
            }
        }
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let got = self
            .bytes
            .get(self.at..self.at + n)
            .ok_or_else(|| refuse(format!("the data ends before byte {}", self.at + n)))?;
        self.at += n;
        Ok(got)
    }

    fn zeros(&mut self, n: usize, what: &str) -> Result<()> {
        let at = self.at;
        if self.take(n)?.iter().any(|&b| b != 0) {
            return Err(refuse(format!("the {what} at data byte {at} is not zero")));
        }
        Ok(())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(u32::from_be_bytes(
            self.take(4)?.try_into().unwrap(),
        )))
    }

    fn vec4(&mut self) -> Result<[f32; 4]> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }

    fn vec3(&mut self) -> Result<[f32; 3]> {
        let v = [self.f32()?, self.f32()?, self.f32()?];
        self.zeros(4, "fourth lane")?;
        Ok(v)
    }

    fn region(&mut self) -> Result<WaterMapRegion> {
        Ok(WaterMapRegion {
            centre_and_height: self.vec3()?,
            extent_and_weight: self.vec3()?,
        })
    }

    fn body(&mut self) -> Result<WaterMapBody> {
        let region = self.region()?;
        let count = self.u8()?;
        self.zeros(1, "body pad")?;
        let start = self.u16()?;
        self.zeros(12, "body tail")?;
        Ok(WaterMapBody {
            region,
            count,
            start,
        })
    }

    fn array<T>(
        &mut self,
        align: usize,
        n: usize,
        mut read: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        self.zeros(self.at.next_multiple_of(align) - self.at, "alignment pad")?;
        (0..n).map(|_| read(self)).collect()
    }
}

fn put_floats(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend_from_slice(&v.to_bits().to_be_bytes());
    }
}

fn put_region(out: &mut Vec<u8>, region: &WaterMapRegion) {
    for lanes in [region.centre_and_height, region.extent_and_weight] {
        put_floats(out, &lanes);
        out.extend_from_slice(&[0; 4]);
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("waterheight: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows of three columns: row 0 stores columns 1..=2, row 1 nothing.
    fn map() -> WaterMap {
        let region = WaterMapRegion {
            centre_and_height: [10.0, -20.0, 30.0],
            extent_and_weight: [5.0, 6.0, 0.25],
        };
        WaterMap {
            offset_x: -100.0,
            offset_y: 100.0,
            tile_size_x: 50.0,
            tile_size_y: 50.0,
            num_tile_cols: 3,
            tile_rows: vec![
                WaterMapTileRow {
                    first: 1,
                    count: 2,
                    start: 0,
                },
                WaterMapTileRow::default(),
            ],
            tiles: vec![0, NO_REFS],
            ref_ids: vec![WaterMapRefId(0x2000 | 0x80), WaterMapRefId(0x8000 | 0x6000)],
            river_points: vec![[1.0, 2.0, 3.0, 250.0], [4.0, 5.0, 6.0, 250.0]],
            rivers: vec![WaterMapBody {
                region,
                count: 2,
                start: 0,
            }],
            lake_boxes: vec![[0.0, 0.0, 10.0, 10.0]],
            lakes: vec![WaterMapBody {
                region,
                count: 1,
                start: 0,
            }],
            pools: vec![region],
            river_colours: vec![[1, 2, 3, 255]],
            lake_colours: vec![[4, 5, 6, 255]],
            pool_colours: vec![[7, 8, 9, 255]],
            ..WaterMap::default()
        }
    }

    #[test]
    fn a_map_saves_its_aligned_arrays_and_reopens_equal() {
        let bytes = map().save().expect("saves");
        // Rows (8) and tiles (4) and references (4) end at 16, already 16-aligned.
        let data = 8 + HEADER_LEN;
        assert_eq!(&bytes[data..data + 8], &[1, 2, 0, 0, 0, 0, 0, 0]);
        let data_len = 16 + 2 * 16 + 48 + 16 + 48 + 32;
        assert_eq!(&bytes[8..12], &(data_len as u32).to_be_bytes());
        assert_eq!(bytes.len(), data + data_len + 3 * 4);
        let reopened = WaterMap::open(&bytes).expect("reopens");
        assert_eq!(reopened, map());

        let refs = reopened.tile_refs(1, 0).expect("column 1 holds water");
        let kinds: Vec<_> = refs.iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, [WaterMapRefType::River, WaterMapRefType::Pool]);
        assert!(
            reopened.tile_refs(2, 0).is_none(),
            "its tile holds no reference"
        );
        assert!(reopened.tile_refs(0, 0).is_none(), "outside the row's run");
        assert!(reopened.tile_refs(1, 1).is_none(), "an empty row");
    }

    #[test]
    fn damaged_bytes_are_refused_by_name() {
        let bytes = map().save().expect("saves");
        let mut short = bytes.clone();
        short.pop();
        let error = WaterMap::open(&short).unwrap_err().to_string();
        assert!(error.contains("waterheight"), "{error}");
        let mut sizes = bytes.clone();
        sizes[0x33] = 8;
        assert!(WaterMap::open(&sizes).is_err());
        let mut lane = bytes.clone();
        let w = 8 + HEADER_LEN + 16 + 2 * 16 + 12;
        lane[w] = 1;
        assert!(WaterMap::open(&lane)
            .unwrap_err()
            .to_string()
            .contains("fourth lane"));
        let mut wide = map();
        wide.tile_rows[0].count = 3;
        assert!(wide.save().unwrap_err().to_string().contains("tile row 0"));
        let mut pale = map();
        pale.pool_colours.clear();
        assert!(pale.save().is_err());
    }
}
