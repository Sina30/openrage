//! Which variant every tagged type selects in the corpus, and which declared
//! types a walk from each format's root reaches.
//!
//! A tag value no variant is declared for fails: the walk stops at the base,
//! so whatever the variant holds is read as nothing and saved as nothing. A
//! declared variant no file selects, and a declared type no file reaches, is
//! printed: a fact about the corpus that says what the other gates did not see.

use std::collections::BTreeSet;

use or_io::gta5::formats;
use or_io_verify::census::{self, Census};
use or_io_verify::gates::{self, Opened};

/// Tag values whose layout is the tagged type's own, as `(the formats they occur in, type, tag, why)`.
///
/// ⚠ A row is a claim both ways: a format it lists fails if no file there
/// holds the value, and a value no row or variant answers fails.
const THE_BASE_ITSELF: &[(&[&str], &str, i64, &str)] = &[(
    &["ybn", "yft"],
    "PhPolygon",
    0,
    "a polygon, whose layout is the base's own: the area, three vertex indices and \
         three neighbouring polygons",
)];

/// Walk every staged file of `ext` on each target, print what each tag selected
/// and what was reached, and return every tag no variant or row answers.
fn variants_of(ext: &str) -> Vec<String> {
    let fmt = gates::format(ext);
    let mut failures = Vec::new();
    let mut held = BTreeSet::new();
    let mut every_target = true;
    for target in fmt.targets().collect::<BTreeSet<_>>() {
        let slug = target.slug();
        let Some(paths) = gates::files(ext, Some(target), &format!("the .{ext} variant census"))
        else {
            every_target = false;
            continue;
        };
        let pack = gates::pack(ext, target);
        let root = fmt.root_name(target).expect("declared");
        let mut total = Census::default();
        for path in &paths {
            let mut one = census::take(&pack, Opened::read(path).image(), &root, 0);
            for u in &mut one.undeclared {
                *u = format!("{}: {u}", gates::name(path));
            }
            total.fold(one);
        }
        let declared = pack.types.len();
        println!(
            ".{ext} {slug}: {} file(s), {} of {declared} declared type(s) reached",
            paths.len(),
            total.types.len()
        );
        for ty in pack.types.iter().filter(|t| t.selector.is_some()) {
            let selector = ty.selector.as_ref().expect("tagged");
            let seen = total.tags.get(&ty.name);
            println!("    {} tagged by `{}`:", ty.name, selector.tag);
            for (value, n) in seen.into_iter().flatten() {
                let base = THE_BASE_ITSELF
                    .iter()
                    .find(|(e, t, v, _)| e.contains(&ext) && *t == ty.name && v == value);
                let variant = match (selector.type_for(*value), base) {
                    (Some(variant), _) => variant.to_string(),
                    (None, Some((.., why))) => {
                        held.insert((ty.name.clone(), *value));
                        format!("the base itself: {why}")
                    }
                    (None, None) => {
                        let first = total
                            .undeclared
                            .iter()
                            .find(|u| u.contains(&format!("`{}` ", ty.name)))
                            .cloned()
                            .unwrap_or_default();
                        failures.push(format!(
                            ".{ext} {slug}: `{}` holds tag {value} in {n} instance(s), and no \
                             variant is declared for it; the first: {first}",
                            ty.name
                        ));
                        "!! no declared variant".into()
                    }
                };
                println!("        {value:>12} {n:>8}  {variant}");
            }
            for (value, variant) in &selector.variants {
                if seen.is_none_or(|s| !s.contains_key(value)) {
                    println!(
                        "        {value:>12} {:>8}  {variant}, which no file selects",
                        0
                    );
                }
            }
        }
        let unreached: Vec<&str> = pack
            .types
            .iter()
            .map(|t| t.name.as_str())
            .filter(|n| !total.types.contains_key(*n))
            .collect();
        if !unreached.is_empty() {
            println!("    not reached: {}", unreached.join(", "));
        }
        assert!(
            total.types.contains_key(&root),
            ".{ext} {slug}: the walk did not reach its root"
        );
    }
    if every_target {
        for (_, ty, value, _) in THE_BASE_ITSELF.iter().filter(|(e, ..)| e.contains(&ext)) {
            if !held.contains(&(ty.to_string(), *value)) {
                failures.push(format!(
                    "a row for tag {value} of `{ty}`, which no .{ext} holds"
                ));
            }
        }
    }
    failures
}

macro_rules! variant_census {
    ($($ext:literal => $census:ident),* $(,)?) => {
        const CENSUSED: &[&str] = &[$($ext),*];
        $(
            #[test]
            fn $census() {
                let failures = variants_of($ext);
                assert!(failures.is_empty(), "{}", failures.join("\n"));
            }
        )*
    };
}

variant_census! {
    "yfd" => every_yfd_tag_selects_a_declared_variant,
    "ybn" => every_ybn_tag_selects_a_declared_variant,
    "ybd" => every_ybd_tag_selects_a_declared_variant,
    "yld" => every_yld_tag_selects_a_declared_variant,
    "yed" => every_yed_tag_selects_a_declared_variant,
    "ynd" => every_ynd_tag_selects_a_declared_variant,
    "ynv" => every_ynv_tag_selects_a_declared_variant,
    "yvr" => every_yvr_tag_selects_a_declared_variant,
    "ywr" => every_ywr_tag_selects_a_declared_variant,
    "ydd" => every_ydd_tag_selects_a_declared_variant,
    "yft" => every_yft_tag_selects_a_declared_variant,
    "ydr" => every_ydr_tag_selects_a_declared_variant,
    "ycd" => every_ycd_tag_selects_a_declared_variant,
    "ypt" => every_ypt_tag_selects_a_declared_variant,
    "ytd" => every_ytd_tag_selects_a_declared_variant,
}

#[test]
fn every_declared_format_is_censused() {
    let declared: Vec<&str> = formats::FORMATS.iter().map(|f| f.ext).collect();
    assert_eq!(CENSUSED, declared.as_slice());
}
