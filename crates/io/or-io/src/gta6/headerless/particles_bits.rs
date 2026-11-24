//! A particle rule's class word: one bit per class of the behaviours it lists, the k-th set bit
//! the k-th behaviour's, and the three masks of it the rule also holds.

use super::*;

/// How the class a bit stands for is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassNamedBy {
    /// A keyframed value its behaviours hold names the class.
    Kfp,
    /// The class GTA V's update order holds at the bit's place among the named ones.
    Order,
    /// GTA V has no class at the bit's place.
    Nothing,
}

/// Each bit a rule may set, the class it stands for and how that class is named.
const CLASS_BITS: [(u8, Option<&str>, ClassNamedBy); 28] = [
    (0, Some("ptxu_CameraEvolution"), ClassNamedBy::Kfp),
    (1, Some("ptxu_Age"), ClassNamedBy::Order),
    (2, Some("ptxu_Acceleration"), ClassNamedBy::Kfp),
    (3, Some("ptxu_Attractor"), ClassNamedBy::Kfp),
    (4, Some("ptxu_Velocity"), ClassNamedBy::Kfp),
    (5, Some("ptxu_Rotation"), ClassNamedBy::Kfp),
    (6, Some("ptxu_Noise"), ClassNamedBy::Order),
    (7, Some("ptxu_Size"), ClassNamedBy::Kfp),
    (8, Some("ptxu_Dampening"), ClassNamedBy::Kfp),
    (9, Some("ptxu_MatrixWeight"), ClassNamedBy::Kfp),
    (10, Some("ptxu_River"), ClassNamedBy::Order),
    (11, Some("ptxu_Wind"), ClassNamedBy::Order),
    (12, Some("ptxu_ZCull"), ClassNamedBy::Kfp),
    (14, None, ClassNamedBy::Nothing),
    (16, Some("ptxu_Collision"), ClassNamedBy::Kfp),
    (17, Some("ptxu_AnimateTexture"), ClassNamedBy::Kfp),
    (18, None, ClassNamedBy::Nothing),
    (19, Some("ptxu_Colour"), ClassNamedBy::Kfp),
    (20, Some("ptxu_Fire"), ClassNamedBy::Order),
    (21, Some("ptxu_Decal"), ClassNamedBy::Kfp),
    // GTA V's order holds three classes here: DecalPool, DecalTrail and Liquid.
    (22, None, ClassNamedBy::Nothing),
    (23, Some("ptxu_Light"), ClassNamedBy::Kfp),
    (24, Some("ptxu_FogVolume"), ClassNamedBy::Kfp),
    (25, None, ClassNamedBy::Nothing),
    (26, None, ClassNamedBy::Nothing),
    (27, Some("ptxd_Sprite"), ClassNamedBy::Order),
    (28, Some("ptxd_Trail"), ClassNamedBy::Kfp),
    (29, Some("ptxd_Model"), ClassNamedBy::Order),
];

/// The class bit of `ptxu_ZCull`, which a rule with a third spawner holds.
const ZCULL_BIT: u8 = 12;
/// Bits no rule sets.
const NEVER: u32 = (1 << 13) | (1 << 15) | (1 << 30) | (1 << 31);
/// The bits +0x198 drops from the class word.
const UPDATE_DROP: u32 = (1 << 18) | (7 << 27);
/// The bits +0x1A0 keeps of it.
const FINALIZE: u32 = 1 << 19;
/// The bits +0x1A8 keeps of it: 0, 19, 20, 23, 24 and 25.
const LAST: u32 = 0x0398_0001;

/// The class `bit` stands for, if any is known, and how it is named; `None` for a bit no rule sets.
pub fn class_of_bit(bit: u8) -> Option<(Option<&'static str>, ClassNamedBy)> {
    let &(_, class, by) = CLASS_BITS.iter().find(|(b, _, _)| *b == bit)?;
    Some((class, by))
}

fn bit_of_class(class: &str) -> Option<u8> {
    let &(bit, _, _) = CLASS_BITS.iter().find(|(_, c, _)| *c == Some(class))?;
    Some(bit)
}

fn set(word: u32, bit: u8) -> bool {
    (word >> bit) & 1 == 1
}

