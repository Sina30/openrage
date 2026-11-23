//! Every byte a declared format's description does not claim as data, read out
//! of the corpus and out of each file's canonical save: declared padding,
//! opaque bytes, pointers at nothing described, and bytes no field covers.
//!
//! A region that holds something in a shipped file is live, and every live
//! region carries a recorded reason: a runtime slot the writer rebuilds, a
//! payload the description does not name but the save carries, a word a save
//! writes null by decision, or data a save loses. The fate is measured on
//! `or-io`'s canonical save and held to it.
//!
//! ⚠ **What a save writes null is read from the one save-loss list,**
//! `census::RULED` and `census::STILL_LOST`, never recorded here: a region a
//! save zeroes fails unless that list names it for its format and target. A
//! lost region is a defect listed rather than failed on, and it fails once a
//! save carries it, so it cannot outlive the fix.
//!
//! ⚠ Live means non-zero, and a pointer slot also says whether its word leads
//! to bytes in the resource. A value that is not an address, such as a vtable,
//! reads as a live word and nothing more. The one such word a zero crashes the
//! game on is held apart: each format's census fails on a saved `pgBase` with a
//! zero vtable, and on a saved root that should be a `pgBase` and is not
//! reached as one; [`a_saved_pg_base_never_has_a_zero_vtable`] shows the rule
//! catching one.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use or_io::desc::{FieldRepr, Image, Pack};
use or_io::gta5::formats::{self, Format};
use or_io::Generation;
use or_io_verify::census::{self, Liveness, RULED, STILL_LOST};
use or_io_verify::gates::{self, name, Opened};

/// What a save does with a live region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// A runtime slot the writer computes and stores again.
    Rebuilt,
    /// Data the description does not name, carried by the save as it is.
    Payload,
    /// A word a save writes null by decision, which [`RULED`] names.
    Dropped,
    /// Data a save writes null over: a defect [`STILL_LOST`] names until the save carries it.
    Lost,
}

/// Why a live region holds what it does, and what a save does with it.
struct Reason {
    /// The formats the region is live in.
    formats: &'static [&'static str],
    ty: &'static str,
    region: &'static str,
    /// What a save that carries the region does with it, measured on `or-io`'s
    /// canonical save; `None` where a save writes it null, which the save-loss
    /// list says (see [`listed`]).
    carried: Option<Fate>,
    why: &'static str,
}

/// Every live region.
///
/// ⚠ A row is a claim both ways: a format it lists fails if the region is not
/// live there, and a live region no row lists for its format fails.
const REASONS: &[Reason] = &[
    Reason {
        formats: &[
            "yfd", "ybn", "ybd", "yld", "yed", "ytd", "ycd", "ydr", "ydd", "yft", "ypt",
        ],
        ty: "PgBase",
        region: "first_node",
        carried: None,
        why: "pgBase's list head, a word from the process that wrote the file",
    },
    Reason {
        formats: &["ybn", "ydr"],
        ty: "PhPrimBox",
        region: "_pad_after_pad0",
        carried: Some(Fate::Payload),
        why: "the byte before a box's vertex indices, which holds whatever the writing process \
              left there, as the two pad bytes before it do; the polygon run is carried whole",
    },
    Reason {
        formats: &["ydr"],
        ty: "GrmShaderGroup",
        region: "shader_datas",
        carried: None,
        why: "a pointer at something the description does not describe, leading into the \
              resource in the instances the leads column counts",
    },
    Reason {
        formats: &["ydr", "ydd", "yft", "ypt"],
        ty: "GrcInstanceDataEntry",
        region: "data",
        carried: Some(Fate::Payload),
        why: "a texture when the entry's count is zero and rows inside the shader's own blob \
              otherwise; the drawable codecs carry the blob whole and rebase it",
    },
    Reason {
        formats: &["ydr", "ydd", "yft", "ypt"],
        ty: "GrcInstanceDataG9",
        region: "params",
        carried: Some(Fate::Payload),
        why: "the Enhanced parameter block, one allocation with the slot tables; the drawable \
              codecs carry it whole and rebase it",
    },
    Reason {
        formats: &["ydr", "ydd", "yft", "ypt"],
        ty: "GrcInstanceDataG9",
        region: "texture_refs",
        carried: Some(Fate::Payload),
        why: "a part of the Enhanced parameter block's allocation; the drawable codecs carry it \
              whole and rebase it",
    },
    Reason {
        formats: &["ydr", "ydd", "yft"],
        ty: "CrSkeletonData",
        region: "bone_tags",
        carried: Some(Fate::Rebuilt),
        why: "the bone-id hash table, which the drawable codecs derive again from the bones",
    },
    Reason {
        formats: &["yft"],
        ty: "OctantVertsTable",
        region: "octant_verts",
        carried: Some(Fate::Payload),
        why: "the octant map's vertex lists, which the bounds codec carries and points at again",
    },
    Reason {
        formats: &["yft"],
        ty: "PhBoundGeometry",
        region: "octant_vert_counts",
        carried: Some(Fate::Payload),
        why: "the octant map's counts, which the bounds codec lays out beside its vertex lists",
    },
];

