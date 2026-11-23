//! Whether a geometry's matrix palette holds bone indices or bone tags, and
//! how often the skeleton it is resolved against is not the one it was
//! authored against.
//!
//! The two hypotheses are separable: an index-based palette has every entry
//! below the bone count, and a tag-based one does not, because a tag ranges
//! over sixteen bits while a skeleton has a few hundred bones.
//!
//! ⚠ **A bounds check is not an identity check.** `ydr_invariants.rs` already
//! holds that every palette entry is below the bone count of *the skeleton in
//! the same file*. That says nothing about a drawable paired with a different
//! skeleton, which is what the preview does every time it picks a mesh source
//! and a skeleton source separately.

mod fragment;

use std::path::PathBuf;

use or_anim::Skeleton;
use or_io::gta5::{File as _, GrmGeometry, GtaDrawable, PgDictionary};

fn corpus(extension: &str) -> Vec<PathBuf> {
    or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), extension).find()
}

fn geometries(drawable: &GtaDrawable) -> Vec<&GrmGeometry<or_io::decl::Gen8>> {
    drawable
        .base
        .lod_group
        .lods
        .iter()
        .flat_map(|lod| lod.models.iter())
        .flat_map(|model| model.geometries.iter().map(|g| &**g))
        .collect()
}

/// ⚠ **The palette holds indices, and the count is what says so.** Converting
/// it to tags would change the file format rather than the resolution.
#[test]
fn a_matrix_palette_holds_bone_indices_rather_than_bone_tags() {
    let files = corpus("ydr");
    if files.is_empty() {
        or_test_support::skip_corpus(
            "a_matrix_palette_holds_bone_indices_rather_than_bone_tags",
            "no .ydr corpus found. Stage one beside the repository to run",
        );
        return;
    }

    let mut with_skeleton = 0usize;
    let mut entries = 0usize;
    let mut below_bone_count = 0usize;
    let mut names_a_tag = 0usize;
    let mut equals_own_index = 0usize;
    let mut widest_tag = 0u16;
    let mut widest_entry = 0u16;

    for path in &files {
        let Ok(drawable) = GtaDrawable::open(path) else {
            continue;
        };
        let Some(skeleton) = drawable.base.skeleton.as_ref() else {
            continue;
        };
        if skeleton.bones.is_empty() {
            continue;
        }
        with_skeleton += 1;
        widest_tag = widest_tag.max(skeleton.bones.iter().map(|b| b.bone_id).max().unwrap_or(0));

        for geometry in geometries(&drawable) {
            for entry in &geometry.matrix_palette {
                entries += 1;
                widest_entry = widest_entry.max(*entry);
                if (*entry as usize) < skeleton.bones.len() {
                    below_bone_count += 1;
                    // The bone that index names — does its own tag happen to
                    // be the same number? On a real skeleton it does not.
                    if skeleton.bones[*entry as usize].bone_id == *entry {
                        equals_own_index += 1;
                    }
                }
                if skeleton.bones.iter().any(|b| b.bone_id == *entry) {
                    names_a_tag += 1;
                }
            }
        }
    }

    eprintln!(
        "\n=== what a matrix palette addresses, over the staged .ydr corpus ===\n  \
         {with_skeleton} of {} drawable(s) carry a skeleton; {entries} palette entry/entries\n  \
         {below_bone_count} are below the bone count (an index)\n  \
         {names_a_tag} name a bone tag (a tag)\n  \
         {equals_own_index} are a number that is both\n  \
         widest entry {widest_entry}, widest shipped tag {widest_tag}",
        files.len()
    );

    if entries == 0 {
        or_test_support::skip_corpus(
            "a_matrix_palette_holds_bone_indices_rather_than_bone_tags",
            "no staged drawable carries both a skeleton and a matrix palette",
        );
        return;
    }

    assert_eq!(
        below_bone_count, entries,
        "a palette entry is not below its own skeleton's bone count, so it is not \
         an index into it"
    );
    assert!(
        names_a_tag * 4 < entries,
        "{names_a_tag} of {entries} palette entries also name a bone tag, which is \
         too many to call the palette index-based on this evidence — re-derive \
         which it is before relying on either"
    );
}

