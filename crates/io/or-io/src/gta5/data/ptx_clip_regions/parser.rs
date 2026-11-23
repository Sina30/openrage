use std::collections::HashMap;

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ClipRegionEntry {
    pub texture_name: String,
    pub texture_hash: u32,
    pub num_columns: u32,
    pub num_rows: u32,
    pub frames: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Default)]
pub struct ClipRegionTable {
    by_hash: HashMap<u32, ClipRegionEntry>,
}

impl ClipRegionTable {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn parse(text: &str) -> Result<Self, ClipRegionParseError> {
        let mut tok = Tokenizer::new(text);

        let num_textures = tok.next_u32()?;
        let total_clip_regions = tok.next_u32()?;

        let mut by_hash = HashMap::with_capacity(num_textures as usize);
        let mut total_frames_loaded: u32 = 0;
        for _ in 0..num_textures {
            let raw_name = tok.next_token()?;
            let texture_name = raw_name.to_ascii_lowercase();
            let texture_hash = crate::string_hash(texture_name.as_bytes());
            let num_columns = tok.next_u32()?;
            let num_rows = tok.next_u32()?;
            let count = num_columns.checked_mul(num_rows).ok_or(
                ClipRegionParseError::FrameCountOverflow {
                    texture: texture_name.clone(),
                    columns: num_columns,
                    rows: num_rows,
                },
            )?;
            let mut frames = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let u_min = tok.next_f32()?;
                let u_max = tok.next_f32()?;
                let v_min = tok.next_f32()?;
                let v_max = tok.next_f32()?;
                frames.push([u_min, u_max, v_min, v_max]);
            }
            total_frames_loaded += count;

            by_hash.insert(
                texture_hash,
                ClipRegionEntry {
                    texture_name,
                    texture_hash,
                    num_columns,
                    num_rows,
                    frames,
                },
            );
        }

        if total_frames_loaded != total_clip_regions {
            return Err(ClipRegionParseError::FrameCountMismatch {
                expected: total_clip_regions,
                actual: total_frames_loaded,
            });
        }

