//! GTA VI's world data on the command line: `rpf scan-world` counts every world-data body a scan
//! of an archive finds, a shard of the archive and a body at a time, and joins the lane graph;
//! `rpf scan-lights` lists the lights of one map body. Nothing is written.

use std::fs;
use std::ops::Range;

use or_io::container::rpf::rpf9_scan::Rpf9BodyReader;
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind, Rpf9Scan, Rpf9ScanCarry, Rpf9ZlibRun};
use or_io::gta6::map::lanes::{LaneBody, LaneGraph};
use or_io::gta6::map::lights::{self, LightAttr, LightOwner, MapLights};
use or_io::gta6::map::world::{Census, Counted};
use or_io::gta6::map::{self, MapBody};

const WORLD_USAGE: &str =
    "usage: rpf scan-world <archive.rpf> [--from <offset>] [--to <offset>] [--limit <bytes>] [--shard <bytes>] [--list]";
const LIGHTS_USAGE: &str = "usage: rpf scan-lights <archive.rpf> <offset> [--limit <bytes>]";

/// The bytes of the archive one shard scans, unless `--shard` says another size.
const SHARD: u64 = 2 << 30;

/// The ranges of at most `size` bytes that cover `start` to `end` in order.
fn shards(start: u64, end: u64, size: u64) -> impl Iterator<Item = Range<u64>> {
    let size = size.max(1);
    (0..)
        .map(move |k: u64| start.saturating_add(k.saturating_mul(size)))
        .take_while(move |at| *at < end)
        .map(move |at| at..at.saturating_add(size).min(end))
}

fn point(p: [f32; 3]) -> String {
    format!("({:.2}, {:.2}, {:.2})", p[0], p[1], p[2])
}

/// What a walk keeps beside its counts: the three lists of the lane graph, which are small.
#[derive(Default)]
struct Kept {
    nodes: Option<Vec<map::lanes::PathNode>>,
    links: Option<Vec<map::lanes::Link>>,
    junctions: Option<Vec<map::lanes::Junction>>,
    /// The bodies whose bytes did not read, so what they are is not known.
    not_read: u64,
}

impl Kept {
    /// Count the body at `offset` into `census` and say it in one line; `None` for a body that is
    /// no world data.
    fn body(
        &mut self,
        bodies: &mut Rpf9BodyReader<&fs::File>,
        offset: u64,
        limit: u64,
        census: &mut Census,
    ) -> Option<String> {
        let (find, bytes) = match bodies.read(offset, limit) {
            Ok(read) => read,
            Err(e) => {
                self.not_read += 1;
                return Some(format!("  {offset:#012X}  not read: {e}"));
            }
        };
        let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            resource: Some(resource),
            ..
        }) = find.kind
        else {
            return None;
        };
        let (virtual_section, physical) = resource.sections(&bytes);
        let said = match census.add_body(virtual_section, physical)? {
            Ok(Counted::Map(MapBody::Data(m))) => {
                format!("CMapData      {} entities", m.entities.len())
            }
            Ok(Counted::Map(MapBody::Types(t))) => {
                format!("CMapTypes     {} archetypes", t.archetypes.len())
            }
            Ok(Counted::Group(g)) => {
                format!("entity group  {:#010X}, {} entities", g.name, g.entities)
            }
            Ok(Counted::Lanes(body)) => self.keep(body),
            Err(e) => format!("unread: {e}"),
        };
        Some(format!("  {offset:#012X}  {said}"))
    }

    fn keep(&mut self, body: LaneBody) -> String {
        match body {
            LaneBody::Nodes(n) => {
                let said = format!("lane nodes    {}", n.len());
                self.nodes = Some(n);
                said
            }
            LaneBody::Links(l) => {
                let said = format!("lane links    {}", l.len());
                self.links = Some(l);
                said
            }
            LaneBody::Junctions(j) => {
                let said = format!("junctions     {}", j.len());
                self.junctions = Some(j);
                said
            }
        }
    }

    /// The three lists joined, said in lines; with whether they are all there and do not join.
    fn graph(self) -> (Vec<String>, bool) {
        let (Some(nodes), Some(links), Some(junctions)) = (self.nodes, self.links, self.junctions)
        else {
            return (
                vec![
                    "  lane graph    not joined: the walk did not find its three lists".to_string(),
                ],
                false,
            );
        };
        match LaneGraph::join(nodes, links, junctions) {
            Ok(graph) => (graph_lines(&graph), false),
            Err(e) => (vec![format!("  lane graph    does not join: {e}")], true),
        }
    }
}

