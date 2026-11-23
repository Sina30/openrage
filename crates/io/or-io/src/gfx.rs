//! `.gfx` — a Scaleform movie: an SWF body under a `GFX` signature, or `CFX`
//! when the body after the eight-byte header is a zlib stream.
//!
//! The movie is kept as its tags, each a code and its body bytes, so a tag
//! this does not interpret round-trips untouched. A tag written in the long
//! form although its length fits the short one keeps that form, and a
//! compressed movie whose tags are unchanged keeps its original zlib stream,
//! since no zlib level reproduces the stream shipped files carry.
//!
//! The same body under `FWS` or `CWS` is a standard SWF, which is how a movie
//! goes to an external editor and comes back.

use std::io::{Read, Write};

use crate::error::{IoError, Result};

/// The tag that ends a movie's tag list.
pub const END: u16 = 0;

/// A short tag header's six-bit length, and the mark of the long form.
const LONG_LENGTH: usize = 0x3F;

/// The highest code a tag header's ten bits hold.
const MAX_CODE: u16 = 0x3FF;

/// Bytes before the body: three signature bytes, the version, the length.
const HEADER_BYTES: usize = 8;

/// One tag: its code and the bytes after its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub code: u16,
    /// Written with the long header even when the length fits the short one.
    pub long: bool,
    pub body: Vec<u8>,
}

/// A texture a movie names by file rather than embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureRef {
    /// The index of the tag that names it.
    pub tag: usize,
    pub id: u32,
    /// The name ActionScript knows it by; a gradient has none.
    pub export_name: Option<String>,
    pub file_name: String,
}

/// A character a movie takes from another movie by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub tag: usize,
    pub url: String,
    pub id: u16,
    pub name: String,
}

/// A Scaleform movie, opened from a `.gfx` or from the SWF it exports to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GfxMovie {
    /// The SWF version byte.
    pub version: u8,
    /// Whether the body after the header is a zlib stream.
    pub compressed: bool,
    /// The frame rectangle, frame rate and frame count, as stored.
    pub frame_header: Vec<u8>,
    pub tags: Vec<Tag>,
    /// The zlib stream the movie was read from, and the body it inflated to.
    original_stream: Option<(Vec<u8>, Vec<u8>)>,
}

impl GfxMovie {
    /// Opens a `.gfx` (`GFX`, `CFX`) or an SWF (`FWS`, `CWS`).
    pub fn open(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_BYTES {
            return Err(refuse(format!(
                "{} bytes is shorter than the 8-byte header",
                bytes.len()
            )));
        }
        let compressed = match &bytes[..3] {
            b"GFX" | b"FWS" => false,
            b"CFX" | b"CWS" => true,
            b"ZWS" => {
                return Err(refuse(
                    "an LZMA-compressed SWF (`ZWS`) has no .gfx form; save it from the \
                     editor uncompressed or zlib-compressed"
                        .to_string(),
                ))
            }
            other => {
                return Err(refuse(format!(
                    "signature {:?} is none of GFX, CFX, FWS or CWS",
                    String::from_utf8_lossy(other)
                )))
            }
        };
        let version = bytes[3];
        let stated = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let stored = &bytes[HEADER_BYTES..];
        let inflated;
        let body = if compressed {
            // One byte past what the header states is enough to refuse a longer body.
            let limit = stated.saturating_sub(HEADER_BYTES) as u64 + 1;
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(stored)
                .take(limit)
                .read_to_end(&mut out)
                .map_err(|e| refuse(format!("the zlib body does not inflate: {e}")))?;
            inflated = out;
            &inflated[..]
        } else {
            stored
        };
        if HEADER_BYTES + body.len() != stated {
            return Err(refuse(format!(
                "the header states {stated} bytes and the movie holds {}",
                HEADER_BYTES + body.len()
            )));
        }
        let (frame_header, tags) = read_body(body)?;
        let original_stream = compressed.then(|| (stored.to_vec(), body.to_vec()));
        Ok(Self {
            version,
            compressed,
            frame_header,
            tags,
            original_stream,
        })
    }

    /// The movie as a `.gfx`.
    pub fn save(&self) -> Result<Vec<u8>> {
        self.save_with(if self.compressed { b"CFX" } else { b"GFX" })
    }

    /// The movie as a standard SWF, the same body under `FWS` or `CWS`.
    pub fn save_swf(&self) -> Result<Vec<u8>> {
        self.save_with(if self.compressed { b"CWS" } else { b"FWS" })
    }

