//! The owned form of a declaration: ordinary Rust values (`String`, `Vec`,
//! `Option<Box<_>>`) converted to and from the record tree, so a file is
//! opened into a plain struct and saved through the canonical writer.

use std::any::{type_name, Any};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::desc::plan::{Plans, TypePlan};
use crate::desc::record::{Record, RecordRun};
use crate::desc::value::root_address;
use crate::desc::{FieldRepr, PackType, Scalar, Value};
use crate::error::Error;

use super::file::Origin;
use super::prim::{
    Array, Bool8, Count, Hash32, MemberPtr, Open, PhysPtr, Ptr, Str, Vec2, Vec3, Vec4,
};
use super::{Described, Elem, Element, Opaque, OpenEnum, Plain};

/// A tree whose value at `at` is not the shape `what` is held as.
fn shape(what: &str, at: usize, expected: &'static str, found: Option<&Value>) -> Error {
    Error::Shape {
        what: what.into(),
        at,
        expected,
        found: match found {
            None => "nothing",
            Some(v) => shape_of(v),
        },
    }
}

fn shape_of(v: &Value) -> &'static str {
    match v {
        Value::Scalar(_) => "a scalar",
        Value::Enum { .. } => "an enum",
        Value::Struct(_) => "a struct",
        Value::Records(_) => "a run of structs",
        Value::Blob { .. } => "a codec's blob",
        Value::Array { .. } => "an array",
        Value::Bytes(_) => "bytes",
        Value::Ptr { target: None, .. } => "a null pointer",
        Value::Ptr { .. } => "a pointer",
        Value::Alias(_) => "a pointer at a shared target",
        Value::Member { .. } => "a pointer at a member",
        Value::Opaque(_) => "opaque bytes",
        Value::UndescribedPtr(_) => "an unfollowed pointer",
        Value::NotAnAddress(_) => "eight bytes that are not an address",
    }
}

/// The children of the record a value sits in, keyed by offset.
pub type Kids<'a> = &'a [(u32, Value)];

fn kid(kids: Kids<'_>, at: usize) -> Option<&Value> {
    kids.binary_search_by_key(&(at as u32), |(o, _)| *o)
        .ok()
        .map(|i| &kids[i].1)
}

/// The records a value being read or built sits inside, innermost last, each
/// with the address that stands for it; zero for one no pointer names.
#[derive(Default)]
struct Enclosing {
    stack: RefCell<Vec<(&'static str, u64)>>,
    /// The address of the target the pointer being followed holds, for the record it enters next.
    pending: Cell<u64>,
}

impl Enclosing {
    fn enter(&self, type_name: &'static str) -> Entered<'_> {
        let mut stack = self.stack.borrow_mut();
        let source = match self.pending.replace(0) {
            0 if stack.is_empty() => root_address(0),
            source => source,
        };
        stack.push((type_name, source));
        Entered(&self.stack)
    }

    /// The address standing for the record entered last; zero for one no pointer names.
    fn current(&self) -> u64 {
        self.stack.borrow().last().map_or(0, |(_, s)| *s)
    }

    fn innermost(&self, type_name: &str) -> Option<u64> {
        self.stack
            .borrow()
            .iter()
            .rev()
            .find(|(t, _)| *t == type_name)
            .map(|(_, s)| *s)
            .filter(|s| *s != 0)
    }

    /// `f` with `source` standing for the record it enters first.
    fn through<R>(&self, source: u64, f: impl FnOnce() -> R) -> R {
        self.pending.set(source);
        let out = f();
        self.pending.set(0);
        out
    }
}

/// A record entered, left when this drops.
#[doc(hidden)]
pub struct Entered<'a>(&'a RefCell<Vec<(&'static str, u64)>>);

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().pop();
    }
}

/// What reading an owned value from a tree needs besides the value in hand:
/// the records it sits inside, and where each address the tree owns leads.
pub struct Rx<'t> {
    root: &'t Value,
    targets: OnceCell<HashMap<u64, &'t Value>>,
    enclosing: Enclosing,
    /// Per address a `#[keyed_by]` pointer holds, the key its entry states.
    owner_keys: RefCell<HashMap<u64, u32>>,
    /// A reference by key found no key yet, and the read is to run again.
    defer: Cell<bool>,
    missed: Cell<bool>,
    /// Each address a `#[shares]` pointer owns in the file and leaves to the pointer it shares with.
    lent: RefCell<HashSet<u64>>,
    places: OnceCell<Places<'t>>,
    /// Each alias slot of the tree a field reads by value, as a copy of what its owner holds.
    copies: RefCell<HashSet<*const Value>>,
    /// How deep the read is inside a copy read by value, whose records take no anchor.
    copying: Cell<u32>,
    /// Each address an [`OwnedOrKeyed`] reached first and owns, so a `#[keyed_by]` entry aliasing it reads a copy.
    claimed: RefCell<HashSet<u64>>,
}

impl<'t> Rx<'t> {
    /// A read of the tree whose root is `root`, which stands at `root_address(0)`.
    pub fn new(root: &'t Value) -> Self {
        Self {
            root,
            targets: OnceCell::new(),
            enclosing: Enclosing::default(),
            owner_keys: RefCell::default(),
            defer: Cell::new(false),
            missed: Cell::new(false),
            lent: RefCell::default(),
            places: OnceCell::new(),
            copies: RefCell::default(),
            copying: Cell::new(0),
            claimed: RefCell::default(),
        }
    }

    #[doc(hidden)]
    pub fn enter(&self, type_name: &'static str) -> Entered<'_> {
        self.enclosing.enter(type_name)
    }

    /// Record that the pointer at `at` owns a target its entry keys `key`.
    #[doc(hidden)]
    pub fn __own_key(&self, kids: Kids<'_>, at: usize, key: u32) {
        let address = match kid(kids, at) {
            Some(Value::Ptr {
                source,
                target: Some(_),
            }) => *source,
            Some(Value::Alias(a)) => *a,
            _ => return,
        };
        if address != 0 {
            self.owner_keys.borrow_mut().insert(address, key);
        }
    }

    /// Record that the run of pointers at `at` owns each target under the key `keys` gives it in turn.
    #[doc(hidden)]
    pub fn __own_keys(&self, kids: Kids<'_>, at: usize, keys: impl IntoIterator<Item = u32>) {
        let Some(Value::Ptr {
            target: Some(run), ..
        }) = kid(kids, at)
        else {
            return;
        };
        let Value::Array { elements, .. } = run.as_ref() else {
            return;
        };
        let mut owner_keys = self.owner_keys.borrow_mut();
        for (element, key) in elements.iter().zip(keys) {
            if let Some(address) = address_of(element) {
                owner_keys.insert(address, key);
            }
        }
    }

    /// Whether a `#[shares]` pointer left the target at `address` to the pointer it shares with.
    fn is_lent(&self, address: u64) -> bool {
        self.lent.borrow().contains(&address)
    }

    /// The key the map entry owning the target at `address` states.
    fn owner_key(&self, address: u64) -> Option<u32> {
        self.owner_keys.borrow().get(&address).copied()
    }

    /// The target some pointer in the tree owns at `address`.
    fn target(&self, address: u64) -> Option<&'t Value> {
        self.targets
            .get_or_init(|| {
                let mut out = owned_targets(self.root);
                out.insert(root_address(0), self.root);
                out
            })
            .get(&address)
            .copied()
    }

    /// The path of the pointer owning what the alias `v` points at, which marks `v` read by value.
    fn copy_of(&self, v: &Value, address: u64, at: usize) -> Result<String, Error> {
        let around = self
            .enclosing
            .stack
            .borrow()
            .iter()
            .any(|(_, s)| *s == address);
        let places = self.places.get_or_init(|| Places::of(self.root));
        let Some(path) = places.owned.get(&address).filter(|_| !around) else {
            return Err(Error::Invalid {
                what: "an alias".into(),
                why: match around {
                    true => format!("the pointer at {at:#x} points back at a record around it"),
                    false => format!(
                        "the pointer at {at:#x} points at {address:#x}, which no pointer in the tree owns"
                    ),
                },
            });
        };
        self.copies.borrow_mut().insert(v as *const Value);
        Ok(path.clone())
    }

    /// Mark `value` as the record the file held at `address`, unless it is part of a copy read by value.
    ///
    /// ⚠ A copy holds its owner's addresses, so anchoring it would let a
    /// [`Ref`] to a removed owner follow onto the copy instead of being refused.
    fn anchor<T: Owned>(&self, value: &mut T, address: u64) {
        if self.copying.get() == 0 {
            value.__anchor(address);
        }
    }

    /// `f`, read as part of a copy by value.
    fn as_copy<R>(&self, f: impl FnOnce() -> R) -> R {
        self.copying.set(self.copying.get() + 1);
        let out = f();
        self.copying.set(self.copying.get() - 1);
        out
    }

    /// Whether a field reads the alias `v` by value.
    fn is_copy(&self, v: &Value) -> bool {
        self.copies.borrow().contains(&(v as *const Value))
    }

    /// The path a [`Ref`] names the target of `v` by, where another pointer in the tree owns it.
    fn referred(&self, v: Option<&Value>, at: usize) -> Result<Option<RefPath>, Error> {
        let places = self.places.get_or_init(|| Places::of(self.root));
        let (path, anchor) = match v {
            Some(Value::Alias(a)) if !self.is_lent(*a) => {
                let found = places
                    .runs
                    .get(a)
                    .map(|(run, ..)| (format!("{run}[0]"), *a))
                    // Another reference's copy of an element past the first.
                    .or_else(|| self.target(*a).and_then(|t| places.copied(t)))
                    .or_else(|| {
                        let record = matches!(self.target(*a), Some(Value::Struct(_)));
                        let path = places.owned.get(a)?;
                        Some((path.clone(), if record { *a } else { 0 }))
                    })
                    .or_else(|| places.element(*a).map(|path| (path, *a)));
                match found {
                    Some(found) => found,
                    None => {
                        return Err(Error::Invalid {
                            what: "a reference".into(),
                            why: format!(
                                "the pointer at {at:#x} points at {a:#x}, which no pointer in the tree owns"
                            ),
                        })
                    }
                }
            }
            // An element of a run elsewhere, which the tree reads as a copy.
            Some(Value::Ptr {
                source,
                target: Some(t),
            }) => match places
                .element(*source)
                .map(|path| (path, *source))
                .or_else(|| places.copied(t))
            {
                Some(found) => found,
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        // A path through no run names what no edit can move.
        let anchor = if path.contains('[') { anchor } else { 0 };
        Ok(Some(RefPath { path, anchor }))
    }
}

/// Every target a pointer in the tree `v` owns, by its address.
fn owned_targets(v: &Value) -> HashMap<u64, &Value> {
    fn walk<'t>(v: &'t Value, out: &mut HashMap<u64, &'t Value>) {
        match v {
            Value::Ptr {
                source,
                target: Some(t),
            } => {
                if *source != 0 {
                    out.insert(*source, t.as_ref());
                }
                walk(t, out);
            }
            Value::Struct(r) => r.children().iter().for_each(|(_, c)| walk(c, out)),
            Value::Array { elements, .. } => elements.iter().for_each(|e| walk(e, out)),
            Value::Blob { parts, .. } => parts.iter().for_each(|(_, p)| walk(p, out)),
            _ => {}
        }
    }
    let mut out = HashMap::new();
    walk(v, &mut out);
    out
}

/// Whether `a` holds what `b` does, each address `a` names standing for the one `b` names in its place.
fn same(a: &Value, b: &Value, names: &mut HashMap<u64, u64>) -> bool {
    let mut pair = |x: u64, y: u64| x == 0 || y == 0 || *names.entry(x).or_insert(y) == y;
    match (a, b) {
        (
            Value::Ptr {
                source: x,
                target: Some(s),
            },
            Value::Ptr {
                source: y,
                target: Some(t),
            },
        ) => pair(*x, *y) && same(s, t, names),
        (Value::Alias(x), Value::Alias(y)) => pair(*x, *y),
        (Value::Struct(r), Value::Struct(q)) => {
            r.type_name() == q.type_name()
                && r.bytes() == q.bytes()
                && r.children().len() == q.children().len()
                && r.children()
                    .iter()
                    .zip(q.children())
                    .all(|((o, c), (p, d))| o == p && same(c, d, names))
        }
        (
            Value::Array {
                elements: e,
                capacity: c,
            },
            Value::Array {
                elements: f,
                capacity: d,
            },
        ) => c == d && e.len() == f.len() && e.iter().zip(f).all(|(x, y)| same(x, y, names)),
        (Value::Blob { codec: c, parts: p }, Value::Blob { codec: d, parts: q }) => {
            c == d
                && p.len() == q.len()
                && p.iter()
                    .zip(q)
                    .all(|((m, x), (n, y))| m == n && same(x, y, names))
        }
        _ => a == b,
    }
}

/// Every target a pointer in a tree owns, by the path from the root a [`Ref`] names it with.
#[derive(Default)]
struct Places<'t> {
    /// Per address a pointer owns, that pointer's path.
    owned: HashMap<u64, String>,
    /// Per run of records a pointer owns, by its address: its path, stride and length.
    runs: BTreeMap<u64, (String, usize, usize)>,
    /// Per address a pointer in an element of a run of records owns, that element's path, record and address.
    inside: HashMap<u64, (String, &'t Record, u64)>,
}

/// The path of the child at `at` of a record planned by `plan`: the innermost field there.
fn slot_name(plan: &TypePlan, at: u32) -> &str {
    plan.fields()
        .iter()
        .rev()
        .find(|f| f.offset == at as usize)
        .map_or("?", |f| f.path.as_str())
}

fn joined(path: &str, name: &str) -> String {
    match path {
        "" => name.to_string(),
        _ => format!("{path}.{name}"),
    }
}

impl<'t> Places<'t> {
    fn of(root: &'t Value) -> Self {
        let mut places = Places::default();
        places.record(root, "");
        places
    }

