//! GTA VI's radio in audio game data: a music body's stations with the track lists and tracks
//! they name, and a sounds body's streamed sounds with their lengths and audio streams. A track
//! names its streamed sound by index key, which joins a station to the streams it plays.
//!
//! A station's key is also the key of its name in the text tables. A track may name a text item
//! that spells its title and its artists as identifiers; the texts the game shows are keyed by a
//! track id the audio stream carries, which is not read here. No start time is stored, and a
//! list's order is the order it states its tracks: what its flags say of the order is not known.

use std::collections::BTreeMap;

use super::audio_data::{AudioData, AudioDataKind, AudioItem};
use super::audio_links::{layout, ItemLayout};

/// The class of a music body's station items.
pub const STATION_CLASS: u8 = 16;
/// The class of the track lists a station names.
pub const TRACK_LIST_CLASS: u8 = 17;
/// The class of the tracks a track list names.
pub const TRACK_CLASS: u8 = 15;
/// The class of a sounds body's streamed sounds.
pub const STREAMED_SOUND_CLASS: u8 = 8;
/// The class of an item that is a count and that many rows, which a track list may name its
/// tracks through.
pub const ROWS_CLASS: u8 = 2;
/// The class of a track's text: its title's name and its artists.
pub const TRACK_TEXT_CLASS: u8 = 12;
/// The class of an artist a track's text names.
pub const ARTIST_CLASS: u8 = 7;
/// The class of the list of every station, in the order it states them.
pub const STATION_LIST_CLASS: u8 = 14;
/// The class of a map from a key to an item: one whose every entry names a station lists stations.
pub const STATION_MAP_CLASS: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RadioError {
    #[error("GTA VI radio: stations are read from a music body, and this body holds {0}")]
    NotMusic(&'static str),
    #[error("GTA VI radio: streamed sounds are read from a sounds body, and this body holds {0}")]
    NotSounds(&'static str),
    #[error("GTA VI radio: the station {key:#010X} {why}")]
    Station { key: u32, why: String },
    #[error("GTA VI radio: the track list {key:#010X} {why}")]
    TrackList { key: u32, why: String },
    #[error("GTA VI radio: the track {key:#010X} {why}")]
    Track { key: u32, why: String },
    #[error("GTA VI radio: the track text {key:#010X} {why}")]
    TrackText { key: u32, why: String },
    #[error("GTA VI radio: the station list {key:#010X} {why}")]
    StationList { key: u32, why: String },
}

type Result<T> = std::result::Result<T, RadioError>;

/// A station: its index key, which it also states, and its track lists in the order stated.
// Its other fields are not read: a flags word, a float, reference rows whose roles are not
// known, and two counted arrays of rows after the track lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station {
    pub key: u32,
    /// The word after the key: a multiple of 1,000 that stations share. Its role is not known.
    pub band: u32,
    pub lists: Vec<TrackList>,
}

/// A track list a station names, with what it states where the body holds the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackList {
    pub key: u32,
    /// The word after the class byte: two bits a flag, `0b10` where a flag is not stated. The
    /// two lowest flags are set in some lists; what they say of the play order is not known.
    pub flags: Option<u32>,
    /// The key of the item the list names first, which lists of one kind share.
    pub category: Option<u32>,
    pub tracks: Option<Vec<Track>>,
}

/// A track a list names, with the index key of the sound it plays and of its text where the body
/// holds the track and the track names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Track {
    pub key: u32,
    pub sound: Option<u32>,
    pub text: Option<u32>,
}

/// A track's text: the name of its title and of each of its artists, identifiers as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackText {
    pub title: String,
    pub artists: Vec<String>,
}

/// Stations an item lists: every station in the order stated, or a map's in its keys' order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationList {
    pub key: u32,
    /// Whether the item is the list of every station, whose order is its own; a map's is its keys'.
    pub ordered: bool,
    pub stations: Vec<u32>,
}

/// A streamed sound: its length and, for each sound that plays one of its channels, that
/// sound's index key and the audio stream its wave is, where this body holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamedSound {
    pub key: u32,
    pub duration_ms: u32,
    pub channels: Vec<(u32, Option<u32>)>,
}

