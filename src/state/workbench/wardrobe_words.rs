//! What the Wardrobe says it did: a drop in a sentence, the lines of it for the Log, and the garments an edit moved.

use or_wardrobe::edit::{Added, Moved};
use or_wardrobe::import::{LeftOut, STRAY_PICTURE};
use or_wardrobe::shop::ShopReport;

use crate::state::front_door::counted;

/// How many of the pictures that name no texture the one line of the Log names.
const STRAY_SHOWN: usize = 5;

/// What a drop did, in a sentence.
pub(super) fn describe(added: &Added) -> String {
    let n = added.garments.len();
    let held = added.already_held.len();
    let shop = &added.shop;
    if n == 0 {
        if !shop.is_empty() {
            return tr!("Read the shop metadata: {shop}", shop = shop_said(shop));
        }
        if held > 0 {
            return tr_n!(
                "Nothing was added: the garment of the drop is already in the pack",
                "Nothing was added: the {count} garments of the drop are already in the pack",
                held
            );
        }
        return match added.left_out.first() {
            Some(first) => tr_n!(
                "Nothing was added: 1 file was not clothes, {first}",
                "Nothing was added: {count} files were not clothes, {first}",
                added.left_out.len(),
                first = first_left_out(first)
            ),
            None => tr!("Nothing was added: no model, texture or cloth in the drop").to_string(),
        };
    }
    let mut said = tr!(
        "Added {n}, {textures}",
        n = counted(n, "garment"),
        textures = counted(added.textures, "texture")
    );
    if added.cloth > 0 {
        said.push_str(&format!(", {}", counted(added.cloth, "cloth file")));
    }
    if added.unread > 0 {
        said.push_str(&match added.unread {
            1 if n == 1 => tr!(". It names no ped or slot and is marked ?").to_string(),
            1 => tr!(". 1 of them names no ped or slot and is marked ?").to_string(),
            unread => tr!(
                ". {unread} of them name no ped or slot and are marked ?",
                unread = unread
            ),
        });
        said.push_str(tr!(
            ": Say what they are… beside the counts sets them together, or choose them in Garment"
        ));
    }
    if !shop.is_empty() {
        said.push_str(&tr!(". Shop metadata: {shop}", shop = shop_said(shop)));
    }
    if held > 0 {
        said.push_str(&tr_n!(
            ". {garments} already in the pack, so it was not added again",
            ". {garments} already in the pack, so they were not added again",
            held,
            garments = counted(held, "garment")
        ));
    }
    if let Some(first) = added.left_out.first() {
        let left = added.left_out.len();
        said.push_str(&tr_n!(
            ". {files} left out, {first}; the Log has it",
            ". {files} left out, {first}; the Log has them",
            left,
            files = counted(left, "file"),
            first = first_left_out(first)
        ));
    }
    said
}

/// The first file a drop left out and why, as a clause: `the first is x.ydd: a model for a place...`.
fn first_left_out(first: &or_wardrobe::import::LeftOut) -> String {
    match first.path.is_empty() {
        true => format!("because {}", first.why),
        false => tr!(
            "the first is {path}: {why}",
            path = first.path,
            why = first.why
        ),
    }
}

/// What the shop metadata of a drop came to, in a sentence.
fn shop_said(shop: &ShopReport) -> String {
    let mut said = tr!(
        "{resolved} of {entries} entries became the ids of textures",
        resolved = shop.resolved,
        entries = shop.entries
    );
    if !shop.unplaced.is_empty() {
        said.push_str(&tr!(
            ", {count} point at no garment and are kept",
            count = shop.unplaced.len()
        ));
    }
    if !shop.unentried.is_empty() {
        said.push_str(&tr!(
            ", {count} have a texture no entry names",
            count = counted(shop.unentried.len(), "garment")
        ));
    }
    if !shop.clashes.is_empty() {
        said.push_str(&tr!(
            ", {count} claimed twice",
            count = counted(shop.clashes.len(), "id")
        ));
    }
    if !shop.carried.is_empty() {
        said.push_str(&tr!(
            ", {count} carried unchanged",
            count = counted(shop.carried.len(), "file")
        ));
    }
    if !shop.unread.is_empty() {
        said.push_str(&tr!(", {count} not read", count = shop.unread.len()));
    }
    if shop.lines().is_empty() {
        said
    } else {
        tr!("{said}; the Log lists them", said = said)
    }
}

/// The lines of a drop for the Log: each file left out, the pictures that name no texture together in one line, and each entry and file its shop metadata lists.
pub(super) fn notes_of(added: &Added) -> Vec<String> {
    let (stray, others): (Vec<&LeftOut>, Vec<&LeftOut>) =
        (added.left_out.iter()).partition(|l| l.why == STRAY_PICTURE);
    let mut left: Vec<String> = (others.iter())
        .map(|l| tr!("Left out {path}: {why}", path = l.path, why = l.why))
        .collect();
    if !stray.is_empty() {
        let shown: Vec<&str> = stray
            .iter()
            .take(STRAY_SHOWN)
            .map(|l| l.path.as_str())
            .collect();
        let more = match stray.len() - shown.len() {
            0 => String::new(),
            n => tr!(", and {n} more", n = n),
        };
        left.push(tr!(
            "Left out {count} that name no texture ({shown}{more}): {stray_picture}",
            more = more,
            count = counted(stray.len(), "picture"),
            shown = shown.join(", "),
            stray_picture = STRAY_PICTURE
        ));
    }
    let held = (added.already_held.iter()).map(|h| tr!("Already in the pack: {h}", h = h));
    let shop = added
        .shop
        .lines()
        .into_iter()
        .map(|l| tr!("Shop: {l}", l = l));
    left.into_iter().chain(held).chain(shop).collect()
}