    fn save_with(&self, signature: &[u8; 3]) -> Result<Vec<u8>> {
        let body = self.body()?;
        let total = u32::try_from(HEADER_BYTES + body.len())
            .map_err(|_| refuse(format!("{} bytes is past a u32", body.len())))?;
        let mut out = Vec::with_capacity(HEADER_BYTES + body.len());
        out.extend_from_slice(signature);
        out.push(self.version);
        out.extend_from_slice(&total.to_le_bytes());
        if !self.compressed {
            out.extend_from_slice(&body);
            return Ok(out);
        }
        match &self.original_stream {
            Some((stream, inflated)) if *inflated == body => out.extend_from_slice(stream),
            _ => {
                let mut encoder = flate2::write::ZlibEncoder::new(out, flate2::Compression::best());
                encoder.write_all(&body).map_err(IoError::Io)?;
                out = encoder.finish().map_err(IoError::Io)?;
            }
        }
        Ok(out)
    }

    fn body(&self) -> Result<Vec<u8>> {
        let mut out = self.frame_header.clone();
        for (i, tag) in self.tags.iter().enumerate() {
            if tag.code > MAX_CODE {
                return Err(refuse(format!(
                    "tag {i}'s code {} does not fit a tag header's ten bits",
                    tag.code
                )));
            }
            let len = tag.body.len();
            if tag.long || len >= LONG_LENGTH {
                let len = u32::try_from(len)
                    .map_err(|_| refuse(format!("tag {i} is {len} bytes, past a u32")))?;
                out.extend_from_slice(&((tag.code << 6) | LONG_LENGTH as u16).to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
            } else {
                out.extend_from_slice(&((tag.code << 6) | len as u16).to_le_bytes());
            }
            out.extend_from_slice(&tag.body);
        }
        Ok(out)
    }

    /// Every texture an external-image or external-gradient tag names.
    pub fn texture_refs(&self) -> Vec<TextureRef> {
        let mut refs = Vec::new();
        for (i, tag) in self.tags.iter().enumerate() {
            let mut at = Cursor(&tag.body);
            let found = match tag.code {
                // id u32, format u16, width u16, height u16, export name, file name.
                1009 => (|| {
                    let id = at.u32()?;
                    at.skip(6)?;
                    let export_name = at.pstring()?;
                    Some(TextureRef {
                        tag: i,
                        id,
                        export_name: Some(export_name),
                        file_name: at.pstring()?,
                    })
                })(),
                // id u16, format u16, size u16, file name.
                1003 => (|| {
                    let id = at.u16()?.into();
                    at.skip(4)?;
                    Some(TextureRef {
                        tag: i,
                        id,
                        export_name: None,
                        file_name: at.pstring()?,
                    })
                })(),
                _ => continue,
            };
            refs.extend(found);
        }
        refs
    }

    /// Every character an `ImportAssets` or `ImportAssets2` tag takes from another movie.
    pub fn imports(&self) -> Vec<Import> {
        let mut imports = Vec::new();
        for (i, tag) in self.tags.iter().enumerate() {
            if tag.code != 57 && tag.code != 71 {
                continue;
            }
            let mut at = Cursor(&tag.body);
            let _ = (|| {
                let url = at.cstring()?;
                if tag.code == 71 {
                    at.skip(2)?;
                }
                for _ in 0..at.u16()? {
                    let id = at.u16()?;
                    imports.push(Import {
                        tag: i,
                        url: url.clone(),
                        id,
                        name: at.cstring()?,
                    });
                }
                Some(())
            })();
        }
        imports
    }
}

impl Tag {
    /// The tag's name in the SWF specification or Scaleform's extension, if it has one.
    pub fn name(&self) -> Option<&'static str> {
        Some(match self.code {
            0 => "End",
            1 => "ShowFrame",
            2 => "DefineShape",
            4 => "PlaceObject",
            5 => "RemoveObject",
            6 => "DefineBits",
            7 => "DefineButton",
            8 => "JPEGTables",
            9 => "SetBackgroundColor",
            10 => "DefineFont",
            11 => "DefineText",
            12 => "DoAction",
            13 => "DefineFontInfo",
            14 => "DefineSound",
            15 => "StartSound",
            17 => "DefineButtonSound",
            18 => "SoundStreamHead",
            19 => "SoundStreamBlock",
            20 => "DefineBitsLossless",
            21 => "DefineBitsJPEG2",
            22 => "DefineShape2",
            23 => "DefineButtonCxform",
            24 => "Protect",
            26 => "PlaceObject2",
            28 => "RemoveObject2",
            32 => "DefineShape3",
            33 => "DefineText2",
            34 => "DefineButton2",
            35 => "DefineBitsJPEG3",
            36 => "DefineBitsLossless2",
            37 => "DefineEditText",
            39 => "DefineSprite",
            43 => "FrameLabel",
            45 => "SoundStreamHead2",
            46 => "DefineMorphShape",
            48 => "DefineFont2",
            56 => "ExportAssets",
            57 => "ImportAssets",
            58 => "EnableDebugger",
            59 => "DoInitAction",
            60 => "DefineVideoStream",
            61 => "VideoFrame",
            62 => "DefineFontInfo2",
            64 => "EnableDebugger2",
            65 => "ScriptLimits",
            66 => "SetTabIndex",
            69 => "FileAttributes",
            70 => "PlaceObject3",
            71 => "ImportAssets2",
            73 => "DefineFontAlignZones",
            74 => "CSMTextSettings",
            75 => "DefineFont3",
            76 => "SymbolClass",
            77 => "Metadata",
            78 => "DefineScalingGrid",
            82 => "DoABC",
            83 => "DefineShape4",
            84 => "DefineMorphShape2",
            86 => "DefineSceneAndFrameLabelData",
            87 => "DefineBinaryData",
            88 => "DefineFontName",
            89 => "StartSound2",
            90 => "DefineBitsJPEG4",
            91 => "DefineFont4",
            1000 => "ExporterInfo",
            1001 => "DefineExternalImage",
            1002 => "FontTextureInfo",
            1003 => "DefineExternalGradient",
            1004 => "DefineGradientMap",
            1005 => "DefineCompactedFont",
            1006 => "DefineExternalSound",
            1007 => "DefineExternalStreamSound",
            1008 => "DefineSubImage",
            1009 => "DefineExternalImage2",
            _ => return None,
        })
    }

