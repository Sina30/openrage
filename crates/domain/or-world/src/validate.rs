//! What a world file has to hold for the engine to read it the way we did.
//!
//! ⚠ Every check here is a number that subscripts an array the file states
//! separately — a place the reader tolerates what the engine does not, so an
//! edit can produce a file OpenRage reads back identically and the game
//! misreads.
//!
//! Entities form a tree by index: a child names its parent's position in the
//! same `entities` array, and a parent states how many children it has. Both
//! halves are stored, neither is derived on load, and the engine trusts them.

use crate::map_data::MapData;

/// Something about a map that would misbehave in the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A parent index that is not a position in this map's entity array.
    ParentOutOfRange {
        entity: usize,
        parent: usize,
        len: usize,
    },
    /// An entity naming itself as its own LOD parent.
    ParentIsSelf { entity: usize },
    /// A cycle in the parent chain.
    ParentCycle { entity: usize },
    /// `numChildren` disagreeing with how many entities name this one.
    ChildCountWrong {
        entity: usize,
        declared: u32,
        actual: u32,
    },
    /// A room's attached-object index that is not a position in the
    /// interior's entity array.
    AttachedObjectOutOfRange { room: usize, index: u32, len: usize },
    /// An entity set whose two parallel arrays are different lengths.
    EntitySetArraysDiffer {
        set: usize,
        locations: usize,
        entities: usize,
    },
    /// A portal naming a room that does not exist.
    PortalRoomOutOfRange {
        portal: usize,
        room: u32,
        len: usize,
    },
    /// A portal whose quad is not four corners.
    PortalNotAQuad { portal: usize, corners: usize },
    /// A light array of a different length from its siblings.
    LightArraysDiffer {
        field: &'static str,
        len: usize,
        expected: usize,
    },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::ParentOutOfRange {
                entity,
                parent,
                len,
            } => write!(
                f,
                "entity {entity} names LOD parent {parent}, past the {len} entities in this map"
            ),
            Problem::ParentIsSelf { entity } => {
                write!(f, "entity {entity} is its own LOD parent")
            }
            Problem::ParentCycle { entity } => {
                write!(f, "entity {entity} is in a LOD parent cycle")
            }
            Problem::ChildCountWrong {
                entity,
                declared,
                actual,
            } => write!(
                f,
                "entity {entity} declares {declared} children and {actual} name it"
            ),
            Problem::AttachedObjectOutOfRange { room, index, len } => write!(
                f,
                "room {room} attaches object {index}, past the {len} entities in this interior"
            ),
            Problem::EntitySetArraysDiffer {
                set,
                locations,
                entities,
            } => write!(
                f,
                "entity set {set} has {locations} locations for {entities} entities"
            ),
            Problem::PortalRoomOutOfRange { portal, room, len } => write!(
                f,
                "portal {portal} names room {room}, past the {len} rooms in this interior"
            ),
            Problem::PortalNotAQuad { portal, corners } => {
                write!(f, "portal {portal} has {corners} corners rather than four")
            }
            Problem::LightArraysDiffer {
                field,
                len,
                expected,
            } => write!(
                f,
                "light array {field} holds {len} where its siblings hold {expected}"
            ),
        }
    }
}

/// Check a map's LOD hierarchy and its light arrays.
pub fn check(map: &MapData<'_>) -> Vec<Problem> {
    let mut problems = check_hierarchy(map);
    problems.extend(check_light_arrays(map));
    problems
}

/// ⚠ **Only the links this file can resolve.** An entity whose `flags` bit 3
/// is set indexes the *parent* map's array, which one file cannot see, and
/// `numChildren` counts children in child maps as well as this one. Both are
/// [`crate::group`]'s to check, and checking them here reports every shipped
/// map as broken.
fn check_hierarchy(map: &MapData<'_>) -> Vec<Problem> {
    let entities: Vec<_> = map.entities().collect();
    let len = entities.len();
    let mut problems = Vec::new();

    for (i, entity) in entities.iter().enumerate() {
        let Some(parent) = entity.local_parent_index() else {
            continue;
        };
        if parent >= len {
            problems.push(Problem::ParentOutOfRange {
                entity: i,
                parent,
                len,
            });
        } else if parent == i {
            problems.push(Problem::ParentIsSelf { entity: i });
        }
    }

    // A chain longer than the array has to revisit a node.
    for (i, _) in entities.iter().enumerate() {
        let mut at = i;
        for _ in 0..len {
            match entities[at].local_parent_index() {
                Some(p) if p < len && p != at => at = p,
                _ => break,
            }
        }
        if entities[at]
            .local_parent_index()
            .is_some_and(|p| p < len && p != at)
        {
            problems.push(Problem::ParentCycle { entity: i });
            break;
        }
    }

    problems
}