fn graph_lines(graph: &LaneGraph) -> Vec<String> {
    let standing = (graph.nodes.iter())
        .filter(|n| n.position.iter().all(|x| x.is_finite()))
        .count();
    let entrances: usize = graph.junctions.iter().map(|j| j.entrances.len()).sum();
    let turns: usize = (graph.junctions.iter())
        .flat_map(|j| &j.entrances)
        .map(|e| e.turns.len())
        .sum();
    let phases: usize = graph.junctions.iter().map(|j| j.phases.len()).sum();
    vec![
        format!(
            "  lane graph    joined: {} nodes ({standing} at a finite position), {} links, {} of them with a link back",
            graph.nodes.len(),
            graph.links.len(),
            graph.links_with_a_reverse()
        ),
        format!(
            "  junctions     {} ({} signalised), {entrances} entrances, {turns} turns, {phases} traffic-light phases",
            graph.junctions.len(),
            graph.signalised()
        ),
    ]
}

/// One line of what a shard, or the whole walk, counted.
fn counts_line(c: &Census) -> String {
    format!(
        "{} maps, {} entities, {} lights, {} interiors, {} entity groups, {} unread",
        c.maps,
        c.entities,
        c.lights(),
        c.interiors,
        c.entity_groups,
        c.unread
    )
}

fn summary(c: &Census, not_read: u64) -> Vec<String> {
    vec![
        format!("summary: {}", counts_line(c)),
        format!(
            "  maps          {} CMapData, {} of them with entities; {} CMapTypes; {} bodies whose bytes did not read",
            c.maps, c.placement_maps, c.map_types, not_read
        ),
        format!(
            "  entities      {} (CMloInstanceDef {}); {} prop instances, {} grass instances",
            c.entities, c.interior_instances, c.prop_instances, c.grass_instances
        ),
        format!(
            "  archetypes    {} (CMloArchetypeDef {})",
            c.archetypes, c.interiors
        ),
        format!(
            "  lights        {} in {} effects: {} on entities, {} on archetypes, {} on the entities of interiors, {} on those of their entity sets; the lights of {} map bodies did not read",
            c.lights(),
            c.light_effects,
            c.entity_lights,
            c.archetype_lights,
            c.interior_lights,
            c.entity_set_lights,
            c.lights_unread
        ),
        format!(
            "  entity groups {} of {} entities, each in its group's own space",
            c.entity_groups, c.group_entities
        ),
        format!(
            "  lane lists    {} nodes, {} links, {} junctions ({} signalised)",
            c.lane_nodes, c.lane_links, c.junctions, c.signalised_junctions
        ),
    ]
}

pub(crate) fn cmd_scan_world(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let (from, args) = super::rpf::split_offset_flag(&args, "--from")?;
    let (to, args) = super::rpf::split_offset_flag(&args, "--to")?;
    let (shard, args) = super::rpf::split_offset_flag(&args, "--shard")?;
    let list = args.iter().any(|a| a == "--list");
    let rest: Vec<&String> = args.iter().filter(|a| a.as_str() != "--list").collect();
    let [path] = rest.as_slice() else {
        return Err(WORLD_USAGE.to_string());
    };
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("metadata({path}): {e}"))?
        .len();
    let (start, end) = (from.unwrap_or(0), to.unwrap_or(len).min(len));
    let cancelled = super::jobs::on_interrupt();
    let mut whole = Census::default();
    let mut kept = Kept::default();
    let mut carry = Rpf9ScanCarry::default();
    // One window for every body: a body that follows the last closely is read from what is held.
    let mut bodies = Rpf9BodyReader::new(&file).map_err(|e| e.to_string())?;
    // A body that starts in a shard may end past it: the next shard starts no earlier than that.
    let mut reached = start;
    for shard in shards(start, end, shard.unwrap_or(SHARD)) {
        let from = shard.start.max(reached);
        if from >= shard.end {
            continue;
        }
        let mut scan = Rpf9Scan::new(&file, from..shard.end)
            .map_err(|e| e.to_string())?
            .carrying(carry);
        let mut census = Census::default();
        for find in scan.by_ref() {
            if cancelled.is_set() {
                return Err("cancelled".to_string());
            }
            let find = find.map_err(|e| e.to_string())?;
            let starts = matches!(
                find.kind,
                Rpf9BodyKind::Zlib(Rpf9ZlibRun {
                    resource: Some(_),
                    signature: None,
                    ..
                })
            );
            if starts && find.stored_len.is_some() {
                let line = kept.body(&mut bodies, find.offset, limit, &mut census);
                if let Some(line) = line.filter(|_| list) {
                    println!("{line}");
                }
            }
        }
        (carry, reached) = (scan.carry(), scan.position());
        println!(
            "shard {from:#012X} .. {:#012X}  {}",
            shard.end,
            counts_line(&census)
        );
        whole.merge(&census);
    }
    summary(&whole, kept.not_read)
        .iter()
        .for_each(|l| println!("{l}"));
    let (said, failed) = kept.graph();
    said.iter().for_each(|l| println!("{l}"));
    match failed {
        true => Err("the lane graph's three lists do not join".to_string()),
        false => Ok(()),
    }
}

