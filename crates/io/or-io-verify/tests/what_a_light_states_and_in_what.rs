//! What an owned `CLightAttr` states and in what unit, over the staged
//! drawables, drawable dictionaries and fragments of both targets.
//!
//! A round trip is blind to a unit, because the bytes are the same whatever
//! the number means.
//!
//! ⚠ **The cone angle is the one worth settling.** Every other angle this
//! project handles is radians, and a spot cone read as radians when it is
//! degrees is a cone about a fifty-seventh as wide.

use std::sync::OnceLock;

use or_io::gta5::drawable::CLightAttr;
use or_io::Asset;
use or_io_verify::gates::{self, name};

const SPOT: u8 = 2;

/// Every staged light, or `None` with the skip recorded.
///
/// ⚠ One opened file at a time: a fragment holds every drawable and texture it
/// reaches, and only its lights, 0xA8 bytes each, are kept.
fn lights() -> Option<&'static [CLightAttr]> {
    static LIGHTS: OnceLock<Option<Vec<CLightAttr>>> = OnceLock::new();
    LIGHTS
        .get_or_init(|| {
            let mut out = Vec::new();
            let mut staged = false;
            for ext in ["ydr", "ydd", "yft"] {
                let Some(paths) = gates::files(ext, None, "what_a_light_states_and_in_what") else {
                    continue;
                };
                staged = true;
                for path in paths {
                    let asset =
                        or_io::open(&path).unwrap_or_else(|e| panic!("{} opens: {e}", name(&path)));
                    match asset {
                        Asset::Drawable(d) => out.extend(d.lights),
                        Asset::DrawableGen9(d) => out.extend(d.lights),
                        Asset::DrawableDict(d) => {
                            out.extend(d.entries.into_iter().flatten().flat_map(|d| d.lights))
                        }
                        Asset::DrawableDictGen9(d) => {
                            out.extend(d.entries.into_iter().flatten().flat_map(|d| d.lights))
                        }
                        Asset::Fragment(f) => out.extend(f.lights),
                        Asset::FragmentGen9(f) => out.extend(f.lights),
                        other => panic!("{} opened as .{}", name(&path), other.ext()),
                    }
                }
            }
            staged.then_some(out)
        })
        .as_deref()
}

/// What a set of lights states about its cones.
#[derive(Debug, Default)]
struct Cones {
    lights: usize,
    unknown_type: usize,
    spots: usize,
    past_half_pi: usize,
    inside_ninety: usize,
    inner_inside_outer: usize,
    widest: f32,
}

fn cones(lights: impl IntoIterator<Item = (u8, f32, f32)>) -> Cones {
    let mut c = Cones::default();
    for (light_type, inner, outer) in lights {
        c.lights += 1;
        if ![1, SPOT, 4].contains(&light_type) {
            c.unknown_type += 1;
        }
        if light_type != SPOT {
            continue;
        }
        c.spots += 1;
        c.widest = c.widest.max(outer);
        if outer > std::f32::consts::FRAC_PI_2 {
            c.past_half_pi += 1;
        }
        if (0.0..=90.0).contains(&outer) && (0.0..=90.0).contains(&inner) {
            c.inside_ninety += 1;
        }
        if inner <= outer {
            c.inner_inside_outer += 1;
        }
    }
    c
}

/// A half-angle in radians cannot exceed π/2, and a degree half-angle is
/// inside 0..=90: both are counted, so the answer is the gap between them.
fn stated_in_degrees(c: &Cones) -> Result<(), String> {
    if c.unknown_type != 0 {
        return Err(format!(
            "{} light(s) state a `light_type` that is not 1 (point), 2 (spot) or 4 (capsule)",
            c.unknown_type
        ));
    }
    if c.past_half_pi == 0 {
        return Err(format!(
            "not one of {} spot light(s) states an outer half-angle above π/2, so nothing \
             tells degrees from radians",
            c.spots
        ));
    }
    if c.inside_ninety != c.spots {
        return Err(format!(
            "{} of {} spot light(s) state a half-angle outside 0..=90",
            c.spots - c.inside_ninety,
            c.spots
        ));
    }
    if c.inner_inside_outer != c.spots {
        return Err(format!(
            "{} of {} spot light(s) have an inner cone outside the outer one",
            c.spots - c.inner_inside_outer,
            c.spots
        ));
    }
    Ok(())
}

fn staged_spots(what: &str) -> Option<&'static [CLightAttr]> {
    let lights = lights()?;
    if !lights.iter().any(|l| l.light_type == SPOT) {
        or_test_support::skip_corpus(what, "no staged drawable or fragment carries a spot light");
        return None;
    }
    Some(lights)
}

#[test]
fn a_spot_lights_cone_is_stated_in_degrees() {
    let Some(lights) = staged_spots("a_spot_lights_cone_is_stated_in_degrees") else {
        return;
    };
    let c = cones(
        lights
            .iter()
            .map(|l| (l.light_type, l.cone_inner_angle, l.cone_outer_angle)),
    );
    println!("{c:?}");
    stated_in_degrees(&c).unwrap_or_else(|why| panic!("{why}: {c:?}"));
}

/// The control: the same lights with their cones fed in radians are refused.
#[test]
fn a_cone_fed_in_radians_is_refused() {
    let Some(lights) = staged_spots("a_cone_fed_in_radians_is_refused") else {
        return;
    };
    let c = cones(lights.iter().map(|l| {
        (
            l.light_type,
            l.cone_inner_angle.to_radians(),
            l.cone_outer_angle.to_radians(),
        )
    }));
    assert!(stated_in_degrees(&c).is_err(), "{c:?}");
}
