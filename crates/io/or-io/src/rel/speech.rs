//! A speech file's items, typed: `rel_type` 4 keyed by hash, such as
//! `speech2.dat4.rel`.

use std::collections::HashSet;

use super::{put, refuse, RelFile, RelIndex};
use crate::error::Result;

/// One speech item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechItem {
    VariationGroupInfoData(VariationGroupInfoData),
    VoiceContextInfo(VoiceContextInfo),
    VoiceContextInfoVarGroupData(VoiceContextInfoVarGroupData),
    Voice(Voice),
    BankNameTable(BankNameTable),
    VoiceContextVariationMask(VoiceContextVariationMask),
}

/// A byte per variation of a voice's context; the file states their count first, at most 255.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariationGroupInfoData {
    pub variation_data: Vec<u8>,
}

/// How many variations a voice has for a context, and which bank holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceContextInfo {
    pub num_variations: u8,
    /// The key of the [`BankNameTable`] is the hash of this index in decimal.
    pub bank_name_table_index: u16,
}

/// The key of a context's [`VariationGroupInfoData`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceContextInfoVarGroupData {
    pub r#ref: u32,
}

/// A voice's pain type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voice {
    pub pain_type: u8,
}

/// A bank a voice's contexts load from; its class id, 4, is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankNameTable {
    /// 24 bits.
    pub name_table_offset: u32,
    pub bank_name: u32,
}

/// The variations of a context excluded under a prefix, bit 0 for variation 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceContextVariationMask {
    pub variation_mask: u16,
}

const BANK_NAME_TABLE_CLASS_ID: u32 = 4;

impl SpeechItem {
    /// The type's name, as the engine declares it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::VariationGroupInfoData(_) => "VariationGroupInfoData",
            Self::VoiceContextInfo(_) => "VoiceContextInfo",
            Self::VoiceContextInfoVarGroupData(_) => "VoiceContextInfoVarGroupData",
            Self::Voice(_) => "Voice",
            Self::BankNameTable(_) => "BankNameTable",
            Self::VoiceContextVariationMask(_) => "VoiceContextVariationMask",
        }
    }

    /// The item's bytes, refusing a variation group of more than 255 or a name table offset past 24 bits.
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(match self {
            Self::VariationGroupInfoData(group) => {
                let count = u8::try_from(group.variation_data.len()).map_err(|_| {
                    refuse(format!(
                        "a variation group holds {} variations, past 255",
                        group.variation_data.len()
                    ))
                })?;
                let mut out = vec![count];
                out.extend_from_slice(&group.variation_data);
                out
            }
            Self::VoiceContextInfo(info) => {
                let mut out = vec![info.num_variations];
                out.extend_from_slice(&info.bank_name_table_index.to_le_bytes());
                out
            }
            Self::VoiceContextInfoVarGroupData(data) => data.r#ref.to_le_bytes().to_vec(),
            Self::Voice(voice) => vec![voice.pain_type],
            Self::BankNameTable(bank) => {
                if bank.name_table_offset > 0xFF_FFFF {
                    return Err(refuse(format!(
                        "a bank name table's name table offset {:#x} is past 24 bits",
                        bank.name_table_offset
                    )));
                }
                let mut out = Vec::with_capacity(8);
                put(
                    &mut out,
                    BANK_NAME_TABLE_CLASS_ID | bank.name_table_offset << 8,
                );
                put(&mut out, bank.bank_name);
                out
            }
            Self::VoiceContextVariationMask(mask) => mask.variation_mask.to_le_bytes().to_vec(),
        })
    }
}

impl RelFile {
    /// Every item of a speech file in index order, typed; refuses a file keyed by name or an item no type fits.
    pub fn speech_items(&self) -> Result<Vec<SpeechItem>> {
        let RelIndex::Hashed(entries) = &self.index else {
            return Err(refuse("a speech file's items are keyed by hash".into()));
        };
        // ⚠ No item states its type, and a variation group's lengths collide
        // with the other types'. The game reaches each type by a different key,
        // so the type is read from what points at the item: the hash table marks
        // a group reference, a group reference names a variation group, and the
        // pack table marks a bank name table's `bank_name`; every other item's
        // type follows from its length.
        let items = self.items()?;
        let hashed: HashSet<u32> = self.hash_offsets.iter().copied().collect();
        let packed: HashSet<u32> = self.pack_offsets.iter().copied().collect();
        let groups: HashSet<u32> = entries
            .iter()
            .zip(&items)
            .filter(|(e, item)| hashed.contains(&e.offset) && item.body().len() == 4)
            .map(|(_, item)| word(item.body(), 0))
            .collect();
        entries
            .iter()
            .zip(items)
            .enumerate()
            .map(|(i, (e, item))| {
                let body = item.body();
                let len = body.len();
                let typed = if hashed.contains(&e.offset) {
                    (len == 4).then(|| {
                        SpeechItem::VoiceContextInfoVarGroupData(VoiceContextInfoVarGroupData {
                            r#ref: word(body, 0),
                        })
                    })
                } else if groups.contains(&e.hash) {
                    (body[0] as usize == len - 1).then(|| {
                        SpeechItem::VariationGroupInfoData(VariationGroupInfoData {
                            variation_data: body[1..].to_vec(),
                        })
                    })
                } else if e.offset.checked_add(4).is_some_and(|o| packed.contains(&o)) {
                    (len == 8 && word(body, 0) & 0xFF == BANK_NAME_TABLE_CLASS_ID).then(|| {
                        SpeechItem::BankNameTable(BankNameTable {
                            name_table_offset: word(body, 0) >> 8,
                            bank_name: word(body, 4),
                        })
                    })
                } else {
                    match len {
                        1 => Some(SpeechItem::Voice(Voice { pain_type: body[0] })),
                        2 => Some(SpeechItem::VoiceContextVariationMask(
                            VoiceContextVariationMask {
                                variation_mask: u16::from_le_bytes([body[0], body[1]]),
                            },
                        )),
                        3 => Some(SpeechItem::VoiceContextInfo(VoiceContextInfo {
                            num_variations: body[0],
                            bank_name_table_index: u16::from_le_bytes([body[1], body[2]]),
                        })),
                        _ => None,
                    }
                };
                typed.ok_or_else(|| {
                    refuse(format!(
                        "speech item {i} ({:#010x}, {len} bytes at {:#x}) fits no speech type",
                        e.hash, e.offset
                    ))
                })
            })
            .collect()
    }
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
