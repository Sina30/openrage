//! What the Problems panel shows of the findings: the ones a filter lets through, in the order asked and under the headings asked, and the same as text a person keeps.
//!
//! ⚠ **Nothing here draws.** The panel hands the findings and a [`View`] and gets the rows back, so what a filter, a sort and a
//! grouping do is held by tests and the panel only paints.

use or_project::Container;

use super::{build_name, content_name, Counts, Finding, Place, Severity};

/// Which severities are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Show {
    #[default]
    All,
    Errors,
    Warnings,
}

impl Show {
    pub const ALL: [Show; 3] = [Show::All, Show::Errors, Show::Warnings];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => tr!("All"),
            Self::Errors => tr!("Errors"),
            Self::Warnings => tr!("Warnings"),
        }
    }

    fn admits(self, severity: Severity) -> bool {
        match self {
            Self::All => true,
            Self::Errors => severity == Severity::Error,
            Self::Warnings => severity == Severity::Warning,
        }
    }
}

/// What the rows are gathered under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Group {
    #[default]
    None,
    Rule,
    Content,
    Severity,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::None, Group::Rule, Group::Content, Group::Severity];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => tr!("No grouping"),
            Self::Rule => tr!("By rule"),
            Self::Content => tr!("By content"),
            Self::Severity => tr!("By severity"),
        }
    }
}

/// What the rows are ordered by, inside a heading where there are headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Errors first, then what each is about.
    #[default]
    Severity,
    Rule,
    About,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Severity, Sort::Rule, Sort::About];

    pub fn label(self) -> &'static str {
        match self {
            Self::Severity => tr!("Most serious first"),
            Self::Rule => tr!("By rule"),
            Self::About => tr!("By name"),
        }
    }
}

/// What the panel was asked to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    pub show: Show,
    /// Words a row must hold, in what it is about, its sentence, its content or its rule; case does not matter.
    pub text: String,
    pub group: Group,
    pub sort: Sort,
}

impl View {
    /// Whether anything narrows the list.
    pub fn filters(&self) -> bool {
        self.show != Show::All || !self.text.trim().is_empty()
    }
}

/// One row of the list: a heading over the rows under it, or a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row<'a> {
    Heading { label: String, counts: Counts },
    Finding(&'a Finding),
}

/// What a heading says: its group and what is under it, by errors and warnings, as every other count of the screen is said.
pub fn heading_words(label: &str, counts: Counts) -> String {
    // ⚠ The heading of the errors is its own count's name, so it does not say `Errors · 3 errors`.
    let (errors, warnings) = (tr!("Errors"), tr!("Warnings"));
    if (label == errors && counts.warnings == 0) || (label == warnings && counts.errors == 0) {
        counts.said()
    } else {
        format!("{label} · {}", counts.said())
    }
}

/// What a finding is about, in words: a build by the name its resource has, which is the name Content and Builds shows.
pub fn about(finding: &Finding, container: &Container) -> String {
    match &finding.place {
        Place::Build(id) => tr!("Build {container}", container = build_name(container, id)),
        // ⚠ A finding of the pack as a whole, or of a link, is about the content, by the name the Content panel gives it.
        Place::Garment {
            content, id: None, ..
        }
        | Place::Content(content) => content_name(container, content),
        _ => finding.about(),
    }
}

/// The content a finding is in, as a person names it: the name the Content panel gives it, and never the id the project file keeps it by.
pub fn content_of(finding: &Finding, container: &Container) -> String {
    match &finding.place {
        Place::Project => tr!("Project").to_string(),
        Place::Build(id) => tr!("Build {container}", container = build_name(container, id)),
        Place::Content(content) | Place::Asset { content, .. } | Place::Garment { content, .. } => {
            content_name(container, content)
        }
    }
}

/// What a finding's rule says, or none for a finding of no rule.
fn rule_words(finding: &Finding) -> Option<&'static str> {
    finding
        .code
        .and_then(or_wardrobe::rule_of)
        .map(|r| r.summary)
}

