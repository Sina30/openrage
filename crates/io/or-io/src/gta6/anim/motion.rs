//! A motion dictionary's motions: each names the clips it draws on by clip key, maps each clip's
//! time onto its own, and weighs and ranges them over its parameters. No motion holds frame data.

use super::databases::{SimplexDatabase, TableDatabase};
use super::{columns, metadata, AnimError, AnimResource, Record, Section, DICTIONARY_LEN};
use crate::gta6::hash::name_hash32;

const MOTION_LEN: usize = 0x140;
const GROUP_LEN: usize = 0x38;
const TIME_WARP_LEN: usize = 0x28;
const PHASE_CURVE_LEN: usize = 0x20;
const PARAMETER_ITEM_LEN: usize = 0x20;
const ROW_LEN: usize = 0x20;
pub(super) const GRID_LEN: usize = 0x50;

#[derive(Debug, Clone)]
pub struct Motion {
    pub name: String,
    /// The hash of the name without `pack:/` and `.motion`.
    pub key: u32,
    /// 1: one curve and range for the whole motion; 2: one for each group of its clips.
    pub kind: u32,
    /// Each clip it draws on, by clip key: the leaf name's hash low, its clip dictionary's key high.
    pub clips: Vec<u64>,
    /// One for each clip, mapping the clip's time onto the motion's.
    pub time_warps: Vec<TimeWarp>,
    pub weight_lists: Vec<Vec<Weight>>,
    pub parameters: Vec<Parameter>,
    /// At 0x80, for each clip, an item for each parameter.
    pub parameter_items: Vec<Vec<ParameterItem>>,
    pub groups: Vec<MotionGroup>,
    /// A kind-2 motion's own typed objects at 0x130.
    pub parameter_objects: Vec<ParameterObject>,
    pub properties: Vec<Record>,
}

/// A clip's item for one parameter: `(x, y)` points and four floats, whose roles are not known.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterItem {
    pub points: Vec<[f32; 2]>,
    pub floats: [f32; 4],
}

/// An array head `{pointer, u16 count, u16 capacity, u32}` as stated, its elements unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayHead {
    pub pointer: u64,
    pub count: u16,
    pub capacity: u16,
    pub word: u32,
}

/// A kind-2 motion's typed object: its type, its array heads at 0x10 and 0x20, a type 4's lists.
// ⚠ A type-1 or type-2 object here is not a group's constants or phase curves, so only its heads
// are read.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterObject {
    pub kind: u32,
    pub heads: [ArrayHead; 2],
    /// Type 4's lists of words at 0x20 and hashes at 0x30.
    pub lists: Option<(Vec<Vec<u32>>, Vec<u32>)>,
}

/// A clip's time warp: `a` and `b`, equal in every motion, and points that never fall.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeWarp {
    pub a: f32,
    pub b: f32,
    pub points: Vec<[f32; 2]>,
}

/// One weight of a weight list: the clip it weighs, by index into [`Motion::clips`], and two values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weight {
    pub clip: u32,
    pub values: [f32; 2],
}

/// One of a motion's parameters: a hash, two words, a third word and its range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parameter {
    pub hash: u32,
    pub words: [u16; 2],
    pub word: u32,
    pub min: f32,
    pub max: f32,
}

/// A range of a motion's clips and weight lists with its curve, its range object and its tags.
#[derive(Debug, Clone)]
pub struct MotionGroup {
    pub first_clip: u32,
    pub clips: u32,
    pub first_weight_list: u32,
    pub weight_lists: u32,
    pub curve: Typed,
    pub range: Typed,
    pub tags: Vec<Record>,
}

