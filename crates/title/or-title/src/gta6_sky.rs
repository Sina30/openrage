//! The air of GTA VI's day as a single-scattering atmosphere whose profile its cycles state: what
//! it lets through towards the sun and the moon, and the sky it lights the ground and the dome with.

use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

use or_io::gta5::data::timecycle::Keyframe;
use or_io::gta6::timecycle::GROUND_ALBEDO;

/// How high the camera stands above the ground, in kilometres.
const CAMERA: f64 = 0.002;
/// The coefficients, per kilometre, a cycle's scattering colours and intensities are relative to:
/// Rayleigh, Mie scattering, Mie absorption and ozone, each the published Earth value over the colour it is given at.
// ⚠ Inferred: the cycles state that model's profile constants exactly, and its colours in these ratios.
const RAYLEIGH: f64 = 0.0331 / 0.9131;
const MIE_SCATTERING: f64 = 0.003996 / 0.57735;
const MIE_ABSORPTION: f64 = 0.000444 / 0.57735;
const OZONE: f64 = 0.001881 / 0.9434;
/// The published Earth coefficients per kilometre, red, green and blue: what a cycle that states no colour gets.
const EARTH_RAYLEIGH: [f64; 3] = [0.005802, 0.013558, 0.0331];
const EARTH_MIE_SCATTERING: [f64; 3] = [0.003996; 3];
const EARTH_MIE_ABSORPTION: [f64; 3] = [0.000444; 3];
const EARTH_OZONE: [f64; 3] = [0.000650, 0.001881, 0.000085];
/// The steps a ray is integrated in: towards the sun from the camera, along a view, and towards the light from a point of a view.
const TRANSMITTANCE_STEPS: u32 = 40;
const VIEW_STEPS: u32 = 24;
const LIGHT_STEPS: u32 = 12;
/// The rings of height and the turns of the compass the sky's irradiance is summed over.
const RINGS: u32 = 8;
const TURNS: u32 = 12;
/// The multiple scattering table: its heights and its cosines of the light's zenith angle, the directions of the sphere each entry gathers from (a side of the grid), and the steps along each.
const TABLE_HEIGHTS: usize = 16;
const TABLE_ANGLES: usize = 16;
const SPHERE_SIDE: u32 = 8;
const GATHER_STEPS: u32 = 20;
const GATHER_LIGHT_STEPS: u32 = 8;

/// The light scattered more than once at each height and angle of the light, per lux and per unit of scattering, which a point of a view adds times what it scatters.
#[derive(Debug, Clone, PartialEq)]
struct Multiple {
    of: Atmosphere,
    table: Vec<[f64; 3]>,
}

