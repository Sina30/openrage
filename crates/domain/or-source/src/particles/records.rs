//! The records of a particle list and the members of each that a document states, by the names the declaration gives them.
//!
//! A record's text is an object of its members, and the members a document does not state are the ones the declaration holds only
//! as runtime words, or that follow from the others: the lists naming a rule's behaviours (they follow from the classes), the map
//! of an evolution list (it follows from the values it names), and the pointers an event or a spawner holds at a rule it names
//! (they follow from the name, once the list is laid out with [`or_particle::list::canonical`]).
//!
//! `mismatches` holds every record to the declaration: a member the declaration shows and neither list names is reported, and so is a
//! name the declaration lacks.

#[cfg(test)]
use or_io::decl::{Described, Gen8, Owned, Raw};
use or_io::decl::{OwnedOrKeyed, Ref};
use or_io::gta5::drawable::{DrawableTarget, RmcDrawable};
use or_io::gta5::particle::{
    AtStringMap, PtxBehaviour, PtxBehaviourAny, PtxBiasLink, PtxDomain, PtxDomainObj, PtxDrawable,
    PtxEffectRule, PtxEffectSpawner, PtxEmitterRule, PtxEvent, PtxEventEmitter, PtxEvolution,
    PtxEvolutionList, PtxEvolvedKeyframe, PtxEvolvedKeyframeProp, PtxKeyframe, PtxKeyframeEntry,
    PtxKeyframeProp, PtxParticleRule, PtxRenderState, PtxShaderInst, PtxShaderVar, PtxShaderVarAny,
    PtxShaderVarKeyframe, PtxShaderVarTexture, PtxShaderVarVector, PtxSpawnedEffectScalars,
    PtxTechniqueDesc, PtxTimeLine, PtxdModel, PtxdSprite, PtxdTrail, PtxuAcceleration, PtxuAge,
    PtxuAnimateTexture, PtxuAttractor, PtxuCameraEvolution, PtxuCollision, PtxuColour,
    PtxuDampening, PtxuDecal, PtxuDecalPool, PtxuFogVolume, PtxuLight, PtxuLiquid,
    PtxuMatrixWeight, PtxuNoise, PtxuRiver, PtxuRotation, PtxuSize, PtxuVelocity, PtxuWind,
    PtxuZCull,
};
use serde_json::{Map, Value};

use super::text::{hash_from, hash_text, Fault, Json, Trail};

/// The name a member is written under: the declaration's, without the `r#` that spells a keyword.
fn shown(name: &str) -> &str {
    name.trim_start_matches("r#")
}

/// Reports the members `probe`'s declaration shows that `stated` and `left_out` do not name, and the names that are not its members.
#[cfg(test)]
fn check<R: Owned>(bad: &mut Vec<String>, ty: &str, stated: &[&str], left_out: &[&str])
where
    Raw<R>: Described,
{
    let Some(declared) = <Raw<R> as Described>::__OWNED_PUBLIC else {
        bad.push(format!("{ty}: the declaration lists no members it shows"));
        return;
    };
    let named: Vec<&str> = stated.iter().chain(left_out).map(|n| shown(n)).collect();
    for (member, _) in declared {
        if !named.contains(member) {
            bad.push(format!(
                "{ty}: `{member}` is shown by the declaration and named by neither list"
            ));
        }
    }
    for name in &named {
        if !declared.iter().any(|(member, _)| member == name) {
            bad.push(format!("{ty}: `{name}` is no member the declaration shows"));
        }
    }
}

