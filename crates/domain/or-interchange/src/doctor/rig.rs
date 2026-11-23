//! The rig a skinned mesh is bound to: whether its bones are the skeleton's, whether its weights add up, and mending both.
//!
//! A skin states its bones by name and a skeleton is found by those names, so a group a modelling tool named
//! `Group.001`, or never named, binds a weight to nothing. [`check`] lists what is wrong, and [`repair`] gives a weight on
//! such a group to the nearest bone above it that the skeleton does have (or drops it), spells a bone as the skeleton
//! does, and then cleans the weights ([`clean_weights`]).
//!
//! ⚠ Nothing here moves a vertex, adds a bone or changes a skeleton: only which bones a vertex is bound to, and how much.

use std::collections::BTreeMap;

use crate::scene::{Bone, Object, Primitive, Scene};

/// A weight under this share of a vertex is noise a skin carries and a game spends a lane on.
pub const FAINT: f32 = 0.01;

/// How far the weights of a vertex may be from adding up to one before they are said not to.
const WHOLE: f32 = 1e-3;

/// What is wrong with a rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Defect {
    /// A bone a tool left with no name, or the name it gives a thing it made itself.
    UnnamedGroup,
    /// A bone named for none the skeleton has.
    UnknownBone,
    /// A bone of the skeleton under another spelling: other letters, or a copy suffix.
    Misspelt,
    /// A vertex no bone has any hold of.
    Unweighted,
    /// A vertex whose weights do not add up to one.
    Unnormalised,
    /// A weight so small it is noise.
    Faint,
}

impl Defect {
    /// The words the interface shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::UnnamedGroup => "Unnamed group",
            Self::UnknownBone => "Bone not in the skeleton",
            Self::Misspelt => "Bone spelt otherwise",
            Self::Unweighted => "Unweighted vertex",
            Self::Unnormalised => "Weights not adding up",
            Self::Faint => "Faint weight",
        }
    }
}

/// One kind of fault, how many it was met in, and the first few.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub defect: Defect,
    /// Vertices it was met in.
    pub count: usize,
    /// The bones it is about, or none for a fault of the weights.
    pub bones: Vec<String>,
}

impl Finding {
    pub fn sentence(&self) -> String {
        let listed = self
            .bones
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = self.bones.len().saturating_sub(4);
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        match self.defect {
            Defect::UnnamedGroup => format!(
                "{} vertices are weighted to groups with no name of a bone ({listed}{tail})",
                self.count
            ),
            Defect::UnknownBone => format!(
                "{} vertices are weighted to bones the skeleton does not have ({listed}{tail})",
                self.count
            ),
            Defect::Misspelt => format!(
                "{} vertices are weighted to bones spelt otherwise than the skeleton spells them ({listed}{tail})",
                self.count
            ),
            Defect::Unweighted => format!("{} vertices have no weight at all", self.count),
            Defect::Unnormalised => {
                format!("{} vertices have weights that do not add up to one", self.count)
            }
            Defect::Faint => format!(
                "{} vertices carry a weight under {} percent",
                self.count,
                FAINT * 100.0
            ),
        }
    }
}

/// Whether `name` is one no person gave a bone: none, `bone3` as a reader numbers a joint that had none, or what a tool calls what it made.
pub fn is_unnamed(name: &str) -> bool {
    let name = crate::gltf::read::strip_copy_suffix(name.trim());
    let lower = name.to_ascii_lowercase();
    let numbered = |stem: &str| {
        lower
            .strip_prefix(stem)
            .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
    };
    lower.is_empty()
        || numbered("bone")
        || numbered("group")
        || numbered("joint")
        || numbered("node")
        || matches!(lower.as_str(), "armature" | "empty" | "root" | "object")
}

/// The skeleton's spelling of `name`, where it is the same bone spelt otherwise.
fn spelt_as<'a>(name: &str, skeleton: &'a [String]) -> Option<&'a String> {
    let plain = crate::gltf::read::strip_copy_suffix(name.trim()).to_ascii_lowercase();
    skeleton.iter().find(|s| s.to_ascii_lowercase() == plain)
}

/// What a joint of an object is, as far as `skeleton` goes.
enum Joint {
    Fine,
    Unnamed,
    Unknown,
    Misspelt(String),
}

fn classify(name: &str, skeleton: &[String]) -> Joint {
    if skeleton.iter().any(|s| s == name) {
        Joint::Fine
    } else if is_unnamed(name) {
        Joint::Unnamed
    } else if let Some(right) = spelt_as(name, skeleton) {
        Joint::Misspelt(right.clone())
    } else {
        Joint::Unknown
    }
}

fn primitives(object: &Object) -> impl Iterator<Item = &Primitive> {
    object.meshes.iter().flat_map(|m| m.primitives.iter())
}

