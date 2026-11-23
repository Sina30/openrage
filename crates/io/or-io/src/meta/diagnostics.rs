use std::collections::BTreeMap;
use std::fmt;

use crate::meta::error::{MetaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorMode {
    #[default]
    Lenient,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GapKind {
    UnknownDataType,
    UnknownSubType,
    UnsupportedArrayElement,
    MissingSchema,
    UnsupportedMap,
    FieldDecodeError,
    LossyDecode,
}

impl GapKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GapKind::UnknownDataType => "unknown_data_type",
            GapKind::UnknownSubType => "unknown_subtype",
            GapKind::UnsupportedArrayElement => "unsupported_array_element",
            GapKind::MissingSchema => "missing_schema",
            GapKind::UnsupportedMap => "unsupported_map",
            GapKind::FieldDecodeError => "field_decode_error",
            GapKind::LossyDecode => "lossy_decode",
        }
    }
}

impl fmt::Display for GapKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub kind: GapKind,
    pub detail: String,
    pub path: String,
}

impl fmt::Display for Gap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}: {}", self.kind, self.path, self.detail)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    mode: ErrorMode,
    gaps: Vec<Gap>,
    path: Vec<String>,
}

impl Diagnostics {
    pub fn new(mode: ErrorMode) -> Self {
        Self {
            mode,
            gaps: Vec::new(),
            path: Vec::new(),
        }
    }

    pub fn lenient() -> Self {
        Self::new(ErrorMode::Lenient)
    }

    pub fn strict() -> Self {
        Self::new(ErrorMode::Strict)
    }

    pub fn mode(&self) -> ErrorMode {
        self.mode
    }

    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    pub fn is_clean(&self) -> bool {
        self.gaps.is_empty()
    }

    pub fn push_path(&mut self, segment: impl Into<String>) {
        self.path.push(segment.into());
    }

    pub fn pop_path(&mut self) {
        self.path.pop();
    }

    pub fn current_path(&self) -> String {
        self.path.join(".")
    }

    pub fn record(&mut self, kind: GapKind, detail: impl Into<String>) -> Result<()> {
        let gap = Gap {
            kind,
            detail: detail.into(),
            path: self.current_path(),
        };
        if self.mode == ErrorMode::Strict {
            return Err(MetaError::Other(format!("strict: {gap}")));
        }
        tracing::warn!(kind = %gap.kind, path = %gap.path, detail = %gap.detail, "meta reader gap");
        self.gaps.push(gap);
        Ok(())
    }

    pub fn extend(&mut self, other: &Diagnostics) {
        self.gaps.extend(other.gaps.iter().cloned());
    }

    pub fn census(&self) -> Vec<((GapKind, String), usize)> {
        let mut counts: BTreeMap<(GapKind, String), usize> = BTreeMap::new();
        for gap in &self.gaps {
            *counts.entry((gap.kind, gap.detail.clone())).or_insert(0) += 1;
        }
        let mut out: Vec<_> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_collects_and_strict_errors() {
        let mut lenient = Diagnostics::lenient();
        lenient
            .record(GapKind::UnknownDataType, "type 0x99")
            .expect("lenient must not error");
        assert_eq!(lenient.gaps().len(), 1);

        let mut strict = Diagnostics::strict();
        assert!(strict
            .record(GapKind::UnknownDataType, "type 0x99")
            .is_err());
    }

    #[test]
    fn path_tracks_the_subtree() {
        let mut d = Diagnostics::lenient();
        d.push_path("CRoot");
        d.push_path("m_field");
        assert_eq!(d.current_path(), "CRoot.m_field");
        d.record(GapKind::MissingSchema, "nope").unwrap();
        d.pop_path();
        assert_eq!(d.current_path(), "CRoot");
        assert_eq!(d.gaps()[0].path, "CRoot.m_field");
    }

    #[test]
    fn census_ranks_by_frequency_then_key() {
        let mut d = Diagnostics::lenient();
        for _ in 0..3 {
            d.record(GapKind::UnsupportedArrayElement, "Matrix34")
                .unwrap();
        }
        d.record(GapKind::UnknownDataType, "0x21").unwrap();
        let census = d.census();
        assert_eq!(census[0].1, 3);
        assert_eq!(census[0].0 .1, "Matrix34");
        assert_eq!(census[1].1, 1);
    }
}
