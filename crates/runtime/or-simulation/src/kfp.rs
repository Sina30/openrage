use crate::random::RandomTable;
use glam::Vec4;
use or_io::gta5::particle::RawKeyframeEntry;
use or_io::gta5::particle::{
    PtxBiasLink, PtxEffectRule, PtxEmitterRule, PtxKeyframeEntry, PtxKeyframeProp, ShaderVar,
};
use or_particle::behaviour::Behaviour;
use or_particle::keyframe::KeyframePropData;
use or_particle::rules::{EvolutionList, EvolvedKeyframeProp};
pub const MAX_EVOLUTIONS: usize = 8;
pub mod evo_blend {
    pub const AVERAGE: u32 = 0;
    pub const ADD: u32 = 1;
    pub const MAX: u32 = 2;
    pub const FULL_AVERAGE: u32 = 3;
}

pub fn query_kfp(kfp: &PtxKeyframeProp, time: f32) -> Vec4 {
    query_entries(&kfp.keyframe.entries, time)
}

fn query_entries(entries: &[PtxKeyframeEntry], time: f32) -> Vec4 {
    if entries.is_empty() {
        return Vec4::ZERO;
    }
    if entries.len() == 1 {
        return lanes(&entries[0]);
    }
    let first = &entries[0];
    if time <= first.time.x {
        return lanes(first);
    }
    for i in 1..entries.len() {
        let curr = &entries[i];
        if time < curr.time.x {
            let prev = &entries[i - 1];
            let ratio = ((time - prev.time.x) * curr.time.y).clamp(0.0, 1.0);
            return lanes(prev).lerp(lanes(curr), ratio);
        }
    }
    entries.last().map_or(Vec4::ZERO, lanes)
}

fn lanes(e: &PtxKeyframeEntry) -> Vec4 {
    Vec4::new(e.value.x, e.value.y, e.value.z, e.value.w)
}

pub fn query_kpd(kpd: &Option<KeyframePropData>, time: f32, default_value: Vec4) -> Vec4 {
    match kpd {
        Some(k) if !k.entries.is_empty() => query_raw_entries(&k.entries, time),
        _ => default_value,
    }
}

pub fn query_raw_entries(entries: &[RawKeyframeEntry], time: f32) -> Vec4 {
    if entries.is_empty() {
        return Vec4::ZERO;
    }
    if entries.len() == 1 {
        let v = entries[0].value;
        return Vec4::new(v[0], v[1], v[2], v[3]);
    }
    let first = &entries[0];
    if time <= first.time[0] {
        let v = first.value;
        return Vec4::new(v[0], v[1], v[2], v[3]);
    }
    for i in 1..entries.len() {
        let curr = &entries[i];
        if time < curr.time[0] {
            let prev = &entries[i - 1];
            let ratio = ((time - prev.time[0]) * curr.time[1]).clamp(0.0, 1.0);
            let pv = Vec4::new(prev.value[0], prev.value[1], prev.value[2], prev.value[3]);
            let cv = Vec4::new(curr.value[0], curr.value[1], curr.value[2], curr.value[3]);
            return pv.lerp(cv, ratio);
        }
    }
    let last = entries.last().unwrap();
    Vec4::new(last.value[0], last.value[1], last.value[2], last.value[3])
}

pub fn query_kpd_with_evo(
    kpd: &Option<KeyframePropData>,
    time: f32,
    evo_list: Option<&EvolutionList>,
    evo_values: &[f32],
    default_value: Vec4,
) -> Vec4 {
    let base = query_kpd(kpd, time, default_value);
    let Some(kpd) = kpd.as_ref() else {
        return base;
    };
    let Some(list) = evo_list else {
        return base;
    };
    blend_evolved(base, kpd.property_id, list, evo_values, time)
}

pub fn query_kfp_with_evo(
    kfp: &PtxKeyframeProp,
    time: f32,
    evo_list: Option<&EvolutionList>,
    evo_values: &[f32],
) -> Vec4 {
    let base = query_kfp(kfp, time);
    let Some(list) = evo_list else {
        return base;
    };
    blend_evolved(base, kfp.property_id.get(), list, evo_values, time)
}

fn blend_evolved(
    base: Vec4,
    property_id: u32,
    list: &EvolutionList,
    evo_values: &[f32],
    time: f32,
) -> Vec4 {
    let mut result = base;
    for prop in &list.evolved_keyframe_props {
        if prop.property_id != property_id {
            continue;
        }
        result = apply_evolved_prop(result, prop, evo_values, time);
    }
    result
}

