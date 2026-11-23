//! A shop's own metadata: the apparel files a server wrote for its shop, read, resolved to garments and written back.
//!
//! The ids such a file states (`uniqueNameHash`) are what a script, a saved outfit or a shop table of
//! that server is keyed on, so they are kept exactly as written, as text, and never read as a number:
//! they are names the game hashes or literal values, and the staged server's run to 12 hexadecimal
//! digits, which a 32 bit hash does not hold. An entry is resolved to a garment and a texture by the
//! ped, the slot, the drawable and the texture it states. What resolves becomes the default shop id
//! of that texture; what does not is kept, listed and written back, never dropped and never guessed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use or_io::meta::Anchor;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::alloc::{allocate, Allocation, Placed};
use crate::import::LeftOut;
use crate::model::{Carried, Garment, ShopEntry, Stated, Wardrobe};
use crate::names::{Component, Ped, Prop, Slot};

/// The root element of a shop's apparel metadata.
pub const ROOT: &str = "ShopPedApparel";
/// The type a manifest registers a shop's apparel metadata under.
pub const META_TYPE: &str = "SHOP_PED_APPAREL_META_FILE";
/// What every type a manifest registers with the shop begins with.
const SHOP_TYPES: &str = "SHOP_PED_APPAREL";

// ---------------------------------------------------------------------------------------------
// A small XML tree, because an entry carries elements the engine does not name.

/// An element, with the text it holds, its attributes and the elements inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The text of the child `name`, or its `value` where it states one.
    pub(crate) fn value_of(&self, name: &str) -> Option<String> {
        let child = self.child(name)?;
        match child.attrs.iter().find(|(k, _)| k == "value") {
            Some((_, v)) => Some(v.clone()),
            None => Some(child.text.trim().to_string()),
        }
    }
}

fn start_of(e: &BytesStart<'_>) -> Result<Node, String> {
    let mut node = Node {
        name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
        ..Node::default()
    };
    for attr in e.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        let value = attr
            .unescape_value()
            .map(|v| v.into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).into_owned());
        node.attrs.push((
            String::from_utf8_lossy(attr.key.as_ref()).into_owned(),
            value,
        ));
    }
    Ok(node)
}

/// The tree of an XML document: its root element.
pub fn parse_tree(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut attach = |stack: &mut Vec<Node>, done: Node| match stack.last_mut() {
        Some(parent) => {
            parent.children.push(done);
            Ok(())
        }
        None if root.is_none() => {
            root = Some(done);
            Ok(())
        }
        None => Err("more than one root element".to_string()),
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(start_of(&e)?),
            Ok(Event::Empty(e)) => {
                let node = start_of(&e)?;
                attach(&mut stack, node)?;
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().ok_or("a closing tag with nothing open")?;
                attach(&mut stack, node)?;
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.decode().map_err(|e| e.to_string())?);
                }
            }
            Ok(Event::CData(c)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(&c));
                }
            }
            // ⚠ quick-xml 0.38 hands an entity reference over as an event of its own.
            Ok(Event::GeneralRef(r)) => {
                if let Some(top) = stack.last_mut() {
                    let name = r.decode().map_err(|e| e.to_string())?;
                    match r.resolve_char_ref() {
                        Ok(Some(ch)) => top.text.push(ch),
                        _ => match quick_xml::escape::resolve_predefined_entity(&name) {
                            Some(text) => top.text.push_str(text),
                            None => top.text.push_str(&format!("&{name};")),
                        },
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("{e} at byte {}", reader.error_position())),
        }
    }
    if let Some(open) = stack.last() {
        return Err(format!("`{}` is not closed", open.name));
    }
    root.ok_or_else(|| "there is no element".to_string())
}

/// `text` with the five characters XML reserves escaped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn open_tag(node: &Node) -> String {
    let mut tag = format!("<{}", node.name);
    for (k, v) in &node.attrs {
        tag.push_str(&format!(" {k}=\"{}\"", escape(v)));
    }
    tag
}

