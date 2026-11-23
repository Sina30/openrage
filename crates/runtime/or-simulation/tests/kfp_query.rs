use glam::Vec4;
use or_io::gta5::particle::RawKeyframeEntry;
use or_particle::keyframe::KeyframePropData;
use or_particle::rules::{EvolutionList, EvolvedKeyframe, EvolvedKeyframeProp};
use or_simulation::kfp::{evo_blend, query_kpd_with_evo, MAX_EVOLUTIONS};
fn entry(time: f32, inv_dt: f32, val: f32) -> RawKeyframeEntry {
    RawKeyframeEntry {
        time: [time, inv_dt, 0.0, 0.0],
        value: [val, val, val, val],
    }
}

fn kpd(entries: Vec<RawKeyframeEntry>, property_id: u32) -> Option<KeyframePropData> {
    Some(KeyframePropData {
        entries,
        property_id,
        rand_index: 0,
        invert_bias_link: 0,
    })
}

#[test]
fn linear_interp_midpoint_is_average() {
    let kpd = kpd(vec![entry(0.0, 0.0, 0.0), entry(1.0, 1.0, 10.0)], 0xAA);
    let v = query_kpd_with_evo(&kpd, 0.5, None, &[], Vec4::ZERO);
    assert!((v.x - 5.0).abs() < 1e-4);
}

#[test]
fn evolution_add_blend_adds_evolved_value() {
    let property_id = 0xCAFE;
    let base = kpd(vec![entry(0.0, 0.0, 1.0)], property_id);
    let evo = EvolutionList {
        evolutions: Vec::new(),
        evolved_keyframe_props: vec![EvolvedKeyframeProp {
            evolved_keyframes: vec![EvolvedKeyframe {
                entries: vec![entry(0.0, 0.0, 4.0)],
                evolution_index: 0,
                is_lod_evolution: false,
            }],
            property_id,
            blend_mode: evo_blend::ADD,
        }],
    };
    let mut values = vec![0.0; MAX_EVOLUTIONS];
    values[0] = 1.0;
    let v = query_kpd_with_evo(&base, 0.0, Some(&evo), &values, Vec4::ZERO);
    assert!((v.x - 4.0).abs() < 1e-4, "got {v:?}");
}

#[test]
fn evolution_with_zero_slider_is_no_op() {
    let property_id = 0xDEAD;
    let base = kpd(vec![entry(0.0, 0.0, 2.0)], property_id);
    let evo = EvolutionList {
        evolutions: Vec::new(),
        evolved_keyframe_props: vec![EvolvedKeyframeProp {
            evolved_keyframes: vec![EvolvedKeyframe {
                entries: vec![entry(0.0, 0.0, 99.0)],
                evolution_index: 0,
                is_lod_evolution: false,
            }],
            property_id,
            blend_mode: evo_blend::ADD,
        }],
    };
    let values = vec![0.0; MAX_EVOLUTIONS];
    let v = query_kpd_with_evo(&base, 0.0, Some(&evo), &values, Vec4::ZERO);
    assert!(
        (v.x - 2.0).abs() < 1e-4,
        "evolution should be ignored with zero slider, got {v:?}"
    );
}

#[test]
fn evolution_property_id_mismatch_is_no_op() {
    let base = kpd(vec![entry(0.0, 0.0, 1.0)], 0xAAAA);
    let evo = EvolutionList {
        evolutions: Vec::new(),
        evolved_keyframe_props: vec![EvolvedKeyframeProp {
            evolved_keyframes: vec![EvolvedKeyframe {
                entries: vec![entry(0.0, 0.0, 99.0)],
                evolution_index: 0,
                is_lod_evolution: false,
            }],
            property_id: 0xBBBB,
            blend_mode: evo_blend::ADD,
        }],
    };
    let mut values = vec![0.0; MAX_EVOLUTIONS];
    values[0] = 1.0;
    let v = query_kpd_with_evo(&base, 0.0, Some(&evo), &values, Vec4::ZERO);
    assert!((v.x - 1.0).abs() < 1e-4);
}
