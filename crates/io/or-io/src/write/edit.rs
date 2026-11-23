//! Edits to a described tree that keep the tree describable.
//!
//! ⚠ The bare mutators on [`Value`] are not the editing API. `build` carries a
//! count rather than recomputing it, so a bare `elements_mut().push(..)` leaves
//! the count behind and `build` refuses the tree with `CountDisagrees`. And a
//! count is not always a whole field: `MaskedField` puts it in part of a word, so
//! writing a length over the whole word destroys the flags packed beside it and
//! the file still reads (`a_masked_count_keeps_the_bits_that_are_not_the_count`),
//! and `CountAndCapacity` pairs it with a capacity that must not move.
//!
//! `Field`, `MaskedField` and `CountAndCapacity` are maintained; every other
//! [`CountSource`] is refused, and [`why_not_resizable`] says why, so a disabled
//! button can explain itself.

use crate::desc::value::ScalarValue;
use crate::desc::{CountSource, FieldRepr, Kind, Pack, Pointee, Record, RecordRun, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    NoType {
        type_name: String,
    },
    NoField {
        type_name: String,
        field: String,
    },
    NotAnArray {
        field: String,
    },
    /// The description fixes this run's length, so it is not the editor's to
    /// change.
    LengthNotOurs {
        field: String,
        because: String,
    },
    /// The field named as the count is not an integer, so there is nothing to
    /// maintain.
    CountNotAnInteger {
        field: String,
        count_field: String,
    },
    /// The run would outgrow the capacity its own description declares.
    ///
    /// ⚠ Not the same as "too long". A `CountAndCapacity` run lives in an
    /// allocation sized by the capacity, so growing past it needs a
    /// reallocation the game will not do.
    PastCapacity {
        field: String,
        capacity: u128,
        wanted: usize,
    },
    OutOfRange {
        field: String,
        index: usize,
        len: usize,
    },
    /// A packed run holds structs of its own type with no children, and this is not one.
    NotAnElement {
        field: String,
        because: String,
    },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoType { type_name } => write!(f, "no described type `{type_name}`"),
            Self::NoField { type_name, field } => {
                write!(f, "`{type_name}` has no field `{field}`")
            }
            Self::NotAnArray { field } => {
                write!(f, "`{field}` is not an array, so it has no elements")
            }
            Self::LengthNotOurs { field, because } => {
                write!(f, "the length of `{field}` is not editable: {because}")
            }
            Self::CountNotAnInteger { field, count_field } => write!(
                f,
                "`{field}` counts through `{count_field}`, which is not an integer"
            ),
            Self::PastCapacity {
                field,
                capacity,
                wanted,
            } => write!(
                f,
                "`{field}` holds at most {capacity}; {wanted} would need a larger \
                 allocation than its description declares"
            ),
            Self::OutOfRange { field, index, len } => {
                write!(f, "`{field}` has {len} element(s); no index {index}")
            }
            Self::NotAnElement { field, because } => {
                write!(f, "`{field}` cannot take that element: {because}")
            }
        }
    }
}

impl std::error::Error for EditError {}

type Result<T> = std::result::Result<T, EditError>;

/// How this run's length is recorded, once the description has been consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LengthIsIn {
    /// A whole integer field.
    Field(String),
    /// A whole field, paired with another holding the allocated capacity.
    FieldWithCapacity { count: String, capacity: String },
    /// Part of a field, under a mask.
    ///
    /// ⚠ The rest of that word belongs to something else and must survive.
    Masked { field: String, mask: u64 },
}

