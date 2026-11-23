//! A GTA VI colour's maximum drawn where its count of colours in use enables it, an edit of the
//! enable saved as that count, and what is not known of the rest said.

use crate::state::gta6_particles_save::from_ypt;

use super::*;

const MIN: [([f32; 4], [f32; 4]); 1] = [([0.0; 4], [0.25, 0.5, 0.75, 1.0])];
const MAX: [([f32; 4], [f32; 4]); 1] = [([0.0; 4], [1.0, 0.5, 0.25, 0.5])];

/// The test list with a `ptxu_Colour` of `colours_used` and the maximum `max`, its behaviour 4.
fn coloured(colours_used: u32, max: &[([f32; 4], [f32; 4])]) -> ParticleEffects {
    let mut list = list();
    let kfps = vec![
        value(COLOUR_RGBA_KFPS[0], &MIN),
        value(COLOUR_RGBA_KFPS[1], max),
    ];
    let mut colour = behaviour(Some("ptxu_Colour"), 0x1E0, kfps);
    colour.fields = BehaviourFields::Colour { colours_used };
    list.behaviours.push(colour);
    list.particles[0].behaviours.push(4);
    list
}

fn enable(ypt: &mut PtxFxList) -> &mut Bool8 {
    let rule = or_particle::list::particle_rules_mut(ypt)
        .next()
        .expect("a particle rule");
    (rule.all_behaviours.iter_mut())
        .find_map(|b| match b {
            Ref::Own(b) => match &mut **b {
                PtxBehaviourAny::PtxuColour(c) => Some(&mut c.rgba_max_enable),
                _ => None,
            },
            Ref::To(_) => None,
        })
        .expect("a ptxu_Colour")
}

#[test]
fn a_colours_maximum_is_drawn_where_two_or_more_are_in_use_and_an_edit_saves_as_the_count() {
    for (used, drawn) in [(0, false), (1, false), (2, true), (3, true), (5, true)] {
        let stored = coloured(used, &MAX);
        let mut ypt = as_ypt(&stored);
        or_particle::list::canonical(&mut ypt);
        assert_eq!(enable(&mut ypt).get(), drawn, "{used} colours in use");
        assert_eq!(
            from_ypt(&stored, &ypt),
            Ok(stored.clone()),
            "{used} kept as read"
        );
        *enable(&mut ypt) = Bool8::new(!drawn);
        let edited = from_ypt(&stored, &ypt).expect("the enable has a place");
        let BehaviourFields::Colour { colours_used } = edited.behaviours[4].fields else {
            panic!("a ptxu_Colour's count");
        };
        assert_eq!(colours_used >= 2, !drawn, "{used} edited");
        let mut back = as_ypt(&edited);
        or_particle::list::canonical(&mut back);
        assert_eq!(enable(&mut back).get(), !drawn, "{used} read back");
    }
}

#[test]
fn an_enabled_empty_maximum_is_not_drawn_and_colours_past_two_are_said_to_be_not_known() {
    let stored = coloured(2, &[]);
    let mut ypt = as_ypt(&stored);
    or_particle::list::canonical(&mut ypt);
    assert!(!enable(&mut ypt).get(), "an empty maximum is left out");
    assert_eq!(from_ypt(&stored, &ypt), Ok(stored.clone()), "kept as read");
    let said = lines(&stored);
    assert!(said.iter().any(|l| l.contains("which holds no key")));

    let said = lines(&coloured(4, &MAX));
    assert!(said.iter().any(|l| l.contains("counts 4 colours in use")));
    assert!(!said.iter().any(|l| l.contains("which holds no key")));
    let said = lines(&coloured(2, &MAX));
    assert!(!said.iter().any(|l| l.contains("Its ptxu_Colour")));
}
