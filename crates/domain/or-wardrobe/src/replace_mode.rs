//! Replace mode: the files of a ped take the place of the game's own, in a collection of the game or in the ped itself.
//!
//! A replacement is named for the game's collection and the game's drawable number, so a pack writes the files and none
//! of the metadata that states them: the game's own stays. ⚠ Nobody has seen the game load a pack that replaces its
//! files, so every sentence here says so, and a build never claims it does.

use or_project::Contribution;

use crate::alloc::Allocation;
use crate::layout;
use crate::model::Wardrobe;
use crate::names::Ped;
use crate::plan::Build;

/// What a screen and a report say beside any replacement.
pub const UNVERIFIED: &str =
    "not verified in game: nobody has seen the game load a pack that replaces its files";

/// What the files of `ped` replace, as a sentence says it, none where the pack adds a collection of its own.
pub fn replaced(wardrobe: &Wardrobe, ped: Ped) -> Option<String> {
    let replace = wardrobe.replaces(ped)?;
    Some(match &replace.dlc {
        Some(dlc) => format!(
            "the collection {dlc} of the game ({})",
            layout::stem(ped, dlc, false)
        ),
        None => format!(
            "the drawables of the base game's {ped} ({})",
            layout::stem(ped, "", false)
        ),
    })
}

/// Say in the plan what each replaced ped's files replace, and refuse a build that cannot ship a replacement.
pub fn note(
    wardrobe: &Wardrobe,
    allocation: &Allocation,
    build: &Build,
    contribution: &mut Contribution,
) {
    for ped in Ped::ALL {
        let Some(what) = replaced(wardrobe, ped) else {
            continue;
        };
        let mine = || allocation.placed.iter().filter(move |p| p.ped == ped);
        if mine().next().is_none() {
            continue;
        }
        if build.is_pack() {
            contribution.problems.push(format!(
                "the files of {ped} replace {what}, and a singleplayer DLC adds collections: a replacement ships in a FiveM resource"
            ));
            continue;
        }
        let files = mine()
            .filter(|p| !wardrobe.garments[p.garment].is_reserved())
            .count();
        contribution.notes.push(format!(
            "replace mode: {files} garment(s) of {ped} replace {what}, and the game's own metadata for it is not written, so a replaced drawable keeps the textures and flags the game states; {UNVERIFIED}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Replace;

    #[test]
    fn a_collection_and_the_base_game_are_each_named_by_the_files_that_replace_them() {
        let mut w = Wardrobe::new("p");
        w.rules.replace.insert(
            Ped::Male,
            Replace {
                dlc: Some("mp_m_heist".into()),
            },
        );
        w.rules.replace.insert(Ped::Female, Replace::default());
        assert_eq!(
            replaced(&w, Ped::Male).as_deref(),
            Some("the collection mp_m_heist of the game (mp_m_freemode_01_mp_m_heist)")
        );
        assert_eq!(
            replaced(&w, Ped::Female).as_deref(),
            Some("the drawables of the base game's mp_f_freemode_01 (mp_f_freemode_01)")
        );
        assert_eq!(replaced(&Wardrobe::new("p"), Ped::Male), None);
    }
}
