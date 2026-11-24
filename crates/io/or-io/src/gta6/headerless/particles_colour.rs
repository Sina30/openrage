//! `ptxu_Colour`'s count of the colours it uses, the word GTA V's `rgbaMaxEnable` is read from: the
//! maximum is enabled where two or more are in use, which is inferred.

use super::*;

/// The count's offset in the behaviour.
const COLOURS_USED: usize = 0x19C;
/// The minimum, the maximum and three colours GTA V does not have.
const COLOURS: u32 = 5;
/// The least count that enables the maximum.
const MAX_ENABLED: u32 = 2;

/// `ptxu_Colour`'s count of the behaviour at `at`, which runs to `next`; none where it is shorter.
pub(super) fn colour(v: &Bytes, at: usize, next: usize) -> Result<BehaviourFields> {
    if next < at + COLOURS_USED + 4 {
        return Ok(BehaviourFields::None);
    }
    let colours_used = v.u32("ptxu_Colour", at + COLOURS_USED)?;
    need(
        colours_used <= COLOURS,
        "a ptxu_Colour's count of colours in use is above 5",
    )?;
    Ok(BehaviourFields::Colour { colours_used })
}

/// The count stored for `enable`: `read` where it already reads so, else 2 to enable and 1 not to.
fn written(enable: bool, read: u32) -> u32 {
    match (enable, read >= MAX_ENABLED) {
        (on, was) if on == was => read,
        (true, _) => MAX_ENABLED,
        (false, _) => 1,
    }
}

impl BehaviourFields {
    /// Whether a `ptxu_Colour` enables its maximum: two or more colours in use, which is inferred.
    pub fn colour_max_enabled(&self) -> bool {
        matches!(self, Self::Colour { colours_used } if *colours_used >= MAX_ENABLED)
    }

    /// These fields with a `ptxu_Colour`'s maximum enabled or not; any other fields as they are.
    pub fn with_colour_max_enabled(self, enable: bool) -> Self {
        match self {
            Self::Colour { colours_used } => Self::Colour {
                colours_used: written(enable, colours_used),
            },
            other => other,
        }
    }
}

/// The counts `edited` changes from `stored`, each at its section offset.
pub(super) fn patches(stored: &[Behaviour], edited: &[Behaviour]) -> Vec<(usize, Vec<u8>)> {
    (stored.iter().zip(edited))
        .filter_map(|(s, e)| match (s.fields, e.fields) {
            (
                BehaviourFields::Colour { colours_used: was },
                BehaviourFields::Colour { colours_used },
            ) if was != colours_used => Some((
                s.at as usize + COLOURS_USED,
                colours_used.to_le_bytes().to_vec(),
            )),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_enable_keeps_its_count_and_an_edit_reads_back_as_written() {
        for read in 0..=COLOURS {
            let was = read >= MAX_ENABLED;
            assert_eq!(written(was, read), read, "{read} kept");
            assert_eq!(written(!was, read) >= MAX_ENABLED, !was, "{read} edited");
        }
        assert_eq!(written(true, 0), 2);
        assert_eq!(written(true, 1), 2);
        assert_eq!(written(false, 5), 1);
        let off = BehaviourFields::Colour { colours_used: 4 };
        assert!(off.colour_max_enabled());
        let edited = off.with_colour_max_enabled(false);
        assert_eq!(edited, BehaviourFields::Colour { colours_used: 1 });
        assert!(!edited.colour_max_enabled());
        assert_eq!(
            BehaviourFields::Age.with_colour_max_enabled(true),
            BehaviourFields::Age
        );
        assert!(!BehaviourFields::None.colour_max_enabled());
    }

    #[test]
    fn only_a_count_the_edit_changes_is_written_at_its_offset() {
        let held = |at: u64, colours_used: u32| Behaviour {
            at,
            class: Some("ptxu_Colour"),
            class_bit: None,
            size: 0x1F0,
            kfps: Vec::new(),
            fields: BehaviourFields::Colour { colours_used },
        };
        let stored = [held(0x100, 3), held(0x400, 1)];
        let mut edited = stored.clone();
        for b in &mut edited {
            b.fields = b.fields.with_colour_max_enabled(true);
        }
        let at = 0x400 + COLOURS_USED;
        assert_eq!(
            patches(&stored, &edited),
            vec![(at, MAX_ENABLED.to_le_bytes().to_vec())]
        );
        assert!(patches(&stored, &stored).is_empty());
    }
}