    fn record(&mut self, v: &'t Value, path: &str) {
        let Value::Struct(r) = v else { return };
        for (at, child) in r.children() {
            self.slot(child, joined(path, slot_name(r.plan(), *at)));
        }
    }

    fn slot(&mut self, v: &'t Value, path: String) {
        match v {
            Value::Ptr {
                source,
                target: Some(t),
            } => {
                if *source != 0 {
                    self.owned.insert(*source, path.clone());
                }
                self.target(*source, t, path);
            }
            Value::Array { elements, .. } => {
                for (i, e) in elements.iter().enumerate() {
                    self.slot(e, format!("{path}[{i}]"));
                }
            }
            _ => {}
        }
    }

    fn target(&mut self, source: u64, t: &'t Value, path: String) {
        match t {
            Value::Struct(_) => self.record(t, &path),
            Value::Records(run) if source != 0 => {
                self.runs.insert(source, (path, run.stride(), run.len()));
            }
            Value::Array { elements, .. } => {
                for (i, e) in elements.iter().enumerate() {
                    let at = format!("{path}[{i}]");
                    let Value::Struct(r) = e else {
                        self.slot(e, at);
                        continue;
                    };
                    let address = match source {
                        0 => 0,
                        _ => source + (i * r.bytes().len()) as u64,
                    };
                    for (_, c) in r.children() {
                        if let Value::Ptr {
                            source,
                            target: Some(_),
                        } = c
                        {
                            self.inside.insert(*source, (at.clone(), r, address));
                        }
                    }
                    self.record(e, &at);
                }
                if let (Some(Value::Struct(r)), true) = (elements.first(), source != 0) {
                    self.runs
                        .insert(source, (path, r.bytes().len(), elements.len()));
                }
            }
            _ => {}
        }
    }

    /// The path of the element past the first of a run of records that starts at `address`.
    fn element(&self, address: u64) -> Option<String> {
        let (base, (run, stride, len)) = self.runs.range(..address).next_back()?;
        let stride = (*stride as u64).max(1);
        let i = (address - base) / stride;
        ((address - base).is_multiple_of(stride) && (i as usize) < *len)
            .then(|| format!("{run}[{i}]"))
    }

    /// The path and address of the element of a run that `t` copies: the bytes a build copies, with each pointer at what the element's points at.
    fn copied(&self, t: &Value) -> Option<(String, u64)> {
        let Value::Struct(copy) = t else { return None };
        let (path, element, address) = copy.children().iter().find_map(|(_, c)| match c {
            Value::Alias(a) => self.inside.get(a),
            _ => None,
        })?;
        fn copied(r: &Record) -> impl Iterator<Item = Option<&[u8]>> {
            let bytes = r.bytes();
            r.plan()
                .copy_ranges()
                .iter()
                .map(move |&(start, end)| bytes.get(start..end))
        }
        let same = copy.type_name() == element.type_name()
            && copied(copy).eq(copied(element))
            && copy.children().len() == element.children().len()
            && copy
                .children()
                .iter()
                .zip(element.children())
                .all(|((o, c), (p, e))| {
                    o == p
                        && match (address_of(c), address_of(e)) {
                            (None, None) => c == e,
                            (a, b) => a == b,
                        }
                });
        same.then(|| (path.clone(), *address))
    }
}

/// Whether a save may rewrite a count the file states against the run it describes, and a word that is not an address in a shared pointer slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Repair {
    /// Refuse the save, naming each such count, and write each such word back as the file had it.
    #[default]
    Refuse,
    /// Rewrite each count from its run where that drops and reinterprets no byte, and refuse otherwise; point each such slot at the run it shares.
    Lossless,
}

/// What building a tree needs: the description's plans, the records being
/// built around the value in hand, and the address standing for each keyed target.
pub struct Cx {
    plans: Arc<Plans>,
    repair: Repair,
    /// Each count the save rewrote from its run, as a user reads it.
    repaired: RefCell<Vec<String>>,
    next: Cell<u64>,
    enclosing: Enclosing,
    keyed: RefCell<KeyedSources>,
    /// The key the next pointer written owns its target under, set by a `#[keyed_by]` field.
    owner_key: Cell<Option<u32>>,
    /// Each `#[refs]` count to stamp once the tree is whole: the address of its record, its offset and width.
    refs: RefCell<Vec<(u64, usize, usize)>>,
    /// The path each [`Ref`] written so far names, by the stand-in its slot holds until the tree is whole.
    pending: RefCell<Vec<RefPath>>,
    /// Per stand-in of a slot read by value, the copy it holds, which a save writes apart once it differs from its owner.
    copies: RefCell<HashMap<usize, Value>>,
    /// Each record written that a file held at an address, by that address: the source of the pointer
    /// at it, or of the run holding it with its index there.
    anchors: RefCell<Vec<(u64, u64, Option<usize>)>>,
    /// Per key a `#[keyed_by]` entry read by value claimed, its address and the stand-in whose final place it names.
    renamed: RefCell<Vec<(u64, usize)>>,
}