        Ok(Self { by_hash })
    }

    pub fn get_by_hash(&self, hash: u32) -> Option<&ClipRegionEntry> {
        self.by_hash.get(&hash)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&ClipRegionEntry> {
        let hash = crate::string_hash(name.as_bytes());
        self.by_hash.get(&hash)
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &ClipRegionEntry> {
        self.by_hash.values()
    }
}

#[derive(Error, Debug)]
pub enum ClipRegionParseError {
    #[error("ptxclipregions.dat is empty or contains only comments")]
    Empty,
    #[error("expected an integer at offset {offset}, got `{got}`")]
    BadInt { offset: usize, got: String },
    #[error("expected a float at offset {offset}, got `{got}`")]
    BadFloat { offset: usize, got: String },
    #[error("unexpected end of file at offset {offset}; needed `{needed}`")]
    UnexpectedEof { offset: usize, needed: &'static str },
    #[error("frame-count mismatch: header says {expected}, summed frame counts give {actual}")]
    FrameCountMismatch { expected: u32, actual: u32 },
    #[error("texture `{texture}` has overflowing frame count {columns} * {rows}")]
    FrameCountOverflow {
        texture: String,
        columns: u32,
        rows: u32,
    },
}

struct Tokenizer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn next_token(&mut self) -> Result<String, ClipRegionParseError> {
        loop {
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.pos >= self.src.len() {
                return Err(ClipRegionParseError::UnexpectedEof {
                    offset: self.pos,
                    needed: "token",
                });
            }
            if self.src[self.pos] == b'#' {
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            if self.pos + 1 < self.src.len()
                && self.src[self.pos] == b'/'
                && self.src[self.pos + 1] == b'/'
            {
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
        let start = self.pos;
        while self.pos < self.src.len() && !self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let token = std::str::from_utf8(&self.src[start..self.pos]).map_err(|_| {
            ClipRegionParseError::UnexpectedEof {
                offset: start,
                needed: "utf8 token",
            }
        })?;
        if token.is_empty() {
            return Err(ClipRegionParseError::Empty);
        }
        Ok(token.to_string())
    }

    fn next_u32(&mut self) -> Result<u32, ClipRegionParseError> {
        let start = self.pos;
        let token = self.next_token()?;
        token
            .parse::<u32>()
            .map_err(|_| ClipRegionParseError::BadInt {
                offset: start,
                got: token,
            })
    }

    fn next_f32(&mut self) -> Result<f32, ClipRegionParseError> {
        let start = self.pos;
        let token = self.next_token()?;
        token
            .parse::<f32>()
            .map_err(|_| ClipRegionParseError::BadFloat {
                offset: start,
                got: token,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_TEXTURE_DAT: &str = "\
2
5
blood_drop 2 2
0.0 0.5 0.0 0.5
0.5 1.0 0.0 0.5
0.0 0.5 0.5 1.0
0.5 1.0 0.5 1.0
solid_circle 1 1
0.0 1.0 0.0 1.0
";

    #[test]
    fn parses_two_texture_fixture() {
        let table = ClipRegionTable::parse(TWO_TEXTURE_DAT).unwrap();
        assert_eq!(table.len(), 2);

        let drop_entry = table.get_by_name("blood_drop").unwrap();
        assert_eq!(drop_entry.num_columns, 2);
        assert_eq!(drop_entry.num_rows, 2);
        assert_eq!(drop_entry.frames.len(), 4);
        assert_eq!(drop_entry.frames[0], [0.0, 0.5, 0.0, 0.5]);
        assert_eq!(drop_entry.frames[3], [0.5, 1.0, 0.5, 1.0]);

        let solid = table.get_by_name("solid_circle").unwrap();
        assert_eq!(solid.num_columns, 1);
        assert_eq!(solid.num_rows, 1);
        assert_eq!(solid.frames, vec![[0.0, 1.0, 0.0, 1.0]]);
    }

    #[test]
    fn lookup_by_hash_matches_by_name() {
        let table = ClipRegionTable::parse(TWO_TEXTURE_DAT).unwrap();
        let by_name = table.get_by_name("blood_drop").unwrap();
        let by_hash = table
            .get_by_hash(crate::string_hash(b"blood_drop"))
            .unwrap();
        assert_eq!(by_name.texture_hash, by_hash.texture_hash);
        assert_eq!(by_name.num_columns, by_hash.num_columns);
    }

    #[test]
    fn name_hash_is_case_insensitive() {
        let table = ClipRegionTable::parse(TWO_TEXTURE_DAT).unwrap();
        assert!(table.get_by_name("BLOOD_DROP").is_some());
        assert!(table.get_by_name("Blood_Drop").is_some());
    }

    #[test]
    fn frame_count_mismatch_is_an_error() {
        let bad = "\
2
4
blood_drop 2 2
0.0 0.5 0.0 0.5
0.5 1.0 0.0 0.5
0.0 0.5 0.5 1.0
0.5 1.0 0.5 1.0
solid_circle 1 1
0.0 1.0 0.0 1.0
";
        match ClipRegionTable::parse(bad) {
            Err(ClipRegionParseError::FrameCountMismatch { expected, actual }) => {
                assert_eq!(expected, 4);
                assert_eq!(actual, 5);
            }
            other => panic!("expected FrameCountMismatch, got {other:?}"),
        }
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let input = "\
# a comment line
2

5
// another comment
blood_drop 2 2
0.0 0.5 0.0 0.5
0.5 1.0 0.0 0.5
0.0 0.5 0.5 1.0
0.5 1.0 0.5 1.0
solid_circle 1 1
0.0 1.0 0.0 1.0
";
        let table = ClipRegionTable::parse(input).unwrap();
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn empty_input_errors() {
        assert!(matches!(
            ClipRegionTable::parse("   \n\n# nothing\n"),
            Err(ClipRegionParseError::UnexpectedEof { .. })
        ));
    }

    #[test]
    fn entries_iterates_all_textures() {
        let table = ClipRegionTable::parse(TWO_TEXTURE_DAT).unwrap();
        let names: Vec<&str> = table.entries().map(|e| e.texture_name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"blood_drop"));
        assert!(names.contains(&"solid_circle"));
    }
}