/// How many clothing components carry a skeleton of their own, and therefore
/// whether a palette can be retargeted at all.
///
/// ⚠ **Most do not**, so their palette indexes a skeleton in another file
/// entirely. But **not all**: the assumption that none did was refuted by 14
/// of 42 staged skinned components, which is what makes a retarget possible.
#[test]
fn a_clothing_component_addresses_a_skeleton_it_does_not_carry() {
    let files = corpus("ydd");
    if files.is_empty() {
        or_test_support::skip_corpus(
            "a_clothing_component_addresses_a_skeleton_it_does_not_carry",
            "no .ydd corpus found",
        );
        return;
    }

    let mut entries_in_dictionary = 0usize;
    let mut skinned = 0usize;
    let mut with_own_skeleton = 0usize;
    let mut palette_entries = 0usize;
    let mut widest_entry = 0u16;

    for path in &files {
        let Ok(dict) = PgDictionary::<GtaDrawable>::open(path) else {
            continue;
        };
        for drawable in dict.entries.iter().flatten() {
            entries_in_dictionary += 1;
            let palette: Vec<u16> = geometries(drawable)
                .iter()
                .flat_map(|g| g.matrix_palette.iter().copied())
                .collect();
            if palette.is_empty() {
                continue;
            }
            skinned += 1;
            palette_entries += palette.len();
            widest_entry = widest_entry.max(palette.iter().copied().max().unwrap_or(0));
            if drawable
                .base
                .skeleton
                .as_ref()
                .is_some_and(|s| !s.bones.is_empty())
            {
                with_own_skeleton += 1;
            }
        }
    }

    eprintln!(
        "{skinned} of {entries_in_dictionary} dictionary entr(ies) across {} file(s) \
         carry a matrix palette; {with_own_skeleton} of those carry a skeleton of \
         their own. {palette_entries} palette entry/entries, widest {widest_entry}",
        files.len()
    );

    if skinned == 0 {
        or_test_support::skip_corpus(
            "a_clothing_component_addresses_a_skeleton_it_does_not_carry",
            "no staged dictionary entry carries a matrix palette",
        );
        return;
    }
    assert!(
        with_own_skeleton < skinned,
        "every skinned component carries its own skeleton, so nothing in this \
         corpus needs a skeleton supplied from outside and the cross-boundary case \
         R41e is about is unexercised"
    );
    assert!(
        with_own_skeleton > 0,
        "not one skinned component carries its own skeleton, so a palette retarget \
         has nothing to derive tags from and `retarget_palette` cannot be exercised \
         against a real file"
    );
}

/// The two skeletons a retarget has to reconcile, as the corpus supplies them.
///
/// ⚠ **Two skeletons of the same ped are not the same skeleton.** A retarget
/// is only meaningful if the tags agree where the indices do not.
#[test]
fn two_shipped_skeletons_share_tags_without_sharing_indices() {
    let mut skeletons: Vec<(String, Skeleton)> = Vec::new();
    for path in corpus("yft") {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if let Some(data) = fragment::skeleton(&path).filter(|s| s.bones.len() > 8) {
            skeletons.push((name.to_string(), Skeleton::build(&data)));
        }
    }

    if skeletons.len() < 2 {
        or_test_support::skip_corpus(
            "two_shipped_skeletons_share_tags_without_sharing_indices",
            "fewer than two staged skeletons with more than eight bones",
        );
        return;
    }

    let mut compared = 0usize;
    let mut shared_tags = 0usize;
    let mut tag_at_a_different_index = 0usize;

    for (i, (_, a)) in skeletons.iter().enumerate() {
        for (_, b) in skeletons.iter().skip(i + 1) {
            compared += 1;
            for bone in &a.bones {
                if let Some(other) = b.index_for_tag(bone.tag) {
                    shared_tags += 1;
                    if other != bone.index {
                        tag_at_a_different_index += 1;
                    }
                }
            }
        }
    }

    eprintln!(
        "{} skeleton(s), {compared} pair(s): {shared_tags} shared tag(s), \
         {tag_at_a_different_index} of them at a different index in the two",
        skeletons.len()
    );

    assert!(
        shared_tags > 0,
        "no two staged skeletons share a single tag"
    );
    assert!(
        tag_at_a_different_index > 0,
        "every shared tag sits at the same index in both skeletons, so this corpus \
         cannot show that a retarget does anything — index addressing would be \
         indistinguishable from tag addressing on it"
    );
}

