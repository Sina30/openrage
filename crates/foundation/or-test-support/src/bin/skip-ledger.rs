use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use or_test_support::ledger::{self, SkipReason, SkipRecord};

const LIST_LIMIT: usize = 20;

fn main() -> ExitCode {
    let mut dir: Option<PathBuf> = None;
    let mut keep = false;
    let mut require = false;
    let mut all = false;
    let mut not_run: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => match args.next() {
                Some(value) => dir = Some(PathBuf::from(value)),
                None => {
                    eprintln!("--dir needs a path");
                    return ExitCode::from(2);
                }
            },
            "--not-run" => match args.next() {
                Some(value) => not_run = Some(PathBuf::from(value)),
                None => {
                    eprintln!("--not-run needs a path");
                    return ExitCode::from(2);
                }
            },
            "--keep" => keep = true,
            "--all" => all = true,
            "--require" => require = true,
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown flag: {other}");
                print_usage();
                return ExitCode::from(2);
            }
        }
    }

    let dir = match dir.map(Ok).unwrap_or_else(ledger::ledger_dir) {
        Ok(dir) => dir,
        // ⚠ "Not located" is not "nothing skipped": no record was read here,
        // and no suite could have written one, so the report says neither.
        Err(reason) => {
            eprintln!("\nskip ledger: NOT LOCATED — nothing below, because there is no below.\n");
            eprintln!("{reason}\n");
            eprintln!(
                "This is not a report that nothing was skipped. No records were read,\n\
                 and the suites that would have written them could not either.\n"
            );
            eprintln!("  pass --dir <path>, or set {}.", ledger::SKIP_LEDGER_ENV);
            return ExitCode::from(2);
        }
    };

    let mut records = ledger::read_dir(&dir);
    if let Some(path) = &not_run {
        match std::fs::read_to_string(path) {
            Ok(text) => records.extend(tier_records(&text)),
            // Louder than `read_dir`'s malformed-line policy: an unreadable
            // ledger file loses one suite's skips, while an unreadable
            // `--not-run` loses every tier record, and the report would render
            // as though the tier had run everything.
            Err(err) => {
                eprintln!("--not-run: cannot read {}: {err}", path.display());
                return ExitCode::from(2);
            }
        }
    }
    let report = Report::of(&records);

    print!("{}", report.render(&dir.display().to_string(), all));

    if is_github_actions() {
        emit_annotation(&report);
        write_job_summary(&report);
    }

    if !keep {
        let _ = std::fs::remove_dir_all(&dir);
    }

    if require && report.staged_content_total > 0 {
        eprintln!(
            "--require: {} suite(s) did not run for want of staged content.",
            report.staged_content_suites
        );
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

fn print_usage() {
    eprintln!("skip-ledger — report what a test run did not check");
    eprintln!();
    eprintln!("  --dir <path>      where the ledger was written");
    eprintln!("  --keep            do not clear the ledger after reporting");
    eprintln!("  --require         exit non-zero if anything skipped for want of content");
    eprintln!("  --all             list every check, not just the first few");
    eprintln!("  --not-run <path>  `cargo nextest list` output for the tests a tier excluded");
}

/// One [`SkipReason::Tier`] record per `<binary-id> <test-name>` line of
/// `cargo nextest list`; the binary id (`or-anim`, `or-io::awc_roundtrip`)
/// is already the name the rest of the ledger calls a suite.
fn tier_records(listing: &str) -> Vec<SkipRecord> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (suite, test) = line.split_once(' ')?;
            Some(SkipRecord {
                reason: SkipReason::Tier,
                suite: suite.to_string(),
                what: test.trim().to_string(),
                detail: "not selected by this tier".to_string(),
                site: suite.to_string(),
            })
        })
        .collect()
}

struct Report {
    by_reason: BTreeMap<SkipReason, Vec<SkipRecord>>,
    suites_by_reason: BTreeMap<SkipReason, usize>,
    total: usize,
    suites: usize,
    staged_content_total: usize,
    staged_content_suites: usize,
    tier_total: usize,
    tier_suites: usize,
}

