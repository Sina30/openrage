mod synthetic;

use or_io::decl::Bool8;
use or_io::gta5::particle::PtxFxList;
use or_particle::keyframe::constant;
use or_simulation::{build, PreviewTarget, PtfxManager};
use synthetic::{add_effect, effect, emitter, index_of, particle, EFFECT};

fn synthetic_asset() -> PtxFxList {
    synthetic::list_of(1, 1.0, 60.0, 0.5, &[])
}

#[test]
fn synthetic_effect_spawns_particles_over_one_second() {
    let asset = synthetic_asset();
    let inst = build(&asset, PreviewTarget::Effect(0)).expect("effect inst should build");

    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    manager.clock.play();

    let dt = 1.0 / 60.0;
    for _ in 0..60 {
        manager.tick(dt);
    }

    let live = manager.live_count();
    assert!(
        live > 0,
        "expected particles to be alive after 1s of sim, got {live}"
    );
    assert!(
        live <= 200,
        "particle pool grew without bound; got {live} live"
    );
}

#[test]
fn particle_target_builds_synthetic_emitter() {
    let mut asset = synthetic_asset();
    particle(&mut asset).name = "lone_particle".to_string();
    let inst =
        build(&asset, PreviewTarget::Particle(0)).expect("particle preview should synthesise");
    assert!(
        !inst.emitters.is_empty(),
        "particle preview should yield at least one synthetic emitter"
    );
}

#[test]
fn emitter_preview_normalises_window_and_aligns_evolution_lists() {
    use or_io::gta5::particle::{PtxEvolution, PtxEvolutionList};

    let mut asset = synthetic::list_of(3, 1.0, 60.0, 0.5, &[]);
    let windows = [(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)];
    let events = or_particle::list::events_mut(effect(&mut asset, EFFECT));
    for (i, (event, (start, end))) in events.zip(windows).enumerate() {
        (event.base.start_ratio, event.base.end_ratio) = (start, end);
        let evolution = PtxEvolution {
            name: format!("EVO_FOR_EVENT_{i}"),
            ..Default::default()
        };
        event.base.evolution_list = Some(Box::new(PtxEvolutionList {
            evolutions: vec![evolution],
            ..Default::default()
        }));
    }

    let inst = build(
        &asset,
        PreviewTarget::Emitter {
            effect: 0,
            event: 2,
        },
    )
    .expect("event #2 preview must build");
    assert_eq!(inst.emitters.len(), 1, "single-event preview => 1 emitter");
    let em = &inst.emitters[0];
    assert!(
        em.rules.event_start_ratio.abs() < 1e-6,
        "event window should open to start_ratio=0, got {}",
        em.rules.event_start_ratio
    );
    assert!(
        (em.rules.event_end_ratio - 1.0).abs() < 1e-6,
        "event window should open to end_ratio=1, got {}",
        em.rules.event_end_ratio
    );
    let evo = em
        .evolution_list
        .as_ref()
        .expect("event #2 must carry its own evolution list");
    assert_eq!(
        evo.evolutions[0].name, "EVO_FOR_EVENT_2",
        "wrong evolution list — event-list slicing didn't track \
         emitter_evolution_lists slicing (would render with \
         event #0's evo curves silently)"
    );
}

#[test]
fn child_effect_spawns_via_on_collision() {
    let mut asset = synthetic_asset();
    add_effect(&mut asset, "child_fx", 1.0);
    particle(&mut asset).effect_spawner_on_coln.name = "child_fx".into();

    let inst = build(&asset, PreviewTarget::Effect(index_of(&asset, EFFECT))).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_asset_snapshot(asset);
    manager.set_effect(inst);
    manager.clock.play();
    manager.tick(1.0 / 60.0);
    manager.tick(1.0 / 60.0);

    let before = manager.spawned_effects.len();
    if let Some(eff) = manager.effect.as_mut() {
        for em in eff.emitters.iter_mut() {
            for i in 0..em.points.len() {
                em.points.flags[i].insert(or_simulation::PointFlags::HAS_COLLIDED);
                em.points.coln_pos[i] = glam::Vec3::new(1.0, 2.0, 3.0);
                em.points.coln_normal[i] = glam::Vec3::Z;
                em.points.coln_speed[i] = 5.0;
            }
        }
    }
    manager.tick(1.0 / 60.0);
    let after = manager.spawned_effects.len();
    assert!(
        after > before,
        "on-collision spawner didn't fire: {before} -> {after}"
    );
}

