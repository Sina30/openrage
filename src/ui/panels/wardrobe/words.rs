//! What the Wardrobe calls a slot and a ped, in the words of someone who dresses one.

use or_wardrobe::{Component, Ped, Prop, Slot};

/// The garments of a slot, as a heading: `Tops` for `jbib`.
pub fn slot_words(slot: Slot) -> &'static str {
    match slot {
        Slot::Component(c) => match c {
            Component::Head => tr!("Heads"),
            Component::Berd => tr!("Masks"),
            Component::Hair => tr!("Hair"),
            Component::Uppr => tr!("Torsos"),
            Component::Lowr => tr!("Legs"),
            Component::Hand => tr!("Bags"),
            Component::Feet => tr!("Shoes"),
            Component::Teef => tr!("Accessories"),
            Component::Accs => tr!("Undershirts"),
            Component::Task => tr!("Armor"),
            Component::Decl => tr!("Decals"),
            Component::Jbib => tr!("Tops"),
        },
        Slot::Prop(p) => match p {
            Prop::Head => tr!("Hats"),
            Prop::Eyes => tr!("Glasses"),
            Prop::Ears => tr!("Earrings"),
            Prop::Mouth => tr!("Mouth props"),
            Prop::LeftHand => tr!("Left hand props"),
            Prop::RightHand => tr!("Right hand props"),
            Prop::LeftWrist => tr!("Left wrist props"),
            Prop::RightWrist => tr!("Right wrist props"),
            Prop::Hip => tr!("Hip props"),
        },
    }
}

/// Where a slot comes in a list: Tops first, which is what most packs are for, then the rest as the game numbers them.
pub fn slot_order(slot: Option<Slot>) -> (bool, Option<Slot>) {
    (slot != Some(Slot::Component(Component::Jbib)), slot)
}

/// The characters a name is made of words at: a pack's names are `ped_collection^slot_number_skin`.
const BOUNDARIES: [char; 5] = ['_', '^', ' ', '-', '.'];

/// `names` as a list shows them: each without the words the others begin and end with, so that what tells a garment apart is what is left.
///
/// ⚠ Cut at a boundary and never to nothing: `a_b_1` and `a_b_2` are `1` and `2`, while `a_b` and `a_b_c` stay as they are.
pub fn distinct_parts(names: &[&str]) -> Vec<String> {
    let whole = || names.iter().map(|n| (*n).to_string()).collect();
    let [first, rest @ ..] = names else {
        return Vec::new();
    };
    if rest.is_empty() {
        return whole();
    }
    let chars: Vec<Vec<char>> = names.iter().map(|n| n.chars().collect()).collect();
    let shared = |from_end: bool| {
        let mut n = 0;
        'grow: loop {
            let at = |c: &Vec<char>| match from_end {
                false => c.get(n).copied(),
                true => c.len().checked_sub(n + 1).and_then(|i| c.get(i).copied()),
            };
            let Some(want) = at(&chars[0]) else {
                break 'grow;
            };
            if chars.iter().any(|c| at(c) != Some(want)) {
                break 'grow;
            }
            n += 1;
        }
        n
    };
    // The shared beginning ends after its last boundary; the shared end begins at its first.
    let begin = first.chars().take(shared(false)).collect::<Vec<_>>();
    let cut_front = begin
        .iter()
        .rposition(|c| BOUNDARIES.contains(c))
        .map_or(0, |i| i + 1);
    let end = chars[0][chars[0].len() - shared(true)..].to_vec();
    let cut_back = end
        .iter()
        .position(|c| BOUNDARIES.contains(c))
        .map_or(0, |i| end.len() - i);
    let cut = |front: usize, back: usize| -> Option<Vec<String>> {
        chars
            .iter()
            .map(|c| {
                (front + back < c.len())
                    .then(|| c[front..c.len() - back].iter().collect::<String>())
            })
            .collect()
    };
    cut(cut_front, cut_back)
        .or_else(|| cut(cut_front, 0))
        .unwrap_or_else(whole)
}

