//! A constant buffer's bytes, written by variable name into the layout the game's own reflection gives it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use or_io::gta5::shaders::spirv_pack::ConstantBuffer;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConstantError {
    #[error("no variable named {0}")]
    NoSuchVariable(String),
    #[error("{name} has room for {room} words and {given} were given")]
    TooMany {
        name: String,
        room: usize,
        given: usize,
    },
}

/// The bytes of one constant buffer, starting at zero.
pub struct Filler<'a> {
    layout: &'a ConstantBuffer,
    bytes: Vec<u8>,
}

impl<'a> Filler<'a> {
    pub fn new(layout: &'a ConstantBuffer) -> Self {
        Self {
            layout,
            bytes: vec![0; layout.size as usize],
        }
    }

    /// Write 32-bit `words` as the variable `name`: a matrix goes in as its registers one after the other.
    pub fn set_words(&mut self, name: &str, words: &[u32]) -> Result<(), ConstantError> {
        let variable = self
            .layout
            .variables
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| ConstantError::NoSuchVariable(name.to_string()))?;
        let room = (variable.size / 4) as usize;
        if words.len() > room {
            return Err(ConstantError::TooMany {
                name: name.to_string(),
                room,
                given: words.len(),
            });
        }
        let at = variable.offset as usize;
        for (i, word) in words.iter().enumerate() {
            let place = at + 4 * i;
            if let Some(slot) = self.bytes.get_mut(place..place + 4) {
                slot.copy_from_slice(&word.to_le_bytes());
            }
        }
        Ok(())
    }

    /// Write `values` as the variable `name`.
    pub fn set(&mut self, name: &str, values: &[f32]) -> Result<(), ConstantError> {
        let words: Vec<u32> = values.iter().map(|v| v.to_bits()).collect();
        self.set_words(name, &words)
    }

    pub fn bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// A uniform buffer a frame writes anew: the buffer, the layout of the game's buffer of that name, and the values below the engine's.
pub struct Uniform {
    buffer: wgpu::Buffer,
    layout: ConstantBuffer,
    /// What the effect declares for the buffer's variables, by name and as 32-bit words, cut to their sizes: what each holds until the engine or the material sets it.
    defaults: Vec<(String, Vec<u32>)>,
    /// The material's values by the name of the variable each sets, cut to its size.
    material: Vec<(String, Vec<f32>)>,
    plan: Mutex<Option<Plan>>,
}

/// What a buffer holds for the names a frame sets, worked out once: the engine sets the same names every frame, so a frame only copies numbers to places found before, and writes the buffer only when they changed.
struct Plan {
    names: Vec<&'static str>,
    /// Each name that is a variable here: its place among `names`, its byte offset and its room in words.
    hits: Vec<(usize, usize, usize)>,
    /// The effect's defaults with the material's values over them, and none of the engine's.
    base: Vec<u8>,
    bytes: Vec<u8>,
    /// What the buffer holds now.
    written: Vec<u8>,
}

impl Plan {
    fn new(
        layout: &ConstantBuffer,
        defaults: &[(String, Vec<u32>)],
        material: &[(String, Vec<f32>)],
        values: &BTreeMap<&'static str, Vec<f32>>,
    ) -> Self {
        let mut filler = Filler::new(layout);
        for (variable, words) in defaults {
            let _ = filler.set_words(variable, words);
        }
        // A drawable carries a value for what the engine sets per draw, such as a wheel's matrices, left at nought; the engine's is the one a draw reads
        for (variable, numbers) in material {
            let _ = filler.set(variable, numbers);
        }
        let hits = values
            .keys()
            .enumerate()
            .filter_map(|(at, name)| {
                let variable = layout.variables.iter().find(|v| v.name == *name)?;
                Some((at, variable.offset as usize, (variable.size / 4) as usize))
            })
            .collect();
        let base = filler.bytes();
        Self {
            names: values.keys().copied().collect(),
            hits,
            bytes: base.clone(),
            base,
            written: Vec::new(),
        }
    }

    fn matches(&self, values: &BTreeMap<&'static str, Vec<f32>>) -> bool {
        self.names.iter().copied().eq(values.keys().copied())
    }

    /// Set `bytes` to the base with `values` over it.
    fn fill(&mut self, values: &BTreeMap<&'static str, Vec<f32>>) {
        self.bytes.copy_from_slice(&self.base);
        let mut hits = self.hits.iter().peekable();
        for (at, numbers) in values.values().enumerate() {
            let Some(&(_, offset, room)) = hits.next_if(|hit| hit.0 == at) else {
                continue;
            };
            // ⚠ A value longer than its variable is left out whole, and the variable keeps what the effect declares
            if numbers.len() > room {
                continue;
            }
            for (i, number) in numbers.iter().enumerate() {
                let place = offset + 4 * i;
                if let Some(slot) = self.bytes.get_mut(place..place + 4) {
                    slot.copy_from_slice(&number.to_bits().to_le_bytes());
                }
            }
        }
    }
}

impl Uniform {
    pub fn new(
        buffer: wgpu::Buffer,
        layout: ConstantBuffer,
        defaults: Vec<(String, Vec<u32>)>,
        material: Vec<(String, Vec<f32>)>,
    ) -> Self {
        Self {
            buffer,
            layout,
            defaults,
            material,
            plan: Mutex::new(None),
        }
    }

    /// Write the buffer: what the effect declares, the material's values over it, then `values` over those; nothing is written where the bytes are what the buffer holds.
    pub fn write(&self, queue: &wgpu::Queue, values: &BTreeMap<&'static str, Vec<f32>>) {
        let mut cache = self.plan.lock().unwrap_or_else(|e| e.into_inner());
        if !cache.as_ref().is_some_and(|plan| plan.matches(values)) {
            *cache = Some(Plan::new(
                &self.layout,
                &self.defaults,
                &self.material,
                values,
            ));
        }
        let Some(plan) = cache.as_mut() else {
            return;
        };
        plan.fill(values);
        // A write is a whole number of words, and no longer than the buffer
        let len = plan.bytes.len().min(self.buffer.size() as usize) & !3;
        if plan.written != plan.bytes[..len] {
            queue.write_buffer(&self.buffer, 0, &plan.bytes[..len]);
            plan.written.clear();
            plan.written.extend_from_slice(&plan.bytes[..len]);
        }
    }
}

#[cfg(test)]
mod tests;