/// An object whose head `{u64 0, u32 type, u32 0}` states its type.
#[derive(Debug, Clone, PartialEq)]
pub enum Typed {
    /// Type 1: a value for each clip.
    Constants(Vec<f32>),
    /// Type 2: a phase curve for each clip.
    PhaseCurves(Vec<PhaseCurve>),
    /// Type 3: a parameter range, its hashes and its bounds.
    Range {
        lo: f32,
        hi: f32,
        hashes: Vec<u32>,
        bounds: Vec<(u32, f32)>,
    },
    /// Type 4: lists of words and a list of hashes.
    Lists {
        /// The array at 0x10, whose element is not known.
        head: ArrayHead,
        lists: Vec<Vec<u32>>,
        hashes: Vec<u32>,
    },
    /// Type 5: a database of one table of rows over one grid.
    Table(TableDatabase),
    /// Type 6: a database of tables of rows and of grids.
    Rows(Database),
    /// Type 8: a database of tables over grids and of corners over a grid of its own.
    Simplices(SimplexDatabase),
}

/// A type-6 database: its keys, its tables, its grids and its bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct Database {
    /// `{hash, u16, u16}` at 0x10.
    pub keys: Vec<(u32, [u16; 2])>,
    /// Rows of four 8-byte items of four f16 halves each, whose meaning is not known.
    pub tables: Vec<Vec<[u16; 16]>>,
    pub grids: Vec<Grid>,
    /// The word at 0x50: the motion's weight-list count.
    pub lists: u32,
    /// Four min and max pairs at 0x54.
    pub bounds: [f32; 8],
    /// The hash at 0x80.
    pub hash: u32,
}

/// A database's grid: two words, three integer cell ranges, then twelve floats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub words: [u32; 2],
    /// Each axis's lowest and highest cell, `floor(bound / cell size)`.
    pub cells: [[i32; 2]; 3],
    /// Each axis's min and max, then its cell size, then its inverse.
    pub floats: [f32; 12],
}

impl Typed {
    /// The type word the object's head states.
    pub fn type_word(&self) -> u32 {
        match self {
            Self::Constants(_) => 1,
            Self::PhaseCurves(_) => 2,
            Self::Range { .. } => 3,
            Self::Lists { .. } => 4,
            Self::Table(_) => 5,
            Self::Rows(_) => 6,
            Self::Simplices(_) => 8,
        }
    }
}

/// A curve over `cycles` cycles of the domain `x0` to `x1`, its y never falling, entered part-way
/// where its x falls once.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseCurve {
    pub points: Vec<[f32; 2]>,
    pub x0: f32,
    pub x1: f32,
    pub y1: f32,
    pub cycles: u32,
}

