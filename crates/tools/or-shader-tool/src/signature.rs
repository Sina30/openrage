//! The input and output signatures of a DXBC container, from its `ISGN`, `OSGN` and `OSG5` parts.

use or_io::gta5::shaders::fxc::DxbcBlob;
use or_io::gta5::shaders::spirv_pack::SignatureElement;

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn name_at(bytes: &[u8], at: usize) -> Option<String> {
    let tail = bytes.get(at..)?;
    let end = tail.iter().position(|b| *b == 0)?;
    Some(String::from_utf8_lossy(&tail[..end]).into_owned())
}

/// The elements of one signature part, whose elements are `stride` bytes and start with `skip` bytes before the 24-byte element proper.
fn elements(part: &[u8], skip: usize) -> Vec<SignatureElement> {
    let Some(count) = u32_at(part, 0) else {
        return Vec::new();
    };
    let stride = 24 + skip;
    let mut out = Vec::new();
    for i in 0..count as usize {
        let at = 8 + i * stride + skip;
        let (Some(name), Some(index), Some(system_value), Some(component_type), Some(register)) = (
            u32_at(part, at).and_then(|offset| name_at(part, offset as usize)),
            u32_at(part, at + 4),
            u32_at(part, at + 8),
            u32_at(part, at + 12),
            u32_at(part, at + 16),
        ) else {
            return Vec::new();
        };
        let Some(mask) = part.get(at + 20) else {
            return Vec::new();
        };
        out.push(SignatureElement {
            semantic: name,
            index,
            system_value,
            component_type,
            register,
            mask: *mask,
        });
    }
    out
}

/// The input and output signature of a container; a part that is missing or does not read gives an empty one.
pub fn signatures(blob: &DxbcBlob<'_>) -> (Vec<SignatureElement>, Vec<SignatureElement>) {
    let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
    for part in blob.parts() {
        match &part.tag {
            b"ISGN" => inputs = elements(part.data, 0),
            b"OSGN" => outputs = elements(part.data, 0),
            // A geometry shader's outputs lead each element with its stream.
            b"OSG5" => outputs = elements(part.data, 4),
            _ => {}
        }
    }
    (inputs, outputs)
}

/// How a pixel stage's inputs relate to the outputs of the vertex stage before it, by semantic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Linkage {
    /// Every input has an output of its semantic at its register with its mask.
    Exact,
    /// Every input is narrower than the output it meets.
    Narrower,
    /// Some input reads a component its output does not write.
    Wider,
    /// Some input meets its semantic at another register.
    Moved,
    /// Some input has no output of its semantic.
    Unmatched,
}

/// The worst way any input that is not a system value fails to meet an output, or [`Linkage::Exact`].
pub fn link(outputs: &[SignatureElement], inputs: &[SignatureElement]) -> Linkage {
    let mut worst = Linkage::Exact;
    for input in inputs.iter().filter(|i| i.system_value == 0) {
        let found = outputs
            .iter()
            .find(|o| o.semantic.eq_ignore_ascii_case(&input.semantic) && o.index == input.index);
        let relation = match found {
            None => Linkage::Unmatched,
            Some(o) if o.register != input.register => Linkage::Moved,
            Some(o) if input.mask & !o.mask != 0 => Linkage::Wider,
            Some(o) if input.mask != o.mask => Linkage::Narrower,
            Some(_) => Linkage::Exact,
        };
        worst = worst.max(relation);
    }
    worst
}

#[cfg(all(test, feature = "native-front"))]
mod tests;