/// Per (type, key), the address a reference by that key names, and who has claimed it.
type KeyedSources = HashMap<(&'static str, u32), KeyedSource>;

/// ⚠ Keys are scoped to the map owning them, so two maps may reuse one
/// (a clip's properties each key by name); only a reference by a key two
/// pointers own is refused, because it names neither.
struct KeyedSource {
    address: u64,
    owned: bool,
    referenced: bool,
    owned_twice: bool,
}

/// Where the addresses a build makes up begin: above any address a file holds.
const MADE_UP: u64 = 0x4F52_0000_0000_0000;

/// Where the stand-ins a build puts at a [`Ref`] begin: above any address a file holds or a build makes up.
const REFERRED: u64 = 0x5245_4600_0000_0000;

impl Cx {
    pub fn new(plans: Arc<Plans>) -> Self {
        Self {
            plans,
            repair: Repair::Refuse,
            repaired: RefCell::default(),
            next: Cell::new(MADE_UP),
            enclosing: Enclosing::default(),
            keyed: RefCell::default(),
            owner_key: Cell::new(None),
            refs: RefCell::default(),
            pending: RefCell::default(),
            copies: RefCell::default(),
            anchors: RefCell::default(),
            renamed: RefCell::default(),
        }
    }

    /// This context, saving under `repair`.
    pub fn repairing(mut self, repair: Repair) -> Self {
        self.repair = repair;
        self
    }

    pub fn repair(&self) -> Repair {
        self.repair
    }

    /// Note a count the save rewrites from its run.
    pub fn note_repair(&self, what: String) {
        self.repaired.borrow_mut().push(what);
    }

    /// Each count the save rewrote from its run, in the order it reached them.
    pub fn repairs(&self) -> Vec<String> {
        self.repaired.borrow().clone()
    }

    /// Note that the record the file held at `anchor` is written at `source`, element `index` of the run there if it has one.
    fn anchored(&self, anchor: u64, source: u64, index: Option<usize>) {
        if anchor != 0 {
            self.anchors.borrow_mut().push((anchor, source, index));
        }
    }

    #[doc(hidden)]
    pub fn enter(&self, type_name: &'static str) -> Entered<'_> {
        self.enclosing.enter(type_name)
    }

    /// `f`, whose pointer owns its target under `key`.
    #[doc(hidden)]
    pub fn __keyed_by<R>(&self, key: u32, f: impl FnOnce() -> R) -> R {
        self.owner_key.set(Some(key));
        let out = f();
        self.owner_key.set(None);
        out
    }

    /// Count the pointers at the record being built into its `width` bytes at `at`, once the tree is whole.
    #[doc(hidden)]
    pub fn __refs(&self, at: usize, width: usize) {
        let address = self.enclosing.current();
        if address != 0 {
            self.refs.borrow_mut().push((address, at, width));
        }
    }

    /// `value`, and for a root, each [`Ref`] pointed at its target and every `#[refs]` count its tree implies stamped in.
    #[doc(hidden)]
    pub fn __settle(&self, mut value: Value) -> Result<Value, Error> {
        if !self.enclosing.stack.borrow().is_empty() {
            return Ok(value);
        }
        if !self.pending.borrow().is_empty() {
            self.resolve(&mut value)?;
        }
        if self.refs.borrow().is_empty() {
            return Ok(value);
        }
        let mut wanted: HashMap<u64, Vec<(usize, usize)>> = HashMap::new();
        for (address, at, width) in self.refs.take() {
            wanted.entry(address).or_default().push((at, width));
        }
        let mut counts = HashMap::new();
        count_pointers(&value, &mut counts);
        stamp_counts(&mut value, &wanted, &counts)?;
        Ok(value)
    }

    /// An address standing for one target of this build.
    fn fresh(&self) -> u64 {
        let n = self.next.get();
        self.next.set(n + 1);
        n
    }

    /// A stand-in for the pointer a [`Ref`] naming `path` holds, until the tree is whole.
    fn refer(&self, path: &RefPath) -> Value {
        let mut pending = self.pending.borrow_mut();
        pending.push(path.clone());
        Value::Alias(REFERRED + (pending.len() - 1) as u64)
    }

    /// A stand-in for the slot holding `p`, read as a copy of what the pointer at `path` owns, until the tree is whole.
    ///
    /// ⚠ The copy is built with keys of its own, so a keyed target inside it
    /// neither claims nor answers a key its owner's twin holds.
    ///
    /// A `#[keyed_by]` entry's own key is the exception: it names wherever the stand-in lands.
    fn by_value<P: Owned>(&self, path: &str, p: &P) -> Result<Value, Error> {
        self.copied(path, || p.to_element(self))
    }

    /// [`Cx::by_value`] for the slot `build` writes.
    fn copied(
        &self,
        path: &str,
        build: impl FnOnce() -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        let entry = self.owner_key.get();
        let outer = self.keyed.take();
        let copy = build();
        let inner = self.keyed.replace(outer);
        let copy = copy?;
        let id = self.pending.borrow().len();
        if let (Some(key), Value::Ptr { source, .. }) = (entry, &copy) {
            let claimed = inner
                .iter()
                .find(|((_, k), s)| *k == key && s.owned && s.address == *source);
            if let Some((&(type_name, _), _)) = claimed {
                let named = self.keyed(type_name, key, true)?;
                self.renamed.borrow_mut().push((named, id));
            }
        }
        self.copies.borrow_mut().insert(id, copy);
        Ok(self.refer(&path.into()))
    }

    /// The path each of `pending` names in `root`: where its target now is, for one opened from a file.
    ///
    /// ⚠ A target the file held is found by the address it carries, so a
    /// reference follows it through edits to the runs around it, and one whose
    /// target was removed is refused rather than moved onto whatever took its place.
    fn followed(&self, root: &Value, pending: Vec<RefPath>) -> Result<Vec<String>, Error> {
        if pending.iter().all(|p| p.anchor == 0) {
            return Ok(pending.into_iter().map(|p| p.path).collect());
        }
        let mut now: HashMap<u64, (u64, Option<usize>)> = HashMap::new();
        for &(anchor, source, index) in self.anchors.borrow().iter() {
            now.entry(anchor).or_insert((source, index));
        }
        let places = Places::of(root);
        pending
            .into_iter()
            .map(|p| {
                if p.anchor == 0 {
                    return Ok(p.path);
                }
                let Some((source, index)) = now.get(&p.anchor) else {
                    return Err(Error::Invalid {
                        what: "a reference".into(),
                        why: format!("it names `{}`, whose target was removed", p.path),
                    });
                };
                Ok(match (places.owned.get(source), index) {
                    (Some(run), Some(i)) => format!("{run}[{i}]"),
                    (Some(path), None) => path.clone(),
                    (None, _) => p.path,
                })
            })
            .collect()
    }

    /// Point each [`Ref`] stand-in in `root` where the canonical writer lays its target out.
    fn resolve(&self, root: &mut Value) -> Result<(), Error> {
        let pending = self.followed(root, self.pending.take())?;
        let mut wanted: HashMap<&str, Vec<usize>> = HashMap::new();
        for (id, path) in pending.iter().enumerate() {
            wanted.entry(path).or_default().push(id);
        }
        let mut elements: HashMap<&str, Vec<usize>> = HashMap::new();
        for path in wanted.keys() {
            let Some((run, i)) = path.strip_suffix(']').and_then(|p| p.rsplit_once('[')) else {
                continue;
            };
            if let Ok(i) = i.parse() {
                elements.entry(run).or_default().push(i);
            }
        }
        let mut aims = Aims {
            cx: self,
            wanted: &wanted,
            elements,
            found: vec![None; pending.len()],
        };
        aims.record(root, "")?;
        let copies = self.copies.take();
        let owned = match copies.is_empty() {
            true => HashMap::new(),
            false => owned_targets(root),
        };
        let finals = (0..pending.len())
            .map(|id| match copies.get(&id) {
                Some(copy) => Ok(aims.by_value(id, copy, &owned)),
                None => aims.settle(id, &pending),
            })
            .collect::<Result<Vec<_>, _>>()?;
        drop(owned);
        let renamed: HashMap<u64, u64> = self
            .renamed
            .take()
            .into_iter()
            .filter_map(|(named, id)| match &finals[id] {
                Value::Alias(a) => Some((named, *a)),
                Value::Ptr { source, .. } => Some((named, *source)),
                _ => None,
            })
            .collect();
        substitute(root, &finals, &copies, false);
        if !renamed.is_empty() {
            rename(root, &renamed);
        }
        Ok(())
    }

    /// The address standing for the `T` keyed `key`; `owner` claims it for the pointer that holds it.
    fn keyed(&self, type_name: &'static str, key: u32, owner: bool) -> Result<u64, Error> {
        let mut keyed = self.keyed.borrow_mut();
        let entry = keyed
            .entry((type_name, key))
            .or_insert_with(|| KeyedSource {
                address: self.fresh(),
                owned: false,
                referenced: false,
                owned_twice: false,
            });
        let taken = if owner {
            entry.owned_twice |= entry.owned;
            entry.owned = true;
            entry.owned_twice
        } else {
            entry.referenced = true;
            false
        };
        if entry.owned_twice && entry.referenced {
            return Err(Error::Invalid {
                what: type_name.into(),
                why: format!(
                    "two of them hold the key {key:#010x}, so a reference by key names neither"
                ),
            });
        }
        Ok(if taken { self.fresh() } else { entry.address })
    }

    fn record(
        &self,
        type_name: &str,
        bytes: Vec<u8>,
        kids: Vec<(u32, Value)>,
    ) -> Result<Value, Error> {
        let mut record =
            Record::from_bytes(&self.plans, type_name, bytes).map_err(|e| Error::Invalid {
                what: type_name.to_string(),
                why: e.to_string(),
            })?;
        record.set_children(kids);
        Ok(Value::Struct(record))
    }

    fn run(
        &self,
        type_name: &str,
        bytes: Vec<u8>,
        capacity: Option<usize>,
    ) -> Result<Value, Error> {
        RecordRun::from_bytes(&self.plans, type_name, bytes)
            .map(|mut run| {
                run.set_capacity(capacity);
                Value::Records(run)
            })
            .map_err(|e| Error::Invalid {
                what: type_name.to_string(),
                why: e.to_string(),
            })
    }
}

/// How many pointers in the tree point at each address: its owner and every alias.
fn count_pointers(v: &Value, out: &mut HashMap<u64, usize>) {
    match v {
        Value::Ptr {
            source,
            target: Some(t),
        } => {
            if *source != 0 {
                *out.entry(*source).or_default() += 1;
            }
            count_pointers(t, out);
        }
        Value::Alias(a) => *out.entry(*a).or_default() += 1,
        Value::Struct(r) => r
            .children()
            .iter()
            .for_each(|(_, c)| count_pointers(c, out)),
        Value::Array { elements, .. } => elements.iter().for_each(|e| count_pointers(e, out)),
        Value::Blob { parts, .. } => parts.iter().for_each(|(_, p)| count_pointers(p, out)),
        _ => {}
    }
}

fn stamp_counts(
    v: &mut Value,
    wanted: &HashMap<u64, Vec<(usize, usize)>>,
    counts: &HashMap<u64, usize>,
) -> Result<(), Error> {
    match v {
        Value::Ptr {
            source,
            target: Some(t),
        } => {
            if let (Some(fields), Value::Struct(r)) = (wanted.get(source), t.as_mut()) {
                let count = counts.get(source).copied().unwrap_or(0) as u64;
                for &(at, width) in fields {
                    let fits = width >= 8 || count >> (8 * width) == 0;
                    let slot = r.bytes_mut().get_mut(at..at + width).filter(|_| fits);
                    let Some(slot) = slot else {
                        return Err(Error::Invalid {
                            what: r.type_name().to_string(),
                            why: format!(
                                "{count} pointers point at it, and its {width}-byte count at {at:#x} cannot hold that"
                            ),
                        });
                    };
                    slot.copy_from_slice(&count.to_le_bytes()[..width]);
                }
            }
            stamp_counts(t, wanted, counts)
        }
        Value::Struct(r) => r
            .children_mut()
            .try_for_each(|(_, c)| stamp_counts(c, wanted, counts)),
        Value::Array { elements, .. } => elements
            .iter_mut()
            .try_for_each(|e| stamp_counts(e, wanted, counts)),
        Value::Blob { parts, .. } => parts
            .iter_mut()
            .try_for_each(|(_, p)| stamp_counts(p, wanted, counts)),
        _ => Ok(()),
    }
}

/// The word in the pointer slot at `at` where the file put one that is not an address there.
pub(crate) fn word_at(kids: Kids<'_>, at: usize) -> Option<u64> {
    match kid(kids, at) {
        Some(Value::NotAnAddress(word)) => Some(*word),
        _ => None,
    }
}

/// The bytes a value holds on the heap beyond its own size, counting each allocation at its capacity.
pub trait Heap {
    fn heap_bytes(&self) -> usize;
}

impl<T: Heap> Heap for Vec<T> {
    fn heap_bytes(&self) -> usize {
        self.capacity() * std::mem::size_of::<T>()
            + self.iter().map(Heap::heap_bytes).sum::<usize>()
    }
}

impl<T: Heap> Heap for Box<T> {
    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<T>() + (**self).heap_bytes()
    }
}

impl<T: Heap> Heap for Option<T> {
    fn heap_bytes(&self) -> usize {
        self.as_ref().map_or(0, Heap::heap_bytes)
    }
}

impl<T: Heap, const N: usize> Heap for [T; N] {
    fn heap_bytes(&self) -> usize {
        self.iter().map(Heap::heap_bytes).sum()
    }
}

impl Heap for String {
    fn heap_bytes(&self) -> usize {
        self.capacity()
    }
}

/// A value with an on-disk form: `Raw` is the bytes a record holds for it,
/// and anything it points at is a child of that record at its offset.
pub trait Owned: Sized + Heap {
    type Raw: Plain;

    /// Whether nothing inside points anywhere, so a run of it is plain bytes.
    const LEAF: bool;

    fn from_raw(raw: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error>;

    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error>;

    /// This value as one element the tree holds, read within `rx`.
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        match v {
            Value::Struct(record) => {
                let raw = Self::Raw::read_from(record.bytes()).ok_or_else(|| Error::Invalid {
                    what: record.type_name().to_string(),
                    why: "the record is shorter than its layout".into(),
                })?;
                let _entered = rx.enter(type_name::<Self>());
                Self::from_raw(&raw, record.children(), 0, rx)
            }
            other => Err(shape(type_name::<Self>(), 0, "a struct", Some(other))),
        }
    }

    /// This value from the tree `v` is the root of.
    ///
    /// ⚠ Read twice where a reference by key comes before the map entry that keys its target.
    fn from_element(v: &Value) -> Result<Self, Error> {
        let rx = Rx::new(v);
        rx.defer.set(true);
        let first = Self::from_value(v, &rx);
        if !rx.missed.get() {
            return first;
        }
        rx.defer.set(false);
        Self::from_value(v, &rx)
    }

    fn to_element(&self, cx: &Cx) -> Result<Value, Error>;

    /// A whole run of these, as the tree holds it.
    fn run_from_value(v: &Value, rx: &Rx<'_>) -> Result<Vec<Self>, Error> {
        match v {
            Value::Bytes(bytes) => {
                let size = std::mem::size_of::<Self::Raw>().max(1);
                bytes
                    .chunks_exact(size)
                    .map(|c| {
                        Self::from_raw(&Self::Raw::read_from(c).expect("a whole chunk"), &[], 0, rx)
                    })
                    .collect()
            }
            Value::Records(run) => run
                .iter()
                .map(|r| {
                    Self::from_raw(
                        &Self::Raw::read_from(r.bytes()).expect("a whole record"),
                        &[],
                        0,
                        rx,
                    )
                })
                .collect(),
            Value::Array { elements, .. } => {
                elements.iter().map(|e| Self::from_value(e, rx)).collect()
            }
            other => Err(shape(type_name::<Self>(), 0, "a run", Some(other))),
        }
    }

    /// The key a [`Keyed`] reference finds this by, for a type declared with `key = "field"`.
    fn key(&self) -> Option<u32> {
        None
    }

    /// [`Owned::key`], read from the raw layout.
    fn raw_key(_: &Self::Raw) -> Option<u32> {
        None
    }

    /// Mark this record as the one the file held at `address`, which a [`Ref`] finds it by.
    #[doc(hidden)]
    fn __anchor(&mut self, _address: u64) {}

    /// The address [`Owned::__anchor`] marked this with; zero for none.
    #[doc(hidden)]
    fn __anchored(&self) -> u64 {
        0
    }

    fn run_to_value(items: &[Self], capacity: Option<usize>, cx: &Cx) -> Result<Value, Error> {
        Ok(Value::Array {
            elements: items
                .iter()
                .map(|i| i.to_element(cx))
                .collect::<Result<_, _>>()?,
            capacity,
        })
    }
}

/// The raw form of an owned declaration: what a peek sees.
pub type Raw<T> = <T as Owned>::Raw;

macro_rules! scalars {
    ($($t:ty),* $(,)?) => {$(
        impl Heap for $t {
            fn heap_bytes(&self) -> usize {
                0
            }
        }

        impl Owned for $t {
            type Raw = $t;
            const LEAF: bool = true;
            fn from_raw(raw: &$t, _: Kids<'_>, _: usize, _: &Rx<'_>) -> Result<Self, Error> {
                Ok(*raw)
            }
            fn to_raw(&self, _: &mut Vec<(u32, Value)>, _: usize, _: &Cx) -> Result<$t, Error> {
                Ok(*self)
            }
            fn to_element(&self, _: &Cx) -> Result<Value, Error> {
                Err(Error::Invalid { what: stringify!($t).into(), why: "a scalar is held in bytes, not as an element".into() })
            }
            fn run_to_value(items: &[Self], _: Option<usize>, _: &Cx) -> Result<Value, Error> {
                let mut bytes = Vec::with_capacity(items.len() * std::mem::size_of::<$t>());
                for i in items {
                    bytes.extend_from_slice(Plain::as_bytes(i));
                }
                Ok(Value::Bytes(bytes))
            }
        }
    )*};
}

scalars!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64, Bool8, Hash32, Vec2, Vec3, Vec4);

