//! Left and right: a mesh turned about the plane x = 0, with its skin given to the bones on the other side.
//!
//! ⚠ **Mirroring twice is the identity.** The coordinates are negated, the two last corners of every triangle are swapped,
//! a tangent's sign is turned and each bone is exchanged for its counterpart, and every one of those is its own inverse,
//! so what comes back is bit for bit what went.
//!
//! ⚠ A skin can only be mirrored with the names of the bones, since a left bone is found by the word `L` in its name. A
//! weight on a bone with a side and no counterpart is refused, naming the bone.

use std::collections::HashMap;

use or_io::error::{IoError, Result};
use or_io::gta5::{RmcDrawable, SpdAABB};

use crate::import::Imported;
use crate::scene::{Bone, Scene};

/// Which bone of a skeleton stands where a bone's mirror image does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneMirror {
    /// By bone: its counterpart, itself on the middle line, and `None` for a bone with a side and no partner.
    counterpart: Vec<Option<u16>>,
    names: Vec<String>,
}

/// The name of the bone on the other side, or none for a name that states no side.
fn other_side(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    (1..bytes.len().saturating_sub(1)).find_map(|at| {
        let side = bytes[at];
        let sided = bytes[at - 1] == b'_' && bytes[at + 1] == b'_';
        let swapped = match side {
            b'L' => b'R',
            b'R' => b'L',
            b'l' => b'r',
            b'r' => b'l',
            _ => return None,
        };
        sided.then(|| {
            let mut out = bytes.to_vec();
            out[at] = swapped;
            String::from_utf8_lossy(&out).into_owned()
        })
    })
}

impl BoneMirror {
    /// The pairs of the bones named `names`, in the order a palette indexes them.
    pub fn of_names(names: &[String]) -> Self {
        let by_name: HashMap<String, usize> = names
            .iter()
            .enumerate()
            .rev()
            .map(|(i, n)| (n.to_ascii_lowercase(), i))
            .collect();
        let counterpart = names
            .iter()
            .enumerate()
            .map(|(i, name)| match other_side(name) {
                None => Some(i as u16),
                Some(other) => by_name
                    .get(&other.to_ascii_lowercase())
                    .map(|j| *j as u16)
                    // A name two bones share is no pair: the swap would not be its own inverse.
                    .filter(|j| other_side(&names[*j as usize]).is_some()),
            })
            .collect::<Vec<_>>();
        // Held to being its own inverse: a pair is two bones that name each other.
        let held = counterpart
            .iter()
            .enumerate()
            .map(|(i, c)| c.filter(|c| counterpart[*c as usize] == Some(i as u16)))
            .collect();
        Self {
            counterpart: held,
            names: names.to_vec(),
        }
    }

    pub fn of(bones: &[Bone]) -> Self {
        let names: Vec<String> = bones.iter().map(|b| b.name.clone()).collect();
        Self::of_names(&names)
    }

    /// The bone on the other side of bone `bone`; none when it has a side and nothing stands opposite.
    pub fn counterpart(&self, bone: u16) -> Option<u16> {
        self.counterpart.get(usize::from(bone)).copied().flatten()
    }

    /// How many bones have a partner on the other side.
    pub fn pairs(&self) -> usize {
        self.counterpart
            .iter()
            .enumerate()
            .filter(|(i, c)| c.is_some_and(|c| usize::from(c) != *i))
            .count()
            / 2
    }

    /// The name of bone `bone`.
    pub fn name(&self, bone: u16) -> String {
        self.names
            .get(usize::from(bone))
            .cloned()
            .unwrap_or_else(|| format!("bone {bone}"))
    }
}

/// What a mirror did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorReport {
    pub vertices: usize,
    pub triangles: usize,
    /// Vertices whose skin moved to the bones on the other side.
    pub rebound: usize,
    /// Pairs of bones the skin was exchanged between.
    pub pairs: usize,
}

impl MirrorReport {
    pub fn lines(&self) -> Vec<String> {
        let mut line = format!(
            "Mirrored {} vertices and turned {} triangles about the middle line",
            self.vertices, self.triangles
        );
        if self.rebound > 0 {
            line.push_str(&format!(
                "; {} vertices were given to the bones on the other side ({} pairs of bones)",
                self.rebound, self.pairs
            ));
        }
        vec![line]
    }
}

