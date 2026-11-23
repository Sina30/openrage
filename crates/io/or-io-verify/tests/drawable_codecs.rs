//! The drawable codecs' owned forms over the corpus, both targets: every
//! Legacy parameter list, Enhanced parameter block and bone-tag table opened
//! owned and saved in its record's place saves as the tree saves.

mod declared;

use std::mem::offset_of;
use std::path::PathBuf;

use declared::{files, format, name, Opened};
use or_io::decl::{
    Codec, Cx, Described, Heap, Kids, Opaque, Owned, Plain, Ptr, Raw, Rx, Site, SiteMut,
};
use or_io::desc::{Record, Value};
use or_io::error::Error;
use or_io::gta5::drawable;
use or_io::gta5::drawable_codecs::{gen8, gen9, BoneTags};
use or_io::gta5::skeleton;
use or_io::Generation;

const EXTS: [&str; 4] = ["ydr", "ydd", "yft", "ypt"];

/// A texture slot carried as the tree holds it, so each site is judged on the codec's regions alone.
#[derive(Debug, Clone, Default)]
struct Slot(Option<Value>);

impl Heap for Slot {
    fn heap_bytes(&self) -> usize {
        0
    }
}

impl Owned for Slot {
    type Raw = Ptr<Opaque>;
    const LEAF: bool = false;
    fn from_raw(_: &Ptr<Opaque>, kids: Kids<'_>, at: usize, _: &Rx<'_>) -> Result<Self, Error> {
        Ok(Self(
            kids.iter()
                .find(|(o, _)| *o as usize == at)
                .map(|(_, v)| v.clone()),
        ))
    }
    fn to_raw(
        &self,
        kids: &mut Vec<(u32, Value)>,
        at: usize,
        cx: &Cx,
    ) -> Result<Ptr<Opaque>, Error> {
        kids.push((at as u32, self.to_element(cx)?));
        Ok(Plain::zeroed())
    }
    fn to_element(&self, _: &Cx) -> Result<Value, Error> {
        Ok(self.0.clone().unwrap_or_else(Value::null_ptr))
    }
}

/// The site at `field` of `record` opened as `C` and saved back in its place.
fn reopen<C: Codec, O: Described + Plain>(record: &mut Record, field: usize) -> Result<(), Error> {
    let root = Value::null_ptr();
    let opened = C::open(
        &Site::new::<O>(record.bytes(), record.children(), 0, field),
        &Rx::new(&root),
    )?;
    let mut kids: Vec<(u32, Value)> = record
        .children()
        .iter()
        .filter(|(o, _)| *o as usize != field)
        .cloned()
        .collect();
    let cx = Cx::new(record.plans().clone());
    let mut site = SiteMut::new::<O>(record.bytes(), &mut kids, 0, field);
    opened.save(&mut site, &cx)?;
    let raw: O = site.finish()?;
    record.bytes_mut().copy_from_slice(Plain::as_bytes(&raw));
    kids.sort_by_key(|(o, _)| *o);
    record.set_children(kids);
    Ok(())
}

#[derive(Default)]
struct Tally {
    materials: usize,
    skeletons: usize,
    refused: Vec<String>,
}

fn reopen_all(v: &mut Value, target: Generation, n: &mut Tally) {
    match v {
        Value::Struct(record) => {
            let got = match (record.type_name(), target) {
                ("GrcInstanceData", _) => {
                    n.materials += 1;
                    reopen::<
                        Vec<gen8::GrcInstanceDataEntry<Slot>>,
                        Raw<drawable::gen8::GrcInstanceData>,
                    >(
                        record,
                        offset_of!(Raw<drawable::gen8::GrcInstanceData>, entries),
                    )
                }
                ("GrcInstanceDataG9", _) => {
                    n.materials += 1;
                    reopen::<gen9::Params<Slot>, Raw<drawable::gen9::GrcInstanceDataG9>>(
                        record,
                        offset_of!(Raw<drawable::gen9::GrcInstanceDataG9>, params),
                    )
                }
                ("CrSkeletonData", _) => {
                    n.skeletons += 1;
                    reopen::<BoneTags, Raw<skeleton::CrSkeletonData>>(
                        record,
                        offset_of!(Raw<skeleton::CrSkeletonData>, bone_tags),
                    )
                }
                _ => Ok(()),
            };
            if let Err(e) = got {
                n.refused.push(e.to_string());
            }
            for (_, child) in record.children_mut() {
                reopen_all(child, target, n);
            }
        }
        Value::Array { elements, .. } => elements.iter_mut().for_each(|e| reopen_all(e, target, n)),
        Value::Ptr {
            target: Some(t), ..
        } => reopen_all(t, target, n),
        _ => {}
    }
}

/// What one extension and target gave: files, sites opened, saves that
/// matched the tree's, and saves both refused for one reason.
#[derive(Default)]
struct Run {
    files: usize,
    same: usize,
    refused: usize,
    sites: Tally,
}

fn run(ext: &str, target: Generation, paths: &[PathBuf], failures: &mut Vec<String>) -> Run {
    let fmt = format(ext);
    let mut got = Run::default();
    for path in paths {
        let opened = Opened::read(path);
        let tree = fmt.read(target, opened.image()).expect("the tree reads");
        got.files += 1;
        let mut owned = tree.clone();
        let before = got.sites.refused.len();
        reopen_all(&mut owned, target, &mut got.sites);
        if let Some(why) = got.sites.refused.get(before) {
            failures.push(format!("{}: opening owned: {why}", name(path)));
            continue;
        }
        match (fmt.save(target, &tree), fmt.save(target, &owned)) {
            (Ok(a), Ok(b)) if a == b => got.same += 1,
            (Err(a), Err(b)) if a.to_string() == b.to_string() => got.refused += 1,
            (a, b) => failures.push(format!(
                "{}: the tree saves {:?} and the owned sites {:?}",
                name(path),
                a.map(|b| b.len()),
                b.map(|b| b.len())
            )),
        }
    }
    got
}

/// Every corpus material and skeleton opened owned and saved in place
/// saves as its tree saves, and a tree the save refuses is still refused the same way.
#[test]
fn every_material_and_skeleton_opens_owned_and_saves_as_its_tree_saves() {
    let mut failures = Vec::new();
    for target in [Generation::Gen8, Generation::Gen9] {
        let (mut materials, mut skeletons) = (0usize, 0usize);
        for ext in EXTS {
            let Some(paths) = files(ext, Some(target), "the drawable codecs' owned forms") else {
                continue;
            };
            let got = run(ext, target, &paths, &mut failures);
            println!(
                "{} .{ext}: {} material(s) and {} skeleton(s) in {} file(s) opened owned; {} \
                 save as the tree saves, {} refused as the tree is",
                target.edition_label(),
                got.sites.materials,
                got.sites.skeletons,
                got.files,
                got.same,
                got.refused
            );
            materials += got.sites.materials;
            skeletons += got.sites.skeletons;
        }
        println!(
            "{}: {materials} material(s) and {skeletons} skeleton(s) in all",
            target.edition_label()
        );
    }
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