impl Report {
    fn of(records: &[SkipRecord]) -> Self {
        let mut by_reason: BTreeMap<SkipReason, Vec<SkipRecord>> = BTreeMap::new();
        for record in records {
            by_reason
                .entry(record.reason)
                .or_default()
                .push(record.clone());
        }

        let suites_by_reason: BTreeMap<SkipReason, usize> = by_reason
            .iter()
            .map(|(reason, records)| {
                let mut names: Vec<&str> = records.iter().map(|r| r.suite.as_str()).collect();
                names.sort_unstable();
                names.dedup();
                (*reason, names.len())
            })
            .collect();

        let mut all_suites: Vec<&str> = records.iter().map(|r| r.suite.as_str()).collect();
        all_suites.sort_unstable();
        all_suites.dedup();

        let mut content_suites: Vec<&str> = records
            .iter()
            .filter(|r| r.reason.is_about_staged_content())
            .map(|r| r.suite.as_str())
            .collect();
        content_suites.sort_unstable();
        content_suites.dedup();

        let tier_total = by_reason.get(&SkipReason::Tier).map_or(0, Vec::len);
        let tier_suites = suites_by_reason
            .get(&SkipReason::Tier)
            .copied()
            .unwrap_or(0);

        Self {
            by_reason,
            suites_by_reason,
            total: records.len(),
            suites: all_suites.len(),
            staged_content_total: records
                .iter()
                .filter(|r| r.reason.is_about_staged_content())
                .count(),
            staged_content_suites: content_suites.len(),
            tier_total,
            tier_suites,
        }
    }

    fn headline(&self) -> String {
        if self.total == 0 {
            return "skip ledger: nothing skipped — every suite that could run, ran.".to_string();
        }
        format!(
            "skip ledger: {} check(s) across {} suite(s) did not run.",
            self.total, self.suites
        )
    }

    fn render(&self, dir: &str, all: bool) -> String {
        let mut out = String::new();
        out.push('\n');
        out.push_str(&self.headline());
        out.push('\n');

        if self.total == 0 {
            out.push_str(&format!("  (ledger: {dir})\n"));
            return out;
        }

        out.push('\n');
        for (reason, records) in &self.by_reason {
            let suites = self.suites_by_reason.get(reason).copied().unwrap_or(0);
            out.push_str(&format!(
                "  {:>6}  {:>4} check(s), {:>3} suite(s)   remedy: {}\n",
                reason.tag(),
                records.len(),
                suites,
                reason.remedy()
            ));
        }

        out.push('\n');
        let listed: Vec<&SkipRecord> = self.by_reason.values().flatten().collect();
        let limit = if all { listed.len() } else { LIST_LIMIT };
        for record in listed.iter().take(limit) {
            out.push_str(&format!(
                "  [{}] {}: {}  ({})\n",
                record.reason.tag(),
                record.suite,
                record.what,
                record.site
            ));
        }
        if listed.len() > limit {
            out.push_str(&format!(
                "  … and {} more; pass --all to list them\n",
                listed.len() - limit
            ));
        }

        out.push('\n');
        if self.tier_total > 0 {
            out.push_str(&format!(
                "  This tier did not load {} check(s) across {} suite(s). `just test` runs \
                 them.\n",
                self.tier_total, self.tier_suites
            ));
        }
        if self.staged_content_total > 0 {
            out.push_str(
                "  A skipped check is not a passing one. Stage a corpus beside this workspace \
                 to run them,\n  and set OPENRAGE_REQUIRE_FIXTURES=1 to make a skip fail on a \
                 machine that has one.\n",
            );
        }
        out.push_str(&format!("  (ledger: {dir})\n"));
        out
    }

    fn markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("### Suites that did not run\n\n");
        if self.total == 0 {
            out.push_str("Nothing skipped — every suite that could run, ran.\n");
            return out;
        }
        out.push_str(&format!("{}\n\n", self.headline()));
        out.push_str("| reason | checks | suites | remedy |\n|---|---:|---:|---|\n");
        for (reason, records) in &self.by_reason {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                reason.tag(),
                records.len(),
                self.suites_by_reason.get(reason).copied().unwrap_or(0),
                reason.remedy()
            ));
        }
        out.push_str("\n<details><summary>Every check that did not run</summary>\n\n");
        for (reason, records) in &self.by_reason {
            for record in records {
                out.push_str(&format!(
                    "- `{}` **{}** — {}: {}\n",
                    reason.tag(),
                    record.suite,
                    record.what,
                    record.detail
                ));
            }
        }
        out.push_str("\n</details>\n");
        out
    }
}