impl<W> Heap for Count<W> {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl<E: OpenEnum> Heap for Open<E> {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl<W: Plain> Owned for Count<W> {
    type Raw = Count<W>;
    const LEAF: bool = true;
    fn from_raw(raw: &Self, _: Kids<'_>, _: usize, _: &Rx<'_>) -> Result<Self, Error> {
        Ok(*raw)
    }
    fn to_raw(&self, _: &mut Vec<(u32, Value)>, _: usize, _: &Cx) -> Result<Self, Error> {
        Ok(*self)
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Err(Error::Invalid {
            what: "Count".into(),
            why: "a scalar is held in bytes".into(),
        })
    }
}

impl<E: OpenEnum> Owned for Open<E>
where
    Open<E>: Plain,
{
    type Raw = Open<E>;
    const LEAF: bool = true;
    fn from_raw(raw: &Self, _: Kids<'_>, _: usize, _: &Rx<'_>) -> Result<Self, Error> {
        Ok(*raw)
    }
    fn to_raw(&self, _: &mut Vec<(u32, Value)>, _: usize, _: &Cx) -> Result<Self, Error> {
        Ok(*self)
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Err(Error::Invalid {
            what: E::name().into(),
            why: "a scalar is held in bytes".into(),
        })
    }
}

/// Whether an array of `R` is described as pointer slots, which the tree holds as one array at the field.
fn pointer_slots<R: Element>() -> bool {
    matches!(R::elem(), Elem::Ptr(_) | Elem::Str)
}

impl<T: Owned, const N: usize> Owned for [T; N]
where
    [T::Raw; N]: Plain,
    T::Raw: Element,
{
    type Raw = [T::Raw; N];
    const LEAF: bool = T::LEAF;
    fn from_raw(raw: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        let items: Vec<T> = if pointer_slots::<T::Raw>() {
            let elements = match kid(kids, at) {
                Some(Value::Array { elements, .. }) if elements.len() == N => elements,
                other => return Err(shape(type_name::<Self>(), at, "an array", other)),
            };
            elements
                .iter()
                .map(|e| pointer_in::<T>(e, rx))
                .collect::<Result<_, _>>()?
        } else {
            let stride = std::mem::size_of::<T::Raw>();
            raw.iter()
                .enumerate()
                .map(|(i, r)| T::from_raw(r, kids, at + i * stride, rx))
                .collect::<Result<_, _>>()?
        };
        Ok(items.try_into().ok().expect("N elements"))
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        let mut out = <[T::Raw; N] as Plain>::zeroed();
        if pointer_slots::<T::Raw>() {
            let mut elements = Vec::with_capacity(N);
            for item in self {
                let mut slot = Vec::new();
                item.to_raw(&mut slot, 0, cx)?;
                elements.push(match slot.pop() {
                    Some((0, v)) if slot.is_empty() => v,
                    _ => return Err(shape(type_name::<T>(), at, "one pointer", None)),
                });
            }
            kids.push((
                at as u32,
                Value::Array {
                    elements,
                    capacity: None,
                },
            ));
            return Ok(out);
        }
        let stride = std::mem::size_of::<T::Raw>();
        for (i, item) in self.iter().enumerate() {
            out[i] = item.to_raw(kids, at + i * stride, cx)?;
        }
        Ok(out)
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Err(Error::Invalid {
            what: "an array".into(),
            why: "an inline array is not an element".into(),
        })
    }
}

/// A NUL-terminated string behind a pointer; empty stands for a null pointer.
impl Owned for String {
    type Raw = Str;
    const LEAF: bool = false;
    fn from_raw(_: &Str, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        string_from(kid(kids, at), at, rx)
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, _: &Cx) -> Result<Str, Error> {
        kids.push((at as u32, string_value(self)));
        Ok(Plain::zeroed())
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Ok(string_value(self))
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        string_from(Some(v), 0, rx)
    }
}

fn string_from(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<String, Error> {
    let bytes = match v {
        Some(Value::Ptr { target: None, .. }) => return Ok(String::new()),
        Some(Value::Ptr {
            target: Some(t), ..
        }) => t.as_ref(),
        Some(alias @ Value::Alias(a)) if rx.is_copy(alias) => match rx.target(*a) {
            Some(t) => t,
            None => return Err(shape("a string", at, "a pointer", v)),
        },
        other => return Err(shape("a string", at, "a pointer", other)),
    };
    match bytes {
        Value::Bytes(b) => {
            let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf8(b[..end].to_vec()).map_err(|e| Error::Invalid {
                what: "a string".into(),
                why: e.to_string(),
            })
        }
        other => Err(shape("a string", at, "bytes", Some(other))),
    }
}

fn string_value(s: &str) -> Value {
    if s.is_empty() {
        return Value::null_ptr();
    }
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    Value::ptr_to(Value::Bytes(bytes))
}

/// The key of the `T` the tree holds as `v`, if `T` is keyed.
fn key_in<T: Owned>(v: &Value) -> Option<u32> {
    match v {
        Value::Struct(r) => T::Raw::read_from(r.bytes()).and_then(|raw| T::raw_key(&raw)),
        _ => None,
    }
}

/// Whether a pointer holding an alias of `t` at `address` still owns it: a keyed target a reference reached first, or one a `#[shares]` pointer left to it.
fn owns_alias<T: Owned>(t: &Value, address: u64, rx: &Rx<'_>) -> bool {
    let keyed = key_in::<T>(t).is_some() || rx.owner_key(address).is_some();
    (keyed && !rx.claimed.borrow().contains(&address)) || rx.is_lent(address)
}

fn pointer_from<T: Owned>(
    v: Option<&Value>,
    at: usize,
    rx: &Rx<'_>,
) -> Result<Option<Box<T>>, Error> {
    match v {
        Some(Value::Ptr { target: None, .. }) => Ok(None),
        Some(Value::Ptr {
            source,
            target: Some(t),
        }) => {
            let mut value = rx.enclosing.through(*source, || T::from_value(t, rx))?;
            rx.anchor(&mut value, *source);
            Ok(Some(Box::new(value)))
        }
        // A target this pointer still owns, or one a field reads by value.
        Some(alias @ Value::Alias(address)) => match rx.target(*address) {
            Some(t) if owns_alias::<T>(t, *address, rx) => {
                let mut value = rx.enclosing.through(*address, || T::from_value(t, rx))?;
                rx.anchor(&mut value, *address);
                Ok(Some(Box::new(value)))
            }
            Some(t) if rx.is_copy(alias) => {
                Ok(Some(Box::new(rx.as_copy(|| {
                    rx.enclosing.through(*address, || T::from_value(t, rx))
                })?)))
            }
            // A `#[keyed_by]` entry whose target an `OwnedOrKeyed` reached first: known only once that is read.
            Some(t) if rx.claimed.borrow().contains(address) => {
                rx.copy_of(alias, *address, at)?;
                Ok(Some(Box::new(rx.as_copy(|| {
                    rx.enclosing.through(*address, || T::from_value(t, rx))
                })?)))
            }
            _ => Err(shape(type_name::<T>(), at, "a pointer", v)),
        },
        other => Err(shape(type_name::<T>(), at, "a pointer", other)),
    }
}

/// A pointer at `t`, which stands at a made-up address, or at its key's if it or its entry has one.
fn pointer_to<T: Owned>(t: &T, cx: &Cx) -> Result<Value, Error> {
    let entry = cx.owner_key.take();
    let source = match t.key().or(entry) {
        Some(key) => cx.keyed(type_name::<T>(), key, true)?,
        None => cx.fresh(),
    };
    let target = cx.enclosing.through(source, || t.to_element(cx))?;
    cx.anchored(t.__anchored(), source, None);
    Ok(Value::Ptr {
        source,
        target: Some(Box::new(target)),
    })
}

/// A pointer the game allows to be null.
impl<T: Owned> Owned for Option<Box<T>>
where
    T::Raw: 'static,
{
    type Raw = Ptr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        pointer_from(kid(kids, at), at, rx)
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        pointer_from(Some(v), 0, rx)
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        match self {
            None => Ok(Value::null_ptr()),
            Some(t) => pointer_to(t.as_ref(), cx),
        }
    }
}

/// A pointer the game never leaves null.
impl<T: Owned> Owned for Box<T>
where
    T::Raw: 'static,
{
    type Raw = Ptr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        pointer_from(kid(kids, at), at, rx)?.ok_or_else(|| Error::Invalid {
            what: type_name::<T>().into(),
            why: format!("the pointer at {at:#x} is null and the game never leaves it null"),
        })
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        pointer_from(Some(v), 0, rx)?.ok_or_else(|| Error::Invalid {
            what: type_name::<T>().into(),
            why: "a null pointer the game never leaves null".into(),
        })
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        pointer_to(self.as_ref(), cx)
    }
}

/// A pointer whose null the file means as the pointee's empty value, which a save writes back as null.
#[derive(Debug, Clone, Default)]
pub struct NullIsEmpty<T> {
    value: T,
    /// The file pointed at a value equal to the empty one, so a save points at one again.
    live: bool,
}

impl<T> NullIsEmpty<T> {
    pub fn new(value: T) -> Self {
        Self { value, live: false }
    }
}

impl<T> From<T> for NullIsEmpty<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> Deref for NullIsEmpty<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for NullIsEmpty<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: PartialEq> PartialEq for NullIsEmpty<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Owned + Default + PartialEq> NullIsEmpty<T> {
    fn read(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        Ok(match pointer_from::<T>(v, at, rx)? {
            None => Self::default(),
            Some(value) => Self {
                live: *value == T::default(),
                value: *value,
            },
        })
    }
}

impl<T: Heap> Heap for NullIsEmpty<T> {
    fn heap_bytes(&self) -> usize {
        self.value.heap_bytes()
    }
}

impl<T: Owned + Default + PartialEq> Owned for NullIsEmpty<T>
where
    T::Raw: 'static,
{
    type Raw = Ptr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        Self::read(kid(kids, at), at, rx)
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        Self::read(Some(v), 0, rx)
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        if !self.live && self.value == T::default() {
            return Ok(Value::null_ptr());
        }
        pointer_to(&self.value, cx)
    }
}

fn run_from<T: Owned>(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Vec<T>, Error> {
    match v {
        Some(Value::Ptr { target: None, .. }) => Ok(Vec::new()),
        Some(Value::Ptr {
            source,
            target: Some(t),
        }) => Ok(anchored_run(T::run_from_value(t, rx)?, *source, t, rx)),
        Some(run @ (Value::Array { .. } | Value::Records(_) | Value::Bytes(_))) => {
            T::run_from_value(run, rx)
        }
        // A run a `#[shares]` pointer left to this one: this pointer still owns it.
        Some(Value::Alias(address)) if rx.is_lent(*address) => match rx.target(*address) {
            Some(run) => Ok(anchored_run(T::run_from_value(run, rx)?, *address, run, rx)),
            None => Err(shape(type_name::<T>(), at, "a run", v)),
        },
        Some(alias @ Value::Alias(address)) if rx.is_copy(alias) => match rx.target(*address) {
            Some(run) => rx.as_copy(|| T::run_from_value(run, rx)),
            None => Err(shape(type_name::<T>(), at, "a run", v)),
        },
        other => Err(shape(type_name::<T>(), at, "a run", other)),
    }
}

/// `items`, each record of which is marked with where the file held it in `run`, at `base`.
fn anchored_run<T: Owned>(mut items: Vec<T>, base: u64, run: &Value, rx: &Rx<'_>) -> Vec<T> {
    let stride = match run {
        Value::Records(run) => run.stride(),
        Value::Array { elements, .. } => match elements.first() {
            Some(Value::Struct(r)) => r.bytes().len(),
            _ => 0,
        },
        _ => 0,
    };
    if base != 0 && stride != 0 {
        for (i, item) in items.iter_mut().enumerate() {
            rx.anchor(item, base + (i * stride) as u64);
        }
    }
    items
}

fn run_to<T: Owned>(items: &[T], capacity: Option<usize>, cx: &Cx) -> Result<Value, Error> {
    if items.is_empty() {
        return Ok(Value::null_ptr());
    }
    let run = T::run_to_value(items, capacity, cx)?;
    let mut source = 0;
    for (i, item) in items.iter().enumerate() {
        let anchor = item.__anchored();
        if anchor != 0 {
            if source == 0 {
                source = cx.fresh();
            }
            cx.anchored(anchor, source, Some(i));
        }
    }
    Ok(Value::Ptr {
        source,
        target: Some(Box::new(run)),
    })
}

/// The count a counted array of `len` elements states.
fn counted_len(len: usize) -> Result<u16, Error> {
    u16::try_from(len).map_err(|_| Error::Invalid {
        what: "a counted array".into(),
        why: format!("{len} elements, and its count is sixteen bits"),
    })
}

/// A counted array (`atArray`): its length is its own.
impl<T: Owned> Owned for Vec<T>
where
    T::Raw: 'static,
{
    type Raw = Array<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        run_from(kid(kids, at), at, rx)
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        let len = counted_len(self.len())?;
        kids.push((at as u32, run_to(self, Some(self.len()), cx)?));
        Ok(Array::counted(len))
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Err(Error::Invalid {
            what: "a counted array".into(),
            why: "not an element".into(),
        })
    }
}

/// Whether `v` is a live pointer at a run of nothing.
fn live_empty_run(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Ptr { target: Some(run), .. })
        if run.run_len() == Some(0) || matches!(run.as_ref(), Value::Bytes(b) if b.is_empty()))
}

/// A run behind a pointer whose length another field states.
///
/// ⚠ A live pointer beside a stated length of zero is refused: a save would
/// drop what it points at, which in the graphics half is a payload the length
/// does not state. A virtual run that shares no allocation keeps the pointer
/// in its carry instead, and is not read through here.
pub fn run_from_child<T: Owned>(kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Vec<T>, Error> {
    if live_empty_run(kid(kids, at)) {
        return Err(Error::Invalid {
            what: type_name::<T>().into(),
            why: format!(
                "the pointer at {at:#x} is live and the length its owner states is zero, \
                 so what it points at could not be kept"
            ),
        });
    }
    run_from(kid(kids, at), at, rx)
}

/// The child of a run whose length another field states.
pub fn run_to_child<T: Owned>(
    items: &[T],
    kids: &mut Vec<(u32, Value)>,
    at: usize,
    cx: &Cx,
) -> Result<(), Error> {
    kids.push((at as u32, run_to(items, None, cx)?));
    Ok(())
}

/// Pads the byte run `run_to_child` just wrote at `abs` with zeros up to `len`; false where `abs` holds no byte run.
pub fn pad_run(kids: &mut [(u32, Value)], abs: usize, len: usize) -> bool {
    let Some((_, kid)) = kids.iter_mut().rev().find(|(at, _)| *at as usize == abs) else {
        return false;
    };
    let Value::Ptr {
        target: Some(target),
        ..
    } = kid
    else {
        return false;
    };
    match target.as_mut() {
        Value::Bytes(bytes) if bytes.len() <= len => {
            bytes.resize(len, 0);
            true
        }
        _ => false,
    }
}

/// The child of a run that follows its owner directly, with no pointer.
pub fn run_to_trailing<T: Owned>(
    items: &[T],
    kids: &mut Vec<(u32, Value)>,
    at: usize,
    cx: &Cx,
) -> Result<(), Error> {
    kids.push((at as u32, T::run_to_value(items, None, cx)?));
    Ok(())
}

/// The owned form of an allocation whose regions are computed, at a `#[codec(RawType)]` field.
///
/// ⚠ `save` runs after every other field of the owner is written, so it
/// sees their children and has the last word on the owner fields and children
/// it stamps. At a private field the allocation is derived: `open` only
/// checks it and the value is dropped, and a save lays out `Self::default()`.
pub trait Codec: Sized + Default + Heap {
    /// This value from the blob the codec's reader made of the file.
    fn open(site: &Site<'_>, rx: &Rx<'_>) -> Result<Self, Error>;

    /// The blob the canonical writer lays out, and the owner fields it implies.
    fn save(&self, site: &mut SiteMut<'_>, cx: &Cx) -> Result<(), Error>;
}

fn int_in(owner: &PackType, bytes: &[u8], name: &str) -> Option<i128> {
    let field = owner.field_by_name(name)?;
    match &field.repr {
        FieldRepr::Scalar(Scalar::Int(width)) | FieldRepr::Enum { width, .. } => {
            width.read(bytes, field.offset)
        }
        _ => None,
    }
}

/// The record a codec field sits in, as the field is opened.
pub struct Site<'a> {
    owner: PackType,
    bytes: &'a [u8],
    kids: Kids<'a>,
    at: usize,
    field: usize,
}

impl<'a> Site<'a> {
    #[doc(hidden)]
    pub fn new<O: Described>(bytes: &'a [u8], kids: Kids<'a>, at: usize, field: usize) -> Self {
        Self {
            owner: O::describe(),
            bytes,
            kids,
            at,
            field,
        }
    }

    /// The owner's integer field `name`.
    pub fn int(&self, name: &str) -> Option<i128> {
        int_in(&self.owner, self.bytes, name)
    }

    /// What the codec field's pointer holds.
    pub fn value(&self) -> Option<&'a Value> {
        kid(self.kids, self.at + self.field)
    }

    /// What the owner's pointer field `name` holds.
    pub fn child(&self, name: &str) -> Option<&'a Value> {
        let field = self.owner.field_by_name(name)?;
        kid(self.kids, self.at + field.offset)
    }

    /// A refusal naming the owner and the field.
    pub fn refuse(&self, why: impl Into<String>) -> Error {
        refusal(&self.owner, self.field, why.into())
    }
}

fn refusal(owner: &PackType, field: usize, why: String) -> Error {
    let name = owner
        .fields
        .iter()
        .find(|f| f.offset == field)
        .map_or("?", |f| f.name.as_str());
    Error::Invalid {
        what: format!("{}.{name}", owner.name),
        why,
    }
}

/// The record a codec field sits in, as the field is saved: the owner's
/// bytes so far, which the codec may stamp, and the children written so far.
pub struct SiteMut<'a> {
    owner: PackType,
    bytes: Vec<u8>,
    kids: &'a mut Vec<(u32, Value)>,
    at: usize,
    field: usize,
}

impl<'a> SiteMut<'a> {
    #[doc(hidden)]
    pub fn new<O: Described>(
        bytes: &[u8],
        kids: &'a mut Vec<(u32, Value)>,
        at: usize,
        field: usize,
    ) -> Self {
        Self {
            owner: O::describe(),
            bytes: bytes.to_vec(),
            kids,
            at,
            field,
        }
    }

