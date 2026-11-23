use super::super::lights::tests::{block, model_of, ptr, put, put_floats};
use super::*;
use crate::meta::binary::rbm::schema::RbmDataBlock;

fn model(blocks: Vec<RbmDataBlock>) -> RbmModel {
    model_of(LAYOUTS.iter(), 1, blocks)
}

/// A counted list: its count twice, then its pointer.
fn counted(buf: &mut [u8], at: usize, pointer: u64, count: u32) {
    put(buf, at, &count.to_le_bytes());
    put(buf, at + 4, &count.to_le_bytes());
    put(buf, at + 8, &pointer.to_le_bytes());
}

fn root(pointer: u64, count: u32) -> Vec<u8> {
    let mut root = vec![0u8; 0x10];
    counted(&mut root, 0, pointer, count);
    root
}

fn put_address(buf: &mut [u8], at: usize, address: NodeAddress) {
    put(buf, at, &address.0.to_le_bytes());
    put(buf, at + 2, &address.1.to_le_bytes());
}

/// Node `k` of the three: links name it by (0, k), junctions by (7, k).
fn the_node(k: u16, first_link: u32) -> PathNode {
    PathNode {
        position: [f32::from(k), 10.0, -1.0],
        flags: 0x0004_0000_0000_0008 | u64::from(k),
        address: NodeAddress(0, k),
        hash_2522b86a: NodeAddress(9, k),
        street_name: 0x5700 | u32::from(k),
        junction_address: NodeAddress(7, k),
        first_link,
    }
}

fn node_bytes(n: &PathNode) -> Vec<u8> {
    let mut b = vec![0u8; NODE_SIZE];
    put_floats(&mut b, 0x00, &n.position);
    put(&mut b, 0x0C, &n.flags.to_le_bytes());
    put_address(&mut b, 0x14, n.address);
    put_address(&mut b, 0x18, n.junction_address);
    put_address(&mut b, 0x1C, n.hash_2522b86a);
    put(&mut b, 0x20, &n.street_name.to_le_bytes());
    put_address(&mut b, 0x34, n.junction_address);
    put(&mut b, 0x38, &n.first_link.to_le_bytes());
    b
}

fn the_nodes() -> Vec<PathNode> {
    vec![the_node(0, 0), the_node(1, 2), the_node(2, 3)]
}

fn nodes_model() -> RbmModel {
    let nodes: Vec<u8> = the_nodes().iter().flat_map(node_bytes).collect();
    model(vec![
        block(NODE_LIST, root(ptr(2, 0), 3)),
        block(NODE_ELEMENT, nodes),
    ])
}

fn the_links() -> Vec<Link> {
    let to = |k: u16, lanes: [u8; 2]| Link {
        target: NodeAddress(0, k),
        distance: 40 + k,
        lanes_from_other_node: lanes[0],
        lanes_to_other_node: lanes[1],
        flags: 0x400 | u32::from(k),
    };
    vec![to(1, [1, 1]), to(2, [2, 0]), to(0, [1, 1]), to(0, [0, 2])]
}

fn links_model() -> RbmModel {
    let links: Vec<u8> = (the_links().iter())
        .flat_map(|l| {
            let mut b = vec![0u8; LINK_SIZE];
            put_address(&mut b, 0x00, l.target);
            put(&mut b, 0x04, &l.distance.to_le_bytes());
            put(
                &mut b,
                0x06,
                &[l.lanes_from_other_node, l.lanes_to_other_node],
            );
            put(&mut b, 0x0C, &l.flags.to_le_bytes());
            b
        })
        .collect();
    // The list is the second block here: the root is found by its id, never as block 1.
    let mut model = model(vec![
        block(LINK, links),
        block(LINK_LIST, root(ptr(1, 0), 4)),
    ]);
    model.root_block_id = 2;
    model
}

