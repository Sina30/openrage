//! Every effect of every Gen8 corpus `.ypt` simulates as it simulated when
//! the baseline was recorded: the manager's snapshot after a fixed run of
//! fixed ticks, set up as the app sets up its preview.

#[path = "../../../domain/or-interchange/tests/baseline/mod.rs"]
mod baseline;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use baseline::{Key, Now};
use or_io::gta5::particle::PtxFxList;
use or_simulation::manager::{rules_of, EmitterSnapshot, SnapshotFrame};
use or_simulation::point::{PointBuffer, PointFlags};
use or_simulation::{build, PreviewTarget, PtfxManager};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const KIND: &str = "simulation";
const STEP: f32 = 1.0 / 60.0;
/// The ticks after which a snapshot joins the digest: half a second, one and two.
const SNAPSHOTS_AT: [usize; 3] = [30, 60, 120];

const FLAGS: [PointFlags; 14] = [
    PointFlags::IS_SPRITE,
    PointFlags::IS_MODEL,
    PointFlags::IS_TRAIL,
    PointFlags::COLLIDES,
    PointFlags::HAS_COLLIDED,
    PointFlags::AT_REST,
    PointFlags::SPAWN_EFFECT,
    PointFlags::LOOPED,
    PointFlags::IS_BEING_INITIALISED,
    PointFlags::LIFE_PAUSED,
    PointFlags::LIFE_UNPAUSED,
    PointFlags::FLIP_U,
    PointFlags::FLIP_V,
    PointFlags::CLIPPED_BY_DISTANCE,
];

#[derive(Default)]
struct Stream(Vec<u8>);

impl Stream {
    fn f32s(&mut self, values: &[f32]) {
        for v in values {
            self.0.extend_from_slice(&v.to_bits().to_le_bytes());
        }
    }
    fn bytes(&mut self, values: &[u8]) {
        self.0
            .extend_from_slice(&(values.len() as u64).to_le_bytes());
        self.0.extend_from_slice(values);
    }
    fn index(&mut self, value: Option<usize>) {
        self.0
            .extend_from_slice(&value.map_or(u64::MAX, |v| v as u64).to_le_bytes());
    }

