use crate::desc::value::ScalarValue;
use crate::desc::{FieldRepr, ImageMut, Pack, PackType, Scalar, WriteError};

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub field: String,
    pub value: ScalarValue,
}

impl Edit {
    pub fn new(field: impl Into<String>, value: ScalarValue) -> Self {
        Self {
            field: field.into(),
            value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    NoType {
        type_name: String,
    },
    NoField {
        type_name: String,
        field: String,
    },
    Structural {
        type_name: String,
        field: String,
    },
    BitPacked {
        type_name: String,
        field: String,
    },
    NotAScalar {
        type_name: String,
        field: String,
    },
    NotADeclaredValue {
        type_name: String,
        field: String,
        value: i64,
    },
    Refused {
        type_name: String,
        field: String,
        reason: WriteError,
    },
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoType { type_name } => {
                write!(f, "the pack describes no type named `{type_name}`")
            }
            Self::NoField { type_name, field } => {
                write!(f, "`{type_name}` has no field named `{field}`")
            }
            Self::Structural { type_name, field } => write!(
                f,
                "`{type_name}::{field}` is structural -- a count, a capacity or a \
                 tagged base's selector -- and changing it needs a rebuild rather \
                 than a patch"
            ),
            Self::BitPacked { type_name, field } => write!(
                f,
                "`{type_name}::{field}` shares its word with other values, so writing \
                 it whole would rewrite them"
            ),
            Self::NotAScalar { type_name, field } => write!(
                f,
                "`{type_name}::{field}` is not a scalar, and everything else moves \
                 something"
            ),
            Self::NotADeclaredValue {
                type_name,
                field,
                value,
            } => write!(
                f,
                "{value} is not a value the enum behind `{type_name}::{field}` declares"
            ),
            Self::Refused {
                type_name,
                field,
                reason,
            } => write!(f, "`{type_name}::{field}` was not written: {reason}"),
        }
    }
}

impl std::error::Error for PatchError {}

impl From<PatchError> for crate::IoError {
    fn from(error: PatchError) -> Self {
        crate::IoError::SerializationFailed {
            offset: 0,
            message: error.to_string(),
        }
    }
}

pub fn patch(
    pack: &Pack,
    type_name: &str,
    image: &mut ImageMut<'_>,
    at: usize,
    edits: &[Edit],
) -> Result<(), PatchError> {
    let ty = pack
        .type_by_name(type_name)
        .ok_or_else(|| PatchError::NoType {
            type_name: type_name.to_string(),
        })?;

    let mut planned = Vec::with_capacity(edits.len());
    for edit in edits {
        planned.push(plan(pack, ty, edit)?);
    }

    for (offset, scalar, value, edit) in planned {
        image
            .put_scalar(at + offset, scalar, value)
            .map_err(|reason| PatchError::Refused {
                type_name: ty.name.clone(),
                field: edit.clone(),
                reason,
            })?;
    }
    Ok(())
}

fn plan(
    pack: &Pack,
    ty: &PackType,
    edit: &Edit,
) -> Result<(usize, Scalar, ScalarValue, String), PatchError> {
    let field = ty
        .field_by_name(&edit.field)
        .ok_or_else(|| PatchError::NoField {
            type_name: ty.name.clone(),
            field: edit.field.clone(),
        })?;

    if ty.is_structural(&field.name) {
        return Err(PatchError::Structural {
            type_name: ty.name.clone(),
            field: field.name.clone(),
        });
    }
    if field.is_bit_packed() {
        return Err(PatchError::BitPacked {
            type_name: ty.name.clone(),
            field: field.name.clone(),
        });
    }

    let scalar = match &field.repr {
        FieldRepr::Scalar(scalar) => *scalar,

        FieldRepr::Enum { type_name, width } => {
            let value = match edit.value {
                ScalarValue::Int(v) => v,
                _ => {
                    return Err(PatchError::Refused {
                        type_name: ty.name.clone(),
                        field: field.name.clone(),
                        reason: WriteError::WrongKind,
                    })
                }
            };
            let declared = pack
                .enum_by_name(type_name)
                .ok_or_else(|| PatchError::NoType {
                    type_name: type_name.clone(),
                })?;
            let value = i64::try_from(value).map_err(|_| PatchError::NotADeclaredValue {
                type_name: ty.name.clone(),
                field: field.name.clone(),
                value: i64::MAX,
            })?;
            if declared.name_of(value).is_none() {
                return Err(PatchError::NotADeclaredValue {
                    type_name: ty.name.clone(),
                    field: field.name.clone(),
                    value,
                });
            }
            Scalar::Int(*width)
        }

        _ => {
            return Err(PatchError::NotAScalar {
                type_name: ty.name.clone(),
                field: field.name.clone(),
            })
        }
    };

    Ok((field.offset, scalar, edit.value.clone(), field.name.clone()))
}
