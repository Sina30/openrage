use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        })
    }
}

/// One named value a diagnostic carries alongside its sentence, typed so a
/// consumer reads it rather than parsing it back out of the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Int(i128),
    /// An address or offset, so a report can render it as hex without guessing
    /// from the magnitude.
    Addr(u64),
    Text(String),
}

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arg::Int(n) => write!(f, "{n}"),
            Arg::Addr(a) => write!(f, "{a:#x}"),
            Arg::Text(s) => f.write_str(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub key: &'static str,

    pub severity: Severity,

    pub message: String,

    pub location: Option<String>,

    /// The values the message was built from, in the order they were added;
    /// grouping uses their *names* only, see [`Diagnostic::signature`].
    pub args: Vec<(&'static str, Arg)>,
}

impl Diagnostic {
    pub fn new(severity: Severity, key: &'static str, message: impl Into<String>) -> Self {
        Self {
            key,
            severity,
            message: message.into(),
            location: None,
            args: Vec::new(),
        }
    }

    pub fn with(mut self, name: &'static str, value: impl Into<Arg>) -> Self {
        self.args.push((name, value.into()));
        self
    }

    pub fn arg(&self, name: &str) -> Option<&Arg> {
        self.args.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    /// What makes two diagnostics the same *defect*: the key plus the argument
    /// *names*, independent of both the wording and the values.
    ///
    /// ⚠ Grouping by message shape — masking every word containing a digit —
    /// splits one defect into a line per field it names (`GrmShader::params out
    /// of bounds` against `GrmShader::textures out of bounds`) and merges any two
    /// messages differing only in numbers.
    /// `the_signature_groups_what_a_message_shape_cannot` holds this.
    pub fn signature(&self) -> String {
        let mut names: Vec<&str> = self.args.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        if names.is_empty() {
            return self.key.to_string();
        }
        format!("{}({})", self.key, names.join(","))
    }

    pub fn info(key: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, key, message)
    }

    pub fn warning(key: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, key, message)
    }

    pub fn error(key: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, key, message)
    }

    pub fn at(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }
}

impl From<i128> for Arg {
    fn from(n: i128) -> Self {
        Arg::Int(n)
    }
}

impl From<i64> for Arg {
    fn from(n: i64) -> Self {
        Arg::Int(i128::from(n))
    }
}

impl From<u32> for Arg {
    fn from(n: u32) -> Self {
        Arg::Int(i128::from(n))
    }
}

impl From<usize> for Arg {
    fn from(n: usize) -> Self {
        Arg::Int(n as i128)
    }
}

impl From<&str> for Arg {
    fn from(s: &str) -> Self {
        Arg::Text(s.to_string())
    }
}

impl From<String> for Arg {
    fn from(s: String) -> Self {
        Arg::Text(s)
    }
}

// ⚠ No `From<u64>` for `Arg`: a `u64` here is nearly always an address or an
// offset, which a report wants in hex, so it goes through `Arg::Addr` by name
// rather than silently decimal. A genuine `u64` count converts with
// `Arg::Int(n as i128)`.
impl From<Arg> for String {
    fn from(arg: Arg) -> Self {
        arg.to_string()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.key, self.message)?;
        if let Some(location) = &self.location {
            write!(f, " (at {location})")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    messages: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.messages.push(diagnostic);
    }

    pub fn info(&mut self, key: &'static str, message: impl Into<String>) {
        self.push(Diagnostic::info(key, message));
    }

    pub fn warn(&mut self, key: &'static str, message: impl Into<String>) {
        self.push(Diagnostic::warning(key, message));
    }

    pub fn error(&mut self, key: &'static str, message: impl Into<String>) {
        self.push(Diagnostic::error(key, message));
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.messages.extend(other.messages);
    }

