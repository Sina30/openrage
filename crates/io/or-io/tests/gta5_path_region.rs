//! A `.ynd` authored as a plain Rust value, with invented content: two nodes
//! holding three links, one junction over a 3 x 1 patch of height samples and
//! its map entry, saved whole, reopened, and refused where its link counts do
//! not close on its links. Needs no game file.

use or_io::gta5::*;

fn node(index: u16, num_links: usize) -> CPathNode {
    let mut node = CPathNode {
        address: CNodeAddress {
            region: 7,
            index,
            ..Default::default()
        },
        coors_x: 100 * index as i16,
        ..Default::default()
    };
    node.set_num_links(num_links).unwrap();
    node
}

fn link(index: u16) -> CPathNodeLink {
    CPathNodeLink {
        other_node: CNodeAddress {
            region: 7,
            index,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn junction(x_samples: u8, y_samples: u8) -> CPathVirtualJunction {
    CPathVirtualJunction {
        max_z: 10,
        height_base_world: 0xFFFE,
        x_samples,
        y_samples,
        ..Default::default()
    }
}

fn authored(second_links: usize) -> CPathRegion {
    CPathRegion {
        first_node: Some(Box::new(PgBasePageMap {
            subtype: 2,
            ..Default::default()
        })),
        nodes: vec![node(0, 2), node(1, second_links)],
        num_nodes_car_nodes: 1,
        num_nodes_ped_nodes: 1,
        links: vec![link(1), link(0), link(0)],
        virtual_junctions: vec![junction(3, 1)],
        height_samples: vec![1, 2, 3],
        junction_map: JunctionMapContainer {
            junction_map: AtBinaryMap {
                sorted: 1,
                data: vec![AtBinaryMapDataPair {
                    key: 7 | (1 << 16),
                    data: 0,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

fn payload(file: &[u8]) -> Vec<u8> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let o = Rsc7::open(file).unwrap();
    let mut out = o.seal.header_bytes();
    out.extend_from_slice(&o.data);
    out
}

/// What a saved region states: each node's first link and each junction's first height sample.
fn starts(file: &[u8]) -> (Vec<i16>, Vec<u16>) {
    let peek = CPathRegion::peek(file).unwrap();
    let root = peek.root().unwrap();
    let nodes = root.nodes().unwrap();
    let junctions = root.virtual_junctions().unwrap();
    (
        nodes.iter().map(|n| n.start_index_of_links).collect(),
        junctions
            .iter()
            .map(|j| j.start_index_of_height_samples)
            .collect(),
    )
}

/// ⚠ The map's pairs follow three height samples, so the save places them
/// off four-byte alignment; the declaration is packed so a peek still opens.
#[test]
fn an_authored_ynd_saves_whole_reopens_and_reaches_a_fixed_point() {
    let region = authored(1);
    let first = region.to_bytes().unwrap();
    let again = CPathRegion::from_bytes(&first).unwrap();
    assert_eq!(again.nodes[1].num_links(), 1);
    assert_eq!(
        starts(&first).0,
        [0, 2],
        "the first links are written from the counts"
    );
    assert_eq!(again.links[0].other_node.index, 1);
    assert_eq!(again.virtual_junctions[0].num_height_samples(), 3);
    assert_eq!(again.height_samples, [1, 2, 3]);
    assert_eq!(again.junction_map.junction_map.data[0].address(), (7, 1));
    assert!(again.first_node.is_some());
    assert_eq!(payload(&again.to_bytes().unwrap()), payload(&first));

    let peek = CPathRegion::peek(&first).unwrap();
    let root = peek.root().unwrap();
    assert_eq!(
        (
            root.num_nodes,
            root.num_links,
            root.num_junctions,
            root.num_height_samples
        ),
        (2, 3, 1, 3),
        "the counts are written from the Vecs"
    );
    assert_eq!(
        { root.junction_map().junction_map().data().unwrap()[0].key },
        7 | (1 << 16)
    );
}

#[test]
fn a_region_whose_link_counts_do_not_close_on_its_links_is_refused() {
    let why = authored(2).to_bytes().unwrap_err().to_string();
    assert!(
        why.contains("the nodes' link counts sum to 4 and the region holds 3 links"),
        "{why}"
    );
}

/// The second node's first link, read as 2, goes stale when the first gives up a link.
#[test]
fn a_node_saves_the_links_before_it_as_its_first() {
    let mut region = CPathRegion::from_bytes(&authored(1).to_bytes().unwrap()).unwrap();
    region.nodes[0].set_as_integer2(0x0001_0005);
    region.nodes[0].set_num_links(1).unwrap();
    region.links.pop();
    let saved = region.to_bytes().unwrap();
    assert_eq!(starts(&saved).0, [0, 1]);
    let peek = CPathRegion::peek(&saved).unwrap();
    let word = peek
        .root()
        .unwrap()
        .nodes()
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .as_integer2;
    assert_eq!(word, 0x0001_0005 | (1 << 11), "the flags and the count");
}

/// The first junction's first sample, read as 0, goes stale when a junction is inserted before it.
#[test]
fn a_junction_saves_the_samples_before_it_as_its_first() {
    let mut region = CPathRegion::from_bytes(&authored(1).to_bytes().unwrap()).unwrap();
    region.virtual_junctions.insert(0, junction(2, 2));
    region.height_samples.splice(0..0, [9; 4]);
    let saved = region.to_bytes().unwrap();
    assert_eq!(starts(&saved).1, [0, 4]);
}

#[test]
fn a_node_s_link_count_and_flags_share_their_word_apart() {
    let mut n = node(0, 3);
    n.set_as_integer2(u32::MAX);
    assert_eq!(n.num_links(), 3, "the flags leave the count alone");
    n.set_num_links(31).unwrap();
    assert_eq!(
        n.as_integer2(),
        !(0x1f << 11),
        "the count leaves the flags alone"
    );
    let why = n.set_num_links(32).unwrap_err().to_string();
    assert!(why.contains("5 bits wide"), "{why}");
    assert_eq!(n.num_links(), 31, "a refused count changes nothing");
}
