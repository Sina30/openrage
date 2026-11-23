//! A `.ycd` built as an owned value with invented content: one animation, two
//! clips chained in one bucket that both play it, a tag and a property, saved
//! with every pointer the file derives written non-null. Needs no game file.

use or_io::decl::{File, Keyed, Open, Owned, Raw, Record, View};
use or_io::gta5::clip_dict::entries;
use or_io::gta5::*;

const WALK: u32 = 0x5741_4C4B;
const ONE: u32 = 0x0000_0C01;
const TWO: u32 = 0x0000_0C02;

fn name(string: &str, key: u32) -> StringKey {
    StringKey {
        string: string.into(),
        key,
        ..Default::default()
    }
}

fn property(key: u32) -> CrProperty {
    let attribute = CrPropertyAttributeFloat {
        base: CrPropertyAttribute {
            r#type: Open::new(EType::Float),
            name: name("weight", 0xA7),
            ..Default::default()
        },
        float: 1.5,
        ..Default::default()
    };
    CrProperty {
        name: name("blend", key),
        attributes: vec![Some(Box::new(
            CrPropertyAttributeAny::CrPropertyAttributeFloat(attribute),
        ))],
        ..Default::default()
    }
}

fn one_bucket<T: Owned>(entries: Vec<(u32, T)>) -> AtMap<T>
where
    Raw<T>: Record,
{
    let head = entries.into_iter().rev().fold(None, |next, (key, data)| {
        Some(Box::new(AtMapEntry {
            key,
            data: Some(Box::new(data)),
            next,
            ..Default::default()
        }))
    });
    AtMap {
        hash: vec![head],
        ..Default::default()
    }
}

fn authored(plays: u32) -> CrClipDictionary {
    let walk = CrAnimation {
        num_frames: 2,
        frames_per_chunk: 127,
        duration: 1.0 / 30.0,
        name: "walk".into(),
        blocks: vec![Some(Box::new(CrBlockStream {
            num_frames: 2,
            data: vec![1, 2, 3, 4],
            ..Default::default()
        }))],
        dofs: vec![Dof {
            id: 0x2E28,
            track: 1,
            ..Default::default()
        }],
        ..Default::default()
    };
    let tag = CrTag {
        base: property(0xB1),
        start: 0.25,
        end: 0.5,
        ..Default::default()
    };
    let one = CrClipAnimation {
        base: CrClip {
            r#type: Open::new(EClipType::Animation),
            name: AtString::new("one"),
            tags: Some(Box::new(CrTags {
                tags: vec![Some(Box::new(tag))],
                ..Default::default()
            })),
            properties: Some(Box::new(CrProperties {
                properties: one_bucket(vec![(0xB2, property(0xB2))]),
                ..Default::default()
            })),
            ..Default::default()
        },
        animation: Keyed::to(plays),
        end_time: 1.0 / 30.0,
        rate: 1.0,
        ..Default::default()
    };
    let two = CrClipAnimations {
        base: CrClip {
            r#type: Open::new(EClipType::Animations),
            name: AtString::new("two"),
            ..Default::default()
        },
        animations: vec![Animation {
            animation: Keyed::to(plays),
            rate: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    CrClipDictionary {
        anim_dictionary: Some(Box::new(CrAnimDictionary {
            animations: one_bucket(vec![(WALK, walk)]),
            ..Default::default()
        })),
        clips: one_bucket(vec![
            (ONE, CrClipAny::CrClipAnimation(one)),
            (TWO, CrClipAny::CrClipAnimations(two)),
        ]),
        ..Default::default()
    }
}

#[test]
fn an_authored_ycd_reopens_equal_and_reaches_a_fixed_point() {
    let first = authored(WALK).to_bytes().unwrap();
    let back = CrClipDictionary::from_bytes(&first).unwrap();
    assert_eq!(back, authored(WALK));
    assert_eq!(back.to_bytes().unwrap(), first);
}

/// ⚠ A null here reads as a clip with no dictionary, no animation or a tag in no list.
#[test]
fn an_authored_ycd_saves_every_reference_non_null() {
    let file = authored(WALK).to_bytes().unwrap();
    let peek = CrClipDictionary::peek(&file).unwrap();
    let root = peek.root().unwrap();
    let anims = root.anim_dictionary().unwrap().expect("the animations");
    let animations = entries(&anims.animations()).unwrap();
    assert_eq!(animations.len(), 1);
    let walk = animations[0].1.data().unwrap().expect("the animation");
    assert_eq!(
        { walk.ref_count },
        3,
        "two clips naming it and the entry owning it"
    );
    let at = |v: Option<View<'_, Raw<CrAnimation>>>| v.map(|v| View::offset(&v));
    let walk_at = Some(View::offset(&walk));
    let clips = entries(&root.clips()).unwrap();
    assert_eq!(
        clips.iter().map(|(b, e)| (*b, e.key)).collect::<Vec<_>>(),
        [(0, ONE), (0, TWO)]
    );
    let mut named = 0;
    for (_, entry) in &clips {
        let clip = entry.data().unwrap().expect("a clip");
        assert_eq!({ clip.ref_count }, 1);
        let dictionary = clip.dictionary().unwrap().expect("the dictionary");
        assert_eq!(View::offset(&dictionary), View::offset(&root));
        if let Some(c) = View::variant::<Raw<CrClipAnimation>>(&clip).unwrap() {
            assert_eq!(at(c.animation().unwrap()), walk_at);
            named += 1;
            let tags = clip.tags().unwrap().expect("the tags");
            let tag = tags.tags().unwrap().follow(0).unwrap().expect("a tag");
            let list = tag.tags().unwrap().expect("the list holding the tag");
            assert_eq!(View::offset(&list), View::offset(&tags));
        }
        if let Some(c) = View::variant::<Raw<CrClipAnimations>>(&clip).unwrap() {
            for a in c.animations().unwrap().views() {
                assert_eq!(at(a.animation().unwrap()), walk_at);
                named += 1;
            }
        }
    }
    assert_eq!(named, 2);
}

#[test]
fn a_clip_naming_an_animation_the_dictionary_lacks_is_refused() {
    let why = authored(WALK + 1).to_bytes().unwrap_err().to_string();
    assert!(why.contains("CrClipAnimation::animation"), "{why}");
}
