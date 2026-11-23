use std::ops::{ControlFlow, Range};
use std::sync::atomic::{AtomicBool, Ordering};

use ropey::Rope;

pub const MATCH_CAP: usize = 100_000;

pub const MATCH_BATCH: usize = 1024;

pub const SEARCH_CHUNK_BYTES: usize = 64 * 1024;

pub const SEARCH_CHUNK_OVERLAP: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, Default)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

pub enum Query {
    Literal {
        needle: String,
        needle_folded: String,
        case_sensitive: bool,
        whole_word: bool,
    },
    Regex(regex::Regex),
}

impl Query {
    pub fn is_empty(&self) -> bool {
        match self {
            Query::Literal { needle, .. } => needle.is_empty(),
            Query::Regex(r) => r.as_str().is_empty(),
        }
    }
}

pub fn build_query(pattern: &str, opts: FindOptions) -> Result<Query, String> {
    if pattern.is_empty() {
        return Ok(Query::Literal {
            needle: String::new(),
            needle_folded: String::new(),
            case_sensitive: opts.case_sensitive,
            whole_word: opts.whole_word,
        });
    }
    if opts.regex {
        let prefixed = if opts.case_sensitive {
            pattern.to_string()
        } else {
            format!("(?i){}", pattern)
        };
        let re = regex::RegexBuilder::new(&prefixed)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Query::Regex(re))
    } else {
        let folded = if opts.case_sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };
        Ok(Query::Literal {
            needle: pattern.to_string(),
            needle_folded: folded,
            case_sensitive: opts.case_sensitive,
            whole_word: opts.whole_word,
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SearchOutcome {
    pub total_emitted: usize,
    pub truncated: bool,
    pub cancelled: bool,
}

pub fn find_iter(
    rope: &Rope,
    scope: Range<usize>,
    query: &Query,
    cancel: &AtomicBool,
    on_batch: &mut dyn FnMut(&[Range<usize>]) -> ControlFlow<()>,
) -> SearchOutcome {
    let mut outcome = SearchOutcome::default();
    if query.is_empty() {
        return outcome;
    }
    let total_chars = rope.len_chars();
    let scope = clamp_scope(scope, total_chars);
    if scope.is_empty() {
        return outcome;
    }

    let mut batch: Vec<Range<usize>> = Vec::with_capacity(MATCH_BATCH);
    let mut last_emitted_end: usize = 0;
    let mut last_emitted_start: usize = usize::MAX;
    let mut emit_one = |range: Range<usize>,
                        outcome: &mut SearchOutcome,
                        batch: &mut Vec<Range<usize>>,
                        last_emitted_start: &mut usize,
                        _last_emitted_end: &mut usize|
     -> ControlFlow<()> {
        if range.start == *last_emitted_start {
            return ControlFlow::Continue(());
        }
        *last_emitted_start = range.start;
        batch.push(range);
        outcome.total_emitted += 1;
        if batch.len() >= MATCH_BATCH {
            let r = on_batch_drain(batch, on_batch);
            if r.is_break() {
                return ControlFlow::Break(());
            }
        }
        if outcome.total_emitted >= MATCH_CAP {
            outcome.truncated = true;
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    };

    match query {
        Query::Literal { .. } => {
            let outcome_ptr = &mut outcome;
            let _ = scan_literal(rope, scope, query, cancel, &mut |range| {
                emit_one(
                    range,
                    outcome_ptr,
                    &mut batch,
                    &mut last_emitted_start,
                    &mut last_emitted_end,
                )
            });
        }
        Query::Regex(re) => {
            let outcome_ptr = &mut outcome;
            let _ = scan_regex(rope, scope, re, cancel, &mut |range| {
                emit_one(
                    range,
                    outcome_ptr,
                    &mut batch,
                    &mut last_emitted_start,
                    &mut last_emitted_end,
                )
            });
        }
    }

    if cancel.load(Ordering::Relaxed) {
        outcome.cancelled = true;
    }
    if !batch.is_empty() {
        let _ = on_batch_drain(&mut batch, on_batch);
    }
    outcome
}

fn on_batch_drain(
    batch: &mut Vec<Range<usize>>,
    on_batch: &mut dyn FnMut(&[Range<usize>]) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let r = on_batch(batch);
    batch.clear();
    r
}

fn clamp_scope(scope: Range<usize>, total_chars: usize) -> Range<usize> {
    let start = scope.start.min(total_chars);
    let end = scope.end.min(total_chars).max(start);
    start..end
}

fn scan_literal(
    rope: &Rope,
    scope: Range<usize>,
    query: &Query,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Range<usize>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let (needle, needle_folded, case_sensitive, whole_word) = match query {
        Query::Literal {
            needle,
            needle_folded,
            case_sensitive,
            whole_word,
        } => (
            needle.as_str(),
            needle_folded.as_str(),
            *case_sensitive,
            *whole_word,
        ),
        _ => return ControlFlow::Continue(()),
    };
    if needle.is_empty() {
        return ControlFlow::Continue(());
    }

    let scope_byte_start = rope.char_to_byte(scope.start);
    let scope_byte_end = rope.char_to_byte(scope.end);
    if scope_byte_end <= scope_byte_start {
        return ControlFlow::Continue(());
    }

    let chunk_bytes = SEARCH_CHUNK_BYTES.max(needle.len() * 2);
    let overlap = SEARCH_CHUNK_OVERLAP.max(needle.len().saturating_sub(1));

    let mut window = String::with_capacity(chunk_bytes + overlap);
    let mut window_byte_start: usize = scope_byte_start;
    let mut window_byte_end: usize = scope_byte_start;
    let mut last_emitted_byte_end: usize = 0;
    let mut lc_buf = String::with_capacity(chunk_bytes + overlap);
    let needle_search_bytes: &[u8] = if case_sensitive {
        needle.as_bytes()
    } else {
        needle_folded.as_bytes()
    };
    let finder = memchr::memmem::Finder::new(needle_search_bytes);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        let target_end = (window_byte_end + chunk_bytes).min(scope_byte_end);
        if target_end > window_byte_end {
            extend_window_from_rope(rope, &mut window, window_byte_end, target_end);
            window_byte_end = target_end;
        }

        let search_str: &str = if case_sensitive {
            window.as_str()
        } else {
            lc_buf.clear();
            if window.is_ascii() {
                lc_buf.reserve(window.len());
                for b in window.bytes() {
                    lc_buf.push(b.to_ascii_lowercase() as char);
                }
            } else {
                lc_buf.push_str(&window.to_lowercase());
            }
            lc_buf.as_str()
        };

        let use_direct_indexing = case_sensitive || window.is_ascii();
        if use_direct_indexing {
            for byte_match in finder.find_iter(search_str.as_bytes()) {
                let abs_byte = window_byte_start + byte_match;
                let abs_byte_end = abs_byte + needle.len();
                if abs_byte_end <= last_emitted_byte_end {
                    continue;
                }
                last_emitted_byte_end = abs_byte_end;
                let start_char = rope.byte_to_char(abs_byte);
                let end_char = rope.byte_to_char(abs_byte_end);
                if whole_word && !is_whole_word(rope, start_char, end_char) {
                    continue;
                }
                if emit(start_char..end_char).is_break() {
                    return ControlFlow::Break(());
                }
            }
        } else {
            let needle_chars = needle.chars().count();
            for (start, _) in window.char_indices() {
                if start < (last_emitted_byte_end - window_byte_start) {
                    continue;
                }
                let mut count = 0;
                let mut end = start;
                for (i, _) in window[start..].char_indices() {
                    if count == needle_chars {
                        break;
                    }
                    end = start
                        + i
                        + window[start + i..]
                            .chars()
                            .next()
                            .map_or(0, |c| c.len_utf8());
                    count += 1;
                }
                if count != needle_chars {
                    break;
                }
                if window[start..end].to_lowercase() == needle_folded {
                    let abs_byte = window_byte_start + start;
                    let abs_byte_end = window_byte_start + end;
                    last_emitted_byte_end = abs_byte_end;
                    let start_char = rope.byte_to_char(abs_byte);
                    let end_char = rope.byte_to_char(abs_byte_end);
                    if whole_word && !is_whole_word(rope, start_char, end_char) {
                        continue;
                    }
                    if emit(start_char..end_char).is_break() {
                        return ControlFlow::Break(());
                    }
                }
            }
        }

        if window_byte_end >= scope_byte_end {
            break;
        }
        let to_drop = window.len().saturating_sub(overlap);
        if to_drop > 0 {
            let mut drop_at = to_drop;
            while drop_at < window.len() && !window.is_char_boundary(drop_at) {
                drop_at += 1;
            }
            window.drain(..drop_at);
            window_byte_start += drop_at;
        }
    }
    ControlFlow::Continue(())
}

fn extend_window_from_rope(rope: &Rope, window: &mut String, from_byte: usize, to_byte: usize) {
    if to_byte <= from_byte {
        return;
    }
    let (chunks, mut chunk_byte_start, _, _) = rope.chunks_at_byte(from_byte);
    let mut take_pos = from_byte;
    for chunk in chunks {
        if take_pos >= to_byte {
            break;
        }
        let chunk_end = chunk_byte_start + chunk.len();
        if chunk_end <= take_pos {
            chunk_byte_start = chunk_end;
            continue;
        }
        let local_start = take_pos.saturating_sub(chunk_byte_start);
        let local_end = (to_byte - chunk_byte_start).min(chunk.len());
        if local_start < local_end {
            window.push_str(&chunk[local_start..local_end]);
            take_pos = chunk_byte_start + local_end;
        }
        chunk_byte_start = chunk_end;
    }
}

fn is_whole_word(rope: &Rope, start: usize, end: usize) -> bool {
    let prev_ok = start == 0 || !is_word_char(char_at(rope, start - 1));
    let next_ok = end >= rope.len_chars() || !is_word_char(char_at(rope, end));
    prev_ok && next_ok
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn char_at(rope: &Rope, idx: usize) -> char {
    if idx >= rope.len_chars() {
        return ' ';
    }
    rope.char(idx)
}

fn scan_regex(
    rope: &Rope,
    scope: Range<usize>,
    re: &regex::Regex,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Range<usize>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let scope_byte_start = rope.char_to_byte(scope.start);
    let scope_byte_end = rope.char_to_byte(scope.end);
    if scope_byte_end <= scope_byte_start {
        return ControlFlow::Continue(());
    }

    let chunk_bytes = SEARCH_CHUNK_BYTES;
    let overlap = SEARCH_CHUNK_OVERLAP;

    let mut window = String::with_capacity(chunk_bytes + overlap);
    let mut window_byte_start = scope_byte_start;
    let mut window_byte_end = scope_byte_start;
    let mut last_emitted_byte_end: usize = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        let target_end = (window_byte_end + chunk_bytes).min(scope_byte_end);
        if target_end > window_byte_end {
            extend_window_from_rope(rope, &mut window, window_byte_end, target_end);
            window_byte_end = target_end;
        }

        for m in re.find_iter(&window) {
            let abs_start = window_byte_start + m.start();
            let abs_end = window_byte_start + m.end();
            if abs_end <= last_emitted_byte_end {
                continue;
            }
            last_emitted_byte_end = abs_end;
            let start_char = rope.byte_to_char(abs_start);
            let end_char = rope.byte_to_char(abs_end);
            if emit(start_char..end_char).is_break() {
                return ControlFlow::Break(());
            }
        }

        if window_byte_end >= scope_byte_end {
            break;
        }
        let to_drop = window.len().saturating_sub(overlap);
        if to_drop > 0 {
            let mut drop_at = to_drop;
            while drop_at < window.len() && !window.is_char_boundary(drop_at) {
                drop_at += 1;
            }
            window.drain(..drop_at);
            window_byte_start += drop_at;
        }
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn collect_all(rope: &Rope, q: &Query) -> Vec<Range<usize>> {
        let cancel = AtomicBool::new(false);
        let mut out = Vec::new();
        find_iter(rope, 0..rope.len_chars(), q, &cancel, &mut |b| {
            out.extend_from_slice(b);
            ControlFlow::Continue(())
        });
        out
    }

    #[test]
    fn literal_basic() {
        let r = Rope::from_str("hello world hello");
        let q = build_query("hello", FindOptions::default()).unwrap();
        let m = collect_all(&r, &q);
        assert_eq!(m, vec![0..5, 12..17]);
    }

    #[test]
    fn literal_case_insensitive() {
        let r = Rope::from_str("Hello World hello");
        let opts = FindOptions {
            case_sensitive: false,
            ..Default::default()
        };
        let q = build_query("hello", opts).unwrap();
        let m = collect_all(&r, &q);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn literal_whole_word() {
        let r = Rope::from_str("foo foobar foo_baz foo");
        let opts = FindOptions {
            case_sensitive: true,
            whole_word: true,
            regex: false,
        };
        let q = build_query("foo", opts).unwrap();
        let m = collect_all(&r, &q);
        assert_eq!(m, vec![0..3, 19..22]);
    }

    #[test]
    fn regex_basic() {
        let r = Rope::from_str("ab12 cd34 ef56");
        let opts = FindOptions {
            case_sensitive: true,
            whole_word: false,
            regex: true,
        };
        let q = build_query(r"\d+", opts).unwrap();
        let m = collect_all(&r, &q);
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn cross_chunk_match() {
        let prefix_len = SEARCH_CHUNK_BYTES - 3;
        let mut s = String::with_capacity(prefix_len + 64);
        for _ in 0..prefix_len {
            s.push('a');
        }
        s.push_str("XYZ");
        for _ in 0..32 {
            s.push('a');
        }
        let r = Rope::from_str(&s);
        let q = build_query("XYZ", FindOptions::default()).unwrap();
        let m = collect_all(&r, &q);
        assert_eq!(m.len(), 1);
        let needle_at = m[0].clone();
        assert_eq!(needle_at.end - needle_at.start, 3);
    }

    #[test]
    fn match_cap_truncates() {
        let s = "a".repeat(MATCH_CAP * 2 + 100);
        let r = Rope::from_str(&s);
        let q = build_query("a", FindOptions::default()).unwrap();
        let cancel = AtomicBool::new(false);
        let mut total = 0usize;
        let outcome = find_iter(&r, 0..r.len_chars(), &q, &cancel, &mut |b| {
            total += b.len();
            ControlFlow::Continue(())
        });
        assert!(outcome.truncated);
        assert!(total >= MATCH_CAP);
        assert!(total <= MATCH_CAP + MATCH_BATCH);
    }
}
