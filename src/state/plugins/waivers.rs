//! A finding of a validator plugin a project lets stand, and why: kept in the project file, so it travels with the project.
//!
//! ⚠ By the code the plugin states (`plugin.<id>.<code>`), for the whole project: a plugin states no garment or file a
//! waiver could be narrower than. A finding the plugin states again after the waiver is gone stands again.

use serde_json::{json, Value};

use crate::state::AppState;

/// The key of the project file that holds the waivers.
pub const KEY: &str = "plugin_waivers";

/// A finding of a validator the project lets stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginWaiver {
    pub code: String,
    pub reason: String,
}

/// The code a finding of a plugin states at the start of its text (`plugin.<id>.<code>: message`), none for any other text.
pub fn code_of(text: &str) -> Option<String> {
    let (code, _) = text.split_once(": ")?;
    let rest = code.strip_prefix("plugin.")?;
    (rest.contains('.') && !code.contains(' ')).then(|| code.to_string())
}

impl AppState {
    /// The waivers the open project holds, in the order they were made.
    pub fn plugin_waivers(&self) -> Vec<PluginWaiver> {
        (self.project_carried.top.get(KEY).and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some(PluginWaiver {
                    code: entry.get("code")?.as_str()?.to_string(),
                    reason: entry.get("reason")?.as_str()?.to_string(),
                })
            })
            .collect()
    }

    fn keep_plugin_waivers(&mut self, waivers: &[PluginWaiver]) {
        let entries: Vec<Value> = waivers
            .iter()
            .map(|w| json!({ "code": w.code, "reason": w.reason }))
            .collect();
        if entries.is_empty() {
            self.project_carried.top.remove(KEY);
        } else {
            self.project_carried
                .top
                .insert(KEY.to_string(), Value::Array(entries));
        }
    }

    /// Let the finding `code` of a validator stand for `reason`, as one step of History that Undo takes back.
    pub fn waive_plugin_finding(&mut self, code: &str, reason: &str) -> Result<(), String> {
        if let Some(why) = self.read_only_reason() {
            return Err(why);
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("a waiver needs a reason".into());
        }
        if code_of(&format!("{code}: ")).is_none() {
            return Err(format!("`{code}` is not the code of a plugin's finding"));
        }
        let mut waivers = self.plugin_waivers();
        match waivers.iter_mut().find(|w| w.code == code) {
            Some(held) => held.reason = reason.to_string(),
            None => waivers.push(PluginWaiver {
                code: code.to_string(),
                reason: reason.to_string(),
            }),
        }
        self.keep_plugin_waivers(&waivers);
        self.workbench.want_scan();
        self.record_step(format!("Waived {code}: {reason}"));
        Ok(())
    }

    /// Take the waiver of `code` away, so the finding stands again.
    pub fn remove_plugin_waiver(&mut self, code: &str) {
        if self.refused_as_read_only() {
            return;
        }
        let mut waivers = self.plugin_waivers();
        let before = waivers.len();
        waivers.retain(|w| w.code != code);
        if waivers.len() == before {
            return;
        }
        self.keep_plugin_waivers(&waivers);
        self.workbench.want_scan();
        self.record_step(format!("Removed the waiver of {code}"));
    }
}

#[cfg(test)]
mod tests {
    use super::code_of;

    #[test]
    fn a_finding_of_a_plugin_states_its_code_before_its_message_and_nothing_else_does() {
        assert_eq!(
            code_of("plugin.catalogue.space-in-path: a b.lua has a space").as_deref(),
            Some("plugin.catalogue.space-in-path")
        );
        for text in [
            "plugin catalogue (Catalogue) could not run over this plan: x",
            "plugin.catalogue: no code",
            "a file is missing: plugin.x.y",
            "plugin.x.y",
        ] {
            assert_eq!(code_of(text), None, "{text}");
        }
    }
}
