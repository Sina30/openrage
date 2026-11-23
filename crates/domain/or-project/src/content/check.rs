//! What a kind of content checks of a build once it is written: one line each, with whether it held.

/// One thing that was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    /// What it found: what held, or what did not.
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verification {
    pub checks: Vec<Check>,
}

impl Verification {
    /// Record the check `name`: it held when `problems` is empty, and says `held` then.
    pub fn add(&mut self, name: impl Into<String>, problems: Vec<String>, held: String) {
        let passed = problems.is_empty();
        self.checks.push(Check {
            name: name.into(),
            passed,
            detail: if passed { held } else { problems.join("; ") },
        });
    }

    pub fn is_clean(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    /// One sentence for each check that failed.
    pub fn problems(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect()
    }
}