/// The element on one line, with no space between its tags.
pub(crate) fn compact(node: &Node) -> String {
    let tag = open_tag(node);
    if node.children.is_empty() && node.text.is_empty() {
        return format!("{tag}/>");
    }
    let inner: String = node.children.iter().map(compact).collect();
    format!("{tag}>{}{inner}</{}>", escape(&node.text), node.name)
}

/// The element indented by `indent` spaces, a line to each element that holds others.
fn pretty(node: &Node, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    let tag = open_tag(node);
    if node.children.is_empty() {
        match node.text.is_empty() {
            true => out.push_str(&format!("{pad}{tag}/>\n")),
            false => out.push_str(&format!(
                "{pad}{tag}>{}</{}>\n",
                escape(&node.text),
                node.name
            )),
        }
        return;
    }
    out.push_str(&format!("{pad}{tag}>\n"));
    for child in &node.children {
        pretty(child, indent + 2, out);
    }
    out.push_str(&format!("{pad}</{}>\n", node.name));
}

// ---------------------------------------------------------------------------------------------
// A shop file.

/// One entry as the file states it, before it is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw {
    pub prop: bool,
    /// `eCompType` or `eAnchorPoint` as written.
    pub kind: String,
    /// `localDrawableIndex` or `localPropIndex`.
    pub drawable: u32,
    pub texture: u32,
    pub entry: ShopEntry,
}

/// A shop apparel file read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopFile {
    /// Where it was read from, as the import saw its path.
    pub shown: String,
    /// The freemode ped its `pedName` names, none for any other.
    pub ped: Option<Ped>,
    pub entries: Vec<Raw>,
    /// Entries that could not be read as one, with the reason.
    pub unread: Vec<LeftOut>,
}

/// Whether the start of a file says its root element is the shop apparel's.
pub fn looks_like_shop(head: &[u8]) -> bool {
    let head = &head[..head.len().min(2048)];
    String::from_utf8_lossy(head).contains(&format!("<{ROOT}"))
}

fn number(node: &Node, name: &str) -> Option<u32> {
    node.value_of(name)?.trim().parse().ok()
}

/// Read a shop apparel file: its ped, and each entry of its two sections with the id kept as written.
pub fn parse(text: &str, shown: &str) -> Result<ShopFile, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let root = parse_tree(text)?;
    if root.name != ROOT {
        return Err(format!(
            "its root element is `{}` and a shop's is `{ROOT}`",
            root.name
        ));
    }
    let ped = root.value_of("pedName").and_then(|n| Ped::from_model(&n));
    let mut file = ShopFile {
        shown: shown.to_string(),
        ped,
        entries: Vec::new(),
        unread: Vec::new(),
    };
    let mut number_of_entry = 0u32;
    for (section, prop) in [("pedComponents", false), ("pedProps", true)] {
        let Some(section) = root.child(section) else {
            continue;
        };
        for item in section.children.iter().filter(|c| c.name == "Item") {
            let at = number_of_entry;
            number_of_entry += 1;
            let (kind_name, index_name) = match prop {
                false => ("eCompType", "localDrawableIndex"),
                true => ("eAnchorPoint", "localPropIndex"),
            };
            let stated = (
                item.value_of("uniqueNameHash"),
                item.value_of(kind_name),
                number(item, index_name),
                number(item, "textureIndex"),
            );
            let (Some(id), Some(kind), Some(drawable), Some(texture)) = stated else {
                file.unread.push(LeftOut {
                    path: format!("{shown}, entry {at}"),
                    why: format!(
                        "it does not state all of uniqueNameHash, {kind_name}, {index_name} and textureIndex"
                    ),
                });
                continue;
            };
            let known = [
                "cost",
                "uniqueNameHash",
                kind_name,
                index_name,
                "textureIndex",
            ];
            file.entries.push(Raw {
                prop,
                kind,
                drawable,
                texture,
                entry: ShopEntry {
                    id,
                    from: shown.to_string(),
                    entry: at,
                    cost: item.value_of("cost"),
                    fields: item
                        .children
                        .iter()
                        .filter(|c| !known.contains(&c.name.as_str()))
                        .map(compact)
                        .collect(),
                },
            });
        }
    }
    Ok(file)
}

