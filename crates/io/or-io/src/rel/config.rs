//! An audio configuration file's items, typed: `rel_type` 4 keyed by name,
//! such as `audioconfig.dat4.rel`. Each item states its type in its first byte.

use super::{len32, put, refuse, Reader, RelFile, RelIndex};
use crate::error::Result;

/// One configuration item: the header every type shares, then its value.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigItem {
    /// 24 bits.
    pub name_table_offset: u32,
    pub flags: u32,
    pub value: ConfigValue,
}

/// A configuration item's value, by its class id.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    /// Class id 0.
    Int(i32),
    /// Class id 1.
    UnsignedInt(u32),
    /// Class id 2.
    Float(f32),
    /// Class id 3: at most 63 bytes, stored NUL-padded to 64.
    String(String),
    /// Class id 5: `value` is x, y, z and w, 16-byte aligned after `padding`.
    Vector3 { padding: [u8; 8], value: [f32; 4] },
    /// Class id 7.
    VariableList(Vec<Variable>),
    /// Class id 8: a wave slot.
    Slot(Slot),
    /// Class id 9: each slot's name hash.
    WaveSlots(Vec<u32>),
    /// Class id 10: early reflection settings.
    ErSettings(Box<ErSettings>),
}

/// A variable's name hash and the value it starts at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Variable {
    pub name: u32,
    pub initial_value: f32,
}

/// A wave slot's load type and sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub load_type: u8,
    pub padding0: u8,
    pub padding1: u8,
    pub padding2: u8,
    pub max_header_size: u32,
    pub size: u32,
    pub static_bank: u32,
    pub max_metadata_size: u32,
    pub max_data_size: u32,
}

/// Early reflection settings: a room, a listener and the reflection network's gains and filters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErSettings {
    pub room_size: f32,
    pub room_dimensions: RoomDimensions,
    /// x, y, z.
    pub listener_pos: [f32; 3],
    pub num_allpasses: u8,
    /// Aligns `allpasses` to 4 bytes.
    pub padding: [u8; 3],
    pub allpasses: [Allpass; 4],
    pub node_gain_matrix: NodeGainMatrix,
    pub gain_1st_order: Speakers,
    pub gain_2nd_order: Speakers,
    pub gain_3rd_order: Speakers,
    pub num_node_lpf_1st_orders: u32,
    /// Cutoffs, one per speaker.
    pub node_lpf_1st_order: [Speakers; 6],
    pub num_node_lpf_2nd_orders: u32,
    /// Cutoffs, one per speaker.
    pub node_lpf_2nd_order: [Speakers; 6],
    pub num_node_lpf_3rd_orders: u32,
    /// Cutoffs, one per speaker.
    pub node_lpf_3rd_order: [Speakers; 6],
}

/// A room's width, length and height as ratios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoomDimensions {
    pub width_ratio: f32,
    pub length_ratio: f32,
    pub height_ratio: f32,
}

/// One allpass stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Allpass {
    pub diffusion: f32,
    pub delay_length: u32,
}

/// A gain per speaker for each of the six reflection nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeGainMatrix {
    pub x1: Speakers,
    pub x2: Speakers,
    pub y1: Speakers,
    pub y2: Speakers,
    pub z1: Speakers,
    pub z2: Speakers,
}

/// A value per speaker: front left, front right, rear left, rear right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speakers {
    pub fl: f32,
    pub fr: f32,
    pub rl: f32,
    pub rr: f32,
}

const STRING_BYTES: usize = 64;

impl ConfigValue {
    /// The value's type, as the engine names it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "Int",
            Self::UnsignedInt(_) => "UnsignedInt",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::Vector3 { .. } => "Vector3",
            Self::VariableList(_) => "VariableList",
            Self::Slot(_) => "Slot",
            Self::WaveSlots(_) => "WaveSlots",
            Self::ErSettings(_) => "ERSettings",
        }
    }
}

