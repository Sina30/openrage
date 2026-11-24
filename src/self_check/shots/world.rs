//! The `world` steps of a shots script, which make a frame of the GTA VI world one that draws the
//! same again: a camera, an hour, a clean frame, a wait for what streams in, and a dump of what
//! the viewer wants and draws for a check against what the game loads.
//!
//! ⚠ The viewer is locked for a statement at a time and never across a frame, which locks it too.

use std::path::Path;
use std::time::{Duration, Instant};

use super::script::{file_stem, numbers, split_word};
use super::Shell;
use crate::state::gta6_world::worldview::viewer;
use crate::state::AppState;
use crate::ui::dock::Workspace;
use crate::ui::panels::world_viewer::{self, Capture, Pose};

/// How long nothing may arrive or change before the world counts as settled.
const QUIET: Duration = Duration::from_millis(1500);
/// How long `world settle` waits where its line gives no number of seconds.
const SETTLE_DEADLINE: Duration = Duration::from_secs(180);
/// The longest deadline a line may give, in seconds.
const MOST_SECONDS: f32 = 86_400.0;
/// How long `world camera` waits for the install's index: a first build reads every archive of the level.
const INDEX_DEADLINE: Duration = Duration::from_secs(1800);
/// The levels of detail in the order a map numbers them, named as the truth model names them.
const LEVELS: [&str; 7] = ["HD", "LOD", "SLOD1", "SLOD2", "SLOD3", "ORPHANHD", "SLOD4"];
/// The kinds of node a selection's census counts, in its order.
const KINDS: [&str; 4] = ["entity", "prop list", "grass list", "far grass list"];

/// A step that acts on the World screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum WorldStep {
    /// The camera put where a camera line says.
    Camera(Pose),
    /// The hour of the day the world is drawn at, from 0 up to 24.
    Hour(f32),
    /// The view drawn alone over the whole window, or back in the dock.
    Clean(bool),
    /// Frames drawn until nothing arrives any more, for this long at most.
    Settle(Duration),
    /// What the viewer wants and draws at its camera, written as JSON to this file beside the shots.
    Dump(String),
    /// The game modes that stream: bit 0 the base game, bit 1 story, bit 2 the shared minigames.
    Modes(u8),
    /// The camera moved at this velocity, in metres a second, for this long, a frame at a time.
    Fly(glam::Vec3, Duration),
}

/// The words `world modes` takes, each with its bit.
const MODES: [(&str, u8); 3] = [("base", 1), ("story", 2), ("minigames", 4)];

/// The step the words after `world` name.
pub(super) fn parse(rest: &str) -> Result<WorldStep, String> {
    let (word, rest) = split_word(rest);
    let args: Vec<&str> = rest.split_whitespace().collect();
    match word {
        "camera" => Pose::read(rest).map(WorldStep::Camera),
        "hour" => {
            let [hour] = numbers::<1>(&args)?;
            if !(0.0..24.0).contains(&hour) {
                return Err("an hour is from 0 up to 24, and never 24".into());
            }
            Ok(WorldStep::Hour(hour))
        }
        "clean" => match rest {
            "on" => Ok(WorldStep::Clean(true)),
            "off" => Ok(WorldStep::Clean(false)),
            other => Err(format!("no clean `{other}`; use on or off")),
        },
        "settle" if args.is_empty() => Ok(WorldStep::Settle(SETTLE_DEADLINE)),
        "settle" => {
            let [seconds] = numbers::<1>(&args)?;
            if seconds <= 0.0 || seconds > MOST_SECONDS {
                return Err(format!(
                    "a deadline is a number of seconds above 0, {MOST_SECONDS} at most"
                ));
            }
            Ok(WorldStep::Settle(Duration::from_secs_f32(seconds)))
        }
        // ⚠ A name, not a path: a dump lands beside the shots, never in the tree.
        "dump" => file_stem(rest).map(WorldStep::Dump),
        "modes" if !args.is_empty() => (args.iter())
            .try_fold(0, |bits, word| {
                let (_, bit) = (MODES.iter().find(|(name, _)| name == word))
                    .ok_or(format!("no mode `{word}`; use base, story or minigames"))?;
                Ok(bits | bit)
            })
            .map(WorldStep::Modes),
        "fly" => {
            let [x, y, z, seconds] = numbers::<4>(&args)?;
            if seconds <= 0.0 || seconds > MOST_SECONDS {
                return Err(format!(
                    "a flight lasts a number of seconds above 0, {MOST_SECONDS} at most"
                ));
            }
            Ok(WorldStep::Fly(
                glam::Vec3::new(x, y, z),
                Duration::from_secs_f32(seconds),
            ))
        }
        other => Err(format!(
            "no world step `{other}`; use camera, hour, clean, settle, dump, modes or fly"
        )),
    }
}