/// ⚠ **The LOD-light structs are eight parallel arrays under one subscript**,
/// and `DistantLODLightsSOA` is two more beside a separately-stated count.
/// The engine walks them together, so one short array is a read past its end
/// at whatever index the longest one reaches.
fn check_light_arrays(map: &MapData<'_>) -> Vec<Problem> {
    use crate::field;
    use or_io::hash::literal_string_hash;

    const LOD_LIGHT_FIELDS: &[&str] = &[
        "direction",
        "falloff",
        "falloffExponent",
        "timeAndStateFlags",
        "hash",
        "coneInnerAngle",
        "coneOuterAngleOrCapExt",
        "coronaIntensity",
    ];
    const DISTANT_FIELDS: &[&str] = &["position", "RGBI"];

    let mut problems = Vec::new();
    for (key, fields) in [
        (crate::keys::LOD_LIGHTS_SOA, LOD_LIGHT_FIELDS),
        (crate::keys::DISTANT_LOD_LIGHTS_SOA, DISTANT_FIELDS),
    ] {
        let Some(soa) = field::struct_at(map.instance(), key) else {
            continue;
        };
        let lengths: Vec<usize> = fields
            .iter()
            .map(|name| field::array_at(soa, literal_string_hash(name.as_bytes())).len())
            .collect();
        let Some(&expected) = lengths.iter().max() else {
            continue;
        };
        if expected == 0 {
            continue;
        }
        for (name, len) in fields.iter().zip(&lengths) {
            if *len != expected {
                problems.push(Problem::LightArraysDiffer {
                    field: name,
                    len: *len,
                    expected,
                });
            }
        }
    }
    problems
}

/// Check an interior's rooms, portals and entity sets against each other.
pub fn check_interior(interior: &crate::map_types::Interior<'_>) -> Vec<Problem> {
    let mut problems = Vec::new();
    let entity_count = interior.entity_count();
    let room_count = interior.room_count() as u32;

    for (i, room) in interior.rooms().enumerate() {
        for index in room.attached_objects() {
            if index as usize >= entity_count {
                problems.push(Problem::AttachedObjectOutOfRange {
                    room: i,
                    index,
                    len: entity_count,
                });
            }
        }
    }

    for (i, portal) in interior.portals().enumerate() {
        for room in [portal.room_from(), portal.room_to()] {
            if room >= room_count {
                problems.push(Problem::PortalRoomOutOfRange {
                    portal: i,
                    room,
                    len: room_count as usize,
                });
            }
        }
        let corners = portal.corners().len();
        if corners != 4 {
            problems.push(Problem::PortalNotAQuad { portal: i, corners });
        }
        for index in portal.attached_objects() {
            if index as usize >= entity_count {
                problems.push(Problem::AttachedObjectOutOfRange {
                    room: i,
                    index,
                    len: entity_count,
                });
            }
        }
    }

    for (i, set) in interior.entity_sets().enumerate() {
        let locations = set.location_count();
        let entities = set.entity_count();
        if locations != entities {
            problems.push(Problem::EntitySetArraysDiffer {
                set: i,
                locations,
                entities,
            });
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::meta::{MetaInstance, Value};

    pub(crate) fn map_with_flags(parents: &[i32], children: &[u32], flags: &[u32]) -> MetaInstance {
        let items = (0..parents.len())
            .map(|i| {
                let mut ent = MetaInstance::new("CEntityDef");
                ent.set("parentIndex", Value::Int(parents[i] as i64));
                ent.set("numChildren", Value::UInt(children[i] as u64));
                ent.set("flags", Value::UInt(flags[i] as u64));
                Value::Pointer(Some(Box::new(ent)))
            })
            .collect();
        let mut root = MetaInstance::new("CMapData");
        root.set("entities", Value::Array(items));
        root
    }

    fn map(parents: &[i32], children: &[u32]) -> MetaInstance {
        map_with_flags(parents, children, &vec![0; parents.len()])
    }

    #[test]
    fn a_consistent_hierarchy_has_nothing_to_report() {
        let root = map(&[-1, 0, 0], &[2, 0, 0]);
        assert_eq!(check(&MapData::new(&root).unwrap()), vec![]);
    }

    /// ⚠ The index is into the same array, so one past the end is a read of
    /// whatever follows rather than a missing link.
    #[test]
    fn a_parent_index_past_the_end_is_caught() {
        let root = map(&[-1, 5], &[0, 0]);
        let problems = check(&MapData::new(&root).unwrap());
        assert!(problems.contains(&Problem::ParentOutOfRange {
            entity: 1,
            parent: 5,
            len: 2
        }));
    }

    #[test]
    fn an_index_into_the_parent_map_is_not_this_files_to_judge() {
        let root = map_with_flags(&[-1, 5], &[0, 0], &[0, 1 << 3]);
        assert_eq!(check(&MapData::new(&root).unwrap()), vec![]);
    }

    #[test]
    fn an_entity_cannot_be_its_own_parent() {
        let root = map(&[0], &[0]);
        let problems = check(&MapData::new(&root).unwrap());
        assert!(problems.contains(&Problem::ParentIsSelf { entity: 0 }));
    }

    #[test]
    fn a_cycle_is_found_rather_than_walked_forever() {
        let root = map(&[1, 0], &[1, 1]);
        let problems = check(&MapData::new(&root).unwrap());
        assert!(problems
            .iter()
            .any(|p| matches!(p, Problem::ParentCycle { .. })));
    }

    #[test]
    fn a_child_count_is_not_checked_from_one_file() {
        let root = map(&[-1, 0], &[7, 0]);
        let problems = check(&MapData::new(&root).unwrap());
        assert!(!problems
            .iter()
            .any(|p| matches!(p, Problem::ChildCountWrong { .. })));
    }

    #[test]
    fn a_problem_says_what_it_is_in_a_sentence() {
        let text = Problem::ParentOutOfRange {
            entity: 1,
            parent: 5,
            len: 2,
        }
        .to_string();
        assert!(text.contains("entity 1"), "{text}");
        assert!(text.contains('5'), "{text}");
    }
}
