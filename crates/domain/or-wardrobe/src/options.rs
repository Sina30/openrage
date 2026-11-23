//! What a build may change about the files a clothing pack writes.
//!
//! The options are keys of the `clothing` object on the build's own entry in the project file, and
//! only the ones that differ from the default are written, so each build is its own override and a
//! file written by this version opens in an older one, which carries the object it does not read.

use or_project::Output;
use serde_json::{Map, Value};

/// The key of the build's entry that holds the options.
pub const KEY: &str = "clothing";

const SUBFOLDERS: &str = "subfolders";
const CREATURE: &str = "creature_metadata";
const ALTERNATES: &str = "alternates";
const PREFIX: &str = "collection_prefix";
const DUMP: &str = "clothes_dump";
const CREDITS: &str = "credits";
const TAGS: &str = "include_tags";

/// The most characters a collection prefix has, since the game hashes a name it keeps whole.
pub const PREFIX_LENGTH: usize = 32;

/// What a build writes for server scripts beside the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DumpKind {
    /// Nothing.
    #[default]
    Off,
    /// `data/clothes_dump.json`.
    Json,
    /// That file and `data/clothes_dump.lua`, the same data as a Lua table.
    Both,
}

impl DumpKind {
    pub const ALL: [DumpKind; 3] = [DumpKind::Off, DumpKind::Json, DumpKind::Both];

    /// The word the project file stores.
    pub fn word(self) -> &'static str {
        match self {
            DumpKind::Off => "off",
            DumpKind::Json => "json",
            DumpKind::Both => "both",
        }
    }

    /// What the screen calls it.
    pub fn label(self) -> &'static str {
        match self {
            DumpKind::Off => "No clothes dump",
            DumpKind::Json => "A clothes dump as JSON",
            DumpKind::Both => "A clothes dump as JSON and as a Lua table",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.word() == word)
    }
}

/// The options of one build; a build that states none gets the default, which is what a build did before there were any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// A folder for each slot inside the folder of a ped's files; off, the files of a ped sit in one. A pack of a singleplayer DLC is always flat, as the game's own are.
    pub subfolders: bool,
    /// The creature metadata and the line of each shop file that names it; off, the expressions a garment states are not written.
    pub creature_metadata: bool,
    /// The alternate models of a drawable and the count the variation metadata states for them.
    pub alternates: bool,
    /// Text put in front of every collection name the build writes, none for none.
    pub prefix: String,
    pub dump: DumpKind,
    /// A credits file at the root of the resource, listing who made each source the pack uses and on what terms ([`crate::credits`]).
    pub credits: bool,
    /// Only the garments filed under one of these tags ship; the others keep their place as a drawable with no model, so the numbers of the rest do not move. None for all.
    pub include_tags: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            subfolders: true,
            creature_metadata: true,
            alternates: true,
            prefix: String::new(),
            dump: DumpKind::Off,
            credits: false,
            include_tags: Vec::new(),
        }
    }
}

impl Options {
    /// Whether the build states nothing, so a screen that keeps the options of the build it shows need not copy them.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The options `output` states, and a sentence for each value that is not one of the ones a key takes (the default is used for it).
    pub fn of(output: &Output) -> (Self, Vec<String>) {
        let mut options = Self::default();
        let mut problems = Vec::new();
        let Some(stated) = output.extra.get(KEY) else {
            return (options, problems);
        };
        let Value::Object(stated) = stated else {
            problems.push(format!(
                "build `{}` has `{KEY}` {stated}, which is not an object of options",
                output.id
            ));
            return (options, problems);
        };
        let mut flag = |key: &str, into: &mut bool| match stated.get(key) {
            None => {}
            Some(Value::Bool(on)) => *into = *on,
            Some(v) => problems.push(format!(
                "build `{}` has `{KEY}.{key}` {v}, which is not true or false",
                output.id
            )),
        };
        flag(SUBFOLDERS, &mut options.subfolders);
        flag(CREATURE, &mut options.creature_metadata);
        flag(ALTERNATES, &mut options.alternates);
        flag(CREDITS, &mut options.credits);
        match stated.get(TAGS) {
            None => {}
            Some(Value::Array(words)) => {
                let tags: Option<Vec<String>> = (words.iter())
                    .map(|w| w.as_str().map(|t| t.trim().to_string()))
                    .collect();
                match tags {
                    Some(tags) => options.include_tags = tidy_tags(tags),
                    None => problems.push(format!(
                        "build `{}` has `{KEY}.{TAGS}` {}, which is not a list of words",
                        output.id,
                        Value::Array(words.clone())
                    )),
                }
            }
            Some(v) => problems.push(format!(
                "build `{}` has `{KEY}.{TAGS}` {v}, which is not a list of words",
                output.id
            )),
        }
        match stated.get(PREFIX) {
            None => {}
            Some(Value::String(text)) => match prefix_problem(text) {
                None => options.prefix.clone_from(text),
                Some(why) => problems.push(format!(
                    "build `{}` has `{KEY}.{PREFIX}` {text:?}, which {why}",
                    output.id
                )),
            },
            Some(v) => problems.push(format!(
                "build `{}` has `{KEY}.{PREFIX}` {v}, which is not text",
                output.id
            )),
        }
        match stated.get(DUMP) {
            None => {}
            Some(Value::String(word)) => match DumpKind::from_word(word) {
                Some(dump) => options.dump = dump,
                None => problems.push(format!(
                    "build `{}` has `{KEY}.{DUMP}` {word:?}, which is none of off, json and both",
                    output.id
                )),
            },
            Some(v) => problems.push(format!(
                "build `{}` has `{KEY}.{DUMP}` {v}, which is not text",
                output.id
            )),
        }
        (options, problems)
    }