/// Which garments an edit moved to another place, in a sentence; empty when none moved.
pub(super) fn moves_said(moved: &[Moved]) -> String {
    let move_of = |m: &Moved| {
        tr!(
            "{name} from {from} to {to}",
            name = m.name,
            from = format!("{:03}", m.from),
            to = format!("{:03}", m.to)
        )
    };
    match moved {
        [] => String::new(),
        [one] => tr!("Moved: {one}", one = move_of(one)),
        many => {
            let shown: Vec<String> = many.iter().take(3).map(move_of).collect();
            let more = match many.len() - shown.len() {
                0 => String::new(),
                n => tr!(", and {n} more", n = n),
            };
            tr!(
                "{count} garments moved: {shown}{more}",
                more = more,
                count = many.len(),
                shown = shown.join(", ")
            )
        }
    }
}

/// `said`, then what an edit moved, as one sentence.
pub(super) fn with_moves(said: String, moved: &[Moved]) -> String {
    let moves = moves_said(moved);
    match (said.is_empty(), moves.is_empty()) {
        (_, true) => said,
        (true, false) => moves,
        (false, false) => format!("{said}. {moves}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_wardrobe::import::LeftOut;

    fn moved(name: &str, from: u32, to: u32) -> Moved {
        Moved {
            garment: name.into(),
            name: name.into(),
            from,
            to,
        }
    }

    #[test]
    fn a_drop_says_how_many_it_added_and_names_the_first_file_it_left_out_and_why() {
        let added = Added {
            garments: vec!["a".into(), "b".into()],
            textures: 3,
            left_out: vec![
                LeftOut {
                    path: "mods/readme.ydd".into(),
                    why: "its name does not state what kind of file it is".into(),
                },
                LeftOut {
                    path: "mods/b.ymt".into(),
                    why: "a build writes the metadata".into(),
                },
            ],
            ..Added::default()
        };
        let said = describe(&added);
        assert!(said.starts_with("Added 2 garments, 3 textures"), "{said}");
        assert!(
            said.contains(
                "2 files left out, the first is mods/readme.ydd: its name does not state what kind of file it is; the Log has them"
            ),
            "{said}"
        );
        assert!(!said.contains("(s)"), "{said}");

        let one = Added {
            garments: vec!["a".into()],
            textures: 1,
            unread: 1,
            left_out: vec![LeftOut {
                path: "x.ymt".into(),
                why: "a .ymt".into(),
            }],
            ..Added::default()
        };
        let said = describe(&one);
        assert!(said.starts_with("Added 1 garment, 1 texture"), "{said}");
        assert!(
            said.contains("It names no ped or slot and is marked ?"),
            "{said}"
        );
        assert!(
            said.contains("1 file left out, the first is x.ymt: a .ymt; the Log has it"),
            "{said}"
        );
    }

    #[test]
    fn stray_pictures_are_one_line_of_the_log_and_every_other_file_left_out_keeps_its_own() {
        let stray = "an image is a texture of a garment only when its name says which, such as `jbib_diff_000_a_uni.png`";
        let left = |path: &str, why: &str| LeftOut {
            path: path.into(),
            why: why.into(),
        };
        let added = Added {
            left_out: vec![
                left("preview.png", stray),
                left(
                    "mods/readme.ydd",
                    "its name does not state what kind of file it is",
                ),
                left("cover.jpg", stray),
                left("mods/shot.tga", stray),
            ],
            ..Added::default()
        };
        let lines = notes_of(&added);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with("Left out mods/readme.ydd: "),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with(
                "Left out 3 pictures that name no texture (preview.png, cover.jpg, mods/shot.tga)"
            ),
            "{lines:?}"
        );
    }

    #[test]
    fn a_drop_of_garments_the_pack_holds_says_how_many_were_not_added_again() {
        let mut added = Added {
            already_held: vec!["a.ydd".into(), "b.ydd".into(), "c.ydd".into()],
            ..Added::default()
        };
        assert_eq!(
            describe(&added),
            "Nothing was added: the 3 garments of the drop are already in the pack"
        );
        added.already_held.truncate(1);
        assert_eq!(
            describe(&added),
            "Nothing was added: the garment of the drop is already in the pack"
        );
        added.garments = vec!["new".into()];
        added.textures = 1;
        assert_eq!(
            describe(&added),
            "Added 1 garment, 1 texture. 1 garment already in the pack, so it was not added again"
        );
    }

    #[test]
    fn what_an_edit_moved_is_one_sentence_that_names_who_and_where() {
        assert_eq!(moves_said(&[]), "");
        assert_eq!(
            moves_said(&[moved("coat", 5, 4)]),
            "Moved: coat from 005 to 004"
        );
        let many: Vec<Moved> = (1..=5).map(|i| moved(&format!("g{i}"), i + 1, i)).collect();
        assert_eq!(
            moves_said(&many),
            "5 garments moved: g1 from 002 to 001, g2 from 003 to 002, g3 from 004 to 003, and 2 more"
        );
        assert_eq!(
            with_moves("Removed coat from the pack".into(), &[moved("hat", 3, 2)]),
            "Removed coat from the pack. Moved: hat from 003 to 002"
        );
        assert_eq!(with_moves("Removed coat".into(), &[]), "Removed coat");
    }
}