    pub fn messages(&self) -> &[Diagnostic] {
        &self.messages
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn fatal(&self) -> Option<&Diagnostic> {
        self.messages.iter().find(|d| d.severity == Severity::Error)
    }

    pub fn has_errors(&self) -> bool {
        self.fatal().is_some()
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.messages.iter().any(|d| d.key == key)
    }

    pub fn at_least(&self, severity: Severity) -> impl Iterator<Item = &Diagnostic> {
        self.messages.iter().filter(move |d| d.severity >= severity)
    }

    pub fn key_counts(&self) -> std::collections::BTreeMap<&'static str, usize> {
        let mut counts = std::collections::BTreeMap::new();
        for message in &self.messages {
            *counts.entry(message.key).or_insert(0) += 1;
        }
        counts
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = Diagnostic>>(iter: T) -> Self {
        Self {
            messages: iter.into_iter().collect(),
        }
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, message) in self.messages.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{message}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome<T> {
    pub value: T,
    pub diagnostics: Diagnostics,
}

impl<T> Outcome<T> {
    pub fn clean(value: T) -> Self {
        Self {
            value,
            diagnostics: Diagnostics::new(),
        }
    }

    pub fn new(value: T, diagnostics: Diagnostics) -> Self {
        Self { value, diagnostics }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Outcome<U> {
        Outcome {
            value: f(self.value),
            diagnostics: self.diagnostics,
        }
    }

    pub fn into_value(self, sink: &mut Diagnostics) -> T {
        sink.extend(self.diagnostics);
        self.value
    }

    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fatal_message_is_still_in_the_list() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.error("BAD_MAGIC", "not an RSC7 resource");

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics.has_errors());
        assert_eq!(diagnostics.fatal().unwrap().key, "BAD_MAGIC");
        assert_eq!(diagnostics.messages()[0].key, "BAD_MAGIC");
    }

    #[test]
    fn warnings_survive_a_successful_parse() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.warn("MIP_COUNT_MISMATCH", "header says 9 mips, chain has 8");

        let outcome = Outcome::new(42u32, diagnostics);

        assert_eq!(outcome.value, 42);
        assert!(!outcome.is_clean());
        assert!(!outcome.diagnostics.has_errors());
        assert!(outcome.diagnostics.has_key("MIP_COUNT_MISMATCH"));
    }

    #[test]
    fn nested_diagnostics_reach_the_outer_report() {
        let mut inner = Diagnostics::new();
        inner.warn("ENTRY_SKIPPED", "entry 3 has a null pointer");
        let child = Outcome::new("child", inner);

        let mut outer = Diagnostics::new();
        let value = child.into_value(&mut outer);

        assert_eq!(value, "child");
        assert!(
            outer.has_key("ENTRY_SKIPPED"),
            "an inner parse's warnings must not be dropped at the boundary"
        );
    }

    #[test]
    fn keys_are_matchable_and_countable() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.warn("ENTRY_SKIPPED", "entry 1");
        diagnostics.warn("ENTRY_SKIPPED", "entry 2");
        diagnostics.info("SHADER_UNKNOWN", "shader_38DD00DF");

        let counts = diagnostics.key_counts();
        assert_eq!(counts["ENTRY_SKIPPED"], 2);
        assert_eq!(counts["SHADER_UNKNOWN"], 1);
    }

    #[test]
    fn severity_filtering_is_ordered() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.info("A", "a");
        diagnostics.warn("B", "b");
        diagnostics.error("C", "c");

        assert_eq!(diagnostics.at_least(Severity::Warning).count(), 2);
        assert_eq!(diagnostics.at_least(Severity::Error).count(), 1);
    }

    #[test]
    fn display_renders_key_and_location() {
        let diagnostic =
            Diagnostic::warning("BOUND_GEOM_NO_VERTICES", "bound has no vertices").at("child 4");
        assert_eq!(
            diagnostic.to_string(),
            "[BOUND_GEOM_NO_VERTICES] bound has no vertices (at child 4)"
        );
    }
}