    /// Set the option the command line calls `key` from `value`: `on` or `off` for a switch, text or `none` for the prefix, `off`, `json` or `both` for the dump.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let switch = |value: &str| match value {
            "on" => Ok(true),
            "off" => Ok(false),
            other => Err(format!("{key} is on or off, not {other}")),
        };
        match key {
            "subfolders" => self.subfolders = switch(value)?,
            "creature-metadata" => self.creature_metadata = switch(value)?,
            "alternates" => self.alternates = switch(value)?,
            "credits" => self.credits = switch(value)?,
            "include-tags" => {
                let words = if value == "none" { "" } else { value };
                self.include_tags = tidy_tags(words.split(',').map(String::from).collect());
            }
            "collection-prefix" => {
                let text = if value == "none" { "" } else { value };
                if let Some(why) = prefix_problem(text) {
                    return Err(format!("the prefix {text:?} {why}"));
                }
                self.prefix = text.to_string();
            }
            "clothes-dump" => {
                self.dump = DumpKind::from_word(value)
                    .ok_or_else(|| format!("clothes-dump is off, json or both, not {value}"))?;
            }
            other => {
                return Err(format!(
                    "{other} is no clothing option: subfolders, creature-metadata, alternates, credits, include-tags, collection-prefix or clothes-dump"
                ))
            }
        }
        Ok(())
    }

    /// State these options on `output`: a key only where it differs from the default, the others of the object kept, and the object gone when nothing is left.
    pub fn store(&self, output: &mut Output) {
        let mut object = match output.extra.remove(KEY) {
            Some(Value::Object(object)) => object,
            _ => Map::new(),
        };
        let mut put = |key: &str, value: Option<Value>| match value {
            Some(value) => {
                object.insert(key.to_string(), value);
            }
            None => {
                object.remove(key);
            }
        };
        let default = Self::default();
        put(
            SUBFOLDERS,
            (self.subfolders != default.subfolders).then_some(Value::Bool(self.subfolders)),
        );
        put(
            CREATURE,
            (self.creature_metadata != default.creature_metadata)
                .then_some(Value::Bool(self.creature_metadata)),
        );
        put(
            ALTERNATES,
            (self.alternates != default.alternates).then_some(Value::Bool(self.alternates)),
        );
        put(
            CREDITS,
            (self.credits != default.credits).then_some(Value::Bool(self.credits)),
        );
        put(
            TAGS,
            (!self.include_tags.is_empty()).then(|| {
                Value::Array(
                    (self.include_tags.iter())
                        .map(|t| Value::String(t.clone()))
                        .collect(),
                )
            }),
        );
        put(
            PREFIX,
            (!self.prefix.is_empty()).then(|| Value::String(self.prefix.clone())),
        );
        put(
            DUMP,
            (self.dump != default.dump).then(|| Value::String(self.dump.word().to_string())),
        );
        if !object.is_empty() {
            output.extra.insert(KEY.to_string(), Value::Object(object));
        }
    }
}

/// The words of a tag rule without the empty ones and without a word twice, whatever the case.
fn tidy_tags(words: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for word in words.into_iter().map(|w| w.trim().to_string()) {
        if !word.is_empty() && !kept.iter().any(|k| k.eq_ignore_ascii_case(&word)) {
            kept.push(word);
        }
    }
    kept
}

/// Why `text` cannot begin a collection name, none when it can.
pub fn prefix_problem(text: &str) -> Option<&'static str> {
    if text.chars().count() > PREFIX_LENGTH {
        return Some("is longer than a prefix may be");
    }
    if !text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some("holds a character a name the game hashes does not: letters, digits and underscores only");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_project::OutputKind;
    use serde_json::json;

    fn output() -> Output {
        Output::of(OutputKind::FivemResource, "fivem", "pack", &["wardrobe"])
    }

    #[test]
    fn a_build_that_states_none_gets_the_default_and_writes_nothing_back() {
        let mut out = output();
        let (options, problems) = Options::of(&out);
        assert!(options.is_default() && problems.is_empty());
        options.store(&mut out);
        assert!(!out.extra.contains_key(KEY));
    }

    #[test]
    fn what_differs_from_the_default_is_stored_and_read_back_and_nothing_else() {
        let mut out = output();
        let options = Options {
            subfolders: false,
            prefix: "srv_".into(),
            dump: DumpKind::Both,
            ..Options::default()
        };
        options.store(&mut out);
        assert_eq!(
            out.extra[KEY],
            json!({ "subfolders": false, "collection_prefix": "srv_", "clothes_dump": "both" })
        );
        assert_eq!(Options::of(&out), (options, Vec::new()));
        Options::default().store(&mut out);
        assert!(
            !out.extra.contains_key(KEY),
            "back to the default is no key"
        );
    }

    #[test]
    fn a_key_this_build_does_not_read_is_kept_and_a_value_it_cannot_use_is_said() {
        let mut out = output();
        out.extra.insert(
            KEY.into(),
            json!({ "future": 1, "alternates": "no", "clothes_dump": "xml", "collection_prefix": "a b" }),
        );
        let (options, problems) = Options::of(&out);
        assert!(options.is_default());
        assert_eq!(problems.len(), 3, "{problems:?}");
        Options {
            creature_metadata: false,
            ..Options::default()
        }
        .store(&mut out);
        assert_eq!(out.extra[KEY]["future"], json!(1));
        assert_eq!(out.extra[KEY]["creature_metadata"], json!(false));
    }
}
