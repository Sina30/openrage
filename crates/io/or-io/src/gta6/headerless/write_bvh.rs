//! A BVH record fitted again over items that moved: its frame set from a box that changed, each
//! leaf that holds a moved item boxed again in that frame, and every parent the union of its two
//! children. The nodes' order, their subtree counts and the items each leaf names are kept.

use super::bytes::sections;
use super::write_physics::{local_box, put, put_vec3, refuse};
use super::{Bound, Result};

/// A BVH node's bytes: a quantised box, then a first item or a subtree's node count, then an item
/// count, zero for a parent.
pub(super) const NODE: usize = 16;

/// A box, its minimum then its maximum.
pub(super) type Span = ([f32; 3], [f32; 3]);

/// The box that holds both `a` and `b`.
pub(super) fn union(a: Span, b: Span) -> Span {
    (
        std::array::from_fn(|k| a.0[k].min(b.0[k])),
        std::array::from_fn(|k| a.1[k].max(b.1[k])),
    )
}

/// The eight corners of `span` placed by `m`, whose first three rows are the axes and whose fourth
/// is the origin.
pub(super) fn corners(m: &[[f32; 4]; 4], (lo, hi): Span) -> [[f32; 3]; 8] {
    std::array::from_fn(|k| {
        let p = [
            if k & 1 == 0 { lo[0] } else { hi[0] },
            if k & 2 == 0 { lo[1] } else { hi[1] },
            if k & 4 == 0 { lo[2] } else { hi[2] },
        ];
        std::array::from_fn(|a| p[0] * m[0][a] + p[1] * m[1][a] + p[2] * m[2][a] + m[3][a])
    })
}

/// The box of `span` placed by `m`.
fn placed(m: &[[f32; 4]; 4], span: Span) -> Span {
    let [first, rest @ ..] = corners(m, span);
    rest.iter().fold((first, first), |s, c| union(s, (*c, *c)))
}

/// GTA V's quantiser of a BVH box face, in f32: down for a minimum, up for a maximum, clamped to
/// sixteen bits.
pub(super) fn face(x: f32, centre: f32, scale: f32, up: bool) -> i16 {
    let steps = (x - centre) * scale;
    let steps = if up { steps.ceil() } else { steps.floor() };
    steps.clamp(-32768.0, 32767.0) as i16
}

/// What a refit reads of a BVH record.
pub(super) struct Tree {
    at: usize,
    centre: [f32; 3],
    /// Quanta a metre, GTA V's 65534 / size.
    scale: [f32; 3],
    /// The box [`Tree::reframe`] set the frame from.
    reframed: Option<Span>,
    nodes_at: usize,
    nodes: Vec<[u8; NODE]>,
}

/// The BVH record at `at` of `section`.
pub(super) fn tree(section: &[u8], at: usize) -> Result<Tree> {
    let tree = sections(section, Some(&[]), |v, _| {
        let nodes_at = v.to("BVH nodes", at + 0x50)?;
        let count = v.u32("BVH node count", at + 0x58)? as usize;
        let nodes = (v.array("BVH nodes", nodes_at, count, NODE)?)
            .as_chunks::<NODE>()
            .0
            .to_vec();
        Ok(Tree {
            at,
            centre: v.vec3("BVH centre", at + 0x20)?,
            scale: v.vec3("BVH scale", at + 0x30)?,
            reframed: None,
            nodes_at,
            nodes,
        })
    })?;
    if tree.scale.iter().any(|s| !s.is_finite()) {
        return Err(refuse("the BVH's scale is not a number"));
    }
    Ok(tree)
}

fn word(node: &[u8; NODE], o: usize) -> u16 {
    u16::from_le_bytes([node[o], node[o + 1]])
}

fn node_box(node: &[u8; NODE]) -> [i16; 6] {
    std::array::from_fn(|k| word(node, 2 * k) as i16)
}

impl Tree {
    /// `span` as a leaf of this frame holds it.
    fn quantised(&self, (lo, hi): Span) -> [i16; 6] {
        let (c, s) = (self.centre, self.scale);
        std::array::from_fn(|k| match k {
            0..3 => face(lo[k], c[k], s[k], false),
            _ => face(hi[k - 3], c[k - 3], s[k - 3], true),
        })
    }

    /// The frame GTA V's `SetExtents` gives `span`, which [`Tree::write`] writes: the box, its
    /// centre, 65534 quanta over its size and its size over 65534 a quantum.
    pub(super) fn reframe(&mut self, (lo, hi): Span) -> Result<()> {
        let size: [f32; 3] = std::array::from_fn(|a| hi[a] - lo[a]);
        let centre: [f32; 3] = std::array::from_fn(|a| (lo[a] + hi[a]) / 2.0);
        let scale = size.map(|s| 65534.0 / s);
        let finite = centre.iter().chain(&scale).all(|x| x.is_finite());
        if !(finite && size.iter().all(|s| *s > 0.0)) {
            return Err(refuse("a BVH's box would have no extent on an axis"));
        }
        (self.centre, self.scale, self.reframed) = (centre, scale, Some((lo, hi)));
        Ok(())
    }

