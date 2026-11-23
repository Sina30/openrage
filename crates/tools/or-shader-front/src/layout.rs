//! Where each Direct3D register lands among the descriptors of the SPIR-V the translator writes.

/// A run of bindings: register `r` of a class sits at binding `base + r`, and `len` registers fit.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub base: u32,
    pub len: u32,
}

/// The classes of descriptor a DXBC shader declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceClass {
    /// `b#`
    Cbv,
    /// `s#`
    Sampler,
    /// `t#`
    Srv,
    /// `u#`
    Uav,
    /// The hidden counter of an append, consume or counted `u#`.
    UavCounter,
}

/// A descriptor set and binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub set: u32,
    pub binding: u32,
}

/// One window per class, in one descriptor set per register space and stage.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingLayout {
    pub cbv: Window,
    pub sampler: Window,
    pub srv: Window,
    pub uav: Window,
    pub uav_counter: Window,
    /// The set of register space 0.
    pub set_base: u32,
    /// How many sets apart two register spaces sit.
    pub set_stride: u32,
}

impl BindingLayout {
    /// Direct3D 11's slot counts (14 constant buffers, 16 samplers, 128 resources, 64 UAVs), rounded up and placed side by side, for a vertex or compute shader; [`Self::for_program_type`] moves a pixel shader's registers to the next set.
    pub const OPENRAGE: Self = Self {
        cbv: Window { base: 0, len: 16 },
        sampler: Window { base: 16, len: 16 },
        srv: Window { base: 32, len: 128 },
        uav: Window { base: 160, len: 64 },
        uav_counter: Window { base: 224, len: 64 },
        set_base: 0,
        set_stride: 2,
    };

    /// This layout for a program of D3D program type `program_type` (0 pixel, 1 vertex, 5 compute): Direct3D 11 keeps a table of slots for each stage, so a vertex shader's `t0` and a pixel shader's are two resources and the pixel shader's registers take the set after the vertex shader's.
    pub const fn for_program_type(self, program_type: u32) -> Self {
        Self {
            set_base: self.set_base + if program_type == 0 { 1 } else { 0 },
            ..self
        }
    }

    pub const fn window(&self, class: ResourceClass) -> Window {
        match class {
            ResourceClass::Cbv => self.cbv,
            ResourceClass::Sampler => self.sampler,
            ResourceClass::Srv => self.srv,
            ResourceClass::Uav => self.uav,
            ResourceClass::UavCounter => self.uav_counter,
        }
    }

    /// The binding of register `register` in register space `space`, or `None` where it falls outside its class's window.
    pub fn binding(&self, class: ResourceClass, space: u32, register: u32) -> Option<Binding> {
        let window = self.window(class);
        (register < window.len).then(|| Binding {
            set: self.set_base + self.set_stride * space,
            binding: window.base + register,
        })
    }
}

/// The program type of a container's shader, from the first word of its program part: 0 pixel, 1 vertex, 2 geometry, 3 hull, 4 domain, 5 compute.
pub fn program_type(dxbc: &[u8]) -> Option<u32> {
    let word = |at: usize| Some(u32::from_le_bytes(dxbc.get(at..at + 4)?.try_into().ok()?));
    if dxbc.get(..4)? != b"DXBC" {
        return None;
    }
    for part in 0..word(28)? as usize {
        let at = word(32 + 4 * part)? as usize;
        if matches!(dxbc.get(at..at + 4)?, b"SHDR" | b"SHEX") {
            return Some(word(at + 8)? >> 16);
        }
    }
    None
}

impl Default for BindingLayout {
    fn default() -> Self {
        Self::OPENRAGE
    }
}