impl StreamedSound {
    /// The audio streams of its channels that the body names.
    pub fn streams(&self) -> impl Iterator<Item = u32> + '_ {
        self.channels.iter().filter_map(|c| c.1)
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The index key the reference field at `at` of `item` names; `None` where no field is there.
fn row_at(item: &AudioItem, at: usize) -> Option<u32> {
    let found = (item.references).binary_search_by_key(&at, |r| r.at as usize);
    found.ok().map(|i| item.references[i].key)
}

fn bytes_of<'a>(data: &'a AudioData, item: &AudioItem) -> &'a [u8] {
    let start = item.offset as usize;
    (data.data.get(start..start + item.span as usize)).unwrap_or_default()
}

/// A station's band and its track lists' keys.
// +8 its own key, +12 the band, +60 a count of 12-byte track list entries from +64, each a
// reference row first; then a count of 8-byte entries, and a count of 12-byte entries.
fn station(item: &AudioItem, bytes: &[u8]) -> std::result::Result<(u32, Vec<u32>), String> {
    let word =
        |at: usize| u32_at(bytes, at).ok_or_else(|| format!("ends before the word at +{at}"));
    if word(8)? != item.key {
        return Err("does not state its own key at +8".into());
    }
    let band = word(12)?;
    // Each array's count, then where the array ends: refused past the bytes to the next item.
    let after = |count_at: usize, stride: usize| -> std::result::Result<(usize, usize), String> {
        let n = word(count_at)? as usize;
        let end = (n.checked_mul(stride))
            .and_then(|len| len.checked_add(count_at + 4))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| format!("counts {n} entries at +{count_at}, past its end"))?;
        Ok((n, end))
    };
    let (lists, lists_end) = after(60, 12)?;
    let (_, pairs_end) = after(lists_end, 8)?;
    after(pairs_end, 12)?;
    let keys = (0..lists)
        .map(|i| {
            let at = 64 + 12 * i;
            row_at(item, at).ok_or_else(|| format!("holds no reference row at +{at}"))
        })
        .collect::<std::result::Result<_, _>>()?;
    Ok((band, keys))
}

/// The keys an item of rows names: a count at +8, then a row for each.
fn rows_item(item: &AudioItem, bytes: &[u8]) -> std::result::Result<Vec<u32>, String> {
    let count = u32_at(bytes, 8).ok_or("ends before its count at +8")? as usize;
    if (count.checked_mul(4)).is_none_or(|len| len > bytes.len().saturating_sub(12)) {
        return Err(format!("counts {count} rows, past its end"));
    }
    (0..count)
        .map(|i| {
            let at = 12 + 4 * i;
            row_at(item, at).ok_or_else(|| format!("holds no reference row at +{at}"))
        })
        .collect()
}

/// The NUL-padded name in the `len` bytes at `at` of `bytes`.
fn name_at(bytes: &[u8], at: usize, len: usize) -> Option<String> {
    let field = bytes.get(at..at + len)?;
    let end = field.iter().position(|b| *b == 0)?;
    let padded = field[end..].iter().all(|b| *b == 0);
    let name = std::str::from_utf8(&field[..end]).ok()?;
    (padded && !name.is_empty()).then(|| name.to_string())
}

/// A track list's tracks' keys stated in the list itself: a `u8` count at +72, three zero bytes,
/// then a row for each. A list may also name an item of rows at +68, which the caller reads.
fn track_list(item: &AudioItem, bytes: &[u8]) -> std::result::Result<Vec<u32>, String> {
    let head = (bytes.get(72..76)).ok_or("ends before its count at +72")?;
    if head[1..] != [0, 0, 0] {
        return Err("states a count at +72 that is not one byte".into());
    }
    let count = usize::from(head[0]);
    if 76 + 4 * count > bytes.len() {
        return Err(format!("counts {count} tracks, past its end"));
    }
    (0..count)
        .map(|i| {
            let at = 76 + 4 * i;
            row_at(item, at).ok_or_else(|| format!("holds no reference row at +{at}"))
        })
        .collect()
}

