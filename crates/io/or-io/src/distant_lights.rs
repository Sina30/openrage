//! `distantlights.dat` and `distantlights_hd.dat`: the far-off traffic and
//! street lights, as groups of points along roads, bucketed into a square of
//! world blocks and stored big-endian. The `_hd` file has a 32-block grid and
//! longer groups, the other 16 blocks.

use crate::error::{IoError, Result};

/// The block grid's side in `distantlights.dat`.
pub const BLOCKS: usize = 16;
/// The block grid's side in `distantlights_hd.dat`, whose groups carry two more fields.
pub const BLOCKS_HD: usize = 32;

const POINT_LEN: usize = 6;
const GROUP_LEN: usize = 14;
const GROUP_LEN_HD: usize = 24;

/// One distant lights file: every point, every group and each block's range of groups.
#[derive(Debug, Clone, PartialEq)]
pub struct DistantLights {
    /// Each block's first group, `blocks()` squared of them in the file's order.
    pub block_first_group: Vec<i32>,
    /// How many of a block's groups, from its first, are coastal.
    pub block_num_coastal_groups: Vec<i32>,
    /// How many inland groups follow a block's coastal ones.
    pub block_num_inland_groups: Vec<i32>,
    pub points: Vec<DistantLightPoint>,
    pub groups: Vec<DistantLightGroup>,
}

/// A point on a group's road, in whole world units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DistantLightPoint {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

/// A run of points that lights slide along, and how they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DistantLightGroup {
    pub centre_x: i16,
    pub centre_y: i16,
    pub centre_z: i16,
    pub radius: i16,
    /// The group's first point in [`DistantLights::points`].
    pub point_offset: i16,
    pub point_count: i16,
    /// Bit-packed: density 0..=3, speed 4..=7, lane count 8..=11, two-way 12, street lights 13.
    pub display_properties: u16,
    /// Stored only in the 32-block (`_hd`) file.
    pub distance_offset: f32,
    /// Stored only in the 32-block (`_hd`) file.
    pub random_seed: i32,
}

impl DistantLightGroup {
    pub fn density(&self) -> u8 {
        (self.display_properties & 0xf) as u8
    }
    pub fn speed(&self) -> u8 {
        (self.display_properties >> 4 & 0xf) as u8
    }
    pub fn num_lanes(&self) -> u8 {
        (self.display_properties >> 8 & 0xf) as u8
    }
    pub fn two_way(&self) -> bool {
        self.display_properties & (1 << 12) != 0
    }
    pub fn use_streetlights(&self) -> bool {
        self.display_properties & (1 << 13) != 0
    }
}