/// `text` with its middle replaced by `…` until `fits`, so that what a name ends with, where names differ, stays in sight.
///
/// ⚠ A third of what is kept is the beginning: the end tells `jbib_000_u` from `jbib_001_u`, and the beginning only says what the name is of.
pub fn cut_in_the_middle(text: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let made = |kept: usize| {
        let front = kept / 3;
        let (head, tail) = (&chars[..front], &chars[chars.len() - (kept - front)..]);
        format!(
            "{}…{}",
            head.iter().collect::<String>(),
            tail.iter().collect::<String>()
        )
    };
    // The most characters that still fit: `fits` only gets harder as more are kept.
    let (mut low, mut high) = (0, chars.len());
    while low + 1 < high {
        let middle = (low + high) / 2;
        match fits(&made(middle)) {
            true => low = middle,
            false => high = middle,
        }
    }
    made(low)
}

/// A ped, or the question mark of a name that said none.
pub fn ped_words(ped: Option<Ped>) -> &'static str {
    match ped {
        Some(Ped::Male) => tr!("Male"),
        Some(Ped::Female) => tr!("Female"),
        None => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tops_come_first_and_the_other_slots_keep_the_order_the_game_numbers_them_in() {
        let mut slots: Vec<Option<Slot>> = Slot::all().map(Some).collect();
        slots.sort_by_key(|s| slot_order(*s));
        assert_eq!(slots[0], Some(Slot::Component(Component::Jbib)));
        let rest: Vec<Option<Slot>> = slots[1..].to_vec();
        let mut numbered: Vec<Option<Slot>> = Slot::all()
            .filter(|s| *s != Slot::Component(Component::Jbib))
            .map(Some)
            .collect();
        numbered.sort();
        assert_eq!(rest, numbered);
    }

    #[test]
    fn a_garment_is_called_by_what_the_others_of_its_pack_do_not_say() {
        assert_eq!(distinct_parts(&["a_b_1", "a_b_2"]), ["1", "2"]);
        assert_eq!(
            distinct_parts(&[
                "mp_m_freemode_01_mp_m_harbor^jbib_000_u",
                "mp_m_freemode_01_mp_m_harbor^lowr_001_u",
                "mp_m_freemode_01_mp_m_harbor^feet_000_u",
            ]),
            ["jbib_000", "lowr_001", "feet_000"]
        );
        // What the names are cut to is never nothing, and a lone name is whole.
        assert_eq!(distinct_parts(&["a_b", "a_b_c"]), ["b", "b_c"]);
        assert_eq!(distinct_parts(&["_a_", "_a_"]), ["_a_", "_a_"]);
        assert_eq!(distinct_parts(&["my cap"]), ["my cap"]);
        assert_eq!(distinct_parts(&["same", "same"]), ["same", "same"]);
        assert!(distinct_parts(&[]).is_empty());
        // Words, not letters: the shared beginning stops at the boundary before the letters that differ.
        assert_eq!(
            distinct_parts(&["red jacket", "red jumper"]),
            ["jacket", "jumper"]
        );
        assert_eq!(distinct_parts(&["x^red", "x^redder"]), ["red", "redder"]);
    }

    #[test]
    fn a_name_that_is_too_long_is_cut_in_the_middle_and_keeps_its_end() {
        let fits = |width: usize| move |t: &str| t.chars().count() <= width;
        let long = "mp_m_freemode_01_mp_m_unity_clothes^jbib_000_u";
        assert_eq!(cut_in_the_middle(long, fits(long.chars().count())), long);
        for width in [12, 20, 30] {
            let cut = cut_in_the_middle(long, fits(width));
            assert!(cut.chars().count() <= width, "{cut}");
            assert!(cut.contains('…') && cut.ends_with("_000_u"), "{cut}");
        }
        // Two names that differ only at their end are still told apart once cut.
        let (a, b) = (
            cut_in_the_middle("mp_m_freemode_01_mp_m_unity_clothes^jbib_000_u", fits(20)),
            cut_in_the_middle("mp_m_freemode_01_mp_m_unity_clothes^jbib_001_u", fits(20)),
        );
        assert_ne!(a, b);
        assert_eq!(cut_in_the_middle("ab", fits(1)), "…");
    }

    #[test]
    fn every_slot_has_a_heading_of_its_own() {
        let words: std::collections::BTreeSet<&str> = Slot::all().map(slot_words).collect();
        assert_eq!(words.len(), Slot::all().count());
    }
}
