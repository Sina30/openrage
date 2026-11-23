//! Which elements of two runs line up: lines of two texts, or records of two
//! arrays, matched in order.

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Range;

/// ponytail: a stretch where no element is found once on each side is
/// lined up by a table of its lengths' product, up to this many cells, and
/// past it is left unmatched; Myers' linear-space diff would lift the ceiling.
const TABLE_CELLS: usize = 1 << 22;

/// How deep the anchoring nests before a stretch goes to the table, which keeps a worker's stack shallow.
const DEPTH: usize = 48;

/// The pairs of `a` and `b` that line up, ascending on both sides: the ends
/// they share, then the elements found once on each side, then the longest
/// common run of what lies between.
pub fn align<K: Eq + Hash>(a: &[K], b: &[K]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    stretch(a, b, 0..a.len(), 0..b.len(), 0, &mut out);
    out
}

fn stretch<K: Eq + Hash>(
    a: &[K],
    b: &[K],
    mut ra: Range<usize>,
    mut rb: Range<usize>,
    depth: usize,
    out: &mut Vec<(usize, usize)>,
) {
    while !ra.is_empty() && !rb.is_empty() && a[ra.start] == b[rb.start] {
        out.push((ra.start, rb.start));
        (ra.start, rb.start) = (ra.start + 1, rb.start + 1);
    }
    let mut tail = Vec::new();
    while !ra.is_empty() && !rb.is_empty() && a[ra.end - 1] == b[rb.end - 1] {
        (ra.end, rb.end) = (ra.end - 1, rb.end - 1);
        tail.push((ra.end, rb.end));
    }
    if !ra.is_empty() && !rb.is_empty() {
        let anchors = match depth < DEPTH {
            true => anchors(a, b, ra.clone(), rb.clone()),
            false => Vec::new(),
        };
        if anchors.is_empty() {
            table(a, b, ra, rb, out);
        } else {
            let (mut i, mut j) = (ra.start, rb.start);
            for (ai, bj) in anchors {
                stretch(a, b, i..ai, j..bj, depth + 1, out);
                out.push((ai, bj));
                (i, j) = (ai + 1, bj + 1);
            }
            stretch(a, b, i..ra.end, j..rb.end, depth + 1, out);
        }
    }
    out.extend(tail.into_iter().rev());
}

/// The elements found once in each range, as the longest run of them that ascends on both sides.
fn anchors<K: Eq + Hash>(
    a: &[K],
    b: &[K],
    ra: Range<usize>,
    rb: Range<usize>,
) -> Vec<(usize, usize)> {
    // Per element: how often each side holds it, and where it last did.
    let mut seen: HashMap<&K, [usize; 4]> = HashMap::new();
    for i in ra {
        let e = seen.entry(&a[i]).or_insert([0; 4]);
        (e[0], e[1]) = (e[0] + 1, i);
    }
    for j in rb {
        if let Some(e) = seen.get_mut(&b[j]) {
            (e[2], e[3]) = (e[2] + 1, j);
        }
    }
    let mut once: Vec<(usize, usize)> = (seen.values())
        .filter(|e| e[0] == 1 && e[2] == 1)
        .map(|e| (e[1], e[3]))
        .collect();
    once.sort_unstable();
    ascending(&once)
}

/// The longest run of `pairs`, sorted by their first, whose seconds ascend too.
fn ascending(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // `ends[k]` is the pair ending the run of length k + 1 that ends lowest.
    let mut ends: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; pairs.len()];
    for (n, &(_, j)) in pairs.iter().enumerate() {
        let k = ends.partition_point(|&e| pairs[e].1 < j);
        before[n] = k.checked_sub(1).map(|k| ends[k]);
        match ends.get_mut(k) {
            Some(end) => *end = n,
            None => ends.push(n),
        }
    }
    let mut run = Vec::new();
    let mut at = ends.last().copied();
    while let Some(n) = at {
        run.push(pairs[n]);
        at = before[n];
    }
    run.reverse();
    run
}

/// The longest common run of the two ranges, by a table of its lengths; nothing past [`TABLE_CELLS`].
fn table<K: Eq>(
    a: &[K],
    b: &[K],
    ra: Range<usize>,
    rb: Range<usize>,
    out: &mut Vec<(usize, usize)>,
) {
    let (n, m) = (ra.len(), rb.len());
    if n.saturating_mul(m) > TABLE_CELLS {
        return;
    }
    // `longest[i * w + j]` is the longest common run of `a[i..]` and `b[j..]`, from the ranges' starts.
    let w = m + 1;
    let mut longest = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            longest[i * w + j] = if a[ra.start + i] == b[rb.start + j] {
                longest[(i + 1) * w + j + 1] + 1
            } else {
                longest[(i + 1) * w + j].max(longest[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[ra.start + i] == b[rb.start + j] {
            out.push((ra.start + i, rb.start + j));
            (i, j) = (i + 1, j + 1);
        } else if longest[(i + 1) * w + j] >= longest[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<&str> {
        text.split(' ').filter(|s| !s.is_empty()).collect()
    }

    /// Every pair matches equal elements and both sides ascend.
    fn holds(a: &[&str], b: &[&str], pairs: &[(usize, usize)]) {
        for (i, j) in pairs {
            assert_eq!(a[*i], b[*j], "({i}, {j}) pairs unequal elements");
        }
        for w in pairs.windows(2) {
            assert!(w[0].0 < w[1].0 && w[0].1 < w[1].1, "{pairs:?} goes back");
        }
    }

    #[test]
    fn two_equal_runs_line_up_one_for_one() {
        let a = lines("a b c d");
        assert_eq!(align(&a, &a), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn what_one_side_adds_or_drops_leaves_the_rest_lined_up() {
        let (a, b) = (lines("a x c d e"), lines("a c d n e"));
        let pairs = align(&a, &b);
        holds(&a, &b, &pairs);
        assert_eq!(pairs, vec![(0, 0), (2, 1), (3, 2), (4, 4)]);
    }

    #[test]
    fn a_change_at_each_end_of_a_long_run_leaves_everything_between_lined_up() {
        let a: Vec<String> = (0..20_000).map(|n| format!("line {n}")).collect();
        let mut b = a.clone();
        b[0] = "first, edited".into();
        b[19_999] = "last, edited".into();
        b.insert(10_000, "inserted".into());
        let pairs = align(&a, &b);
        assert_eq!(pairs.len(), 19_998, "every unchanged line pairs");
        assert_eq!(pairs.first(), Some(&(1, 1)));
        assert_eq!(pairs.last(), Some(&(19_998, 19_999)));
    }

    #[test]
    fn repeated_elements_with_nothing_unique_still_line_up() {
        let (a, b) = (lines("d e d e d"), lines("e d e d d"));
        let pairs = align(&a, &b);
        holds(&a, &b, &pairs);
        assert_eq!(
            pairs.len(),
            4,
            "the longest common run is e d e d: {pairs:?}"
        );
    }

    #[test]
    fn two_runs_with_nothing_in_common_line_up_nowhere() {
        assert!(align(&lines("a b c"), &lines("x y z")).is_empty());
        assert!(align(&lines(""), &lines("x")).is_empty());
    }
}