impl DistantLights {
    /// Reads a whole file, refusing a length no block grid matches or a range outside its arrays.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes, at: 0 };
        let num_points = r.count("the point count")?;
        let num_groups = r.count("the group count")?;
        // No field states the grid: only one grid's length can match the stated counts.
        let blocks = [BLOCKS, BLOCKS_HD]
            .into_iter()
            .find(|&b| bytes.len() == file_len(b, num_points, num_groups))
            .ok_or_else(|| {
                refuse(format!(
                    "{} bytes is neither the 16-block ({}) nor the 32-block ({}) length for \
                     {num_points} points and {num_groups} groups",
                    bytes.len(),
                    file_len(BLOCKS, num_points, num_groups),
                    file_len(BLOCKS_HD, num_points, num_groups)
                ))
            })?;
        let cells = blocks * blocks;
        let mut table = || (0..cells).map(|_| r.i32()).collect::<Vec<_>>();
        let block_first_group = table();
        let block_num_coastal_groups = table();
        let block_num_inland_groups = table();
        let points = (0..num_points)
            .map(|_| DistantLightPoint {
                x: r.i16(),
                y: r.i16(),
                z: r.i16(),
            })
            .collect();
        let mut groups = Vec::with_capacity(num_groups);
        for i in 0..num_groups {
            let mut group = DistantLightGroup {
                centre_x: r.i16(),
                centre_y: r.i16(),
                centre_z: r.i16(),
                radius: r.i16(),
                point_offset: r.i16(),
                point_count: r.i16(),
                display_properties: r.i16() as u16,
                ..Default::default()
            };
            if blocks == BLOCKS_HD {
                if r.i16() != 0 {
                    return Err(refuse(format!(
                        "group {i}'s two bytes before its distance offset are not zero"
                    )));
                }
                group.distance_offset = f32::from_bits(r.i32() as u32);
                group.random_seed = r.i32();
            }
            groups.push(group);
        }
        let lights = Self {
            block_first_group,
            block_num_coastal_groups,
            block_num_inland_groups,
            points,
            groups,
        };
        lights.check()?;
        Ok(lights)
    }

    /// Writes the file, refusing what [`DistantLights::open`] would refuse.
    pub fn save(&self) -> Result<Vec<u8>> {
        self.check()?;
        let hd = self.blocks() == BLOCKS_HD;
        let mut out = Vec::with_capacity(file_len(
            self.blocks(),
            self.points.len(),
            self.groups.len(),
        ));
        out.extend_from_slice(&(self.points.len() as i32).to_be_bytes());
        out.extend_from_slice(&(self.groups.len() as i32).to_be_bytes());
        for table in [
            &self.block_first_group,
            &self.block_num_coastal_groups,
            &self.block_num_inland_groups,
        ] {
            for v in table {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        for p in &self.points {
            for v in [p.x, p.y, p.z] {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        for g in &self.groups {
            for v in [
                g.centre_x,
                g.centre_y,
                g.centre_z,
                g.radius,
                g.point_offset,
                g.point_count,
            ] {
                out.extend_from_slice(&v.to_be_bytes());
            }
            out.extend_from_slice(&g.display_properties.to_be_bytes());
            if hd {
                out.extend_from_slice(&[0, 0]);
                out.extend_from_slice(&g.distance_offset.to_bits().to_be_bytes());
                out.extend_from_slice(&g.random_seed.to_be_bytes());
            }
        }
        Ok(out)
    }

    /// The block grid's side: [`BLOCKS`] or [`BLOCKS_HD`].
    pub fn blocks(&self) -> usize {
        match self.block_first_group.len() {
            n if n == BLOCKS_HD * BLOCKS_HD => BLOCKS_HD,
            _ => BLOCKS,
        }
    }

    /// A group's points, or none when its range leaves [`DistantLights::points`].
    pub fn points_of(&self, group: &DistantLightGroup) -> &[DistantLightPoint] {
        usize::try_from(group.point_offset)
            .ok()
            .zip(usize::try_from(group.point_count).ok())
            .and_then(|(at, n)| self.points.get(at..at + n))
            .unwrap_or(&[])
    }

    fn check(&self) -> Result<()> {
        let blocks = self.blocks();
        let cells = blocks * blocks;
        let tables = [
            ("first-group", &self.block_first_group),
            ("coastal", &self.block_num_coastal_groups),
            ("inland", &self.block_num_inland_groups),
        ];
        if let Some((name, table)) = tables.iter().find(|(_, t)| t.len() != cells) {
            return Err(refuse(format!(
                "the block {name} table holds {} entries where a {blocks}-block grid has {cells}",
                table.len()
            )));
        }
        if self.points.len() > i16::MAX as usize + 1 {
            return Err(refuse(format!(
                "{} points, more than a group's 16-bit offset reaches",
                self.points.len()
            )));
        }
        for (i, g) in self.groups.iter().enumerate() {
            let fits = usize::try_from(g.point_offset)
                .ok()
                .zip(usize::try_from(g.point_count).ok())
                .is_some_and(|(at, n)| at + n <= self.points.len());
            if !fits {
                return Err(refuse(format!(
                    "group {i} names {} points from {}, outside the {} points",
                    g.point_count,
                    g.point_offset,
                    self.points.len()
                )));
            }
            if blocks == BLOCKS && (g.distance_offset != 0.0 || g.random_seed != 0) {
                return Err(refuse(format!(
                    "group {i} carries a distance offset or seed, which a 16-block file cannot store"
                )));
            }
        }
        for cell in 0..cells {
            let first = self.block_first_group[cell];
            let (coastal, inland) = (
                self.block_num_coastal_groups[cell],
                self.block_num_inland_groups[cell],
            );
            let n = i64::from(coastal) + i64::from(inland);
            let fits = first >= 0
                && coastal >= 0
                && inland >= 0
                && i64::from(first) + n <= self.groups.len() as i64;
            if !fits {
                return Err(refuse(format!(
                    "block {cell} names {n} groups from {first}, outside the {} groups",
                    self.groups.len()
                )));
            }
        }
        Ok(())
    }
}

fn file_len(blocks: usize, points: usize, groups: usize) -> usize {
    let group = if blocks == BLOCKS_HD {
        GROUP_LEN_HD
    } else {
        GROUP_LEN
    };
    8 + 3 * 4 * blocks * blocks + POINT_LEN * points + group * groups
}

/// Reads big-endian values; [`DistantLights::open`] checks the whole length first.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn count(&mut self, what: &str) -> Result<usize> {
        let raw = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| refuse(format!("the file ends before {what}")))?;
        self.at += 4;
        let n = i32::from_be_bytes(raw.try_into().unwrap());
        usize::try_from(n).map_err(|_| refuse(format!("{what} is {n}")))
    }
    fn i16(&mut self) -> i16 {
        let v = i16::from_be_bytes([self.bytes[self.at], self.bytes[self.at + 1]]);
        self.at += 2;
        v
    }
    fn i32(&mut self) -> i32 {
        let v = i32::from_be_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("distant lights: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lights(blocks: usize) -> DistantLights {
        let cells = blocks * blocks;
        let mut first = vec![0; cells];
        let mut coastal = vec![0; cells];
        let mut inland = vec![0; cells];
        first[3] = 0;
        coastal[3] = 1;
        inland[3] = 1;
        let hd = blocks == BLOCKS_HD;
        let group = |offset, count| DistantLightGroup {
            centre_x: -1200,
            centre_y: 300,
            centre_z: 40,
            radius: 250,
            point_offset: offset,
            point_count: count,
            display_properties: 0x2312,
            distance_offset: if hd { 12.5 } else { 0.0 },
            random_seed: if hd { -7 } else { 0 },
        };
        DistantLights {
            block_first_group: first,
            block_num_coastal_groups: coastal,
            block_num_inland_groups: inland,
            points: (0..5)
                .map(|i| DistantLightPoint {
                    x: i,
                    y: -i,
                    z: 2 * i,
                })
                .collect(),
            groups: vec![group(0, 3), group(3, 2)],
        }
    }

    #[test]
    fn both_grids_save_their_length_and_reopen_equal() {
        for blocks in [BLOCKS, BLOCKS_HD] {
            let file = lights(blocks);
            let bytes = file.save().expect("saves");
            assert_eq!(bytes.len(), file_len(blocks, 5, 2));
            let back = DistantLights::open(&bytes).expect("reopens");
            assert_eq!(back.blocks(), blocks);
            assert_eq!(back, file);
            assert_eq!(back.points_of(&back.groups[1]).len(), 2);
        }
    }

    #[test]
    fn display_properties_unpack() {
        let g = lights(BLOCKS).groups[0];
        assert_eq!((g.density(), g.speed(), g.num_lanes()), (2, 1, 3));
        assert!(!g.two_way());
        assert!(g.use_streetlights());
    }

    #[test]
    fn damaged_bytes_are_refused_by_name() {
        let bytes = lights(BLOCKS).save().expect("saves");
        let error = DistantLights::open(&bytes[..bytes.len() - 1])
            .unwrap_err()
            .to_string();
        assert!(error.contains("distant lights"), "{error}");
        let mut far = lights(BLOCKS);
        far.groups[1].point_count = 3;
        assert!(far.save().is_err());
        let mut seeded = lights(BLOCKS);
        seeded.groups[0].random_seed = 1;
        assert!(seeded.save().is_err());
        let mut padded = lights(BLOCKS_HD).save().expect("saves");
        let first_group = file_len(BLOCKS_HD, 5, 0);
        padded[first_group + 14] = 1;
        assert!(DistantLights::open(&padded).is_err());
    }
}
