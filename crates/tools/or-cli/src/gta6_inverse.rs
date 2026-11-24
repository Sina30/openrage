//! What the in-memory inverse checks share: one scanned body read, and a line for each edit saying
//! whether the edit and its inverse give the stored body back. Nothing is written.

use std::fs;

use or_io::container::rpf::{read_rpf9_body, Rpf9ScanFind, Rpf9Sections};

/// The verdict of an inverse that compares bytes.
pub(crate) const BYTES: &str = "its sections are the stored body's, byte for byte";
/// The verdict of an inverse that compares what the body reads as.
// An added part's bytes stay past the section's end, where nothing names them.
pub(crate) const READ: &str = "it reads as the stored body";

/// How an edit and its inverse left a body.
pub(crate) enum Outcome {
    /// Given back, as the text says it was compared.
    Same(&'static str),
    /// The edit was made and its inverse did not give the body back, or was refused.
    Differs(String),
    /// The edit itself was refused, so there was nothing to undo.
    Refused(String),
}

/// The body the `<archive.rpf> <offset> [--limit <bytes>]` of `args` name, as a scan reads it.
pub(crate) fn scanned(args: &[String], usage: &str) -> Result<(Rpf9ScanFind, Vec<u8>), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let [path, offset] = args.as_slice() else {
        return Err(usage.to_string());
    };
    let offset = super::rpf::parse_offset(offset)?;
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())
}

/// The body `args` name and its sections.
pub(crate) fn sections(
    args: &[String],
    usage: &str,
) -> Result<(Rpf9ScanFind, Rpf9Sections), String> {
    let (find, bytes) = scanned(args, usage)?;
    let sections = Rpf9Sections::of(&find, &bytes)
        .ok_or_else(|| format!("the body at {:#X} is not a resource", find.offset))?;
    Ok((find, sections))
}

/// Where `a` and `b` first differ, said as an offset.
pub(crate) fn first_difference(a: &[u8], b: &[u8]) -> String {
    match (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
        Some(at) => format!(
            "the bytes differ from {at:#X} ({} and {} bytes)",
            a.len(),
            b.len()
        ),
        None => "the bytes are the same".to_string(),
    }
}

/// Whether `work` is `stored` byte for byte, and where not, the section that differs and where.
pub(crate) fn compared(work: &Rpf9Sections, stored: &Rpf9Sections) -> Outcome {
    let (v, held) = (&work.virtual_section, &stored.virtual_section);
    if v != held {
        return Outcome::Differs(format!("virtual section: {}", first_difference(v, held)));
    }
    if work.physical != stored.physical {
        let at = first_difference(&work.physical, &stored.physical);
        return Outcome::Differs(format!("physical section: {at}"));
    }
    match work.form == stored.form {
        true => Outcome::Same(BYTES),
        false => Outcome::Differs("the sections are written in another form".to_string()),
    }
}

/// One line for the edit `what`; false where it did not give the body back.
pub(crate) fn said(what: &str, outcome: Outcome) -> bool {
    match outcome {
        Outcome::Same(how) => {
            println!("  ok       {what}: {how}");
            true
        }
        Outcome::Differs(how) => {
            println!("  FAIL     {what}: {how}");
            false
        }
        Outcome::Refused(why) => {
            println!("  refused  {what}: {why}");
            true
        }
    }
}

/// The end of a run of lines: an error where one did not give the body back.
pub(crate) fn verdict(held: bool) -> Result<(), String> {
    println!("Each edit and its inverse ran in memory; nothing was written.");
    match held {
        true => Ok(()),
        false => Err("an edit and its inverse did not give the stored body back".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_difference_is_said_where_it_starts() {
        assert_eq!(
            first_difference(&[1, 2, 3], &[1, 2, 4, 5]),
            "the bytes differ from 0x2 (3 and 4 bytes)"
        );
        assert_eq!(first_difference(&[1], &[1]), "the bytes are the same");
        assert!(said(
            "an edit",
            Outcome::Refused("no such part".to_string())
        ));
        assert!(!said(
            "an edit",
            Outcome::Differs("reads otherwise".to_string())
        ));
        assert!(verdict(true).is_ok());
        assert!(verdict(false).is_err());
    }

    #[test]
    fn sections_compare_byte_for_byte_and_say_which_differs() {
        let stored = Rpf9Sections {
            form: or_io::container::rpf::Rpf9Form::BlockMapped,
            virtual_section: vec![1, 2, 3, 4],
            physical: vec![9, 9],
        };
        assert!(matches!(compared(&stored, &stored), Outcome::Same(BYTES)));
        let mut moved = stored.clone();
        moved.physical[1] = 8;
        let Outcome::Differs(how) = compared(&moved, &stored) else {
            panic!("a changed physical byte compares the same");
        };
        assert_eq!(
            how,
            "physical section: the bytes differ from 0x1 (2 and 2 bytes)"
        );
        moved.virtual_section.push(0);
        let Outcome::Differs(how) = compared(&moved, &stored) else {
            panic!("a longer virtual section compares the same");
        };
        assert_eq!(
            how,
            "virtual section: the bytes differ from 0x4 (5 and 4 bytes)"
        );
    }
}
