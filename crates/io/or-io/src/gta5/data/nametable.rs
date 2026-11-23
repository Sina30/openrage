//! RAGE `.nametable`: the plaintext names for a sibling `.dat` that stores
//! only their hashes.

use crate::error::{IoError, Result};

/// An entry that is not a name, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub index: usize,
    pub reason: String,
}

/// What a `.nametable` held.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameTable {
    /// The names, in file order.
    ///
    /// ⚠ The order is the file's and is preserved on write, because a `.dat`
    /// addresses its name table by index as well as by hash.
    pub names: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// Read a `.nametable`.
///
/// ⚠ **One bad entry is a bad entry; mostly-bad entries are not a name
/// table.** Retail ships at least one table with a newline inside a name —
/// `HS4_IGUARD4\n_PVG` in `dlchei4_game.dat151.nametable`, 1 of that file's
/// 1,057 entries — and refusing the file over it throws away the other 1,056.
/// Refusing nothing would accept a `.dat` handed to the wrong reader, so the
/// line is drawn at which outcome is in the majority.
pub fn parse(bytes: &[u8]) -> Result<NameTable> {
    let mut table = NameTable::default();
    for (index, field) in bytes.split(|b| *b == 0).enumerate() {
        if field.is_empty() {
            continue;
        }
        match check(field) {
            Ok(name) => table.names.push(name.to_string()),
            Err(reason) => table.skipped.push(Skipped { index, reason }),
        }
    }

    if !table.skipped.is_empty() && table.skipped.len() >= table.names.len() {
        return Err(IoError::InvalidData {
            context: format!(
                "{} of {} entries are not names (first: {}); this is not a name table",
                table.skipped.len(),
                table.skipped.len() + table.names.len(),
                table.skipped[0].reason
            ),
        });
    }
    Ok(table)
}

fn check(field: &[u8]) -> std::result::Result<&str, String> {
    let name = std::str::from_utf8(field).map_err(|e| format!("not UTF-8: {e}"))?;
    match name.bytes().find(|b| *b < 0x20 || *b == 0x7F) {
        Some(bad) => Err(format!(
            "control byte 0x{bad:02X} in {}",
            name.escape_debug()
        )),
        None => Ok(name),
    }
}

/// Write names back in the game's own shape: NUL after every name, including
/// the last.
pub fn write<S: AsRef<str>>(names: &[S]) -> Vec<u8> {
    let mut out = Vec::new();
    for name in names {
        out.extend_from_slice(name.as_ref().as_bytes());
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first bytes of `x64/audio/dlcbeach_game.dat150.nametable` from a
    /// retail Legacy install, truncated at a name boundary.
    const REAL_PREFIX: &[u8] = b"AUDIO_ITEM_SNSPISTOL\0BIFTA\0BIFTA_ENGINE\0KALAHARI\0";

    #[test]
    fn a_real_name_table_parses_to_its_names() {
        let table = parse(REAL_PREFIX).expect("parse");
        assert_eq!(
            table.names,
            vec!["AUDIO_ITEM_SNSPISTOL", "BIFTA", "BIFTA_ENGINE", "KALAHARI"]
        );
        assert!(table.skipped.is_empty());
    }

    #[test]
    fn writing_what_was_parsed_reproduces_the_bytes() {
        let table = parse(REAL_PREFIX).expect("parse");
        assert_eq!(write(&table.names), REAL_PREFIX);
    }

    /// ⚠ A trailing NUL is the terminator of the last name, not an empty
    /// name. Treating it as one puts `""` in the index, which answers every
    /// empty field in the game.
    #[test]
    fn a_trailing_nul_does_not_become_an_empty_name() {
        assert_eq!(parse(b"A\0B\0").unwrap().names, vec!["A", "B"]);
        assert_eq!(parse(b"A\0B").unwrap().names, vec!["A", "B"]);
        assert_eq!(parse(b"A\0\0\0B\0").unwrap().names, vec!["A", "B"]);
        assert!(parse(b"").unwrap().names.is_empty());
        assert!(parse(b"\0\0\0").unwrap().names.is_empty());
    }

    /// The shape retail actually ships, reproduced: one malformed name among
    /// many good ones.
    #[test]
    fn one_bad_name_is_skipped_and_named_rather_than_losing_the_file() {
        let table = parse(b"HS4_IGUARD1\0HS4_IGUARD4\n_PVG\0HS4_IGUARD5\0").expect("parse");
        assert_eq!(table.names, vec!["HS4_IGUARD1", "HS4_IGUARD5"]);
        assert_eq!(table.skipped.len(), 1);
        assert_eq!(table.skipped[0].index, 1);
        assert!(
            table.skipped[0].reason.contains("0x0A"),
            "{}",
            table.skipped[0].reason
        );
    }

    #[test]
    fn a_file_that_is_not_a_name_table_is_refused() {
        // A `.dat` mistaken for its own `.nametable`: two little-endian hash
        // words, the first `BIFTA`'s, which are not UTF-8.
        let err = parse(&[0x97, 0x82, 0x29, 0xEB, 0xB4, 0x50, 0xEE, 0x0E])
            .expect_err("binary accepted as names");
        assert!(format!("{err}").contains("not a name table"), "{err}");

        // Valid UTF-8 and still not a name table.
        let err = parse(b"\x07\x08\x0B\0\x01\x02\x03").expect_err("control bytes accepted");
        assert!(format!("{err}").contains("control byte"), "{err}");

        // ⚠ The line is the majority, so a table that is half rubbish is
        // refused and one that is one entry short of half is kept.
        assert!(parse(b"GOOD\0\x01\0").is_err());
        assert_eq!(parse(b"GOOD\0ALSOGOOD\0\x01\0").unwrap().names.len(), 2);
    }

    /// ⚠ The names are hashed with the **case-folding** function. Measured:
    /// all 42 names across `mpbeach`'s three `game.dat` tables hash under it
    /// to a 32-bit word in the sibling `.dat`, and under the literal function
    /// to a word in none of them.
    #[test]
    fn the_names_are_indexed_under_the_function_the_dat_uses() {
        use crate::names::{HashFlavour, NameIndex};

        let mut index = NameIndex::new("gta5");
        index.extend(parse(REAL_PREFIX).expect("parse").names);

        assert_eq!(
            index.resolve_with(HashFlavour::CaseFolding, 0xEB29_8297),
            Some("BIFTA")
        );
        assert_eq!(
            index.resolve_with(HashFlavour::CaseFolding, 0x0585_2838),
            Some("KALAHARI")
        );
    }
}