fn the_junctions() -> Vec<Junction> {
    let turn = |k: u16| Turn {
        node: NodeAddress(7, k),
        path_length: 12.5 + f32::from(k),
        ends: [[1.0, 2.0, 3.0], [4.0, 5.0, f32::from(k)]],
    };
    vec![
        Junction {
            node: NodeAddress(7, 1),
            entrances: vec![Entrance {
                node: NodeAddress(7, 0),
                is_stop: true,
                is_yield: false,
                turns: vec![turn(1), turn(2)],
            }],
            phases: vec![
                Phase {
                    duration: 10.0,
                    light_boxes: Vec::new(),
                },
                Phase {
                    duration: 30.0,
                    light_boxes: vec![LightBox {
                        entity_id: 0x1122_3344_5566_7788,
                        box_id: 3,
                    }],
                },
            ],
        },
        Junction {
            node: NodeAddress(7, 2),
            entrances: Vec::new(),
            phases: Vec::new(),
        },
    ]
}

/// Block 2 the two junctions, 3 the entrance, 4 its turns, 5 the phases, 6 the light box.
fn junctions_model() -> RbmModel {
    let read = the_junctions();
    let mut junctions = vec![0u8; 2 * JUNCTION_SIZE];
    put_address(&mut junctions, 0x00, read[0].node);
    counted(&mut junctions, 0x08, ptr(3, 0), 1);
    counted(&mut junctions, 0x28, ptr(5, 0), 2);
    put_address(&mut junctions, JUNCTION_SIZE, read[1].node);
    let entrance = &read[0].entrances[0];
    let mut e = vec![0u8; ENTRANCE_SIZE];
    counted(&mut e, 0x08, ptr(4, 0), 2);
    put_address(&mut e, 0x18, entrance.node);
    e[0x47] = 1;
    let mut turns = vec![0u8; 2 * TURN_SIZE];
    for (k, t) in entrance.turns.iter().enumerate() {
        let b = &mut turns[k * TURN_SIZE..];
        put_address(b, 0x78, t.node);
        put_floats(b, 0x8C, &[t.path_length]);
        put_floats(b, 0x90, &t.ends[0]);
        put_floats(b, 0x9C, &t.ends[1]);
    }
    let mut phases = vec![0u8; 2 * PHASE_SIZE];
    put_floats(&mut phases, 0x00, &[10.0]);
    put_floats(&mut phases, PHASE_SIZE, &[30.0]);
    counted(&mut phases, PHASE_SIZE + 0x08, ptr(6, 0), 1);
    let mut light_box = vec![0u8; LIGHT_BOX_SIZE];
    put(&mut light_box, 0, &0x1122_3344_5566_7788u64.to_le_bytes());
    light_box[8] = 3;
    model(vec![
        block(JUNCTION_LIST, root(ptr(2, 0), 2)),
        block(JUNCTION, junctions),
        block(C_JUNCTION_ENTRANCE, e),
        block(TURN, turns),
        block(PHASE, phases),
        block(LIGHT_BOX, light_box),
    ])
}

#[test]
fn each_of_the_three_lists_reads_its_records() {
    assert_eq!(
        read(&nodes_model()).expect("the nodes read"),
        LaneBody::Nodes(the_nodes())
    );
    assert_eq!(
        read(&links_model()).expect("the links read"),
        LaneBody::Links(the_links())
    );
    assert_eq!(
        read(&junctions_model()).expect("the junctions read"),
        LaneBody::Junctions(the_junctions())
    );
    assert!(is_lane_root(NODE_LIST) && is_lane_root(LINK_LIST) && is_lane_root(JUNCTION_LIST));
    assert!(!is_lane_root(super::super::C_MAP_DATA));
}

#[test]
fn a_node_whose_position_is_not_finite_is_kept() {
    let mut m = nodes_model();
    put_floats(&mut m.data_blocks[1].data, 0x00, &[f32::NAN]);
    let LaneBody::Nodes(nodes) = read(&m).expect("it reads") else {
        panic!("the nodes");
    };
    assert!(nodes[0].position[0].is_nan());
    assert_eq!(nodes[1], the_node(1, 2));
}

