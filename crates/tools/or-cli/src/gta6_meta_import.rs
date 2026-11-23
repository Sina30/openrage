//! `rpf scan-from-xml`: a scanned or loose GTA VI headerless metadata body with the tree an XML
//! file states written in, its arrays grown or shrunk to it as the app's Save As does, as a loose
//! resource that `rpf scan-open` reads at offset 0.

use std::fs;
use std::io::Cursor;

use or_io::container::rpf::{read_rpf9_body, write_rpf9_resource, Rpf9Sections};
use or_io::meta::binary::layout::ContainerLayout;
use or_io::meta::binary::rbm::gta6::open;
use or_io::meta::binary::rbm::gta6_grow_tree::write_grown;
use or_io::meta::{write_meta, MetaDocument, MetaInstance};

const USAGE: &str = "usage: rpf scan-from-xml <archive.rpf|file> <offset> <edited.xml> <out> \
                     [--limit <bytes>]";

pub(crate) fn cmd_scan_from_xml(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let [path, offset, xml, out] = args.as_slice() else {
        return Err(USAGE.to_string());
    };
    let offset = super::rpf::parse_offset(offset)?;
    if super::navmesh_edit::same_file(path, out) {
        return Err(format!("the output is the input {path}: name another file"));
    }
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    let sections = Rpf9Sections::of(&find, &bytes)
        .filter(|s| s.virtual_section.get(0x10..0x14) == Some(b"0DRP"))
        .ok_or_else(|| format!("the body at {offset:#X} is no headerless GTA VI metadata body"))?;
    let text = fs::read(xml).map_err(|e| format!("read({xml}): {e}"))?;
    let (written, edited) = with_tree(&sections, text)?;
    let saved = write_rpf9_resource(&written).map_err(|e| e.to_string())?;
    // The file is written only once it reads back to the tree the XML states.
    let (find, bytes) = read_rpf9_body(Cursor::new(&saved), 0, u64::MAX)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    let back = Rpf9Sections::of(&find, &bytes).ok_or("the written resource does not read back")?;
    let (reread, _) = open(&back.virtual_section, &back.physical)
        .map_err(|e| format!("the written resource does not read back: {e}"))?;
    // Both trees go through the XML reader, so a value is typed alike on each side.
    let written_xml = write_meta(&reread).map_err(|e| e.to_string())?;
    let reread = MetaDocument::parse(super::meta::schema(), written_xml.into_bytes())
        .map_err(|e| format!("the written resource's XML does not parse: {e}"))?;
    if reread.instance != edited {
        return Err(
            "the written resource does not read back to the XML's tree: an edit was not placed, \
             or the XML leaves out a field the body holds"
                .to_string(),
        );
    }
    fs::write(out, &saved).map_err(|e| format!("write({out}): {e}"))?;
    println!(
        "{path} at {offset:#X} + {xml} -> {out} ({} bytes)",
        saved.len()
    );
    println!("This checks the body's bytes. It does not test that GTA VI loads them.");
    Ok(())
}

/// `sections` of a headerless metadata body with the tree the XML `text` states written in, and
/// that tree.
fn with_tree(
    sections: &Rpf9Sections,
    text: Vec<u8>,
) -> Result<(Rpf9Sections, MetaInstance), String> {
    let (_, layout) = open(&sections.virtual_section, &sections.physical)
        .map_err(|e| format!("GTA VI's RBM reader does not read this body: {e}"))?;
    let ContainerLayout::Rbm(rbm) = layout.container else {
        return Err("GTA VI's RBM reader gave this body no headerless layout".to_string());
    };
    let tree = MetaDocument::parse(super::meta::schema(), text)
        .map_err(|e| format!("the XML does not parse: {e}"))?;
    let grown = write_grown(&rbm, &tree.instance).map_err(|e| e.to_string())?;
    let written = Rpf9Sections {
        form: sections.form,
        virtual_section: grown.0,
        physical: grown.1,
    };
    Ok((written, tree.instance))
}