/// What one frame finds of the world viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Found {
    /// An index of the install is held.
    pub indexed: bool,
    /// The maps, models, textures and maps' lights asked for and not there yet.
    pub waiting: [usize; 4],
    /// What stands streamed in: a change in it is something that came or went.
    pub standing: [usize; 4],
}

/// The rule `world settle` waits by: an index held, nothing waited for, and nothing changed for [`QUIET`].
// ⚠ Nothing waited for is not enough: for a frame or two after the camera jumps, the world's
// thread has not yet asked for the maps of the new place.
#[derive(Debug, Default)]
pub(super) struct Settle {
    last: Option<Found>,
    since: Duration,
}

impl Settle {
    /// Whether the world has settled at `now`, a time on a clock that only runs forward, given what this frame found.
    pub(super) fn settled(&mut self, now: Duration, found: Found) -> bool {
        let quiet = found.indexed && found.waiting == [0; 4];
        if !quiet || self.last != Some(found) {
            (self.last, self.since) = (Some(found), now);
            return false;
        }
        now.saturating_sub(self.since) >= QUIET
    }

    /// What was still arriving when the wait gave up, for the line that fails the run.
    pub(super) fn still(&self) -> String {
        match self.last {
            None => "no frame was drawn".into(),
            Some(found) if !found.indexed => "no index of the install is held".into(),
            Some(Found {
                waiting: [0, 0, 0, 0], ..
            }) => "what is streamed in was still changing".into(),
            Some(Found {
                waiting: [maps, models, textures, lights],
                ..
            }) => format!(
                "{maps} maps, {models} models, {textures} textures and {lights} maps' lights were still waited for"
            ),
        }
    }
}

/// What the viewer holds and waits for now, or why it has no index.
fn found() -> Result<Found, String> {
    let viewer = viewer();
    if let Some(why) = &viewer.failed {
        return Err(format!("the GTA VI world was not indexed: {why}"));
    }
    let streaming = viewer.streaming();
    Ok(Found {
        indexed: viewer.index.is_some(),
        waiting: [
            streaming.maps_waiting,
            viewer.models_waiting,
            viewer.textures_waiting,
            viewer.lights.as_ref().map_or(0, |lights| lights.waiting()),
        ],
        standing: [
            streaming.maps_wanted,
            streaming.maps_loaded,
            streaming.nodes,
            streaming.drawn,
        ],
    })
}

/// A census's counts as `"KIND LEVEL": count`, leaving out what it counts none of.
fn by_level(counts: &[[usize; 7]; 4]) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for (kind, row) in KINDS.iter().zip(counts) {
        for (level, n) in LEVELS.iter().zip(row) {
            if *n > 0 {
                out.insert(format!("{kind} {level}"), (*n).into());
            }
        }
    }
    out
}

/// 32-bit names as eight hexadecimal digits, sorted.
fn hex(names: &[u32]) -> Vec<String> {
    let mut out: Vec<String> = names.iter().map(|n| format!("{n:08X}")).collect();
    out.sort_unstable();
    out
}

/// Write what the viewer wants and draws at its camera to `path` as JSON.
fn dump(path: &Path) -> Result<(), String> {
    let mut viewer = viewer();
    let selected = (viewer.selected(&mut Vec::new()))
        .ok_or("no world streams yet: put `world settle` before `world dump`")?;
    let pose = Pose::of(&viewer.camera);
    let (census, s) = (&selected.census, selected.streaming);
    let records: usize = selected.cards.iter().map(|list| list.records.len()).sum();
    // ⚠ A node's fade is written as `MAP:INDEX:FADE`, so a fade in time can be judged against the distance's.
    let mut drawn_nodes: Vec<String> = (census.drawn_nodes.iter())
        .map(|(map, at, fade)| format!("{map:08X}:{at}:{fade:.3}"))
        .collect();
    drawn_nodes.sort_unstable();
    let json = serde_json::json!({
        "camera": {
            "pos": [pose.position.x, pose.position.y, pose.position.z],
            "yaw": pose.yaw,
            "pitch": pose.pitch,
            "fov": pose.fov,
        },
        "hour": viewer.hour,
        "lod_scale": viewer.lod_scale,
        "modes": viewer.enabled.modes,
        "groups_on": hex(&viewer.enabled.groups),
        "streaming": {
            "maps_wanted": s.maps_wanted,
            "maps_loaded": s.maps_loaded,
            "maps_waiting": s.maps_waiting,
            "nodes": s.nodes,
            "drawn": s.drawn,
        },
        "wanted_map_names": hex(&census.wanted),
        "loaded_map_names": hex(&census.loaded),
        "nodes_by_level": by_level(&census.nodes),
        "drawn_by_level": by_level(&census.drawn),
        "list_instances": census.list_instances,
        "drawn_nodes": drawn_nodes,
        "cards": { "lists": selected.cards.len(), "records": records },
    });
    drop(viewer);
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    println!("dump  {}", path.display());
    Ok(())
}