/// `words` with its first letter in capitals, as a heading begins.
fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The findings the view lets through, in the order it asks for.
pub fn kept<'a>(findings: &'a [Finding], view: &View, container: &Container) -> Vec<&'a Finding> {
    let words: Vec<String> = view
        .text
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut kept: Vec<&Finding> = findings
        .iter()
        .filter(|f| view.show.admits(f.severity))
        .filter(|f| {
            let held = format!(
                "{} {} {} {}",
                about(f, container),
                f.text,
                content_of(f, container),
                f.code.unwrap_or_default()
            )
            .to_lowercase();
            words.iter().all(|word| held.contains(word))
        })
        .collect();
    match view.sort {
        Sort::Severity => {
            kept.sort_by_key(|f| (std::cmp::Reverse(f.severity), about(f, container)))
        }
        Sort::Rule => kept.sort_by_key(|f| (f.code.unwrap_or("~"), about(f, container))),
        Sort::About => kept.sort_by_key(|f| {
            (
                about(f, container).to_lowercase(),
                std::cmp::Reverse(f.severity),
            )
        }),
    }
    kept
}

/// The rows of the list: the findings the view lets through, under a heading for each group where it groups.
pub fn rows<'a>(findings: &'a [Finding], view: &View, container: &Container) -> Vec<Row<'a>> {
    let kept = kept(findings, view, container);
    let heading = |f: &Finding| -> Option<String> {
        match view.group {
            Group::None => None,
            Group::Rule => {
                Some(rule_words(f).map_or_else(|| tr!("Other problems").to_string(), capitalised))
            }
            Group::Content => Some(content_of(f, container)),
            Group::Severity => Some(match f.severity {
                Severity::Error => tr!("Errors").to_string(),
                Severity::Warning => tr!("Warnings").to_string(),
            }),
        }
    };
    if view.group == Group::None {
        return kept.into_iter().map(Row::Finding).collect();
    }
    // The groups keep the order their first finding had, which the sort decided.
    let mut groups: Vec<(String, Vec<&Finding>)> = Vec::new();
    for finding in kept {
        let label = heading(finding).unwrap_or_default();
        match groups.iter_mut().find(|(known, _)| *known == label) {
            Some((_, rows)) => rows.push(finding),
            None => groups.push((label, vec![finding])),
        }
    }
    let mut rows = Vec::new();
    for (label, members) in groups {
        rows.push(Row::Heading {
            label,
            counts: Counts::of(members.iter().copied()),
        });
        rows.extend(members.into_iter().map(Row::Finding));
    }
    rows
}

/// How an export is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Csv,
    Json,
}

impl Format {
    /// The format a file called `name` asks for by its extension: JSON, CSV, or lines of text for anything else.
    pub fn of(name: &str) -> Self {
        match name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("json") => Self::Json,
            Some("csv") => Self::Csv,
            _ => Self::Text,
        }
    }
}

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// A field of a CSV row: quoted when it holds a comma, a quote or a line break, with the quotes inside it doubled.
fn csv_field(field: &str) -> String {
    match field.contains([',', '"', '\n', '\r']) {
        true => format!("\"{}\"", field.replace('"', "\"\"")),
        false => field.to_string(),
    }
}