/// The class word of the rule at `at`, refused where its masks, its pads or its draw bits break
/// the rules measured.
pub(super) fn rule_words(v: &Bytes, at: usize, draw_type: u8) -> Result<u32> {
    const WHAT: &str = "particle rule";
    let bits = v.u32(WHAT, at + 0x180)?;
    need(
        bits & NEVER == 0,
        "a rule's class word sets bit 13, 15, 30 or 31, which no rule sets",
    )?;
    for (o, mask) in [
        (0x198, bits & !UPDATE_DROP),
        (0x1A0, bits & FINALIZE),
        (0x1A8, bits & LAST),
    ] {
        need(
            v.u32(WHAT, at + o)? == mask,
            "a rule's words at +0x198, +0x1A0 and +0x1A8 are not the masks of its class word",
        )?;
    }
    for o in [0x184, 0x19C, 0x1A4, 0x1AC] {
        need(
            v.u32(WHAT, at + o)? == 0,
            "a rule's words at +0x184, +0x19C, +0x1A4 and +0x1AC are not 0",
        )?;
    }
    let drawn = set(bits, 29) == (draw_type == 1)
        && set(bits, 28) == (draw_type == 2)
        && (!set(bits, 27) || draw_type == 0);
    need(
        drawn,
        "a rule's class word disagrees with its draw type at bit 27, 28 or 29",
    )?;
    Ok(bits)
}

/// The class word `bits` and its three masks, each by its offset in a rule.
pub(super) fn class_words(bits: u32) -> [(usize, u32); 4] {
    [
        (0x180, bits),
        (0x198, bits & !UPDATE_DROP),
        (0x1A0, bits & FINALIZE),
        (0x1A8, bits & LAST),
    ]
}

/// Each behaviour given its bit: the k-th set bit of a rule's class word is the k-th behaviour it
/// lists. Refused where the word sets another count of bits, a named class is not at its bit, two
/// rules give one behaviour two bits, or a third spawner starts an effect with no `ptxu_ZCull`.
pub(super) fn assign(behaviours: &mut [Behaviour], rules: &[ParticleRule]) -> Result<()> {
    let mut given: Vec<Option<u8>> = vec![None; behaviours.len()];
    for rule in rules {
        need(
            rule.class_bits.count_ones() as usize == rule.behaviours.len(),
            "a rule's class word does not set one bit for each behaviour it lists",
        )?;
        let bits = (0..32u8).filter(|&bit| set(rule.class_bits, bit));
        for (bit, &b) in bits.zip(&rule.behaviours) {
            let Some(held) = behaviours.get(b) else {
                return refuse("a rule lists a behaviour the list does not hold");
            };
            need(
                held.class
                    .is_none_or(|class| bit_of_class(class) == Some(bit)),
                "a behaviour's keyframed values name a class other than its bit's",
            )?;
            match given.get_mut(b) {
                Some(Some(was)) => {
                    need(*was == bit, "two rules give one behaviour two class bits")?
                }
                Some(slot) => *slot = Some(bit),
                None => return refuse("a rule lists a behaviour the list does not hold"),
            }
        }
        need(
            rule.spawners[2].effect.is_none() || set(rule.class_bits, ZCULL_BIT),
            "a rule's third spawner starts an effect and the rule holds no ptxu_ZCull",
        )?;
    }
    for (held, bit) in behaviours.iter_mut().zip(given) {
        held.class_bit = bit;
    }
    Ok(())
}

/// `ptxu_ZCull`'s mode and reference space of the behaviour at `at`, which runs to `next`; none
/// where it is shorter than its words.
pub(super) fn zcull(v: &Bytes, at: usize, next: usize) -> Result<BehaviourFields> {
    const WHAT: &str = "ptxu_ZCull";
    if next < at + 0xA0 {
        return Ok(BehaviourFields::None);
    }
    Ok(BehaviourFields::ZCull {
        mode: v.u32(WHAT, at + 0x94)? as i32,
        reference_space: v.u32(WHAT, at + 0x98)? as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_class_has_one_bit_and_the_bits_ascend() {
        let bits: Vec<u8> = CLASS_BITS.iter().map(|(b, _, _)| *b).collect();
        assert!(bits.windows(2).all(|w| w[0] < w[1]));
        assert!(bits.iter().all(|&b| NEVER & (1 << b) == 0));
        assert_eq!(bit_of_class("ptxu_ZCull"), Some(ZCULL_BIT));
        assert_eq!(
            class_of_bit(11),
            Some((Some("ptxu_Wind"), ClassNamedBy::Order))
        );
        assert_eq!(class_of_bit(18), Some((None, ClassNamedBy::Nothing)));
        assert_eq!(class_of_bit(13), None);
        // Every class a keyframed value names has its bit.
        for (_, name) in BEHAVIOUR_KFPS {
            let class = name.split(':').next().unwrap_or_default();
            assert!(bit_of_class(class).is_some(), "{class}");
        }
    }

    #[test]
    fn a_behaviour_no_rule_lists_has_no_class_bit() {
        let held = Behaviour {
            at: 0,
            class: Some("ptxu_Acceleration"),
            class_bit: Some(0),
            size: 0x10,
            kfps: Vec::new(),
            fields: BehaviourFields::None,
        };
        let mut behaviours = [held];
        assign(&mut behaviours, &[]).expect("no rule to break");
        assert_eq!(behaviours[0].class_bit, None);
    }
}
