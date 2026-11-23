//! A motion database's columns: each names a component of one of the parameters the motion's
//! `interface` property lists, the parameters taken in the order of their name hashes.

use std::collections::HashSet;

use super::databases::Column;
use super::metadata::{AttributeValue, Record};
use super::motion::Typed;
use crate::gta6::hash::name_hash32;
use crate::hash::{finalize_hash, partial_string_hash};

const INTERFACE: u32 = name_hash32(b"interface");
const PARAMETERS: u32 = name_hash32(b"parameters");
/// A vector component's letter, which follows the parameter's name and a `.` in its column's hash.
const COMPONENTS: &[u8; 4] = b"xyzw";

/// The parameter a column names: its name hash and, for a vector, the component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnParameter {
    pub parameter: u32,
    /// 0 to 3 for `x` to `w`; none for a scalar.
    pub component: Option<u8>,
}

impl ColumnParameter {
    /// `name.x` for a vector's component and `name` for a scalar, `named` naming the hash.
    pub fn label(&self, named: impl Fn(u32) -> String) -> String {
        let name = named(self.parameter);
        match self.component {
            Some(c) => format!("{name}.{}", char::from(COMPONENTS[usize::from(c)])),
            None => name,
        }
    }
}

/// The parameter and component `column` names among the motion's `properties`.
pub fn column_parameter(properties: &[Record], column: Column) -> Result<ColumnParameter, String> {
    resolve(&parameters(properties)?, column)
}

/// Whether every column of `typed`, if a database, names a parameter's component, no two alike.
pub(super) fn check(properties: &[Record], typed: &Typed) -> Result<(), String> {
    let columns: &[Column] = match typed {
        Typed::Table(database) => &database.columns,
        Typed::Rows(database) => &database.keys,
        Typed::Simplices(database) => &database.columns,
        _ => return Ok(()),
    };
    let parameters = parameters(properties)?;
    let mut named = HashSet::new();
    for &column in columns {
        resolve(&parameters, column)?;
        let [x, y] = column.1;
        if !named.insert(column.1) {
            return Err(format!(
                "two of a database's columns name component {y} of its parameter {x}"
            ));
        }
    }
    Ok(())
}

/// The attributes of the first `interface` property's `parameters` object, by name hash rising.
fn parameters(properties: &[Record]) -> Result<Vec<(u32, &AttributeValue)>, String> {
    for record in properties.iter().filter(|r| r.name == INTERFACE) {
        for attribute in &record.attributes {
            if attribute.name != PARAMETERS {
                continue;
            }
            if let AttributeValue::Object(object) = &attribute.value {
                let mut sorted: Vec<_> = object.iter().map(|a| (a.name, &a.value)).collect();
                sorted.sort_by_key(|(name, _)| *name);
                return Ok(sorted);
            }
        }
    }
    Err("its motion has no `interface` property with a `parameters` object, which a database's columns index".into())
}

/// The column `{hash, x, y}`: component y of parameter x, its hash the parameter's name for a
/// scalar and the name continued by `.` and the component's letter for a vector.
fn resolve(
    parameters: &[(u32, &AttributeValue)],
    column: Column,
) -> Result<ColumnParameter, String> {
    let (hash, [x, y]) = column;
    let Some(&(name, value)) = parameters.get(usize::from(x)) else {
        return Err(format!(
            "a database's column names parameter {x} of the {} its motion lists",
            parameters.len()
        ));
    };
    // Columns were seen to name floats, Vector3s and Vector4s only, so how another type's
    // components are named is not known.
    let width = match value.type_word() {
        1 => 1,
        6 => 3,
        7 => 4,
        other => {
            return Err(format!(
                "a database's column names parameter {x}, of type {other}, and how the components of a parameter of that type are named is not known"
            ))
        }
    };
    if y >= width {
        return Err(format!(
            "a database's column names component {y} of parameter {x}, which has {width}"
        ));
    }
    let (component, want) = match width {
        1 => (None, name),
        _ => (Some(y as u8), component_hash(name, y as u8)),
    };
    if hash != want {
        return Err(format!(
            "a database's column hash {hash:#010X} is not the one component {y} of parameter {x} gives, {want:#010X}"
        ));
    }
    Ok(ColumnParameter {
        parameter: name,
        component,
    })
}

/// The hash of the name that hashes to `name` followed by `.` and component `c`'s letter.
fn component_hash(name: u32, c: u8) -> u32 {
    let continued = [b'.', COMPONENTS[usize::from(c)]];
    finalize_hash(partial_string_hash(&continued, unfinalize(name)))
}

/// The partial hash [`finalize_hash`] turns into `hash`: each of its three steps is a bijection.
fn unfinalize(hash: u32) -> u32 {
    let h = hash.wrapping_mul(inverse(1 + (1 << 15)));
    let mut x = h;
    // `x ^= x >> 11` is undone by three rounds, each fixing 11 more bits.
    for _ in 0..3 {
        x = h ^ (x >> 11);
    }
    x.wrapping_mul(inverse(9))
}

/// The inverse of odd `a` modulo 2^32: each Newton step doubles the bits that hold.
fn inverse(a: u32) -> u32 {
    let mut x = a;
    for _ in 0..5 {
        x = x.wrapping_mul(2u32.wrapping_sub(a.wrapping_mul(x)));
    }
    x
}

#[cfg(test)]
#[path = "columns_tests.rs"]
mod tests;