impl Shell {
    /// Run a `world` step on the World screen, which the script has put up before it; a dump goes into `out`.
    pub(super) fn world(
        &mut self,
        state: &mut AppState,
        step: &WorldStep,
        out: &Path,
    ) -> Result<(), String> {
        // Every step ends on drawn frames, so with the World screen up its viewer has opened the install.
        if state.active_workspace != Workspace::World || !world_viewer::offered(state) {
            return Err(
                "the World screen is not drawn: put `game gta6 FOLDER` and `workspace World` first"
                    .into(),
            );
        }
        match step {
            WorldStep::Camera(pose) => {
                // ⚠ The viewer puts its camera over the middle of the maps as it takes the index, so a camera is set after that.
                self.indexed(state)?;
                pose.put(&mut viewer().camera);
            }
            WorldStep::Hour(hour) => {
                // What is placed for some hours alone follows the viewer's hour and the light follows the day's: a frame has one.
                viewer().hour = *hour;
                let day = &mut state.drivers.time_of_day;
                day.set_hour(*hour);
                // ⚠ A day that runs, or days gone by, which the moon and the clouds follow, would draw no two runs alike.
                (day.playing, day.days) = (false, 0.0);
            }
            WorldStep::Clean(on) => state.panels.get::<Capture>().clean = *on,
            WorldStep::Settle(deadline) => self.settle(state, *deadline)?,
            WorldStep::Dump(name) => dump(&out.join(name))?,
            // The groups follow the modes, as they do on the screen until a group is ticked.
            WorldStep::Modes(modes) => viewer().enabled.modes = *modes,
            WorldStep::Fly(velocity, seconds) => self.fly(state, *velocity, *seconds)?,
        }
        Ok(())
    }

    /// Draw frames for `seconds`, the camera moved at `velocity` by the time each is drawn.
    fn fly(
        &mut self,
        state: &mut AppState,
        velocity: glam::Vec3,
        seconds: Duration,
    ) -> Result<(), String> {
        let (started, from) = (Instant::now(), viewer().camera.position);
        // ⚠ The last frame is not cut short at `seconds`: a short last step would read as a camera
        // slowing to a stop, and a dump after the flight is to see it still flying.
        loop {
            let flown = started.elapsed();
            viewer().camera.position = from + velocity * flown.as_secs_f32();
            self.idle(state, 1)?;
            if flown >= seconds {
                return Ok(());
            }
        }
    }

    /// Draw frames until the install's index is held.
    fn indexed(&mut self, state: &mut AppState) -> Result<(), String> {
        let until = Instant::now() + INDEX_DEADLINE;
        while !found()?.indexed {
            if Instant::now() > until {
                return Err(format!(
                    "the GTA VI world was still being indexed after {} minutes",
                    INDEX_DEADLINE.as_secs() / 60
                ));
            }
            self.idle(state, 1)?;
            std::thread::sleep(Duration::from_millis(30));
        }
        Ok(())
    }