    /// Every node's box: each leaf boxed again where `leaf`, given its first item and its item
    /// count, gives a box (`None` keeps it), and every parent the union of its two, bottom up.
    pub(super) fn refit(
        &self,
        mut leaf: impl FnMut(usize, usize) -> Result<Option<Span>>,
    ) -> Result<Vec<[i16; 6]>> {
        let mut boxes: Vec<[i16; 6]> = self.nodes.iter().map(node_box).collect();
        // Children follow their parent: the left at the next node, the right after the left's subtree.
        let shape = || refuse("the BVH's nodes are not in the preorder their subtree counts state");
        let mut size = vec![0usize; self.nodes.len()];
        for i in (0..self.nodes.len()).rev() {
            let node = &self.nodes[i];
            let (first, items) = (usize::from(word(node, 0xC)), usize::from(word(node, 0xE)));
            if items > 0 {
                size[i] = 1;
                if let Some(span) = leaf(first, items)? {
                    boxes[i] = self.quantised(span);
                }
                continue;
            }
            let left = i + 1;
            let right = left + size.get(left).copied().ok_or_else(shape)?;
            let right_size = size.get(right).copied().ok_or_else(shape)?;
            if 1 + size[left] + right_size != first {
                return Err(shape());
            }
            size[i] = first;
            let (l, r) = (boxes[left], boxes[right]);
            boxes[i] = std::array::from_fn(|k| match k {
                0..3 => l[k].min(r[k]),
                _ => l[k].max(r[k]),
            });
        }
        if size.first().is_some_and(|root| *root != self.nodes.len()) {
            return Err(shape());
        }
        Ok(boxes)
    }

    /// The frame [`Tree::reframe`] set, if any, and each node of `boxes` that changed, written;
    /// each frame vector's fourth lane kept.
    pub(super) fn write(&self, section: &mut [u8], boxes: &[[i16; 6]]) -> Result<()> {
        if let Some((lo, hi)) = self.reframed {
            let quantum: [f32; 3] = std::array::from_fn(|a| (hi[a] - lo[a]) / 65534.0);
            put_vec3(section, self.at, lo)?;
            put_vec3(section, self.at + 0x10, hi)?;
            put_vec3(section, self.at + 0x20, self.centre)?;
            put_vec3(section, self.at + 0x30, self.scale)?;
            put_vec3(section, self.at + 0x40, quantum)?;
        }
        for (i, (node, now)) in self.nodes.iter().zip(boxes).enumerate() {
            if node_box(node) != *now {
                let bytes: Vec<u8> = now.iter().flat_map(|q| q.to_le_bytes()).collect();
                put(section, self.nodes_at + NODE * i, &bytes)?;
            }
        }
        Ok(())
    }
}

/// The BVH over the children of the composite at `at`, fitted again over each child of `edited`
/// whose box placed by its matrix is not `read`'s, every leaf where `reframe` sets its frame from
/// `edited`'s box; nothing where the composite has none.
pub(super) fn refit_composite(
    section: &mut [u8],
    at: usize,
    read: &Bound,
    edited: &Bound,
    reframe: bool,
) -> Result<()> {
    let bvh = sections(section, Some(&[]), |v, _| v.ptr("composite BVH", at + 0xA0))?;
    let Some(bvh) = bvh else {
        return Ok(());
    };
    let boxed = |b: &Bound| -> Vec<Option<Span>> {
        (b.children.iter())
            .map(|c| {
                let shape = c.bound.as_ref()?.as_ref().ok()?;
                Some(placed(&c.matrix, local_box(shape)?))
            })
            .collect()
    };
    let (was, now) = (boxed(read), boxed(edited));
    if was.len() != now.len() {
        return Err(refuse("the bound holds another number of children"));
    }
    let changed: Vec<bool> = was.iter().zip(&now).map(|(w, n)| w != n).collect();
    if !reframe && !changed.contains(&true) {
        return Ok(());
    }
    let mut tree = tree(section, bvh)?;
    if reframe {
        tree.reframe((edited.box_min, edited.box_max))?;
    }
    let boxes = tree.refit(|first, items| {
        let children = first..first + items;
        let past = || refuse("a composite BVH's leaf names a child past its children");
        let spans = now.get(children.clone()).ok_or_else(past)?;
        let moved = changed.get(children).ok_or_else(past)?;
        if !reframe && !moved.contains(&true) {
            return Ok(None);
        }
        let mut span: Option<Span> = None;
        for s in spans.iter().copied() {
            let s = s.ok_or(refuse(
                "a composite BVH's leaf names a child whose box is not read",
            ))?;
            span = Some(span.map_or(s, |held| union(held, s)));
        }
        Ok(span)
    })?;
    tree.write(section, &boxes)
}
