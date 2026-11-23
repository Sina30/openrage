//! One glob, for names and for paths, so the two cannot come to disagree.
//!
//! ⚠ A matcher that does not know `/` exists lets `*` cross separators, so
//! `*.ydr` matches `a/b/c.ydr` and `data/*.ydr` cannot mean *directly under
//! `data`*. `or-cli`'s `search` and the VFS both take this one.
//!
//! | | matches |
//! |---|---|
//! | `?` | one character, never `/` |
//! | `*` | any run of characters, never `/` |
//! | `**` | any run of whole segments, including none |
//!
//! `**` is only `**` when it is a **whole segment**. `a**b` is two stars in
//! one segment, which is `a*b`; reading it as a segment wildcard makes `**`
//! look like it works when the pattern is really matched as a flat string.

/// A compiled path pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// `**` — any run of whole segments, including none.
    AnyDepth,
    Literal(String),
}

impl Pattern {
    /// Compile a pattern. Case is folded, because RAGE paths are.
    pub fn new(pattern: &str) -> Self {
        let segments = pattern
            .split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s == "**" {
                    Segment::AnyDepth
                } else {
                    Segment::Literal(s.to_ascii_lowercase())
                }
            })
            .collect();
        Self { segments }
    }

    /// Is this a single-segment pattern with no `**`, matched against the file
    /// name rather than the whole path?
    pub fn is_name_only(&self) -> bool {
        self.segments.len() == 1 && matches!(self.segments[0], Segment::Literal(_))
    }

    /// ⚠ Called once per file of an install by a search over its index, so it
    /// folds case as it compares rather than lowering a copy of every segment,
    /// and answers `**/name` from the last segment alone.
    pub fn matches_path(&self, path: &str) -> bool {
        if let [Segment::AnyDepth, Segment::Literal(name)] = self.segments.as_slice() {
            let leaf = path.rsplit(['/', '\\']).find(|s| !s.is_empty());
            return leaf.is_some_and(|leaf| segment_matches(name, leaf));
        }
        let text: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        matches_from(&self.segments, &text)
    }

    /// Match the last segment only — the shape a name search wants.
    pub fn matches_name(&self, name: &str) -> bool {
        match self.segments.last() {
            Some(Segment::Literal(literal)) => segment_matches(literal, name),
            _ => true,
        }
    }
}

fn matches_from(pattern: &[Segment], text: &[&str]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((Segment::AnyDepth, rest)) => {
            // Zero segments, then one, then two… `**` at the end matches
            // everything remaining, which is why the loop runs to `len()`.
            (0..=text.len()).any(|skip| matches_from(rest, &text[skip..]))
        }
        Some((Segment::Literal(literal), rest)) => match text.split_first() {
            Some((head, tail)) if segment_matches(literal, head) => matches_from(rest, tail),
            _ => false,
        },
    }
}

/// `*` and `?` inside one segment of `text`, whose case is folded here; the
/// pattern was folded when it was compiled. Neither crosses a separator,
/// because a segment has none left in it by the time this is called.
fn segment_matches(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);

    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some(p);
                mark = t;
                p += 1;
            }
            Some(b'?') => {
                p += 1;
                t += 1;
            }
            Some(c) if *c == text[t].to_ascii_lowercase() => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some(s) => {
                    p = s + 1;
                    mark += 1;
                    t = mark;
                }
                None => return false,
            },
        }
    }
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_does_not_cross_a_separator_and_a_double_star_does() {
        let shallow = Pattern::new("data/*.ydr");
        assert!(shallow.matches_path("data/x.ydr"));
        assert!(
            !shallow.matches_path("data/deeper/x.ydr"),
            "`*` crossing a separator is the defect this primitive exists for"
        );

        let deep = Pattern::new("data/**/*.ydr");
        assert!(deep.matches_path("data/x.ydr"), "`**` matches no segments");
        assert!(deep.matches_path("data/a/x.ydr"));
        assert!(deep.matches_path("data/a/b/c/x.ydr"));
        assert!(!deep.matches_path("other/x.ydr"));
    }

    #[test]
    fn the_definition_of_done_pattern_reaches_every_depth() {
        let p = Pattern::new("**/*.ydr");
        for path in [
            "x.ydr",
            "a/x.ydr",
            "a.rpf/x.ydr",
            "a.rpf/b.rpf/c.rpf/d.rpf/x.ydr",
        ] {
            assert!(p.matches_path(path), "{path}");
        }
        assert!(!p.matches_path("a.rpf/x.ytd"));
    }

    /// ⚠ `**` is a segment, not two stars.
    #[test]
    fn a_double_star_inside_a_segment_is_not_a_segment_wildcard() {
        let p = Pattern::new("a**b");
        assert!(p.is_name_only(), "`a**b` is one segment, so it is a name");
        assert!(p.matches_path("axxb"));
        assert!(
            !p.matches_path("a/x/b"),
            "reading `a**b` as a segment wildcard is how a flat matcher \
             passes for a path one"
        );
    }

    #[test]
    fn a_name_pattern_says_so_and_a_path_pattern_does_not() {
        assert!(Pattern::new("*.ypt").is_name_only());
        assert!(Pattern::new("a?c").is_name_only());
        assert!(!Pattern::new("**/*.ypt").is_name_only());
        assert!(!Pattern::new("data/*.ypt").is_name_only());
    }

    /// The cases `or-cli`'s own matcher pins, against this one.
    #[test]
    fn every_case_the_cli_matcher_pinned_still_answers_the_same() {
        for (pattern, name, expected) in [
            ("*.ypt", "scr_foo.ypt", true),
            ("weapon_*.meta", "weapon_railgunxm3.meta", true),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("*.ypt", "foo.ytd", false),
            ("*", "anything", true),
        ] {
            assert_eq!(
                Pattern::new(pattern).matches_name(name),
                expected,
                "{pattern} against {name}"
            );
        }
    }

    /// ⚠ `**/name` is answered from the last segment alone, which has to agree with the walk over every segment.
    #[test]
    fn a_name_at_any_depth_is_answered_from_the_last_segment() {
        let p = Pattern::new("**/*lod*");
        assert!(p.matches_path("LOD2.ydr"));
        assert!(p.matches_path("a.rpf\\b/x_lod.ydr"));
        assert!(
            p.matches_path("a/lod_x/"),
            "a trailing separator ends no segment"
        );
        assert!(
            !p.matches_path("lod/x.ydr"),
            "a folder's name is not the file's"
        );
        assert!(!p.matches_path(""));
    }

    #[test]
    fn case_is_folded_on_both_sides() {
        assert!(Pattern::new("**/*.YDR").matches_path("A.RPF/X.ydr"));
        assert!(Pattern::new("*.ydr").matches_name("X.YDR"));
    }
}
