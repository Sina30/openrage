use std::sync::atomic::{AtomicBool, Ordering};

use ropey::Rope;

pub const MAX_FOLDS: usize = 200_000;

pub const FOLD_INLINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line: u32,
    pub end_line: u32,
}

impl FoldRange {
    pub fn line_count(&self) -> u32 {
        self.end_line.saturating_sub(self.start_line)
    }
    pub fn contains(&self, source_line: u32) -> bool {
        source_line >= self.start_line && source_line <= self.end_line
    }
}

pub struct FoldTree {
    pub ranges: Vec<FoldRange>,
    pub collapsed: Vec<bool>,
    pub tag_indices: Vec<u32>,
    pub tag_pool: Vec<String>,
    pub rope_revision: u64,
}

impl FoldTree {
    pub fn new(
        rope_revision: u64,
        ranges: Vec<FoldRange>,
        tags: Vec<u32>,
        tag_pool: Vec<String>,
    ) -> Self {
        let n = ranges.len();
        Self {
            collapsed: vec![false; n],
            tag_indices: tags,
            tag_pool,
            ranges,
            rope_revision,
        }
    }

    pub fn toggle(&mut self, idx: usize) {
        if let Some(c) = self.collapsed.get_mut(idx) {
            *c = !*c;
        }
    }

    pub fn set_collapsed(&mut self, idx: usize, value: bool) {
        if let Some(c) = self.collapsed.get_mut(idx) {
            *c = value;
        }
    }