/// The types a saved file's `pgBase` is declared as.
const PG_BASES: &[&str] = &["PgBase"];

/// Formats whose root is no `pgBase`, and why.
const ROOTS_WITHOUT_PG_BASE: &[(&str, &str)] = &[];

/// `pgBase`'s members as `(offset, name)`, read from its own description.
fn pg_base_members() -> &'static [(usize, String)] {
    static MEMBERS: OnceLock<Vec<(usize, String)>> = OnceLock::new();
    MEMBERS.get_or_init(|| {
        use or_io::decl::{describe, root, Raw};
        use or_io::gta5::shared::PgBase;
        let pack = describe("gta5", "gen8", &[root::<Raw<PgBase>>]).expect("describes");
        let members: Vec<_> = pack
            .type_by_name("PgBase")
            .expect("described")
            .fields
            .iter()
            .map(|f| (f.offset, f.name.clone()))
            .collect();
        // ⚠ No members would make every type a pgBase.
        assert!(!members.is_empty(), "pgBase is described with no members");
        members
    })
}

/// Whether `ty` is a `pgBase`: one of [`PG_BASES`], or a type declaring its members inline.
fn is_pg_base(pack: &Pack, ty: &str) -> bool {
    PG_BASES.contains(&ty)
        || pack.type_by_name(ty).is_some_and(|t| {
            pg_base_members()
                .iter()
                .all(|(at, name)| t.fields.iter().any(|f| f.offset == *at && f.name == *name))
        })
}

/// Whether `ty`, or what it embeds at offset zero at any depth, is a `pgBase`.
fn starts_with_pg_base(pack: &Pack, ty: &str) -> bool {
    let mut ty = ty.to_string();
    let mut seen = BTreeSet::new();
    while seen.insert(ty.clone()) {
        if is_pg_base(pack, &ty) {
            return true;
        }
        let first = pack.type_by_name(&ty).and_then(|t| {
            t.fields.iter().find_map(|f| match &f.repr {
                FieldRepr::Inline { type_name } if f.offset == 0 => Some(type_name.clone()),
                _ => None,
            })
        });
        match first {
            Some(inner) => ty = inner,
            None => return false,
        }
    }
    false
}

#[derive(Default)]
struct Measured {
    files: usize,
    saved: usize,
    refused: Vec<String>,
    shipped: BTreeMap<(String, String), Liveness>,
    after: BTreeMap<(String, String), Liveness>,
    /// Each region's `(at, bytes, pointer)`.
    regions: BTreeMap<(String, String), (usize, usize, bool)>,
    root_is_pg_base: bool,
    zero_vtables: Vec<String>,
    /// Saved files whose root the walk did not reach as a `pgBase`.
    roots_missed: Vec<String>,
    pg_bases: usize,
}

fn fold(
    into: &mut BTreeMap<(String, String), Liveness>,
    from: BTreeMap<(String, String), Liveness>,
) {
    for (key, l) in from {
        into.entry(key).or_default().add(l);
    }
}