// ---------------------------------------------------------------------------------------------
// A manifest.

/// A file a manifest registers: the line `<directive> '<kind>' '<path>'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub directive: String,
    pub kind: String,
    pub path: String,
}

impl Registered {
    /// Whether it is registered with the shop.
    pub fn is_shop(&self) -> bool {
        self.kind.starts_with(SHOP_TYPES)
    }
}

/// The lines of a Lua manifest that register a file, whichever directive they use (`data_file`, or a server's own ending in it).
pub fn registered(manifest: &str) -> Vec<Registered> {
    // Comments go first, so a registration that is switched off is not read.
    let mut code = String::with_capacity(manifest.len());
    let chars: Vec<char> = manifest.chars().collect();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                code.push(c);
                if c == '\\' && i + 1 < chars.len() {
                    code.push(chars[i + 1]);
                    i += 1;
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '-' && chars.get(i + 1) == Some(&'-') => {
                if chars.get(i + 2) == Some(&'[') && chars.get(i + 3) == Some(&'[') {
                    let end = chars[i..].windows(2).position(|w| w == [']', ']']);
                    i += end.map_or(chars.len() - i, |e| e + 1);
                } else {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                }
                code.push(c);
            }
        }
        i += 1;
    }

    let chars: Vec<char> = code.chars().collect();
    let skip = |mut at: usize, set: &[char]| {
        while chars
            .get(at)
            .is_some_and(|c| c.is_whitespace() || set.contains(c))
        {
            at += 1;
        }
        at
    };
    let string_at = |at: usize| -> Option<(String, usize)> {
        let q = *chars.get(at).filter(|c| **c == '\'' || **c == '"')?;
        let mut out = String::new();
        let mut at = at + 1;
        while let Some(&c) = chars.get(at) {
            at += 1;
            match c {
                '\\' => {
                    out.push(*chars.get(at)?);
                    at += 1;
                }
                c if c == q => return Some((out, at)),
                c => out.push(c),
            }
        }
        None
    };
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if !(chars[at].is_ascii_alphanumeric() || chars[at] == '_') {
            at += 1;
            continue;
        }
        let start = at;
        while at < chars.len() && (chars[at].is_ascii_alphanumeric() || chars[at] == '_') {
            at += 1;
        }
        let word: String = chars[start..at].iter().collect();
        if !word.ends_with("data_file") {
            continue;
        }
        let first = skip(at, &['(']);
        let Some((kind, after)) = string_at(first) else {
            continue;
        };
        let second = skip(after, &[',', ')', '(']);
        let Some((path, after)) = string_at(second) else {
            continue;
        };
        found.push(Registered {
            directive: word,
            kind,
            path,
        });
        at = after;
    }
    found
}

fn is_directive(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------------------------
// Finding the shop's files among the files offered.

/// A file offered to an import or a drop: the path its name is read from, and where it is on the disk.
#[derive(Debug, Clone)]
pub struct Offered {
    pub shown: String,
    pub path: PathBuf,
}

/// What the files offered say of the shop.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub shops: Vec<ShopFile>,
    /// The shop apparel files read: the build writes the metadata again from the pack, so these are not carried.
    pub consumed: Vec<String>,
    /// Files registered with the shop that this engine does not read, by the path they were offered as.
    pub carried: Vec<(String, Registered)>,
    /// What could not be read or found, with the reason.
    pub unread: Vec<LeftOut>,
}

fn normal(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}