fn apply_evolved_prop(
    base: Vec4,
    prop: &EvolvedKeyframeProp,
    evo_values: &[f32],
    time: f32,
) -> Vec4 {
    let weight_of = |ek_idx: usize| -> f32 {
        let i = prop.evolved_keyframes[ek_idx].evolution_index as usize;
        evo_values.get(i).copied().unwrap_or(0.0)
    };
    let curve_of = |ek_idx: usize| -> Vec4 {
        query_raw_entries(&prop.evolved_keyframes[ek_idx].entries, time)
    };
    let mut lod: Option<(f32, Vec4)> = None;
    for (i, ek) in prop.evolved_keyframes.iter().enumerate() {
        if ek.is_lod_evolution {
            let w = weight_of(i);
            if w > 0.0 {
                lod = Some((w, curve_of(i)));
                break;
            }
        }
    }

    let mut result = match prop.blend_mode {
        evo_blend::ADD => {
            let mut accum = Vec4::ZERO;
            for (i, ek) in prop.evolved_keyframes.iter().enumerate() {
                if ek.is_lod_evolution {
                    continue;
                }
                let w = weight_of(i);
                if w <= 0.0 {
                    continue;
                }
                let evolved = curve_of(i);
                accum += (evolved - base) * w;
            }
            base + accum
        }
        evo_blend::MAX => {
            let mut curr = base;
            for (i, ek) in prop.evolved_keyframes.iter().enumerate() {
                if ek.is_lod_evolution {
                    continue;
                }
                let w = weight_of(i);
                if w <= 0.0 {
                    continue;
                }
                let evolved = curve_of(i);
                let lerped = base.lerp(evolved, w);
                curr = Vec4::new(
                    curr.x.max(lerped.x),
                    curr.y.max(lerped.y),
                    curr.z.max(lerped.z),
                    curr.w.max(lerped.w),
                );
            }
            curr
        }
        evo_blend::FULL_AVERAGE => {
            let mut accum = Vec4::ZERO;
            let mut count = 0.0_f32;
            for (i, ek) in prop.evolved_keyframes.iter().enumerate() {
                if ek.is_lod_evolution {
                    continue;
                }
                let w = weight_of(i);
                let evolved = curve_of(i);
                accum += base.lerp(evolved, w);
                count += 1.0;
            }
            if count > 0.0 {
                accum * (1.0 / count)
            } else {
                base
            }
        }
        _ => {
            let mut accum = Vec4::ZERO;
            let mut count = 0.0_f32;
            for (i, ek) in prop.evolved_keyframes.iter().enumerate() {
                if ek.is_lod_evolution {
                    continue;
                }
                let w = weight_of(i);
                if w <= 0.0 {
                    continue;
                }
                let evolved = curve_of(i);
                accum += base.lerp(evolved, w);
                count += 1.0;
            }
            if count > 0.0 {
                accum * (1.0 / count)
            } else {
                base
            }
        }
    };

    if let Some((lod_w, lod_v)) = lod {
        result = result + (lod_v - result) * lod_w;
    }

    result
}

pub fn bias_value_kpd(
    table: &RandomTable,
    kpd: &Option<KeyframePropData>,
    particle_rand_index: u8,
) -> f32 {
    let (kpd_rand, invert) = kpd
        .as_ref()
        .map(|k| (k.rand_index as i32, k.invert_bias_link != 0))
        .unwrap_or((0, false));
    let combined = ((particle_rand_index as i32).wrapping_add(kpd_rand)) as u32;
    let v = table.value(combined);
    if invert {
        1.0 - v
    } else {
        v
    }
}

pub fn bias_value(table: &RandomTable, kfp: &PtxKeyframeProp, particle_rand_index: u8) -> f32 {
    let combined = (particle_rand_index as u32).wrapping_add(kfp.rand_index as u32);
    let v = table.value(combined);
    if kfp.invert_bias_link.get() {
        1.0 - v
    } else {
        v
    }
}

pub fn apply_bias_links_to_behaviours(bias_links: &[PtxBiasLink], behaviours: &mut [Behaviour]) {
    if bias_links.is_empty() {
        return;
    }
    let overrides = build_bias_link_overrides(bias_links);
    for behaviour in behaviours {
        apply_to_behaviour(behaviour, &overrides);
    }
}