fn measure(fmt: &Format, target: Generation, pack: &Pack) -> Option<Measured> {
    let ext = fmt.ext;
    let paths = gates::files(ext, Some(target), &format!("the .{ext} padding census"))?;
    let root = fmt.root_name(target).expect("declared");
    let regions = census::regions(pack);
    let mut out = Measured {
        regions: regions
            .iter()
            .map(|r| {
                (
                    (r.type_name.clone(), r.label.clone()),
                    (r.at, r.len, r.pointer),
                )
            })
            .collect(),
        root_is_pg_base: starts_with_pg_base(pack, &root),
        ..Default::default()
    };
    for path in &paths {
        out.files += 1;
        let opened = Opened::read(path);
        let image = opened.image();
        let (_, sites) = census::walk(pack, image, &root, 0);
        fold(&mut out.shipped, census::liveness(&regions, image, &sites));
        let saved = match fmt.read(target, image).and_then(|t| fmt.save(target, &t)) {
            Ok(bytes) => bytes,
            Err(e) => {
                out.refused.push(format!("{}: {e}", name(path)));
                continue;
            }
        };
        out.saved += 1;
        let saved = Opened::of(&saved);
        let image = saved.image();
        let (_, sites) = census::walk(pack, image, &root, 0);
        fold(&mut out.after, census::liveness(&regions, image, &sites));
        let bases: Vec<_> = sites.iter().filter(|(t, _)| is_pg_base(pack, t)).collect();
        out.pg_bases += bases.len();
        if out.root_is_pg_base && !bases.iter().any(|(_, at)| *at == 0) {
            out.roots_missed.push(name(path));
        }
        for at in zero_vtables(pack, image, &sites) {
            out.zero_vtables.push(format!("{}: at {at:#x}", name(path)));
        }
    }
    Some(out)
}

/// Where a `pgBase` among `sites` holds a zero vtable.
fn zero_vtables(pack: &Pack, image: Image<'_>, sites: &BTreeSet<(String, usize)>) -> Vec<usize> {
    sites
        .iter()
        .filter(|(t, _)| is_pg_base(pack, t))
        .filter(|(_, at)| image.pointer_at(*at).is_ok_and(|vft| vft == 0))
        .map(|(_, at)| *at)
        .collect()
}

fn reason(ext: &str, ty: &str, region: &str) -> Option<&'static Reason> {
    REASONS
        .iter()
        .find(|r| r.formats.contains(&ext) && r.ty == ty && r.region == region)
}

/// What the save-loss list says a save writes null at `ty.region` in `ext` on `slug`.
fn listed(ext: &str, slug: &str, ty: &str, region: &str) -> Option<Fate> {
    let site = format!("{ty}.{region}");
    if RULED.iter().any(|(s, _)| *s == site) {
        return Some(Fate::Dropped);
    }
    STILL_LOST
        .iter()
        .any(|(e, t, s, _)| (*e, *t, *s) == (ext, slug, site.as_str()))
        .then_some(Fate::Lost)
}

/// Why `fate` does not hold for a region live in `shipped` and measured
/// `after` a save, or `None` when it holds.
fn contradiction(fate: Fate, pointer: bool, shipped: Liveness, after: Liveness) -> Option<String> {
    if after.reached == 0 {
        return Some(format!(
            "is recorded {fate:?}, and no saved instance reaches it to hold the save to"
        ));
    }
    let zeroed = after.non_zero == 0;
    match fate {
        Fate::Rebuilt | Fate::Payload if zeroed => Some(format!(
            "is recorded {fate:?}, and a save zeroes it in all {} saved instance(s)",
            after.reached
        )),
        Fate::Dropped | Fate::Lost if !zeroed => Some(format!(
            "is recorded {fate:?}, and a save leaves it live in {} of {}",
            after.non_zero, after.reached
        )),
        Fate::Dropped if pointer && shipped.leads_somewhere > 0 => Some(format!(
            "is recorded Dropped, and leads into the resource in {} shipped instance(s), \
             so a save loses what it points at",
            shipped.leads_somewhere
        )),
        _ => None,
    }
}