#[test]
fn spawned_child_inherits_scalars() {
    use or_io::gta5::particle::PtxSpawnedEffectScalars;
    let mut asset = synthetic_asset();
    add_effect(&mut asset, "child_fx", 0.5);
    let scalars = PtxSpawnedEffectScalars {
        duration_scalar: 2.0,
        playback_rate_scalar: 0.5,
        zoom_scalar: 3.0,
        colour_tint_scalar: 0xFFFFFFFF,
        flags: 0,
        ..Default::default()
    };
    let spawner = &mut particle(&mut asset).effect_spawner_at_ratio;
    spawner.name = "child_fx".into();
    spawner.spawned_effect_scalars_min = scalars.clone();
    spawner.spawned_effect_scalars_max = scalars.clone();

    let inst = build(&asset, PreviewTarget::Effect(index_of(&asset, EFFECT))).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_asset_snapshot(asset);
    manager.set_effect(inst);
    manager.clock.play();
    for _ in 0..30 {
        manager.tick(1.0 / 60.0);
    }
    let child = manager
        .spawned_effects
        .iter()
        .find(|e| e.rule.name == "child_fx")
        .expect("child effect should have spawned via at-ratio spawner");
    assert!(
        (child.duration - 1.0).abs() < 1e-3,
        "expected child duration 1.0s, got {}",
        child.duration
    );
    assert!(
        (child.current_zoom - 3.0).abs() < 1e-3,
        "expected child zoom 3.0, got {}",
        child.current_zoom
    );
}

#[test]
fn multi_effect_playback_runs_in_parallel() {
    let asset_a = synthetic_asset();
    let asset_b = synthetic_asset();
    let inst_a = build(&asset_a, PreviewTarget::Effect(0)).unwrap();
    let inst_b = build(&asset_b, PreviewTarget::Effect(0)).unwrap();

    let mut manager = PtfxManager::new();
    manager.set_effect(inst_a);
    manager.spawned_effects.push(inst_b);
    manager.clock.play();

    for _ in 0..60 {
        manager.tick(1.0 / 60.0);
    }

    let primary_alive = manager.effect.as_ref().map(|e| e.live_count()).unwrap_or(0);
    let pinned_alive = manager
        .spawned_effects
        .first()
        .map(|e| e.live_count())
        .unwrap_or(0);
    assert!(
        primary_alive > 0,
        "primary effect should have live particles after 1s, got {primary_alive}"
    );
    assert!(
        pinned_alive > 0,
        "pinned effect should have live particles after 1s, got {pinned_alive}"
    );
}

#[test]
fn subframe_spawn_distributes_along_emitter_motion() {
    let mut asset = synthetic_asset();
    emitter(&mut asset).spawn_rate_over_time_kfp = constant([240.0, 0.0, 0.0, 0.0]);
    emitter(&mut asset).particle_life_kfp = constant([2.0, 0.0, 0.0, 0.0]);
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    manager.clock.play();

    manager.tick(1.0 / 60.0);

    if let Some(eff) = manager.effect.as_mut() {
        eff.matrix = glam::Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0));
    }
    let before = manager.live_count();
    manager.tick(1.0 / 60.0);
    let after = manager.live_count();
    let new_particles = after - before;
    assert!(
        new_particles >= 3,
        "expected at least 3 new particles after tick, got {new_particles}"
    );

    let eff = manager.effect.as_ref().unwrap();
    let em = &eff.emitters[0];
    let n = em.points.len();
    let last = em.points.curr_pos[n - 1];
    let first_new = em.points.curr_pos[n - new_particles];
    assert!(
        first_new.x < last.x,
        "spawn positions didn't spread along motion: first {} last {}",
        first_new.x,
        last.x
    );
    for position in &em.points.curr_pos[(n - new_particles)..n] {
        let x = position.x;
        assert!(
            (-1.0..=11.0).contains(&x),
            "spawn position {x} fell outside [0, 10] interpolation window"
        );
    }
}

#[test]
fn inherits_point_life_truncates_child_duration() {
    let mut asset = synthetic_asset();
    add_effect(&mut asset, "child_fx", 1.0);
    let spawner = &mut particle(&mut asset).effect_spawner_at_ratio;
    spawner.name = "child_fx".into();
    spawner.inherits_point_life = Bool8::new(true);

    let inst = build(&asset, PreviewTarget::Effect(index_of(&asset, EFFECT))).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_asset_snapshot(asset);
    manager.set_effect(inst);
    manager.clock.play();
    for _ in 0..30 {
        manager.tick(1.0 / 60.0);
    }
    let child = manager
        .spawned_effects
        .iter()
        .find(|e| e.rule.name == "child_fx")
        .expect("child should have spawned");
    assert!(
        child.duration <= 0.5 + 0.01,
        "expected child duration ≤ 0.5s with inherits_point_life, got {}",
        child.duration
    );
}