/// What is wrong with the rig `scene` is bound to, against the bones `skeleton` names.
///
/// With no names to check against only the weights are judged.
pub fn check(scene: &Scene, skeleton: &[String]) -> Vec<Finding> {
    let mut by_bone: BTreeMap<Defect, (usize, Vec<String>)> = BTreeMap::new();
    let mut note = |defect: Defect, count: usize, bone: Option<String>| {
        let entry = by_bone.entry(defect).or_default();
        entry.0 += count;
        if let Some(bone) = bone.filter(|b| !entry.1.contains(b)) {
            entry.1.push(bone);
        }
    };
    for object in &scene.objects {
        let names = &object.bones;
        for primitive in primitives(object).filter(|p| p.is_skinned()) {
            let mut per_bone: BTreeMap<u16, usize> = BTreeMap::new();
            for (joints, weights) in primitive.joints.iter().zip(&primitive.weights) {
                let total: f32 = weights.iter().sum();
                if total <= 0.0 {
                    note(Defect::Unweighted, 1, None);
                    continue;
                }
                if (total - 1.0).abs() > WHOLE {
                    note(Defect::Unnormalised, 1, None);
                }
                if weights.iter().any(|w| *w > 0.0 && *w < FAINT) {
                    note(Defect::Faint, 1, None);
                }
                for lane in (0..4).filter(|k| weights[*k] > 0.0) {
                    *per_bone.entry(joints[lane]).or_default() += 1;
                }
            }
            if skeleton.is_empty() || names.is_empty() {
                continue;
            }
            for (joint, vertices) in per_bone {
                let name = names
                    .get(usize::from(joint))
                    .map_or_else(|| format!("bone {joint}"), |b| b.name.clone());
                let shown = if name.is_empty() {
                    format!("bone {joint}")
                } else {
                    name.clone()
                };
                match classify(&name, skeleton) {
                    Joint::Fine => {}
                    Joint::Unnamed => note(Defect::UnnamedGroup, vertices, Some(shown)),
                    Joint::Unknown => note(Defect::UnknownBone, vertices, Some(shown)),
                    Joint::Misspelt(_) => note(Defect::Misspelt, vertices, Some(shown)),
                }
            }
        }
    }
    by_bone
        .into_iter()
        .map(|(defect, (count, bones))| Finding {
            defect,
            count,
            bones,
        })
        .collect()
}

/// The nearest bone above `joint` that `keep` accepts, following the skeleton's parents; none when there is none.
fn nearest_above(bones: &[Bone], joint: usize, keep: &dyn Fn(usize) -> bool) -> Option<usize> {
    let mut at = bones.get(joint)?.parent;
    let mut guard = bones.len();
    while let Some(i) = at {
        if keep(i) {
            return Some(i);
        }
        at = bones.get(i)?.parent;
        guard -= 1;
        if guard == 0 {
            return None;
        }
    }
    None
}

/// What cleaning the weights did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// Vertices that lost a faint weight.
    pub faint: usize,
    /// Vertices whose weights were made to add up to one.
    pub renormalised: usize,
    /// Vertices with no weight, given the skin of the nearest vertex that has one.
    pub unweighted: usize,
}

impl Cleaned {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.faint > 0 {
            lines.push(format!(
                "{} vertices lost a weight under {} percent",
                self.faint,
                FAINT * 100.0
            ));
        }
        if self.renormalised > 0 {
            lines.push(format!(
                "{} vertices had their weights made to add up to one",
                self.renormalised
            ));
        }
        if self.unweighted > 0 {
            lines.push(format!(
                "{} vertices with no weight were given the skin of the nearest vertex that has one",
                self.unweighted
            ));
        }
        lines
    }
}

fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]) * (a[k] - b[k])).sum()
}