fn light_line(light: &LightAttr) -> String {
    let [r, g, b] = light.colour;
    format!(
        "{:<8} posn {} colour #{r:02X}{g:02X}{b:02X} intensity {:.2} falloff {:.2} exponent {:.2} cone {:.1}/{:.1} direction {} timeFlags {:#08X} flags {:#X}",
        light.light_type_name().map_or_else(|| format!("type {}", light.light_type), String::from),
        point(light.posn),
        light.intensity,
        light.falloff,
        light.falloff_exponent,
        light.cone_inner_angle,
        light.cone_outer_angle,
        point(light.direction),
        light.time_flags,
        light.flags
    )
}

/// The lights of a map body, an effect and then a light a line; an entity's own lights with where
/// they stand in the world.
fn lights_lines(body: &MapBody, found: &MapLights) -> Vec<String> {
    let mut out = vec![format!(
        "  lights        {} in {} effects; {} extensions of other classes",
        found.lights(),
        found.effects.len(),
        found.other_extensions
    )];
    for (owner, effect) in &found.effects {
        let (held, entity) = match (owner, body) {
            (LightOwner::Entity(k), MapBody::Data(m)) => {
                (format!("entity {k}"), m.entities.get(*k))
            }
            (LightOwner::Entity(k), _) => (format!("entity {k}"), None),
            (LightOwner::Archetype(k), _) => (
                format!("archetype {k}, once at each entity that places it"),
                None,
            ),
            (
                LightOwner::InteriorEntity {
                    archetype,
                    entity,
                    pose,
                },
                _,
            ) => (
                format!(
                    "entity {entity} of interior archetype {archetype}, at {} of the interior's space",
                    point(pose.position)
                ),
                None,
            ),
            (
                LightOwner::EntitySetEntity {
                    archetype,
                    set,
                    entity,
                    pose,
                },
                _,
            ) => (
                format!(
                    "entity {entity} of entity set {set} of interior archetype {archetype}, at {} of the interior's space",
                    point(pose.position)
                ),
                None,
            ),
        };
        out.push(format!(
            "    effect {:#010X} of {held}: offsetPosition {}",
            effect.name,
            point(effect.offset_position)
        ));
        for light in effect.lights() {
            let at = entity.map_or_else(String::new, |e| {
                format!(" world {}", point(lights::place(effect, light, e).position))
            });
            out.push(format!("      {}{at}", light_line(light)));
        }
    }
    out
}

