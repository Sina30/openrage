//! A ceiling on what a census will hold in memory, and the line it prints
//! when it stops short.
//!
//! ⚠ **A census that silently truncates is worse than one that refuses**: a
//! figure from a truncated run reads as a measurement over an install and is
//! one over however much fitted. A command that reaches its ceiling must say so
//! through [`Cap::note`].
//!
//! ⚠ **The ceiling may not be removable by accident.** A flag defaulting to
//! zero and meaning *unlimited* is one forgotten default away from an unbounded
//! run, which is how a single process came to hold 38.4 GB of a 61.6 GB
//! machine. Unlimited is spelled [`UNLIMITED`], as a word, and nothing else
//! means it.

/// What a caller writes to lift the ceiling, knowing what it costs.
pub(crate) const UNLIMITED: &str = "unlimited";

/// What a ceiling counts.
///
/// ⚠ **The unit is a real distinction.** `names` and `search` bound how many
/// *results* they hold, because a result is a fixed cost. `formats` bounds
/// *bytes classified*, because a file is not a unit of anything there: an
/// install's `.ytd` and `.ydr` differ by four orders of magnitude and a file
/// count is generous on one and useless on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapUnit {
    /// A plain count, named for the report.
    Count(&'static str),
    /// Bytes, written and read as GiB.
    Gibibytes,
}

impl CapUnit {
    fn render(&self, amount: u64) -> String {
        match self {
            Self::Count(noun) => format!("{amount} {noun}"),
            Self::Gibibytes => format!("{:.1} GiB", amount as f64 / (1024.0 * 1024.0 * 1024.0)),
        }
    }

    fn takes(&self) -> &'static str {
        match self {
            Self::Count(_) => "a count",
            Self::Gibibytes => "a size in GiB",
        }
    }
}

/// A ceiling, and whether the caller asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cap {
    limit: Option<u64>,
    unit: CapUnit,
}

impl Cap {
    /// A cap at its command's default, counting `unit`.
    pub(crate) const fn new(default: u64, unit: CapUnit) -> Self {
        Self {
            limit: Some(default),
            unit,
        }
    }

    /// A cap at an explicit amount, for a caller that parsed its own request.
    pub(crate) const fn at(limit: u64, unit: CapUnit) -> Self {
        Self {
            limit: Some(limit),
            unit,
        }
    }