    /// Draw frames until the world has settled, or fail at `deadline` by what was still arriving.
    fn settle(&mut self, state: &mut AppState, deadline: Duration) -> Result<(), String> {
        let started = Instant::now();
        let mut rule = Settle::default();
        loop {
            self.idle(state, 1)?;
            let now = started.elapsed();
            if rule.settled(now, found()?) {
                println!("  settled after {:.1} s", now.as_secs_f32());
                return Ok(());
            }
            if now > deadline {
                return Err(format!(
                    "the world had not settled after {:.1} s: {}",
                    deadline.as_secs_f32(),
                    rule.still()
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: Found = Found {
        indexed: true,
        waiting: [0; 4],
        standing: [40, 40, 900, 700],
    };

    fn at(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn the_steps_of_the_world_are_read() {
        assert_eq!(
            parse("camera -120.5 340 62.25 135 -20 60"),
            Ok(WorldStep::Camera(Pose {
                position: glam::Vec3::new(-120.5, 340.0, 62.25),
                yaw: 135.0,
                pitch: -20.0,
                fov: Some(60.0),
            }))
        );
        assert_eq!(parse("hour 17.5"), Ok(WorldStep::Hour(17.5)));
        assert_eq!(parse("hour 0"), Ok(WorldStep::Hour(0.0)));
        assert_eq!(parse("clean on"), Ok(WorldStep::Clean(true)));
        assert_eq!(parse("clean off"), Ok(WorldStep::Clean(false)));
        assert_eq!(parse("settle"), Ok(WorldStep::Settle(SETTLE_DEADLINE)));
        assert_eq!(parse("settle 2.5"), Ok(WorldStep::Settle(at(2500))));
        assert_eq!(
            parse("dump truth-01.json"),
            Ok(WorldStep::Dump("truth-01.json".into()))
        );
        assert_eq!(parse("modes base story"), Ok(WorldStep::Modes(3)));
        assert_eq!(parse("modes minigames"), Ok(WorldStep::Modes(4)));
        assert_eq!(
            parse("fly 30 0 -1.5 2"),
            Ok(WorldStep::Fly(glam::Vec3::new(30.0, 0.0, -1.5), at(2000)))
        );
    }

    #[test]
    fn a_census_is_written_by_kind_and_level_and_names_as_sorted_hexadecimal() {
        let mut counts = [[0; 7]; 4];
        (counts[0][0], counts[2][5], counts[3][2]) = (12, 3, 1);
        let written = by_level(&counts);
        assert_eq!(written.len(), 3, "a count of none is left out: {written:?}");
        assert_eq!(written["entity HD"], 12);
        assert_eq!(written["grass list ORPHANHD"], 3);
        assert_eq!(written["far grass list SLOD1"], 1);
        assert_eq!(
            hex(&[0xABC, 0x1, 0xFFFF_FFFF]),
            ["00000001", "00000ABC", "FFFFFFFF"]
        );
    }

    #[test]
    fn a_wrong_world_step_is_refused() {
        for bad in [
            "",
            "fly",
            "camera",
            "camera 1 2 3",
            "camera 1 2 3 0 95",
            "hour",
            "hour 24",
            "hour -1",
            "hour noon",
            "clean",
            "clean yes",
            "settle 0",
            "settle -3",
            "settle soon",
            "settle 1 2",
            "settle 1e30",
            "dump",
            "dump ../out.json",
            "dump C:/out.json",
            "dump two words",
            "modes",
            "modes base online",
            "fly 1 2 3",
            "fly 1 2 3 0",
            "fly 1 2 3 soon",
        ] {
            assert!(parse(bad).is_err(), "`world {bad}` should be refused");
        }
    }

    #[test]
    fn the_world_settles_once_nothing_is_waited_for_and_nothing_has_changed_for_the_quiet_time() {
        let mut rule = Settle::default();
        assert!(!rule.settled(at(0), IDLE), "one frame is no quiet time");
        assert!(!rule.settled(QUIET - at(1), IDLE));
        assert!(rule.settled(QUIET, IDLE));
    }

    #[test]
    fn something_waited_for_or_changed_starts_the_quiet_time_again() {
        let waiting = Found {
            waiting: [0, 3, 0, 0],
            ..IDLE
        };
        let grown = Found {
            standing: [40, 40, 900, 701],
            ..IDLE
        };
        let mut rule = Settle::default();
        assert!(!rule.settled(at(0), IDLE));
        // A model asked for after a quiet stretch: the camera jumped and the selection caught up.
        assert!(!rule.settled(QUIET - at(1), waiting));
        assert!(
            !rule.settled(QUIET * 3, waiting),
            "never while something is waited for"
        );
        assert!(!rule.settled(QUIET * 3 + at(1), IDLE));
        assert!(
            !rule.settled(QUIET * 4, IDLE),
            "the quiet time runs from the last change, not from the first frame"
        );
        // Nothing is waited for, yet what stands streamed in moved: a parent gave way to its children.
        assert!(!rule.settled(QUIET * 4 + at(1), grown));
        assert!(!rule.settled(QUIET * 5, grown));
        assert!(rule.settled(QUIET * 5 + at(1), grown));
    }

    #[test]
    fn a_world_with_no_index_never_settles_and_the_wait_says_what_was_still_arriving() {
        let unindexed = Found {
            indexed: false,
            ..Found::default()
        };
        let mut rule = Settle::default();
        assert_eq!(rule.still(), "no frame was drawn");
        assert!(!rule.settled(at(0), unindexed));
        assert!(
            !rule.settled(QUIET * 10, unindexed),
            "no index is nothing to shoot"
        );
        assert_eq!(rule.still(), "no index of the install is held");
        let waiting = Found {
            waiting: [2, 0, 7, 0],
            ..IDLE
        };
        assert!(!rule.settled(QUIET * 11, waiting));
        assert_eq!(
            rule.still(),
            "2 maps, 0 models, 7 textures and 0 maps' lights were still waited for"
        );
        assert!(!rule.settled(QUIET * 12, IDLE));
        assert_eq!(rule.still(), "what is streamed in was still changing");
    }
}
