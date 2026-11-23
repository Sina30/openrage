use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetaError {
    #[error("xml parse error: {0}")]
    Xml(String),

    #[error("schema type `{0}` not found in library")]
    UnknownType(String),

    #[error("schema enum `{0}` not found in library")]
    UnknownEnum(String),

    #[error("expected element `{expected}` at root, got `{actual}`")]
    UnexpectedRoot { expected: String, actual: String },

    #[error("invalid attribute value for `{field}`: {value}")]
    InvalidAttribute { field: String, value: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("snapshot decode error: {0}")]
    Snapshot(String),

    /// A field shape the writer recognises but has no encoder for, countable
    /// by [`UnsupportedShape`] rather than by message.
    ///
    /// Neither known shape occurs in retail content — measured: 0 occurrences
    /// across 6,071 files — so both are refused rather than guessed at.
    #[error("cannot write {shape}: {detail}")]
    UnsupportedShape {
        shape: UnsupportedShape,
        detail: String,
    },

    /// The value tree no longer fits the allocation the document arrived
    /// with: an array that grew, a string too long for its buffer, a pointer
    /// that gained or lost a target.
    ///
    /// ⚠ **The only error a caller may retry through synthesis on.**
    /// Re-encoding against the layout a document arrived with is
    /// byte-identical for everything that did not change shape — 3,527 of
    /// 3,527 PSO and 2,338 of 2,338 RBM documents in the staged corpus — and
    /// re-encoding against a synthesised layout is byte-identical for none
    /// of them, so deciding by message would be one reworded error away from
    /// rewriting every file it touches.
    #[error("{0}")]
    OutgrewItsLayout(String),

    #[error("{0}")]
    Other(String),
}

impl MetaError {
    /// Whether retrying against a synthesised layout could get past this.
    pub fn outgrew_its_layout(&self) -> bool {
        matches!(self, MetaError::OutgrewItsLayout(_))
    }

    /// Say where in the document this happened, without losing what it is.
    ///
    /// ⚠ Re-wrapping with [`MetaError::Other`] would turn every
    /// [`MetaError::OutgrewItsLayout`] raised below the root into an ordinary
    /// failure, and the caller that retries against a synthesised layout would
    /// never see the refusal it keys on. Measured: with the wrapper
    /// flattening, 2,338 of 2,338 grown documents reported the right message
    /// under the wrong variant.
    pub fn in_context(self, path: &str) -> Self {
        match self {
            MetaError::OutgrewItsLayout(m) => MetaError::OutgrewItsLayout(format!("{path}: {m}")),
            MetaError::UnsupportedShape { shape, detail } => MetaError::UnsupportedShape {
                shape,
                detail: format!("{path}: {detail}"),
            },
            other => MetaError::Other(format!("{path}: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnsupportedShape {
    PopulatedAtBinaryMap,
    WideString,
}

impl UnsupportedShape {
    pub fn as_str(self) -> &'static str {
        match self {
            UnsupportedShape::PopulatedAtBinaryMap => "populated_at_binary_map",
            UnsupportedShape::WideString => "wide_string",
        }
    }
}

impl std::fmt::Display for UnsupportedShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<quick_xml::Error> for MetaError {
    fn from(value: quick_xml::Error) -> Self {
        MetaError::Xml(value.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for MetaError {
    fn from(value: quick_xml::events::attributes::AttrError) -> Self {
        MetaError::Xml(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MetaError>;