fn parent_of(shown: &str) -> &str {
    shown.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Find the shop apparel files among `offered`: those a manifest registers as shop apparel data and those whose root is the shop's element; and the other files a manifest registers with the shop.
pub fn discover(offered: &[Offered]) -> Found {
    let mut found = Found::default();
    let by_name: BTreeMap<String, &Offered> =
        offered.iter().map(|o| (normal(&o.shown), o)).collect();

    // Registered files, from every manifest offered.
    let mut registered_files: Vec<(&Offered, Registered)> = Vec::new();
    for manifest in offered.iter().filter(|o| {
        let name = o
            .shown
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        name == "fxmanifest.lua" || name == "__resource.lua"
    }) {
        let Ok(text) = std::fs::read_to_string(&manifest.path) else {
            continue;
        };
        let dir = parent_of(&manifest.shown);
        for line in registered(&text).into_iter().filter(Registered::is_shop) {
            let joined = match dir.is_empty() {
                true => line.path.clone(),
                false => format!("{dir}/{}", line.path),
            };
            match by_name.get(&normal(&joined)) {
                Some(file) => registered_files.push((file, line)),
                None => found.unread.push(LeftOut {
                    path: format!("{}: {}", manifest.shown, line.path),
                    why: format!(
                        "the manifest registers it as {} and it is not among the files offered",
                        line.kind
                    ),
                }),
            }
        }
    }

    let mut read: BTreeSet<String> = BTreeSet::new();
    let mut candidates: Vec<&Offered> = registered_files
        .iter()
        .filter(|(_, line)| line.kind == META_TYPE)
        .map(|(file, _)| *file)
        .collect();
    for o in offered {
        let ext = Path::new(&o.shown)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(ext.as_str(), "meta" | "xml") && head_is_shop(&o.path) {
            candidates.push(o);
        }
    }
    for o in candidates {
        if !read.insert(normal(&o.shown)) {
            continue;
        }
        let text = match std::fs::read(&o.path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                found.unread.push(LeftOut {
                    path: o.shown.clone(),
                    why: format!("it cannot be read: {e}"),
                });
                continue;
            }
        };
        match parse(&text, &o.shown) {
            Ok(file) if file.ped.is_some() => {
                found.consumed.push(o.shown.clone());
                found.shops.push(file);
            }
            Ok(_) => match registered_files.iter().find(|(f, _)| f.shown == o.shown) {
                Some((_, line)) => found.carried.push((o.shown.clone(), line.clone())),
                None => found.unread.push(LeftOut {
                    path: o.shown.clone(),
                    why: "a shop apparel file of a ped that is not one of the two freemode peds, which a manifest does not register".into(),
                }),
            },
            Err(why) => found.unread.push(LeftOut {
                path: o.shown.clone(),
                why: format!("it does not read as shop apparel metadata: {why}"),
            }),
        }
    }

    // The other files registered with the shop travel as they are.
    for (file, line) in registered_files {
        if read.contains(&normal(&file.shown)) {
            continue;
        }
        if !is_directive(&line.directive) {
            found.unread.push(LeftOut {
                path: file.shown.clone(),
                why: format!("its manifest line starts with `{}`", line.directive),
            });
        } else if !found.carried.iter().any(|(shown, _)| *shown == file.shown) {
            found.carried.push((file.shown.clone(), line));
        }
    }
    // ⚠ In the order of their paths and not of the manifest's lines, so that the pack an import makes of a build is the pack the build was made from.
    found.shops.sort_by(|a, b| a.shown.cmp(&b.shown));
    found.carried.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn head_is_shop(path: &Path) -> bool {
    use std::io::Read;
    let mut head = vec![0u8; 2048];
    std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .is_ok_and(|n| looks_like_shop(&head[..n]))
}

/// The path a carried file ships at in a resource.
pub fn carried_path(carried: &Carried) -> String {
    let name = carried.file.rsplit('/').next().unwrap_or(&carried.file);
    format!("data/{name}")
}

// ---------------------------------------------------------------------------------------------
// Resolving entries to garments.

/// What an import or a drop made of the shop files it was offered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShopReport {
    /// The shop apparel files read.
    pub files: Vec<String>,
    /// The entries they hold.
    pub entries: usize,
    /// The entries that became the id of a garment's texture.
    pub resolved: usize,
    /// Entries that point at no garment or texture, kept in the pack and written back.
    pub unplaced: Vec<LeftOut>,
    /// Garments with no entry for one or more of their textures, which the build names.
    pub unentried: Vec<LeftOut>,
    /// Ids that two entries claim.
    pub clashes: Vec<LeftOut>,
    /// Files the manifest registers with the shop, carried unchanged.
    pub carried: Vec<LeftOut>,
    /// What could not be read or found.
    pub unread: Vec<LeftOut>,
}