/// Measure `ext` on every target it declares and print every region; return
/// every live region with no reason, every reason a save contradicts, every
/// saved `pgBase` with a zero vtable or root not reached as one, and every
/// reason for a region not live here.
fn census_of(ext: &str) -> Vec<String> {
    let fmt = gates::format(ext);
    let mut failures = Vec::new();
    let mut live = BTreeSet::new();
    let mut every_target = true;
    let without = ROOTS_WITHOUT_PG_BASE.iter().find(|(e, _)| *e == ext);
    for target in fmt.targets().collect::<BTreeSet<_>>() {
        let pack = gates::pack(ext, target);
        let Some(m) = measure(fmt, target, &pack) else {
            every_target = false;
            continue;
        };
        let slug = target.slug();
        println!(
            ".{ext} {slug}: {} file(s), {} saved, {} of {} region(s) reached",
            m.files,
            m.saved,
            m.shipped.len(),
            m.regions.len()
        );
        for r in &m.refused {
            println!("    save refused: {r}");
        }
        if m.saved == 0 {
            failures.push(format!(
                ".{ext} {slug}: every save of {} file(s) is refused",
                m.files
            ));
        }
        println!(
            "    {:<52} {:>7} {:>7} {:>8} {:>7} {:>7}  fate",
            "region [at; bytes]", "reached", "live", "leads", "saved", "live"
        );
        for (key, (at, len, pointer)) in &m.regions {
            let label = format!("{}.{} [{at:#x}; {len}]", key.0, key.1);
            let Some(shipped) = m.shipped.get(key).copied() else {
                println!("    {label:<52} not reached");
                continue;
            };
            let after = m.after.get(key).copied().unwrap_or_default();
            let recorded = reason(ext, &key.0, &key.1);
            let listed = listed(ext, slug, &key.0, &key.1);
            let fate = recorded.and_then(|r| r.carried).or(listed);
            let shown = match (recorded, fate) {
                (Some(_), Some(f)) if shipped.non_zero > 0 => format!("{f:?}"),
                (Some(_), None) if shipped.non_zero > 0 => "!! not on the save-loss list".into(),
                (None, _) if shipped.non_zero > 0 => "!! no reason".into(),
                _ => String::new(),
            };
            println!(
                "    {label:<52} {:>7} {:>7} {:>8} {:>7} {:>7}  {shown}",
                shipped.reached,
                shipped.non_zero,
                shipped.leads_somewhere,
                after.reached,
                after.non_zero
            );
            if shipped.non_zero == 0 {
                continue;
            }
            live.insert(key.clone());
            let here = format!(".{ext} {slug} {label}");
            let Some(r) = recorded else {
                failures.push(format!(
                    "{here} is live in {} of {} and carries no reason",
                    shipped.non_zero, shipped.reached
                ));
                continue;
            };
            println!("        {}", r.why);
            if let (Some(carried), Some(on_list)) = (r.carried, listed) {
                failures.push(format!(
                    "{here} is recorded {carried:?}, and the save-loss list names it {on_list:?}"
                ));
                continue;
            }
            let Some(fate) = fate else {
                failures.push(format!(
                    "{here} records no carried fate, and the save-loss list does not name it \
                     for .{ext} {slug}"
                ));
                continue;
            };
            if let Some(why) = contradiction(fate, *pointer, shipped, after) {
                failures.push(format!("{here} {why}"));
            }
        }
        println!(
            "    {} pgBase instance(s) in saved files, {} with a zero vtable; the root is {}",
            m.pg_bases,
            m.zero_vtables.len(),
            if m.root_is_pg_base {
                "a pgBase"
            } else {
                "no pgBase"
            }
        );
        match (m.root_is_pg_base, without) {
            (false, None) => failures.push(format!(
                ".{ext} {slug}: the root is not recognised as a pgBase, so the zero-vtable \
                 rule cannot reach it"
            )),
            (true, Some(_)) => failures.push(format!(
                ".{ext} {slug}: the root is a pgBase and ROOTS_WITHOUT_PG_BASE lists it"
            )),
            (false, Some((_, why))) => println!("        {why}"),
            (true, None) => {}
        }
        failures.extend(
            m.roots_missed
                .iter()
                .take(10)
                .map(|f| format!(".{ext} {slug}: {f}: the saved root is not reached as a pgBase")),
        );
        failures.extend(
            m.zero_vtables
                .iter()
                .take(10)
                .map(|z| format!(".{ext} {slug}: a saved pgBase has a zero vtable: {z}")),
        );
    }
    if every_target {
        for r in REASONS.iter().filter(|r| r.formats.contains(&ext)) {
            if !live.contains(&(r.ty.to_string(), r.region.to_string())) {
                failures.push(format!(
                    "a reason for a region not live in any .{ext}: {}.{}",
                    r.ty, r.region
                ));
            }
        }
    }
    failures
}

macro_rules! padding_census {
    ($($ext:literal => $census:ident),* $(,)?) => {
        const CENSUSED: &[&str] = &[$($ext),*];
        $(
            #[test]
            fn $census() {
                let failures = census_of($ext);
                assert!(failures.is_empty(), "{}", failures.join("\n"));
            }
        )*
    };
}