impl ConfigItem {
    /// Reads one item's bytes, refusing a class id with no type here or a length its type does not bear out.
    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes: body, at: 0 };
        let what = "the item";
        let head = r.u32(what)?;
        let flags = r.u32(what)?;
        let value = match head & 0xFF {
            0 => ConfigValue::Int(r.u32(what)? as i32),
            1 => ConfigValue::UnsignedInt(r.u32(what)?),
            2 => ConfigValue::Float(f32(&mut r)?),
            3 => {
                let bytes = r.take(STRING_BYTES, what)?;
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(STRING_BYTES);
                if end == STRING_BYTES || bytes[end..].iter().any(|&b| b != 0) {
                    return Err(refuse(
                        "a string item is not NUL-terminated and zero-padded".into(),
                    ));
                }
                let text = std::str::from_utf8(&bytes[..end])
                    .map_err(|_| refuse("a string item is not UTF-8".into()))?;
                ConfigValue::String(text.to_string())
            }
            5 => ConfigValue::Vector3 {
                padding: r.take(8, what)?.try_into().expect("8 bytes"),
                value: [f32(&mut r)?, f32(&mut r)?, f32(&mut r)?, f32(&mut r)?],
            },
            7 => {
                let count = counted(&mut r, 8)?;
                ConfigValue::VariableList(
                    (0..count)
                        .map(|_| {
                            Ok(Variable {
                                name: r.u32(what)?,
                                initial_value: f32(&mut r)?,
                            })
                        })
                        .collect::<Result<_>>()?,
                )
            }
            8 => {
                let b = r.take(4, what)?;
                ConfigValue::Slot(Slot {
                    load_type: b[0],
                    padding0: b[1],
                    padding1: b[2],
                    padding2: b[3],
                    max_header_size: r.u32(what)?,
                    size: r.u32(what)?,
                    static_bank: r.u32(what)?,
                    max_metadata_size: r.u32(what)?,
                    max_data_size: r.u32(what)?,
                })
            }
            9 => {
                let count = counted(&mut r, 4)?;
                ConfigValue::WaveSlots((0..count).map(|_| r.u32(what)).collect::<Result<_>>()?)
            }
            10 => ConfigValue::ErSettings(Box::new(ErSettings::read(&mut r)?)),
            class => {
                return Err(refuse(format!(
                    "a configuration item's class id {class} has no type here"
                )))
            }
        };
        if r.at != body.len() {
            return Err(refuse(format!(
                "a configuration item of class id {} is {} bytes and its type takes {}",
                head & 0xFF,
                body.len(),
                r.at
            )));
        }
        Ok(Self {
            name_table_offset: head >> 8,
            flags,
            value,
        })
    }

    /// The item's bytes, refusing a name table offset past 24 bits or a string past 63 bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.name_table_offset > 0xFF_FFFF {
            return Err(refuse(format!(
                "a configuration item's name table offset {:#x} is past 24 bits",
                self.name_table_offset
            )));
        }
        let class = match &self.value {
            ConfigValue::Int(_) => 0,
            ConfigValue::UnsignedInt(_) => 1,
            ConfigValue::Float(_) => 2,
            ConfigValue::String(_) => 3,
            ConfigValue::Vector3 { .. } => 5,
            ConfigValue::VariableList(_) => 7,
            ConfigValue::Slot(_) => 8,
            ConfigValue::WaveSlots(_) => 9,
            ConfigValue::ErSettings(_) => 10,
        };
        let mut out = Vec::with_capacity(16);
        put(&mut out, class | self.name_table_offset << 8);
        put(&mut out, self.flags);
        match &self.value {
            ConfigValue::Int(v) => put(&mut out, *v as u32),
            ConfigValue::UnsignedInt(v) => put(&mut out, *v),
            ConfigValue::Float(v) => put(&mut out, v.to_bits()),
            ConfigValue::String(text) => {
                if text.len() >= STRING_BYTES || text.contains('\0') {
                    return Err(refuse(format!(
                        "a string item holds `{text}`, past 63 bytes or with a NUL"
                    )));
                }
                out.extend_from_slice(text.as_bytes());
                out.resize(out.len() + STRING_BYTES - text.len(), 0);
            }
            ConfigValue::Vector3 { padding, value } => {
                out.extend_from_slice(padding);
                value.iter().for_each(|v| put(&mut out, v.to_bits()));
            }
            ConfigValue::VariableList(variables) => {
                put(&mut out, len32(variables.len(), "a variable list")?);
                for v in variables {
                    put(&mut out, v.name);
                    put(&mut out, v.initial_value.to_bits());
                }
            }
            ConfigValue::Slot(slot) => {
                out.extend_from_slice(&[
                    slot.load_type,
                    slot.padding0,
                    slot.padding1,
                    slot.padding2,
                ]);
                for word in [
                    slot.max_header_size,
                    slot.size,
                    slot.static_bank,
                    slot.max_metadata_size,
                    slot.max_data_size,
                ] {
                    put(&mut out, word);
                }
            }
            ConfigValue::WaveSlots(slots) => {
                put(&mut out, len32(slots.len(), "a wave slot list")?);
                slots.iter().for_each(|&s| put(&mut out, s));
            }
            ConfigValue::ErSettings(er) => er.write(&mut out),
        }
        Ok(out)
    }
}