impl ShopReport {
    /// Whether the files offered held no shop metadata at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Everything the report lists, as the lines a log or a terminal shows.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (what, list) in [
            ("points at nothing in the pack, kept", &self.unplaced),
            ("has no shop entry", &self.unentried),
            ("claims an id another entry claims", &self.clashes),
            ("carried unchanged", &self.carried),
            ("not read", &self.unread),
        ] {
            for item in list {
                lines.push(format!("{}: {what}: {}", item.path, item.why));
            }
        }
        lines
    }
}

/// The slot an entry's `eCompType` or `eAnchorPoint` names.
pub fn slot_of(kind: &str, prop: bool) -> Option<Slot> {
    match prop {
        true => Prop::ALL
            .into_iter()
            .find(|p| {
                Anchor(p.anchor())
                    .name()
                    .is_some_and(|n| n.eq_ignore_ascii_case(kind))
            })
            .map(Slot::Prop),
        false => Component::ALL
            .into_iter()
            .find(|c| {
                kind.eq_ignore_ascii_case(&format!("PV_COMP_{}", c.name().to_ascii_uppercase()))
            })
            .map(Slot::Component),
    }
}

/// The `eCompType` or `eAnchorPoint` text of a slot.
pub fn kind_of(slot: Slot) -> String {
    match slot {
        Slot::Component(c) => format!("PV_COMP_{}", c.name().to_ascii_uppercase()),
        Slot::Prop(p) => Anchor(p.anchor())
            .name()
            .unwrap_or("ANCHOR_HEAD")
            .to_string(),
    }
}

fn describe(ped: Ped, raw: &Raw) -> String {
    format!(
        "{ped} {} {}, texture {}",
        raw.kind, raw.drawable, raw.texture
    )
}

/// The ped, slot and drawable number that an entry finds a garment by.
pub type Place = (Ped, Slot, u32);

