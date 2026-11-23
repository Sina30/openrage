//! What loaded plugins draw over the open document, as the line vertices the viewport renders.

use glam::Vec3;
use or_plugin::Primitive;
use or_renderer::LineVertex;

use crate::state::AppState;

/// The vertices of `primitives`: a point is a cross whose arms reach its size, and a line runs from one colour to the other.
///
/// ⚠ A primitive with a position that is not finite is skipped, so one bad number from a guest
/// does not put a vertex at infinity in the renderer's buffer.
pub fn lines_of(primitives: &[Primitive]) -> Vec<LineVertex> {
    let mut out = Vec::new();
    for primitive in primitives {
        match primitive {
            Primitive::Point { at, size, colour } => {
                let at = Vec3::from(*at);
                if !at.is_finite() || !size.is_finite() {
                    continue;
                }
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    for end in [at - axis * *size, at + axis * *size] {
                        out.push(LineVertex::new(end.to_array(), *colour));
                    }
                }
            }
            Primitive::Line { from, to } => {
                if !Vec3::from(from.at).is_finite() || !Vec3::from(to.at).is_finite() {
                    continue;
                }
                out.push(LineVertex::new(from.at, from.colour));
                out.push(LineVertex::new(to.at, to.colour));
            }
        }
    }
    out
}

/// Add the overlays plugins drew over the open document to `lines`, which draw in front of the meshes.
pub fn append(lines: &mut Vec<LineVertex>, state: &AppState) {
    for overlay in state.plugin_overlays() {
        lines.extend(lines_of(overlay));
    }
}

#[cfg(test)]
mod tests {
    use or_plugin::Vertex;

    use super::*;

    fn red() -> [f32; 4] {
        [1.0, 0.0, 0.0, 1.0]
    }

    #[test]
    fn a_point_is_a_cross_and_a_line_keeps_the_colour_of_each_end() {
        let drawn = lines_of(&[
            Primitive::Point {
                at: [1.0, 2.0, 3.0],
                size: 0.5,
                colour: red(),
            },
            Primitive::Line {
                from: Vertex {
                    at: [0.0; 3],
                    colour: [0.0, 0.0, 1.0, 1.0],
                },
                to: Vertex {
                    at: [1.0; 3],
                    colour: red(),
                },
            },
        ]);

        assert_eq!(
            drawn.len(),
            6 + 2,
            "three arms of two vertices, and a segment"
        );
        let positions: Vec<[f32; 3]> = drawn.iter().map(|v| v.position).collect();
        assert_eq!(positions[0], [0.5, 2.0, 3.0]);
        assert_eq!(positions[1], [1.5, 2.0, 3.0]);
        assert_eq!(positions[6], [0.0; 3]);
        assert_eq!(drawn[6].colour, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(drawn[7].colour, red());
    }

    #[test]
    fn a_primitive_at_infinity_draws_nothing() {
        let drawn = lines_of(&[
            Primitive::Point {
                at: [f32::INFINITY, 0.0, 0.0],
                size: 1.0,
                colour: red(),
            },
            Primitive::Point {
                at: [0.0; 3],
                size: f32::NAN,
                colour: red(),
            },
            Primitive::Line {
                from: Vertex {
                    at: [f32::NAN; 3],
                    colour: red(),
                },
                to: Vertex {
                    at: [0.0; 3],
                    colour: red(),
                },
            },
        ]);
        assert!(drawn.is_empty());
    }
}