macro_rules! records {
    ($(
        [$($g:tt)*] $ty:ty => $probe:ty {
            $($f:ident),* $(,)?
        } left_out { $($l:ident),* $(,)? }
    )+) => {
        $(
            impl<$($g)*> Json for $ty {
                fn blank() -> Self {
                    Self::default()
                }

                fn write(&self) -> Value {
                    // A member a document does not state is the declaration's own default, so one equal to it is left out.
                    let blank = Self::default();
                    let mut o = Map::new();
                    $(
                        let stated = self.$f.write();
                        if stated != blank.$f.write() {
                            o.insert(shown(stringify!($f)).to_string(), stated);
                        }
                    )*
                    Value::Object(o)
                }

                fn read(&mut self, v: &Value, at: &mut Trail) -> Result<(), Fault> {
                    let o = at.object(v)?;
                    at.only(o, &[$(shown(stringify!($f))),*])?;
                    $(
                        if let Some(x) = o.get(shown(stringify!($f))) {
                            at.field(stringify!($f), |at| self.$f.read(x, at))?;
                        }
                    )*
                    Ok(())
                }
            }
        )+

        /// Every record held to the declaration: what the declaration shows and no list names, and what a list names it lacks.
        #[cfg(test)]
        pub fn mismatches() -> Vec<String> {
            let mut bad = Vec::new();
            $( check::<$probe>(&mut bad, stringify!($ty), &[$(stringify!($f)),*], &[$(stringify!($l)),*]); )+
            bad
        }
    };
}