/// Resolve the entries of `files` against the garments of `wardrobe`, keeping them.
///
/// `place_of` says the ped, slot and drawable a garment stands for, which is its place in the
/// collection and not always the position the allocator gives it: a dropped garment is at the
/// number its file name states. An entry whose texture another entry already took, whose slot is
/// none, or that points at no garment or no such texture is kept in the pack as it was stated.
pub fn apply(
    wardrobe: &mut Wardrobe,
    files: &[ShopFile],
    place_of: &dyn Fn(&Wardrobe, usize) -> Option<Place>,
) -> ShopReport {
    let mut report = ShopReport::default();
    let from: BTreeSet<&str> = files.iter().map(|f| f.shown.as_str()).collect();

    // Importing a file again replaces what the first import of it kept.
    for garment in &mut wardrobe.garments {
        for v in &mut garment.variations {
            if v.shop
                .as_ref()
                .is_some_and(|e| from.contains(e.from.as_str()))
            {
                v.shop = None;
            }
        }
    }
    wardrobe
        .shop
        .unplaced
        .retain(|s| !from.contains(s.entry.from.as_str()));

    let mut places: BTreeMap<(Ped, Slot, u32), usize> = BTreeMap::new();
    for index in 0..wardrobe.garments.len() {
        if let Some(key) = place_of(wardrobe, index) {
            places.entry(key).or_insert(index);
        }
    }
    let mut took: BTreeSet<(usize, usize)> = BTreeSet::new();
    for file in files {
        report.files.push(file.shown.clone());
        report.unread.extend(file.unread.iter().cloned());
        let Some(ped) = file.ped else { continue };
        for raw in &file.entries {
            report.entries += 1;
            let at = format!("{}, entry {}", file.shown, raw.entry.entry);
            let target = slot_of(&raw.kind, raw.prop).and_then(|slot| {
                let index = *places.get(&(ped, slot, raw.drawable))?;
                let texture = usize::try_from(raw.texture).ok()?;
                (texture < wardrobe.garments[index].variations.len()).then_some((index, texture))
            });
            let why = match target {
                Some((index, texture)) if took.insert((index, texture)) => {
                    wardrobe.garments[index].variations[texture].shop = Some(raw.entry.clone());
                    report.resolved += 1;
                    continue;
                }
                Some(_) => "a second entry for a texture another entry took".to_string(),
                None if slot_of(&raw.kind, raw.prop).is_none() => {
                    format!("`{}` is no slot of a freemode ped", raw.kind)
                }
                None => format!(
                    "the pack has no such drawable and texture: {}",
                    describe(ped, raw)
                ),
            };
            report.unplaced.push(LeftOut {
                path: at,
                why: format!("{why} (id {})", raw.entry.id),
            });
            wardrobe.shop.unplaced.push(Stated {
                ped,
                kind: raw.kind.clone(),
                prop: raw.prop,
                drawable: raw.drawable,
                texture: raw.texture,
                entry: raw.entry.clone(),
            });
        }
    }

    // A garment of a ped whose shop was read, with a texture no entry named.
    let read_for: BTreeSet<Ped> = files.iter().filter_map(|f| f.ped).collect();
    for garment in &wardrobe.garments {
        if garment.is_reserved() || !garment.ped.is_some_and(|p| read_for.contains(&p)) {
            continue;
        }
        let missing: Vec<String> = garment
            .variations
            .iter()
            .enumerate()
            .filter(|(_, v)| v.shop.is_none())
            .map(|(i, _)| char::from(b'a' + u8::try_from(i).unwrap_or(25).min(25)).to_string())
            .collect();
        if !missing.is_empty() {
            report.unentried.push(LeftOut {
                path: crate::rules::name_of(garment),
                why: format!(
                    "texture(s) {} of {} have no entry in the shop metadata, so the build names them",
                    missing.join(", "),
                    garment.variations.len()
                ),
            });
        }
    }

    let allocation = allocate(wardrobe);
    for (id, owners) in clashes(wardrobe, &allocation) {
        report.clashes.push(LeftOut {
            path: id,
            why: format!("claimed by {}", owners.join(" and ")),
        });
    }
    report
}

// ---------------------------------------------------------------------------------------------
// The ids a build writes.

/// One entry a build writes: where it stands, and the id it is written with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub ped: Ped,
    /// Which collection of the ped it is in: 0 is the pack's own.
    pub part: u32,
    /// The garment it belongs to, none for an entry kept because it points at no garment.
    pub garment: Option<usize>,
    /// Whether it is in the props of the file; the components otherwise.
    pub prop: bool,
    /// `eCompType` or `eAnchorPoint` as the file states it.
    pub kind: String,
    pub position: u32,
    pub texture: usize,
    pub id: String,
}

/// The id texture `texture` of `garment`, placed at `placed`, is written with: the one somebody chose, else the one an import read, else the name a build gives.
pub fn id_of(wardrobe: &Wardrobe, garment: &Garment, placed: &Placed, texture: usize) -> String {
    match garment
        .variations
        .get(texture)
        .and_then(|v| v.stated_shop_id())
    {
        Some(id) => id.to_string(),
        None => crate::metadata::shop_name(wardrobe, placed, texture),
    }
}

/// Whether the build writes the entry of texture `texture` of `garment`: a garment that ships a model has one for each texture, and a place held has the ones an import read.
pub fn is_written(garment: &Garment, texture: usize) -> bool {
    !garment.is_reserved()
        || garment
            .variations
            .get(texture)
            .is_some_and(|v| v.shop.is_some() || v.shop_id.is_some())
}

