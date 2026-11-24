//! The air of GTA VI's day as a single-scattering atmosphere whose profile its cycles state: what
//! it lets through towards the sun and the moon, and the sky it lights the ground and the dome with.

use std::f64::consts::PI;

use or_io::gta5::data::timecycle::Keyframe;

/// How high the camera stands above the ground, in kilometres.
const CAMERA: f64 = 0.002;
/// The coefficients, per kilometre, a cycle's scattering colours and intensities are relative to:
/// Rayleigh, Mie scattering, Mie absorption and ozone, each the published Earth value over the colour it is given at.
// ⚠ Inferred: the cycles state that model's profile constants exactly, and its colours in these ratios.
const RAYLEIGH: f64 = 0.0331 / 0.9131;
const MIE_SCATTERING: f64 = 0.003996 / 0.57735;
const MIE_ABSORPTION: f64 = 0.000444 / 0.57735;
const OZONE: f64 = 0.001881 / 0.9434;
/// The steps a ray is integrated in: towards the sun from the camera, along a view, and towards the light from a point of a view.
const TRANSMITTANCE_STEPS: u32 = 40;
const VIEW_STEPS: u32 = 24;
const LIGHT_STEPS: u32 = 12;
/// The rings of height and the turns of the compass the sky's irradiance is summed over.
const RINGS: u32 = 8;
const TURNS: u32 = 12;

/// The atmosphere of one moment, in kilometres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atmosphere {
    ground: f64,
    top: f64,
    rayleigh: [f64; 3],
    mie_scattering: [f64; 3],
    mie_absorption: [f64; 3],
    ozone: [f64; 3],
    rayleigh_height: f64,
    mie_height: f64,
    mie_g: f64,
    ozone_width: f64,
    ozone_below: [f64; 2],
    ozone_above: [f64; 2],
}

/// The Rayleigh phase function at the cosine `c` of the angle between the view and the light.
fn rayleigh_phase(c: f64) -> f64 {
    3.0 / (16.0 * PI) * (1.0 + c * c)
}