#[test]
fn the_three_lists_join_into_each_node_s_links_and_the_node_an_address_names() {
    let graph = LaneGraph::join(the_nodes(), the_links(), the_junctions()).expect("they join");
    assert_eq!(graph.links_of(0), &the_links()[0..2]);
    assert_eq!(graph.links_of(1), &the_links()[2..3]);
    assert_eq!(graph.links_of(2), &the_links()[3..4]);
    assert!(graph.links_of(3).is_empty());
    assert_eq!(graph.target(&graph.links[1]), Some(2));
    assert_eq!(graph.junction_node(graph.junctions[0].node), Some(1));
    assert_eq!(graph.junction_node(NodeAddress(0, 1)), None);
    assert_eq!(graph.links_with_a_reverse(), 4);
    assert_eq!(graph.signalised(), 1);
    assert!(graph.junctions[0].is_signalised() && !graph.junctions[1].is_signalised());

    // Without the link from node 2 back to node 0, the link from 0 to 2 has no reverse.
    let mut nodes = the_nodes();
    nodes[2].first_link = 3;
    let graph = LaneGraph::join(nodes, the_links()[..3].to_vec(), Vec::new()).expect("they join");
    assert_eq!(graph.links_with_a_reverse(), 2);
}

#[test]
fn lists_that_do_not_join_are_refused_by_what_breaks() {
    let mut nodes = the_nodes();
    nodes[2].first_link = 1;
    let e = LaneGraph::join(nodes, the_links(), Vec::new()).expect_err("out of order");
    assert!(
        matches!(
            e,
            LaneError::FirstLink {
                node: 2,
                first: 1,
                links: 4
            }
        ),
        "{e}"
    );
    let mut nodes = the_nodes();
    nodes[2].first_link = 5;
    let e = LaneGraph::join(nodes, the_links(), Vec::new()).expect_err("past the links");
    assert!(matches!(e, LaneError::FirstLink { node: 2, .. }), "{e}");
    let mut links = the_links();
    links[3].target = NodeAddress(1, 0);
    let e = LaneGraph::join(the_nodes(), links, Vec::new()).expect_err("no such node");
    assert!(
        matches!(
            e,
            LaneError::LinkTarget {
                link: 3,
                target: NodeAddress(1, 0)
            }
        ),
        "{e}"
    );
    let mut junctions = the_junctions();
    junctions[1].node = NodeAddress(0, 2);
    let e = LaneGraph::join(the_nodes(), the_links(), junctions).expect_err("no such node");
    assert!(
        matches!(e, LaneError::JunctionNode { junction: 1, .. }),
        "{e}"
    );
}

#[test]
fn a_list_that_does_not_read_as_measured_refuses_the_body() {
    let mut m = nodes_model();
    put(&mut m.data_blocks[0].data, 4, &2u32.to_le_bytes());
    let e = read(&m).expect_err("two counts");
    assert!(e.to_string().contains("two counts differ"), "{e}");

    let mut m = nodes_model();
    m.data_blocks[1].data.truncate(2 * NODE_SIZE);
    let e = read(&m).expect_err("short");
    assert!(e.to_string().contains("do not fit a block"), "{e}");

    let mut m = nodes_model();
    m.data_blocks[1].data[0x18] = 0xEE;
    let e = read(&m).expect_err("two addresses");
    assert!(e.to_string().contains("not the one at 0x18"), "{e}");

    let mut m = links_model();
    m.data_blocks[0].data_type = TURN;
    let e = read(&m).expect_err("another class");
    assert!(e.to_string().contains("do not fit a block"), "{e}");

    // A turn list that names more records than every block of the body could hold.
    let mut m = junctions_model();
    put(&mut m.data_blocks[2].data, 0x08, &[0xFF, 0xFF, 0xFF, 0x7F]);
    put(&mut m.data_blocks[2].data, 0x0C, &[0xFF, 0xFF, 0xFF, 0x7F]);
    let e = read(&m).expect_err("too many");
    assert!(
        e.to_string().contains("more records than its blocks hold"),
        "{e}"
    );

    let mut m = nodes_model();
    m.data_blocks[0].data_type = super::super::C_MAP_DATA;
    let e = read(&m).expect_err("no lane body");
    assert!(matches!(e, LaneError::NotALaneBody(_)), "{e}");

    let mut m = nodes_model();
    let class = m.classes.get_mut(&C_PATH_NODE).expect("declared");
    class.data_size = 0x38;
    let e = read(&m).expect_err("another size");
    let sized = MapError::Size {
        class: "CPathNode",
        found: 0x38,
        expected: 0x34,
    };
    assert!(
        matches!(&e, LaneError::Body(b) if b.to_string() == sized.to_string()),
        "{e}"
    );
}