padding_census! {
    "yfd" => what_a_yfd_holds_where_it_declares_nothing,
    "ybn" => what_a_ybn_holds_where_it_declares_nothing,
    "ybd" => what_a_ybd_holds_where_it_declares_nothing,
    "yld" => what_a_yld_holds_where_it_declares_nothing,
    "yed" => what_a_yed_holds_where_it_declares_nothing,
    "ynd" => what_a_ynd_holds_where_it_declares_nothing,
    "ynv" => what_a_ynv_holds_where_it_declares_nothing,
    "yvr" => what_a_yvr_holds_where_it_declares_nothing,
    "ywr" => what_a_ywr_holds_where_it_declares_nothing,
    "ydd" => what_a_ydd_holds_where_it_declares_nothing,
    "yft" => what_a_yft_holds_where_it_declares_nothing,
    "ydr" => what_a_ydr_holds_where_it_declares_nothing,
    "ycd" => what_a_ycd_holds_where_it_declares_nothing,
    "ypt" => what_a_ypt_holds_where_it_declares_nothing,
    "ytd" => what_a_ytd_holds_where_it_declares_nothing,
}

#[test]
fn every_declared_format_is_censused() {
    let declared: Vec<&str> = formats::FORMATS.iter().map(|f| f.ext).collect();
    assert_eq!(CENSUSED, declared.as_slice());
}

/// The rule each format's census holds its saves to, shown to catch a zero,
/// on a `pgBase` declared as a type and on one whose members a root declares inline.
#[test]
fn a_saved_pg_base_never_has_a_zero_vtable() {
    use or_io::decl::{describe, root, Aligned, Raw};
    use or_io::gta5::path_region::CPathRegion;
    use or_io::gta5::shared::PgBaseRefCounted;
    let cases = [
        (
            describe("gta5", "gen8", &[root::<Raw<PgBaseRefCounted>>]).expect("describes"),
            "PgBaseRefCounted",
        ),
        (
            describe("gta5", "gen8", &[root::<Raw<CPathRegion>>]).expect("describes"),
            "CPathRegion",
        ),
    ];
    for (pack, root) in cases {
        let size = pack.type_by_name(root).expect("described").size;
        assert!(starts_with_pg_base(&pack, root), "{root} is a pgBase");
        let walked = |bytes: &Aligned| {
            let image = Image::new(bytes.bytes(), size);
            let (_, sites) = census::walk(&pack, image, root, 0);
            zero_vtables(&pack, image, &sites)
        };
        let mut filled = vec![0u8; size];
        assert_eq!(
            walked(&Aligned::from_bytes(&filled)),
            [0],
            "{root}: a zero vtable is named"
        );
        filled[..8].copy_from_slice(&or_io::constants::DEFAULT_VFT.to_le_bytes());
        assert!(walked(&Aligned::from_bytes(&filled)).is_empty(), "{root}");
    }
}

/// A fate is held to what the save leaves, and a save that reaches nothing holds none.
#[test]
fn a_fate_no_saved_instance_reaches_is_not_held() {
    let live = |reached, non_zero, leads_somewhere| Liveness {
        reached,
        non_zero,
        leads_somewhere,
    };
    for fate in [Fate::Rebuilt, Fate::Payload, Fate::Dropped, Fate::Lost] {
        assert!(
            contradiction(fate, false, live(4, 4, 0), live(0, 0, 0)).is_some(),
            "{fate:?}"
        );
    }
    assert!(contradiction(Fate::Payload, false, live(4, 4, 0), live(4, 4, 0)).is_none());
    assert!(contradiction(Fate::Rebuilt, false, live(4, 4, 0), live(4, 0, 0)).is_some());
    assert!(contradiction(Fate::Dropped, true, live(4, 4, 0), live(4, 0, 0)).is_none());
    assert!(contradiction(Fate::Dropped, true, live(4, 4, 2), live(4, 0, 0)).is_some());
    assert!(contradiction(Fate::Lost, true, live(4, 4, 2), live(4, 0, 0)).is_none());
    assert!(contradiction(Fate::Lost, true, live(4, 4, 2), live(4, 1, 1)).is_some());
}

/// A region a save writes null takes its fate from the save-loss list, per format and target.
#[test]
fn a_written_null_region_takes_its_fate_from_the_save_loss_list() {
    assert_eq!(
        listed("ytd", "gen8", "PgBase", "first_node"),
        Some(Fate::Dropped)
    );
    assert_eq!(
        listed("ydr", "gen8", "GrmShaderGroup", "shader_datas"),
        Some(Fate::Lost)
    );
    assert_eq!(
        listed("ydr", "gen9", "GrmShaderGroup", "shader_datas"),
        None
    );
    assert_eq!(
        listed("ydd", "gen8", "GrmShaderGroup", "shader_datas"),
        None
    );
}