/// Where `field`'s element count lives, or why it is not ours to set.
fn length_of(pack: &Pack, type_name: &str, field: &str) -> Result<LengthIsIn> {
    let ty = pack
        .type_by_name(type_name)
        .ok_or_else(|| EditError::NoType {
            type_name: type_name.to_string(),
        })?;
    let described =
        ty.fields
            .iter()
            .find(|f| f.name == field)
            .ok_or_else(|| EditError::NoField {
                type_name: type_name.to_string(),
                field: field.to_string(),
            })?;

    let count = match &described.repr {
        FieldRepr::Ptr(Pointee::Many { count, .. }) => count,
        FieldRepr::Ptr(_) => {
            return Err(EditError::LengthNotOurs {
                field: field.to_string(),
                because: "it points at one value, not a run".into(),
            })
        }
        // ⚠ A slot table's length *is* the table, and an index into it is the
        // identity of what sits there — `as_ptr_array` on a `Vec<Option<T>>`
        // keeps empty slots at their own index precisely so that stays true. So
        // removing one must blank the slot rather than shift the rest, which is
        // a different operation and is refused here rather than done wrongly.
        FieldRepr::PtrSlots { .. } => {
            return Err(EditError::LengthNotOurs {
                field: field.to_string(),
                because: "it is a slot table, where an index is an identity — \
                          clear a slot rather than resizing it"
                    .into(),
            })
        }
        _ => {
            return Err(EditError::NotAnArray {
                field: field.to_string(),
            })
        }
    };

    match count {
        CountSource::Field(name) => Ok(LengthIsIn::Field(name.clone())),
        CountSource::CountAndCapacity { count, capacity } => Ok(LengthIsIn::FieldWithCapacity {
            count: count.clone(),
            capacity: capacity.clone(),
        }),
        CountSource::MaskedField { field: name, mask }
        | CountSource::MaskedOrMipChain { field: name, mask } => Ok(LengthIsIn::Masked {
            field: name.clone(),
            mask: *mask,
        }),
        // ⚠ Refused rather than treated as `Field(count)`. The stride is the
        // element size in bytes, so a run whose length is a product is one whose
        // elements this description cannot size on its own — growing it means
        // choosing a stride, which is an invention.
        CountSource::Product { count, stride } => Err(EditError::LengthNotOurs {
            field: field.to_string(),
            because: format!(
                "its length is `{count}` x `{stride}`, so resizing it would mean \
                 choosing a stride"
            ),
        }),
        // ⚠ Refused rather than treated as `Field(count)`: the array's length is
        // not the count field's value, and writing one into the other puts the
        // prefix entry into the count.
        CountSource::PrefixedAboveOne { count } => Err(EditError::LengthNotOurs {
            field: field.to_string(),
            because: format!(
                "its length is `{count}` plus a prefix entry above one, so the \
                 count field is not the length"
            ),
        }),
        // ⚠ Refused: the run is several parts back to back, and which part an
        // insertion grows is not something the description says.
        CountSource::Sum { fields } => Err(EditError::LengthNotOurs {
            field: field.to_string(),
            because: format!(
                "its length is the sum of `{}`, so resizing it would mean choosing a part",
                fields.join("` + `")
            ),
        }),
        CountSource::Fixed(n) => Err(EditError::LengthNotOurs {
            field: field.to_string(),
            because: format!("the description says it is always {n} long"),
        }),
        CountSource::Inline => Err(EditError::LengthNotOurs {
            field: field.to_string(),
            because: "it is inline, so its length is part of the enclosing struct".into(),
        }),
        CountSource::Unknown => Err(EditError::LengthNotOurs {
            field: field.to_string(),
            because: "the description does not say how long it is".into(),
        }),
    }
}

fn type_name_of(value: &Value) -> Result<&str> {
    value.type_name().ok_or_else(|| EditError::NotAnArray {
        field: "<root>".into(),
    })
}

fn record_of(value: &Value) -> Result<&Record> {
    value.record().ok_or_else(|| EditError::NotAnArray {
        field: "<root>".into(),
    })
}

/// The integer field `name` of `owner`, or why it cannot hold a count.
fn int_kind(owner: &Record, name: &str) -> Result<()> {
    match owner.plan().field(name) {
        Some(field) if matches!(field.kind, Some(Kind::Int(_))) => Ok(()),
        Some(_) => Err(EditError::CountNotAnInteger {
            field: name.to_string(),
            count_field: name.to_string(),
        }),
        None => Err(EditError::NoField {
            type_name: owner.type_name().to_string(),
            field: name.to_string(),
        }),
    }
}