/// Every entry the build writes for the pack, the ones kept for want of a garment last.
pub fn written(wardrobe: &Wardrobe, allocation: &Allocation) -> Vec<Written> {
    let mut out = Vec::new();
    for p in &allocation.placed {
        let garment = &wardrobe.garments[p.garment];
        for texture in 0..garment.variations.len() {
            if is_written(garment, texture) {
                out.push(Written {
                    ped: p.ped,
                    part: p.part,
                    garment: Some(p.garment),
                    prop: p.slot.is_prop(),
                    kind: kind_of(p.slot),
                    position: p.position,
                    texture,
                    id: id_of(wardrobe, garment, p, texture),
                });
            }
        }
    }
    for s in &wardrobe.shop.unplaced {
        out.push(Written {
            ped: s.ped,
            part: 0,
            garment: None,
            prop: s.prop,
            kind: s.kind.clone(),
            position: s.drawable,
            texture: s.texture as usize,
            id: s.entry.id.clone(),
        });
    }
    out
}

/// The ids two or more entries of the build claim, each with who claims it.
///
/// Compared without regard to case, as the game hashes a name.
pub fn clashes(wardrobe: &Wardrobe, allocation: &Allocation) -> Vec<(String, Vec<String>)> {
    let mut by_id: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for w in written(wardrobe, allocation) {
        let who = match w.garment {
            Some(g) => format!(
                "{}, texture {}",
                crate::rules::name_of(&wardrobe.garments[g]),
                w.texture
            ),
            None => format!(
                "an entry kept for want of a garment ({} {})",
                w.ped, w.position
            ),
        };
        by_id
            .entry(w.id.to_ascii_lowercase())
            .or_insert_with(|| (w.id.clone(), Vec::new()))
            .1
            .push(who);
    }
    by_id
        .into_values()
        .filter(|(_, owners)| owners.len() > 1)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Writing the entries.

/// The text of one entry, indented as the entries of a shop file are.
pub fn write_entry(
    out: &mut String,
    prop: bool,
    id: &str,
    kind: &str,
    drawable: u32,
    texture: usize,
    stated: Option<&ShopEntry>,
) {
    let cost = stated.and_then(|e| e.cost.as_deref()).unwrap_or("0");
    let fields: Vec<Node> = stated
        .map(|e| e.fields.iter().filter_map(|f| parse_tree(f).ok()).collect())
        .unwrap_or_default();
    out.push_str("    <Item>\n");
    out.push_str(&format!("      <cost value=\"{}\"/>\n", escape(cost)));
    out.push_str(&format!(
        "      <uniqueNameHash>{}</uniqueNameHash>\n",
        escape(id)
    ));
    if prop {
        match fields.iter().find(|f| f.name == "restrictionTags") {
            Some(tags) => pretty(tags, 6, out),
            None => out.push_str("      <restrictionTags/>\n"),
        }
        out.push_str(&format!(
            "      <localPropIndex value=\"{drawable}\"/>\n      <eAnchorPoint>{}</eAnchorPoint>\n",
            escape(kind)
        ));
    } else {
        out.push_str(&format!(
            "      <localDrawableIndex value=\"{drawable}\"/>\n      <eCompType>{}</eCompType>\n",
            escape(kind)
        ));
    }
    out.push_str(&format!("      <textureIndex value=\"{texture}\"/>\n"));
    for field in fields
        .iter()
        .filter(|f| !(prop && f.name == "restrictionTags"))
    {
        pretty(field, 6, out);
    }
    out.push_str("    </Item>\n");
}

/// What a shop file points at: the section, the kind, the drawable and the texture of an entry.
pub type Key = (bool, String, u32, u32);

/// The key of an entry in the form a file read back is compared by.
pub fn key_of(prop: bool, kind: &str, drawable: u32, texture: u32) -> Key {
    (prop, kind.to_ascii_uppercase(), drawable, texture)
}

/// The shop file `text` read back for the check that a build wrote what the plan said: the id of each entry by what it points at, in the order the file has them.
pub fn ids_in(text: &str) -> Result<Vec<(Key, String)>, String> {
    let file = parse(text, "the shop metadata")?;
    Ok(file
        .entries
        .into_iter()
        .map(|r| (key_of(r.prop, &r.kind, r.drawable, r.texture), r.entry.id))
        .collect())
}