fn is_github_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
}

fn emit_annotation(report: &Report) {
    if report.total == 0 {
        return;
    }
    println!(
        "::warning title=Suites that did not run::{} These were skipped rather than passed.",
        report.headline()
    );
}

fn write_job_summary(report: &Report) {
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = write!(file, "{}", report.markdown());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(reason: SkipReason, suite: &str, what: &str) -> SkipRecord {
        SkipRecord {
            reason,
            suite: suite.to_string(),
            what: what.to_string(),
            detail: "detail".to_string(),
            site: format!("tests/{suite}.rs:1"),
        }
    }

    #[test]
    fn an_empty_ledger_says_so_rather_than_saying_nothing() {
        let report = Report::of(&[]);
        assert_eq!(report.total, 0);
        assert!(report.headline().contains("nothing skipped"));
        assert!(report.markdown().contains("Nothing skipped"));
    }

    #[test]
    fn suites_are_counted_once_however_many_checks_they_skipped() {
        let records = vec![
            record(SkipReason::Corpus, "ydd_roundtrip", "read"),
            record(SkipReason::Corpus, "ydd_roundtrip", "write"),
            record(SkipReason::Corpus, "ydr_roundtrip", "read"),
        ];
        let report = Report::of(&records);
        assert_eq!(report.total, 3);
        assert_eq!(report.suites, 2);
        assert_eq!(
            report.headline(),
            "skip ledger: 3 check(s) across 2 suite(s) did not run."
        );
    }

    #[test]
    fn a_build_skip_is_counted_but_does_not_answer_to_require() {
        let records = vec![
            record(
                SkipReason::Build,
                "pack_bound_reads_the_corpus",
                "differential",
            ),
            record(SkipReason::Corpus, "ydd_roundtrip", "read"),
        ];
        let report = Report::of(&records);
        assert_eq!(report.total, 2);
        assert_eq!(report.staged_content_total, 1);
        assert_eq!(report.staged_content_suites, 1);
    }

    #[test]
    fn a_nextest_listing_becomes_one_record_per_test() {
        // Both binary-id shapes nextest emits: a lib suite is the package
        // name alone, an integration suite is `package::binary`.
        let records = tier_records(
            "or-anim binding::tests::a_clip_binds\n\
             or-io::awc_roundtrip every_clip_round_trips\n\
             or-io::awc_roundtrip every_clip_keeps_its_rate\n\
             \n",
        );
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|r| r.reason == SkipReason::Tier));
        assert_eq!(records[0].suite, "or-anim");
        assert_eq!(records[1].suite, "or-io::awc_roundtrip");
        assert_eq!(records[1].what, "every_clip_round_trips");

        let report = Report::of(&records);
        assert_eq!(report.tier_total, 3);
        assert_eq!(report.tier_suites, 2);
    }

    #[test]
    fn a_tier_report_says_what_it_did_not_load_rather_than_asking_for_a_corpus() {
        let records = tier_records("or-io::awc_roundtrip every_clip_round_trips\n");
        let rendered = Report::of(&records).render("/tmp/ledger", true);
        assert!(rendered.contains("did not load 1 check(s) across 1 suite(s)"));
        assert!(rendered.contains("just test"));
        assert!(
            !rendered.contains("Stage a corpus"),
            "a tier skip is not a staging fault and must not read as one"
        );
    }

    #[test]
    fn the_rendered_report_names_every_suite_it_counted() {
        let records = vec![
            record(SkipReason::Corpus, "ydd_roundtrip", "read"),
            record(SkipReason::Install, "the_install_testimony_holds", "check"),
        ];
        let rendered = Report::of(&records).render("/tmp/ledger", true);
        assert!(rendered.contains("ydd_roundtrip"));
        assert!(rendered.contains("the_install_testimony_holds"));
        assert!(rendered.contains("OPENRAGE_REQUIRE_FIXTURES"));
    }
}