fn int_field(owner: &Value, name: &str) -> Result<i128> {
    let record = record_of(owner)?;
    int_kind(record, name)?;
    record.int(name).ok_or_else(|| EditError::NoField {
        type_name: record.type_name().to_string(),
        field: name.to_string(),
    })
}

/// A run to resize, whichever way the tree holds it.
enum RunMut<'v> {
    Values(&'v mut Vec<Value>),
    Packed(&'v mut RecordRun),
}

impl RunMut<'_> {
    fn len(&self) -> usize {
        match self {
            RunMut::Values(elements) => elements.len(),
            RunMut::Packed(run) => run.len(),
        }
    }

    /// Why `element` cannot go in this run, found before anything is written.
    fn refuses(&self, field: &str, element: &Value) -> Result<()> {
        let RunMut::Packed(run) = self else {
            return Ok(());
        };
        let refuse = |because: String| EditError::NotAnElement {
            field: field.to_string(),
            because,
        };
        let Some(record) = element.record() else {
            return Err(refuse(format!(
                "it is a packed run of `{}`, and this is not a struct",
                run.type_name()
            )));
        };
        if record.type_name() != run.type_name()
            || record.pack_id() != run.pack_id()
            || !record.children().is_empty()
        {
            return Err(refuse(format!(
                "it is a packed run of `{}` from `{}`, and this is a `{}` from `{}`",
                run.type_name(),
                run.pack_id(),
                record.type_name(),
                record.pack_id()
            )));
        }
        Ok(())
    }

    fn insert(self, field: &str, at: usize, element: Value) -> Result<()> {
        match self {
            RunMut::Values(elements) => {
                elements.insert(at, element);
                Ok(())
            }
            RunMut::Packed(run) => {
                let record = element.record().ok_or_else(|| EditError::NotAnElement {
                    field: field.to_string(),
                    because: "not a struct".into(),
                })?;
                run.insert(at, record).map_err(|e| EditError::NotAnElement {
                    field: field.to_string(),
                    because: e.to_string(),
                })
            }
        }
    }

    fn remove(self, at: usize) -> Value {
        match self {
            RunMut::Values(elements) => elements.remove(at),
            RunMut::Packed(run) => {
                Value::Struct(run.remove(at).expect("an index checked against the run"))
            }
        }
    }
}

fn elements_mut<'v>(owner: &'v mut Value, field: &str) -> Result<RunMut<'v>> {
    let slot = owner
        .child_mut(field)
        .ok_or_else(|| EditError::NotAnArray {
            field: field.to_string(),
        })?;
    let array = match slot {
        Value::Ptr { .. } => slot.target_mut().ok_or_else(|| EditError::LengthNotOurs {
            field: field.to_string(),
            because: "the pointer is null, so there is no run to edit. Author one first.".into(),
        })?,
        other => other,
    };
    match array {
        Value::Array { elements, .. } => Ok(RunMut::Values(elements)),
        Value::Records(run) => Ok(RunMut::Packed(run)),
        _ => Err(EditError::NotAnArray {
            field: field.to_string(),
        }),
    }
}

fn set_int(owner: &mut Value, name: &str, to: i128) -> Result<()> {
    let record = owner.record_mut().ok_or_else(|| EditError::NoField {
        type_name: "<this value>".into(),
        field: name.to_string(),
    })?;
    int_kind(record, name)?;
    // A count lands in the record's bytes at the field's own width, so a
    // length that does not fit is refused here rather than truncated.
    record
        .set_structural(name, ScalarValue::Int(to))
        .map_err(|_| EditError::PastCapacity {
            field: name.to_string(),
            capacity: 0,
            wanted: usize::try_from(to).unwrap_or(usize::MAX),
        })
}