    /// The owner's integer field `name`, as written so far.
    pub fn int(&self, name: &str) -> Option<i128> {
        int_in(&self.owner, &self.bytes, name)
    }

    /// Write the owner's integer field `name`; refused where it cannot hold `value`.
    pub fn set_int(&mut self, name: &str, value: i128) -> Result<(), Error> {
        let written = self.owner.field_by_name(name).and_then(|field| {
            let (FieldRepr::Scalar(Scalar::Int(width)) | FieldRepr::Enum { width, .. }) =
                &field.repr
            else {
                return None;
            };
            Some(width.write(&mut self.bytes, field.offset, value))
        });
        match written {
            Some(true) => Ok(()),
            Some(false) => Err(self.refuse(format!("`{name}` cannot hold {value}"))),
            None => Err(self.refuse(format!("the owner has no integer field `{name}`"))),
        }
    }

    /// What the owner's pointer field `name` holds, as written so far.
    pub fn child(&self, name: &str) -> Option<&Value> {
        let at = self.at + self.owner.field_by_name(name)?.offset;
        self.kids
            .iter()
            .rev()
            .find(|(o, _)| *o as usize == at)
            .map(|(_, v)| v)
    }

    /// [`SiteMut::child`], to edit.
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Value> {
        let at = self.at + self.owner.field_by_name(name)?.offset;
        self.kids
            .iter_mut()
            .rev()
            .find(|(o, _)| *o as usize == at)
            .map(|(_, v)| v)
    }

    /// The codec field's child: its pointer.
    pub fn put(&mut self, value: Value) {
        self.kids.push(((self.at + self.field) as u32, value));
    }

    /// A refusal naming the owner and the field.
    pub fn refuse(&self, why: impl Into<String>) -> Error {
        refusal(&self.owner, self.field, why.into())
    }

    /// The owner's raw layout, with what the codec stamped.
    #[doc(hidden)]
    pub fn finish<R: Plain>(self) -> Result<R, Error> {
        R::read_from(&self.bytes).ok_or_else(|| Error::Invalid {
            what: self.owner.name.clone(),
            why: "the record is shorter than its layout".into(),
        })
    }
}

/// The owned pointer `P` the tree holds as `v`, as a field of that type would read it.
pub fn pointer_in<P: Owned>(v: &Value, rx: &Rx<'_>) -> Result<P, Error> {
    P::from_raw(
        &Plain::zeroed(),
        std::slice::from_ref(&(0, v.clone())),
        0,
        rx,
    )
}

/// The `R` the alias `v` points at, as a copy of the record another pointer owns; `None` where `v` is not an alias of an `R`.
///
/// ⚠ A save writes the copy as a record of its own, so two slots that shared one record in the file own one each.
pub fn alias_copy<R: Owned>(v: &Value, rx: &Rx<'_>) -> Result<Option<R>, Error>
where
    R::Raw: Described,
{
    let Value::Alias(address) = v else {
        return Ok(None);
    };
    let Some(target @ Value::Struct(record)) = rx.target(*address) else {
        return Ok(None);
    };
    if record.type_name() != <R::Raw as Described>::name() {
        return Ok(None);
    }
    rx.as_copy(|| rx.enclosing.through(*address, || R::from_value(target, rx)))
        .map(Some)
}

/// A pointer that may share the allocation another field of its record points at: `Same` holds nothing, declared with `#[shares(other)]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Shared<V> {
    Same,
    Own(V),
}

impl<V: Heap> Heap for Shared<V> {
    fn heap_bytes(&self) -> usize {
        match self {
            Shared::Same => 0,
            Shared::Own(v) => v.heap_bytes(),
        }
    }
}

impl<V: Default> Default for Shared<V> {
    fn default() -> Self {
        Shared::Own(V::default())
    }
}

/// Where the pointer a `#[shares]` field may share with sits: the child at
/// `at` of the record, then the pointer at the field path `inner` of the
/// record that child points at, then element `index` of the run there.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct Along {
    pub at: usize,
    pub inner: Option<&'static str>,
    pub index: Option<usize>,
}

impl Along {
    /// The pointer at `at` itself.
    pub fn at(at: usize) -> Self {
        Self {
            at,
            inner: None,
            index: None,
        }
    }
}

/// The address a live pointer or an alias stands for.
fn address_of(v: &Value) -> Option<u64> {
    match v {
        Value::Ptr {
            source,
            target: Some(_),
        } if *source != 0 => Some(*source),
        Value::Alias(a) if *a != 0 => Some(*a),
        _ => None,
    }
}

/// The value `along` names, from the child at its offset.
fn aimed<'v>(first: Option<&'v Value>, along: &Along) -> Option<&'v Value> {
    let mut v = first?;
    if let Some(path) = along.inner {
        let Value::Ptr {
            target: Some(t), ..
        } = v
        else {
            return None;
        };
        let Value::Struct(record) = t.as_ref() else {
            return None;
        };
        v = record.child(path)?;
    }
    let Some(i) = along.index else {
        return Some(v);
    };
    match v {
        Value::Array { elements, .. } => elements.get(i),
        Value::Ptr {
            target: Some(run), ..
        } => match run.as_ref() {
            Value::Array { elements, .. } => elements.get(i),
            _ => None,
        },
        _ => None,
    }
}