pub(crate) fn cmd_scan_lights(args: &[String]) -> Result<(), String> {
    let (limit, args) = super::rpf::split_limit(args)?;
    let [path, offset] = args.as_slice() else {
        return Err(LIGHTS_USAGE.to_string());
    };
    let offset = super::rpf::parse_offset(offset)?;
    let file = fs::File::open(path).map_err(|e| format!("open({path}): {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    let Rpf9BodyKind::Zlib(Rpf9ZlibRun {
        resource: Some(resource),
        ..
    }) = find.kind
    else {
        return Err(format!("no headerless resource starts at {offset:#X}"));
    };
    let (virtual_section, physical) = resource.sections(&bytes);
    let body = map::open(virtual_section, physical)
        .map_err(|e| format!("GTA VI's map reader refuses this body: {e}"))?;
    let found = lights::open(virtual_section, physical)
        .map_err(|e| format!("GTA VI's light reader refuses this body: {e}"))?;
    lights_lines(&body, &found)
        .iter()
        .for_each(|l| println!("{l}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta6::map::lanes::{Junction, Link, NodeAddress, PathNode, Phase};
    use or_io::gta6::map::lights::LightEffect;
    use or_io::gta6::map::{Aabb, Entity, MapData};

    #[test]
    fn shards_cover_a_range_in_order_and_the_last_is_cut_at_its_end() {
        let all: Vec<Range<u64>> = shards(10, 35, 10).collect();
        assert_eq!(all, [10..20, 20..30, 30..35]);
        assert_eq!(shards(5, 5, 10).count(), 0);
        assert_eq!(shards(0, 3, 0).count(), 3, "a size of 0 is taken as 1");
        let last = shards(u64::MAX - 4, u64::MAX, 3).last();
        assert_eq!(last, Some(u64::MAX - 1..u64::MAX));
    }

    #[test]
    fn scan_world_and_scan_lights_say_their_usage() {
        assert_eq!(cmd_scan_world(&[]), Err(WORLD_USAGE.to_string()));
        assert_eq!(
            cmd_scan_world(&["--list".to_string()]),
            Err(WORLD_USAGE.to_string())
        );
        assert_eq!(cmd_scan_lights(&[]), Err(LIGHTS_USAGE.to_string()));
        assert_eq!(
            cmd_scan_lights(&["a.rpf".to_string()]),
            Err(LIGHTS_USAGE.to_string())
        );
    }

    #[test]
    fn a_summary_says_the_six_counts_first_and_then_each_in_full() {
        let census = Census {
            maps: 7,
            placement_maps: 3,
            map_types: 2,
            entities: 40,
            interior_instances: 1,
            archetypes: 9,
            interiors: 4,
            light_effects: 5,
            entity_lights: 6,
            archetype_lights: 10,
            interior_lights: 2,
            entity_set_lights: 4,
            lights_unread: 1,
            entity_groups: 8,
            group_entities: 30,
            lane_nodes: 3,
            lane_links: 4,
            junctions: 2,
            signalised_junctions: 1,
            unread: 11,
            ..Census::default()
        };
        assert_eq!(
            counts_line(&census),
            "7 maps, 40 entities, 22 lights, 4 interiors, 8 entity groups, 11 unread"
        );
        let text = summary(&census, 12).join("\n");
        for wanted in [
            "summary: 7 maps, 40 entities, 22 lights",
            "7 CMapData, 3 of them with entities; 2 CMapTypes; 12 bodies whose bytes did not read",
            "22 in 5 effects: 6 on entities, 10 on archetypes, 2 on the entities of interiors, 4 on those of their entity sets; the lights of 1 map bodies",
            "8 of 30 entities",
            "3 nodes, 4 links, 2 junctions (1 signalised)",
        ] {
            assert!(text.contains(wanted), "{wanted}\n{text}");
        }
    }

    fn node(k: u16, first_link: u32) -> PathNode {
        PathNode {
            position: [f32::from(k), 0.0, 0.0],
            flags: 0,
            address: NodeAddress(0, k),
            hash_2522b86a: NodeAddress(0, 0),
            street_name: 0,
            junction_address: NodeAddress(5, k),
            first_link,
        }
    }

    fn link(to: u16) -> Link {
        Link {
            target: NodeAddress(0, to),
            distance: 10,
            lanes_from_other_node: 1,
            lanes_to_other_node: 1,
            flags: 0,
        }
    }

    #[test]
    fn the_lane_graph_is_joined_once_its_three_lists_are_found_and_a_broken_one_fails() {
        let junction = |k: u16, phases: usize| Junction {
            node: NodeAddress(5, k),
            entrances: Vec::new(),
            phases: vec![
                Phase {
                    duration: 10.0,
                    light_boxes: Vec::new()
                };
                phases
            ],
        };
        let mut kept = Kept::default();
        let mut nodes = vec![node(0, 0), node(1, 1)];
        nodes[1].position[0] = f32::NAN;
        assert_eq!(kept.keep(LaneBody::Nodes(nodes)), "lane nodes    2");
        let (said, failed) = Kept {
            nodes: kept.nodes.clone(),
            ..Kept::default()
        }
        .graph();
        assert!(
            !failed && said[0].contains("did not find its three lists"),
            "{said:?}"
        );
        kept.keep(LaneBody::Links(vec![link(1), link(0)]));
        kept.keep(LaneBody::Junctions(vec![junction(0, 2), junction(1, 0)]));
        let broken = Kept {
            nodes: kept.nodes.clone(),
            links: Some(vec![link(7)]),
            junctions: Some(Vec::new()),
            not_read: 0,
        };
        let (said, failed) = kept.graph();
        assert!(!failed, "{said:?}");
        let text = said.join("\n");
        for wanted in [
            "joined: 2 nodes (1 at a finite position), 2 links, 2 of them with a link back",
            "2 (1 signalised), 0 entrances, 0 turns, 2 traffic-light phases",
        ] {
            assert!(text.contains(wanted), "{wanted}\n{text}");
        }
        let (said, failed) = broken.graph();
        assert!(failed && said[0].contains("does not join"), "{said:?}");
    }

    fn light(light_type: u8) -> LightAttr {
        LightAttr {
            hash_30eca388: [1.0, 0.0, 0.0],
            posn: [1.0, 0.0, 0.0],
            colour: [255, 128, 0],
            vol_outer_colour: [0; 3],
            intensity: 2.0,
            flags: 0x40,
            time_flags: 0xFF_FFFF,
            falloff: 12.0,
            falloff_exponent: 8.0,
            vol_intensity: 0.0,
            vol_size_scale: 0.0,
            vol_outer_intensity: 0.0,
            vol_outer_exponent: 0.0,
            corona_size: 0.0,
            corona_intensity: 0.0,
            corona_z_bias: 0.0,
            shadow_near_clip: 0.0,
            direction: [0.0, 0.0, -1.0],
            tangent: [1.0, 0.0, 0.0],
            cone_inner_angle: 20.0,
            cone_outer_angle: 45.0,
            extents: [0.0; 3],
            flashiness: 0,
            light_fade_distance: 0,
            shadow_fade_distance: 0,
            specular_fade_distance: 0,
            volumetric_fade_distance: 0,
            light_type,
            group_id: 0,
            shadow_blur: 0,
            light_hash: 0,
        }
    }

    #[test]
    fn a_map_s_lights_are_listed_by_owner_and_an_entity_s_own_with_where_they_stand() {
        let entity = Entity {
            interior: false,
            id: 1,
            rotation: [0.0, 0.0, 0.0, 1.0],
            archetype_word: 2,
            position: [100.0, 200.0, 5.0],
            archetype_name: 3,
            flags: 0,
            scale_xy: 2.0,
            scale_z: 1.0,
            parent_index: -1,
            hash_e74c3c23: 0,
            lod_dist: -1.0,
            child_lod_dist: 0.0,
            num_children: 0,
            tint_value: 0,
            priority_level: 0,
            lod_level: 5,
        };
        let unit = Aabb {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let body = MapBody::Data(MapData {
            name: 1,
            name_word: 1,
            parent: 0,
            hash_6c0f9c8c: 0,
            hash_9e8d32a0: 0,
            hash_ec16451d: 0,
            hash_36c898aa: 0,
            flags: 0,
            content_flags: 0,
            streaming_extents: unit,
            entities_extents: unit,
            entities: vec![entity],
            prop_lists: Vec::new(),
            grass_lists: Vec::new(),
        });
        let effect = |instances| LightEffect {
            name: 0xE1,
            offset_position: [0.0, 1.0, 0.0],
            instances,
            hash_c4e96a00: Vec::new(),
        };
        let found = MapLights {
            effects: vec![
                (LightOwner::Entity(0), effect(vec![light(2), light(32)])),
                (LightOwner::Archetype(4), effect(vec![light(1)])),
            ],
            other_extensions: 3,
        };
        let text = lights_lines(&body, &found).join("\n");
        for wanted in [
            "3 in 2 effects; 3 extensions of other classes",
            "effect 0x000000E1 of entity 0: offsetPosition (0.00, 1.00, 0.00)",
            "spot     posn (1.00, 0.00, 0.00) colour #FF8000 intensity 2.00 falloff 12.00",
            "world (102.00, 202.00, 5.00)",
            "type 32  posn",
            "of archetype 4, once at each entity that places it",
        ] {
            assert!(text.contains(wanted), "{wanted}\n{text}");
        }
        assert_eq!(text.matches(" world ").count(), 2, "{text}");
    }
}