/// The stations of the music body `music`, each with its track lists and their tracks.
pub fn stations(music: &AudioData) -> Result<Vec<Station>> {
    if music.kind != AudioDataKind::Music {
        return Err(RadioError::NotMusic(music.kind.label()));
    }
    let items = music.items();
    let by_key: BTreeMap<u32, &AudioItem> = items.iter().map(|i| (i.key, i)).collect();
    // An item of another class under a key a list or a track is named by is refused.
    let of_class = |key: u32, class: u8| match by_key.get(&key) {
        None => Ok(None),
        Some(item) if item.class == Some(class) => Ok(Some(*item)),
        Some(item) => Err(format!(
            "is an item of class {}, where class {class} is named",
            item.class.unwrap_or(0)
        )),
    };
    let track = |key: u32| -> Result<Track> {
        let failed = |why: String| RadioError::Track { key, why };
        let Some(item) = of_class(key, TRACK_CLASS).map_err(failed)? else {
            return Ok(Track {
                key,
                sound: None,
                text: None,
            });
        };
        let sound =
            row_at(item, 12).ok_or_else(|| failed("holds no reference row at +12".to_string()))?;
        Ok(Track {
            key,
            sound: Some(sound),
            text: row_at(item, 16),
        })
    };
    let list = |key: u32| -> Result<TrackList> {
        let failed = |why: String| RadioError::TrackList { key, why };
        let Some(item) = of_class(key, TRACK_LIST_CLASS).map_err(failed)? else {
            return Ok(TrackList {
                key,
                flags: None,
                category: None,
                tracks: None,
            });
        };
        let bytes = bytes_of(music, item);
        let mut keys = track_list(item, bytes).map_err(failed)?;
        // The tracks an item of rows lists follow those the list states itself.
        let rows = match row_at(item, 68) {
            Some(rows) => of_class(rows, ROWS_CLASS).map_err(failed)?,
            None => None,
        };
        if let Some(rows) = rows {
            keys.extend(rows_item(rows, bytes_of(music, rows)).map_err(failed)?);
        }
        let tracks = (keys.into_iter().map(&track)).collect::<Result<Vec<Track>>>()?;
        Ok(TrackList {
            key,
            flags: u32_at(bytes, 1),
            category: row_at(item, 8),
            tracks: Some(tracks),
        })
    };
    (items.iter())
        .filter(|item| item.class == Some(STATION_CLASS))
        .map(|item| {
            let key = item.key;
            let (band, keys) = station(item, bytes_of(music, item))
                .map_err(|why| RadioError::Station { key, why })?;
            let lists = (keys.into_iter().map(&list)).collect::<Result<Vec<TrackList>>>()?;
            Ok(Station { key, band, lists })
        })
        .collect()
}

/// The text of each track text item of the music body `music`, by the item's index key.
// A text: +8 its own key, +12 its title's name in 68 bytes, +96 a count of 8-byte entries from
// +100, each a row naming an artist. An artist: its name in the 67 bytes from +5.
pub fn track_texts(music: &AudioData) -> Result<BTreeMap<u32, TrackText>> {
    if music.kind != AudioDataKind::Music {
        return Err(RadioError::NotMusic(music.kind.label()));
    }
    let items = music.items();
    let artists: BTreeMap<u32, &AudioItem> = (items.iter())
        .filter(|i| i.class == Some(ARTIST_CLASS))
        .map(|i| (i.key, i))
        .collect();
    let text = |item: &AudioItem| -> std::result::Result<TrackText, String> {
        let bytes = bytes_of(music, item);
        if u32_at(bytes, 8) != Some(item.key) {
            return Err("does not state its own key at +8".into());
        }
        let title = name_at(bytes, 12, 68).ok_or("holds no NUL-padded name at +12")?;
        let count = u32_at(bytes, 96).ok_or("ends before its count at +96")? as usize;
        if (count.checked_mul(8)).is_none_or(|len| len > bytes.len().saturating_sub(100)) {
            return Err(format!("counts {count} artists, past its end"));
        }
        let mut named = Vec::with_capacity(count);
        for i in 0..count {
            let at = 100 + 8 * i;
            let key = row_at(item, at).ok_or(format!("holds no reference row at +{at}"))?;
            // An artist the body does not hold is left out.
            if let Some(artist) = artists.get(&key) {
                let name = name_at(bytes_of(music, artist), 5, 67);
                named.push(name.ok_or(format!(
                    "names the artist {key:#010X}, which holds no NUL-padded name at +5"
                ))?);
            }
        }
        Ok(TrackText {
            title,
            artists: named,
        })
    };
    (items.iter())
        .filter(|item| item.class == Some(TRACK_TEXT_CLASS))
        .map(|item| {
            let key = item.key;
            let read = text(item).map_err(|why| RadioError::TrackText { key, why })?;
            Ok((key, read))
        })
        .collect()
}