/// Record `len` wherever this run's length lives, leaving everything else alone.
fn record_length(owner: &mut Value, field: &str, length: &LengthIsIn, len: usize) -> Result<()> {
    match length {
        LengthIsIn::Field(name) => set_int(owner, name, len as i128),
        LengthIsIn::FieldWithCapacity { count, capacity } => {
            let declared = int_field(owner, capacity)?;
            if declared >= 0 && (len as i128) > declared {
                return Err(EditError::PastCapacity {
                    field: field.to_string(),
                    capacity: declared as u128,
                    wanted: len,
                });
            }
            // ⚠ The capacity is **not** touched: it is the size of the allocation
            // the game expects, and lowering it to match a shorter run shrinks
            // that allocation.
            set_int(owner, count, len as i128)
        }
        LengthIsIn::Masked { field: name, mask } => {
            let whole = int_field(owner, name)?;
            let shift = mask.trailing_zeros();
            let mask = *mask as i128;
            let widest = mask >> shift;
            if (len as i128) > widest {
                return Err(EditError::PastCapacity {
                    field: field.to_string(),
                    capacity: widest as u128,
                    wanted: len,
                });
            }
            // ⚠ Read-modify-write. The bits outside the mask are somebody
            // else's — usually flags — and writing the length over the whole
            // word leaves a file that still reads and behaves differently.
            let kept = whole & !mask;
            set_int(owner, name, kept | ((len as i128) << shift))
        }
    }
}

/// Append `element` to `field`'s run, and record the new length.
pub fn insert(pack: &Pack, owner: &mut Value, field: &str, element: Value) -> Result<()> {
    let at = elements_mut(owner, field)?.len();
    insert_at(pack, owner, field, at, element)
}

/// Insert `element` at `at`, and record the new length.
///
/// ⚠ Order is preserved: an index into this run may be stored elsewhere in the
/// same file — a bone index, a shader index, a matrix-palette entry — and a
/// reordering insert silently repoints every one of them.
pub fn insert_at(
    pack: &Pack,
    owner: &mut Value,
    field: &str,
    at: usize,
    element: Value,
) -> Result<()> {
    let length = length_of(pack, type_name_of(owner)?, field)?;

    // The length is recorded first so a refusal leaves the run untouched: an
    // insert that grew the array and then failed to record it leaves a run
    // longer than its count.
    let run = elements_mut(owner, field)?;
    run.refuses(field, &element)?;
    let wanted = run.len() + 1;
    if at >= wanted {
        let len = wanted - 1;
        if at > len {
            return Err(EditError::OutOfRange {
                field: field.to_string(),
                index: at,
                len,
            });
        }
    }
    record_length(owner, field, &length, wanted)?;
    elements_mut(owner, field)?.insert(field, at, element)
}

/// Take the element at `at` out of `field`'s run, and record the new length.
///
/// Hands the element back so an undo can put it where it was.
pub fn remove(pack: &Pack, owner: &mut Value, field: &str, at: usize) -> Result<Value> {
    let length = length_of(pack, type_name_of(owner)?, field)?;

    let len = elements_mut(owner, field)?.len();
    if at >= len {
        return Err(EditError::OutOfRange {
            field: field.to_string(),
            index: at,
            len,
        });
    }
    record_length(owner, field, &length, len - 1)?;
    Ok(elements_mut(owner, field)?.remove(at))
}

/// Why `field`'s run cannot be resized, or `None` when it can.
pub fn why_not_resizable(pack: &Pack, owner: &Value, field: &str) -> Option<String> {
    let Some(type_name) = owner.type_name() else {
        return Some("this is not a struct".into());
    };
    match length_of(pack, type_name, field) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    }
}

/// The names of `type_name`'s fields whose runs can be resized.
pub fn resizable_fields(pack: &Pack, type_name: &str) -> Vec<String> {
    pack.type_by_name(type_name)
        .map(|ty| {
            ty.fields
                .iter()
                .filter(|f| length_of(pack, type_name, &f.name).is_ok())
                .map(|f| f.name.clone())
                .collect()
        })
        .unwrap_or_default()
}