// ponytail: one table held, worked out again (about a tenth of a second) when the air changes.
static MULTIPLE: Mutex<Option<Arc<Multiple>>> = Mutex::new(None);

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
    /// How much of the light scattered more than once the sky takes.
    multiple: f64,
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
        // An unstated colour is Earth's over `per_km`, so that colour times `per_km` is the published value.
        let coefficient = |stem: &str, intensity: &str, per_km: f64, earth: [f64; 3]| {
            let scale = get(intensity, 1.0) * per_km;
            [0, 1, 2].map(|c| {
                let name = format!("{stem}_{}", ["r", "g", "b"][c]);
                k.get(&name).map_or(earth[c] / per_km, f64::from) * scale
            })
        };
        let ground = get("sky_new_planet_radius", 6360.0);
        Self {
            ground,
            top: ground + get("sky_new_karman_line", 100.0),
            rayleigh: coefficient(
                "sky_new_ray_scattering_col",
                "sky_new_ray_scattering_intensity",
                RAYLEIGH,
                EARTH_RAYLEIGH,
            ),
            mie_scattering: coefficient(
                "sky_new_mie_scattering_col",
                "sky_new_mie_scattering_intensity",
                MIE_SCATTERING,
                EARTH_MIE_SCATTERING,
            ),
            mie_absorption: coefficient(
                "sky_new_mie_absorption_col",
                "sky_new_mie_absorption_intensity",
                MIE_ABSORPTION,
                EARTH_MIE_ABSORPTION,
            ),
            ozone: coefficient(
                "sky_new_ozo_extinction_col",
                "sky_new_ozo_extinction_intensity",
                OZONE,
                EARTH_OZONE,
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
            multiple: get("sky_new_intensity_multiscattering", 1.0),
        }
    }

    /// What the air scatters per kilometre at `height`, of all its particles.
    fn scattering(&self, height: f64) -> [f64; 3] {
        let [rayleigh, mie, _] = self.densities(height);
        [0, 1, 2].map(|c| self.rayleigh[c] * rayleigh + self.mie_scattering[c] * mie)
    }

    /// The table of light scattered more than once in this air, held while the air stays the same.
    fn multiple_table(&self) -> Arc<Multiple> {
        let mut held = MULTIPLE.lock().unwrap_or_else(|p| p.into_inner());
        match held.as_ref().filter(|m| m.of == *self) {
            Some(table) => Arc::clone(table),
            None => {
                let table = Arc::new(Multiple {
                    of: *self,
                    table: self.gather_multiple(),
                });
                *held = Some(Arc::clone(&table));
                table
            }
        }
    }

    /// The height, in kilometres, of row `i` of the multiple scattering table: closer together near the ground.
    fn table_height(&self, i: f64) -> f64 {
        let along = i / (TABLE_HEIGHTS - 1) as f64;
        along * along * (self.top - self.ground)
    }

    /// Every entry of the multiple scattering table: from each height and light angle, the light that has scattered once gathered from the whole sphere and the ground with an even phase, made the sum of every further order by one over one less the share the sphere gives back.
    fn gather_multiple(&self) -> Vec<[f64; 3]> {
        let mut table = Vec::with_capacity(TABLE_HEIGHTS * TABLE_ANGLES);
        let directions = f64::from(SPHERE_SIDE * SPHERE_SIDE);
        for row in 0..TABLE_HEIGHTS {
            let r = self.ground + self.table_height(row as f64).max(CAMERA);
            for column in 0..TABLE_ANGLES {
                let mu_light = -1.0 + 2.0 * column as f64 / (TABLE_ANGLES - 1) as f64;
                let light = [(1.0 - mu_light * mu_light).max(0.0).sqrt(), 0.0, mu_light];
                let (mut light_sum, mut share) = ([0.0; 3], [0.0; 3]);
                for i in 0..SPHERE_SIDE {
                    for j in 0..SPHERE_SIDE {
                        let around = 2.0 * PI * (f64::from(i) + 0.5) / f64::from(SPHERE_SIDE);
                        let up = 1.0 - 2.0 * (f64::from(j) + 0.5) / f64::from(SPHERE_SIDE);
                        let across = (1.0 - up * up).sqrt();
                        let view = [across * around.cos(), across * around.sin(), up];
                        let (l, f) = self.gather_along(r, view, light);
                        light_sum = [0, 1, 2].map(|c| light_sum[c] + l[c] / directions);
                        share = [0, 1, 2].map(|c| share[c] + f[c] / directions);
                    }
                }
                table.push([0, 1, 2].map(|c| light_sum[c] / (1.0 - share[c]).max(1e-3)));
            }
        }
        table
    }

    /// Along `view` from radius `r` with the light towards `light`: the light scattered once towards the point with an even phase, and the share of an even light the way gives back.
    fn gather_along(&self, r: f64, view: [f64; 3], light: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let even = 1.0 / (4.0 * PI);
        let ground = self.hits_ground(r, view[2]);
        let length = match ground {
            true => {
                -r * view[2]
                    - (r * r * (view[2] * view[2] - 1.0) + self.ground * self.ground)
                        .max(0.0)
                        .sqrt()
            }
            false => self.distance_to_top(r, view[2]),
        };
        let step = length / f64::from(GATHER_STEPS);
        let (mut through, mut seen, mut share) = ([1.0; 3], [0.0; 3], [0.0; 3]);
        for i in 0..GATHER_STEPS {
            let t = (f64::from(i) + 0.5) * step;
            let point = [view[0] * t, view[1] * t, r + view[2] * t];
            let at = dot(point, point).sqrt();
            let height = at - self.ground;
            let (e, s) = (self.extinction(height), self.scattering(height));
            let to_light = self.transmittance_from(at, dot(point, light) / at, GATHER_LIGHT_STEPS);
            for c in 0..3 {
                let kept = (-e[c] * step).exp();
                let taken = match e[c] > 0.0 {
                    true => (1.0 - kept) / e[c],
                    false => step,
                };
                seen[c] += through[c] * s[c] * even * to_light[c] * taken;
                share[c] += through[c] * s[c] * taken;
                through[c] *= kept;
            }
        }
        if ground {
            let point = [view[0] * length, view[1] * length, r + view[2] * length];
            let normal = point.map(|x| x / dot(point, point).sqrt());
            let cosine = dot(normal, light).max(0.0);
            let to_light = self.transmittance_from(self.ground, cosine, GATHER_LIGHT_STEPS);
            let bounce = f64::from(GROUND_ALBEDO) / PI * cosine;
            seen = [0, 1, 2].map(|c| seen[c] + through[c] * to_light[c] * bounce);
        }
        (seen, share)
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
    fn distance_to_top(&self, r: f64, mu: f64) -> f64 {
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
        let step = self.distance_to_top(r, mu) / f64::from(steps);
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

    /// The sky's radiance seen from the camera along `view`, lit by a body towards `light` of `illuminance` lux: scattered once, and more than once as the multiple scattering table holds it.
    pub fn radiance(&self, view: [f32; 3], light: [f32; 3], illuminance: f32) -> [f32; 3] {
        let multiple = self.multiple_table();
        let seen = self.radiance_in(unit(view), unit(light), VIEW_STEPS, &multiple);
        seen.map(|c| (c * f64::from(illuminance)) as f32)
    }

    /// The light scattered more than once at `height` with the light at cos-zenith `mu`, per unit of scattering, from the table between its nearest entries.
    fn multiple_at(multiple: &Multiple, height: f64, mu: f64) -> [f64; 3] {
        let air = &multiple.of;
        let row = ((height / (air.top - air.ground)).clamp(0.0, 1.0).sqrt()
            * (TABLE_HEIGHTS - 1) as f64)
            .min((TABLE_HEIGHTS - 1) as f64 - 1e-9);
        let column = (((mu.clamp(-1.0, 1.0) + 1.0) * 0.5) * (TABLE_ANGLES - 1) as f64)
            .min((TABLE_ANGLES - 1) as f64 - 1e-9);
        let (r, c) = (row.floor() as usize, column.floor() as usize);
        let (fr, fc) = (row - r as f64, column - c as f64);
        let at = |r: usize, c: usize| multiple.table[r * TABLE_ANGLES + c];
        [0, 1, 2].map(|k| {
            let low = at(r, c)[k] * (1.0 - fc) + at(r, c + 1)[k] * fc;
            let high = at(r + 1, c)[k] * (1.0 - fc) + at(r + 1, c + 1)[k] * fc;
            (low * (1.0 - fr) + high * fr) * air.multiple
        })
    }

    /// The radiance along `view` for a light of one lux, in `steps` steps.
    fn radiance_in(
        &self,
        view: [f64; 3],
        light: [f64; 3],
        steps: u32,
        multiple: &Multiple,
    ) -> [f64; 3] {
        let r0 = self.ground + CAMERA;
        if self.hits_ground(r0, view[2]) {
            return [0.0; 3];
        }
        let step = self.distance_to_top(r0, view[2]) / f64::from(steps);
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
            let mu_light = dot(point, light) / r;
            let towards_light = self.transmittance_from(r, mu_light, LIGHT_STEPS);
            let more = Self::multiple_at(multiple, height, mu_light);
            let [rayleigh, mie, _] = self.densities(height);
            out = [0, 1, 2].map(|c| {
                let once =
                    self.rayleigh[c] * rayleigh * phase_r + self.mie_scattering[c] * mie * phase_m;
                let all = self.rayleigh[c] * rayleigh + self.mie_scattering[c] * mie;
                // The depth to the middle of this step: the steps before it and half of its own.
                let to_middle = depth[c] + e[c] * step * 0.5;
                out[c] + (-to_middle).exp() * (towards_light[c] * once + more[c] * all) * step
            });
            depth = [0, 1, 2].map(|c| depth[c] + e[c] * step);
        }
        out
    }

    /// The sky's irradiance on level ground, in lux, lit by a body towards `light` of `illuminance` lux.
    pub fn irradiance(&self, light: [f32; 3], illuminance: f32) -> [f32; 3] {
        let light = unit(light);
        let multiple = self.multiple_table();
        let mut out = [0.0; 3];
        let solid = (1.0 / f64::from(RINGS)) * (2.0 * PI / f64::from(TURNS));
        for ring in 0..RINGS {
            let up = (f64::from(ring) + 0.5) / f64::from(RINGS);
            let across = (1.0 - up * up).sqrt();
            for turn in 0..TURNS {
                let angle = 2.0 * PI * (f64::from(turn) + 0.5) / f64::from(TURNS);
                let view = [across * angle.cos(), across * angle.sin(), up];
                let seen = self.radiance_in(view, light, LIGHT_STEPS, &multiple);
                out = [0, 1, 2].map(|c| out[c] + seen[c] * up * solid);
            }
        }
        out.map(|c| (c * f64::from(illuminance)) as f32)
    }
}

#[cfg(test)]
#[path = "gta6_sky_tests.rs"]
mod tests;
