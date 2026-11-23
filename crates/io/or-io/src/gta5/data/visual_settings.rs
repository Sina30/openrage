//! `visualsettings.dat`: the named tuning values of the game's look, a line of a name and a number each.

use std::collections::BTreeMap;

/// The platform whose value for a name wins over the plain one.
const PLATFORM: &str = "x64";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VisualSettings {
    values: BTreeMap<String, String>,
}

impl VisualSettings {
    /// Read the lines of `text`: `#` starts a comment, a name ending `.x64` overrides the plain name, and the other platforms' names are left out.
    pub fn parse(text: &str) -> Self {
        let mut plain = BTreeMap::new();
        let mut platform = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once(char::is_whitespace) else {
                continue;
            };
            let value = value.trim().to_string();
            match name.rsplit_once('.') {
                Some((base, suffix)) if suffix == PLATFORM => {
                    platform.insert(base.to_string(), value);
                }
                Some((_, "xb1" | "ps4" | "xbox360" | "ps3" | "orbis" | "durango")) => {}
                _ => {
                    plain.insert(name.to_string(), value);
                }
            }
        }
        plain.extend(platform);
        Self { values: plain }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// The number a name holds, or `default` where there is none.
    pub fn get_f32(&self, name: &str, default: f32) -> f32 {
        self.get(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_read_by_name_and_a_comment_or_a_blank_line_is_not_one() {
        let settings = VisualSettings::parse(
            "# Visual Configuration Table\n\nmisc.MoonDimMult\t\t\t0.3\nrain.NumberParticles 16384\n#skipped 1\n",
        );
        assert_eq!(settings.len(), 2);
        assert_eq!(settings.get_f32("misc.MoonDimMult", 1.0), 0.3);
        assert_eq!(settings.get_f32("rain.NumberParticles", 0.0), 16384.0);
        assert_eq!(settings.get_f32("skipped", 7.0), 7.0);
    }

    #[test]
    fn the_x64_value_wins_wherever_it_comes_and_other_platforms_are_left_out() {
        let settings = VisualSettings::parse("a.x64 2\na 1\nb 1\nb.xb1 9\nb.ps4 8\nc.x64 5\n");
        assert_eq!(settings.get_f32("a", 0.0), 2.0);
        assert_eq!(settings.get_f32("b", 0.0), 1.0);
        assert_eq!(settings.get_f32("c", 0.0), 5.0);
        assert_eq!(settings.len(), 3);
    }
}
