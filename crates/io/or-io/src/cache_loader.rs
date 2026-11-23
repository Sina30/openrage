//! The streaming cache loader's `*_cache_y.dat`: a text header and archive
//! date list, then one module per store of fixed-size little-endian records.
//!
//! `gta5_cache_y.dat` covers the base game and each DLC pack ships its own
//! `<pack>_<hash>_cache_y.dat`; both are the same layout.

use crate::error::{IoError, Result};

/// The only layout version read: every shipped file states 46.
pub const VERSION: u32 = 46;
const HEADER_LEN: usize = 100;
const VERSION_TOKEN: &str = "[VERSION]";
const DATES_START: &[u8] = b"<fileDates>\n";
const DATES_END: &[u8] = b"</fileDates>\n";
const MODULE_START: &[u8] = b"<module>\n";
const MODULE_END: &[u8] = b"</module>\n";

const MAP_DATA_STORE: &str = "fwMapDataStore";
const INTERIOR_PROXY: &str = "CInteriorProxy";
const BOUNDS_STORE: &str = "BoundsStore";

/// One cache file: its archive dates and the three stores' records, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheFile {
    pub version: u32,
    pub file_dates: Vec<CacheFileDate>,
    pub map_data_store: Vec<FwMapDataCacheEntry>,
    pub interior_proxies: Vec<CInteriorProxyCacheEntry>,
    pub bounds_store: Vec<BoundsStoreCacheEntry>,
}

impl Default for CacheFile {
    fn default() -> Self {
        Self {
            version: VERSION,
            file_dates: Vec::new(),
            map_data_store: Vec::new(),
            interior_proxies: Vec::new(),
            bounds_store: Vec::new(),
        }
    }
}

/// One `<fileDates>` line: an archive's name hash and its file time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheFileDate {
    pub name_hash: u32,
    /// A Windows `FILETIME`: 100 ns ticks since 1601.
    pub file_time: u64,
    /// A third number some files add to every line. Always a multiple of 104,
    /// rising by 104 per line; what it counts is not measured.
    pub third_column: Option<u64>,
}

/// One map data file's streaming and physical extents (64 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FwMapDataCacheEntry {
    pub name_hash: u32,
    pub parent_name_hash: u32,
    pub content_flags: u32,
    pub str_min: [f32; 3],
    pub str_max: [f32; 3],
    pub phy_min: [f32; 3],
    pub phy_max: [f32; 3],
    pub dynamic_streaming: bool,
    pub contains_block_info: bool,
    pub is_parent: bool,
}

/// One interior proxy's placement and bounds (104 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CInteriorProxyCacheEntry {
    pub group_id: u32,
    pub floor_id: u32,
    pub num_exit_portals: u32,
    pub proxy_name_hash: u32,
    pub map_data_store_name_hash: u32,
    pub pos: [f32; 3],
    /// X, Y, Z, W.
    pub quat: [f32; 4],
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    /// The engine's 32-byte name slot. Shipped files hold no text here but
    /// stale memory (pointers among it), so it is kept as stated.
    pub proxy_name: [u8; 32],
}

/// One static bounds file's box and asset type (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BoundsStoreCacheEntry {
    pub hash_key: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub asset_type: u8,
    /// The record's three trailing bytes: zero, or `7f 00 00` throughout some
    /// Legacy files; kept as stated.
    pub pad: [u8; 3],
}

impl FwMapDataCacheEntry {
    const SIZE: usize = 64;