records! {
    [] PtxKeyframeEntry => PtxKeyframeEntry { time, value } left_out {}
    [] PtxKeyframe => PtxKeyframe { entries } left_out {}
    [] PtxKeyframeProp => PtxKeyframeProp { property_id, invert_bias_link, rand_index, keyframe } left_out {}
    [] PtxEvolvedKeyframe => PtxEvolvedKeyframe { keyframe, evolution_idx, is_lod_evolution } left_out {}
    [] PtxEvolvedKeyframeProp => PtxEvolvedKeyframeProp { evolved_keyframes, property_id, blend_mode } left_out {}
    [] PtxEvolution => PtxEvolution { name, name_hash, override_value, is_overriden } left_out {}
    [] AtStringMap => AtStringMap { sorted } left_out { data }
    [] PtxEvolutionList => PtxEvolutionList { evolutions, evolved_keyframe_props, evolved_keyframe_prop_map } left_out {}
    [] PtxSpawnedEffectScalars => PtxSpawnedEffectScalars {
        duration_scalar, playback_rate_scalar, colour_tint_scalar, zoom_scalar, flags
    } left_out {}
    [T: DrawableTarget] PtxEffectSpawner<T> => PtxEffectSpawner<Gen8> {
        spawned_effect_scalars_min, spawned_effect_scalars_max, name, trigger_info, inherits_point_life,
        tracks_point_pos, tracks_point_dir, tracks_point_neg_dir
    } left_out { base, effect_rule }
    [] PtxRenderState => PtxRenderState { cull_mode, blend_set, lighting_mode, depth_write, depth_test, alpha_blend } left_out {}
    [] PtxBiasLink => PtxBiasLink { name, keyframe_prop_ids, rand_index } left_out {}
    [] PtxTechniqueDesc => PtxTechniqueDesc {
        diffuse_mode, proj_mode, is_lit, is_soft, is_screen_space, is_refract, is_normal_spec
    } left_out {}
    [T: DrawableTarget] PtxShaderVar<T> => PtxShaderVar<Gen8> { hash_name, r#type, id, is_keyframeable, owns_info } left_out { base }
    [T: DrawableTarget] PtxShaderVarVector<T> => PtxShaderVarVector<Gen8> { base, vector } left_out {}
    [T: DrawableTarget] PtxShaderVarTexture<T> => PtxShaderVarTexture<Gen8> {
        base, texture, texture_name, texture_hash_name, external_reference
    } left_out {}
    [T: DrawableTarget] PtxShaderVarKeyframe<T> => PtxShaderVarKeyframe<Gen8> { base, keyframe } left_out {}
    [T: DrawableTarget] PtxShaderInst<T> => PtxShaderInst<Gen8> {
        shader_template_name, shader_template_technique_name, shader_template_technique_id, technique_desc,
        inst_vars, is_data_in_sync, shader_template_hash_name
    } left_out {}
    [T: DrawableTarget] PtxDrawable<T> => PtxDrawable<Gen8> { bound_info, name, drawable, hash_name } left_out {}
    [T: DrawableTarget] PtxParticleRule<T> => PtxParticleRule<Gen8> {
        effect_spawner_at_ratio, effect_spawner_on_coln, render_state, file_version, tex_frame_id_min,
        tex_frame_id_max, name, all_behaviours, bias_links, shader_inst, drawables, sort_type, draw_type, flags
    } left_out {
        base, init_behaviours, update_behaviours, update_finalize_behaviours, draw_behaviours, release_behaviours
    }
    [] PtxDomain => PtxDomain {
        r#type, shape, is_world_space, is_point_relative, is_creation_relative, is_target_relative, position_kfp,
        rotation_kfp, size_outer_kfp, size_inner_kfp, file_version, keyframe_prop_list
    } left_out { base }
    [] PtxDomainObj => PtxDomainObj { domain, new_domain_shape } left_out {}
    [] PtxEmitterRule => PtxEmitterRule {
        file_version, name, creation_domain_obj, target_domain_obj, attractor_domain_obj, spawn_rate_over_time_kfp,
        spawn_rate_over_dist_kfp, particle_life_kfp, playback_rate_scalar_kfp, speed_scalar_kfp, size_scalar_kfp,
        accn_scalar_kfp, dampening_scalar_kfp, matrix_weight_scalar_kfp, inherit_velocity_kfp, keyframe_prop_list,
        is_one_shot
    } left_out { base }
    [] PtxEvent => PtxEvent { index, r#type, start_ratio, end_ratio, evolution_list } left_out {}
    [T: DrawableTarget] PtxEventEmitter<T> => PtxEventEmitter<Gen8> {
        base, emitter_rule_name, particle_rule_name, playback_rate_scalar_min, playback_rate_scalar_max,
        zoom_scalar_min, zoom_scalar_max, colour_tint_min, colour_tint_max
    } left_out { emitter_rule, particle_rule }
    [T: DrawableTarget] PtxTimeLine<T> => PtxTimeLine<Gen8> { events } left_out {}
    [T: DrawableTarget] PtxEffectRule<T> => PtxEffectRule<Gen8> {
        file_version, name, timeline, evolution_list, num_loops, sort_events_by_distance, draw_list_id,
        is_short_lived, has_no_shadows, rand_offset_pos, pre_update_time, pre_update_time_interval, duration_min,
        duration_max, playback_rate_scalar_min, playback_rate_scalar_max, viewport_culling_mode,
        render_when_viewport_culled, update_when_viewport_culled, emit_when_viewport_culled, distance_culling_mode,
        render_when_distance_culled, update_when_distance_culled, emit_when_distance_culled,
        viewport_culling_sphere_offset, viewport_culling_sphere_radius, distance_culling_fade_dist,
        distance_culling_cull_dist, lod_evo_dist_min, lod_evo_dist_max, coln_range, coln_probe_dist, coln_type,
        share_entity_coln, only_use_bvh_coln, game_flags, colour_tint_min_kfp, colour_tint_max_kfp, zoom_scalar_kfp,
        data_sphere_kfp, data_capsule_kfp, keyframe_prop_list, colour_tint_max_enable, use_data_volume,
        data_volume_type, num_active_instances, zoom_level
    } left_out { base }
}

/// A pointer at a model of the list's dictionary, by the hash it is keyed by.
impl<T: DrawableTarget> Json for OwnedOrKeyed<RmcDrawable<T>> {
    fn blank() -> Self {
        OwnedOrKeyed::Own(None)
    }

    fn write(&self) -> Value {
        match self {
            OwnedOrKeyed::Key(key) => hash_text(*key),
            OwnedOrKeyed::Own(None) => Value::Null,
            // A model a rule holds that the dictionary does not is no document: the read of it says so.
            OwnedOrKeyed::Own(Some(_)) => Value::String("owned".into()),
        }
    }

    fn read(&mut self, v: &Value, at: &mut Trail) -> Result<(), Fault> {
        *self = match v {
            Value::Null => OwnedOrKeyed::Own(None),
            other => OwnedOrKeyed::Key(
                hash_from(other)
                    .ok_or_else(|| at.fault("is not the hash of a model of the list, or null"))?,
            ),
        };
        Ok(())
    }
}

/// One behaviour class: the variant of [`PtxBehaviourAny`], the class's name, and the members it states besides its base's list.
macro_rules! behaviours {
    ($( $variant:ident $class:literal { $($f:ident),* $(,)? } )+) => {
        /// A behaviour as a document states it.
        pub fn behaviour_write(b: &PtxBehaviourAny) -> Value {
            match b {
                $(
                    PtxBehaviourAny::$variant(x) => {
                        let blank = <$variant>::default();
                        let mut o = Map::new();
                        o.insert("class".into(), Value::String($class.into()));
                        if x.base.keyframe_prop_list != blank.base.keyframe_prop_list {
                            o.insert("keyframe_prop_list".into(), x.base.keyframe_prop_list.write());
                        }
                        $(
                            let stated = x.$f.write();
                            if stated != blank.$f.write() {
                                o.insert(shown(stringify!($f)).to_string(), stated);
                            }
                        )*
                        Value::Object(o)
                    }
                )+
                // A class no layout is declared for is written by its hash, which the read of it refuses by name.
                PtxBehaviourAny::Base(b) => {
                    let mut o = Map::new();
                    o.insert("class".into(), hash_text(b.hash_name));
                    Value::Object(o)
                }
            }
        }

        /// The behaviour a document states.
        pub fn behaviour_read(v: &Value, at: &mut Trail) -> Result<PtxBehaviourAny, Fault> {
            let o = at.object(v)?;
            let class = o
                .get("class")
                .and_then(Value::as_str)
                .ok_or_else(|| at.fault("states no class"))?;
            match class {
                $(
                    $class => {
                        at.only(o, &["class", "keyframe_prop_list", $(shown(stringify!($f))),*])?;
                        let mut x = <$variant>::default();
                        x.base.hash_name = or_io::string_hash_str($class);
                        if let Some(list) = o.get("keyframe_prop_list") {
                            at.field("keyframe_prop_list", |at| x.base.keyframe_prop_list.read(list, at))?;
                        }
                        $(
                            if let Some(m) = o.get(shown(stringify!($f))) {
                                at.field(stringify!($f), |at| x.$f.read(m, at))?;
                            }
                        )*
                        Ok(PtxBehaviourAny::$variant(x))
                    }
                )+
                other => Err(at.fault(format!("is of class `{other}`, which this build has no layout for"))),
            }
        }

        /// Every class held to the declaration, as [`mismatches`] holds the records.
        #[cfg(test)]
        pub fn behaviour_mismatches() -> Vec<String> {
            let mut bad = Vec::new();
            $( check::<$variant>(&mut bad, stringify!($variant), &[$(stringify!($f)),*], &["base"]); )+
            bad
        }

        /// The variant name and the class name of each, in order.
        #[cfg(test)]
        pub const VARIANTS: &[(&str, &str)] = &[$((stringify!($variant), $class)),*];
    };
}

behaviours! {
    PtxuAge "ptxu_Age" {}
    PtxuAcceleration "ptxu_Acceleration" {
        xyz_min_kfp, xyz_max_kfp, reference_space, is_affected_by_zoom, enable_gravity
    }
    PtxuVelocity "ptxu_Velocity" {}
    PtxuRotation "ptxu_Rotation" {
        initial_angle_min_kfp, initial_angle_max_kfp, angle_min_kfp, angle_max_kfp, init_rotation_mode,
        update_rotation_mode, accumulate_angle, rotate_angle_axes, rotate_init_angle_axes, speed_fade_thresh
    }
    PtxuSize "ptxu_Size" {
        whd_min_kfp, whd_max_kfp, tblr_scalar_kfp, tblr_vel_scalar_kfp, keyframe_mode, is_proportional
    }
    PtxuDampening "ptxu_Dampening" {
        xyz_min_kfp, xyz_max_kfp, reference_space, enable_air_resistance
    }
    PtxuMatrixWeight "ptxu_MatrixWeight" { mtx_weight_kfp, reference_space }
    PtxuCollision "ptxu_Collision" {
        bounciness_kfp, bounce_dir_var_kfp, radius_mult, rest_speed, coln_chance, kill_chance, debug_draw,
        override_min_radius
    }
    PtxuAnimateTexture "ptxu_AnimateTexture" {
        anim_rate_kfp, keyframe_mode, last_frame_id, loop_mode, is_randomised, is_scaled_over_particle_life,
        is_held_on_last_frame, do_frame_blending
    }
    PtxuColour "ptxu_Colour" {
        rgba_min_kfp, rgba_max_kfp, emissive_intensity_kfp, keyframe_mode, rgba_max_enable, rgba_proportional,
        rgb_can_tint
    }
    PtxdSprite "ptxd_Sprite" {
        align_axis, alignment_mode, flip_chance_u, flip_chance_v, near_clip_dist, far_clip_dist, projection_depth,
        shadow_cast_intensity, is_screen_space, is_hi_res, near_clip, far_clip, uv_clip, disable_draw
    }
    PtxuWind "ptxu_Wind" {
        influence_kfp, high_lod_range, low_lod_range, high_lod_disturbance_mode, low_lod_disturbance_mode,
        ignore_mtx_weight
    }
    PtxuLight "ptxu_Light" {
        rgb_min_kfp, rgb_max_kfp, intensity_kfp, range_kfp, corona_rgb_min_kfp, corona_rgb_max_kfp,
        corona_intensity_kfp, corona_size_kfp, corona_flare_kfp, corona_z_bias, corona_use_light_col,
        colour_from_particle, colour_per_frame, intensity_per_frame, range_per_frame, casts_shadows,
        corona_not_in_reflection, corona_only_in_reflection, light_type
    }
    PtxdModel "ptxd_Model" { colour_control_shader_id, camera_shrink, shadow_cast_intensity, disable_draw }
    PtxuDecal "ptxu_Decal" {
        dimensions_kfp, alpha_kfp, decal_id, velocity_thresh, total_life, fade_in_time, uv_mult_start, uv_mult_end,
        uv_mult_time, duplicate_reject_dist, flip_u, flip_v, proportional_size, use_complex_coln, projection_depth,
        distance_scale, is_directional
    }
    PtxuZCull "ptxu_ZCull" { height_kfp, fade_dist_kfp, mode, reference_space }
    PtxuNoise "ptxu_Noise" {
        pos_noise_min_kfp, pos_noise_max_kfp, vel_noise_min_kfp, vel_noise_max_kfp, reference_space,
        keep_constant_speed
    }
    PtxuAttractor "ptxu_Attractor" { strength_kfp }
    PtxdTrail "ptxd_Trail" {
        tex_info_kfp, align_axis, alignment_mode, tessellation_u, tessellation_v, smoothness_x, smoothness_y,
        projection_depth, shadow_cast_intensity, flip_u, flip_v, wrap_texture_over_particle_life, disable_draw
    }
    PtxuFogVolume "ptxu_FogVolume" {
        rgb_tint_min_kfp, rgb_tint_max_kfp, density_range_kfp, scale_min_kfp, scale_max_kfp, rotation_min_kfp,
        rotation_max_kfp, falloff, hdr_mult, lighting_type, colour_tint_from_particle, sort_with_particles,
        use_ground_fog_colour, use_effect_evo_values
    }
    PtxuRiver "ptxu_River" { velocity_mult, influence }
    PtxuDecalPool "ptxu_DecalPool" {
        velocity_thresh, liquid_type, decal_id, start_size, end_size, growth_rate
    }
    PtxuLiquid "ptxu_Liquid" {
        velocity_thresh, liquid_type, pool_start_size, pool_end_size, pool_growth_rate, trail_width_min,
        trail_width_max
    }
    PtxuCameraEvolution "ptxu_CameraEvolution" { evolution_curve, settings }
}

impl Json for PtxBehaviourAny {
    fn blank() -> Self {
        PtxBehaviourAny::Base(PtxBehaviour::default())
    }

    fn write(&self) -> Value {
        behaviour_write(self)
    }

    fn read(&mut self, v: &Value, at: &mut Trail) -> Result<(), Fault> {
        *self = behaviour_read(v, at)?;
        Ok(())
    }
}

/// A behaviour a rule owns; one it names by its place is none a document states.
impl Json for Ref<Box<PtxBehaviourAny>> {
    fn blank() -> Self {
        Ref::Own(Box::new(PtxBehaviourAny::blank()))
    }

    fn write(&self) -> Value {
        match self {
            Ref::Own(b) => b.write(),
            Ref::To(_) => Value::String("reference".into()),
        }
    }

    fn read(&mut self, v: &Value, at: &mut Trail) -> Result<(), Fault> {
        *self = Ref::Own(Box::new(behaviour_read(v, at)?));
        Ok(())
    }
}

/// A shader variable as a document states it: its base, and the members of the layout its type selects.
impl<T: DrawableTarget> Json for PtxShaderVarAny<T> {
    fn blank() -> Self {
        PtxShaderVarAny::Base(Default::default())
    }

    fn write(&self) -> Value {
        match self {
            PtxShaderVarAny::PtxShaderVarVector(x) => x.write(),
            PtxShaderVarAny::PtxShaderVarTexture(x) => x.write(),
            PtxShaderVarAny::PtxShaderVarKeyframe(x) => x.write(),
            // A type no layout is declared for is written by its number, which the read of it refuses by name.
            PtxShaderVarAny::Base(b) => {
                let mut base = Map::new();
                base.insert("type".into(), Value::from(b.r#type));
                let mut o = Map::new();
                o.insert("base".into(), Value::Object(base));
                Value::Object(o)
            }
        }
    }

    /// The type in the base picks the layout.
    fn read(&mut self, v: &Value, at: &mut Trail) -> Result<(), Fault> {
        let o = at.object(v)?;
        // A type that is the default, nought, is not stated: it is a vector.
        let kind = match o
            .get("base")
            .and_then(Value::as_object)
            .and_then(|b| b.get("type"))
        {
            None => 0,
            Some(kind) => kind
                .as_u64()
                .ok_or_else(|| at.fault("states a type that is not a whole number"))?,
        };
        *self = match kind {
            0..=5 => {
                let mut x = PtxShaderVarVector::<T>::default();
                x.read(v, at)?;
                PtxShaderVarAny::PtxShaderVarVector(x)
            }
            6 => {
                let mut x = PtxShaderVarTexture::<T>::default();
                x.read(v, at)?;
                PtxShaderVarAny::PtxShaderVarTexture(x)
            }
            7 => {
                let mut x = PtxShaderVarKeyframe::<T>::default();
                x.read(v, at)?;
                PtxShaderVarAny::PtxShaderVarKeyframe(x)
            }
            other => {
                return Err(at.fault(format!(
                    "is a shader variable of type {other}, which this build has no layout for"
                )))
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_member_the_declaration_shows_is_stated_or_left_out_by_name_and_no_name_is_made_up() {
        assert_eq!(mismatches(), Vec::<String>::new());
        assert_eq!(behaviour_mismatches(), Vec::<String>::new());
    }

    #[test]
    fn every_class_is_stated_under_the_name_whose_hash_is_the_tag_its_variant_is_declared_under() {
        let base = <Raw<PtxBehaviour> as Described>::describe();
        let selector = base.selector.expect("PtxBehaviour is tagged");
        for (variant, class) in VARIANTS {
            let tag = selector
                .variants
                .iter()
                .find(|(_, name)| name == variant)
                .map(|(tag, _)| *tag)
                .unwrap_or_else(|| panic!("{variant} is a variant of the behaviour"));
            assert_eq!(
                tag,
                i64::from(or_io::string_hash_str(class)),
                "{variant} is stated as `{class}`"
            );
        }
        assert_eq!(
            VARIANTS.len(),
            selector.variants.len(),
            "a class the declaration has and this does not"
        );
    }
}