impl ErSettings {
    fn read(r: &mut Reader<'_>) -> Result<Self> {
        let room_size = f32(r)?;
        let room_dimensions = RoomDimensions {
            width_ratio: f32(r)?,
            length_ratio: f32(r)?,
            height_ratio: f32(r)?,
        };
        let listener_pos = [f32(r)?, f32(r)?, f32(r)?];
        let b = r.take(4, "the item")?;
        let (num_allpasses, padding) = (b[0], [b[1], b[2], b[3]]);
        let mut allpasses = [Allpass {
            diffusion: 0.0,
            delay_length: 0,
        }; 4];
        for a in &mut allpasses {
            *a = Allpass {
                diffusion: f32(r)?,
                delay_length: r.u32("the item")?,
            };
        }
        let node_gain_matrix = NodeGainMatrix {
            x1: Speakers::read(r)?,
            x2: Speakers::read(r)?,
            y1: Speakers::read(r)?,
            y2: Speakers::read(r)?,
            z1: Speakers::read(r)?,
            z2: Speakers::read(r)?,
        };
        let gain_1st_order = Speakers::read(r)?;
        let gain_2nd_order = Speakers::read(r)?;
        let gain_3rd_order = Speakers::read(r)?;
        let mut lpf = || -> Result<(u32, [Speakers; 6])> {
            let count = r.u32("the item")?;
            let mut stages = [Speakers::default(); 6];
            for s in &mut stages {
                *s = Speakers::read(r)?;
            }
            Ok((count, stages))
        };
        let (num_node_lpf_1st_orders, node_lpf_1st_order) = lpf()?;
        let (num_node_lpf_2nd_orders, node_lpf_2nd_order) = lpf()?;
        let (num_node_lpf_3rd_orders, node_lpf_3rd_order) = lpf()?;
        Ok(Self {
            room_size,
            room_dimensions,
            listener_pos,
            num_allpasses,
            padding,
            allpasses,
            node_gain_matrix,
            gain_1st_order,
            gain_2nd_order,
            gain_3rd_order,
            num_node_lpf_1st_orders,
            node_lpf_1st_order,
            num_node_lpf_2nd_orders,
            node_lpf_2nd_order,
            num_node_lpf_3rd_orders,
            node_lpf_3rd_order,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        let d = &self.room_dimensions;
        for v in [
            self.room_size,
            d.width_ratio,
            d.length_ratio,
            d.height_ratio,
        ]
        .iter()
        .chain(&self.listener_pos)
        {
            put(out, v.to_bits());
        }
        out.push(self.num_allpasses);
        out.extend_from_slice(&self.padding);
        for a in &self.allpasses {
            put(out, a.diffusion.to_bits());
            put(out, a.delay_length);
        }
        let m = &self.node_gain_matrix;
        for s in [
            m.x1,
            m.x2,
            m.y1,
            m.y2,
            m.z1,
            m.z2,
            self.gain_1st_order,
            self.gain_2nd_order,
            self.gain_3rd_order,
        ] {
            s.write(out);
        }
        for (count, stages) in [
            (self.num_node_lpf_1st_orders, &self.node_lpf_1st_order),
            (self.num_node_lpf_2nd_orders, &self.node_lpf_2nd_order),
            (self.num_node_lpf_3rd_orders, &self.node_lpf_3rd_order),
        ] {
            put(out, count);
            stages.iter().for_each(|s| s.write(out));
        }
    }
}

impl Default for Speakers {
    fn default() -> Self {
        Self {
            fl: 0.0,
            fr: 0.0,
            rl: 0.0,
            rr: 0.0,
        }
    }
}

impl Speakers {
    fn read(r: &mut Reader<'_>) -> Result<Self> {
        Ok(Self {
            fl: f32(r)?,
            fr: f32(r)?,
            rl: f32(r)?,
            rr: f32(r)?,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        for v in [self.fl, self.fr, self.rl, self.rr] {
            put(out, v.to_bits());
        }
    }
}

impl RelFile {
    /// Every item of an audio configuration file in index order, typed; refuses a file keyed by hash.
    pub fn config_items(&self) -> Result<Vec<ConfigItem>> {
        if !matches!(self.index, RelIndex::Named(_)) {
            return Err(refuse(
                "an audio configuration file's items are keyed by name".into(),
            ));
        }
        self.items()?
            .iter()
            .enumerate()
            .map(|(i, item)| {
                ConfigItem::decode(item.body())
                    .map_err(|e| refuse(format!("configuration item {i} ({:?}): {e}", item.key())))
            })
            .collect()
    }
}

/// A counted array's count, refused when its entries of `stride` bytes would pass the item's end.
fn counted(r: &mut Reader<'_>, stride: usize) -> Result<u32> {
    let count = r.u32("the item")?;
    if count as usize > (r.bytes.len() - r.at) / stride {
        return Err(refuse(format!(
            "a configuration item counts {count} entries of {stride} bytes past its end"
        )));
    }
    Ok(count)
}

fn f32(r: &mut Reader<'_>) -> Result<f32> {
    Ok(f32::from_bits(r.u32("the item")?))
}
