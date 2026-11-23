//! `.sps` — the shader preset a drawable's material actually names.

use std::fmt;

/// The preset every drawable shader is sorted by.
pub const DRAW_BUCKET_PRESET: &str = "__rage_drawbucket";

/// A value a preset pins, in the two spellings `.sps` uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpsValue {
    Int(i64),
    Float(f32),
}

impl SpsValue {
    pub fn as_f32(self) -> f32 {
        match self {
            Self::Int(i) => i as f32,
            Self::Float(f) => f,
        }
    }

    pub fn as_u32(self) -> Option<u32> {
        match self {
            Self::Int(i) => u32::try_from(i).ok(),
            Self::Float(f) if f.fract() == 0.0 && f >= 0.0 => Some(f as u32),
            Self::Float(_) => None,
        }
    }
}

impl fmt::Display for SpsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpsOverride {
    pub name: String,
    pub values: Vec<SpsValue>,
}

/// A shader preset: which effect it binds, and what it pins before binding.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpsPreset {
    /// The preset's own name — this, not `effect`, is what a `.ydr` stores.
    pub name: String,
    /// The `.fxc` this preset binds.
    pub effect: String,
    pub overrides: Vec<SpsOverride>,
}

impl SpsPreset {
    /// ⚠ **`name` cannot be recovered from the text** — a preset states the
    /// effect it binds and never its own name, and 124 of the 391 presets in a
    /// retail Legacy install name an effect spelled differently from the file
    /// they are in. Pass the file stem.
    pub fn parse(name: &str, text: &str) -> Self {
        let mut preset = SpsPreset {
            name: name.to_ascii_lowercase(),
            effect: String::new(),
            overrides: Vec::new(),
        };

        let mut pending: Option<String> = None;
        let mut values: Vec<SpsValue> = Vec::new();

        for line in text.lines() {
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }

            if let Some(rest) = line.strip_prefix('}') {
                if let Some(name) = pending.take() {
                    preset.overrides.push(SpsOverride {
                        name,
                        values: std::mem::take(&mut values),
                    });
                }
                if !rest.trim().is_empty() {
                    continue;
                }
                continue;
            }

            if pending.is_some() {
                if let Some(value) = parse_value(line) {
                    values.push(value);
                }
                continue;
            }

            if let Some(open) = line.strip_suffix('{') {
                let key = open.trim();
                if !key.is_empty() {
                    pending = Some(key.to_string());
                }
                continue;
            }

            let mut words = line.split_whitespace();
            if let (Some("shader"), Some(effect)) = (words.next(), words.next()) {
                preset.effect = effect.trim_end_matches(".fxc").to_ascii_lowercase();
            }
        }

        if let Some(name) = pending {
            preset.overrides.push(SpsOverride { name, values });
        }

        preset
    }

    pub fn get(&self, name: &str) -> Option<&SpsOverride> {
        self.overrides.iter().find(|o| o.name == name)
    }

    /// The bucket this preset pins, which is the whole reason a `.ydr` sorts
    /// `cutout` differently from `default` while both bind the same effect.
    pub fn draw_bucket(&self) -> Option<u32> {
        self.get(DRAW_BUCKET_PRESET)?
            .values
            .first()
            .and_then(|v| v.as_u32())
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(at) => &line[..at],
        None => line,
    }
}

fn parse_value(line: &str) -> Option<SpsValue> {
    let mut words = line.split_whitespace();
    let kind = words.next()?;
    let raw = words.next()?;
    match kind {
        "int" => raw.parse::<i64>().ok().map(SpsValue::Int),
        "float" => raw.parse::<f32>().ok().map(SpsValue::Float),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_preset_names_only_its_effect() {
        let p = SpsPreset::parse("normal_spec_reflect", "shader normal_spec_reflect \r\n");
        assert_eq!(p.name, "normal_spec_reflect");
        assert_eq!(p.effect, "normal_spec_reflect");
        assert!(p.overrides.is_empty());
        assert_eq!(p.draw_bucket(), None);
    }

    #[test]
    fn overrides_carry_their_type_and_value() {
        let text = "shader vehicle_vehglass\r\n__rage_drawbucket {\r\n\tint 1\r\n}\t\r\nSpecularColor {\r\n\tfloat 0.150000\r\n}\r\n";
        let p = SpsPreset::parse("vehicle_vehglass_inner", text);
        assert_eq!(p.name, "vehicle_vehglass_inner");
        assert_eq!(p.effect, "vehicle_vehglass");
        assert_eq!(p.draw_bucket(), Some(1));
        assert_eq!(
            p.get("SpecularColor").map(|o| o.values.as_slice()),
            Some([SpsValue::Float(0.15)].as_slice())
        );
    }

    /// ⚠ The preset's name and the effect's are different namespaces, and a
    /// third of them disagree. Keying a database by the effect loses the
    /// bucket that is the only thing the preset added.
    #[test]
    fn a_preset_may_name_an_effect_spelled_nothing_like_it() {
        let p = SpsPreset::parse(
            "cutout",
            "shader default\n__rage_drawbucket {\n\tint 3\n}\n",
        );
        assert_eq!(p.name, "cutout");
        assert_eq!(p.effect, "default");
        assert_eq!(p.draw_bucket(), Some(3));
    }

    #[test]
    fn a_name_is_lowercased_because_a_material_hashes_it_that_way() {
        let p = SpsPreset::parse("Vehicle_Paint1", "shader Vehicle_Paint1\n");
        assert_eq!(p.name, "vehicle_paint1");
        assert_eq!(p.effect, "vehicle_paint1");
    }

    #[test]
    fn an_empty_file_yields_a_preset_with_no_effect() {
        let p = SpsPreset::parse("stub", "");
        assert!(p.effect.is_empty());
        assert!(p.overrides.is_empty());
    }
}
