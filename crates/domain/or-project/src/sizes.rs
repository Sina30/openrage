//! What a build costs, ranked: the bytes of the files a plan writes, by the content that made each.
//!
//! A plan's files come from the contributions of the contents it takes, and a few are the build's own (the manifest). The weights add
//! up to the plan's totals, or to what a build wrote once it is given the sizes its cooked files came out at, so a ranking never
//! shows a figure the build does not have.

use std::collections::BTreeMap;

use crate::build::Cooked;
use crate::plan::{Plan, Totals};

/// What the files of one content, or of one garment or slot, come to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Weight {
    pub name: String,
    pub files: u64,
    pub bytes: u64,
}

/// The files of a plan, weighed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weights {
    /// One row for each content that made a file, the heaviest first.
    pub rows: Vec<Weight>,
    /// The files no content made: the manifest the build writes.
    pub own: Weight,
    /// Everything, which is the plan's totals, or what the build wrote when it was given the cooked sizes.
    pub total: Totals,
}

/// Order rows by bytes, the heaviest first, and by name where they weigh the same.
pub fn ranked(rows: &mut [Weight]) {
    rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
}

/// The files of `plan` weighed by the content that made each; a cooked file counts the bytes it came out at in `cooked`, else the payload the plan counts it as.
pub fn by_content(plan: &Plan, cooked: &[Cooked]) -> Weights {
    let mut rows: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    let mut own = (0u64, 0u64);
    for f in &plan.files {
        let bytes = (cooked.iter())
            .find(|c| c.path == f.path)
            .map_or_else(|| f.bytes(), |c| c.bytes);
        let row = match plan.made_by.get(&f.path) {
            Some(module) => rows.entry(module.as_str()).or_default(),
            None => &mut own,
        };
        row.0 += 1;
        row.1 += bytes;
    }
    let total = Totals {
        files: rows.values().map(|r| r.0).sum::<u64>() + own.0,
        bytes: rows.values().map(|r| r.1).sum::<u64>() + own.1,
    };
    let mut rows: Vec<Weight> = (rows.into_iter())
        .map(|(name, (files, bytes))| Weight {
            name: name.to_string(),
            files,
            bytes,
        })
        .collect();
    ranked(&mut rows);
    Weights {
        rows,
        own: Weight {
            name: "the build's own files".to_string(),
            files: own.0,
            bytes: own.1,
        },
        total,
    }
}