    fn read(b: &[u8]) -> Result<Self> {
        let flag = |at: usize| match b[at] {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(refuse(format!(
                "{MAP_DATA_STORE}: a flag byte is {other}, not 0 or 1"
            ))),
        };
        if b[63] != 0 {
            return Err(refuse(format!(
                "{MAP_DATA_STORE}: a record's pad byte is {}",
                b[63]
            )));
        }
        Ok(Self {
            name_hash: u32_at(b, 0),
            parent_name_hash: u32_at(b, 4),
            content_flags: u32_at(b, 8),
            str_min: vec3_at(b, 12),
            str_max: vec3_at(b, 24),
            phy_min: vec3_at(b, 36),
            phy_max: vec3_at(b, 48),
            dynamic_streaming: flag(60)?,
            contains_block_info: flag(61)?,
            is_parent: flag(62)?,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        for v in [self.name_hash, self.parent_name_hash, self.content_flags] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [self.str_min, self.str_max, self.phy_min, self.phy_max] {
            put_floats(out, &v);
        }
        out.extend_from_slice(&[
            u8::from(self.dynamic_streaming),
            u8::from(self.contains_block_info),
            u8::from(self.is_parent),
            0,
        ]);
    }
}

impl CInteriorProxyCacheEntry {
    const SIZE: usize = 104;

    fn read(b: &[u8]) -> Result<Self> {
        Ok(Self {
            group_id: u32_at(b, 0),
            floor_id: u32_at(b, 4),
            num_exit_portals: u32_at(b, 8),
            proxy_name_hash: u32_at(b, 12),
            map_data_store_name_hash: u32_at(b, 16),
            pos: vec3_at(b, 20),
            quat: [f32_at(b, 32), f32_at(b, 36), f32_at(b, 40), f32_at(b, 44)],
            bbox_min: vec3_at(b, 48),
            bbox_max: vec3_at(b, 60),
            proxy_name: b[72..104].try_into().expect("32 bytes"),
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        for v in [
            self.group_id,
            self.floor_id,
            self.num_exit_portals,
            self.proxy_name_hash,
            self.map_data_store_name_hash,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        put_floats(out, &self.pos);
        put_floats(out, &self.quat);
        put_floats(out, &self.bbox_min);
        put_floats(out, &self.bbox_max);
        out.extend_from_slice(&self.proxy_name);
    }
}

impl BoundsStoreCacheEntry {
    const SIZE: usize = 32;

    fn read(b: &[u8]) -> Result<Self> {
        Ok(Self {
            hash_key: u32_at(b, 0),
            min: vec3_at(b, 4),
            max: vec3_at(b, 16),
            asset_type: b[28],
            pad: [b[29], b[30], b[31]],
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.hash_key.to_le_bytes());
        put_floats(out, &self.min);
        put_floats(out, &self.max);
        out.push(self.asset_type);
        out.extend_from_slice(&self.pad);
    }
}

impl CacheFile {
    /// Reads a whole cache file, refusing a header, line or module it cannot save back as stated.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let header = bytes
            .get(..HEADER_LEN)
            .ok_or_else(|| refuse(format!("{} bytes is shorter than the header", bytes.len())))?;
        let version = read_header(header)?;

        let mut at = HEADER_LEN;
        expect(bytes, &mut at, DATES_START)?;
        let end = find(&bytes[at..], DATES_END)
            .filter(|&end| end == 0 || bytes[at + end - 1] == b'\n')
            .ok_or_else(|| refuse("no `</fileDates>` line".into()))?;
        let file_dates = std::str::from_utf8(&bytes[at..at + end])
            .map_err(|_| refuse("a `<fileDates>` line is not text".into()))?
            .lines()
            .map(read_date)
            .collect::<Result<Vec<_>>>()?;
        at += end + DATES_END.len();

        let mut file = Self {
            version,
            file_dates,
            ..Self::default()
        };
        for name in [MAP_DATA_STORE, INTERIOR_PROXY, BOUNDS_STORE] {
            let records = read_module(bytes, &mut at, name)?;
            match name {
                MAP_DATA_STORE => {
                    file.map_data_store = records_of(records, FwMapDataCacheEntry::SIZE, name)?
                        .map(FwMapDataCacheEntry::read)
                        .collect::<Result<_>>()?
                }
                INTERIOR_PROXY => {
                    file.interior_proxies =
                        records_of(records, CInteriorProxyCacheEntry::SIZE, name)?
                            .map(CInteriorProxyCacheEntry::read)
                            .collect::<Result<_>>()?
                }
                _ => {
                    file.bounds_store = records_of(records, BoundsStoreCacheEntry::SIZE, name)?
                        .map(BoundsStoreCacheEntry::read)
                        .collect::<Result<_>>()?
                }
            }
        }
        if at != bytes.len() {
            return Err(refuse(format!(
                "{} bytes follow the last module",
                bytes.len() - at
            )));
        }
        Ok(file)
    }

    /// Writes the file back: the header, the dates, then the three modules.
    pub fn save(&self) -> Result<Vec<u8>> {
        let mut out = format!("{VERSION_TOKEN}\n{}\n", self.version).into_bytes();
        if out.len() > HEADER_LEN {
            return Err(refuse(format!(
                "version {} does not fit the header",
                self.version
            )));
        }
        out.resize(HEADER_LEN, 0);
        out.extend_from_slice(DATES_START);
        for date in &self.file_dates {
            let line = match date.third_column {
                Some(third) => format!("{} {} {third}\n", date.name_hash, date.file_time),
                None => format!("{} {}\n", date.name_hash, date.file_time),
            };
            out.extend_from_slice(line.as_bytes());
        }
        out.extend_from_slice(DATES_END);

        let mut module = |name: &str, len: usize, write: &dyn Fn(&mut Vec<u8>)| -> Result<()> {
            out.extend_from_slice(MODULE_START);
            out.extend_from_slice(name.as_bytes());
            out.push(b'\n');
            let len = u32::try_from(len)
                .map_err(|_| refuse(format!("{name}: {len} bytes passes u32")))?;
            out.extend_from_slice(&len.to_le_bytes());
            write(&mut out);
            out.extend_from_slice(MODULE_END);
            Ok(())
        };
        module(
            MAP_DATA_STORE,
            self.map_data_store.len() * FwMapDataCacheEntry::SIZE,
            &|out| self.map_data_store.iter().for_each(|e| e.write(out)),
        )?;
        module(
            INTERIOR_PROXY,
            self.interior_proxies.len() * CInteriorProxyCacheEntry::SIZE,
            &|out| self.interior_proxies.iter().for_each(|e| e.write(out)),
        )?;
        module(
            BOUNDS_STORE,
            self.bounds_store.len() * BoundsStoreCacheEntry::SIZE,
            &|out| self.bounds_store.iter().for_each(|e| e.write(out)),
        )?;
        Ok(out)
    }
}

fn read_header(header: &[u8]) -> Result<u32> {
    let text_len = header.iter().position(|&b| b == 0).unwrap_or(HEADER_LEN);
    if header[text_len..].iter().any(|&b| b != 0) {
        return Err(refuse("the header's padding is not zero".into()));
    }
    let text = std::str::from_utf8(&header[..text_len])
        .map_err(|_| refuse("the header is not text".into()))?;
    let version = text
        .strip_prefix(VERSION_TOKEN)
        .and_then(|rest| rest.strip_prefix('\n'))
        .and_then(|rest| rest.strip_suffix('\n'))
        .filter(|number| canonical(number))
        .and_then(|number| number.parse::<u32>().ok())
        .ok_or_else(|| {
            refuse(format!(
                "the header is {text:?}, not `[VERSION]` and a number"
            ))
        })?;
    if version != VERSION {
        return Err(refuse(format!(
            "version {version}; only version {VERSION} is read"
        )));
    }
    Ok(version)
}

fn read_date(line: &str) -> Result<CacheFileDate> {
    let bad = || {
        refuse(format!(
            "the date line {line:?} is not two or three numbers"
        ))
    };
    let fields: Vec<&str> = line.split(' ').collect();
    if !(2..=3).contains(&fields.len()) || !fields.iter().all(|f| canonical(f)) {
        return Err(bad());
    }
    Ok(CacheFileDate {
        name_hash: fields[0].parse().map_err(|_| bad())?,
        file_time: fields[1].parse().map_err(|_| bad())?,
        third_column: match fields.get(2) {
            Some(f) => Some(f.parse().map_err(|_| bad())?),
            None => None,
        },
    })
}

/// A decimal number a save prints back the same: digits with no leading zero.
fn canonical(number: &str) -> bool {
    !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && (number == "0" || !number.starts_with('0'))
}

fn read_module<'a>(bytes: &'a [u8], at: &mut usize, name: &str) -> Result<&'a [u8]> {
    expect(bytes, at, MODULE_START)?;
    expect(bytes, at, name.as_bytes())?;
    expect(bytes, at, b"\n")?;
    let len = bytes
        .get(*at..*at + 4)
        .map(|b| u32_at(b, 0) as usize)
        .ok_or_else(|| refuse(format!("{name}: the size is cut off")))?;
    *at += 4;
    let records = bytes
        .get(*at..*at + len)
        .ok_or_else(|| refuse(format!("{name}: {len} bytes are stated and fewer follow")))?;
    *at += len;
    expect(bytes, at, MODULE_END)?;
    Ok(records)
}

fn records_of<'a>(
    bytes: &'a [u8],
    size: usize,
    name: &str,
) -> Result<std::slice::ChunksExact<'a, u8>> {
    if !bytes.len().is_multiple_of(size) {
        return Err(refuse(format!(
            "{name}: {} bytes is not a whole number of {size}-byte records",
            bytes.len()
        )));
    }
    Ok(bytes.chunks_exact(size))
}

fn expect(bytes: &[u8], at: &mut usize, token: &[u8]) -> Result<()> {
    if bytes.get(*at..*at + token.len()) != Some(token) {
        return Err(refuse(format!(
            "expected {:?} at byte {at}",
            String::from_utf8_lossy(token)
        )));
    }
    *at += token.len();
    Ok(())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("four bytes"))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

fn vec3_at(b: &[u8], at: usize) -> [f32; 3] {
    [f32_at(b, at), f32_at(b, at + 4), f32_at(b, at + 8)]
}

fn put_floats(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("cache file: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CacheFile {
        CacheFile {
            file_dates: vec![
                CacheFileDate {
                    name_hash: 2740459947,
                    file_time: 130680580712018938,
                    third_column: Some(8944),
                },
                CacheFileDate {
                    name_hash: 7,
                    file_time: 0,
                    third_column: None,
                },
            ],
            map_data_store: vec![FwMapDataCacheEntry {
                name_hash: 1,
                parent_name_hash: 2,
                content_flags: 16,
                str_min: [-1.0, -2.0, -3.0],
                str_max: [1.0, 2.0, 3.0],
                dynamic_streaming: true,
                is_parent: true,
                ..Default::default()
            }],
            interior_proxies: vec![CInteriorProxyCacheEntry {
                num_exit_portals: 6,
                quat: [0.0, 0.0, 0.0, 1.0],
                proxy_name: [0x7f; 32],
                ..Default::default()
            }],
            bounds_store: vec![BoundsStoreCacheEntry {
                hash_key: 9,
                asset_type: 2,
                pad: [0x7f, 0, 0],
                ..Default::default()
            }],
            ..CacheFile::default()
        }
    }

    #[test]
    fn a_cache_file_saves_its_layout_and_reopens() {
        let file = sample();
        let bytes = file.save().expect("saves");
        assert!(bytes.starts_with(b"[VERSION]\n46\n\0"));
        assert_eq!(&bytes[HEADER_LEN..][..12], b"<fileDates>\n");
        let dates = b"<fileDates>\n2740459947 130680580712018938 8944\n7 0\n</fileDates>\n";
        let modules = HEADER_LEN + dates.len();
        assert_eq!(&bytes[HEADER_LEN..modules], dates);
        assert_eq!(&bytes[modules..][..24], b"<module>\nfwMapDataStore\n");
        assert_eq!(u32_at(&bytes, modules + 24), 64);
        assert_eq!(
            bytes.len(),
            modules + 3 * 20 + "fwMapDataStoreCInteriorProxyBoundsStore".len() + 3 * 4 + 200
        );
        assert_eq!(CacheFile::open(&bytes).expect("reopens"), file);
    }

    #[test]
    fn bytes_a_save_would_not_reproduce_are_refused() {
        let bytes = sample().save().expect("saves");
        let refused = |bytes: &[u8]| CacheFile::open(bytes).unwrap_err().to_string();

        let mut padded_number = bytes.clone();
        let at = find(&padded_number, b" 8944").expect("the third column") + 1;
        padded_number[at] = b'0';
        assert!(refused(&padded_number).contains("date line"));

        let mut version = bytes.clone();
        version[10] = b'5';
        assert!(refused(&version).contains("version 56"));

        let mut flag = bytes.clone();
        let record = find(&flag, b"fwMapDataStore\n").expect("the module") + 15 + 4;
        flag[record + 60] = 2;
        assert!(refused(&flag).contains("flag byte"));

        let mut short = bytes.clone();
        let size = find(&short, b"BoundsStore\n").expect("the module") + 12;
        short[size] = 64;
        assert!(refused(&short).contains("BoundsStore"));

        assert!(refused(&bytes[..bytes.len() - 1]).contains("</module>"));
        assert!(refused(&[bytes.as_slice(), b"x"].concat()).contains("follow"));
    }
}
