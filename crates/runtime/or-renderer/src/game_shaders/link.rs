//! What a pipeline needs of a vertex stage and a pixel stage together that neither says alone.

use std::collections::HashMap;

use naga::{Binding, Interpolation, Sampling, ShaderStage, TypeInner};

/// How each location the pixel stage reads is interpolated.
fn wanted(fragment: &naga::Module) -> HashMap<u32, (Option<Interpolation>, Option<Sampling>)> {
    let mut by_location = HashMap::new();
    let mut note = |binding: &Option<Binding>| {
        if let Some(Binding::Location {
            location,
            interpolation,
            sampling,
            ..
        }) = binding
        {
            by_location.insert(*location, (*interpolation, *sampling));
        }
    };
    for entry in fragment
        .entry_points
        .iter()
        .filter(|e| e.stage == ShaderStage::Fragment)
    {
        for argument in &entry.function.arguments {
            match &fragment.types[argument.ty].inner {
                TypeInner::Struct { members, .. } => {
                    members.iter().for_each(|m| note(&m.binding));
                }
                _ => note(&argument.binding),
            }
        }
    }
    by_location
}

/// Give each output of `vertex` the interpolation the pixel stage reads its location with.
///
/// ⚠ Direct3D declares interpolation on the pixel stage alone, so a vertex shader's own says nothing, and a pipeline refuses a pair whose two stages disagree.
pub fn match_interpolation(vertex: &mut naga::Module, fragment: &naga::Module) {
    let wanted = wanted(fragment);
    let set = |binding: &mut Option<Binding>| {
        if let Some(Binding::Location {
            location,
            interpolation,
            sampling,
            ..
        }) = binding
        {
            if let Some((wanted_interpolation, wanted_sampling)) = wanted.get(location) {
                *interpolation = *wanted_interpolation;
                *sampling = *wanted_sampling;
            }
        }
    };
    let mut results = Vec::new();
    for entry in vertex
        .entry_points
        .iter_mut()
        .filter(|e| e.stage == ShaderStage::Vertex)
    {
        if let Some(result) = entry.function.result.as_mut() {
            set(&mut result.binding);
            results.push(result.ty);
        }
    }
    for handle in results {
        let mut ty = vertex.types[handle].clone();
        let TypeInner::Struct { members, .. } = &mut ty.inner else {
            continue;
        };
        let before = members.clone();
        members.iter_mut().for_each(|m| set(&mut m.binding));
        // ⚠ The handle has to stay the one the function body's expressions are typed with, and `replace` panics unless the type really changed
        if *members != before {
            vertex.types.replace(handle, ty);
        }
    }
}

#[cfg(test)]
mod tests;