/// [`aimed`], to edit.
fn aimed_mut<'v>(first: Option<&'v mut Value>, along: &Along) -> Option<&'v mut Value> {
    let mut v = first?;
    if let Some(path) = along.inner {
        let Value::Ptr {
            target: Some(t), ..
        } = v
        else {
            return None;
        };
        let Value::Struct(record) = t.as_mut() else {
            return None;
        };
        v = record.child_mut(path)?;
    }
    let Some(i) = along.index else {
        return Some(v);
    };
    match v {
        Value::Array { elements, .. } => elements.get_mut(i),
        Value::Ptr {
            target: Some(run), ..
        } => match run.as_mut() {
            Value::Array { elements, .. } => elements.get_mut(i),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the pointer at `at` points where the one `along` names does, whichever of the two the file made the owner.
#[doc(hidden)]
pub fn shares(kids: Kids<'_>, at: usize, along: &Along) -> bool {
    let here = kid(kids, at).and_then(address_of);
    let there = aimed(kid(kids, along.at), along).and_then(address_of);
    here.is_some() && here == there
}

/// Where the pointer at `at` owns what the one `along` names only aliases, leave it to that one, which owns it once opened.
#[doc(hidden)]
pub fn lend(rx: &Rx<'_>, kids: Kids<'_>, at: usize, along: &Along) {
    let Some(Value::Ptr {
        source,
        target: Some(_),
    }) = kid(kids, at)
    else {
        return;
    };
    let aliased = matches!(aimed(kid(kids, along.at), along), Some(Value::Alias(a)) if a == source);
    if *source != 0 && aliased {
        rx.lent.borrow_mut().insert(*source);
    }
}

/// Point the pointer at `at` where the one already written that `along` names points; null where that one is null or absent.
///
/// ⚠ `first` says a reader reaches the pointer at `at` before the one `along`
/// names, so it takes over the target and leaves that one the alias: the
/// canonical writer lays a target out at its owner's turn.
#[doc(hidden)]
pub fn share(
    kids: &mut Vec<(u32, Value)>,
    at: usize,
    along: &Along,
    first: bool,
    cx: &Cx,
) -> Result<(), Error> {
    let aim = kids
        .iter_mut()
        .rev()
        .find(|(o, _)| *o as usize == along.at)
        .map(|(_, v)| v);
    if aim.is_none() {
        return Err(Error::Invalid {
            what: "a shared pointer".into(),
            why: format!("nothing at {:#x} to share", along.at),
        });
    }
    let value = match aimed_mut(aim, along) {
        None | Some(Value::Ptr { target: None, .. }) => Value::null_ptr(),
        Some(Value::Alias(a)) => Value::Alias(*a),
        Some(owner) => {
            let Value::Ptr { source, .. } = owner else {
                return Err(shape("a shared pointer", at, "a pointer", Some(owner)));
            };
            if *source == 0 {
                *source = cx.fresh();
            }
            let alias = Value::Alias(*source);
            if first {
                std::mem::replace(owner, alias)
            } else {
                alias
            }
        }
    };
    kids.push((at as u32, value));
    Ok(())
}

/// How many `T` the run `along` names holds, as a save wrote it; zero where it is null.
#[doc(hidden)]
pub fn shared_len<T: Owned>(kids: &[(u32, Value)], along: &Along) -> usize {
    fn owned_at(v: &Value, address: u64) -> Option<&Value> {
        match v {
            Value::Ptr {
                source,
                target: Some(t),
            } if *source == address => Some(t),
            Value::Ptr {
                target: Some(t), ..
            } => owned_at(t, address),
            Value::Struct(r) => r.children().iter().find_map(|(_, c)| owned_at(c, address)),
            Value::Array { elements, .. } => elements.iter().find_map(|e| owned_at(e, address)),
            Value::Blob { parts, .. } => parts.iter().find_map(|(_, p)| owned_at(p, address)),
            _ => None,
        }
    }
    let first = kids
        .iter()
        .rev()
        .find(|(o, _)| *o as usize == along.at)
        .map(|(_, v)| v);
    let run = match aimed(first, along) {
        Some(Value::Ptr {
            target: Some(t), ..
        }) => Some(t.as_ref()),
        Some(Value::Alias(a)) => kids.iter().find_map(|(_, v)| owned_at(v, *a)),
        _ => None,
    };
    match run {
        Some(Value::Bytes(b)) => b.len() / std::mem::size_of::<T::Raw>().max(1),
        Some(run) => run.run_len().unwrap_or(0),
        None => 0,
    }
}

/// A pointer that owns its target, or names by its entry's key one a `#[keyed_by]` map elsewhere in the tree owns.
#[derive(Debug, Clone, PartialEq)]
pub enum OwnedOrKeyed<T> {
    Own(Option<Box<T>>),
    Key(u32),
}

impl<T> Default for OwnedOrKeyed<T> {
    fn default() -> Self {
        OwnedOrKeyed::Own(None)
    }
}

impl<T: Heap> Heap for OwnedOrKeyed<T> {
    fn heap_bytes(&self) -> usize {
        match self {
            OwnedOrKeyed::Own(v) => v.heap_bytes(),
            OwnedOrKeyed::Key(_) => 0,
        }
    }
}

impl<T: Owned> Owned for OwnedOrKeyed<T>
where
    T::Raw: 'static,
{
    type Raw = Ptr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        let address = match kid(kids, at) {
            Some(Value::Alias(a)) if !rx.is_lent(*a) => *a,
            other => {
                if let Some(Value::Ptr {
                    source,
                    target: Some(_),
                }) = other
                {
                    rx.claimed.borrow_mut().insert(*source);
                }
                return pointer_from(other, at, rx).map(OwnedOrKeyed::Own);
            }
        };
        match rx
            .target(address)
            .and_then(key_in::<T>)
            .or_else(|| rx.owner_key(address))
        {
            Some(key) => Ok(OwnedOrKeyed::Key(key)),
            None if rx.defer.get() => {
                rx.missed.set(true);
                Ok(Self::default())
            }
            None => Err(Error::Invalid {
                what: type_name::<T>().into(),
                why: format!(
                    "the pointer at {at:#x} points at what another pointer owns, and neither \
                     what it points at nor a map entry owning it states a key"
                ),
            }),
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        match self {
            OwnedOrKeyed::Own(pointer) => pointer.to_element(cx),
            OwnedOrKeyed::Key(key) => Ok(Value::Alias(cx.keyed(type_name::<T>(), *key, false)?)),
        }
    }
}

/// Per slot read by value, its index in a run and the path of the pointer owning its target.
type Copies = Vec<(Option<usize>, String)>;

/// A pointer slot, or a run of them, that reads a target another pointer owns as a copy of it.
#[doc(hidden)]
pub trait ByValue: Owned {
    /// Each alias `v` holds that this reads by value.
    fn copies(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Copies, Error>;

    /// This value written with each slot `copies` names as a stand-in for the alias of its owner.
    fn to_raw_by_value(
        &self,
        copies: &Copies,
        kids: &mut Vec<(u32, Value)>,
        at: usize,
        cx: &Cx,
    ) -> Result<Self::Raw, Error>;
}

/// The one alias a single pointer slot reads by value.
fn one_copy(v: Option<&Value>, at: usize, rx: &Rx<'_>, owns: bool) -> Result<Copies, Error> {
    match v {
        Some(alias @ Value::Alias(a)) if !owns => Ok(vec![(None, rx.copy_of(alias, *a, at)?)]),
        _ => Ok(Vec::new()),
    }
}

/// `p` at `at`, as the stand-in of a slot read by value where `copies` names one.
fn one_by_value<P: Owned>(
    p: &P,
    copies: &Copies,
    kids: &mut Vec<(u32, Value)>,
    at: usize,
    cx: &Cx,
) -> Result<(), Error> {
    let value = match copies.iter().find(|(i, _)| i.is_none()) {
        Some((_, path)) => cx.by_value(path, p)?,
        None => p.to_element(cx)?,
    };
    kids.push((at as u32, value));
    Ok(())
}

impl<T: Owned> ByValue for Option<Box<T>>
where
    T::Raw: 'static,
{
    fn copies(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Copies, Error> {
        let owns = match v {
            Some(Value::Alias(a)) => rx.target(*a).is_some_and(|t| owns_alias::<T>(t, *a, rx)),
            _ => true,
        };
        one_copy(v, at, rx, owns)
    }
    fn to_raw_by_value(
        &self,
        copies: &Copies,
        kids: &mut Vec<(u32, Value)>,
        at: usize,
        cx: &Cx,
    ) -> Result<Self::Raw, Error> {
        one_by_value(self, copies, kids, at, cx)?;
        Ok(Plain::zeroed())
    }
}

impl<T: Owned> ByValue for Box<T>
where
    T::Raw: 'static,
{
    fn copies(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Copies, Error> {
        Option::<Box<T>>::copies(v, at, rx)
    }
    fn to_raw_by_value(
        &self,
        copies: &Copies,
        kids: &mut Vec<(u32, Value)>,
        at: usize,
        cx: &Cx,
    ) -> Result<Self::Raw, Error> {
        one_by_value(self, copies, kids, at, cx)?;
        Ok(Plain::zeroed())
    }
}

impl ByValue for String {
    fn copies(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Copies, Error> {
        let owns = matches!(v, Some(Value::Alias(a)) if rx.is_lent(*a));
        one_copy(v, at, rx, owns)
    }
    fn to_raw_by_value(
        &self,
        copies: &Copies,
        kids: &mut Vec<(u32, Value)>,
        at: usize,
        cx: &Cx,
    ) -> Result<Str, Error> {
        one_by_value(self, copies, kids, at, cx)?;
        Ok(Plain::zeroed())
    }
}

/// The run `items` as the tree holds it, each element `copies` names as the stand-in of a slot read by value.
fn run_by_value<E: ByValue>(
    items: &[E],
    copies: &Copies,
    capacity: Option<usize>,
    cx: &Cx,
) -> Result<Value, Error> {
    if items.is_empty() {
        return Ok(Value::null_ptr());
    }
    let elements = items
        .iter()
        .enumerate()
        .map(|(i, e)| match copies.iter().find(|(j, _)| *j == Some(i)) {
            Some((_, path)) => cx.by_value(path, e),
            None => e.to_element(cx),
        })
        .collect::<Result<_, _>>()?;
    Ok(Value::ptr_to(Value::Array { elements, capacity }))
}

impl<E: ByValue> ByValue for Vec<E>
where
    E::Raw: 'static,
{
    fn copies(v: Option<&Value>, at: usize, rx: &Rx<'_>) -> Result<Copies, Error> {
        let Some(Value::Ptr {
            target: Some(run), ..
        }) = v
        else {
            return Ok(Vec::new());
        };
        let Value::Array { elements, .. } = run.as_ref() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (i, e) in elements.iter().enumerate() {
            for (_, path) in E::copies(Some(e), at, rx)? {
                out.push((Some(i), path));
            }
        }
        Ok(out)
    }
    fn to_raw_by_value(
        &self,
        copies: &Copies,
        kids: &mut Vec<(u32, Value)>,
        at: usize,
        cx: &Cx,
    ) -> Result<Self::Raw, Error> {
        let len = counted_len(self.len())?;
        kids.push((at as u32, run_by_value(self, copies, Some(self.len()), cx)?));
        Ok(Array::counted(len))
    }
}

/// Keep in `carry`, for the field at `at` (absolute `abs`), each alias it reads by value.
#[doc(hidden)]
pub fn keep_copies<P: ByValue>(
    carry: &mut Carry,
    at: usize,
    kids: Kids<'_>,
    abs: usize,
    rx: &Rx<'_>,
) -> Result<(), Error> {
    for (i, path) in P::copies(kid(kids, abs), abs, rx)? {
        carry.copies.push((at as u32, i, path));
    }
    Ok(())
}

/// The field at `at` (absolute `abs`) written, each slot `carry` read by value as its stand-in.
#[doc(hidden)]
pub fn write_by_value<P: ByValue>(
    p: &P,
    carry: &Carry,
    at: usize,
    kids: &mut Vec<(u32, Value)>,
    abs: usize,
    cx: &Cx,
) -> Result<P::Raw, Error> {
    match carry.copies_at(at) {
        copies if copies.is_empty() => p.to_raw(kids, abs, cx),
        copies => p.to_raw_by_value(&copies, kids, abs, cx),
    }
}

/// [`run_to_child`], each element `carry` read by value for the field at `at` (absolute `abs`) as its stand-in.
#[doc(hidden)]
pub fn run_to_child_by_value<E: ByValue>(
    items: &[E],
    carry: &Carry,
    at: usize,
    kids: &mut Vec<(u32, Value)>,
    abs: usize,
    cx: &Cx,
) -> Result<(), Error> {
    match carry.copies_at(at) {
        copies if copies.is_empty() => run_to_child(items, kids, abs, cx),
        copies => {
            kids.push((abs as u32, run_by_value(items, &copies, None, cx)?));
            Ok(())
        }
    }
}

/// Keep in `carry`, for the run at `at` (absolute `abs`), the path of the pointer owning the run it aliases, which it reads by value.
#[doc(hidden)]
pub fn keep_run_copy(
    carry: &mut Carry,
    at: usize,
    kids: Kids<'_>,
    abs: usize,
    rx: &Rx<'_>,
) -> Result<(), Error> {
    if let Some(alias @ Value::Alias(a)) = kid(kids, abs) {
        if !rx.is_lent(*a) {
            let path = rx.copy_of(alias, *a, abs)?;
            carry.copies.push((at as u32, None, path));
        }
    }
    Ok(())
}

/// The run `items` with `capacity`, as the stand-in of the alias `carry` read by value for the field at `at` where it did.
fn run_copied<E: Owned>(
    items: &[E],
    capacity: Option<usize>,
    carry: &Carry,
    at: usize,
    cx: &Cx,
) -> Result<Value, Error> {
    match carry.copies_at(at).into_iter().find(|(i, _)| i.is_none()) {
        Some((_, path)) => cx.copied(&path, || run_to(items, capacity, cx)),
        None => run_to(items, capacity, cx),
    }
}

/// [`run_to_child`], as the stand-in of the alias `carry` read by value for the field at `at` (absolute `abs`) where it did.
#[doc(hidden)]
pub fn run_to_child_copied<E: Owned>(
    items: &[E],
    carry: &Carry,
    at: usize,
    kids: &mut Vec<(u32, Value)>,
    abs: usize,
    cx: &Cx,
) -> Result<(), Error> {
    kids.push((abs as u32, run_copied(items, None, carry, at, cx)?));
    Ok(())
}

/// A counted array, as the stand-in of the alias `carry` read by value for the field at `at` (absolute `abs`) where it did.
///
/// ⚠ The stand-in is out of [`Carry::apply_capacity`]'s reach, so the copy takes its carried capacity here.
#[doc(hidden)]
pub fn counted_run_copied<E: Owned>(
    items: &[E],
    carry: &Carry,
    at: usize,
    kids: &mut Vec<(u32, Value)>,
    abs: usize,
    cx: &Cx,
) -> Result<Array<E::Raw>, Error> {
    let len = counted_len(items.len())?;
    let capacity = carry
        .capacities
        .iter()
        .find(|(o, _)| *o as usize == at)
        .map_or(items.len(), |&(_, cap)| items.len().max(cap.into()));
    kids.push((
        abs as u32,
        run_copied(items, Some(capacity), carry, at, cx)?,
    ));
    Ok(Array::counted(len))
}

/// A pointer that owns its target as `P` does, or names by its path from the root one a pointer elsewhere in the tree owns.
///
/// ⚠ The canonical writer places a target only where an allocation starts,
/// so a reference to an element past the first of a run of records is saved as
/// the tree saves it: a copy of the element whose pointers point at the element's.
#[derive(Debug, Clone, PartialEq)]
pub enum Ref<P> {
    Own(P),
    /// `main.rig` is the target of the pointer `rig` in the record `main` points at; `all[2]` is element 2 of the run `all`.
    To(RefPath),
}

/// The path a [`Ref`] names its target by; one opened from a file follows that target through edits to the runs around it.
#[derive(Clone, Default)]
pub struct RefPath {
    path: String,
    /// The address the file held the target at, which the target carries; zero for a path written by hand.
    anchor: u64,
}

impl Deref for RefPath {
    type Target = str;
    fn deref(&self) -> &str {
        &self.path
    }
}

impl From<&str> for RefPath {
    fn from(path: &str) -> Self {
        path.to_string().into()
    }
}

impl From<String> for RefPath {
    fn from(path: String) -> Self {
        Self { path, anchor: 0 }
    }
}

impl PartialEq for RefPath {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl std::fmt::Debug for RefPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.path.fmt(f)
    }
}

impl<P: Default> Default for Ref<P> {
    fn default() -> Self {
        Ref::Own(P::default())
    }
}

/// A pointer at one target, which a [`Ref`] holds.
#[doc(hidden)]
pub trait OnePointer: Owned {}

impl<T: Owned> OnePointer for Box<T> where T::Raw: 'static {}
impl<T: Owned> OnePointer for Option<Box<T>> where T::Raw: 'static {}
impl OnePointer for String {}

impl<P: Heap> Heap for Ref<P> {
    fn heap_bytes(&self) -> usize {
        match self {
            Ref::Own(p) => p.heap_bytes(),
            Ref::To(path) => path.path.capacity(),
        }
    }
}

impl<P: OnePointer> Owned for Ref<P> {
    type Raw = P::Raw;
    const LEAF: bool = false;
    fn from_raw(raw: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        match rx.referred(kid(kids, at), at)? {
            Some(path) => Ok(Ref::To(path)),
            None => P::from_raw(raw, kids, at, rx).map(Ref::Own),
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        match self {
            Ref::Own(p) => p.to_raw(kids, at, cx),
            Ref::To(path) => {
                kids.push((at as u32, cx.refer(path)));
                Ok(Plain::zeroed())
            }
        }
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        pointer_in(v, rx)
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        match self {
            Ref::Own(p) => p.to_element(cx),
            Ref::To(path) => Ok(cx.refer(path)),
        }
    }
}

/// Where a [`Ref`]'s path leads in a tree being built.
#[derive(Clone)]
enum Aim {
    At(u64),
    /// The slot there is another [`Ref`]'s, by its stand-in's index.
    Chain(usize),
    /// A copy of an element past the first of a run, as a pointer at it.
    Copy(Value),
}

/// The targets a build's [`Ref`]s name, found in the tree it built.
struct Aims<'a> {
    cx: &'a Cx,
    wanted: &'a HashMap<&'a str, Vec<usize>>,
    /// Per run path, the index of each element of it a path names.
    elements: HashMap<&'a str, Vec<usize>>,
    found: Vec<Option<Aim>>,
}

impl Aims<'_> {
    fn put(&mut self, path: &str, aim: Aim) {
        for &id in &self.wanted[path] {
            self.found[id] = Some(aim.clone());
        }
    }

    fn record(&mut self, v: &mut Value, path: &str) -> Result<(), Error> {
        let Value::Struct(r) = v else { return Ok(()) };
        let names: Vec<String> = r
            .children()
            .iter()
            .map(|(at, _)| joined(path, slot_name(r.plan(), *at)))
            .collect();
        for ((_, child), name) in r.children_mut().zip(names) {
            self.slot(child, name)?;
        }
        Ok(())
    }

    fn slot(&mut self, v: &mut Value, path: String) -> Result<(), Error> {
        if self.wanted.contains_key(path.as_str()) {
            let aim = match v {
                Value::Ptr {
                    source,
                    target: Some(_),
                } => Some(Aim::At(self.named(source))),
                Value::Alias(a) => match a.checked_sub(REFERRED) {
                    Some(id) if (id as usize) < self.found.len() => Some(Aim::Chain(id as usize)),
                    _ => Some(Aim::At(*a)),
                },
                // Nothing is there, which a reference refuses and a copy is written apart for.
                _ => None,
            };
            if let Some(aim) = aim {
                self.put(&path, aim);
            }
        }
        match v {
            Value::Ptr {
                source,
                target: Some(t),
            } => self.target(source, t.as_mut(), &path),
            Value::Array { elements, .. } => {
                for (i, e) in elements.iter_mut().enumerate() {
                    self.slot(e, format!("{path}[{i}]"))?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn target(&mut self, source: &mut u64, t: &mut Value, path: &str) -> Result<(), Error> {
        match t {
            Value::Struct(_) => self.record(t, path),
            Value::Array { elements, .. } => {
                for (i, e) in elements.iter_mut().enumerate() {
                    let at = format!("{path}[{i}]");
                    let Value::Struct(r) = e else {
                        self.slot(e, at)?;
                        continue;
                    };
                    if self.wanted.contains_key(at.as_str()) {
                        let aim = match i {
                            0 => Aim::At(self.named(source)),
                            _ => Aim::Copy(self.copy(r)),
                        };
                        self.put(&at, aim);
                    }
                    self.record(e, &at)?;
                }
                Ok(())
            }
            Value::Records(run) => {
                let Some(indices) = self.elements.get(path).cloned() else {
                    return Ok(());
                };
                let stride = run.stride();
                for i in indices {
                    let at = format!("{path}[{i}]");
                    let aim = match i {
                        0 => Aim::At(self.named(source)),
                        _ if i < run.len() => {
                            let bytes = run.bytes()[i * stride..(i + 1) * stride].to_vec();
                            Aim::Copy(Value::Ptr {
                                source: self.cx.fresh(),
                                target: Some(Box::new(self.cx.record(
                                    run.type_name(),
                                    bytes,
                                    Vec::new(),
                                )?)),
                            })
                        }
                        _ => continue,
                    };
                    self.put(&at, aim);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The address standing for the target of the pointer whose source is `source`, made up where it has none.
    fn named(&self, source: &mut u64) -> u64 {
        if *source == 0 {
            *source = self.cx.fresh();
        }
        *source
    }

    /// A pointer at a copy of the run element `r`, each pointer in it at what the element's points at.
    fn copy(&self, r: &mut Record) -> Value {
        fn alias(v: &mut Value, cx: &Cx) -> Value {
            match v {
                Value::Ptr {
                    source,
                    target: Some(_),
                } => {
                    if *source == 0 {
                        *source = cx.fresh();
                    }
                    Value::Alias(*source)
                }
                Value::Array { elements, capacity } => Value::Array {
                    elements: elements.iter_mut().map(|e| alias(e, cx)).collect(),
                    capacity: *capacity,
                },
                other => other.clone(),
            }
        }
        let mut copy = r.clone();
        let kids: Vec<Value> = r.children_mut().map(|(_, c)| alias(c, self.cx)).collect();
        for ((_, c), k) in copy.children_mut().zip(kids) {
            *c = k;
        }
        Value::Ptr {
            source: self.cx.fresh(),
            target: Some(Box::new(Value::Struct(copy))),
        }
    }

    /// The value the stand-in `id` of a slot read by value becomes: an alias of its owner's target while `copy` still equals it, else `copy`.
    fn by_value(&self, id: usize, copy: &Value, owned: &HashMap<u64, &Value>) -> Value {
        if let (
            Some(Aim::At(a)),
            Value::Ptr {
                target: Some(c), ..
            },
        ) = (&self.found[id], copy)
        {
            if owned
                .get(a)
                .is_some_and(|t| same(c, t, &mut HashMap::new()))
            {
                return Value::Alias(*a);
            }
        }
        copy.clone()
    }

    /// The value the stand-in `id` becomes: an alias of its target, or the copy that stands for it.
    fn settle(&self, id: usize, pending: &[String]) -> Result<Value, Error> {
        let mut at = id;
        for _ in 0..=self.found.len() {
            match &self.found[at] {
                None => return Err(nothing_at(&pending[at])),
                Some(Aim::At(a)) => return Ok(Value::Alias(*a)),
                // The first reference to name the element holds its copy, and the rest alias it.
                Some(Aim::Copy(copy))
                    if at == id && self.wanted[pending[id].as_str()].first() == Some(&id) =>
                {
                    return Ok(copy.clone())
                }
                Some(Aim::Copy(Value::Ptr { source, .. })) => return Ok(Value::Alias(*source)),
                Some(Aim::Copy(_)) => return Err(nothing_at(&pending[at])),
                Some(Aim::Chain(next)) => at = *next,
            }
        }
        Err(Error::Invalid {
            what: "a reference".into(),
            why: format!("`{}` names a reference that names it back", pending[id]),
        })
    }
}

fn nothing_at(path: &str) -> Error {
    Error::Invalid {
        what: "a reference".into(),
        why: format!("it names `{path}`, where nothing in the tree is"),
    }
}

/// Replace each [`Ref`] stand-in in `v` with what it became; within a copy, an alias of what a copy became.
///
/// A slot read by value (`by_value` holds its stand-in) is written apart as its own allocation, not as a copy's.
fn substitute(v: &mut Value, finals: &[Value], by_value: &HashMap<usize, Value>, in_copy: bool) {
    if let Value::Alias(a) = v {
        if let Some(id) = a
            .checked_sub(REFERRED)
            .map(|id| id as usize)
            .filter(|id| *id < finals.len())
        {
            let apart = by_value.contains_key(&id);
            *v = match &finals[id] {
                Value::Ptr { source, .. } if in_copy && !apart => Value::Alias(*source),
                other => other.clone(),
            };
            if let Value::Ptr {
                target: Some(t), ..
            } = v
            {
                substitute(t, finals, by_value, in_copy || !apart);
            }
            return;
        }
    }
    match v {
        Value::Ptr {
            target: Some(t), ..
        } => substitute(t, finals, by_value, in_copy),
        Value::Struct(r) => r
            .children_mut()
            .for_each(|(_, c)| substitute(c, finals, by_value, in_copy)),
        Value::Array { elements, .. } => elements
            .iter_mut()
            .for_each(|e| substitute(e, finals, by_value, in_copy)),
        Value::Blob { parts, .. } => parts
            .iter_mut()
            .for_each(|(_, p)| substitute(p, finals, by_value, in_copy)),
        _ => {}
    }
}

/// Point each alias in `v` at an address `renamed` holds at the address it maps to.
fn rename(v: &mut Value, renamed: &HashMap<u64, u64>) {
    match v {
        Value::Alias(a) => {
            if let Some(to) = renamed.get(a) {
                *a = *to;
            }
        }
        Value::Ptr {
            target: Some(t), ..
        } => rename(t, renamed),
        Value::Struct(r) => r.children_mut().for_each(|(_, c)| rename(c, renamed)),
        Value::Array { elements, .. } => elements.iter_mut().for_each(|e| rename(e, renamed)),
        Value::Blob { parts, .. } => parts.iter_mut().for_each(|(_, p)| rename(p, renamed)),
        _ => {}
    }
}

/// A counted array of pointers whose element `i` owns its target under the key `keys` gives it `i`th.
#[doc(hidden)]
///
/// An element `carry` read by value for the field at `within` is written as its
/// stand-in, and its key names whatever that stand-in becomes.
pub fn keyed_run_to<T: Owned>(
    items: &[T],
    keys: impl IntoIterator<Item = u32>,
    carry: &Carry,
    within: usize,
    kids: &mut Vec<(u32, Value)>,
    at: usize,
    cx: &Cx,
) -> Result<Array<T::Raw>, Error> {
    let len = u16::try_from(items.len()).map_err(|_| Error::Invalid {
        what: "a counted array".into(),
        why: format!("{} elements, and its count is sixteen bits", items.len()),
    })?;
    let value = if items.is_empty() {
        Value::null_ptr()
    } else {
        let copies = carry.copies_at(within);
        let mut keys = keys.into_iter();
        let elements = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let copy = copies.iter().find(|(j, _)| *j == Some(i));
                match (keys.next(), copy) {
                    (Some(key), Some((_, path))) => cx.__keyed_by(key, || cx.by_value(path, item)),
                    (Some(key), None) => cx.__keyed_by(key, || item.to_element(cx)),
                    (None, Some((_, path))) => cx.by_value(path, item),
                    (None, None) => item.to_element(cx),
                }
            })
            .collect::<Result<_, _>>()?;
        Value::ptr_to(Value::Array {
            elements,
            capacity: Some(items.len()),
        })
    };
    kids.push((at as u32, value));
    Ok(Array::counted(len))
}

/// A pointer at the nearest record around it that is a `T`: derived on save,
/// so it holds nothing.
///
/// ⚠ Its impls bound nothing on `T`: a derive would require `T: Clone`, which
/// a record generic over its target is not.
pub struct Back<T> {
    /// The file held null here rather than the derivation.
    null: bool,
    _t: PhantomData<fn(&T)>,
}

impl<T> Clone for Back<T> {
    fn clone(&self) -> Self {
        Self {
            null: self.null,
            _t: PhantomData,
        }
    }
}

impl<T> PartialEq for Back<T> {
    fn eq(&self, other: &Self) -> bool {
        self.null == other.null
    }
}

impl<T> Eq for Back<T> {}

impl<T> Default for Back<T> {
    fn default() -> Self {
        Self {
            null: false,
            _t: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for Back<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let null = if self.null { "(null)" } else { "" };
        write!(f, "Back<{}>{null}", type_name::<T>())
    }
}

impl<T> Heap for Back<T> {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl<T: Owned> Owned for Back<T>
where
    T::Raw: 'static,
{
    type Raw = Ptr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        match kid(kids, at) {
            Some(Value::Ptr { target: None, .. }) => Ok(Self {
                null: true,
                _t: PhantomData,
            }),
            Some(Value::Alias(a)) if rx.enclosing.innermost(type_name::<T>()) == Some(*a) => {
                Ok(Self::default())
            }
            other => Err(Error::Invalid {
                what: type_name::<T>().into(),
                why: format!(
                    "the pointer at {at:#x} is {}, not one at the {} around it",
                    other.map_or("missing", shape_of),
                    type_name::<T>()
                ),
            }),
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        if self.null {
            return Ok(Value::null_ptr());
        }
        cx.enclosing
            .innermost(type_name::<T>())
            .map(Value::Alias)
            .ok_or_else(|| Error::Invalid {
                what: type_name::<T>().into(),
                why: "a pointer back at a record no pointer holds, so it has nothing to name"
                    .into(),
            })
    }
}

/// A pointer at the `T` whose key is `key`, wherever the value holds it; `None` is null.
#[derive(Clone, PartialEq, Eq)]
pub struct Keyed<T> {
    pub key: Option<u32>,
    _t: PhantomData<fn(&T)>,
}

impl<T> Keyed<T> {
    pub fn to(key: u32) -> Self {
        Self {
            key: Some(key),
            _t: PhantomData,
        }
    }
}

impl<T> Default for Keyed<T> {
    fn default() -> Self {
        Self {
            key: None,
            _t: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for Keyed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.key {
            Some(k) => write!(f, "Keyed<{}>({k:#010x})", type_name::<T>()),
            None => write!(f, "Keyed<{}>(null)", type_name::<T>()),
        }
    }
}

impl<T> Heap for Keyed<T> {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl<T: Owned> Owned for Keyed<T>
where
    T::Raw: 'static,
{
    type Raw = Ptr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, rx: &Rx<'_>) -> Result<Self, Error> {
        let (address, target) = match kid(kids, at) {
            Some(Value::Ptr { target: None, .. }) => return Ok(Self::default()),
            Some(Value::Ptr {
                source,
                target: Some(t),
            }) => (*source, Some(t.as_ref())),
            Some(Value::Alias(a)) => (*a, rx.target(*a)),
            other => return Err(shape(type_name::<T>(), at, "a pointer", other)),
        };
        match target
            .and_then(key_in::<T>)
            .or_else(|| rx.owner_key(address))
        {
            Some(key) => Ok(Self::to(key)),
            None if rx.defer.get() => {
                rx.missed.set(true);
                Ok(Self::default())
            }
            None => Err(Error::Invalid {
                what: type_name::<T>().into(),
                why: format!(
                    "the pointer at {at:#x} is held by key, and neither what it points at nor \
                     a map entry owning it states one"
                ),
            }),
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn to_element(&self, cx: &Cx) -> Result<Value, Error> {
        match self.key {
            None => Ok(Value::null_ptr()),
            Some(key) => Ok(Value::Alias(cx.keyed(type_name::<T>(), key, false)?)),
        }
    }
}

/// A key field's value as the 32 bits a [`Keyed`] reference holds.
#[doc(hidden)]
pub trait KeyWord {
    fn word(&self) -> u32;
}

impl KeyWord for u32 {
    fn word(&self) -> u32 {
        *self
    }
}

impl KeyWord for Hash32 {
    fn word(&self) -> u32 {
        self.get()
    }
}

/// An address the description does not follow: a runtime pointer, carried
/// through the tree and written as the canonical writer writes it.
impl Heap for Ptr<Opaque> {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl Owned for Ptr<Opaque> {
    type Raw = Ptr<Opaque>;
    const LEAF: bool = false;
    fn from_raw(raw: &Self, kids: Kids<'_>, at: usize, _: &Rx<'_>) -> Result<Self, Error> {
        match kid(kids, at) {
            Some(Value::UndescribedPtr(_)) | Some(Value::NotAnAddress(_)) | None => Ok(*raw),
            Some(Value::Ptr { target: None, .. }) => Ok(Plain::zeroed()),
            other => Err(shape("an unfollowed pointer", at, "an address", other)),
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, _: &Cx) -> Result<Self, Error> {
        kids.push((at as u32, Value::UndescribedPtr(self.raw())));
        Ok(*self)
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Ok(Value::UndescribedPtr(self.raw()))
    }
    fn run_to_value(items: &[Self], _: Option<usize>, _: &Cx) -> Result<Value, Error> {
        Ok(Value::Bytes(
            items.iter().flat_map(|p| p.raw().to_le_bytes()).collect(),
        ))
    }
}

/// A pointer at a member of the record that holds it, named by that member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member<T> {
    pub field: String,
    pub index: Option<usize>,
    _t: PhantomData<fn(&T)>,
}

impl<T> Default for Member<T> {
    fn default() -> Self {
        Self {
            field: String::new(),
            index: None,
            _t: PhantomData,
        }
    }
}

impl<T> Member<T> {
    pub fn to(field: &str) -> Self {
        Self {
            field: field.into(),
            index: None,
            _t: PhantomData,
        }
    }
}

impl<T> Heap for Member<T> {
    fn heap_bytes(&self) -> usize {
        self.field.capacity()
    }
}

impl<T: Owned> Owned for Member<T>
where
    T::Raw: 'static,
{
    type Raw = MemberPtr<T::Raw>;
    const LEAF: bool = false;
    fn from_raw(_: &Self::Raw, kids: Kids<'_>, at: usize, _: &Rx<'_>) -> Result<Self, Error> {
        match kid(kids, at) {
            Some(Value::Member { field, index }) => Ok(Self {
                field: field.clone(),
                index: *index,
                _t: PhantomData,
            }),
            other => Err(shape("a member pointer", at, "a member", other)),
        }
    }
    fn to_raw(&self, kids: &mut Vec<(u32, Value)>, at: usize, cx: &Cx) -> Result<Self::Raw, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn from_value(v: &Value, rx: &Rx<'_>) -> Result<Self, Error> {
        Self::from_raw(
            &Plain::zeroed(),
            std::slice::from_ref(&(0, v.clone())),
            0,
            rx,
        )
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Ok(Value::Member {
            field: self.field.clone(),
            index: self.index,
        })
    }
}

/// A byte payload in the graphics half, whose length another field states.
impl Heap for PhysPtr<[u8]> {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl Owned for PhysPtr<[u8]> {
    type Raw = PhysPtr<[u8]>;
    const LEAF: bool = false;
    fn from_raw(raw: &Self, _: Kids<'_>, _: usize, _: &Rx<'_>) -> Result<Self, Error> {
        Ok(*raw)
    }
    fn to_raw(&self, _: &mut Vec<(u32, Value)>, _: usize, _: &Cx) -> Result<Self, Error> {
        Ok(*self)
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Err(Error::Invalid {
            what: "a payload".into(),
            why: "not an element".into(),
        })
    }
}

/// A record's own bytes and children as a struct value of `type_name`.
pub fn record_value(
    cx: &Cx,
    type_name: &str,
    bytes: &[u8],
    kids: Vec<(u32, Value)>,
) -> Result<Value, Error> {
    cx.record(type_name, bytes.to_vec(), kids)
}

/// Rewrite each record of the run `v` holds or points at, in order, as its
/// raw `R`: how a `recompute` stamps a field derived across a run.
pub fn stamp_run<R: Plain>(
    v: &mut Value,
    mut f: impl FnMut(&mut R) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut stamp = |bytes: &mut [u8]| {
        let mut raw = R::read_from(bytes)
            .ok_or_else(|| shape(type_name::<R>(), 0, "a whole record", None))?;
        f(&mut raw)?;
        bytes[..std::mem::size_of::<R>()].copy_from_slice(raw.as_bytes());
        Ok(())
    };
    let v = match v {
        Value::Ptr {
            target: Some(t), ..
        } => t.as_mut(),
        other => other,
    };
    match v {
        Value::Ptr { target: None, .. } => Ok(()),
        Value::Struct(r) => stamp(r.bytes_mut()),
        Value::Records(run) => {
            let stride = run.stride().max(1);
            run.bytes_mut().chunks_exact_mut(stride).try_for_each(stamp)
        }
        Value::Array { elements, .. } => elements.iter_mut().try_for_each(|e| match e {
            Value::Struct(r) => stamp(r.bytes_mut()),
            other => Err(shape(type_name::<R>(), 0, "a struct", Some(other))),
        }),
        other => Err(shape(type_name::<R>(), 0, "a run of structs", Some(other))),
    }
}

/// A run of pointer-free records as one block.
pub fn records_value<T: Owned>(
    items: &[T],
    type_name: &str,
    capacity: Option<usize>,
    cx: &Cx,
) -> Result<Value, Error> {
    let mut bytes = Vec::with_capacity(items.len() * std::mem::size_of::<T::Raw>());
    let mut kids = Vec::new();
    for item in items {
        bytes.extend_from_slice(Plain::as_bytes(&item.to_raw(&mut kids, 0, cx)?));
    }
    cx.run(type_name, bytes, capacity)
}

/// What a struct carries from its file that its fields do not say: where it
/// was opened from, each counted array's capacity, each empty run the file
/// still pointed at, which pointer owns each target a slot reads by value,
/// and each field the declaration keeps private (padding, a
/// count, a reference count, a vft word or a runtime pointer), which the
/// description still names.
///
/// ⚠ A capacity is an allocation the game sized, so a save keeps it rather
/// than shrinking it to the length; it grows only when a `Vec` outgrows it.
#[derive(Debug, Clone, Default)]
pub struct Carry {
    origin: Origin,
    capacities: Vec<(u32, u16)>,
    hidden: Vec<(u32, Box<dyn Stash>)>,
    /// Per `String` or `Option<Box<T>>` slot the file filled with a word that is not an address, that word.
    words: Vec<(u32, u64)>,
    /// Each `#[len]` run the file pointed live at nothing beside a length of zero, and each
    /// `String` slot it pointed live at an empty string: a field is one or the other.
    empty_runs: Vec<u32>,
    /// Per slot that reads by value a target another pointer owns: its field, its index in a run, and that pointer's path.
    copies: Vec<(u32, Option<usize>, String)>,
    /// The address the file held this record at, which a [`Ref`] opened from it finds it by; zero for none.
    anchor: u64,
}

/// A private field's value as a [`Carry`] holds it.
#[doc(hidden)]
pub trait Stash: Any + std::fmt::Debug + Send + Sync + Heap {
    fn clone_box(&self) -> Box<dyn Stash>;
    fn as_any(&self) -> &dyn Any;
    /// Its own size and its heap: what the box holding it allocates.
    fn boxed_bytes(&self) -> usize;
}

impl<T: Any + Clone + std::fmt::Debug + Send + Sync + Heap> Stash for T {
    fn clone_box(&self) -> Box<dyn Stash> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn boxed_bytes(&self) -> usize {
        std::mem::size_of::<T>() + self.heap_bytes()
    }
}

impl Heap for Carry {
    fn heap_bytes(&self) -> usize {
        fn run<T>(v: &Vec<T>) -> usize {
            v.capacity() * std::mem::size_of::<T>()
        }
        let origin = (self.origin.path()).map_or(0, |p| {
            std::mem::size_of::<std::path::PathBuf>() + p.as_os_str().len()
        });
        let hidden: usize = self.hidden.iter().map(|(_, v)| v.boxed_bytes()).sum();
        let copies: usize = self.copies.iter().map(|(.., path)| path.capacity()).sum();
        origin
            + run(&self.capacities)
            + run(&self.hidden)
            + hidden
            + run(&self.words)
            + run(&self.empty_runs)
            + run(&self.copies)
            + copies
    }
}

impl Clone for Box<dyn Stash> {
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}

impl PartialEq for Carry {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Carry {
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub fn origin_mut(&mut self) -> &mut Origin {
        &mut self.origin
    }

    #[doc(hidden)]
    pub fn __anchor(&mut self, address: u64) {
        self.anchor = address;
    }

    #[doc(hidden)]
    pub fn __anchored(&self) -> u64 {
        self.anchor
    }

    /// Stop keeping the private field at `at`, which a save then writes as it writes a fresh value's.
    #[cfg(feature = "gta5")]
    pub(crate) fn forget(&mut self, at: usize) {
        self.hidden.retain(|(o, _)| *o as usize != at);
    }

    /// Keep the private field at `at` (within its struct), which a save writes unchecked.
    pub(crate) fn hide<V: Stash>(&mut self, at: usize, value: V) {
        match self.hidden.iter_mut().find(|(o, _)| *o as usize == at) {
            Some((_, v)) => *v = Box::new(value),
            None => self.hidden.push((at as u32, Box::new(value))),
        }
    }

    /// The private field at `at`, as it was read or last kept; `None` for a value built from scratch.
    pub(crate) fn hidden<V: 'static>(&self, at: usize) -> Option<&V> {
        self.hidden
            .iter()
            .find(|(o, _)| *o as usize == at)
            .and_then(|(_, v)| (**v).as_any().downcast_ref())
    }

    /// The bit that keys a `#[derived]` field's opening derivation apart from its stated value, both kept by offset.
    const OPENED: usize = 1 << 31;

    /// Keep what the `#[derived]` field at `at` derived to when its record was opened.
    #[doc(hidden)]
    pub fn __keep_derived<V: Stash + Clone>(&mut self, at: usize, derived: Option<V>) {
        self.hide(at | Self::OPENED, derived);
    }

    /// What the `#[derived]` field at `at` saves as: its derivation, or what the file stated while the derivation is what it was at open.
    ///
    /// ⚠ A stated value that disagrees with a changed derivation is refused,
    /// never rewritten: the edit that moved the derivation left the file's
    /// other statements about the same bytes behind.
    #[doc(hidden)]
    pub fn __derive<V: Stash + Clone + PartialEq>(
        &self,
        at: usize,
        now: Option<V>,
        fresh: impl FnOnce() -> V,
        what: &str,
    ) -> Result<V, Error> {
        let stated = self.hidden::<V>(at);
        match (stated, now) {
            (Some(s), Some(d)) if *s != d => {
                let opened = self.hidden::<Option<V>>(at | Self::OPENED);
                if opened == Some(&Some(d.clone())) {
                    return Ok(s.clone());
                }
                Err(Error::Invalid {
                    what: what.to_string(),
                    why: format!(
                        "the file states {s:?}, and what it is derived from now derives {d:?}"
                    ),
                })
            }
            (_, Some(d)) => Ok(d),
            (Some(s), None) => Ok(s.clone()),
            (None, None) => Ok(fresh()),
        }
    }

    /// Keep the word that is not an address in the pointer slot at `at` (within its struct), which reads as empty.
    pub(crate) fn keep_word(&mut self, at: usize, word: u64) {
        self.words.push((at as u32, word));
    }

    /// Write back the word kept for the shared slot just written at `abs`, or point the slot at the run it shares when `cx` repairs.
    ///
    /// ⚠ The slot is a second pointer at an allocation another one owns, which the game fills in at load, so
    /// the word it held in the file is stale and a repair names the address it should hold.
    pub(crate) fn put_stale_word(
        &self,
        what: &str,
        at: usize,
        abs: usize,
        kids: &mut [(u32, Value)],
        cx: &Cx,
    ) {
        let Some(&(_, word)) = self.words.iter().find(|(o, _)| *o as usize == at) else {
            return;
        };
        if cx.repair() == Repair::Lossless {
            cx.note_repair(format!(
                "{what}: {word:#x} is not an address, now the address the run shares"
            ));
            return;
        }
        if let Some((_, kid)) = kids.iter_mut().rev().find(|(o, _)| *o as usize == abs) {
            *kid = Value::NotAnAddress(word);
        }
    }

    /// Write back the word kept for the slot at `at` (absolute `abs`) where the slot is still empty.
    pub(crate) fn put_word(&self, at: usize, abs: usize, kids: &mut [(u32, Value)]) {
        let Some(&(_, word)) = self.words.iter().find(|(o, _)| *o as usize == at) else {
            return;
        };
        if let Some((_, kid)) = kids.iter_mut().rev().find(|(o, _)| *o as usize == abs) {
            if matches!(kid, Value::Ptr { target: None, .. }) {
                *kid = Value::NotAnAddress(word);
            }
        }
    }

    /// Keep whether the string slot at `at` (within its struct, absolute `abs`) points live at an empty string.
    pub(crate) fn keep_empty_string(&mut self, at: usize, kids: Kids<'_>, abs: usize) {
        if let Some(Value::Ptr {
            target: Some(t), ..
        }) = kid(kids, abs)
        {
            if matches!(t.as_ref(), Value::Bytes(b) if b.first().is_none_or(|&c| c == 0)) {
                self.empty_runs.push(at as u32);
            }
        }
    }

    /// Point the string slot just written at `abs` at an empty string again where it was read so and is still empty.
    pub(crate) fn put_empty_string(&self, at: usize, abs: usize, kids: &mut [(u32, Value)]) {
        if !self.empty_runs.contains(&(at as u32)) {
            return;
        }
        if let Some((_, kid)) = kids.iter_mut().rev().find(|(o, _)| *o as usize == abs) {
            if matches!(kid, Value::Ptr { target: None, .. }) {
                *kid = Value::ptr_to(Value::Bytes(vec![0]));
            }
        }
    }

    /// Whether the run at `at` (within its struct, absolute `abs`) points live at nothing, which the carry keeps.
    ///
    /// ⚠ Beside a stated length of zero (`zero`) a pointer at what another pointer owns counts too: no element is
    /// read through it, and in the files that do this its target is not a run of this type at all.
    pub(crate) fn keep_empty_run(
        &mut self,
        at: usize,
        kids: Kids<'_>,
        abs: usize,
        zero: bool,
        rx: &Rx<'_>,
    ) -> bool {
        let live = live_empty_run(kid(kids, abs))
            || (zero && matches!(kid(kids, abs), Some(Value::Alias(a)) if !rx.is_lent(*a)));
        if live {
            self.empty_runs.push(at as u32);
        }
        live
    }

    /// Point the run of `E` just written at `abs` live at nothing again where it was read so and is still empty.
    pub(crate) fn put_empty_run<E: Owned>(
        &self,
        at: usize,
        abs: usize,
        kids: &mut [(u32, Value)],
        cx: &Cx,
    ) -> Result<(), Error> {
        if !self.empty_runs.contains(&(at as u32)) {
            return Ok(());
        }
        if let Some((_, kid)) = kids.iter_mut().rev().find(|(o, _)| *o as usize == abs) {
            if matches!(kid, Value::Ptr { target: None, .. }) {
                *kid = Value::ptr_to(E::run_to_value(&[], None, cx)?);
            }
        }
        Ok(())
    }

    /// The slots of the field at `at` (within its struct) read by value, with their owners' paths.
    fn copies_at(&self, at: usize) -> Copies {
        self.copies
            .iter()
            .filter(|(o, ..)| *o as usize == at)
            .map(|(_, i, path)| (*i, path.clone()))
            .collect()
    }

    /// Remember the capacity the counted array at `at` (within its struct) was read with.
    pub fn keep_capacity<T>(&mut self, at: usize, raw: &Array<T>) {
        // An empty array with a live pointer keeps its allocation too.
        if raw.capacity() != raw.count() || (raw.count() == 0 && raw.raw() != 0) {
            self.capacities.push((at as u32, raw.capacity() as u16));
        }
    }

    /// Give the counted array of `E` just written at `at` (absolute `abs`)
    /// its carried capacity; an empty one keeps its allocation.
    pub fn apply_capacity<E: Owned, T>(
        &self,
        at: usize,
        abs: usize,
        kids: &mut [(u32, Value)],
        raw: &mut Array<T>,
        cx: &Cx,
    ) -> Result<(), Error> {
        let Some(&(_, cap)) = self.capacities.iter().find(|(o, _)| *o as usize == at) else {
            return Ok(());
        };
        let cap = cap.max(raw.count() as u16);
        *raw = Array::with_capacity(raw.count() as u16, cap);
        let Some((_, kid)) = kids.iter_mut().rev().find(|(o, _)| *o as usize == abs) else {
            return Ok(());
        };
        if matches!(kid, Value::Ptr { target: None, .. }) {
            *kid = Value::ptr_to(E::run_to_value(&[], Some(cap as usize), cx)?);
        }
        if let Value::Ptr {
            target: Some(run), ..
        } = kid
        {
            match run.as_mut() {
                Value::Array { capacity, .. } => *capacity = Some(cap as usize),
                Value::Records(r) => r.set_capacity(Some(cap as usize)),
                // Bytes state no capacity, and an empty run of scalars has no element to hold one.
                Value::Bytes(b) if b.is_empty() => {
                    **run = Value::Array {
                        elements: Vec::new(),
                        capacity: Some(cap as usize),
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
