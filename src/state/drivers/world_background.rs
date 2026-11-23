/// The colour the viewport is cleared to before anything is drawn, whether Shaded draws the game's sky over it, and whether it lays a ground under an effect.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldBackgroundDriver {
    pub colour: [f32; 4],
    pub sky: bool,
    pub ground: bool,
}

impl Default for WorldBackgroundDriver {
    fn default() -> Self {
        Self {
            colour: [0.176, 0.176, 0.188, 1.0],
            sky: false,
            ground: false,
        }
    }
}