    /// The character id this tag defines, if it defines one.
    pub fn character_id(&self) -> Option<u32> {
        let mut at = Cursor(&self.body);
        match self.code {
            2 | 6 | 7 | 10 | 11 | 14 | 20 | 21 | 22 | 32 | 33 | 34 | 35 | 36 | 37 | 39 | 46
            | 48 | 60 | 75 | 83 | 84 | 87 | 90 | 91 | 1005 | 1008 => at.u16().map(u32::from),
            1009 => at.u32(),
            _ => None,
        }
    }
}

/// The frame header and the tags up to and including `End`.
fn read_body(body: &[u8]) -> Result<(Vec<u8>, Vec<Tag>)> {
    let Some(first) = body.first() else {
        return Err(refuse("the body is empty".to_string()));
    };
    let bits = usize::from(first >> 3);
    let frame_len = (5 + 4 * bits).div_ceil(8) + 4;
    if body.len() < frame_len {
        return Err(refuse(format!(
            "the frame header needs {frame_len} bytes and the body holds {}",
            body.len()
        )));
    }
    let mut tags = Vec::new();
    let mut at = frame_len;
    loop {
        let Some(header) = body.get(at..at + 2) else {
            return Err(refuse(format!(
                "the tags stop at byte {at} with no End tag"
            )));
        };
        let header = u16::from_le_bytes([header[0], header[1]]);
        at += 2;
        let code = header >> 6;
        let mut len = usize::from(header & LONG_LENGTH as u16);
        let long = len == LONG_LENGTH;
        if long {
            let Some(word) = body.get(at..at + 4) else {
                return Err(refuse(format!(
                    "tag {} ({code}) loses its length at byte {at}",
                    tags.len()
                )));
            };
            len = u32::from_le_bytes([word[0], word[1], word[2], word[3]]) as usize;
            at += 4;
        }
        let Some(tag_body) = body.get(at..).and_then(|rest| rest.get(..len)) else {
            return Err(refuse(format!(
                "tag {} ({code}) states {len} bytes at byte {at}, past the body's {}",
                tags.len(),
                body.len()
            )));
        };
        tags.push(Tag {
            code,
            long,
            body: tag_body.to_vec(),
        });
        at += len;
        if code == END {
            break;
        }
    }
    if at != body.len() {
        return Err(refuse(format!(
            "{} bytes follow the End tag",
            body.len() - at
        )));
    }
    Ok((body[..frame_len].to_vec(), tags))
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, rest) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = rest;
        Some(head)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(drop)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A string led by its one-byte length.
    fn pstring(&mut self) -> Option<String> {
        let len = usize::from(*self.take(1)?.first()?);
        self.take(len)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// A string ended by a zero byte.
    fn cstring(&mut self) -> Option<String> {
        let end = self.0.iter().position(|&b| b == 0)?;
        let text = String::from_utf8_lossy(&self.0[..end]).into_owned();
        self.0 = &self.0[end + 1..];
        Some(text)
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("gfx: {why}"),
    }
}