impl Section<'_> {
    /// Whether a root whose word at 0x6C is 0 is a motion dictionary: its map's first object
    /// states 1 at 0x10 and a name at 0x30, where an animation holds zero at 0x38.
    pub(super) fn is_motion_dictionary(&self) -> Result<bool, AnimError> {
        let Some(&(_, entry)) = self.map("root", 0x48, 0x10, true)?.first() else {
            return Ok(false);
        };
        let at = self.target("root map object", entry + 8)?;
        Ok(self.u64("root map object", at + 0x10)? == 1
            && self.u64("root map object", at + 0x38)? != 0)
    }

    pub(super) fn motion_dictionary(&self, r: &mut AnimResource) -> Result<(), AnimError> {
        self.bytes("root", 0, DICTIONARY_LEN)?;
        for at in self.array("clip dictionary list", 0x70, 16)? {
            r.dictionaries
                .push(self.u32("clip dictionary list", at + 8)?);
        }
        let index = self.map("clip dictionary index", 0x80, 8, false)?;
        let broken = |why: String| AnimError::Map {
            what: "clip dictionary index",
            at: 0x80,
            why,
        };
        if index.len() != r.dictionaries.len() {
            return Err(broken(format!(
                "holds {} keys for a list of {}",
                index.len(),
                r.dictionaries.len()
            )));
        }
        for (key, entry) in index {
            let position = self.u32("clip dictionary index", entry + 4)? as usize;
            if r.dictionaries.get(position) != Some(&(key as u32)) {
                return Err(broken(format!(
                    "maps key {key:#010X} to position {position}, where the list does not hold it"
                )));
            }
        }
        // A body of matchers alone states no metadata region: a null pointer and 0 bytes.
        let null = self.pointer("metadata region", 0xA0)?.is_none()
            && self.u32("metadata region", 0xA8)? == 0;
        let region = if null { None } else { Some(self.region()?) };
        for (key, entry) in self.map("motion", 0x48, 0x10, true)? {
            let at = self.target("motion", entry + 8)?;
            // A matcher holds its name's pointer at 0x38, where a motion holds its name's tail.
            if self.u64("motion", at + 0x38)? >> 28 == 5 {
                r.matchers
                    .push(self.matcher(at, key, r.key, &r.dictionaries)?);
                continue;
            }
            let region = region.ok_or_else(|| AnimError::Metadata {
                at: 0xA0,
                why: "the root states none, where a motion's properties lie".into(),
            })?;
            r.motions.push(self.motion(at, key, r.key, region)?);
        }
        Ok(())
    }

    fn motion(
        &self,
        at: usize,
        key: u64,
        root_key: u32,
        region: metadata::Region,
    ) -> Result<Motion, AnimError> {
        let head = self.bytes("motion", at, MOTION_LEN)?;
        let name = self.name("motion name", at + 0x30)?;
        let bad = |why: &str| AnimError::Motion {
            motion: name.clone(),
            why: why.to_string(),
        };
        let base = name.strip_prefix("pack:/").unwrap_or(&name);
        let base = base
            .strip_suffix(".motion")
            .ok_or_else(|| bad("its name does not end in .motion"))?;
        if key != u64::from(name_hash32(base.as_bytes()))
            || u64::from(self.u32("motion", at + 0x28)?) != key
            || self.u32("motion", at + 0x2C)? != root_key
        {
            return Err(bad(
                "its key words are not its name's hash and its dictionary's key",
            ));
        }
        if head[..0x10]
            .iter()
            .chain(&head[0x40..0x50])
            .any(|b| *b != 0)
            || self.u64("motion", at + 0x10)? != 1
            || self.pointer("motion dictionary", at + 0x18)? != Some(0)
        {
            return Err(bad(
                "its head is not {16 zero bytes, 1, the root} with zero at 0x40",
            ));
        }
        let kind = self.u32("motion", at + 0x58)?;
        if !matches!(kind, 1 | 2) {
            return Err(bad(&format!(
                "its kind word at 0x58 is {kind}, where 1 and 2 are known"
            )));
        }
        let properties_at = self.target("motion properties", at + 0x20)?;
        let properties = metadata::properties(self, region, properties_at)?;
        let mut clips = Vec::new();
        for e in self.array("motion clip list", at + 0x60, 16)? {
            if self.u64("motion clip", e)? != 0 {
                return Err(bad(
                    "a clip entry holds a pointer, where every one names its clip by key",
                ));
            }
            let leaf = self.u32("motion clip", e + 8)?;
            clips.push(u64::from(self.u32("motion clip", e + 12)?) << 32 | u64::from(leaf));
        }
        let mut time_warps = Vec::new();
        for e in self.array("time warp list", at + 0x70, TIME_WARP_LEN)? {
            let points = self.curve(e + 8, 0).map_err(|why| bad(&why))?;
            let (a, b) = (self.f32("time warp", e)?, self.f32("time warp", e + 4)?);
            if !(a.is_finite() && b.is_finite())
                || self
                    .bytes("time warp", e + 0x18, 16)?
                    .iter()
                    .any(|b| *b != 0)
            {
                return Err(bad(
                    "a time warp's ends are not finite or its tail is not zero",
                ));
            }
            time_warps.push(TimeWarp { a, b, points });
        }
        if time_warps.len() != clips.len() {
            return Err(bad("its time warps are not one for each clip"));
        }
        let mut weight_lists = Vec::new();
        for e in self.array("weight lists", at + 0x90, 16)? {
            let mut list = Vec::new();
            for w in self.array("weight list", e, 12)? {
                let weight = Weight {
                    clip: self.u32("weight", w)?,
                    values: [self.f32("weight", w + 4)?, self.f32("weight", w + 8)?],
                };
                if weight.clip as usize >= clips.len()
                    || !weight.values.iter().all(|v| v.is_finite())
                {
                    return Err(bad("a weight names no clip of the motion or is not finite"));
                }
                list.push(weight);
            }
            weight_lists.push(list);
        }
        let parameters = self.parameters(at, clips.len()).map_err(|why| bad(&why))?;
        let parameter_items = self.parameter_items(at)?;
        let lists = weight_lists.len() as u64;
        let mut groups = Vec::new();
        let mut parameter_objects = Vec::new();
        if kind == 1 {
            let tags_at = self.target("motion tags", at + 0x120)?;
            if tags_at != properties_at + 0x28 {
                return Err(bad("its tags at 0x120 are not 0x28 after its properties"));
            }
            groups.push(MotionGroup {
                first_clip: 0,
                clips: clips.len() as u32,
                first_weight_list: 0,
                weight_lists: lists as u32,
                curve: self.typed(self.target("motion curve", at + 0x130)?, &name, lists)?,
                range: self.typed(self.target("motion range", at + 0x138)?, &name, lists)?,
                tags: metadata::tags(self, region, at, tags_at)?,
            });
        } else {
            for g in self.array("motion groups", at + 0x120, GROUP_LEN)? {
                let c0 = self.u32("motion group", g + 0x20)?;
                let cn = self.u32("motion group", g + 0x24)?;
                let l0 = self.u32("motion group", g + 0x28)?;
                let ln = self.u32("motion group", g + 0x2C)?;
                if u64::from(c0) + u64::from(cn) > clips.len() as u64
                    || u64::from(l0) + u64::from(ln) > lists
                    || self.u64("motion group", g + 8)? != 0
                {
                    return Err(bad("a group's clips or weight lists run past the motion's"));
                }
                groups.push(MotionGroup {
                    first_clip: c0,
                    clips: cn,
                    first_weight_list: l0,
                    weight_lists: ln,
                    curve: self.typed(self.target("group curve", g + 0x10)?, &name, ln.into())?,
                    range: self.typed(self.target("group range", g + 0x18)?, &name, ln.into())?,
                    tags: metadata::tags(self, region, at, self.target("group tags", g)?)?,
                });
            }
            for e in self.array("motion parameter objects", at + 0x130, 8)? {
                let object = self.target("motion parameter object", e)?;
                let kind = self.typed_head(object, &name)?;
                let lists = match kind {
                    4 => Some(self.type_four(object)?),
                    _ => None,
                };
                parameter_objects.push(ParameterObject {
                    kind,
                    heads: [self.head(object + 0x10)?, self.head(object + 0x20)?],
                    lists,
                });
            }
        }
        let stated = |t: &Typed| match t {
            Typed::Simplices(database) => u64::from(database.lists) == lists,
            _ => true,
        };
        if !groups.iter().all(|g| stated(&g.curve) && stated(&g.range)) {
            return Err(bad(
                "a type-8 database does not state the motion's weight-list count",
            ));
        }
        for typed in groups.iter().flat_map(|g| [&g.curve, &g.range]) {
            if let Err(why) = columns::check(&properties, typed) {
                return Err(bad(&why));
            }
        }
        Ok(Motion {
            name,
            key: key as u32,
            kind,
            clips,
            time_warps,
            weight_lists,
            parameters,
            parameter_items,
            groups,
            parameter_objects,
            properties,
        })
    }

    /// The lists at 0x80: for each clip an array of 0x20-byte items, each `{(x, y) points, 4 f32}`.
    fn parameter_items(&self, at: usize) -> Result<Vec<Vec<ParameterItem>>, AnimError> {
        let mut clips = Vec::new();
        for list in self.array("motion parameter items", at + 0x80, 16)? {
            let mut items = Vec::new();
            for item in self.array("motion parameter item", list, PARAMETER_ITEM_LEN)? {
                let mut points = Vec::new();
                for p in self.array("motion parameter points", item, 8)? {
                    points.push([
                        self.f32("motion parameter point", p)?,
                        self.f32("motion parameter point", p + 4)?,
                    ]);
                }
                let mut floats = [0.0; 4];
                for (i, f) in floats.iter_mut().enumerate() {
                    *f = self.f32("motion parameter item", item + 0x10 + 4 * i)?;
                }
                items.push(ParameterItem { points, floats });
            }
            clips.push(items);
        }
        Ok(clips)
    }

    /// The array head at `at`, its elements unread.
    fn head(&self, at: usize) -> Result<ArrayHead, AnimError> {
        Ok(ArrayHead {
            pointer: self.u64("array head", at)?,
            count: self.u16("array head", at + 8)?,
            capacity: self.u16("array head", at + 10)?,
            word: self.u32("array head", at + 12)?,
        })
    }

    /// A type-4 object's lists of words at 0x20 and its hashes at 0x30.
    fn type_four(&self, at: usize) -> Result<(Vec<Vec<u32>>, Vec<u32>), AnimError> {
        let words = |at: usize| -> Result<Vec<u32>, AnimError> {
            self.array("typed object words", at, 4)?
                .into_iter()
                .map(|e| self.u32("typed object word", e))
                .collect()
        };
        let mut lists = Vec::new();
        for e in self.array("typed object lists", at + 0x20, 16)? {
            lists.push(words(e)?);
        }
        Ok((lists, words(at + 0x30)?))
    }

    /// The parameter space at 0x80 to 0xD0: one hash, word, minimum and maximum for each
    /// parameter at 0xA0 to 0xD0, and at 0x80 one list for each clip.
    fn parameters(&self, at: usize, clips: usize) -> Result<Vec<Parameter>, String> {
        let e = |e: AnimError| e.to_string();
        let hashes = self.array("motion parameters", at + 0xA0, 8).map_err(e)?;
        let words = self
            .array("motion parameter words", at + 0xB0, 4)
            .map_err(e)?;
        let mins = self
            .array("motion parameter minima", at + 0xC0, 4)
            .map_err(e)?;
        let maxs = self
            .array("motion parameter maxima", at + 0xD0, 4)
            .map_err(e)?;
        let per_clip = usize::from(self.u16("motion", at + 0x88).map_err(e)?);
        let count = hashes.len();
        if words.len() != count || mins.len() != count || maxs.len() != count {
            return Err("its parameter hashes, words, minima and maxima differ in number".into());
        }
        if (count > 0 || per_clip > 0) && (count == 0 || per_clip != clips) {
            return Err("its parameter space is not one list for each clip".into());
        }
        (0..count)
            .map(|i| {
                let parameter = Parameter {
                    hash: self.u32("motion parameter", hashes[i]).map_err(e)?,
                    words: [
                        self.u16("motion parameter", hashes[i] + 4).map_err(e)?,
                        self.u16("motion parameter", hashes[i] + 6).map_err(e)?,
                    ],
                    word: self.u32("motion parameter", words[i]).map_err(e)?,
                    min: self.f32("motion parameter", mins[i]).map_err(e)?,
                    max: self.f32("motion parameter", maxs[i]).map_err(e)?,
                };
                if !(parameter.min.is_finite()
                    && parameter.max.is_finite()
                    && parameter.min <= parameter.max)
                {
                    return Err(
                        "a parameter's minimum is not finite or lies above its maximum".into(),
                    );
                }
                Ok(parameter)
            })
            .collect()
    }

    /// The `(x, y)` points of the array at `at`: finite, y never falling, x falling at most `wraps` times.
    fn curve(&self, at: usize, wraps: usize) -> Result<Vec<[f32; 2]>, String> {
        let mut points = Vec::new();
        for p in self.array("curve", at, 8).map_err(|e| e.to_string())? {
            let x = self.f32("curve", p).map_err(|e| e.to_string())?;
            let y = self.f32("curve", p + 4).map_err(|e| e.to_string())?;
            points.push([x, y]);
        }
        let falls = points.windows(2).filter(|w| w[1][0] < w[0][0]).count();
        if !points.iter().flatten().all(|v| v.is_finite())
            || falls > wraps
            || points.windows(2).any(|w| w[1][1] < w[0][1])
        {
            return Err(format!(
                "a curve of {} points is not finite or falls",
                points.len()
            ));
        }
        Ok(points)
    }

    /// The type-6 database at `at`: its keys, its tables of 32-byte rows, its 0x50-byte grids
    /// whose cell ranges never fall and whose floats are finite, and its finite bounds.
    fn database(&self, at: usize) -> Result<Database, String> {
        let e = |e: AnimError| e.to_string();
        let mut keys = Vec::new();
        for k in self.array("database keys", at + 0x10, 8).map_err(e)? {
            let hash = self.u32("database key", k).map_err(e)?;
            let words = [
                self.u16("database key", k + 4).map_err(e)?,
                self.u16("database key", k + 6).map_err(e)?,
            ];
            keys.push((hash, words));
        }
        let mut tables = Vec::new();
        for t in self.array("database tables", at + 0x20, 16).map_err(e)? {
            tables.push(self.rows(t)?);
        }
        let mut grids = Vec::new();
        for g in self
            .array("database grids", at + 0x30, GRID_LEN)
            .map_err(e)?
        {
            grids.push(self.grid(g)?);
        }
        let mut bounds = [0.0; 8];
        for (i, bound) in bounds.iter_mut().enumerate() {
            *bound = self.f32("database bounds", at + 0x54 + 4 * i).map_err(e)?;
        }
        if !bounds.iter().all(|b| b.is_finite()) {
            return Err(format!(
                "the database at {at:#X} has a bound that is not finite"
            ));
        }
        Ok(Database {
            keys,
            tables,
            grids,
            lists: self.u32("database", at + 0x50).map_err(e)?,
            bounds,
            hash: self.u32("database", at + 0x80).map_err(e)?,
        })
    }

    /// The 32-byte rows of the array at `at`, each four items of four f16 halves.
    pub(super) fn rows(&self, at: usize) -> Result<Vec<[u16; 16]>, String> {
        let e = |e: AnimError| e.to_string();
        let mut rows = Vec::new();
        for r in self.array("database table", at, ROW_LEN).map_err(e)? {
            let bytes = self.bytes("database row", r, ROW_LEN).map_err(e)?;
            let mut row = [0u16; 16];
            for (half, pair) in row.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *half = u16::from_le_bytes(*pair);
            }
            rows.push(row);
        }
        Ok(rows)
    }

    /// The grid at `g`, whose cell ranges never fall and whose floats are finite.
    pub(super) fn grid(&self, g: usize) -> Result<Grid, String> {
        let e = |e: AnimError| e.to_string();
        let word = |i: usize| self.word::<4>("database grid", g + 4 * i).map_err(e);
        let mut grid = Grid {
            words: [u32::from_le_bytes(word(0)?), u32::from_le_bytes(word(1)?)],
            cells: [[0; 2]; 3],
            floats: [0.0; 12],
        };
        for (i, cell) in grid.cells.as_flattened_mut().iter_mut().enumerate() {
            *cell = i32::from_le_bytes(word(2 + i)?);
        }
        for (i, float) in grid.floats.iter_mut().enumerate() {
            *float = f32::from_le_bytes(word(8 + i)?);
        }
        if grid.cells.iter().any(|[lo, hi]| lo > hi) || !grid.floats.iter().all(|f| f.is_finite()) {
            return Err(format!("the database grid at {g:#X} has a cell range that falls or a float that is not finite"));
        }
        Ok(grid)
    }

    /// The type the head `{u64 0, u32 type, u32 0}` at `at` states, among 1 to 6 and 8.
    fn typed_head(&self, at: usize, motion: &str) -> Result<u32, AnimError> {
        let kind = self.u32("typed object", at + 8)?;
        if self.u64("typed object", at)? != 0
            || self.u32("typed object", at + 12)? != 0
            || !matches!(kind, 1..=6 | 8)
        {
            return Err(AnimError::Motion {
                motion: motion.to_string(),
                why: format!("the object at {at:#X} has no head {{0, type, 0}} of a type a motion was seen to hold"),
            });
        }
        Ok(kind)
    }

    /// The typed object at `at` of a group of `lists` weight lists.
    fn typed(&self, at: usize, motion: &str, lists: u64) -> Result<Typed, AnimError> {
        let bad = |why: String| AnimError::Motion {
            motion: motion.to_string(),
            why,
        };
        let kind = self.typed_head(at, motion)?;
        let floats = |at: usize| -> Result<Vec<f32>, AnimError> {
            self.array("typed object values", at, 4)?
                .into_iter()
                .map(|e| self.f32("typed object value", e))
                .collect()
        };
        Ok(match kind {
            1 => {
                let values = floats(at + 0x10)?;
                if !values.iter().all(|v| v.is_finite()) {
                    return Err(bad(format!("the constants at {at:#X} are not finite")));
                }
                Typed::Constants(values)
            }
            2 => {
                let mut curves = Vec::new();
                for e in self.array("phase curves", at + 0x10, PHASE_CURVE_LEN)? {
                    let points = self.curve(e, 1).map_err(bad)?;
                    let (x0, x1) = (
                        self.f32("phase curve", e + 0x10)?,
                        self.f32("phase curve", e + 0x14)?,
                    );
                    let y1 = self.f32("phase curve", e + 0x18)?;
                    if !(x0.is_finite() && x1.is_finite() && y1.is_finite())
                        || x0 > x1
                        || points.iter().any(|p| !(x0..=x1).contains(&p[0]))
                    {
                        return Err(bad(format!("the phase curve at {e:#X} leaves its domain")));
                    }
                    curves.push(PhaseCurve {
                        points,
                        x0,
                        x1,
                        y1,
                        cycles: self.u32("phase curve", e + 0x1C)?,
                    });
                }
                Typed::PhaseCurves(curves)
            }
            3 => {
                let mut hashes = Vec::new();
                for e in self.array("range hashes", at + 0x10, 8)? {
                    hashes.push(self.u32("range hash", e)?);
                }
                let mut bounds = Vec::new();
                for e in self.array("range bounds", at + 0x20, 8)? {
                    let bound = (self.u32("range bound", e)?, self.f32("range bound", e + 4)?);
                    if !bound.1.is_finite() {
                        return Err(bad(format!(
                            "a bound of the range at {at:#X} is not finite"
                        )));
                    }
                    bounds.push(bound);
                }
                let (lo, hi) = (self.f32("range", at + 0x30)?, self.f32("range", at + 0x34)?);
                if !(lo.is_finite() && hi.is_finite() && lo <= hi) {
                    return Err(bad(format!("the range at {at:#X} is {lo} to {hi}")));
                }
                Typed::Range {
                    lo,
                    hi,
                    hashes,
                    bounds,
                }
            }
            4 => {
                let (lists, hashes) = self.type_four(at)?;
                Typed::Lists {
                    head: self.head(at + 0x10)?,
                    lists,
                    hashes,
                }
            }
            5 => Typed::Table(self.table_database(at, lists).map_err(bad)?),
            6 => Typed::Rows(self.database(at).map_err(bad)?),
            8 => Typed::Simplices(self.simplex_database(at).map_err(bad)?),
            other => {
                return Err(bad(format!(
                    "the object at {at:#X} has type {other}, which no motion was seen to hold"
                )))
            }
        })
    }
}
