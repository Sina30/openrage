use or_io::desc::{CountSource, FieldRepr, Image, Pack, PackType, Pointee, POINTER_SIZE};
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Hop {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PackPath {
    pub hops: Vec<Hop>,
}

impl PackPath {
    pub fn is_root(&self) -> bool {
        self.hops.is_empty()
    }

    pub fn then(&self, hop: Hop) -> Self {
        let mut hops = self.hops.clone();
        hops.push(hop);
        Self { hops }
    }
}

impl std::fmt::Display for PackPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.hops.is_empty() {
            return f.write_str("(root)");
        }
        for (i, hop) in self.hops.iter().enumerate() {
            if i > 0 {
                f.write_str(" > ")?;
            }
            match hop {
                Hop::Field(name) => f.write_str(name)?,
                Hop::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub label: String,
    pub hop: Option<Hop>,
    pub type_name: Option<String>,
    pub at: usize,
    pub children: Vec<Node>,
    pub note: Option<String>,
    /// Set where a limit left out what the file holds under this node.
    pub cut: Option<Cut>,
}

/// What a limit of the outline left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The part's own parts: it lies [`MAX_DEPTH`] levels down.
    Depth,
    /// A run's elements past the first [`MAX_ELEMENTS`]; the run holds this many.
    Elements(usize),
}

impl Node {
    fn leaf(hop: Hop, label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hop: Some(hop),
            type_name: None,
            at: 0,
            children: Vec::new(),
            note: None,
            cut: None,
        }
    }

    fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    pub fn child(&self, hop: &Hop) -> Option<&Node> {
        self.children.iter().find(|c| c.hop.as_ref() == Some(hop))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub root: Node,
}

impl Outline {
    pub fn of(pack: &Pack, type_name: &str, image: Image<'_>, at: usize) -> Option<Self> {
        Self::of_to(pack, type_name, image, at, MAX_DEPTH)
    }

    /// [`Outline::of`], descending `depth` levels of parts in place of [`MAX_DEPTH`].
    pub fn of_to(
        pack: &Pack,
        type_name: &str,
        image: Image<'_>,
        at: usize,
        depth: usize,
    ) -> Option<Self> {
        let ty = concrete(pack, pack.type_by_name(type_name)?, image, at);
        let root = Above {
            depth: 0,
            limit: depth,
            part: (&ty.name, at),
            parent: None,
        };
        let children = children_of(pack, ty, image, at, &root);
        Some(Self {
            root: Node {
                label: ty.name.clone(),
                hop: None,
                type_name: Some(ty.name.clone()),
                at,
                children,
                note: None,
                cut: None,
            },
        })
    }

    pub fn find(&self, path: &PackPath) -> Option<&Node> {
        let mut node = &self.root;
        for hop in &path.hops {
            node = node.child(hop)?;
        }
        Some(node)
    }
}

/// How many levels of parts an outline descends; the part at the last notes it goes no further.
pub const MAX_DEPTH: usize = 6;

/// How many elements of one run become nodes; the group notes how many it
/// shows of how many, so a truncated run is never silent.
pub const MAX_ELEMENTS: usize = 128;

/// How far down a part is and how far the walk goes, and the typed parts above it, each with where it lies.
///
/// ⚠ A tagged base resolves to the variant that holds it, whose own inline
/// base may resolve to another that holds that one, so a part is reached again
/// at its own address under itself. [`Above::holds`] ends the walk at the
/// second reach: a part is read once, not once for each level of the limit.
struct Above<'a> {
    depth: usize,
    limit: usize,
    part: (&'a str, usize),
    parent: Option<&'a Above<'a>>,
}

impl Above<'_> {
    /// Whether the `name` at `at` is this part or one above it.
    fn holds(&self, name: &str, at: usize) -> bool {
        let mut above = Some(self);
        while let Some(here) = above {
            if here.part == (name, at) {
                return true;
            }
            above = here.parent;
        }
        false
    }
}

fn concrete<'p>(pack: &'p Pack, ty: &'p PackType, image: Image<'_>, at: usize) -> &'p PackType {
    ty.selector
        .as_ref()
        .and_then(|_| crate::resolve_tag(pack, ty, image.at(at)))
        .unwrap_or(ty)
}