pub fn apply_bias_links_to_emitter_rule(bias_links: &[PtxBiasLink], emitter: &mut PtxEmitterRule) {
    if bias_links.is_empty() {
        return;
    }
    let overrides = build_bias_link_overrides(bias_links);
    for kfp in [
        &mut emitter.spawn_rate_over_time_kfp,
        &mut emitter.spawn_rate_over_dist_kfp,
        &mut emitter.particle_life_kfp,
        &mut emitter.playback_rate_scalar_kfp,
        &mut emitter.speed_scalar_kfp,
        &mut emitter.size_scalar_kfp,
        &mut emitter.accn_scalar_kfp,
        &mut emitter.dampening_scalar_kfp,
        &mut emitter.matrix_weight_scalar_kfp,
        &mut emitter.inherit_velocity_kfp,
    ] {
        if let Some(&idx) = overrides.get(&kfp.property_id.get()) {
            kfp.rand_index = idx;
        }
    }
}

pub fn apply_bias_links_to_effect_rule(
    bias_links: &[PtxBiasLink],
    effect: &mut PtxEffectRule<or_io::decl::Gen8>,
) {
    if bias_links.is_empty() {
        return;
    }
    let overrides = build_bias_link_overrides(bias_links);
    for kfp in [
        &mut effect.colour_tint_min_kfp,
        &mut effect.colour_tint_max_kfp,
        &mut effect.zoom_scalar_kfp,
        &mut effect.data_sphere_kfp,
        &mut effect.data_capsule_kfp,
    ] {
        if let Some(&idx) = overrides.get(&kfp.property_id.get()) {
            kfp.rand_index = idx;
        }
    }
}

#[inline]
fn build_bias_link_overrides(bias_links: &[PtxBiasLink]) -> std::collections::HashMap<u32, i8> {
    let mut overrides: std::collections::HashMap<u32, i8> = std::collections::HashMap::new();
    for link in bias_links {
        for prop_id in &link.keyframe_prop_ids {
            overrides.insert(prop_id.get(), link.rand_index as i8);
        }
    }
    overrides
}

fn apply_to_behaviour(behaviour: &mut Behaviour, overrides: &std::collections::HashMap<u32, i8>) {
    let patch = |kpd: &mut Option<KeyframePropData>| {
        if let Some(k) = kpd.as_mut() {
            if let Some(&idx) = overrides.get(&k.property_id) {
                k.rand_index = idx;
            }
        }
    };
    match behaviour {
        Behaviour::Acceleration(b) => {
            patch(&mut b.xyz_min_keyframes);
            patch(&mut b.xyz_max_keyframes);
        }
        Behaviour::Velocity(b) => {
            patch(&mut b.xyz_min_keyframes);
            patch(&mut b.xyz_max_keyframes);
        }
        Behaviour::Rotation(b) => {
            patch(&mut b.initial_angle_min_keyframes);
            patch(&mut b.initial_angle_max_keyframes);
            patch(&mut b.angle_min_keyframes);
            patch(&mut b.angle_max_keyframes);
        }
        Behaviour::RotationSpeed(b) => patch(&mut b.keyframes),
        Behaviour::Colour(b) => {
            patch(&mut b.rgba_min_keyframes);
            patch(&mut b.rgba_max_keyframes);
            patch(&mut b.emissive_intensity_keyframes);
        }
        Behaviour::Size(b) => {
            patch(&mut b.whd_min_keyframes);
            patch(&mut b.whd_max_keyframes);
            patch(&mut b.tblr_scalar_keyframes);
            patch(&mut b.tblr_vel_scalar_keyframes);
        }
        Behaviour::AnimateTexture(b) => patch(&mut b.anim_rate_keyframes),
        Behaviour::Noise(b) => {
            patch(&mut b.pos_noise_min_keyframes);
            patch(&mut b.pos_noise_max_keyframes);
            patch(&mut b.vel_noise_min_keyframes);
            patch(&mut b.vel_noise_max_keyframes);
        }
        Behaviour::Wind(b) => patch(&mut b.influence_keyframes),
        Behaviour::Dampening(b) => {
            patch(&mut b.xyz_min_keyframes);
            patch(&mut b.xyz_max_keyframes);
        }
        Behaviour::Collision(b) => {
            patch(&mut b.bounciness_keyframes);
            patch(&mut b.bounce_dir_var_keyframes);
        }
        Behaviour::Attractor(b) => patch(&mut b.strength_keyframes),
        Behaviour::Decal(b) => {
            patch(&mut b.dimensions_keyframes);
            patch(&mut b.alpha_keyframes);
        }
        Behaviour::FogVolume(b) => {
            patch(&mut b.rgb_tint_min_keyframes);
            patch(&mut b.rgb_tint_max_keyframes);
            patch(&mut b.density_range_keyframes);
            patch(&mut b.scale_min_keyframes);
            patch(&mut b.scale_max_keyframes);
            patch(&mut b.rotation_min_keyframes);
            patch(&mut b.rotation_max_keyframes);
        }
        Behaviour::Light(b) => {
            patch(&mut b.rgb_min_keyframes);
            patch(&mut b.rgb_max_keyframes);
            patch(&mut b.intensity_keyframes);
            patch(&mut b.range_keyframes);
            patch(&mut b.corona_rgb_min_keyframes);
            patch(&mut b.corona_rgb_max_keyframes);
            patch(&mut b.corona_intensity_keyframes);
            patch(&mut b.corona_size_keyframes);
            patch(&mut b.corona_flare_keyframes);
        }
        Behaviour::ZCulling(b) => {
            patch(&mut b.height_keyframes);
            patch(&mut b.fade_dist_keyframes);
        }
        Behaviour::MatrixWeight(b) => patch(&mut b.weight_keyframes),
        Behaviour::DrawTrail(b) => patch(&mut b.tex_info_keyframes),
        Behaviour::DrawSprite(_)
        | Behaviour::DrawModel(_)
        | Behaviour::FadeIn(_)
        | Behaviour::FadeOut(_)
        | Behaviour::Gravity(_)
        | Behaviour::Pause(_)
        | Behaviour::Age(_)
        | Behaviour::Liquid(_)
        | Behaviour::DecalPool(_)
        | Behaviour::River(_)
        | Behaviour::Unknown { .. } => {}
    }
}

