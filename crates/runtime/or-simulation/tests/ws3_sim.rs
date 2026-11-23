use glam::{Mat4, Vec3, Vec4};
use or_io::gta5::particle::PtxFxList;
use or_particle::behaviour::{
    Behaviour, CollisionBehaviour, DecalPoolBehaviour, LiquidBehaviour, RiverBehaviour,
    VelocityBehaviour,
};
use or_simulation::{
    behaviours::{ConstantRiverField, LiquidEmit},
    build, BvhCollider, PointFlags, PreviewTarget, PtfxManager,
};
use std::sync::Arc;
use synthetic::{add_effect, index_of, particle, EFFECT};

mod synthetic;

fn synthetic_asset_multi(num_events: usize, behaviours: Vec<Behaviour>) -> PtxFxList {
    synthetic::list_of(num_events, 60.0, 60.0, 0.5, &behaviours)
}

fn ground_plane_bvh() -> Arc<BvhCollider> {
    let tris = [(
        Vec3::new(-100.0, -100.0, 0.0),
        Vec3::new(100.0, -100.0, 0.0),
        Vec3::new(0.0, 100.0, 0.0),
    )];
    Arc::new(BvhCollider::from_world_triangles(&tris, &[3]))
}

fn force_collision_on_all(manager: &mut PtfxManager, hit_pos: Vec3, hit_normal: Vec3) {
    if let Some(eff) = manager.effect.as_mut() {
        for em in eff.emitters.iter_mut() {
            for i in 0..em.points.len() {
                em.points.flags[i].insert(PointFlags::HAS_COLLIDED);
                em.points.flags[i].insert(PointFlags::COLLIDES);
                em.points.coln_pos[i] = hit_pos;
                em.points.coln_normal[i] = hit_normal;
                em.points.coln_speed[i] = 5.0;
                em.points.curr_vel[i] = Vec3::new(1.0, 0.0, 0.0);
            }
        }
    }
}

#[test]
fn liquid_pool_grows_at_rate() {
    let liquid = Behaviour::Liquid(LiquidBehaviour {
        velocity_thresh: 0.0,
        liquid_type: 7,
        pool_start_size: 0.4,
        pool_end_size: 2.0,
        pool_growth_rate: 1.0,
        trail_width_min: 0.0,
        trail_width_max: 0.0,
    });
    let asset = synthetic_asset_multi(1, vec![liquid]);
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    manager.clock.play();

    let dt = 1.0 / 60.0;
    manager.tick(dt);
    force_collision_on_all(&mut manager, Vec3::new(0.0, 0.0, 0.0), Vec3::Z);
    manager.tick(dt);

    let pools_after_first = manager.liquid_batch().pools.clone();
    assert!(
        !pools_after_first.is_empty(),
        "liquid behaviour should have queued a pool record after the first \
         post-collision tick (got {} pools)",
        pools_after_first.len()
    );
    let radius_after_first = pools_after_first[0].radius;
    assert!(
        (radius_after_first - 0.4).abs() < 1e-4,
        "expected pool to start at start_size=0.4, got {radius_after_first}"
    );

    if let Some(eff) = manager.effect.as_mut() {
        for em in eff.emitters.iter_mut() {
            em.points.is_dead.iter_mut().for_each(|d| *d = true);
            em.points.sweep_dead();
        }
    }
    let frames = 60;
    for _ in 0..frames {
        manager.tick(dt);
    }
    let pools_after = &manager.liquid_batch().pools;
    assert!(!pools_after.is_empty());
    let grown = pools_after[0].radius;
    let expected_growth = (1.0_f32 * dt * frames as f32).min(2.0 - radius_after_first);
    let observed_growth = grown - radius_after_first;
    assert!(
        (observed_growth - expected_growth).abs() < 1e-2,
        "expected pool radius to grow by {expected_growth:.4}, got {observed_growth:.4} \
         (radius_after_first={radius_after_first:.4}, final={grown:.4})"
    );
    assert!(
        grown <= 2.0 + 1e-4,
        "pool radius {grown} exceeded end_size 2.0"
    );
}

