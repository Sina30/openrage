//! One path type for on-disk and in-archive paths alike.
//!
//! A segment is an archive when it is **named** like one. Asking the archive
//! would mean opening it, so a predicate a UI asks once per row, such as
//! `is_inside_archive`, would cost I/O and could fail.
//!
//! ⚠ Descending into nested archives was never what was missing:
//! [`ArchiveDevice`](super::ArchiveDevice) already recurses through nested
//! `.rpf`s in memory, eight deep. What was missing is a way to ask where the
//! boundaries are without looking for `.rpf` in a string.

/// The extension that makes a path segment an archive boundary.
pub const ARCHIVE_EXTENSION: &str = ".rpf";

/// A `mount:/inside/an.rpf/deeper.rpf/file.ydr` path, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsPath<'a> {
    mount: &'a str,
    rest: &'a str,
}

impl<'a> VfsPath<'a> {
    pub fn parse(input: &'a str) -> Option<Self> {
        let (mount, rest) = input.split_once(':')?;
        if mount.is_empty() {
            return None;
        }
        Some(Self {
            mount,
            rest: rest.trim_start_matches('/'),
        })
    }

    pub fn mount(&self) -> &'a str {
        self.mount
    }

    /// Everything after the mount name, with no leading separator.
    pub fn rest(&self) -> &'a str {
        self.rest
    }

    /// The path below the mount, split on `/` and `\`, with no empty parts.
    pub fn segments(&self) -> impl DoubleEndedIterator<Item = &'a str> {
        self.rest
            .split(['/', '\\'])
            .filter(|segment| !segment.is_empty())
    }

    pub fn file_name(&self) -> Option<&'a str> {
        self.segments().next_back()
    }

    /// Does this path *name* an archive?
    pub fn is_archive(&self) -> bool {
        self.file_name().is_some_and(is_archive_segment)
    }

    /// Is this path *inside* an archive — under one, rather than being one?
    ///
    /// ⚠ The two are not opposites and a path can be both: an `.rpf` nested in
    /// another `.rpf` is an archive **and** inside one. A caller that treats
    /// them as exclusive gets the nested case wrong.
    pub fn is_inside_archive(&self) -> bool {
        let mut segments: Vec<&str> = self.segments().collect();
        segments.pop();
        segments.iter().copied().any(is_archive_segment)
    }

    /// The archives crossed to reach this path, outermost first.
    ///
    /// Empty for a loose file. For `x.rpf/y.rpf/z.ydr` it is `[x.rpf, y.rpf]`;
    /// for `x.rpf/y.rpf` it is `[x.rpf]`, because the last segment is the
    /// destination rather than something crossed to reach it.
    pub fn archive_chain(&self) -> Vec<&'a str> {
        let mut segments: Vec<&'a str> = self.segments().collect();
        segments.pop();
        segments.retain(|s| is_archive_segment(s));
        segments
    }

    /// How many archive boundaries lie between the mount and this path.
    pub fn archive_depth(&self) -> usize {
        self.archive_chain().len()
    }
}

impl std::fmt::Display for VfsPath<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:/{}", self.mount, self.rest)
    }
}

pub fn is_archive_segment(segment: &str) -> bool {
    let segment = segment.trim_end_matches(['/', '\\']);
    segment.len() > ARCHIVE_EXTENSION.len()
        && segment
            .rsplit('.')
            .next()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(&ARCHIVE_EXTENSION[1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_mount_and_rest() {
        let p = VfsPath::parse("common:/data/effects/x.dat").unwrap();
        assert_eq!(p.mount(), "common");
        assert_eq!(p.rest(), "data/effects/x.dat");

        let p = VfsPath::parse("update:foo.dat").unwrap();
        assert_eq!(p.mount(), "update");
        assert_eq!(p.rest(), "foo.dat");

        assert!(VfsPath::parse("no_mount.dat").is_none());
        assert!(VfsPath::parse(":no_name.dat").is_none());
    }

    #[test]
    fn a_parsed_path_prints_back_as_one_that_parses_the_same() {
        for input in [
            "common:/data/effects/x.dat",
            "update:/x.rpf/y.rpf/z.ydr",
            "dlc:/",
        ] {
            let once = VfsPath::parse(input).expect("parses");
            let printed = once.to_string();
            let twice = VfsPath::parse(&printed).expect("prints back as a path");
            assert_eq!(
                (once.mount(), once.rest()),
                (twice.mount(), twice.rest()),
                "{input} did not survive a print and a reparse (printed {printed})"
            );
        }
    }

    /// The two predicates, including the case that makes them not opposites.
    #[test]
    fn an_archive_and_a_path_inside_one_are_different_questions() {
        let cases = [
            // path, is_archive, is_inside_archive, chain
            ("common:/data/x.ydr", false, false, 0),
            ("common:/dlc.rpf", true, false, 0),
            ("common:/dlc.rpf/x.ydr", false, true, 1),
            ("common:/dlc.rpf/nested.rpf", true, true, 1),
            ("common:/dlc.rpf/nested.rpf/x.ydr", false, true, 2),
            ("common:/a.rpf/b.rpf/c.rpf/d.rpf/x.ydr", false, true, 4),
            // ⚠ Not an archive: the extension has to be the whole of it.
            ("common:/notanrpf", false, false, 0),
            ("common:/x.rpfa", false, false, 0),
            ("common:/.rpf", false, false, 0),
        ];

        for (input, archive, inside, depth) in cases {
            let p = VfsPath::parse(input).expect("parses");
            assert_eq!(p.is_archive(), archive, "is_archive for {input}");
            assert_eq!(
                p.is_inside_archive(),
                inside,
                "is_inside_archive for {input}"
            );
            assert_eq!(p.archive_depth(), depth, "archive_depth for {input}");
        }
    }

    #[test]
    fn the_chain_is_what_was_crossed_and_not_where_it_ended() {
        let p = VfsPath::parse("common:/x.rpf/y.rpf/z.ydr").unwrap();
        assert_eq!(p.archive_chain(), vec!["x.rpf", "y.rpf"]);

        let p = VfsPath::parse("common:/x.rpf/y.rpf").unwrap();
        assert_eq!(
            p.archive_chain(),
            vec!["x.rpf"],
            "the destination is not something crossed to reach itself"
        );
    }

    #[test]
    fn a_backslash_separates_as_a_slash_does() {
        let p = VfsPath::parse(r"common:\dlc.rpf\data\x.ydr").unwrap();
        assert!(p.is_inside_archive());
        assert_eq!(p.file_name(), Some("x.ydr"));
    }

    #[test]
    fn the_extension_is_matched_without_regard_to_case() {
        assert!(is_archive_segment("DLC.RPF"));
        assert!(is_archive_segment("dlc.Rpf"));
        assert!(!is_archive_segment("rpf"));
    }
}