/// The retarget, run over real drawables against a skeleton that is not
/// theirs, as the preview does whenever it pairs a mesh source with a
/// skeleton source.
///
/// A foreign skeleton need not hold every bone a mesh wants, so the assertion
/// is that where the retarget *does* resolve, it lands somewhere the raw index
/// would not have.
#[test]
fn retargeting_a_real_palette_moves_it_where_an_index_would_not() {
    let mut authored: Vec<(String, Skeleton, Vec<u16>)> = Vec::new();
    for path in corpus("ydr") {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let Ok(drawable) = GtaDrawable::open(&path) else {
            continue;
        };
        let Some(skeleton) = drawable
            .base
            .skeleton
            .as_ref()
            .filter(|s| s.bones.len() > 8)
        else {
            continue;
        };
        let palette: Vec<u16> = geometries(&drawable)
            .iter()
            .flat_map(|g| g.matrix_palette.iter().copied())
            .collect();
        if palette.is_empty() {
            continue;
        }
        authored.push((name.to_string(), Skeleton::build(skeleton), palette));
    }

    if authored.len() < 2 {
        or_test_support::skip_corpus(
            "retargeting_a_real_palette_moves_it_where_an_index_would_not",
            "fewer than two staged drawables carry both a skeleton and a palette",
        );
        return;
    }

    let mut pairs = 0usize;
    let mut slots = 0usize;
    let mut resolved = 0usize;
    let mut moved = 0usize;
    let mut identity_pairs = 0usize;

    for (i, (_, authoring, palette)) in authored.iter().enumerate() {
        // Against itself: the retarget has to be the identity.
        let same = or_anim::retarget_palette(palette, authoring, authoring);
        assert_eq!(
            &same.slots, palette,
            "a palette retargeted onto its own skeleton must not move"
        );
        assert!(same.is_complete());
        identity_pairs += 1;

        for (_, target, _) in authored.iter().skip(i + 1) {
            pairs += 1;
            let out = or_anim::retarget_palette(palette, authoring, target);
            slots += out.slots.len();
            resolved += out.by_tag;
            moved += out
                .slots
                .iter()
                .zip(palette.iter())
                .filter(|(new, old)| new != old)
                .count();
            // Every resolved slot is inside the target skeleton, which is the
            // one thing skinning cannot survive being wrong about.
            for slot in out
                .slots
                .iter()
                .zip(palette.iter())
                .filter(|(new, old)| new != old)
                .map(|(new, _)| *new)
            {
                assert!(
                    (slot as usize) < target.bone_count(),
                    "a retargeted slot is outside the target skeleton"
                );
            }
        }
    }

    eprintln!(
        "{} authored palette(s), {identity_pairs} identity check(s), {pairs} \
         cross-skeleton pair(s): {resolved} of {slots} slot(s) resolved by tag, \
         {moved} of them landed at a different index than the file stated",
        authored.len()
    );

    assert!(
        moved > 0,
        "not one slot moved across {pairs} pair(s) of different skeletons, so the \
         retarget is indistinguishable from taking the index as read and this \
         corpus cannot show it does anything"
    );
}