pub fn eval_shader_var_keyframe(
    shader_vars: &[ShaderVar],
    name_hash: u32,
    time: f32,
) -> Option<Vec4> {
    for var in shader_vars {
        if let ShaderVar::Keyframe {
            name_hash: nh,
            entries,
            ..
        } = var
        {
            if *nh == name_hash && !entries.is_empty() {
                return Some(query_raw_entries(entries, time));
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyframeMode {
    #[default]
    Point,
    Effect,
    Emitter,
}

pub type KfpClock = KeyframeMode;
#[inline]
pub fn key_time_for_mode(
    mode: KeyframeMode,
    point_life: f32,
    emitter_life: f32,
    effect_life: f32,
) -> f32 {
    let t = match mode {
        KeyframeMode::Point => point_life,
        KeyframeMode::Effect => effect_life,
        KeyframeMode::Emitter => emitter_life,
    };
    t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(time: f32, inv_dt: f32, val: f32) -> PtxKeyframeEntry {
        let mut e = PtxKeyframeEntry::default();
        (e.time.x, e.time.y) = (time, inv_dt);
        (e.value.x, e.value.y, e.value.z, e.value.w) = (val, val, val, val);
        e
    }

    #[test]
    fn empty_returns_zero() {
        let entries: Vec<PtxKeyframeEntry> = Vec::new();
        assert_eq!(query_entries(&entries, 0.5), Vec4::ZERO);
    }

    #[test]
    fn single_entry_returned_verbatim() {
        let entries = vec![entry(0.0, 0.0, 4.0)];
        let v = query_entries(&entries, 0.5);
        assert_eq!(v, Vec4::splat(4.0));
    }

    #[test]
    fn linear_interp_between_entries() {
        let entries = vec![entry(0.0, 0.0, 0.0), entry(1.0, 1.0, 10.0)];
        assert!((query_entries(&entries, 0.0).x - 0.0).abs() < 1e-5);
        assert!((query_entries(&entries, 0.5).x - 5.0).abs() < 1e-5);
        let v_at_one = query_entries(&entries, 1.0).x;
        let v_past = query_entries(&entries, 1.5).x;
        assert!((v_at_one - 10.0).abs() < 1e-3);
        assert!((v_past - 10.0).abs() < 1e-5);
    }

    #[test]
    fn bias_link_overrides_kfp_rand_index() {
        use or_io::decl::Hash32;
        use or_particle::behaviour::ColourBehaviour;
        use or_particle::keyframe::KeyframePropData;
        use or_particle::rules::Colour;
        const PROP_MIN: u32 = 0xAABB_CCDD;
        const PROP_MAX: u32 = 0x1122_3344;
        let colour = ColourBehaviour {
            rgba_min: Colour {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            rgba_max: Colour {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            rgba_min_keyframes: Some(KeyframePropData {
                entries: Vec::new(),
                property_id: PROP_MIN,
                rand_index: 1,
                invert_bias_link: 0,
            }),
            rgba_max_keyframes: Some(KeyframePropData {
                entries: Vec::new(),
                property_id: PROP_MAX,
                rand_index: 2,
                invert_bias_link: 0,
            }),
            ..Default::default()
        };
        let mut behaviours = vec![Behaviour::Colour(colour)];
        let bias_links = vec![PtxBiasLink {
            keyframe_prop_ids: vec![Hash32::from_raw(PROP_MIN), Hash32::from_raw(PROP_MAX)],
            rand_index: 7,
            ..Default::default()
        }];
        apply_bias_links_to_behaviours(&bias_links, &mut behaviours);
        if let Behaviour::Colour(c) = &behaviours[0] {
            assert_eq!(c.rgba_min_keyframes.as_ref().unwrap().rand_index, 7);
            assert_eq!(c.rgba_max_keyframes.as_ref().unwrap().rand_index, 7);
        } else {
            panic!("behaviour vanished");
        }
    }

    #[test]
    fn key_time_modes() {
        assert!((key_time_for_mode(KeyframeMode::Point, 0.3, 0.5, 0.7) - 0.3).abs() < 1e-6);
        assert!((key_time_for_mode(KeyframeMode::Emitter, 0.3, 0.5, 0.7) - 0.5).abs() < 1e-6);
        assert!((key_time_for_mode(KeyframeMode::Effect, 0.3, 0.5, 0.7) - 0.7).abs() < 1e-6);
    }

    fn make_evolved(idx: i32, value: f32, is_lod: bool) -> or_particle::rules::EvolvedKeyframe {
        or_particle::rules::EvolvedKeyframe {
            evolution_index: idx,
            is_lod_evolution: is_lod,
            entries: vec![RawKeyframeEntry {
                time: [0.0; 4],
                value: [value; 4],
            }],
        }
    }

    #[test]
    fn evo_blend_average_matches_engine() {
        let prop = EvolvedKeyframeProp {
            property_id: 1,
            blend_mode: evo_blend::AVERAGE,
            evolved_keyframes: vec![make_evolved(0, 4.0, false), make_evolved(1, 6.0, false)],
        };
        let evo = vec![1.0, 1.0];
        let base = Vec4::new(2.0, 2.0, 2.0, 2.0);
        let r = apply_evolved_prop(base, &prop, &evo, 0.0);
        assert!((r.x - 5.0).abs() < 1e-5, "got {r:?}");
    }

    #[test]
    fn evo_blend_additive_matches_engine() {
        let prop = EvolvedKeyframeProp {
            property_id: 1,
            blend_mode: evo_blend::ADD,
            evolved_keyframes: vec![make_evolved(0, 5.0, false), make_evolved(1, 5.0, false)],
        };
        let evo = vec![0.5, 0.5];
        let base = Vec4::new(2.0, 2.0, 2.0, 2.0);
        let r = apply_evolved_prop(base, &prop, &evo, 0.0);
        assert!((r.x - 5.0).abs() < 1e-5, "got {r:?}");
    }

    #[test]
    fn evo_blend_max_matches_engine() {
        let prop = EvolvedKeyframeProp {
            property_id: 1,
            blend_mode: evo_blend::MAX,
            evolved_keyframes: vec![make_evolved(0, 10.0, false)],
        };
        let evo = vec![0.5];
        let base = Vec4::new(2.0, 2.0, 2.0, 2.0);
        let r = apply_evolved_prop(base, &prop, &evo, 0.0);
        assert!((r.x - 6.0).abs() < 1e-5, "got {r:?}");
    }

    #[test]
    fn evo_blend_full_average_walks_zero_weight_entries() {
        let prop = EvolvedKeyframeProp {
            property_id: 1,
            blend_mode: evo_blend::FULL_AVERAGE,
            evolved_keyframes: vec![make_evolved(0, 8.0, false), make_evolved(1, 8.0, false)],
        };
        let evo = vec![0.0, 1.0];
        let base = Vec4::new(4.0, 4.0, 4.0, 4.0);
        let r = apply_evolved_prop(base, &prop, &evo, 0.0);
        assert!((r.x - 6.0).abs() < 1e-5, "got {r:?}");
    }

    #[test]
    fn evo_blend_lod_post_blend_applies() {
        let prop = EvolvedKeyframeProp {
            property_id: 1,
            blend_mode: evo_blend::AVERAGE,
            evolved_keyframes: vec![make_evolved(0, 4.0, false), make_evolved(1, 10.0, true)],
        };
        let evo = vec![1.0, 0.5];
        let base = Vec4::new(2.0, 2.0, 2.0, 2.0);
        let r = apply_evolved_prop(base, &prop, &evo, 0.0);
        assert!((r.x - 7.0).abs() < 1e-5, "got {r:?}");
    }
}