/// The Cornette-Shanks phase function of eccentricity `g` at the cosine `c`.
fn mie_phase(c: f64, g: f64) -> f64 {
    let g2 = g * g;
    3.0 / (8.0 * PI) * ((1.0 - g2) * (1.0 + c * c))
        / ((2.0 + g2) * (1.0 + g2 - 2.0 * g * c).max(1e-12).powf(1.5))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(v: [f32; 3]) -> [f64; 3] {
    let v = v.map(f64::from);
    let length = dot(v, v).sqrt();
    match length > 0.0 {
        true => v.map(|c| c / length),
        false => [0.0, 0.0, 1.0],
    }
}

impl Atmosphere {
    /// The atmosphere keyframe `k` states by its `sky_new_*` variables; what it does not state is the published Earth default.
    pub fn of(k: &Keyframe) -> Self {
        let get = |name: &str, default: f32| f64::from(k.get_or(name, default));
        let rgb = |stem: &str| ["r", "g", "b"].map(|c| get(&format!("{stem}_{c}"), 1.0));
        let coefficient = |stem: &str, intensity: &str, per_km: f64| {
            let scale = get(intensity, 1.0) * per_km;
            rgb(stem).map(|c| c * scale)
        };
        let ground = get("sky_new_planet_radius", 6360.0);
        Self {
            ground,
            top: ground + get("sky_new_karman_line", 100.0),
            rayleigh: coefficient(
                "sky_new_ray_scattering_col",
                "sky_new_ray_scattering_intensity",
                RAYLEIGH,
            ),
            mie_scattering: coefficient(
                "sky_new_mie_scattering_col",
                "sky_new_mie_scattering_intensity",
                MIE_SCATTERING,
            ),
            mie_absorption: coefficient(
                "sky_new_mie_absorption_col",
                "sky_new_mie_absorption_intensity",
                MIE_ABSORPTION,
            ),
            ozone: coefficient(
                "sky_new_ozo_extinction_col",
                "sky_new_ozo_extinction_intensity",
                OZONE,
            ),
            rayleigh_height: get("sky_new_ray_density_scale", 8.0),
            mie_height: get("sky_new_mie_density_scale", 1.2),
            mie_g: get("sky_new_mie_phase_eccentricity", 0.8),
            ozone_width: get("sky_new_ozo_0_width", 25.0),
            ozone_below: [
                get("sky_new_ozo_0_constant", -2.0 / 3.0),
                get("sky_new_ozo_0_linear", 1.0 / 15.0),
            ],
            ozone_above: [
                get("sky_new_ozo_1_constant", 8.0 / 3.0),
                get("sky_new_ozo_1_linear", -1.0 / 15.0),
            ],
        }
    }

    /// The densities of Rayleigh and Mie particles and of ozone at `height` kilometres above the ground.
    fn densities(&self, height: f64) -> [f64; 3] {
        let [constant, linear] = match height < self.ozone_width {
            true => self.ozone_below,
            false => self.ozone_above,
        };
        [
            (-height / self.rayleigh_height).exp(),
            (-height / self.mie_height).exp(),
            (constant + linear * height).clamp(0.0, 1.0),
        ]
    }

    /// What the air takes out of light per kilometre at `height`.
    fn extinction(&self, height: f64) -> [f64; 3] {
        let [rayleigh, mie, ozone] = self.densities(height);
        [0, 1, 2].map(|c| {
            self.rayleigh[c] * rayleigh
                + (self.mie_scattering[c] + self.mie_absorption[c]) * mie
                + self.ozone[c] * ozone
        })
    }

    /// How far a ray from radius `r` at cos-zenith `mu` runs to the top.
    fn to_top(&self, r: f64, mu: f64) -> f64 {
        -r * mu
            + (r * r * (mu * mu - 1.0) + self.top * self.top)
                .max(0.0)
                .sqrt()
    }

    fn hits_ground(&self, r: f64, mu: f64) -> bool {
        mu < 0.0 && r * r * (mu * mu - 1.0) + self.ground * self.ground >= 0.0
    }

    /// What the air lets through from radius `r` along cos-zenith `mu` to the top, in `steps` midpoint steps; nothing through the ground.
    fn transmittance_from(&self, r: f64, mu: f64, steps: u32) -> [f64; 3] {
        if self.hits_ground(r, mu) {
            return [0.0; 3];
        }
        let step = self.to_top(r, mu) / f64::from(steps);
        let mut depth = [0.0; 3];
        for i in 0..steps {
            let t = (f64::from(i) + 0.5) * step;
            let height = (r * r + t * t + 2.0 * r * t * mu).sqrt() - self.ground;
            let e = self.extinction(height);
            depth = [0, 1, 2].map(|c| depth[c] + e[c] * step);
        }
        depth.map(|d| (-d).exp())
    }

    /// What the air lets through from the camera towards a body whose way has the up part `mu`.
    pub fn transmittance(&self, mu: f32) -> [f32; 3] {
        let through = self.transmittance_from(
            self.ground + CAMERA,
            f64::from(mu).clamp(-1.0, 1.0),
            TRANSMITTANCE_STEPS,
        );
        through.map(|t| t as f32)
    }

    /// The sky's single-scattered radiance seen from the camera along `view`, lit by a body towards `light` of `illuminance` lux.
    pub fn radiance(&self, view: [f32; 3], light: [f32; 3], illuminance: f32) -> [f32; 3] {
        let seen = self.radiance_in(unit(view), unit(light), VIEW_STEPS);
        seen.map(|c| (c * f64::from(illuminance)) as f32)
    }

    /// The radiance along `view` for a light of one lux, in `steps` steps.
    fn radiance_in(&self, view: [f64; 3], light: [f64; 3], steps: u32) -> [f64; 3] {
        let r0 = self.ground + CAMERA;
        if self.hits_ground(r0, view[2]) {
            return [0.0; 3];
        }
        let step = self.to_top(r0, view[2]) / f64::from(steps);
        let cosine = dot(view, light);
        let (phase_r, phase_m) = (rayleigh_phase(cosine), mie_phase(cosine, self.mie_g));
        let mut depth = [0.0; 3];
        let mut out = [0.0; 3];
        for i in 0..steps {
            let t = (f64::from(i) + 0.5) * step;
            let point = [view[0] * t, view[1] * t, r0 + view[2] * t];
            let r = dot(point, point).sqrt();
            let height = r - self.ground;
            let e = self.extinction(height);
            let towards_light = self.transmittance_from(r, dot(point, light) / r, LIGHT_STEPS);
            let [rayleigh, mie, _] = self.densities(height);
            out = [0, 1, 2].map(|c| {
                let scattering =
                    self.rayleigh[c] * rayleigh * phase_r + self.mie_scattering[c] * mie * phase_m;
                // The depth to the middle of this step: the steps before it and half of its own.
                let to_middle = depth[c] + e[c] * step * 0.5;
                out[c] + (-to_middle).exp() * towards_light[c] * scattering * step
            });
            depth = [0, 1, 2].map(|c| depth[c] + e[c] * step);
        }
        out
    }

    /// The sky's irradiance on level ground, in lux, lit by a body towards `light` of `illuminance` lux.
    pub fn irradiance(&self, light: [f32; 3], illuminance: f32) -> [f32; 3] {
        let light = unit(light);
        let mut out = [0.0; 3];
        let solid = (1.0 / f64::from(RINGS)) * (2.0 * PI / f64::from(TURNS));
        for ring in 0..RINGS {
            let up = (f64::from(ring) + 0.5) / f64::from(RINGS);
            let across = (1.0 - up * up).sqrt();
            for turn in 0..TURNS {
                let angle = 2.0 * PI * (f64::from(turn) + 0.5) / f64::from(TURNS);
                let view = [across * angle.cos(), across * angle.sin(), up];
                let seen = self.radiance_in(view, light, LIGHT_STEPS);
                out = [0, 1, 2].map(|c| out[c] + seen[c] * up * solid);
            }
        }
        out.map(|c| (c * f64::from(illuminance)) as f32)
    }
}

#[cfg(test)]
#[path = "gta6_sky_tests.rs"]
mod tests;