fn children_of(
    pack: &Pack,
    ty: &PackType,
    image: Image<'_>,
    base: usize,
    above: &Above<'_>,
) -> Vec<Node> {
    if above.depth >= above.limit {
        return Vec::new();
    }

    let mut out = Vec::new();
    for field in &ty.fields {
        let at = base + field.offset;
        let label = &field.name;
        let hop = || Hop::Field(field.name.clone());

        match &field.repr {
            FieldRepr::Scalar(_)
            | FieldRepr::ScalarArray { .. }
            | FieldRepr::Enum { .. }
            | FieldRepr::Opaque { .. }
            | FieldRepr::Padding { .. } => {}

            FieldRepr::Inline { type_name } => {
                if let Some(node) = described(pack, type_name, image, at, hop(), label, above) {
                    out.push(node);
                }
            }

            FieldRepr::InlineArray { type_name, count } => {
                let Some(elem) = pack.type_by_name(type_name) else {
                    continue;
                };
                let mut group = Node::leaf(hop(), format!("{label} ({count})"));
                for i in 0..(*count).min(MAX_ELEMENTS) {
                    if let Some(node) = described(
                        pack,
                        type_name,
                        image,
                        at + i * elem.size,
                        Hop::Index(i),
                        &format!("[{i}]"),
                        above,
                    ) {
                        group.children.push(node);
                    }
                }
                out.push(elided(group, *count));
            }

            FieldRepr::Ptr(pointee) => {
                if let Some(node) =
                    pointer_node(pack, ty, pointee, image, base, at, hop(), label, above)
                {
                    out.push(node);
                }
            }

            FieldRepr::AtArray(pointee) => match image.at_array(at) {
                Ok(Some(elements)) => {
                    let count = elements.count();
                    match elements.at(0, 0).map(|c| c.base()) {
                        Some(start) if count > 0 => {
                            out.push(run(pack, pointee, image, start, count, hop(), label, above))
                        }
                        _ => out.push(Node::leaf(hop(), format!("{label} (empty)"))),
                    }
                }
                Ok(None) => out.push(Node::leaf(hop(), format!("{label} (empty)"))),
                Err(e) => out.push(Node::leaf(hop(), label.clone()).with_note(e.to_string())),
            },

            FieldRepr::Trailing {
                type_name,
                count,
                element_size,
            } => {
                let Some(n) = resolve_count(ty, count, image, base) else {
                    out.push(
                        Node::leaf(hop(), label.clone())
                            .with_note("the pack does not record how many there are"),
                    );
                    continue;
                };
                if n == 0 {
                    out.push(Node::leaf(hop(), format!("{label} (empty)")));
                    continue;
                }
                out.push(run(
                    pack,
                    &Pointee::Many {
                        type_name: type_name.clone(),
                        count: count.clone(),
                        element_size: *element_size,
                    },
                    image,
                    at,
                    n,
                    hop(),
                    label,
                    above,
                ));
            }

            FieldRepr::PtrSlots { pointee, count } => {
                let mut group = Node::leaf(hop(), format!("{label} ({count})"));
                for i in 0..*count {
                    group.children.push(slot(
                        pack,
                        pointee,
                        image,
                        at + i * POINTER_SIZE,
                        i,
                        above,
                    ));
                }
                out.push(group);
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn pointer_node(
    pack: &Pack,
    owner: &PackType,
    pointee: &Pointee,
    image: Image<'_>,
    owner_base: usize,
    at: usize,
    hop: Hop,
    label: &str,
    above: &Above<'_>,
) -> Option<Node> {
    match pointee {
        Pointee::Str | Pointee::Strings { .. } | Pointee::Bytes { .. } | Pointee::Opaque => None,
        // What a member pointer reaches is already this record's own subtree.
        Pointee::Member { .. } | Pointee::Members { .. } => None,
        Pointee::One { type_name } => match image.pointer_at(at) {
            Ok(0) => None,
            Ok(addr) => match image.resolve(addr) {
                Ok(target) => described(pack, type_name, image, target, hop, label, above),
                Err(e) => Some(Node::leaf(hop, label).with_note(e.to_string())),
            },
            Err(e) => Some(Node::leaf(hop, label).with_note(e.to_string())),
        },

        Pointee::Many { count, .. } | Pointee::ManyPtr { count, .. } => {
            let n = match resolve_count(owner, count, image, owner_base) {
                Some(n) => n,
                None => {
                    return Some(
                        Node::leaf(hop, label)
                            .with_note("the pack does not record how many there are"),
                    )
                }
            };
            if n == 0 {
                return Some(Node::leaf(hop, format!("{label} (empty)")));
            }
            match image.pointer_at(at) {
                Ok(0) => Some(Node::leaf(hop, format!("{label} (none)"))),
                Ok(addr) => match image.resolve(addr) {
                    Ok(start) => Some(run(pack, pointee, image, start, n, hop, label, above)),
                    Err(e) => Some(Node::leaf(hop, label).with_note(e.to_string())),
                },
                Err(e) => Some(Node::leaf(hop, label).with_note(e.to_string())),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    pack: &Pack,
    pointee: &Pointee,
    image: Image<'_>,
    start: usize,
    count: usize,
    hop: Hop,
    label: &str,
    above: &Above<'_>,
) -> Node {
    let mut group = Node::leaf(hop, format!("{label} ({count})"));
    let shown = count.min(MAX_ELEMENTS);
    let name = pointee_type(pointee);

    if matches!(pointee, Pointee::ManyPtr { .. }) {
        for i in 0..shown {
            group.children.push(slot_of(
                pack,
                name,
                image,
                start + i * POINTER_SIZE,
                i,
                above,
            ));
        }
        return elided(group, count);
    }

    let Some(name) = name else {
        return group;
    };
    let declared = match pointee {
        Pointee::Many { element_size, .. } => *element_size,
        _ => None,
    };
    let Some(stride) = pack.type_by_name(name).map(|t| t.size).or(declared) else {
        return group.with_note("the pack does not record how wide an element is");
    };
    if stride == 0 {
        return group.with_note("the pack records a zero-width element");
    }
    for i in 0..shown {
        if let Some(node) = described(
            pack,
            name,
            image,
            start + i * stride,
            Hop::Index(i),
            &format!("[{i}]"),
            above,
        ) {
            group.children.push(node);
        }
    }
    elided(group, count)
}

fn slot(
    pack: &Pack,
    pointee: &Pointee,
    image: Image<'_>,
    at: usize,
    index: usize,
    above: &Above<'_>,
) -> Node {
    slot_of(pack, pointee_type(pointee), image, at, index, above)
}

fn slot_of(
    pack: &Pack,
    type_name: Option<&str>,
    image: Image<'_>,
    at: usize,
    index: usize,
    above: &Above<'_>,
) -> Node {
    let hop = Hop::Index(index);
    match image.pointer_at(at) {
        Ok(0) => Node::leaf(hop, format!("[{index}] (empty)")),
        Ok(addr) => match (type_name, image.resolve(addr)) {
            (Some(name), Ok(target)) => described(
                pack,
                name,
                image,
                target,
                hop.clone(),
                &format!("[{index}]"),
                above,
            )
            .unwrap_or_else(|| Node::leaf(hop, format!("[{index}]"))),
            (None, Ok(_)) => Node::leaf(hop, format!("[{index}]")),
            (_, Err(e)) => Node::leaf(hop, format!("[{index}]")).with_note(e.to_string()),
        },
        Err(e) => Node::leaf(hop, format!("[{index}]")).with_note(e.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn described(
    pack: &Pack,
    type_name: &str,
    image: Image<'_>,
    at: usize,
    hop: Hop,
    label: &str,
    above: &Above<'_>,
) -> Option<Node> {
    let ty = concrete(pack, pack.type_by_name(type_name)?, image, at);
    if above.holds(&ty.name, at) {
        return None;
    }
    let here = Above {
        depth: above.depth + 1,
        limit: above.limit,
        part: (&ty.name, at),
        parent: Some(above),
    };
    let children = children_of(pack, ty, image, at, &here);
    let truncated = children.is_empty() && here.depth >= here.limit;
    let mut node = Node {
        label: label.to_string(),
        hop: Some(hop),
        type_name: Some(ty.name.clone()),
        at,
        children,
        note: None,
        cut: None,
    };
    if truncated {
        node.note = Some(format!("{} levels deep; not followed further", here.limit));
        node.cut = leads_on(ty).then_some(Cut::Depth);
    }
    Some(node)
}

/// Whether the type has a field that becomes a node, which the depth limit then leaves out.
fn leads_on(ty: &PackType) -> bool {
    ty.fields.iter().any(|field| {
        matches!(
            field.repr,
            FieldRepr::Inline { .. }
                | FieldRepr::InlineArray { .. }
                | FieldRepr::AtArray(_)
                | FieldRepr::Trailing { .. }
                | FieldRepr::PtrSlots { .. }
                | FieldRepr::Ptr(
                    Pointee::One { .. } | Pointee::Many { .. } | Pointee::ManyPtr { .. }
                )
        )
    })
}

fn elided(mut group: Node, total: usize) -> Node {
    if total > MAX_ELEMENTS {
        group.note = Some(format!("showing {MAX_ELEMENTS} of {total}",));
        group.cut = Some(Cut::Elements(total));
    }
    group
}

fn pointee_type(pointee: &Pointee) -> Option<&str> {
    match pointee {
        Pointee::One { type_name }
        | Pointee::Member { type_name }
        | Pointee::Members { type_name, .. } => Some(type_name),
        Pointee::Many { type_name, .. } | Pointee::ManyPtr { type_name, .. } => {
            type_name.as_deref()
        }
        Pointee::Str | Pointee::Strings { .. } | Pointee::Bytes { .. } | Pointee::Opaque => None,
    }
}

/// How many a `count` of a field of the `owner` at `owner_base` states; `None` where the pack does not record it.
pub fn resolve_count(
    owner: &PackType,
    count: &CountSource,
    image: Image<'_>,
    owner_base: usize,
) -> Option<usize> {
    let read = |name: &str| -> Option<i128> {
        let field = owner.field_by_name(name)?;
        let width = match field.repr {
            FieldRepr::Scalar(or_io::desc::Scalar::Int(w)) => w,
            FieldRepr::Enum { width, .. } => width,
            _ => return None,
        };
        width.read(image.bytes(), owner_base + field.offset)
    };
    let sane = |v: i128| usize::try_from(v).ok();

    match count {
        CountSource::Field(name) => read(name).and_then(sane),
        CountSource::CountAndCapacity { count, .. } => read(count).and_then(sane),
        CountSource::MaskedField { field, mask } => {
            read(field).and_then(|v| sane(v & i128::from(*mask)))
        }
        CountSource::Product { count, stride } => {
            let (count, stride) = (read(count)?, read(stride)?);
            sane(count.checked_mul(stride)?)
        }
        CountSource::PrefixedAboveOne { count } => read(count)
            .and_then(sane)
            .map(CountSource::prefixed_above_one),
        CountSource::Sum { fields } => fields
            .iter()
            .map(|f| read(f).and_then(sane))
            .try_fold(0usize, |total, n| total.checked_add(n?)),
        CountSource::MaskedOrMipChain { .. } => {
            let raws = count
                .reads_fields()
                .into_iter()
                .map(read)
                .collect::<Option<Vec<_>>>()?;
            count.length_from(&raws)
        }
        CountSource::Fixed(n) => Some(*n),
        CountSource::Inline | CountSource::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::desc::{AddressSpace, IntWidth, PackField, PackSelector, Scalar};
    fn addr(at: usize) -> u64 {
        0x5000_0000 | at as u64
    }

    fn field(name: &str, offset: usize, repr: FieldRepr) -> PackField {
        let space = matches!(
            repr,
            FieldRepr::Ptr(_) | FieldRepr::PtrSlots { .. } | FieldRepr::AtArray(_)
        )
        .then_some(AddressSpace::Virtual);
        PackField {
            name: name.into(),
            offset,
            repr,
            space,
            bits: Vec::new(),
            note: None,
        }
    }

    fn elem_type() -> PackType {
        PackType {
            name: "Elem".into(),
            size: 8,
            fields: vec![field(
                "id",
                0,
                FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
            )],
            selector: None,
        }
    }

    fn pack_of(root: PackType) -> Pack {
        let mut pack = Pack::new("t", "g");
        pack.types.push(root);
        pack.types.push(elem_type());
        pack
    }

    fn buffer() -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[0x00..0x04].copy_from_slice(&2u32.to_le_bytes());
        b[0x08..0x10].copy_from_slice(&addr(0x40).to_le_bytes());
        b[0x10..0x18].copy_from_slice(&addr(0x80).to_le_bytes());

        b[0x40..0x44].copy_from_slice(&11u32.to_le_bytes());
        b[0x48..0x4C].copy_from_slice(&22u32.to_le_bytes());

        b[0x80..0x88].copy_from_slice(&addr(0xC0).to_le_bytes());
        b[0x88..0x90].copy_from_slice(&addr(0xC8).to_le_bytes());

        b[0xC0..0xC4].copy_from_slice(&33u32.to_le_bytes());
        b[0xC8..0xCC].copy_from_slice(&44u32.to_le_bytes());
        b
    }

    fn root_with_both_runs() -> PackType {
        PackType {
            name: "Root".into(),
            size: 0x20,
            fields: vec![
                field("n", 0x00, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
                field(
                    "values",
                    0x08,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: Some("Elem".into()),
                        count: CountSource::Field("n".into()),
                        element_size: None,
                    }),
                ),
                field(
                    "slots",
                    0x10,
                    FieldRepr::Ptr(Pointee::ManyPtr {
                        type_name: Some("Elem".into()),
                        count: CountSource::Field("n".into()),
                    }),
                ),
            ],
            selector: None,
        }
    }

    #[test]
    fn a_run_of_values_and_a_run_of_pointers_are_not_walked_the_same_way() {
        let pack = pack_of(root_with_both_runs());
        let bytes = buffer();
        let image = Image::new(&bytes, bytes.len());

        let outline = Outline::of(&pack, "Root", image, 0).expect("described");
        assert_eq!(
            outline.root.children.len(),
            2,
            "one node per run, no scalars"
        );

        let values = &outline.root.children[0];
        let slots = &outline.root.children[1];

        let offsets = |n: &Node| n.children.iter().map(|c| c.at).collect::<Vec<_>>();

        assert_eq!(offsets(values), vec![0x40, 0x48]);

        assert_eq!(offsets(slots), vec![0xC0, 0xC8]);
    }

    #[test]
    fn scalars_are_never_nodes() {
        let pack = pack_of(PackType {
            name: "Root".into(),
            size: 0x20,
            fields: vec![
                field("n", 0x00, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
                field("flag", 0x04, FieldRepr::Scalar(Scalar::Bool)),
                field(
                    "verts",
                    0x08,
                    FieldRepr::ScalarArray {
                        element: Scalar::F32,
                        count: 4,
                    },
                ),
            ],
            selector: None,
        });
        let bytes = buffer();
        let image = Image::new(&bytes, bytes.len());

        let outline = Outline::of(&pack, "Root", image, 0).expect("described");
        assert!(outline.root.children.is_empty());
        assert_eq!(outline.root.type_name.as_deref(), Some("Root"));
    }

    #[test]
    fn a_null_slot_keeps_its_index() {
        let pack = pack_of(PackType {
            name: "Root".into(),
            size: 0x20,
            fields: vec![field(
                "lods",
                0x00,
                FieldRepr::PtrSlots {
                    pointee: Pointee::One {
                        type_name: "Elem".into(),
                    },
                    count: 4,
                },
            )],
            selector: None,
        });

        let mut bytes = vec![0u8; 0x100];
        bytes[0x08..0x10].copy_from_slice(&addr(0xC0).to_le_bytes());
        bytes[0x18..0x20].copy_from_slice(&addr(0xC8).to_le_bytes());
        let image = Image::new(&bytes, bytes.len());

        let outline = Outline::of(&pack, "Root", image, 0).expect("described");
        let lods = &outline.root.children[0];

        assert_eq!(lods.children.len(), 4, "four slots, holes included");
        let labels: Vec<&str> = lods.children.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["[0] (empty)", "[1]", "[2] (empty)", "[3]"]);

        assert_eq!(lods.children[1].at, 0xC0);
        assert_eq!(lods.children[3].at, 0xC8);
    }

    #[test]
    fn an_unrecorded_count_is_reported_rather_than_guessed() {
        let pack = pack_of(PackType {
            name: "Root".into(),
            size: 0x20,
            fields: vec![field(
                "values",
                0x08,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Elem".into()),
                    count: CountSource::Unknown,
                    element_size: None,
                }),
            )],
            selector: None,
        });
        let bytes = buffer();
        let image = Image::new(&bytes, bytes.len());

        let outline = Outline::of(&pack, "Root", image, 0).expect("described");
        let values = &outline.root.children[0];
        assert!(values.children.is_empty());
        assert!(
            values
                .note
                .as_deref()
                .is_some_and(|n| n.contains("how many")),
            "the reason should be on the node: {:?}",
            values.note
        );
    }

    #[test]
    fn a_path_resolves_to_the_node_the_walk_built() {
        let pack = pack_of(root_with_both_runs());
        let bytes = buffer();
        let image = Image::new(&bytes, bytes.len());
        let outline = Outline::of(&pack, "Root", image, 0).expect("described");

        for (field, expected) in [("values", [0x40, 0x48]), ("slots", [0xC0, 0xC8])] {
            for (i, want) in expected.iter().enumerate() {
                let path = PackPath::default()
                    .then(Hop::Field(field.into()))
                    .then(Hop::Index(i));
                let node = outline
                    .find(&path)
                    .unwrap_or_else(|| panic!("{path} should resolve"));
                assert_eq!(node.at, *want, "{path} landed in the wrong place");
                assert_eq!(node.type_name.as_deref(), Some("Elem"));
            }
        }
    }

    #[test]
    fn a_path_prints_as_a_route() {
        let path = PackPath::default()
            .then(Hop::Field("lod_group".into()))
            .then(Hop::Field("lods".into()))
            .then(Hop::Index(2));
        assert_eq!(path.to_string(), "lod_group > lods > [2]");
        assert_eq!(PackPath::default().to_string(), "(root)");
        assert!(PackPath::default().is_root());
    }

    #[test]
    fn a_path_the_walk_did_not_build_resolves_to_nothing() {
        let pack = pack_of(root_with_both_runs());
        let bytes = buffer();
        let image = Image::new(&bytes, bytes.len());
        let outline = Outline::of(&pack, "Root", image, 0).expect("described");

        let past_the_end = PackPath::default()
            .then(Hop::Field("slots".into()))
            .then(Hop::Index(7));
        assert!(outline.find(&past_the_end).is_none());

        let no_such_field = PackPath::default().then(Hop::Field("nope".into()));
        assert!(outline.find(&no_such_field).is_none());
    }

    #[test]
    fn an_empty_slot_still_has_a_path() {
        let pack = pack_of(PackType {
            name: "Root".into(),
            size: 0x20,
            fields: vec![field(
                "lods",
                0x00,
                FieldRepr::PtrSlots {
                    pointee: Pointee::One {
                        type_name: "Elem".into(),
                    },
                    count: 4,
                },
            )],
            selector: None,
        });
        let mut bytes = vec![0u8; 0x100];
        bytes[0x18..0x20].copy_from_slice(&addr(0xC8).to_le_bytes());
        let image = Image::new(&bytes, bytes.len());
        let outline = Outline::of(&pack, "Root", image, 0).expect("described");

        let hole = PackPath::default()
            .then(Hop::Field("lods".into()))
            .then(Hop::Index(2));
        let node = outline.find(&hole).expect("an empty slot is still a node");
        assert_eq!(node.label, "[2] (empty)");
        assert!(node.type_name.is_none());

        let filled = PackPath::default()
            .then(Hop::Field("lods".into()))
            .then(Hop::Index(3));
        assert_eq!(outline.find(&filled).expect("filled").at, 0xC8);
    }

    #[test]
    fn an_undescribed_type_declines_rather_than_inventing_an_empty_outline() {
        let pack = pack_of(root_with_both_runs());
        let bytes = buffer();
        let image = Image::new(&bytes, bytes.len());
        assert!(Outline::of(&pack, "NotInThePack", image, 0).is_none());
    }

    #[test]
    fn a_run_past_the_limit_is_cut_and_says_how_many_it_holds() {
        let pack = pack_of(root_with_both_runs());
        let held = MAX_ELEMENTS + 22;
        let mut bytes = vec![0u8; 0x600];
        bytes[0x00..0x04].copy_from_slice(&(held as u32).to_le_bytes());
        bytes[0x08..0x10].copy_from_slice(&addr(0x40).to_le_bytes());
        let image = Image::new(&bytes, bytes.len());

        let outline = Outline::of(&pack, "Root", image, 0).expect("described");
        let values = &outline.root.children[0];
        assert_eq!(values.children.len(), MAX_ELEMENTS);
        assert_eq!(values.cut, Some(Cut::Elements(held)));
        assert_eq!(
            values.note.as_deref(),
            Some(format!("showing {MAX_ELEMENTS} of {held}").as_str())
        );

        let within = buffer();
        let image = Image::new(&within, within.len());
        let outline = Outline::of(&pack, "Root", image, 0).expect("described");
        assert_eq!(
            outline.root.children[0].cut, None,
            "two elements are all shown"
        );
    }

    /// A `Root` and `links` parts under it, each pointing to the next, the last pointing nowhere or, where `plain`, holding a scalar only.
    fn chain(links: usize, plain: bool) -> (Pack, Vec<u8>) {
        let to = |name: String| {
            let repr = FieldRepr::Ptr(Pointee::One { type_name: name });
            vec![field("next", 0, repr)]
        };
        let mut pack = Pack::new("t", "g");
        for k in 0..=links {
            let fields = match (k == links, plain) {
                (true, true) => elem_type().fields,
                (true, false) => to(format!("L{k}")),
                (false, _) => to(format!("L{}", k + 1)),
            };
            let name = if k == 0 {
                "Root".into()
            } else {
                format!("L{k}")
            };
            pack.types.push(PackType {
                name,
                size: 8,
                fields,
                selector: None,
            });
        }
        let mut bytes = vec![0u8; 0x10 + 8 * links];
        bytes[0..8].copy_from_slice(&addr(0x10).to_le_bytes());
        for k in 0..links.saturating_sub(1) {
            let at = 0x10 + 8 * k;
            bytes[at..at + 8].copy_from_slice(&addr(at + 8).to_le_bytes());
        }
        (pack, bytes)
    }

    #[test]
    fn a_part_at_the_depth_limit_that_leads_on_is_cut_and_one_that_ends_is_not() {
        let last = |links: usize, plain: bool| {
            let (pack, bytes) = chain(links, plain);
            let image = Image::new(&bytes, bytes.len());
            let outline = Outline::of(&pack, "Root", image, 0).expect("described");
            let (mut node, mut levels) = (&outline.root, 0);
            while let Some(next) = node.children.first() {
                (node, levels) = (next, levels + 1);
            }
            (levels, node.cut)
        };
        assert_eq!(last(MAX_DEPTH + 4, false), (MAX_DEPTH, Some(Cut::Depth)));
        assert_eq!(
            last(MAX_DEPTH, true),
            (MAX_DEPTH, None),
            "nothing lies below a scalar"
        );
        assert_eq!(
            last(3, false),
            (3, None),
            "above the limit nothing is left out"
        );
    }

    #[test]
    fn an_outline_asked_for_more_levels_reads_them_and_says_where_it_stopped() {
        let (pack, bytes) = chain(12, false);
        let image = Image::new(&bytes, bytes.len());
        let last = |depth: usize| {
            let outline = Outline::of_to(&pack, "Root", image, 0, depth).expect("described");
            let (mut node, mut levels) = (&outline.root, 0);
            while let Some(next) = node.children.first() {
                (node, levels) = (next, levels + 1);
            }
            (levels, node.cut, node.note.clone())
        };
        assert_eq!(
            last(9),
            (
                9,
                Some(Cut::Depth),
                Some("9 levels deep; not followed further".to_string())
            )
        );
        assert_eq!(
            last(20),
            (12, None, None),
            "the chain ends before the limit"
        );
    }

    #[test]
    fn a_tagged_base_that_resolves_to_the_part_holding_it_is_not_a_part_under_itself() {
        let tag = |name: &str, repr: FieldRepr, offset: usize| field(name, offset, repr);
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: "Bound".into(),
            size: 8,
            fields: vec![tag("kind", FieldRepr::Scalar(Scalar::Int(IntWidth::U8)), 0)],
            selector: Some(PackSelector {
                tag: "kind".into(),
                tag_bits: None,
                variants: vec![(1, "Geometry".into())],
            }),
        });
        pack.types.push(PackType {
            name: "Geometry".into(),
            size: 16,
            fields: vec![
                tag(
                    "base",
                    FieldRepr::Inline {
                        type_name: "Bound".into(),
                    },
                    0,
                ),
                tag("count", FieldRepr::Scalar(Scalar::Int(IntWidth::U32)), 8),
            ],
            selector: None,
        });
        let mut bytes = vec![0u8; 0x20];
        bytes[0] = 1;
        let image = Image::new(&bytes, bytes.len());

        let outline = Outline::of(&pack, "Bound", image, 0).expect("described");
        assert_eq!(outline.root.type_name.as_deref(), Some("Geometry"));
        assert!(
            outline.root.children.is_empty(),
            "the base is the geometry again, at its own address: {:?}",
            outline.root.children
        );
    }
}
