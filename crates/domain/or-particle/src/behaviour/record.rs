//! A behaviour as the editor holds it, read from the record a particle list
//! holds and written back into it.

use or_io::decl::{Bool8, Hash32, Member};
use or_io::gta5::particle::{self as ptx, AlignmentMode, KeyframeMode, KfpMeta, PtxBehaviourAny};
use or_io::gta5::{Vec3V, Vec4V};

use super::*;
use crate::domain::Vec3;
use crate::evolution::Colour;
use crate::keyframe::{set_entries, KeyframePropData};

impl Behaviour {
    /// The behaviour a record states; a class the list declares no layout for reads as [`Behaviour::Unknown`].
    pub fn of(b: &PtxBehaviourAny) -> Self {
        use PtxBehaviourAny as B;
        match b {
            B::PtxuAge(_) => Behaviour::Age(AgeBehaviour::default()),
            B::PtxuVelocity(_) => Behaviour::Velocity(VelocityBehaviour::default()),
            B::PtxdModel(b) => Behaviour::DrawModel(DrawModelBehaviour {
                colour_control_shader_id: b.colour_control_shader_id,
                camera_shrink: b.camera_shrink,
                shadow_cast_intensity: b.shadow_cast_intensity,
                disable_draw: b.disable_draw.get(),
            }),
            B::PtxuRiver(b) => Behaviour::River(RiverBehaviour {
                velocity_mult: b.velocity_mult,
                influence: b.influence,
            }),
            B::PtxuLiquid(b) => Behaviour::Liquid(LiquidBehaviour {
                velocity_thresh: b.velocity_thresh,
                liquid_type: b.liquid_type as u32,
                pool_start_size: b.pool_start_size,
                pool_end_size: b.pool_end_size,
                pool_growth_rate: b.pool_growth_rate,
                trail_width_min: b.trail_width_min,
                trail_width_max: b.trail_width_max,
            }),
            B::PtxuDecalPool(b) => Behaviour::DecalPool(DecalPoolBehaviour {
                velocity_thresh: b.velocity_thresh,
                liquid_type: b.liquid_type as u32,
                decal_id: b.decal_id as u32,
                start_size: b.start_size,
                end_size: b.end_size,
                growth_rate: b.growth_rate,
            }),
            B::PtxuAcceleration(b) => Behaviour::Acceleration(AccelerationBehaviour {
                xyz_min_keyframes: data(&b.xyz_min_kfp),
                xyz_max_keyframes: data(&b.xyz_max_kfp),
                xyz_min: vec3(&b.xyz_min_kfp),
                xyz_max: vec3(&b.xyz_max_kfp),
                reference_space: (b.reference_space as u32).into(),
                is_affected_by_zoom: b.is_affected_by_zoom.get(),
                enable_gravity: b.enable_gravity.get(),
                kfp_meta: meta(&[&b.xyz_min_kfp, &b.xyz_max_kfp]),
            }),
            B::PtxuRotation(b) => Behaviour::Rotation(RotationBehaviour {
                initial_angle_min_keyframes: data(&b.initial_angle_min_kfp),
                initial_angle_max_keyframes: data(&b.initial_angle_max_kfp),
                angle_min_keyframes: data(&b.angle_min_kfp),
                angle_max_keyframes: data(&b.angle_max_kfp),
                init_rotation_mode: b.init_rotation_mode as u32,
                update_rotation_mode: b.update_rotation_mode as u32,
                accumulate_angle: b.accumulate_angle.get(),
                rotate_angle_axes: b.rotate_angle_axes.get(),
                rotate_init_angle_axes: b.rotate_init_angle_axes.get(),
                speed_fade_thresh: b.speed_fade_thresh,
                kfp_meta: meta(&[
                    &b.initial_angle_min_kfp,
                    &b.initial_angle_max_kfp,
                    &b.angle_min_kfp,
                    &b.angle_max_kfp,
                ]),
            }),
            B::PtxuSize(b) => Behaviour::Size(SizeBehaviour {
                whd_min_keyframes: data(&b.whd_min_kfp),
                whd_max_keyframes: data(&b.whd_max_kfp),
                tblr_scalar_keyframes: data(&b.tblr_scalar_kfp),
                tblr_vel_scalar_keyframes: data(&b.tblr_vel_scalar_kfp),
                keyframe_mode: KeyframeMode::from(b.keyframe_mode as u32),
                is_proportional: b.is_proportional.get(),
                kfp_meta: meta(&[
                    &b.whd_min_kfp,
                    &b.whd_max_kfp,
                    &b.tblr_scalar_kfp,
                    &b.tblr_vel_scalar_kfp,
                ]),
            }),
            B::PtxuDampening(b) => Behaviour::Dampening(DampeningBehaviour {
                xyz_min_keyframes: data(&b.xyz_min_kfp),
                xyz_max_keyframes: data(&b.xyz_max_kfp),
                xyz_min: vec3(&b.xyz_min_kfp),
                xyz_max: vec3(&b.xyz_max_kfp),
                reference_space: (b.reference_space as u32).into(),
                enable_air_resistance: b.enable_air_resistance.get(),
                kfp_meta: meta(&[&b.xyz_min_kfp, &b.xyz_max_kfp]),
            }),
            B::PtxuMatrixWeight(b) => Behaviour::MatrixWeight(MatrixWeightBehaviour {
                weight_keyframes: data(&b.mtx_weight_kfp),
                reference_space: (b.reference_space as u32).into(),
                kfp_meta: meta(&[&b.mtx_weight_kfp]),
            }),
            B::PtxuCollision(b) => Behaviour::Collision(CollisionBehaviour {
                bounciness_keyframes: data(&b.bounciness_kfp),
                bounce_dir_var_keyframes: data(&b.bounce_dir_var_kfp),
                radius_mult: b.radius_mult,
                rest_speed: b.rest_speed,
                collision_chance: b.coln_chance as u32,
                kill_chance: b.kill_chance as u32,
                debug_draw: b.debug_draw.get(),
                override_min_radius: b.override_min_radius,
                kfp_meta: meta(&[&b.bounciness_kfp, &b.bounce_dir_var_kfp]),
            }),
            B::PtxuAnimateTexture(b) => {
                let rate = scalar(&b.anim_rate_kfp);
                Behaviour::AnimateTexture(AnimateTextureBehaviour {
                    anim_rate_keyframes: data(&b.anim_rate_kfp),
                    anim_rate_min: rate,
                    anim_rate_max: rate,
                    keyframe_mode: KeyframeMode::from(b.keyframe_mode as u32),
                    last_frame_id: b.last_frame_id as u32,
                    loop_mode: (b.loop_mode as u32).into(),
                    is_randomised: b.is_randomised.get(),
                    is_scaled_over_particle_life: b.is_scaled_over_particle_life.get(),
                    is_held_on_last_frame: b.is_held_on_last_frame.get(),
                    do_frame_blending: b.do_frame_blending.get(),
                    uv_scroll_rate_u_keyframes: None,
                    uv_scroll_rate_v_keyframes: None,
                    kfp_meta: meta(&[&b.anim_rate_kfp]),
                })
            }
            B::PtxuColour(b) => {
                let (r0, g0, b0, a0) = vec4(&b.rgba_min_kfp);
                let (r1, g1, b1, a1) = vec4(&b.rgba_max_kfp);
                let emissive = scalar(&b.emissive_intensity_kfp);
                Behaviour::Colour(ColourBehaviour {
                    rgba_min_keyframes: data(&b.rgba_min_kfp),
                    rgba_max_keyframes: data(&b.rgba_max_kfp),
                    emissive_intensity_keyframes: data(&b.emissive_intensity_kfp),
                    rgba_min: Colour::new(r0, g0, b0, a0),
                    rgba_max: Colour::new(r1, g1, b1, a1),
                    emissive_intensity_min: emissive,
                    emissive_intensity_max: emissive,
                    keyframe_mode: KeyframeMode::from(b.keyframe_mode as u32),
                    rgba_max_enable: b.rgba_max_enable.get(),
                    rgba_proportional: b.rgba_proportional.get(),
                    rgb_can_tint: b.rgb_can_tint.get(),
                    kfp_meta: meta(&[&b.rgba_min_kfp, &b.rgba_max_kfp, &b.emissive_intensity_kfp]),
                })
            }
            B::PtxuWind(b) => Behaviour::Wind(WindBehaviour {
                influence_keyframes: data(&b.influence_kfp),
                high_lod_range: b.high_lod_range,
                low_lod_range: b.low_lod_range,
                high_lod_disturbance_mode: b.high_lod_disturbance_mode as u32,
                low_lod_disturbance_mode: b.low_lod_disturbance_mode as u32,
                ignore_mtx_weight: b.ignore_mtx_weight.get(),
                kfp_meta: meta(&[&b.influence_kfp]),
            }),
            B::PtxuLight(b) => Behaviour::Light(LightBehaviour {
                rgb_min_keyframes: data(&b.rgb_min_kfp),
                rgb_max_keyframes: data(&b.rgb_max_kfp),
                intensity_keyframes: data(&b.intensity_kfp),
                range_keyframes: data(&b.range_kfp),
                corona_rgb_min_keyframes: data(&b.corona_rgb_min_kfp),
                corona_rgb_max_keyframes: data(&b.corona_rgb_max_kfp),
                corona_intensity_keyframes: data(&b.corona_intensity_kfp),
                corona_size_keyframes: data(&b.corona_size_kfp),
                corona_flare_keyframes: data(&b.corona_flare_kfp),
                corona_z_bias: b.corona_z_bias,
                corona_use_light_col: b.corona_use_light_col.get(),
                colour_from_particle: b.colour_from_particle.get(),
                colour_per_frame: b.colour_per_frame.get(),
                intensity_per_frame: b.intensity_per_frame.get(),
                range_per_frame: b.range_per_frame.get(),
                casts_shadows: b.casts_shadows.get(),
                corona_not_in_reflection: b.corona_not_in_reflection.get(),
                corona_only_in_reflection: b.corona_only_in_reflection.get(),
                light_type: b.light_type as u32,
                kfp_meta: meta(&[
                    &b.rgb_min_kfp,
                    &b.rgb_max_kfp,
                    &b.intensity_kfp,
                    &b.range_kfp,
                    &b.corona_rgb_min_kfp,
                    &b.corona_rgb_max_kfp,
                    &b.corona_intensity_kfp,
                    &b.corona_size_kfp,
                    &b.corona_flare_kfp,
                ]),
            }),
            B::PtxuDecal(b) => Behaviour::Decal(DecalBehaviour {
                dimensions_keyframes: data(&b.dimensions_kfp),
                alpha_keyframes: data(&b.alpha_kfp),
                decal_id: b.decal_id as u32,
                velocity_thresh: b.velocity_thresh,
                total_life: b.total_life,
                fade_in_time: b.fade_in_time,
                uv_mult_start: b.uv_mult_start,
                uv_mult_end: b.uv_mult_end,
                uv_mult_time: b.uv_mult_time,
                duplicate_reject_dist: b.duplicate_reject_dist,
                flip_u: b.flip_u.get(),
                flip_v: b.flip_v.get(),
                proportional_size: b.proportional_size.get(),
                use_complex_coln: b.use_complex_coln.get(),
                projection_depth: b.projection_depth,
                distance_scale: b.distance_scale,
                is_directional: b.is_directional.get(),
                kfp_meta: meta(&[&b.dimensions_kfp, &b.alpha_kfp]),
            }),
            B::PtxuZCull(b) => Behaviour::ZCulling(ZCullingBehaviour {
                height_keyframes: data(&b.height_kfp),
                fade_dist_keyframes: data(&b.fade_dist_kfp),
                mode: b.mode as u32,
                reference_space: (b.reference_space as u32).into(),
                kfp_meta: meta(&[&b.height_kfp, &b.fade_dist_kfp]),
            }),
            B::PtxuNoise(b) => Behaviour::Noise(NoiseBehaviour {
                pos_noise_min_keyframes: data(&b.pos_noise_min_kfp),
                pos_noise_max_keyframes: data(&b.pos_noise_max_kfp),
                vel_noise_min_keyframes: data(&b.vel_noise_min_kfp),
                vel_noise_max_keyframes: data(&b.vel_noise_max_kfp),
                reference_space: (b.reference_space as u32).into(),
                keep_constant_speed: b.keep_constant_speed.get(),
                kfp_meta: meta(&[
                    &b.pos_noise_min_kfp,
                    &b.pos_noise_max_kfp,
                    &b.vel_noise_min_kfp,
                    &b.vel_noise_max_kfp,
                ]),
            }),
            B::PtxuAttractor(b) => {
                let strength = scalar(&b.strength_kfp);
                Behaviour::Attractor(AttractorBehaviour {
                    strength_keyframes: data(&b.strength_kfp),
                    strength_min: strength,
                    strength_max: strength,
                    kfp_meta: meta(&[&b.strength_kfp]),
                })
            }
            B::PtxuFogVolume(b) => Behaviour::FogVolume(FogVolumeBehaviour {
                rgb_tint_min_keyframes: data(&b.rgb_tint_min_kfp),
                rgb_tint_max_keyframes: data(&b.rgb_tint_max_kfp),
                density_range_keyframes: data(&b.density_range_kfp),
                scale_min_keyframes: data(&b.scale_min_kfp),
                scale_max_keyframes: data(&b.scale_max_kfp),
                rotation_min_keyframes: data(&b.rotation_min_kfp),
                rotation_max_keyframes: data(&b.rotation_max_kfp),
                falloff: b.falloff,
                hdr_mult: b.hdr_mult,
                lighting_type: b.lighting_type as u32,
                colour_tint_from_particle: b.colour_tint_from_particle.get(),
                sort_with_particles: b.sort_with_particles.get(),
                use_ground_fog_colour: b.use_ground_fog_colour.get(),
                use_effect_evo_values: b.use_effect_evo_values.get(),
                kfp_meta: meta(&[
                    &b.rgb_tint_min_kfp,
                    &b.rgb_tint_max_kfp,
                    &b.density_range_kfp,
                    &b.scale_min_kfp,
                    &b.scale_max_kfp,
                    &b.rotation_min_kfp,
                    &b.rotation_max_kfp,
                ]),
            }),
            B::PtxdSprite(b) => Behaviour::DrawSprite(DrawSpriteBehaviour {
                align_axis: axis(&b.align_axis),
                alignment_mode: AlignmentMode::from(b.alignment_mode as u32),
                flip_chance_u: b.flip_chance_u,
                flip_chance_v: b.flip_chance_v,
                near_clip_dist: b.near_clip_dist,
                far_clip_dist: b.far_clip_dist,
                projection_depth: b.projection_depth,
                shadow_cast_intensity: b.shadow_cast_intensity,
                is_screen_space: b.is_screen_space.get(),
                is_hi_res: b.is_hi_res.get(),
                near_clip: b.near_clip.get(),
                far_clip: b.far_clip.get(),
                uv_clip: b.uv_clip.get(),
                disable_draw: b.disable_draw.get(),
            }),
            B::PtxdTrail(b) => Behaviour::DrawTrail(DrawTrailBehaviour {
                tex_info_keyframes: data(&b.tex_info_kfp),
                align_axis: axis(&b.align_axis),
                alignment_mode: AlignmentMode::from(b.alignment_mode as u32),
                tessellation_u: b.tessellation_u as u32,
                tessellation_v: b.tessellation_v as u32,
                smoothness_x: b.smoothness_x,
                smoothness_y: b.smoothness_y,
                projection_depth: b.projection_depth,
                shadow_cast_intensity: b.shadow_cast_intensity,
                flip_u: b.flip_u.get(),
                flip_v: b.flip_v.get(),
                wrap_texture_over_particle_life: b.wrap_texture_over_particle_life.get(),
                disable_draw: b.disable_draw.get(),
                kfp_meta: meta(&[&b.tex_info_kfp]),
            }),
            B::PtxuCameraEvolution(b) => Behaviour::Unknown {
                type_id: b.base.hash_name,
                data: Vec::new(),
            },
            B::Base(b) => match b.hash_name {
                ptx::BEHAVIOUR_HASH_FADE_IN => Behaviour::FadeIn(FadeInBehaviour::default()),
                ptx::BEHAVIOUR_HASH_FADE_OUT => Behaviour::FadeOut(FadeOutBehaviour::default()),
                type_id => Behaviour::Unknown {
                    type_id,
                    data: Vec::new(),
                },
            },
        }
    }