/// The findings as a file: one line each, a CSV with a header, or a JSON array.
pub fn export(findings: &[&Finding], format: Format, container: &Container) -> String {
    match format {
        Format::Text => findings
            .iter()
            .map(|f| {
                let rule = f.code.map_or_else(String::new, |c| format!(" [{c}]"));
                format!(
                    "{}: {} - {}: {}{rule}\n",
                    severity_word(f.severity),
                    content_of(f, container),
                    about(f, container),
                    f.text
                )
            })
            .collect(),
        Format::Csv => {
            let mut text = String::from("severity,rule,content,about,text\n");
            for f in findings {
                let fields = [
                    severity_word(f.severity).to_string(),
                    f.code.unwrap_or_default().to_string(),
                    content_of(f, container),
                    about(f, container),
                    f.text.clone(),
                ];
                let line: Vec<String> = fields.iter().map(|x| csv_field(x)).collect();
                text.push_str(&line.join(","));
                text.push('\n');
            }
            text
        }
        Format::Json => {
            let rows: Vec<serde_json::Value> = findings
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "severity": severity_word(f.severity),
                        "rule": f.code,
                        "content": content_of(f, container),
                        "about": about(f, container),
                        "text": f.text,
                    })
                })
                .collect();
            let mut text = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into());
            text.push('\n');
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(
        severity: Severity,
        code: Option<&'static str>,
        content: &str,
        label: &str,
        text: &str,
    ) -> Finding {
        Finding {
            severity,
            place: Place::Garment {
                content: content.to_string(),
                id: Some(label.to_string()),
                label: label.to_string(),
            },
            text: text.to_string(),
            fixes: Vec::new(),
            code,
        }
    }

    fn sample() -> Vec<Finding> {
        vec![
            finding(
                Severity::Warning,
                Some("texture-size"),
                "tops",
                "jacket",
                "Has 2 textures over 2048 pixels",
            ),
            finding(
                Severity::Error,
                Some("model-missing"),
                "tops",
                "cap",
                "Names a model that is not there",
            ),
            finding(
                Severity::Warning,
                Some("texture-size"),
                "shoes",
                "boot",
                "Has 1 texture over 2048 pixels",
            ),
            finding(
                Severity::Warning,
                None,
                "shoes",
                "boot",
                "Something of no rule",
            ),
            finding(
                Severity::Error,
                Some("model-missing"),
                "shoes",
                "sock",
                "Names a model that is not there",
            ),
        ]
    }

    fn about(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Heading { label, counts } => {
                    format!("# {label} ({})", counts.errors + counts.warnings)
                }
                Row::Finding(f) => f.about(),
            })
            .collect()
    }

    #[test]
    fn a_filter_keeps_the_severity_and_the_words_it_is_asked_for_and_nothing_else() {
        let all = sample();
        let view = |show, text: &str| View {
            show,
            text: text.into(),
            ..View::default()
        };
        assert_eq!(
            kept(&all, &view(Show::All, ""), &Container::default()).len(),
            5
        );
        assert_eq!(
            kept(&all, &view(Show::Errors, ""), &Container::default()).len(),
            2
        );
        assert_eq!(
            kept(&all, &view(Show::Warnings, ""), &Container::default()).len(),
            3
        );
        // Words of any case, in any of what a row holds, and every word must be held.
        assert_eq!(
            kept(
                &all,
                &view(Show::All, "SHOES texture"),
                &Container::default()
            )
            .len(),
            1
        );
        assert_eq!(
            kept(
                &all,
                &view(Show::All, "model-missing"),
                &Container::default()
            )
            .len(),
            2,
            "the rule's code is searched too"
        );
        assert_eq!(
            kept(&all, &view(Show::Errors, "texture"), &Container::default()).len(),
            0
        );
        assert!(!View::default().filters());
        assert!(view(Show::All, " cap ").filters());
    }

    #[test]
    fn the_order_asked_for_is_the_order_the_rows_come_in() {
        let all = sample();
        let order = |sort| {
            about(&rows(
                &all,
                &View {
                    sort,
                    ..View::default()
                },
                &Container::default(),
            ))
        };
        assert_eq!(
            order(Sort::Severity),
            ["cap", "sock", "boot", "boot", "jacket"]
        );
        assert_eq!(
            order(Sort::About),
            ["boot", "boot", "cap", "jacket", "sock"]
        );
        // By rule: the findings of no rule come last.
        assert_eq!(order(Sort::Rule), ["cap", "sock", "boot", "jacket", "boot"]);
    }

    #[test]
    fn a_grouping_puts_a_heading_over_each_group_with_its_count_and_keeps_the_sort_inside_it() {
        let all = sample();
        let grouped = |group| {
            about(&rows(
                &all,
                &View {
                    group,
                    ..View::default()
                },
                &Container::default(),
            ))
        };
        assert_eq!(
            grouped(Group::Severity),
            [
                "# Errors (2)",
                "cap",
                "sock",
                "# Warnings (3)",
                "boot",
                "boot",
                "jacket"
            ]
        );
        // The groups come in the order their first finding has, which the sort decided.
        assert_eq!(
            grouped(Group::Content),
            [
                "# tops (2)",
                "cap",
                "jacket",
                "# shoes (3)",
                "sock",
                "boot",
                "boot"
            ]
        );
        let missing = or_wardrobe::rule_of("model-missing").unwrap().summary;
        let size = or_wardrobe::rule_of("texture-size").unwrap().summary;
        assert_eq!(
            grouped(Group::Rule),
            [
                format!("# {} (2)", capitalised(missing)),
                "cap".into(),
                "sock".into(),
                format!("# {} (2)", capitalised(size)),
                "boot".into(),
                "jacket".into(),
                "# Other problems (1)".into(),
                "boot".into(),
            ]
        );
        // A filter and a grouping together: only what is left is under a heading.
        let narrowed = rows(
            &all,
            &View {
                show: Show::Errors,
                group: Group::Content,
                ..View::default()
            },
            &Container::default(),
        );
        assert_eq!(
            about(&narrowed),
            ["# tops (1)", "cap", "# shoes (1)", "sock"]
        );
    }

    #[test]
    fn a_content_and_a_build_are_named_as_the_content_panel_names_them_and_never_by_their_ids() {
        use or_project::{Module, Output};

        let mut container = Container::default();
        container
            .modules
            .push(Module::clothing("uniforms", "uniforms"));
        container.outputs.push(Output::fivem_resource(
            "fivem",
            "harbor_police",
            &["uniforms"],
        ));
        let garment = finding(Severity::Warning, None, "uniforms", "jacket", "Too big");
        let build = Finding {
            place: Place::Build("fivem".into()),
            ..garment.clone()
        };
        let all = [garment, build];
        let view = View {
            group: Group::Content,
            ..View::default()
        };
        let headings: Vec<String> = rows(&all, &view, &container)
            .into_iter()
            .filter_map(|row| match row {
                Row::Heading { label, .. } => Some(label),
                Row::Finding(_) => None,
            })
            .collect();
        assert_eq!(headings, ["Build harbor_police", "Clothing"]);
        // The words a row can be found by, and the file a person keeps, say the same names.
        let by_name = View {
            text: "harbor_police".into(),
            ..View::default()
        };
        assert_eq!(kept(&all, &by_name, &container).len(), 1);
        let shown: Vec<&Finding> = all.iter().collect();
        let csv = export(&shown, Format::Csv, &container);
        assert!(
            csv.contains("Clothing") && csv.contains("Build harbor_police"),
            "{csv}"
        );
        assert!(
            !csv.contains("uniforms") && !csv.contains("fivem"),
            "an id stands in for a name: {csv}"
        );
        // What a heading says of its group is the same count of errors and warnings that every screen says.
        assert_eq!(
            heading_words(
                "Clothing",
                Counts {
                    errors: 1,
                    warnings: 18
                }
            ),
            "Clothing · 1 error, 18 warnings"
        );
        assert_eq!(
            heading_words(
                "Errors",
                Counts {
                    errors: 3,
                    warnings: 0
                }
            ),
            "3 errors"
        );
    }

    #[test]
    fn the_findings_are_written_out_as_lines_a_csv_and_json_and_a_name_chooses_which() {
        assert_eq!(Format::of("problems.JSON"), Format::Json);
        assert_eq!(Format::of("problems.csv"), Format::Csv);
        assert_eq!(Format::of("problems.txt"), Format::Text);
        assert_eq!(Format::of("problems"), Format::Text);

        let mut all = sample();
        all[0].text = "Has \"2\" textures, over\n2048".into();
        let shown = kept(&all, &View::default(), &Container::default());
        let text = export(&shown, Format::Text, &Container::default());
        assert_eq!(
            text.lines().count(),
            6,
            "one line each, and the finding with a line break in it takes two: {text}"
        );
        assert!(
            text.contains("error: tops - cap: Names a model that is not there [model-missing]"),
            "{text}"
        );
        assert!(
            text.contains("warning: shoes - boot: Something of no rule\n"),
            "no rule, no brackets"
        );

        let csv = export(&shown, Format::Csv, &Container::default());
        assert!(csv.starts_with("severity,rule,content,about,text\n"));
        assert!(
            csv.contains(
                "warning,texture-size,tops,jacket,\"Has \"\"2\"\" textures, over\n2048\"\n"
            ),
            "{csv}"
        );

        let json: serde_json::Value =
            serde_json::from_str(&export(&shown, Format::Json, &Container::default())).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 5);
        assert_eq!(json[0]["rule"], "model-missing");
        assert_eq!(json[0]["severity"], "error");
        assert_eq!(json[4]["text"], "Has \"2\" textures, over\n2048");
        assert!(json[3]["rule"].is_null(), "a finding of no rule has none");
    }
}
