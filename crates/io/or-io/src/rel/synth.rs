//! A modular synth file's items, typed: `rel_type` 10, such as
//! `optamp.dat10.rel`. Each item opens with its class id and name table offset in
//! one word, and every type is packed.

use super::layout::{
    rel_items, rel_struct, Counted, Field, ItemReader, ItemWriter, NameTableOffset,
};
use super::refuse;
use crate::error::Result;

rel_struct! {
    pub struct Preset {
        name_table_offset: NameTableOffset,
        flags: u32,
        variable: Vec<Variable> = u8,
    }
}

/// A compiled synth program; its `programSizeBytes` is derived from `operations` on write.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSynth {
    pub name_table_offset: NameTableOffset,
    pub flags: u32,
    pub num_buffers: u32,
    pub num_registers: u32,
    pub num_outputs: u32,
    // ⚠ Every slot is stored whatever `num_outputs` states.
    pub outputs: [Outputs; 4],
    pub num_state_blocks: u32,
    pub cost_estimate: u32,
    pub operations: Vec<u16>,
    pub constants: Vec<Constants>,
    pub input_variables: Vec<InputVariables>,
}

impl CompiledSynth {
    pub(crate) const OWN_ALIGN: usize = 1;
}

impl Field for CompiledSynth {
    const ALIGN: usize = 1;

    // ⚠ `programSizeBytes` counts `operations` in bytes and sits three words
    // before them, past `numStateBlocks` and `costEstimate`.
    fn read_fields(r: &mut ItemReader<'_>) -> Result<Self> {
        let name_table_offset = NameTableOffset::read(r)?;
        let flags = u32::read(r)?;
        let num_buffers = u32::read(r)?;
        let num_registers = u32::read(r)?;
        let num_outputs = u32::read(r)?;
        let outputs = <[Outputs; 4]>::read(r)?;
        let program_size_bytes = u32::read(r)?;
        let num_state_blocks = u32::read(r)?;
        let cost_estimate = u32::read(r)?;
        if program_size_bytes % 2 != 0 {
            return Err(refuse(format!(
                "a compiled synth's program is {program_size_bytes} bytes, not whole operations"
            )));
        }
        let operations = (0..program_size_bytes / 2)
            .map(|_| u16::read(r))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            name_table_offset,
            flags,
            num_buffers,
            num_registers,
            num_outputs,
            outputs,
            num_state_blocks,
            cost_estimate,
            operations,
            constants: Vec::read_counted::<u32>(r)?,
            input_variables: Vec::read_counted::<u32>(r)?,
        })
    }

    fn write_fields(&self, w: &mut ItemWriter) -> Result<()> {
        let program_size_bytes = u32::try_from(self.operations.len() * 2).map_err(|_| {
            refuse(format!(
                "a compiled synth's {} operations are past a u32 of bytes",
                self.operations.len()
            ))
        })?;
        self.name_table_offset.write(w)?;
        self.flags.write(w)?;
        self.num_buffers.write(w)?;
        self.num_registers.write(w)?;
        self.num_outputs.write(w)?;
        self.outputs.write(w)?;
        program_size_bytes.write(w)?;
        self.num_state_blocks.write(w)?;
        self.cost_estimate.write(w)?;
        self.operations.iter().try_for_each(|op| op.write(w))?;
        self.constants.write_counted::<u32>(w)?;
        self.input_variables.write_counted::<u32>(w)
    }
}

rel_struct! {
    pub struct Variable {
        name: u32,
        value: f32,
        variance: f32,
    }
}

rel_struct! {
    pub struct Outputs {
        buffer_id: u8,
    }
}

rel_struct! {
    pub struct Constants {
        value: f32,
    }
}

rel_struct! {
    pub struct InputVariables {
        name: u32,
        initial_value: f32,
    }
}

rel_items! {
    pub enum SynthItem, class byte 0 {
        1 => Preset packed,
        3 => CompiledSynth packed,
    }
}