/// Drop the weights under `faint`, make what is left add up to one and give a vertex with none the skin of the nearest vertex that has one.
///
/// The four lanes stay four: a weight is moved to the lane of the same bone or dropped, never spread over a fifth bone.
pub fn clean_weights(primitive: &mut Primitive, faint: f32) -> Cleaned {
    let mut report = Cleaned::default();
    if !primitive.is_skinned() || primitive.joints.len() != primitive.weights.len() {
        return report;
    }
    let count = primitive.joints.len();
    let mut bare: Vec<usize> = Vec::new();
    for v in 0..count {
        let lanes: Vec<(u16, f32)> = (0..4)
            .filter(|k| primitive.weights[v][*k] > 0.0)
            .map(|k| (primitive.joints[v][k], primitive.weights[v][k]))
            .collect();
        if lanes.is_empty() {
            bare.push(v);
            continue;
        }
        let heaviest = lanes
            .iter()
            .cloned()
            .fold((0u16, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
        let kept: Vec<(u16, f32)> = lanes.iter().cloned().filter(|l| l.1 >= faint).collect();
        // A vertex whose every weight is faint keeps its heaviest bone rather than none.
        let kept = if kept.is_empty() {
            vec![heaviest]
        } else {
            kept
        };
        let total: f32 = kept.iter().map(|l| l.1).sum();
        let dropped = kept.len() != lanes.len();
        let uneven = (total - 1.0).abs() > WHOLE;
        if !dropped && !uneven {
            continue;
        }
        report.faint += usize::from(dropped);
        report.renormalised += usize::from(uneven || dropped);
        let mut joints = [0u16; 4];
        let mut weights = [0.0f32; 4];
        for (k, (joint, weight)) in kept.iter().enumerate() {
            joints[k] = *joint;
            weights[k] = weight / total;
        }
        primitive.joints[v] = joints;
        primitive.weights[v] = weights;
    }
    if !bare.is_empty() {
        let weighted: Vec<usize> = (0..count).filter(|v| !bare.contains(v)).collect();
        if !weighted.is_empty() {
            for v in bare {
                let from = weighted
                    .iter()
                    .copied()
                    .min_by(|a, b| {
                        distance2(primitive.positions[*a], primitive.positions[v])
                            .total_cmp(&distance2(primitive.positions[*b], primitive.positions[v]))
                    })
                    .unwrap_or(0);
                primitive.joints[v] = primitive.joints[from];
                primitive.weights[v] = primitive.weights[from];
                report.unweighted += 1;
            }
        }
    }
    report
}

/// Mend what [`check`] finds in `scene` against `skeleton`, and say what was done.
///
/// A bone spelt otherwise is renamed as the skeleton spells it. A weight on an unnamed group or a bone the skeleton does not
/// have goes to the nearest bone above it that the skeleton does have, and is dropped where there is none; the weights are
/// then cleaned. A bone left with no weight is left in the skeleton of the scene, which costs a file nothing.
pub fn repair(scene: &mut Scene, skeleton: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    for object in &mut scene.objects {
        if skeleton.is_empty() || object.bones.is_empty() {
            continue;
        }
        let verdict: Vec<Joint> = object
            .bones
            .iter()
            .map(|b| classify(&b.name, skeleton))
            .collect();
        // An object whose every bone is the skeleton's is left exactly as it is.
        if verdict.iter().all(|j| matches!(j, Joint::Fine)) {
            continue;
        }
        for (bone, joint) in object.bones.iter_mut().zip(&verdict) {
            if let Joint::Misspelt(right) = joint {
                lines.push(format!("{:?} is spelt {right:?}", bone.name));
                bone.name = right.clone();
            }
        }
        let fine = |i: usize| matches!(verdict.get(i), Some(Joint::Fine | Joint::Misspelt(_)));
        // Where each joint's weight goes: itself, the nearest bone above it, or nowhere.
        let target: Vec<Option<u16>> = (0..object.bones.len())
            .map(|i| {
                if fine(i) {
                    Some(i as u16)
                } else {
                    nearest_above(&object.bones, i, &fine).map(|a| a as u16)
                }
            })
            .collect();
        // Two joints now spelt alike are one bone.
        let twin: Vec<u16> = (0..object.bones.len())
            .map(|i| {
                if !fine(i) {
                    return i as u16;
                }
                (0..i)
                    .find(|j| fine(*j) && object.bones[*j].name == object.bones[i].name)
                    .unwrap_or(i) as u16
            })
            .collect();
        let mut moved: BTreeMap<String, usize> = BTreeMap::new();
        let mut dropped = 0usize;
        let names: Vec<String> = object.bones.iter().map(|b| b.name.clone()).collect();
        for primitive in object
            .meshes
            .iter_mut()
            .flat_map(|m| m.primitives.iter_mut())
            .filter(|p| p.is_skinned())
        {
            for (joints, weights) in primitive
                .joints
                .iter_mut()
                .zip(primitive.weights.iter_mut())
            {
                let mut lanes: Vec<(u16, f32)> = Vec::new();
                for k in (0..4).filter(|k| weights[*k] > 0.0) {
                    let from = usize::from(joints[k]);
                    let to = target.get(from).copied().flatten();
                    let joint = match to {
                        Some(t) => twin[usize::from(t)],
                        None => {
                            dropped += 1;
                            continue;
                        }
                    };
                    if usize::from(joint) != from {
                        let name = names.get(from).cloned().unwrap_or_default();
                        *moved.entry(name).or_default() += 1;
                    }
                    match lanes.iter_mut().find(|l| l.0 == joint) {
                        Some(l) => l.1 += weights[k],
                        None => lanes.push((joint, weights[k])),
                    }
                }
                lanes.sort_by(|a, b| b.1.total_cmp(&a.1));
                lanes.truncate(4);
                *joints = [0; 4];
                *weights = [0.0; 4];
                for (k, (joint, weight)) in lanes.iter().enumerate() {
                    joints[k] = *joint;
                    weights[k] = *weight;
                }
            }
        }
        for (name, vertices) in moved {
            let shown = if name.is_empty() {
                "a bone with no name".to_string()
            } else {
                format!("{name:?}")
            };
            lines.push(format!(
                "{vertices} weights on {shown} went to the bone above it"
            ));
        }
        if dropped > 0 {
            lines.push(format!(
                "{dropped} weights on groups with no bone above them that the skeleton has were dropped"
            ));
        }
        // Weights that were merged or dropped are made to add up again, and a vertex left with none takes its neighbour's.
        for primitive in object
            .meshes
            .iter_mut()
            .flat_map(|m| m.primitives.iter_mut())
        {
            lines.extend(clean_weights(primitive, 0.0).lines());
        }
    }
    lines
}

/// What a look at the heel of a shoe found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heel {
    /// Vertices in the heel's region: the lowest fifth of each foot's height, at the back.
    pub vertices: usize,
    /// How many of them the foot bone holds most.
    pub on_foot: usize,
    /// The bones that hold the others most, and how many each does.
    pub elsewhere: Vec<(String, usize)>,
}

impl Heel {
    /// The share of the heel the foot bone holds.
    pub fn share(&self) -> f32 {
        if self.vertices == 0 {
            1.0
        } else {
            self.on_foot as f32 / self.vertices as f32
        }
    }

    /// Whether the foot bone holds the heel, as a heel that stays rigid with the foot needs.
    pub fn holds(&self) -> bool {
        self.share() >= 0.9
    }

    pub fn sentence(&self) -> String {
        if self.vertices == 0 {
            return "No heel was found: the shoe has no skin to look at".to_string();
        }
        let mut said = format!(
            "The foot bone holds {} of the {} vertices of the heel ({:.0} percent)",
            self.on_foot,
            self.vertices,
            100.0 * self.share()
        );
        if !self.elsewhere.is_empty() {
            let others: Vec<String> = self
                .elsewhere
                .iter()
                .take(3)
                .map(|(n, c)| format!("{n} {c}"))
                .collect();
            said.push_str(&format!("; the rest follow {}", others.join(", ")));
        }
        said
    }
}

/// Look at the heel of the shoes in `scene`: which bone holds the vertices at the back and bottom of each foot.
///
/// A foot is the half of the mesh on one side of the middle line, the heel its lowest fifth by height that lies in the rear half
/// (towards -y, the way a ped faces +y), and the foot bone any bone whose name has `Foot` in it. `skeleton` names the bones where the scene carries none.
pub fn heel_bones(scene: &Scene, skeleton: &[String]) -> Heel {
    let mut heel = Heel::default();
    let mut elsewhere: BTreeMap<String, usize> = BTreeMap::new();
    for object in &scene.objects {
        let name_of = |joint: u16| -> String {
            object
                .bones
                .get(usize::from(joint))
                .map(|b| b.name.clone())
                .or_else(|| skeleton.get(usize::from(joint)).cloned())
                .unwrap_or_else(|| format!("bone {joint}"))
        };
        for primitive in primitives(object).filter(|p| p.is_skinned() && p.lod.is_default_visible())
        {
            for side in [false, true] {
                let members: Vec<usize> = (0..primitive.positions.len())
                    .filter(|v| (primitive.positions[*v][0] > 0.0) == side)
                    .collect();
                if members.len() < 8 {
                    continue;
                }
                let (mut low, mut high) = (f32::MAX, f32::MIN);
                let (mut back, mut front) = (f32::MAX, f32::MIN);
                for v in &members {
                    let p = primitive.positions[*v];
                    low = low.min(p[2]);
                    high = high.max(p[2]);
                    back = back.min(p[1]);
                    front = front.max(p[1]);
                }
                let (floor, middle) = (low + (high - low) * 0.2, (back + front) / 2.0);
                for v in members.into_iter().filter(|v| {
                    let p = primitive.positions[*v];
                    p[2] <= floor && p[1] <= middle
                }) {
                    let lanes = (0..4).filter(|k| primitive.weights[v][*k] > 0.0);
                    let Some(lane) = lanes.max_by(|a, b| {
                        primitive.weights[v][*a].total_cmp(&primitive.weights[v][*b])
                    }) else {
                        continue;
                    };
                    heel.vertices += 1;
                    let name = name_of(primitive.joints[v][lane]);
                    if name.to_ascii_lowercase().contains("foot") {
                        heel.on_foot += 1;
                    } else {
                        *elsewhere.entry(name).or_default() += 1;
                    }
                }
            }
        }
    }
    let mut elsewhere: Vec<(String, usize)> = elsewhere.into_iter().collect();
    elsewhere.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    heel.elsewhere = elsewhere;
    heel
}