/// The stations the music body `music` lists: the list of every station, and each map whose
/// every entry names a station.
// The list: a `u8` count at +5, two zero bytes, then a row for each. A map: a count at +8, then
// 8-byte entries of a key and a row or -1, in the keys' order.
pub fn station_lists(music: &AudioData) -> Result<Vec<StationList>> {
    if music.kind != AudioDataKind::Music {
        return Err(RadioError::NotMusic(music.kind.label()));
    }
    let items = music.items();
    let stations: std::collections::BTreeSet<u32> = (items.iter())
        .filter(|i| i.class == Some(STATION_CLASS))
        .map(|i| i.key)
        .collect();
    let station_at =
        |item: &AudioItem, at: usize| row_at(item, at).filter(|k| stations.contains(k));
    let mut out = Vec::new();
    for item in &items {
        let bytes = bytes_of(music, item);
        let key = item.key;
        match item.class {
            Some(STATION_LIST_CLASS) => {
                let failed = |why: String| RadioError::StationList { key, why };
                let head = (bytes.get(5..8)).ok_or_else(|| failed("ends before +8".into()))?;
                let count = usize::from(head[0]);
                if head[1..] != [0, 0] || bytes.len() < 8 + 4 * count {
                    return Err(failed(format!("counts {count} stations, past its end")));
                }
                let listed = (0..count)
                    .map(|i| {
                        let at = 8 + 4 * i;
                        station_at(item, at)
                            .ok_or_else(|| failed(format!("names no station at +{at}")))
                    })
                    .collect::<Result<Vec<u32>>>()?;
                out.push(StationList {
                    key,
                    ordered: true,
                    stations: listed,
                });
            }
            // A map of another shape, or of other items, is no list of stations.
            Some(STATION_MAP_CLASS) => {
                let count = u32_at(bytes, 8).unwrap_or(0) as usize;
                if count == 0 || count.checked_mul(8) != bytes.len().checked_sub(12) {
                    continue;
                }
                let listed: Option<Vec<u32>> =
                    (0..count).map(|i| station_at(item, 16 + 8 * i)).collect();
                if let Some(listed) = listed {
                    out.push(StationList {
                        key,
                        ordered: false,
                        stations: listed,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The streamed sounds of the sounds body `sounds` by index key, and how many items of their
/// class no layout reads.
pub fn streamed_sounds(sounds: &AudioData) -> Result<(BTreeMap<u32, StreamedSound>, usize)> {
    if sounds.kind != AudioDataKind::Sounds {
        return Err(RadioError::NotSounds(sounds.kind.label()));
    }
    let items = sounds.items();
    let by_key: BTreeMap<u32, &AudioItem> = items.iter().map(|i| (i.key, i)).collect();
    let mut out = BTreeMap::new();
    let mut unread = 0;
    for item in items
        .iter()
        .filter(|i| i.class == Some(STREAMED_SOUND_CLASS))
    {
        let Some(ItemLayout::StreamingSound { duration, rows, .. }) = layout(sounds, item) else {
            unread += 1;
            continue;
        };
        let channels = (rows.iter())
            .filter_map(|row| sounds.references.get(*row as usize))
            .map(|&(key, _)| {
                let wave = by_key.get(&key).and_then(|child| child.waves.first());
                (key, wave.map(|w| w.stream_id()))
            })
            .collect();
        let sound = StreamedSound {
            key: item.key,
            duration_ms: duration,
            channels,
        };
        out.insert(item.key, sound);
    }
    Ok((out, unread))
}

impl Station {
    /// Each track of the station's lists, in order, with the streamed sound of `sounds` it plays.
    pub fn tracks<'a>(
        &'a self,
        sounds: &'a BTreeMap<u32, StreamedSound>,
    ) -> impl Iterator<Item = (&'a Track, Option<&'a StreamedSound>)> {
        (self.lists.iter())
            .flat_map(|list| list.tracks.iter().flatten())
            .map(move |track| (track, track.sound.and_then(|key| sounds.get(&key))))
    }
}

#[cfg(test)]
#[path = "audio_radio_tests.rs"]
mod tests;