    /// A record stating this behaviour: `old` where it is one of its class, so what the behaviour does not state is kept, else a new one listing its keyframed values.
    pub fn record(&self, old: Option<&PtxBehaviourAny>) -> PtxBehaviourAny {
        let b = self;
        use PtxBehaviourAny as B;
        macro_rules! record {
            ($variant:ident, $ty:ident, [$($kfp:literal),*]) => {{
                let mut out = match old {
                    Some(B::$variant(x)) => x.clone(),
                    _ => ptx::$ty {
                        base: ptx::PtxBehaviour {
                            hash_name: b.type_id(),
                            keyframe_prop_list: members(&[$($kfp),*]),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                };
                out.base.hash_name = b.type_id();
                out
            }};
        }
        let ids = b.kfp_property_ids();
        let put = |slot: &mut ptx::PtxKeyframeProp,
                   i: usize,
                   data: &Option<KeyframePropData>,
                   meta: &[KfpMeta]| {
            behaviour_kfp(slot, data, meta.get(i), ids.get(i).copied())
        };
        match b {
            Behaviour::Age(_) => B::PtxuAge(record!(PtxuAge, PtxuAge, [])),
            Behaviour::Velocity(_) => B::PtxuVelocity(record!(PtxuVelocity, PtxuVelocity, [])),
            Behaviour::DrawModel(d) => {
                let mut out = record!(PtxdModel, PtxdModel, []);
                out.colour_control_shader_id = d.colour_control_shader_id;
                out.camera_shrink = d.camera_shrink;
                out.shadow_cast_intensity = d.shadow_cast_intensity;
                set_flag(&mut out.disable_draw, d.disable_draw);
                B::PtxdModel(out)
            }
            Behaviour::River(d) => {
                let mut out = record!(PtxuRiver, PtxuRiver, []);
                out.velocity_mult = d.velocity_mult;
                out.influence = d.influence;
                B::PtxuRiver(out)
            }
            Behaviour::Liquid(d) => {
                let mut out = record!(PtxuLiquid, PtxuLiquid, []);
                out.velocity_thresh = d.velocity_thresh;
                out.liquid_type = d.liquid_type as i32;
                out.pool_start_size = d.pool_start_size;
                out.pool_end_size = d.pool_end_size;
                out.pool_growth_rate = d.pool_growth_rate;
                out.trail_width_min = d.trail_width_min;
                out.trail_width_max = d.trail_width_max;
                B::PtxuLiquid(out)
            }
            Behaviour::DecalPool(d) => {
                let mut out = record!(PtxuDecalPool, PtxuDecalPool, []);
                out.velocity_thresh = d.velocity_thresh;
                out.liquid_type = d.liquid_type as i32;
                out.decal_id = d.decal_id as i32;
                out.start_size = d.start_size;
                out.end_size = d.end_size;
                out.growth_rate = d.growth_rate;
                B::PtxuDecalPool(out)
            }
            Behaviour::Acceleration(d) => {
                let mut out = record!(
                    PtxuAcceleration,
                    PtxuAcceleration,
                    ["xyz_min_kfp", "xyz_max_kfp"]
                );
                put(&mut out.xyz_min_kfp, 0, &d.xyz_min_keyframes, &d.kfp_meta);
                put(&mut out.xyz_max_kfp, 1, &d.xyz_max_keyframes, &d.kfp_meta);
                out.reference_space = d.reference_space.0 as i32;
                set_flag(&mut out.is_affected_by_zoom, d.is_affected_by_zoom);
                set_flag(&mut out.enable_gravity, d.enable_gravity);
                B::PtxuAcceleration(out)
            }
            Behaviour::Rotation(d) => {
                let mut out = record!(
                    PtxuRotation,
                    PtxuRotation,
                    [
                        "initial_angle_min_kfp",
                        "initial_angle_max_kfp",
                        "angle_min_kfp",
                        "angle_max_kfp"
                    ]
                );
                put(
                    &mut out.initial_angle_min_kfp,
                    0,
                    &d.initial_angle_min_keyframes,
                    &d.kfp_meta,
                );
                put(
                    &mut out.initial_angle_max_kfp,
                    1,
                    &d.initial_angle_max_keyframes,
                    &d.kfp_meta,
                );
                put(
                    &mut out.angle_min_kfp,
                    2,
                    &d.angle_min_keyframes,
                    &d.kfp_meta,
                );
                put(
                    &mut out.angle_max_kfp,
                    3,
                    &d.angle_max_keyframes,
                    &d.kfp_meta,
                );
                out.init_rotation_mode = d.init_rotation_mode as i32;
                out.update_rotation_mode = d.update_rotation_mode as i32;
                set_flag(&mut out.accumulate_angle, d.accumulate_angle);
                set_flag(&mut out.rotate_angle_axes, d.rotate_angle_axes);
                set_flag(&mut out.rotate_init_angle_axes, d.rotate_init_angle_axes);
                out.speed_fade_thresh = d.speed_fade_thresh;
                B::PtxuRotation(out)
            }
            Behaviour::Size(d) => {
                let mut out = record!(
                    PtxuSize,
                    PtxuSize,
                    [
                        "whd_min_kfp",
                        "whd_max_kfp",
                        "tblr_scalar_kfp",
                        "tblr_vel_scalar_kfp"
                    ]
                );
                put(&mut out.whd_min_kfp, 0, &d.whd_min_keyframes, &d.kfp_meta);
                put(&mut out.whd_max_kfp, 1, &d.whd_max_keyframes, &d.kfp_meta);
                put(
                    &mut out.tblr_scalar_kfp,
                    2,
                    &d.tblr_scalar_keyframes,
                    &d.kfp_meta,
                );
                put(
                    &mut out.tblr_vel_scalar_kfp,
                    3,
                    &d.tblr_vel_scalar_keyframes,
                    &d.kfp_meta,
                );
                out.keyframe_mode = d.keyframe_mode as i32;
                set_flag(&mut out.is_proportional, d.is_proportional);
                B::PtxuSize(out)
            }
            Behaviour::Dampening(d) => {
                let mut out = record!(PtxuDampening, PtxuDampening, ["xyz_min_kfp", "xyz_max_kfp"]);
                put(&mut out.xyz_min_kfp, 0, &d.xyz_min_keyframes, &d.kfp_meta);
                put(&mut out.xyz_max_kfp, 1, &d.xyz_max_keyframes, &d.kfp_meta);
                out.reference_space = d.reference_space.0 as i32;
                set_flag(&mut out.enable_air_resistance, d.enable_air_resistance);
                B::PtxuDampening(out)
            }
            Behaviour::MatrixWeight(d) => {
                let mut out = record!(PtxuMatrixWeight, PtxuMatrixWeight, ["mtx_weight_kfp"]);
                put(&mut out.mtx_weight_kfp, 0, &d.weight_keyframes, &d.kfp_meta);
                out.reference_space = d.reference_space.0 as i32;
                B::PtxuMatrixWeight(out)
            }
            Behaviour::Collision(d) => {
                let mut out = record!(
                    PtxuCollision,
                    PtxuCollision,
                    ["bounciness_kfp", "bounce_dir_var_kfp"]
                );
                put(
                    &mut out.bounciness_kfp,
                    0,
                    &d.bounciness_keyframes,
                    &d.kfp_meta,
                );
                put(
                    &mut out.bounce_dir_var_kfp,
                    1,
                    &d.bounce_dir_var_keyframes,
                    &d.kfp_meta,
                );
                out.radius_mult = d.radius_mult;
                out.rest_speed = d.rest_speed;
                out.coln_chance = d.collision_chance as i32;
                out.kill_chance = d.kill_chance as i32;
                set_flag(&mut out.debug_draw, d.debug_draw);
                out.override_min_radius = d.override_min_radius;
                B::PtxuCollision(out)
            }
            Behaviour::AnimateTexture(d) => {
                let mut out = record!(PtxuAnimateTexture, PtxuAnimateTexture, ["anim_rate_kfp"]);
                put(
                    &mut out.anim_rate_kfp,
                    0,
                    &d.anim_rate_keyframes,
                    &d.kfp_meta,
                );
                out.keyframe_mode = d.keyframe_mode as i32;
                out.last_frame_id = d.last_frame_id as i32;
                out.loop_mode = d.loop_mode.0 as i32;
                set_flag(&mut out.is_randomised, d.is_randomised);
                set_flag(
                    &mut out.is_scaled_over_particle_life,
                    d.is_scaled_over_particle_life,
                );
                set_flag(&mut out.is_held_on_last_frame, d.is_held_on_last_frame);
                set_flag(&mut out.do_frame_blending, d.do_frame_blending);
                B::PtxuAnimateTexture(out)
            }
            Behaviour::Colour(d) => {
                let mut out = record!(
                    PtxuColour,
                    PtxuColour,
                    ["rgba_min_kfp", "rgba_max_kfp", "emissive_intensity_kfp"]
                );
                put(&mut out.rgba_min_kfp, 0, &d.rgba_min_keyframes, &d.kfp_meta);
                put(&mut out.rgba_max_kfp, 1, &d.rgba_max_keyframes, &d.kfp_meta);
                put(
                    &mut out.emissive_intensity_kfp,
                    2,
                    &d.emissive_intensity_keyframes,
                    &d.kfp_meta,
                );
                out.keyframe_mode = d.keyframe_mode as i32;
                set_flag(&mut out.rgba_max_enable, d.rgba_max_enable);
                set_flag(&mut out.rgba_proportional, d.rgba_proportional);
                set_flag(&mut out.rgb_can_tint, d.rgb_can_tint);
                B::PtxuColour(out)
            }
            Behaviour::Wind(d) => {
                let mut out = record!(PtxuWind, PtxuWind, ["influence_kfp"]);
                put(
                    &mut out.influence_kfp,
                    0,
                    &d.influence_keyframes,
                    &d.kfp_meta,
                );
                out.high_lod_range = d.high_lod_range;
                out.low_lod_range = d.low_lod_range;
                out.high_lod_disturbance_mode = d.high_lod_disturbance_mode as i32;
                out.low_lod_disturbance_mode = d.low_lod_disturbance_mode as i32;
                set_flag(&mut out.ignore_mtx_weight, d.ignore_mtx_weight);
                B::PtxuWind(out)
            }
            Behaviour::Light(d) => {
                let mut out = record!(
                    PtxuLight,
                    PtxuLight,
                    [
                        "rgb_min_kfp",
                        "rgb_max_kfp",
                        "intensity_kfp",
                        "range_kfp",
                        "corona_rgb_min_kfp",
                        "corona_rgb_max_kfp",
                        "corona_intensity_kfp",
                        "corona_size_kfp",
                        "corona_flare_kfp"
                    ]
                );
                let m = &d.kfp_meta;
                put(&mut out.rgb_min_kfp, 0, &d.rgb_min_keyframes, m);
                put(&mut out.rgb_max_kfp, 1, &d.rgb_max_keyframes, m);
                put(&mut out.intensity_kfp, 2, &d.intensity_keyframes, m);
                put(&mut out.range_kfp, 3, &d.range_keyframes, m);
                put(
                    &mut out.corona_rgb_min_kfp,
                    4,
                    &d.corona_rgb_min_keyframes,
                    m,
                );
                put(
                    &mut out.corona_rgb_max_kfp,
                    5,
                    &d.corona_rgb_max_keyframes,
                    m,
                );
                put(
                    &mut out.corona_intensity_kfp,
                    6,
                    &d.corona_intensity_keyframes,
                    m,
                );
                put(&mut out.corona_size_kfp, 7, &d.corona_size_keyframes, m);
                put(&mut out.corona_flare_kfp, 8, &d.corona_flare_keyframes, m);
                out.corona_z_bias = d.corona_z_bias;
                set_flag(&mut out.corona_use_light_col, d.corona_use_light_col);
                set_flag(&mut out.colour_from_particle, d.colour_from_particle);
                set_flag(&mut out.colour_per_frame, d.colour_per_frame);
                set_flag(&mut out.intensity_per_frame, d.intensity_per_frame);
                set_flag(&mut out.range_per_frame, d.range_per_frame);
                set_flag(&mut out.casts_shadows, d.casts_shadows);
                set_flag(
                    &mut out.corona_not_in_reflection,
                    d.corona_not_in_reflection,
                );
                set_flag(
                    &mut out.corona_only_in_reflection,
                    d.corona_only_in_reflection,
                );
                out.light_type = d.light_type as i32;
                B::PtxuLight(out)
            }
            Behaviour::Decal(d) => {
                let mut out = record!(PtxuDecal, PtxuDecal, ["dimensions_kfp", "alpha_kfp"]);
                put(
                    &mut out.dimensions_kfp,
                    0,
                    &d.dimensions_keyframes,
                    &d.kfp_meta,
                );
                put(&mut out.alpha_kfp, 1, &d.alpha_keyframes, &d.kfp_meta);
                out.decal_id = d.decal_id as i32;
                out.velocity_thresh = d.velocity_thresh;
                out.total_life = d.total_life;
                out.fade_in_time = d.fade_in_time;
                out.uv_mult_start = d.uv_mult_start;
                out.uv_mult_end = d.uv_mult_end;
                out.uv_mult_time = d.uv_mult_time;
                out.duplicate_reject_dist = d.duplicate_reject_dist;
                set_flag(&mut out.flip_u, d.flip_u);
                set_flag(&mut out.flip_v, d.flip_v);
                set_flag(&mut out.proportional_size, d.proportional_size);
                set_flag(&mut out.use_complex_coln, d.use_complex_coln);
                out.projection_depth = d.projection_depth;
                out.distance_scale = d.distance_scale;
                set_flag(&mut out.is_directional, d.is_directional);
                B::PtxuDecal(out)
            }
            Behaviour::ZCulling(d) => {
                let mut out = record!(PtxuZCull, PtxuZCull, ["height_kfp", "fade_dist_kfp"]);
                put(&mut out.height_kfp, 0, &d.height_keyframes, &d.kfp_meta);
                put(
                    &mut out.fade_dist_kfp,
                    1,
                    &d.fade_dist_keyframes,
                    &d.kfp_meta,
                );
                out.mode = d.mode as i32;
                out.reference_space = d.reference_space.0 as i32;
                B::PtxuZCull(out)
            }
            Behaviour::Noise(d) => {
                let mut out = record!(
                    PtxuNoise,
                    PtxuNoise,
                    [
                        "pos_noise_min_kfp",
                        "pos_noise_max_kfp",
                        "vel_noise_min_kfp",
                        "vel_noise_max_kfp"
                    ]
                );
                let m = &d.kfp_meta;
                put(&mut out.pos_noise_min_kfp, 0, &d.pos_noise_min_keyframes, m);
                put(&mut out.pos_noise_max_kfp, 1, &d.pos_noise_max_keyframes, m);
                put(&mut out.vel_noise_min_kfp, 2, &d.vel_noise_min_keyframes, m);
                put(&mut out.vel_noise_max_kfp, 3, &d.vel_noise_max_keyframes, m);
                out.reference_space = d.reference_space.0 as i32;
                set_flag(&mut out.keep_constant_speed, d.keep_constant_speed);
                B::PtxuNoise(out)
            }
            Behaviour::Attractor(d) => {
                let mut out = record!(PtxuAttractor, PtxuAttractor, ["strength_kfp"]);
                put(&mut out.strength_kfp, 0, &d.strength_keyframes, &d.kfp_meta);
                B::PtxuAttractor(out)
            }
            Behaviour::FogVolume(d) => {
                let mut out = record!(
                    PtxuFogVolume,
                    PtxuFogVolume,
                    [
                        "rgb_tint_min_kfp",
                        "rgb_tint_max_kfp",
                        "density_range_kfp",
                        "scale_min_kfp",
                        "scale_max_kfp",
                        "rotation_min_kfp",
                        "rotation_max_kfp"
                    ]
                );
                let m = &d.kfp_meta;
                put(&mut out.rgb_tint_min_kfp, 0, &d.rgb_tint_min_keyframes, m);
                put(&mut out.rgb_tint_max_kfp, 1, &d.rgb_tint_max_keyframes, m);
                put(&mut out.density_range_kfp, 2, &d.density_range_keyframes, m);
                put(&mut out.scale_min_kfp, 3, &d.scale_min_keyframes, m);
                put(&mut out.scale_max_kfp, 4, &d.scale_max_keyframes, m);
                put(&mut out.rotation_min_kfp, 5, &d.rotation_min_keyframes, m);
                put(&mut out.rotation_max_kfp, 6, &d.rotation_max_keyframes, m);
                out.falloff = d.falloff;
                out.hdr_mult = d.hdr_mult;
                out.lighting_type = d.lighting_type as i32;
                set_flag(
                    &mut out.colour_tint_from_particle,
                    d.colour_tint_from_particle,
                );
                set_flag(&mut out.sort_with_particles, d.sort_with_particles);
                set_flag(&mut out.use_ground_fog_colour, d.use_ground_fog_colour);
                set_flag(&mut out.use_effect_evo_values, d.use_effect_evo_values);
                B::PtxuFogVolume(out)
            }
            Behaviour::DrawSprite(d) => {
                let mut out = record!(PtxdSprite, PtxdSprite, []);
                set_vec3(
                    &mut out.align_axis,
                    [d.align_axis.x, d.align_axis.y, d.align_axis.z],
                );
                out.alignment_mode = d.alignment_mode as i32;
                out.flip_chance_u = d.flip_chance_u;
                out.flip_chance_v = d.flip_chance_v;
                out.near_clip_dist = d.near_clip_dist;
                out.far_clip_dist = d.far_clip_dist;
                out.projection_depth = d.projection_depth;
                out.shadow_cast_intensity = d.shadow_cast_intensity;
                set_flag(&mut out.is_screen_space, d.is_screen_space);
                set_flag(&mut out.is_hi_res, d.is_hi_res);
                set_flag(&mut out.near_clip, d.near_clip);
                set_flag(&mut out.far_clip, d.far_clip);
                set_flag(&mut out.uv_clip, d.uv_clip);
                set_flag(&mut out.disable_draw, d.disable_draw);
                B::PtxdSprite(out)
            }
            Behaviour::DrawTrail(d) => {
                let mut out = record!(PtxdTrail, PtxdTrail, ["tex_info_kfp"]);
                put(&mut out.tex_info_kfp, 0, &d.tex_info_keyframes, &d.kfp_meta);
                set_vec3(
                    &mut out.align_axis,
                    [d.align_axis.x, d.align_axis.y, d.align_axis.z],
                );
                out.alignment_mode = d.alignment_mode as i32;
                out.tessellation_u = d.tessellation_u as i32;
                out.tessellation_v = d.tessellation_v as i32;
                out.smoothness_x = d.smoothness_x;
                out.smoothness_y = d.smoothness_y;
                out.projection_depth = d.projection_depth;
                out.shadow_cast_intensity = d.shadow_cast_intensity;
                set_flag(&mut out.flip_u, d.flip_u);
                set_flag(&mut out.flip_v, d.flip_v);
                set_flag(
                    &mut out.wrap_texture_over_particle_life,
                    d.wrap_texture_over_particle_life,
                );
                set_flag(&mut out.disable_draw, d.disable_draw);
                B::PtxdTrail(out)
            }
            // ⚠ The owned list declares no layout for these classes, nor for one it does not know: the record the list held is kept, else a bare base.
            Behaviour::RotationSpeed(_)
            | Behaviour::Gravity(_)
            | Behaviour::Pause(_)
            | Behaviour::FadeIn(_)
            | Behaviour::FadeOut(_)
            | Behaviour::Unknown { .. } => match old {
                Some(o) if hash_of(o) == b.type_id() => o.clone(),
                _ => B::Base(ptx::PtxBehaviour {
                    hash_name: b.type_id(),
                    ..Default::default()
                }),
            },
        }
    }

    /// Which of a particle rule's lists besides `all_behaviours` this behaviour's class is in: init, update, update-finalize, draw, release.
    pub fn lists(&self) -> [bool; 5] {
        let type_id = self.type_id();
        use ptx::*;
        match type_id {
            BEHAVIOUR_HASH_AGE
            | BEHAVIOUR_HASH_VELOCITY
            | BEHAVIOUR_HASH_ROTATION_SPEED
            | BEHAVIOUR_HASH_GRAVITY
            | BEHAVIOUR_HASH_ACCELERATION
            | BEHAVIOUR_HASH_ROTATION
            | BEHAVIOUR_HASH_SIZE
            | BEHAVIOUR_HASH_COLOUR
            | BEHAVIOUR_HASH_DAMPENING
            | BEHAVIOUR_HASH_MATRIX_WEIGHT
            | BEHAVIOUR_HASH_NOISE
            | BEHAVIOUR_HASH_WIND
            | BEHAVIOUR_HASH_ANIMATE_TEXTURE
            | BEHAVIOUR_HASH_ATTRACTOR
            | BEHAVIOUR_HASH_PAUSE => [true, true, false, false, false],
            BEHAVIOUR_HASH_COLLISION => [true, false, true, false, false],
            BEHAVIOUR_HASH_FADE_IN | BEHAVIOUR_HASH_FADE_OUT => [true, false, false, false, false],
            BEHAVIOUR_HASH_LIGHT
            | BEHAVIOUR_HASH_DECAL
            | BEHAVIOUR_HASH_ZCULL
            | BEHAVIOUR_HASH_RIVER
            | BEHAVIOUR_HASH_DECAL_POOL
            | BEHAVIOUR_HASH_LIQUID => [false, false, true, false, false],
            BEHAVIOUR_HASH_FOG_VOLUME | BEHAVIOUR_HASH_TRAIL => [false, false, true, true, false],
            BEHAVIOUR_HASH_SPRITE | BEHAVIOUR_HASH_MODEL => [false, false, false, true, false],
            _ => [false; 5],
        }
    }
}

/// The first key of a keyframed value, which the editor shows as its value.
fn first(k: &ptx::PtxKeyframeProp) -> Option<&Vec4V> {
    k.keyframe.entries.first().map(|e| &e.value)
}

fn scalar(k: &ptx::PtxKeyframeProp) -> f32 {
    first(k).map_or(0.0, |v| v.x)
}

fn vec3(k: &ptx::PtxKeyframeProp) -> Vec3 {
    first(k).map_or(Vec3::default(), |v| Vec3::new(v.x, v.y, v.z))
}

fn vec4(k: &ptx::PtxKeyframeProp) -> (f32, f32, f32, f32) {
    first(k).map_or((0.0, 0.0, 0.0, 0.0), |v| (v.x, v.y, v.z, v.w))
}

fn axis(v: &Vec3V) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn data(k: &ptx::PtxKeyframeProp) -> Option<KeyframePropData> {
    KeyframePropData::of(k)
}

fn meta(kfps: &[&ptx::PtxKeyframeProp]) -> Vec<KfpMeta> {
    kfps.iter()
        .map(|k| KfpMeta {
            property_id: k.property_id.get(),
            rand_index: k.rand_index,
            invert_bias_link: k.invert_bias_link.raw(),
        })
        .collect()
}

pub(crate) fn set_flag(slot: &mut Bool8, value: bool) {
    if slot.get() != value {
        *slot = Bool8::new(value);
    }
}

fn set_vec3(slot: &mut Vec3V, [x, y, z]: [f32; 3]) {
    (slot.x, slot.y, slot.z) = (x, y, z);
}

/// A behaviour's keyframed value: its keys, and what it states besides them from the keys, else the meta, else its class.
fn behaviour_kfp(
    slot: &mut ptx::PtxKeyframeProp,
    data: &Option<KeyframePropData>,
    meta: Option<&KfpMeta>,
    class_id: Option<u32>,
) {
    let (property_id, rand_index, invert) = match (data, meta) {
        (Some(d), _) => (d.property_id, d.rand_index, d.invert_bias_link),
        (None, Some(m)) => (m.property_id, m.rand_index, m.invert_bias_link),
        (None, None) => (class_id.unwrap_or(0), 0, 0),
    };
    slot.property_id = Hash32::from_raw(property_id);
    slot.rand_index = rand_index;
    set_flag(&mut slot.invert_bias_link, invert != 0);
    set_entries(
        &mut slot.keyframe,
        data.as_ref().map_or(&[][..], |d| &d.entries),
    );
}

fn members(fields: &[&str]) -> Vec<Member<ptx::PtxKeyframeProp>> {
    fields.iter().map(|f| Member::to(f)).collect()
}

pub(crate) fn hash_of(b: &PtxBehaviourAny) -> u32 {
    use PtxBehaviourAny as B;
    match b {
        B::PtxuAge(x) => x.base.hash_name,
        B::PtxuAcceleration(x) => x.base.hash_name,
        B::PtxuVelocity(x) => x.base.hash_name,
        B::PtxuRotation(x) => x.base.hash_name,
        B::PtxuSize(x) => x.base.hash_name,
        B::PtxuDampening(x) => x.base.hash_name,
        B::PtxuMatrixWeight(x) => x.base.hash_name,
        B::PtxuCollision(x) => x.base.hash_name,
        B::PtxuAnimateTexture(x) => x.base.hash_name,
        B::PtxuColour(x) => x.base.hash_name,
        B::PtxdSprite(x) => x.base.hash_name,
        B::PtxuWind(x) => x.base.hash_name,
        B::PtxuLight(x) => x.base.hash_name,
        B::PtxdModel(x) => x.base.hash_name,
        B::PtxuDecal(x) => x.base.hash_name,
        B::PtxuZCull(x) => x.base.hash_name,
        B::PtxuNoise(x) => x.base.hash_name,
        B::PtxuAttractor(x) => x.base.hash_name,
        B::PtxdTrail(x) => x.base.hash_name,
        B::PtxuFogVolume(x) => x.base.hash_name,
        B::PtxuRiver(x) => x.base.hash_name,
        B::PtxuDecalPool(x) => x.base.hash_name,
        B::PtxuLiquid(x) => x.base.hash_name,
        B::PtxuCameraEvolution(x) => x.base.hash_name,
        B::Base(x) => x.hash_name,
    }
}