    /// Parse a flag value: an amount in this cap's unit, or [`UNLIMITED`].
    ///
    /// ⚠ **Zero is an error rather than a synonym for unlimited.** It is the
    /// one value a forgotten initialiser produces, so accepting it as *no
    /// ceiling* makes the ceiling removable by accident.
    pub(crate) fn parse(&self, flag: &str, value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case(UNLIMITED) {
            return Ok(self.lifted());
        }
        let parsed = match self.unit {
            CapUnit::Count(_) => trimmed.parse::<u64>().ok(),
            CapUnit::Gibibytes => trimmed
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| (n * 1024.0 * 1024.0 * 1024.0) as u64),
        };
        match parsed {
            Some(0) => Err(format!(
                "{flag}=0 is not a ceiling. Write `{flag}={UNLIMITED}` if that is \
                 what you mean — a whole install can hold more than this machine"
            )),
            Some(n) => Ok(Self {
                limit: Some(n),
                unit: self.unit,
            }),
            None => Err(format!(
                "{flag} takes {} or `{UNLIMITED}`; got `{value}`",
                self.unit.takes()
            )),
        }
    }

    /// The same cap with no ceiling, for a command whose override is its own
    /// named flag rather than [`UNLIMITED`].
    pub(crate) const fn lifted(&self) -> Self {
        Self {
            limit: None,
            unit: self.unit,
        }
    }

    /// Whether `held` has reached the ceiling.
    pub(crate) fn reached(&self, held: u64) -> bool {
        self.limit.is_some_and(|limit| held >= limit)
    }

    pub(crate) fn is_unlimited(&self) -> bool {
        self.limit.is_none()
    }

    pub(crate) fn limit(&self) -> Option<u64> {
        self.limit
    }

    /// The ceiling as this cap's unit reads it, for a usage line.
    pub(crate) fn rendered_limit(&self) -> String {
        match self.limit {
            Some(limit) => self.unit.render(limit),
            None => UNLIMITED.to_string(),
        }
    }

    /// The line a report has to carry when the run stopped short.
    ///
    /// `None` when it did not, so a caller can `if let` rather than deciding
    /// what "not truncated" reads as.
    pub(crate) fn note(&self, held: u64, flag: &str) -> Option<String> {
        let limit = self.limit?;
        (held >= limit).then(|| {
            format!(
                "STOPPED SHORT at {}: this is not a figure over the whole install. \
                 Raise it with `{flag}=<n>` or `{flag}={UNLIMITED}`, and mind that \
                 the default is what keeps the run inside memory.",
                self.unit.render(limit)
            )
        })
    }

    /// The same, as the two fields a JSON document should carry.
    pub(crate) fn json(&self, held: u64) -> (bool, Option<u64>) {
        (self.reached(held), self.limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAG: &str = "--max-things";
    const THINGS: CapUnit = CapUnit::Count("things");
    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn a_count_and_the_word_are_the_only_two_spellings() {
        let cap = Cap::new(10, THINGS);
        assert_eq!(cap.parse(FLAG, "5").unwrap().limit(), Some(5));
        assert_eq!(cap.parse(FLAG, " 5 ").unwrap().limit(), Some(5));
        assert!(cap.parse(FLAG, "unlimited").unwrap().is_unlimited());
        assert!(cap.parse(FLAG, "UNLIMITED").unwrap().is_unlimited());
        assert!(cap.parse(FLAG, "lots").is_err());
    }

    /// ⚠ Zero cannot mean infinity, in **every** unit, including the one whose
    /// flag takes a fraction — `--budget-gib 0.0` is the same forgotten
    /// initialiser.
    #[test]
    fn zero_is_refused_rather_than_read_as_unlimited() {
        for (cap, zero) in [
            (Cap::new(10, THINGS), "0"),
            (Cap::new(2 * GIB, CapUnit::Gibibytes), "0"),
            (Cap::new(2 * GIB, CapUnit::Gibibytes), "0.0"),
            // Rounds to zero bytes, so it is a ceiling that stops the census
            // on its first file while reading as a request for some.
            (Cap::new(2 * GIB, CapUnit::Gibibytes), "0.0000000001"),
        ] {
            let why = cap.parse(FLAG, zero).expect_err("zero is not a ceiling");
            assert!(why.contains(UNLIMITED), "{zero}: {why}");
        }
    }

    /// A byte budget is written in GiB and held in bytes.
    #[test]
    fn a_byte_budget_reads_gib_and_reports_gib() {
        let cap = Cap::new(2 * GIB, CapUnit::Gibibytes);
        assert_eq!(cap.parse(FLAG, "0.5").unwrap().limit(), Some(GIB / 2));
        assert_eq!(cap.rendered_limit(), "2.0 GiB");
        assert!(cap.parse(FLAG, "-1").is_err());
        assert!(cap.parse(FLAG, "some").is_err());
        let note = cap.note(3 * GIB, FLAG).expect("it stopped short");
        assert!(note.contains("2.0 GiB"), "{note}");
    }

    #[test]
    fn a_cap_is_reached_at_the_limit_and_not_before() {
        let cap = Cap::new(3, THINGS);
        assert!(!cap.reached(2));
        assert!(cap.reached(3));
        assert!(cap.reached(4));
        assert!(!Cap::new(3, THINGS)
            .parse(FLAG, UNLIMITED)
            .unwrap()
            .reached(1 << 40));
        assert!(!Cap::new(3, THINGS).lifted().reached(1 << 40));
    }

    /// ⚠ The half that makes a truncated figure safe to read.
    #[test]
    fn a_short_run_says_so_and_a_complete_one_says_nothing() {
        let cap = Cap::new(3, THINGS);
        assert!(cap.note(2, FLAG).is_none());
        let note = cap.note(3, FLAG).expect("it stopped short");
        assert!(note.contains("STOPPED SHORT"), "{note}");
        assert!(
            note.contains("not a figure over the whole install"),
            "{note}"
        );
        assert!(note.contains(FLAG), "{note}");
        assert!(note.contains("3 things"), "{note}");

        let lifted = cap.parse(FLAG, UNLIMITED).unwrap();
        assert!(lifted.note(1 << 40, FLAG).is_none());
        assert_eq!(lifted.json(1 << 40), (false, None));
    }

    #[test]
    fn json_carries_both_the_flag_and_the_ceiling() {
        assert_eq!(Cap::new(3, THINGS).json(3), (true, Some(3)));
        assert_eq!(Cap::new(3, THINGS).json(1), (false, Some(3)));
    }
}