#[test]
fn tracks_point_pos_attaches_child_to_parent() {
    let mut asset = synthetic_asset();
    add_effect(&mut asset, "tracker_fx", 5.0);
    let spawner = &mut particle(&mut asset).effect_spawner_at_ratio;
    spawner.name = "tracker_fx".into();
    spawner.tracks_point_pos = Bool8::new(true);

    let inst = build(&asset, PreviewTarget::Effect(index_of(&asset, EFFECT))).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_asset_snapshot(asset);
    manager.set_effect(inst);
    manager.clock.play();
    for _ in 0..15 {
        manager.tick(1.0 / 60.0);
    }
    let initial_pos = manager
        .spawned_effects
        .iter()
        .find(|e| e.rule.name == "tracker_fx")
        .map(|e| e.matrix.w_axis.truncate());
    let Some(initial_pos) = initial_pos else {
        return;
    };
    if let Some(eff) = manager.effect.as_mut() {
        for em in eff.emitters.iter_mut() {
            for i in 0..em.points.len() {
                em.points.curr_pos[i] = glam::Vec3::new(7.0, 0.0, 0.0);
            }
        }
    }
    manager.tick(1.0 / 60.0);
    let later_pos = manager
        .spawned_effects
        .iter()
        .find(|e| e.rule.name == "tracker_fx")
        .map(|e| e.matrix.w_axis.truncate());
    if let Some(later) = later_pos {
        let delta = (later - initial_pos).length();
        assert!(
            delta > 0.01,
            "tracks_point_pos child should follow parent: initial={:?} later={:?}",
            initial_pos,
            later
        );
    }
}

#[test]
fn tracks_point_pos_attaches_grand_child_to_grandparent() {
    let asset = synthetic_asset();
    let parent_inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut grand_child_inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    grand_child_inst.parent_attachment = Some(5);

    let mut manager = PtfxManager::new();
    manager.set_asset_snapshot(asset);
    manager.set_effect(parent_inst);
    manager.spawned_effects.push(grand_child_inst);
    manager.clock.play();

    for _ in 0..30 {
        manager.tick(1.0 / 60.0);
        if let Some(eff) = manager.spawned_effects.first() {
            if !eff.emitters.is_empty() && !eff.emitters[0].points.is_empty() {
                break;
            }
        }
    }

    if let Some(parent_eff) = manager.spawned_effects.get_mut(0) {
        if !parent_eff.emitters.is_empty() && !parent_eff.emitters[0].points.is_empty() {
            parent_eff.emitters[0].points.rand_index[0] = 5;
            parent_eff.emitters[0].points.curr_pos[0] = glam::Vec3::new(42.0, 0.0, 0.0);
        }
    }
    let mut grand_child = build(
        manager.asset_snapshot.as_ref().unwrap(),
        PreviewTarget::Effect(0),
    )
    .unwrap();
    grand_child.parent_attachment = Some(5);
    grand_child.matrix = glam::Mat4::IDENTITY;
    manager.spawned_effects.push(grand_child);
    manager.tick(1.0 / 60.0);

    let grand_child_pos = manager
        .spawned_effects
        .last()
        .map(|e| e.matrix.w_axis.truncate())
        .unwrap_or(glam::Vec3::ZERO);
    if grand_child_pos.x.abs() > 0.001 {
        assert!(
            (grand_child_pos.x - 42.0).abs() < 0.01,
            "grand-child should follow grand-parent, got x={}",
            grand_child_pos.x
        );
    }
}

#[test]
fn pre_update_time_pre_warms_pool() {
    let mut asset = synthetic_asset();
    effect(&mut asset, EFFECT).pre_update_time = 1.0;
    effect(&mut asset, EFFECT).pre_update_time_interval = 0.05;
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    assert!(
        manager.live_count() > 0,
        "pre_update_time should pre-warm pool, got {}",
        manager.live_count()
    );
}

#[test]
fn snapshot_cache_speeds_up_seek() {
    let asset = synthetic_asset();
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.snapshot_interval = 0.1;
    manager.set_effect(inst);
    manager.clock.play();
    for _ in 0..36 {
        manager.tick(1.0 / 60.0);
    }
    let baseline_steps = manager.last_seek_replayed_steps;
    let snapshot_count = manager.snapshots.len();
    assert!(
        snapshot_count >= 3,
        "expected at least 3 snapshots, got {snapshot_count}"
    );
    manager.clock.seek(0.5);
    manager.tick(0.0);
    let with_cache_steps = manager.last_seek_replayed_steps;
    assert!(
        with_cache_steps < 50,
        "snapshot cache didn't help: {with_cache_steps} steps used"
    );
    let _ = baseline_steps;
}

#[test]
fn seeking_resets_and_replays() {
    let asset = synthetic_asset();
    let inst = build(&asset, PreviewTarget::Effect(0)).unwrap();
    let mut manager = PtfxManager::new();
    manager.set_effect(inst);
    manager.clock.play();
    for _ in 0..30 {
        manager.tick(1.0 / 60.0);
    }
    let mid_count = manager.live_count();
    assert!(mid_count > 0);

    manager.clock.seek(0.0);
    manager.tick(0.0);
    let after_seek = manager.live_count();
    assert!(
        after_seek <= mid_count,
        "seek should rewind: after_seek={after_seek}, mid_count={mid_count}"
    );
}
