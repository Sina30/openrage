//! The list of rules as a person or a script is given it: every row of [`RULES`], with its severity, its limit and whether a reason sets a finding of it aside.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Rule, RULES};
use crate::settings::Level;

/// What the list says of a document that is one.
const FORMAT: &str = "openrage-wardrobe-rules";

/// The limit of a rule that has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limit {
    pub default: u64,
    pub unit: String,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exported {
    pub code: String,
    pub severity: Level,
    pub waivable: bool,
    pub threshold: Option<Limit>,
    pub summary: String,
}

impl From<&Rule> for Exported {
    fn from(rule: &Rule) -> Self {
        Self {
            code: rule.code.to_string(),
            severity: rule.severity.into(),
            waivable: rule.waivable,
            threshold: rule.threshold.map(|t| Limit {
                default: t.default,
                unit: t.unit.to_string(),
            }),
            summary: rule.summary.to_string(),
        }
    }
}

/// Every rule of [`RULES`], in the order a report lists them.
pub fn export() -> Value {
    json!({
        "format": FORMAT,
        "rules": RULES.iter().map(Exported::from).collect::<Vec<_>>(),
    })
}

/// The rows of a list that [`export`] wrote, or what is wrong with the document.
pub fn read_export(document: &Value) -> Result<Vec<Exported>, String> {
    match document["format"].as_str() {
        Some(FORMAT) => {}
        other => {
            return Err(format!(
                "it is not a list of rules: its format is {other:?}"
            ))
        }
    }
    serde_json::from_value(document["rules"].clone())
        .map_err(|e| format!("a row of the list does not read: {e}"))
}

/// The list as lines a person reads: the code, how serious it is, whether a reason sets it aside, its limit, and what it says.
pub fn export_text() -> String {
    let mut text = String::new();
    for row in RULES.iter().map(Exported::from) {
        let limit = row
            .threshold
            .map_or_else(String::new, |t| format!(" limit {} {}", t.default, t.unit));
        let level = serde_json::to_value(row.severity)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let waiver = if row.waivable {
            "can be waived"
        } else {
            "cannot be waived"
        };
        text.push_str(&format!(
            "{} {level}, {waiver}{limit}: {}\n",
            row.code, row.summary
        ));
    }
    text
}