    fn points(&mut self, p: &PointBuffer) {
        self.0.extend_from_slice(&(p.len() as u64).to_le_bytes());
        let flat = |v: &[f32]| v.to_vec();
        self.f32s(
            &p.init_rotation
                .iter()
                .flat_map(|q| q.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(
            &p.coln_pos
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(&flat(&p.coln_speed));
        self.f32s(
            &p.coln_normal
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(&flat(&p.pause_timer));
        self.f32s(&flat(&p.one_over_life));
        self.f32s(&flat(&p.emitter_life_ratio));
        self.f32s(&flat(&p.playback_rate));
        self.f32s(&flat(&p.sort_val));
        for flags in &p.flags {
            let bits = FLAGS
                .iter()
                .enumerate()
                .filter(|(_, f)| flags.contains(**f))
                .fold(0u32, |acc, (i, _)| acc | 1 << i);
            self.0.extend_from_slice(&bits.to_le_bytes());
        }
        self.bytes(&p.coln_mtl_id);
        self.bytes(&p.rand_index);
        for column in [&p.prev_pos, &p.curr_pos, &p.spawn_pos] {
            self.f32s(&column.iter().flat_map(|v| v.to_array()).collect::<Vec<_>>());
        }
        self.f32s(&flat(&p.matrix_weight));
        self.f32s(
            &p.curr_vel
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(&flat(&p.tex_frame_ratio));
        self.f32s(
            &p.dimensions
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(
            &p.curr_rot
                .iter()
                .flat_map(|q| q.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(
            &p.curr_angle
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(
            &p.color
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(&flat(&p.emissive_intensity));
        self.bytes(&p.tex_frame_a);
        self.bytes(&p.tex_frame_b);
        self.bytes(&p.init_tex_frame);
        self.f32s(&flat(&p.alpha_mult));
        self.bytes(&p.is_dead.iter().map(|b| u8::from(*b)).collect::<Vec<_>>());
        self.bytes(
            &p.is_broken_trail
                .iter()
                .map(|b| u8::from(*b))
                .collect::<Vec<_>>(),
        );
        self.f32s(&flat(&p.life_ratio));
        self.f32s(
            &p.uv_offset
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>(),
        );
        self.f32s(&flat(&p.evo_values));
        for column in [
            &p.velocity_noise_extra,
            &p.age_atmos_extra,
            &p.colour_tint_extra,
            &p.collision_bounce_extra,
            &p.attractor_orbit_extra,
        ] {
            self.f32s(&column.iter().flat_map(|v| v.to_array()).collect::<Vec<_>>());
        }
        for link in p.prev_in_chain.iter().chain(&p.next_in_chain) {
            self.index(*link);
        }
        self.index(p.chain_head);
        self.index(p.chain_tail);
    }

    fn emitters(&mut self, emitters: &[EmitterSnapshot]) {
        self.0
            .extend_from_slice(&(emitters.len() as u64).to_le_bytes());
        for e in emitters {
            self.points(&e.points);
            self.f32s(&[e.spawn_accumulator, e.current_life_ratio]);
            self.bytes(&[e.next_rand_index, u8::from(e.has_prev_world_pos)]);
            self.f32s(&e.prev_world_pos.to_array());
            self.0
                .extend_from_slice(&(e.pending_spawns.len() as u64).to_le_bytes());
            for spawn in &e.pending_spawns {
                self.bytes(spawn.effect_rule_name.as_bytes());
                self.f32s(&spawn.matrix.to_cols_array());
                let s = &spawn.scalars;
                self.f32s(&[s.duration_scalar, s.playback_rate_scalar, s.zoom_scalar]);
                self.f32s(&s.colour_tint.to_array());
                self.bytes(&[
                    u8::from(s.inherits_point_life),
                    u8::from(s.tracks_point_pos),
                    spawn.parent_rand_index.unwrap_or(0),
                    u8::from(spawn.parent_rand_index.is_some()),
                ]);
                self.f32s(&[spawn.parent_remaining_life.unwrap_or(f32::NAN)]);
            }
            self.f32s(&e.evo_values);
        }
    }

    fn frame(&mut self, f: &SnapshotFrame) {
        self.f32s(&[f.effect_time_seconds]);
        self.bytes(&[
            f.ptx_random_table_cursor,
            f.root_rgba_rand_index,
            f.root_zoom_scalar_rand_index,
            f.root_trail_tex_info_rand_index,
        ]);
        self.emitters(&f.emitter_states);
        self.0
            .extend_from_slice(&(f.spawned.len() as u64).to_le_bytes());
        for child in &f.spawned {
            self.bytes(child.effect_rule_name.as_bytes());
            self.f32s(&child.matrix.to_cols_array());
            self.f32s(&[child.effect_time_seconds]);
            self.emitters(&child.emitter_states);
            self.bytes(&[
                child.rgba_rand_index,
                child.zoom_scalar_rand_index,
                child.trail_tex_info_rand_index,
            ]);
        }
    }
}

/// The snapshots of one effect's run as the bytes the digest is taken over,
/// or `None` when the effect builds nothing.
fn simulate(asset: &PtxFxList, rules: &Arc<PtxFxList>, effect: usize) -> Option<Vec<u8>> {
    let inst = build(asset, PreviewTarget::Effect(effect))?;
    let mut manager = PtfxManager::new();
    manager.asset_snapshot = Some(Arc::clone(rules));
    manager.set_effect(inst);
    manager.clock.play();
    let mut stream = Stream::default();
    for tick in 1..=SNAPSHOTS_AT[SNAPSHOTS_AT.len() - 1] {
        manager.tick(STEP);
        if SNAPSHOTS_AT.contains(&tick) {
            manager.capture_snapshot();
            let frame = manager.snapshots.last().expect("a snapshot was captured");
            stream.frame(frame);
        }
    }
    Some(stream.0)
}

/// Each effect's stream keyed by its index and name, or why the file would not read.
fn effects(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let asset = PtxFxList::open(path).map_err(|e| e.to_string())?;
    let rules = rules_of(asset.clone());
    Ok(or_particle::list::effect_rules(&asset)
        .enumerate()
        .filter_map(|(index, effect)| {
            let stream = simulate(&asset, &rules, index)?;
            Some((format!("{index} {}", effect.name), stream))
        })
        .collect())
}

fn corpus(what: &str) -> Option<(PathBuf, Vec<PathBuf>)> {
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "ypt")
        .target(TARGET_GEN8)
        .find_or_skip(what)?;
    Some((root, files))
}

#[test]
fn every_corpus_effect_simulates_as_recorded() {
    let what = "every_corpus_effect_simulates_as_recorded";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let mut now: BTreeMap<Key, Now> = BTreeMap::new();
    for path in &files {
        match effects(path) {
            Ok(effects) => {
                for (effect, stream) in effects {
                    now.insert(
                        baseline::key(&root, path, Some(&effect), TARGET_GEN8),
                        Now::Digest(baseline::digest(&stream)),
                    );
                }
            }
            Err(why) => {
                now.insert(
                    baseline::key(&root, path, None, TARGET_GEN8),
                    Now::Refused(why),
                );
            }
        }
    }
    baseline::settle(env!("CARGO_MANIFEST_DIR"), KIND, &now);
}

#[test]
fn a_perturbed_snapshot_names_its_file() {
    let what = "a_perturbed_snapshot_names_its_file";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let path = files
        .iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()))
        .expect("the corpus holds a .ypt");
    let (effect, stream) = effects(path)
        .expect("the smallest corpus .ypt reads")
        .into_iter()
        .next()
        .expect("it holds an effect that builds");
    // The stream opens on the first snapshot's effect time; byte 0 is its lowest mantissa byte.
    baseline::a_perturbed_stream_is_named(
        baseline::key(&root, path, Some(&effect), TARGET_GEN8),
        &stream,
        0,
    );
}