fn refuse<T>(context: impl Into<String>) -> Result<T> {
    Err(IoError::InvalidData {
        context: context.into(),
    })
}

/// Mirror every mesh of `scene` about x = 0; `bones` pairs the skeleton their skin is bound to, and an object that carries its own skeleton is paired by it.
///
/// Refused, with nothing changed, a skin with no pairs to move it by and a weight on a bone with a side and no partner.
pub fn mirror_scene(scene: &mut Scene, bones: Option<&BoneMirror>) -> Result<MirrorReport> {
    let mut report = MirrorReport::default();
    // Everything is checked before anything moves, so a refusal leaves the scene as it was.
    let mut pairings: Vec<Option<BoneMirror>> = Vec::new();
    for object in &scene.objects {
        let own = (!object.bones.is_empty()).then(|| BoneMirror::of(&object.bones));
        let pairing = own.or_else(|| bones.cloned());
        for primitive in object.meshes.iter().flat_map(|m| m.primitives.iter()) {
            for (joints, weights) in primitive.joints.iter().zip(&primitive.weights) {
                for lane in (0..4).filter(|k| weights[*k] > 0.0) {
                    let Some(pairing) = &pairing else {
                        return refuse(
                            "the mesh is skinned against a skeleton nobody has named here, so there is no telling \
                             which bone is on the other side: give the ped's skeleton",
                        );
                    };
                    if pairing.counterpart(joints[lane]).is_none() {
                        return refuse(format!(
                            "a vertex is weighted to {}, which has a side and no bone opposite it",
                            pairing.name(joints[lane])
                        ));
                    }
                }
            }
        }
        pairings.push(pairing);
    }
    for (object, pairing) in scene.objects.iter_mut().zip(&pairings) {
        if let Some(pairing) = pairing {
            report.pairs = report.pairs.max(pairing.pairs());
        }
        for primitive in object
            .meshes
            .iter_mut()
            .flat_map(|m| m.primitives.iter_mut())
        {
            report.vertices += primitive.positions.len();
            report.triangles += primitive.indices.len() / 3;
            for p in &mut primitive.positions {
                p[0] = -p[0];
            }
            for n in &mut primitive.normals {
                n[0] = -n[0];
            }
            // A tangent's last component is the handedness of the frame, which a mirror turns over.
            for t in &mut primitive.tangents {
                t[0] = -t[0];
                t[3] = -t[3];
            }
            for triangle in primitive.indices.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
            let Some(pairing) = pairing else { continue };
            for (joints, weights) in primitive.joints.iter_mut().zip(&primitive.weights) {
                let mut moved = false;
                for lane in (0..4).filter(|k| weights[*k] > 0.0) {
                    let to = pairing.counterpart(joints[lane]).unwrap_or(joints[lane]);
                    moved |= to != joints[lane];
                    joints[lane] = to;
                }
                report.rebound += usize::from(moved);
            }
        }
    }
    Ok(report)
}

/// The box turned about x = 0.
fn mirrored_box(b: &mut SpdAABB) {
    let (low, high) = (b.min.x, b.max.x);
    b.min.x = -high;
    b.max.x = -low;
}

/// `drawable` mirrored about x = 0 on a clone, every level of detail, with its boxes and its bounding sphere.
///
/// `skeleton` names the bones a ped component's skin is bound to; a drawable with a skeleton of its own is paired by that.
pub fn mirror_drawable<T: Imported>(
    drawable: &RmcDrawable<T>,
    skeleton: Option<&[Bone]>,
) -> Result<(RmcDrawable<T>, MirrorReport)> {
    let (mut made, report) =
        super::through_scene(drawable, skeleton, |scene| mirror_scene(scene, None))?;
    mirrored_box(&mut made.lod_group.bounds_box);
    made.lod_group.bounds_sphere.center_and_radius.x =
        -made.lod_group.bounds_sphere.center_and_radius.x;
    Ok((made, report))
}
