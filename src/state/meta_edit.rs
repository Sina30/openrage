//! Edits of a metadata instance as undo holds them: each its place in the tree and the side the instance does not hold.
//!
//! ⚠ A place is a path of names and indices, which holds only because steps
//! flip newest first, so each finds the tree as it left it; an edit no step
//! holds clears the history (`Document::mark_edited`).

use or_io::meta::{MetaInstance, Value};

use super::presets::MetaStep;

#[derive(Debug, Clone, PartialEq)]
pub enum MetaEdit {
    /// The value at `path`.
    Set { path: Vec<MetaStep>, other: Value },
    /// Element `at` of the array at `path`, held while it is out of the array.
    Element {
        path: Vec<MetaStep>,
        at: usize,
        other: Option<Value>,
    },
    /// Field `at` of the struct at `path`, named `name`, held while it is out of the struct.
    Field {
        path: Vec<MetaStep>,
        name: String,
        at: usize,
        other: Option<Value>,
    },
    /// Elements `a` and `b` of the array at `path`, which trade places.
    Swap {
        path: Vec<MetaStep>,
        a: usize,
        b: usize,
    },
}

impl MetaEdit {
    /// What the edit did, as the history lists it.
    pub fn label(&self) -> String {
        match self {
            Self::Set { path, .. } => format!("Set {}", shown(path)),
            Self::Element {
                path, other: None, ..
            } => format!("Add to {}", shown(path)),
            Self::Element { path, at, .. } => format!("Delete {}[{at}]", shown(path)),
            Self::Field { path, name, .. } => {
                let mut field = path.clone();
                field.push(MetaStep::Field(name.clone()));
                format!("Add {}", shown(&field))
            }
            Self::Swap { path, a, .. } => format!("Move {}[{a}]", shown(path)),
        }
    }

    /// Swap what this edit holds with what `inst` holds, so it then undoes what it just did; `false` where its place is gone.
    fn flip(&mut self, inst: &mut MetaInstance) -> bool {
        match self {
            Self::Set { path, other } => value_at(inst, path)
                .map(|value| std::mem::swap(value, other))
                .is_some(),
            Self::Element { path, at, other } => match value_at(inst, path) {
                Some(Value::Array(items)) => splice(items, *at, other),
                _ => false,
            },
            Self::Field {
                path,
                name,
                at,
                other,
            } => {
                let Some(fields) = struct_at(inst, path).map(|s| &mut s.fields) else {
                    return false;
                };
                let mut held = other.take().map(|value| (name.clone(), value));
                let flipped = splice(fields, *at, &mut held);
                *other = held.map(|(_, value)| value);
                flipped
            }
            Self::Swap { path, a, b } => match value_at(inst, path) {
                Some(Value::Array(items)) if *a < items.len() && *b < items.len() => {
                    items.swap(*a, *b);
                    true
                }
                _ => false,
            },
        }
    }
}

/// Undo or redo `edits` on `inst`, newest first, leaving them in the order the next flip takes; `false`, changing nothing, where one's place is gone.
pub fn flip_all(edits: &mut [MetaEdit], inst: &mut MetaInstance) -> bool {
    super::edit_history::flip_whole(edits, |edit| edit.flip(inst)).is_ok()
}

/// Put `other` in at `at`, or take what is there out into it; each undoes the other.
pub fn splice<T>(run: &mut Vec<T>, at: usize, other: &mut Option<T>) -> bool {
    match other.take() {
        Some(element) if at <= run.len() => {
            run.insert(at, element);
            true
        }
        None if at < run.len() => {
            *other = Some(run.remove(at));
            true
        }
        kept => {
            *other = kept;
            false
        }
    }
}

/// The value `path` reaches from `inst`, through struct fields, pointers and elements.
pub fn value_at<'a>(inst: &'a mut MetaInstance, path: &[MetaStep]) -> Option<&'a mut Value> {
    let (first, rest) = path.split_first()?;
    let mut value = field_of(inst, first)?;
    for step in rest {
        value = match (step, value) {
            (MetaStep::Index(i), Value::Array(items)) => items.get_mut(*i)?,
            (step, Value::Struct(inner) | Value::Pointer(Some(inner))) => field_of(inner, step)?,
            _ => return None,
        };
    }
    Some(value)
}

/// The struct `path` reaches, which for an empty path is `inst` itself.
fn struct_at<'a>(inst: &'a mut MetaInstance, path: &[MetaStep]) -> Option<&'a mut MetaInstance> {
    if path.is_empty() {
        return Some(inst);
    }
    match value_at(inst, path)? {
        Value::Struct(inner) | Value::Pointer(Some(inner)) => Some(inner),
        _ => None,
    }
}

fn field_of<'a>(inst: &'a mut MetaInstance, step: &MetaStep) -> Option<&'a mut Value> {
    let (name, nth) = match step {
        MetaStep::Field(name) => (name, 0),
        MetaStep::Nth(name, nth) => (name, *nth),
        MetaStep::Index(_) => return None,
    };
    inst.fields
        .iter_mut()
        .filter(|(n, _)| n == name)
        .nth(nth)
        .map(|(_, value)| value)
}

/// `path` as a reader writes it: `entities[3].lodDist`.
fn shown(path: &[MetaStep]) -> String {
    let mut out = String::new();
    for step in path {
        match step {
            MetaStep::Field(name) | MetaStep::Nth(name, _) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            MetaStep::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}
