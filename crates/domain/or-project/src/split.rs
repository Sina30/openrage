//! The split of one output into several builds, at the boundaries between contents.
//!
//! An output that declares `split` and is over a ceiling is divided: its contents are taken in the
//! order of `take`, and a new resource starts when the next content would take the current one over
//! a ceiling. A content never goes back into an earlier resource, so each resource holds one
//! unbroken run of `take`, and a content is never cut. Each resource is planned as an output of
//! its own, with the claims of the whole project still checked, and is named `<name>_1`,
//! `<name>_2` and so on.

use std::path::Path;

use crate::container::Container;
use crate::contribution::Contribution;
use crate::plan::{assemble, files_contributions, NoSuchOutput, Plan, Source};

/// The plans of `output`: one, or one for each resource it divides into.
pub fn plans(
    container: &Container,
    output: &str,
    folder: &Path,
    sources: &[Source],
) -> Result<Vec<Plan>, NoSuchOutput> {
    let contributions = files_contributions(container, output, folder, sources)?;
    plans_with(container, output, folder, &contributions)
}

/// [`plans`] from what the modules contribute.
pub fn plans_with(
    container: &Container,
    output: &str,
    folder: &Path,
    contributions: &[Contribution],
) -> Result<Vec<Plan>, NoSuchOutput> {
    let declared = container
        .output(output)
        .ok_or_else(|| NoSuchOutput(output.to_string()))?;
    let planned = |take: &[String], resource: &str| {
        assemble(container, declared, take, resource, folder, contributions)
    };
    let whole = planned(&declared.take, &declared.name);
    if !declared.split || !whole.over_a_limit() {
        return Ok(vec![whole]);
    }

    let mut parts: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for id in &declared.take {
        if current.contains(id) || parts.iter().any(|part| part.contains(id)) {
            continue;
        }
        let mut with = current.clone();
        with.push(id.clone());
        if !current.is_empty() && planned(&with, &declared.name).over_a_limit() {
            parts.push(std::mem::take(&mut current));
        }
        current.push(id.clone());
    }
    parts.push(current);

    let count = parts.len();
    Ok(parts
        .iter()
        .enumerate()
        .map(|(n, take)| {
            let resource = match count {
                1 => declared.name.clone(),
                _ => format!("{}_{}", declared.name, n + 1),
            };
            let mut part = planned(take, &resource);
            if let ([only], true) = (take.as_slice(), part.over_a_limit()) {
                part.problems.push(format!(
                    "content `{only}` is over a ceiling by itself, and a split divides between contents, not inside one"
                ));
            }
            part
        })
        .collect())
}
