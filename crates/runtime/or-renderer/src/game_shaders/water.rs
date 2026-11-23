//! What water reads that the game makes once for it: the row that says, for the cosine of the angle a ray meets the surface at, how much it reflects and how much foam shows, and the maps of its own dictionary.

use super::mesh::Image;

/// The maps the game's water reads out of its own texture dictionary: the ripples of its surface, and the foam laid over it.
#[derive(Debug, Clone, Default)]
pub struct WaterTextures {
    pub bump: Option<Image>,
    pub bump2: Option<Image>,
    pub foam: Option<Image>,
}

/// Texels across the row, as the game's render target of it is.
pub const BLEND_WIDTH: usize = 128;

/// The refractive indices of air and of water.
const AIR: f32 = 1.0;
const WATER: f32 = 1.33;

/// The share of light a surface between the media reflects at `cosine` of the angle of incidence.
fn reflection_coefficient(cosine: f32) -> f32 {
    let first = AIR * cosine;
    let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
    let second = WATER * (1.0 - (AIR / WATER * sine).powi(2)).sqrt();
    ((first - second) / (first + second)).powi(2)
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The row the game's water blend pass writes, as red the reflection and as green the foam, each texel at the middle of its span of the row.
pub fn blend_row() -> Vec<[f32; 4]> {
    (0..BLEND_WIDTH)
        .map(|i| {
            let along = (i as f32 + 0.5) / BLEND_WIDTH as f32;
            [
                reflection_coefficient(along + 0.0001),
                smoothstep(0.5, 0.7, along),
                0.0,
                0.0,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_along_the_normal_reflects_two_per_cent_and_one_along_the_surface_reflects_all() {
        let row = blend_row();
        assert_eq!(row.len(), BLEND_WIDTH);
        // Fresnel at normal incidence, ((n1 - n2) / (n1 + n2)) squared, with the first texel nearly grazing
        let normal = reflection_coefficient(1.0);
        assert!((normal - 0.0201).abs() < 1e-4, "{normal}");
        assert!(row[0][0] > 0.6, "{:?}", row[0]);
        // The reflection falls as the ray comes nearer the normal
        assert!(row.windows(2).all(|pair| pair[1][0] <= pair[0][0]));
        assert!(row[BLEND_WIDTH - 1][0] < 0.03);
    }

    #[test]
    fn foam_shows_from_the_middle_of_the_row_and_is_whole_at_its_end() {
        let row = blend_row();
        assert_eq!(row[BLEND_WIDTH / 4][1], 0.0);
        assert!(row[BLEND_WIDTH * 3 / 5][1] > 0.0 && row[BLEND_WIDTH * 3 / 5][1] < 1.0);
        assert_eq!(row[BLEND_WIDTH - 1][1], 1.0);
    }
}
