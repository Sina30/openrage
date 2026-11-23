//! How serious a rule of a kind of content is, and the row each kind keeps its rules in.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// The number a rule holds a value to, which a person may set again for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Threshold {
    /// What it is until a project says otherwise.
    pub default: u64,
    /// What the number counts, in words: `pixels`, `vertices`, `bytes`.
    pub unit: &'static str,
}

/// A kind of problem a kind of content can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub code: &'static str,
    pub severity: Severity,
    /// Whether a waiver can set a finding of the rule aside.
    pub waivable: bool,
    /// What it means, in a sentence.
    pub summary: &'static str,
    /// The limit it holds a value to, for a rule that has one.
    pub threshold: Option<Threshold>,
}

impl Rule {
    pub const fn new(
        code: &'static str,
        severity: Severity,
        waivable: bool,
        summary: &'static str,
    ) -> Self {
        Self {
            code,
            severity,
            waivable,
            summary,
            threshold: None,
        }
    }

    /// What a screen calls the rule in place of its code: the summary as a sentence with a capital letter.
    pub fn sentence(&self) -> String {
        let mut letters = self.summary.chars();
        match letters.next() {
            Some(first) => first.to_uppercase().chain(letters).collect(),
            None => String::new(),
        }
    }

    /// The same rule, holding a value to `default` `unit`.
    pub const fn with_threshold(self, default: u64, unit: &'static str) -> Self {
        Self {
            threshold: Some(Threshold { default, unit }),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_has_a_limit_only_when_it_is_given_one() {
        let plain = Rule::new("a", Severity::Error, false, "says a");
        assert_eq!(plain.threshold, None);
        let sized = Rule::new("b", Severity::Warning, true, "says b").with_threshold(512, "pixels");
        assert_eq!(
            sized.threshold,
            Some(Threshold {
                default: 512,
                unit: "pixels"
            })
        );
        assert_eq!(
            (sized.code, sized.severity, sized.waivable),
            ("b", Severity::Warning, true)
        );
    }

    #[test]
    fn a_rule_is_called_by_its_summary_as_a_sentence_and_never_by_its_code() {
        let rule = Rule::new("slot-cap", Severity::Warning, true, "a slot is full");
        assert_eq!(rule.sentence(), "A slot is full");
        assert!(!rule.sentence().contains(rule.code));
    }
}