#[test]
fn river_applies_velocity_mult_in_field() {
    let river = Behaviour::River(RiverBehaviour {
        velocity_mult: 2.0,
        influence: 1000.0,
    });
    let asset = synthetic_asset_multi(1, vec![river]);
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_river_field(Some(Arc::new(ConstantRiverField {
        velocity: Vec3::new(1.0, 0.0, 0.0),
    })));
    manager.set_effect(inst);
    manager.clock.play();

    let dt = 1.0 / 60.0;
    manager.tick(dt);
    if let Some(eff) = manager.effect.as_mut() {
        for em in eff.emitters.iter_mut() {
            for i in 0..em.points.len() {
                em.points.curr_vel[i] = Vec3::ZERO;
            }
        }
    }
    manager.tick(dt);
    let mut found_match = false;
    if let Some(eff) = manager.effect.as_ref() {
        for em in eff.emitters.iter() {
            for &v in &em.points.curr_vel[..em.points.len()] {
                if (v - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-3 {
                    found_match = true;
                    break;
                }
            }
        }
    }
    assert!(
        found_match,
        "expected at least one particle's velocity to equal (2,0,0) after river blend"
    );
}

#[test]
fn decal_pool_merges_overlapping_decals() {
    let decal = Behaviour::DecalPool(DecalPoolBehaviour {
        velocity_thresh: 0.0,
        liquid_type: 0,
        decal_id: 42,
        start_size: 0.5,
        end_size: 2.0,
        growth_rate: 0.0,
    });
    let asset = synthetic_asset_multi(1, vec![decal]);
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    manager.clock.play();

    let dt = 1.0 / 60.0;
    manager.tick(dt);
    force_collision_on_all(&mut manager, Vec3::new(0.5, 0.5, 0.0), Vec3::Z);
    manager.tick(dt);
    let decals = &manager.decal_pool_batch().decals;
    assert_eq!(
        decals.len(),
        1,
        "all overlapping decal emits should collapse into a single batch entry, \
         got {} decals: {:?}",
        decals.len(),
        decals
    );
    assert_eq!(decals[0].decal_id, 42);
}

#[test]
fn parallel_tick_matches_serial_tick() {
    let asset = synthetic_asset_multi(6, vec![]);
    let inst_a = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let inst_b = build(&asset, PreviewTarget::Effect(0)).unwrap();

    let mut mgr_a = PtfxManager::new();
    mgr_a.set_effect(inst_a);
    mgr_a.clock.play();

    let mut mgr_b = PtfxManager::new();
    mgr_b.set_effect(inst_b);
    mgr_b.clock.play();

    let serial_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build single-thread rayon pool");
    let parallel_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .expect("build multi-thread rayon pool");

    let dt = 1.0 / 60.0;
    for _ in 0..120 {
        serial_pool.install(|| mgr_a.tick(dt));
        parallel_pool.install(|| mgr_b.tick(dt));
    }

    let emitters_a: Vec<_> = mgr_a.emitters().collect();
    let emitters_b: Vec<_> = mgr_b.emitters().collect();
    assert_eq!(
        emitters_a.len(),
        emitters_b.len(),
        "emitter count diverged across serial / parallel runs"
    );
    for (i, (em_a, em_b)) in emitters_a.iter().zip(emitters_b.iter()).enumerate() {
        let pa = &em_a.points;
        let pb = &em_b.points;
        assert_eq!(pa.len(), pb.len(), "emitter {i} live count diverged");
        for j in 0..pa.len() {
            assert_eq!(
                pa.curr_pos[j], pb.curr_pos[j],
                "emitter {i} particle {j} curr_pos"
            );
            assert_eq!(
                pa.prev_pos[j], pb.prev_pos[j],
                "emitter {i} particle {j} prev_pos"
            );
            assert_eq!(
                pa.curr_vel[j], pb.curr_vel[j],
                "emitter {i} particle {j} curr_vel"
            );
            assert_eq!(
                pa.life_ratio[j], pb.life_ratio[j],
                "emitter {i} particle {j} life_ratio"
            );
            assert_eq!(
                pa.rand_index[j], pb.rand_index[j],
                "emitter {i} particle {j} rand_index"
            );
            assert_eq!(pa.color[j], pb.color[j], "emitter {i} particle {j} color");
            assert_eq!(
                pa.dimensions[j], pb.dimensions[j],
                "emitter {i} particle {j} dimensions"
            );
        }
    }
}

#[test]
fn parallel_pending_spawns_drains_completely() {
    use or_io::gta5::particle::PtxSpawnedEffectScalars;

    let mut asset = synthetic_asset_multi(4, vec![]);
    add_effect(&mut asset, "child_fx", 0.5);
    #[allow(clippy::field_reassign_with_default)]
    let identity = {
        let mut s = PtxSpawnedEffectScalars::default();
        s.duration_scalar = 1.0;
        s.playback_rate_scalar = 1.0;
        s.zoom_scalar = 1.0;
        s.colour_tint_scalar = 0xFFFFFFFF;
        s.flags = 0;
        s
    };
    let spawner = &mut particle(&mut asset).effect_spawner_at_ratio;
    spawner.name = "child_fx".into();
    spawner.spawned_effect_scalars_min = identity.clone();
    spawner.spawned_effect_scalars_max = identity;

    let inst = build(&asset, PreviewTarget::Effect(index_of(&asset, EFFECT))).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_asset_snapshot(asset);
    manager.set_effect(inst);
    manager.clock.play();

    let dt = 1.0 / 60.0;
    for _ in 0..30 {
        manager.tick(dt);
    }

    if let Some(eff) = manager.effect.as_ref() {
        for (i, em) in eff.emitters.iter().enumerate() {
            assert!(
                em.pending_spawns.is_empty(),
                "emitter {i} still has {} undrained pending_spawns after parallel walk",
                em.pending_spawns.len()
            );
            assert!(
                em.pending_liquid_emits.is_empty(),
                "emitter {i} still has {} undrained pending_liquid_emits",
                em.pending_liquid_emits.len()
            );
            assert!(
                em.pending_decal_pool_emits.is_empty(),
                "emitter {i} still has {} undrained pending_decal_pool_emits",
                em.pending_decal_pool_emits.len()
            );
        }
    }
    assert!(
        !manager.spawned_effects.is_empty(),
        "expected at-ratio spawner to have produced child effects under \
         the parallel emitter walk"
    );
}

#[test]
fn collision_uses_bvh_when_present() {
    let col = CollisionBehaviour {
        collision_chance: 100,
        kill_chance: 0,
        bounciness_keyframes: Some(or_particle::keyframe::KeyframePropData {
            entries: vec![or_io::gta5::particle::RawKeyframeEntry {
                time: [0.0; 4],
                value: [50.0, 50.0, 0.0, 0.0],
            }],
            ..Default::default()
        }),
        ..CollisionBehaviour::default()
    };
    let asset = synthetic_asset_multi(
        1,
        vec![
            Behaviour::Velocity(VelocityBehaviour::default()),
            Behaviour::Collision(col),
        ],
    );
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_collision_bvh(Some(ground_plane_bvh()));
    manager.set_effect(inst);
    manager.clock.play();

    let dt = 1.0 / 60.0;
    manager.tick(dt);
    if let Some(eff) = manager.effect.as_mut() {
        for em in eff.emitters.iter_mut() {
            for i in 0..em.points.len() {
                em.points.prev_pos[i] = Vec3::new(0.0, 0.0, 1.0);
                em.points.curr_pos[i] = Vec3::new(0.0, 0.0, 1.0);
                em.points.curr_vel[i] = Vec3::new(0.0, 0.0, -120.0);
            }
        }
    }
    manager.tick(dt);
    if let Some(eff) = manager.effect.as_ref() {
        let any_hit = eff.emitters.iter().any(|em| {
            (0..em.points.len()).any(|i| em.points.flags[i].contains(PointFlags::HAS_COLLIDED))
        });
        assert!(
            any_hit,
            "BVH ray-cast should have flagged HAS_COLLIDED on at least one particle"
        );
    }
    if let Some(eff) = manager.effect.as_ref() {
        let any_up = eff
            .emitters
            .iter()
            .any(|em| (0..em.points.len()).any(|i| em.points.curr_vel[i].z > 0.0));
        assert!(any_up, "post-collision velocity should reflect upward");
    }
    let _ = (
        Vec4::ONE,
        Mat4::IDENTITY,
        LiquidEmit {
            pos: Vec3::ZERO,
            normal: Vec3::Z,
            color: Vec4::ONE,
            mtl_id: 0,
            liquid_type: 0,
            trail_width_min: 0.0,
            trail_width_max: 0.0,
            pool_start_size: 0.0,
            pool_end_size: 0.0,
            pool_growth_rate: 0.0,
        },
    );
}
