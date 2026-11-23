//! What `rpf scan-open` prints of a body that is a pool of names: printable ASCII names, each
//! ended by a NUL, which an audio configuration body's items point into by offset.

/// The names of `bytes` with their offsets, where every byte is a name's or the NUL that ends it.
fn names(bytes: &[u8]) -> Option<Vec<(usize, &str)>> {
    let body = bytes.strip_suffix(b"\0")?;
    let mut at = 0;
    body.split(|b| *b == 0)
        .map(|name| {
            let start = at;
            at += name.len() + 1;
            let printable = !name.is_empty() && name.iter().all(|b| (0x20..0x7F).contains(b));
            let name = std::str::from_utf8(name).ok().filter(|_| printable)?;
            Some((start, name))
        })
        .collect()
}

/// The names of `bytes` one to a line after their offsets, as `rpf scan-open` lists them; `None` where they are no pool.
pub fn listing(bytes: &[u8]) -> Option<String> {
    let mut out = String::new();
    for (at, name) in names(bytes)? {
        out.push_str(&format!("{at:#08X}  {name}\n"));
    }
    Some(out)
}

/// Prints `bytes` as a name pool, each name at its offset; `false` where they are not one.
pub(crate) fn print(bytes: &[u8]) -> bool {
    let Some(names) = names(bytes) else {
        return false;
    };
    println!(
        "  name pool     {} names in {} bytes, each ended by a NUL: an audio configuration item \
         states the offset of its name in bytes 1 to 3, and the name's hash is its key",
        names.len(),
        bytes.len()
    );
    for (at, name) in names {
        println!("    {at:#08X}  {name}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{listing, names};

    #[test]
    fn a_run_of_names_each_ended_by_a_nul_is_a_pool_and_anything_else_is_not() {
        assert_eq!(
            names(b"alpha\0b c\0last_one\0"),
            Some(vec![(0, "alpha"), (6, "b c"), (10, "last_one")])
        );
        assert_eq!(
            listing(b"alpha\0b c\0").as_deref(),
            Some("0x000000  alpha\n0x000006  b c\n")
        );
        assert_eq!(listing(b"alpha"), None);
        for not_a_pool in [
            &b""[..],
            b"\0",
            b"alpha",
            b"alpha\0\0beta\0",
            b"alpha\0be\x01ta\0",
            b"caf\xC3\xA9\0",
        ] {
            assert_eq!(names(not_a_pool), None, "{not_a_pool:?}");
        }
    }
}