    pub fn innermost_at(&self, source_line: u32) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, r) in self.ranges.iter().enumerate() {
            if r.contains(source_line) {
                match best {
                    None => best = Some(i),
                    Some(bi) => {
                        if r.line_count() < self.ranges[bi].line_count() {
                            best = Some(i);
                        }
                    }
                }
            }
        }
        best
    }

    pub fn carry_collapsed_state(&mut self, prev: &FoldTree) {
        for (pi, &was_collapsed) in prev.collapsed.iter().enumerate() {
            if !was_collapsed {
                continue;
            }
            let prev_tag = prev.tag_pool.get(prev.tag_indices[pi] as usize);
            let prev_start = prev.ranges[pi].start_line as i64;

            let mut best: Option<usize> = None;
            let mut best_delta = i64::MAX;
            for (ni, range) in self.ranges.iter().enumerate() {
                let delta = (range.start_line as i64 - prev_start).abs();
                if delta > 5 {
                    continue;
                }
                let new_tag = self.tag_pool.get(self.tag_indices[ni] as usize);
                if new_tag != prev_tag {
                    continue;
                }
                if delta < best_delta {
                    best_delta = delta;
                    best = Some(ni);
                }
            }
            if let Some(ni) = best {
                self.collapsed[ni] = true;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HiddenSpan {
    source_start: u32,
    hidden_lines: u32,
}

pub struct FoldMap {
    spans: Vec<HiddenSpan>,
    prefix_hidden: Vec<u32>,
    total_lines: u32,
    visual_lines: u32,
}

impl FoldMap {
    pub fn empty(total_lines: u32) -> Self {
        Self {
            spans: Vec::new(),
            prefix_hidden: vec![0],
            total_lines,
            visual_lines: total_lines,
        }
    }

    pub fn build(tree: &FoldTree, total_source_lines: u32) -> Self {
        let mut spans: Vec<HiddenSpan> = Vec::new();
        let mut covered_until: i64 = -1;
        for (i, r) in tree.ranges.iter().enumerate() {
            if !tree.collapsed[i] {
                continue;
            }
            if (r.start_line as i64) <= covered_until {
                continue;
            }
            spans.push(HiddenSpan {
                source_start: r.start_line,
                hidden_lines: r.end_line.saturating_sub(r.start_line),
            });
            covered_until = r.end_line as i64;
        }
        let mut prefix_hidden = Vec::with_capacity(spans.len() + 1);
        prefix_hidden.push(0);
        let mut acc = 0u32;
        for s in &spans {
            acc = acc.saturating_add(s.hidden_lines);
            prefix_hidden.push(acc);
        }
        let visual_lines = total_source_lines.saturating_sub(acc);
        Self {
            spans,
            prefix_hidden,
            total_lines: total_source_lines,
            visual_lines,
        }
    }

    pub fn visual_line_count(&self) -> u32 {
        self.visual_lines
    }

    pub fn visual_to_source(&self, vis: u32) -> u32 {
        if self.spans.is_empty() {
            return vis.min(self.total_lines);
        }
        let mut lo = 0usize;
        let mut hi = self.spans.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let span_visual_pos = self.spans[mid].source_start - self.prefix_hidden[mid];
            if span_visual_pos <= vis {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        (vis + self.prefix_hidden[lo]).min(self.total_lines.saturating_sub(1))
    }

    pub fn source_to_visual(&self, src: u32) -> Option<u32> {
        if self.spans.is_empty() {
            return Some(src.min(self.total_lines));
        }
        let span_idx = match self.spans.binary_search_by_key(&src, |s| s.source_start) {
            Ok(i) => i,
            Err(i) => i,
        };
        let mut hidden_before: u32 = 0;
        for s in &self.spans {
            if s.source_start < src {
                let span_end = s.source_start + s.hidden_lines;
                if span_end < src {
                    hidden_before = hidden_before.saturating_add(s.hidden_lines);
                } else {
                    if src == s.source_start {
                    } else {
                        return None;
                    }
                }
            }
        }
        let _ = span_idx;
        Some(src.saturating_sub(hidden_before))
    }

    pub fn is_hidden(&self, src: u32) -> bool {
        self.source_to_visual(src).is_none()
    }
}

pub fn compute_xml_folds(
    rope: &Rope,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64),
) -> (Vec<FoldRange>, Vec<u32>, Vec<String>) {
    let mut ranges: Vec<FoldRange> = Vec::new();
    let mut tags: Vec<u32> = Vec::new();
    let mut tag_pool: Vec<String> = vec!["".to_string()];

    let mut stack: Vec<(u32, u32)> = Vec::new();
    enum MlState {
        None,
        Comment { start_line: u32 },
        Cdata { start_line: u32 },
    }
    let mut ml = MlState::None;

    let total_lines = rope.len_lines();
    let mut bytes_seen: u64 = 0;
    let mut last_progress: u64 = 0;
    let mut intern_cache: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    'lines: for line_idx in 0..total_lines {
        if line_idx % 256 == 0 && cancel.load(Ordering::Relaxed) {
            break;
        }
        let line = rope.line(line_idx);
        let s: std::borrow::Cow<str> = line.into();
        bytes_seen = bytes_seen.saturating_add(s.len() as u64);
        if bytes_seen - last_progress > 1024 * 1024 {
            last_progress = bytes_seen;
            on_progress(bytes_seen);
        }
        let line_no = line_idx as u32;

        let mut bytes = s.as_bytes();
        let mut col: usize = 0;
        match &ml {
            MlState::Comment { start_line } => {
                if let Some(end) = find_subseq(bytes, b"-->") {
                    let start = *start_line;
                    if line_no > start {
                        push_range(
                            &mut ranges,
                            &mut tags,
                            FoldRange {
                                start_line: start,
                                end_line: line_no,
                            },
                            0,
                        );
                    }
                    col = end + 3;
                    ml = MlState::None;
                } else {
                    continue 'lines;
                }
            }
            MlState::Cdata { start_line } => {
                if let Some(end) = find_subseq(bytes, b"]]>") {
                    let start = *start_line;
                    if line_no > start {
                        push_range(
                            &mut ranges,
                            &mut tags,
                            FoldRange {
                                start_line: start,
                                end_line: line_no,
                            },
                            0,
                        );
                    }
                    col = end + 3;
                    ml = MlState::None;
                } else {
                    continue 'lines;
                }
            }
            MlState::None => {}
        }
        bytes = &bytes[col..];
        let line_str = &s[col..];

        let mut i = 0usize;
        while i < bytes.len() {
            let lt = match memchr::memchr(b'<', &bytes[i..]) {
                Some(pos) => i + pos,
                None => break,
            };
            i = lt;
            if line_str[i..].starts_with("<!--") {
                if let Some(end) = line_str[i + 4..].find("-->") {
                    i = i + 4 + end + 3;
                    continue;
                } else {
                    ml = MlState::Comment {
                        start_line: line_no,
                    };
                    break;
                }
            }
            if line_str[i..].starts_with("<![CDATA[") {
                if let Some(end) = line_str[i + 9..].find("]]>") {
                    i = i + 9 + end + 3;
                    continue;
                } else {
                    ml = MlState::Cdata {
                        start_line: line_no,
                    };
                    break;
                }
            }
            if line_str[i..].starts_with("<?") {
                if let Some(end) = line_str[i + 2..].find("?>") {
                    i = i + 2 + end + 2;
                    continue;
                } else {
                    break;
                }
            }
            if line_str[i..].starts_with("<!") {
                if let Some(end) = line_str[i + 2..].find('>') {
                    i = i + 2 + end + 1;
                    continue;
                } else {
                    break;
                }
            }
            let close_rel = match find_unquoted_close(&line_str[i + 1..]) {
                Some(p) => p,
                None => break,
            };
            let tag_close = i + 1 + close_rel;
            let inner = &line_str[i + 1..tag_close];
            let is_close = inner.starts_with('/');
            let inner = if is_close { &inner[1..] } else { inner };
            let self_close = inner.ends_with('/');
            let inner_no_self = if self_close {
                &inner[..inner.len() - 1]
            } else {
                inner
            };
            let name_end = inner_no_self
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inner_no_self.len());
            let name = inner_no_self[..name_end].trim();
            if name.is_empty() {
                i = tag_close + 1;
                continue;
            }
            if !is_close && !self_close {
                let tag_idx = intern_tag(&mut intern_cache, &mut tag_pool, name);
                stack.push((tag_idx, line_no));
            } else if is_close {
                while let Some(&(tag_idx, start_line)) = stack.last() {
                    let same = tag_pool[tag_idx as usize].eq_ignore_ascii_case(name);
                    stack.pop();
                    if same {
                        if line_no > start_line {
                            push_range(
                                &mut ranges,
                                &mut tags,
                                FoldRange {
                                    start_line,
                                    end_line: line_no,
                                },
                                tag_idx,
                            );
                        }
                        break;
                    }
                }
            }
            i = tag_close + 1;
        }
    }

    let mut zipped: Vec<(FoldRange, u32)> = ranges.into_iter().zip(tags).collect();
    zipped.sort_by_key(|(r, _)| (r.start_line, std::cmp::Reverse(r.end_line)));
    let (sorted_ranges, sorted_tags): (Vec<_>, Vec<_>) = zipped.into_iter().unzip();
    (sorted_ranges, sorted_tags, tag_pool)
}

fn push_range(ranges: &mut Vec<FoldRange>, tags: &mut Vec<u32>, r: FoldRange, tag_idx: u32) {
    if ranges.len() >= MAX_FOLDS {
        return;
    }
    ranges.push(r);
    tags.push(tag_idx);
}

fn intern_tag(
    cache: &mut std::collections::HashMap<String, u32>,
    pool: &mut Vec<String>,
    name: &str,
) -> u32 {
    if let Some(&idx) = cache.get(name) {
        return idx;
    }
    let idx = pool.len() as u32;
    pool.push(name.to_string());
    cache.insert(name.to_string(), idx);
    idx
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let finder = memchr::memmem::Finder::new(needle);
    finder.find(haystack)
}

fn find_unquoted_close(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'"' || ch == b'\'' {
            let close = s[i + 1..].find(ch as char).map(|p| i + 1 + p + 1);
            i = close.unwrap_or(bytes.len());
            continue;
        }
        if ch == b'>' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn folds_of(s: &str) -> Vec<FoldRange> {
        let r = Rope::from_str(s);
        let cancel = AtomicBool::new(false);
        let (ranges, _, _) = compute_xml_folds(&r, &cancel, &mut |_| {});
        ranges
    }

    #[test]
    fn single_line_element_is_not_foldable() {
        let f = folds_of("<a>hi</a>");
        assert!(f.is_empty());
    }

    #[test]
    fn multiline_element_folds() {
        let s = "<root>\n  <child/>\n</root>\n";
        let f = folds_of(s);
        assert_eq!(
            f,
            vec![FoldRange {
                start_line: 0,
                end_line: 2
            }]
        );
    }

    #[test]
    fn nested_folds() {
        let s = "<a>\n<b>\n<c/>\n</b>\n</a>\n";
        let f = folds_of(s);
        assert_eq!(
            f,
            vec![
                FoldRange {
                    start_line: 0,
                    end_line: 4
                },
                FoldRange {
                    start_line: 1,
                    end_line: 3
                },
            ]
        );
    }

    #[test]
    fn comment_block_folds() {
        let s = "<!--\nhello\n-->\n";
        let f = folds_of(s);
        assert_eq!(
            f,
            vec![FoldRange {
                start_line: 0,
                end_line: 2
            }]
        );
    }

    #[test]
    fn fold_map_round_trip() {
        let r = Rope::from_str("<root>\n<a>\n<b/>\n</a>\n<c/>\n</root>\n");
        let cancel = AtomicBool::new(false);
        let (ranges, tags, pool) = compute_xml_folds(&r, &cancel, &mut |_| {});
        let mut tree = FoldTree::new(0, ranges, tags, pool);
        assert!(!tree.ranges.is_empty());
        let inner = tree.ranges.iter().position(|r| r.start_line == 1).unwrap();
        tree.set_collapsed(inner, true);
        let total_lines = r.len_lines() as u32;
        let map = FoldMap::build(&tree, total_lines);
        assert!(map.visual_line_count() < total_lines);
        assert!(!map.is_hidden(1));
        assert!(map.is_hidden(2));
        assert!(map.is_hidden(3));
    }
}
