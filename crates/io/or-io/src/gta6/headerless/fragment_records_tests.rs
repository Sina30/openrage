use std::ops::Range;

use super::*;
use crate::gta6::headerless::bytes::sections;

const HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// A virtual section of 16-aligned records, its first 0x100 bytes a fragment's root.
struct Image(Vec<u8>);

impl Image {
    fn new() -> Self {
        Self(vec![0; 0x100])
    }
    fn alloc(&mut self, len: usize) -> usize {
        let at = self.0.len().next_multiple_of(16);
        self.0.resize(at + len, 0);
        at
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn f32s(&mut self, at: usize, v: &[f32]) {
        for (k, x) in v.iter().enumerate() {
            self.put(at + 4 * k, &x.to_le_bytes());
        }
    }
    fn ptr(&mut self, at: usize, target: usize) {
        self.put(at, &(0x5000_0000u64 + target as u64).to_le_bytes());
    }
    /// An array slot `{pointer, n, n, 0}` at `at` to a new array of `n` elements of `stride`.
    fn slot(&mut self, at: usize, n: u16, stride: usize) -> usize {
        let target = self.alloc(usize::from(n) * stride);
        self.ptr(at, target);
        self.u16(at + 8, n);
        self.u16(at + 0xA, n);
        target
    }
}

fn read<T>(img: &Image, f: impl FnOnce(Bytes<'_>) -> Result<T>) -> Result<T> {
    sections(&img.0, None, |v, _| f(v))
}

fn bone(name: &str, rotation: [f32; 4], translation: [f32; 3]) -> Bone {
    Bone {
        name: name.to_string(),
        hash: 0,
        parent: None,
        rotation,
        translation,
        scale: [1.0; 3],
        inverse_bind: [[0.0; 3]; 4],
    }
}

fn group(bone: u16, children: Range<u16>) -> FragmentGroup {
    FragmentGroup {
        bone,
        bone_hash: 0,
        parent: None,
        child_groups: 0..0,
        second_set: children.end..children.end,
        children,
    }
}

/// A window of `child` whose header states `width` and the length its `rows` encode to.
fn window(child: u16, width: u16, rows: Vec<GlassRow>) -> GlassWindow {
    let mut bytes = Vec::new();
    rows.iter().for_each(|r| r.write(&mut bytes));
    let mut header = [0u8; WINDOW];
    header[..4].copy_from_slice(&0.25f32.to_le_bytes());
    header[0x40..0x44].copy_from_slice(b"CWGV");
    header[0x44..0x46].copy_from_slice(&child.to_le_bytes());
    header[0x46] = 7;
    header[0x48..0x4A].copy_from_slice(&width.to_le_bytes());
    header[0x4A..0x4C].copy_from_slice(&(rows.len() as u16).to_le_bytes());
    let map = (2 * rows.len() + bytes.len()) as u16;
    header[0x4C..0x4E].copy_from_slice(&map.to_le_bytes());
    GlassWindow { header, rows }
}

fn glass() -> VehicleGlass {
    let runs = GlassRow::Runs {
        start1: 2,
        data1: vec![0xF0, 0x80, 0x11],
        start2: 9,
        data2: vec![0x40, 0x22],
    };
    let one = GlassRow::Runs {
        start1: 0,
        data1: vec![0x33],
        start2: NO_RUN,
        data2: Vec::new(),
    };
    let rows = vec![GlassRow::Blank, runs, GlassRow::Empty { start: 5 }, one];
    let wide = vec![
        GlassRow::Raw(vec![5, 0x76, 1, 2, 3]),
        GlassRow::Raw(vec![0xFF]),
    ];
    VehicleGlass {
        stride: 0x70,
        after_offsets: 0,
        windows: vec![
            window(3, 12, rows),
            window(6, 300, wide),
            window(9, 4, Vec::new()),
        ],
    }
}

/// The section with `block` at its own record, and where that is.
fn image_of(block: &[u8]) -> (Image, usize) {
    let mut img = Image::new();
    let at = img.alloc(block.len());
    img.put(at, block);
    (img, at)
}

#[test]
fn a_vehicle_glass_block_reads_each_row_form_and_encodes_to_its_bytes() {
    let glass = glass();
    let block = glass.encode();
    // Three windows take four table slots, the last (0, 0); the first window follows the word
    // after the table.
    assert_eq!(&block[..4], b"HWGV");
    assert_eq!(u32_at(&block, 8) as usize, block.len());
    let first = 12 + 8 * 4 + 4;
    assert_eq!((u32_at(&block, 12), u32_at(&block, 16)), (3, first as u32));
    assert_eq!((u32_at(&block, 36), u32_at(&block, 40)), (0, 0));
    assert_eq!(&block[first + 0x40..first + 0x44], b"CWGV");
    // The rows' offsets run from the end of the table: one byte, then 2 + 3 + 1 + 1 + 2, then 2.
    let table = first + WINDOW;
    let offsets: Vec<u16> = (0..4).map(|i| le16(&block, table + 2 * i)).collect();
    assert_eq!(offsets, [0, 1, 1 + 9, 1 + 9 + 2]);
    assert_eq!(block.len() % 16, 0);

    let (img, at) = image_of(&block);
    let read = read(&img, |v| vehicle_glass(v, at, 10)).expect("the block reads");
    assert_eq!(read, glass);
    assert_eq!(
        read.windows
            .iter()
            .map(GlassWindow::child)
            .collect::<Vec<_>>(),
        [3, 6, 9]
    );
}

#[test]
fn a_vehicle_glass_block_that_is_not_what_its_windows_encode_to_is_refused() {
    let block = glass().encode();
    let refused = |block: &[u8], children: usize| {
        let (img, at) = image_of(block);
        read(&img, |v| vehicle_glass(v, at, children)).is_err()
    };
    assert!(!refused(&block, 10));
    // A child the fragment lacks.
    assert!(refused(&block, 9));
    // A table entry that is not its window's place.
    let mut moved = block.clone();
    moved[16] += 0x10;
    assert!(refused(&moved, 10));
    // A byte of the padding after the first window's 4 offsets and 16 bytes of rows set.
    let mut padded = block.clone();
    let end = 12 + 8 * 4 + 4 + WINDOW + 2 * 4 + 16;
    assert_ne!(end % 16, 0);
    padded[end] = 1;
    assert!(refused(&padded, 10));
    // A window without its magic.
    let mut magic = block.clone();
    magic[12 + 8 * 4 + 4 + 0x40] = b'X';
    assert!(refused(&magic, 10));
    // A total length past the block.
    let mut long = block.clone();
    long[8..12].copy_from_slice(&(block.len() as u32 + 0x10).to_le_bytes());
    assert!(refused(&long, 10));
    // Windows whose children do not ascend.
    let mut glass = glass();
    glass.windows.swap(0, 1);
    assert!(refused(&glass.encode(), 10));
}

/// A pane's base, width, height, outlines' vertex counts and owner word.
type Spec = ([f32; 3], [f32; 3], [f32; 3], Vec<u32>, u32);

/// A root whose list at 0x98 points at `panes`.
fn panes_image(panes: &[Spec]) -> (Image, usize) {
    let mut img = Image::new();
    let list = img.alloc(8 * panes.len());
    img.ptr(0x98, list);
    img.0[0xDA] = panes.len() as u8;
    for (k, (base, width, height, outlines, owner)) in panes.iter().enumerate() {
        let at = img.alloc(PANE);
        img.ptr(list + 8 * k, at);
        img.f32s(at, base);
        img.f32s(at + 0x10, width);
        img.f32s(at + 0x20, height);
        img.f32s(at + 0x30, &[0.125, 0.25, 0.75, 0.875]);
        img.u32(at + 0x40, outlines.len() as u32);
        for (j, n) in outlines.iter().enumerate() {
            img.u32(at + 0x44 + 4 * j, *n);
        }
        img.u32(at + 0x170, *owner);
    }
    (img, list)
}

#[test]
fn a_glass_pane_s_corners_are_placed_by_its_owner_bone_and_a_shared_owner_word_is_unread() {
    // Bone 1 is a quarter turn about z at (1, 2, 3): (x, y, z) goes to (1 - y, 2 + x, 3 + z).
    let bones = [
        bone("chassis", [0.0, 0.0, 0.0, 1.0], [0.0; 3]),
        bone("window_lf", [0.0, 0.0, HALF, HALF], [1.0, 2.0, 3.0]),
    ];
    let worlds = bind_worlds(&bones);
    let groups = [group(0, 0..1), group(1, 1..2)];
    let (base, width, height) = ([0.5, 0.0, 0.0], [0.0, 0.25, 0.0], [0.0, 0.0, 0.75]);
    let (img, list) = panes_image(&[
        (base, width, height, vec![4], 1),
        (base, height, width, vec![4, 5], 99),
    ]);
    let read = read(&img, |v| panes(v, list, &groups, &worlds)).expect("the panes read");
    assert_eq!(read.len(), 2);
    assert_eq!((read[0].owner, read[1].owner), (Some(1), None));
    assert_eq!(
        (read[0].uv_min, read[0].uv_max),
        ([0.125, 0.25], [0.75, 0.875])
    );
    assert_eq!(read[1].outlines, [4, 5]);
    assert_eq!(read[1].corners, None);
    let placed = |[x, y, z]: [f32; 3]| [1.0 - y, 2.0 + x, 3.0 + z];
    let local = [
        [0.5, 0.0, 0.0],
        [0.5, 0.25, 0.0],
        [0.5, 0.25, 0.75],
        [0.5, 0.0, 0.75],
    ];
    let corners = read[0].corners.expect("an owner places its corners");
    for (got, want) in corners.iter().zip(local.map(placed)) {
        assert!(
            (0..3).all(|a| (got[a] - want[a]).abs() < 1e-5),
            "{corners:?}"
        );
    }

    let refused = |pane: Spec| {
        let (img, list) = panes_image(&[pane]);
        sections(&img.0, None, |v, _| panes(v, list, &groups, &worlds)).is_err()
    };
    // One outline and an owner the fragment lacks.
    assert!(refused((base, width, height, vec![4], 2)));
    // Edges that are not perpendicular.
    assert!(refused((base, width, [0.0, 0.5, 0.0], vec![4], 1)));
    // An outline of two vertices.
    assert!(refused((base, width, height, vec![2], 1)));
}

#[test]
fn self_collision_pairs_name_groups_once_each() {
    let pairs = |list: &[[u16; 2]]| {
        let mut img = Image::new();
        let at = img.alloc(0x10);
        img.u32(at, list.len() as u32);
        img.u32(at + 4, list.len() as u32);
        let to = img.alloc(4 * list.len());
        img.ptr(at + 8, to);
        for (k, [a, b]) in list.iter().enumerate() {
            img.u16(to + 4 * k, *a);
            img.u16(to + 4 * k + 2, *b);
        }
        read(&img, |v| self_collision(v, at, 3))
    };
    let list = [[0, 1], [2, 0], [1, 2]];
    assert_eq!(pairs(&list).expect("the pairs read"), list);
    assert!(pairs(&[[0, 1], [0, 1]]).is_err());
    assert!(pairs(&[[0, 3]]).is_err());
}

#[test]
fn group_links_read_up_to_their_count_and_the_rest_is_the_fill() {
    let links = |fill: f32| {
        let mut img = Image::new();
        let at = img.alloc(0x10);
        let entries = img.alloc(3 * 0x18);
        img.ptr(at, entries);
        img.u16(at + 8, 2);
        img.u16(at + 0xA, 3);
        for (k, (a, b, value)) in [(0u16, 2u16, 1.0f32 / 128.0), (1, 0, 0.5)]
            .iter()
            .enumerate()
        {
            img.u16(entries + 0x18 * k + 0x10, *a);
            img.u16(entries + 0x18 * k + 0x12, *b);
            img.f32s(entries + 0x18 * k + 0x14, &[*value]);
        }
        img.f32s(entries + 0x30, &[fill]);
        img.u32(entries + 0x34, 1);
        read(&img, |v| group_links(v, at, 3))
    };
    let read = links(UNUSED_LINK).expect("the links read");
    assert_eq!(read.capacity, 3);
    let pairs: Vec<([u16; 2], f32)> = read.links.iter().map(|l| (l.groups, l.value)).collect();
    assert_eq!(pairs, [([0, 2], 1.0 / 128.0), ([1, 0], 0.5)]);
    assert!(links(5.0).is_err());
}

/// Vertex `x * 4 + y * 2 + z` of a 3 by 2 by 2 grid, each axis on its own scale.
fn grid(i: u16) -> [f32; 3] {
    let (x, y, z) = (i / 4, (i / 2) % 2, i % 2);
    [
        1.5 * f32::from(x),
        0.25 + 0.5 * f32::from(y),
        -1.0 + 2.0 * f32::from(z),
    ]
}

/// A lattice of two cells side by side along x over two bones, and its vehicle record when
/// `vehicle`; `back` is what cell 1 states across its face 0.
fn lattice_image(vehicle: bool, back: u16, slack: f32) -> (Image, usize, Option<usize>) {
    let mut img = Image::new();
    let at = img.alloc(0x150);
    img.u16(at, 0x2A3);
    img.u16(at + 2, if vehicle { 0 } else { 2 });
    img.u32(at + 4, 2);
    img.u32(at + 8, 3);
    img.slot(at + 0x20, 2, 0x30);
    img.slot(at + 0x30, 3, 0x20);
    let vertices = img.slot(at + 0x40, 12, 12);
    for i in 0..12u16 {
        img.f32s(vertices + 12 * usize::from(i), &grid(i));
    }
    img.slot(at + 0x50, 2, 2);
    let cells = img.slot(at + 0x60, 2, 0x20);
    let boxes = img.slot(at + 0xD0, 2, 0x20);
    for c in 0..2u16 {
        let e = cells + 0x20 * usize::from(c);
        let corners: Vec<u16> = (0..8).map(|k| 4 * c + k).collect();
        for (k, i) in corners.iter().enumerate() {
            img.u16(e + 2 * k, *i);
        }
        let faces = match c {
            0 => [NONE, 1, NONE, NONE, NONE, NONE],
            _ => [back, NONE, NONE, NONE, NONE, NONE],
        };
        for (f, x) in faces.iter().enumerate() {
            img.u16(e + 0x10 + 2 * f, *x);
        }
        img.u32(e + 0x1C, 1);
        let lane = |a: usize, pick: fn(f32, f32) -> f32| {
            (corners.iter().map(|i| grid(*i)[a])).fold(grid(corners[0])[a], pick)
        };
        let (min, max): (Vec<f32>, Vec<f32>) = (0..3)
            .map(|a| (lane(a, f32::min), lane(a, f32::max)))
            .unzip();
        let b = boxes + 0x20 * usize::from(c);
        img.f32s(b, &[min[0] + slack, min[1], min[2], min[0] + slack]);
        img.f32s(b + 0x10, &[max[0], max[1], max[2], max[0]]);
    }
    let record = vehicle.then(|| {
        let e = img.alloc(0xD0);
        img.u32(e, 0x3E9);
        let units = img.slot(e + 0x10, 1, 16);
        for (k, n) in [2u32, 3, 12, 2].iter().enumerate() {
            img.u32(units + 4 * k, *n);
        }
        img.slot(e + 0x20, 2, 8);
        img.slot(e + 0x30, 3, 8);
        e
    });
    (img, at, record)
}

#[test]
fn a_lattice_reads_its_cells_each_boxed_by_its_corners_and_naming_its_neighbours_back() {
    let (img, at, vehicle) = lattice_image(false, 0, 0.0);
    let cage = read(&img, |v| lattice(v, at, 2, vehicle)).expect("the lattice reads");
    assert_eq!(cage.vertices.len(), 12);
    assert_eq!(cage.vertices[7], [1.5, 0.75, 1.0]);
    assert!(!cage.vehicle);
    assert_eq!(cage.cells[0].corners, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(cage.cells[1].corners, [4, 5, 6, 7, 8, 9, 10, 11]);
    assert_eq!(
        cage.cells[0].neighbours,
        [None, Some(1), None, None, None, None]
    );
    assert_eq!(
        cage.cells[1].neighbours,
        [Some(0), None, None, None, None, None]
    );

    let refused = |vehicle: bool, back: u16, slack: f32, bones: usize| {
        let (img, at, record) = lattice_image(vehicle, back, slack);
        read(&img, |v| lattice(v, at, bones, record)).is_err()
    };
    assert!(refused(false, 0, 0.01, 2), "a box off its corners");
    assert!(refused(false, NONE, 0.0, 2), "a neighbour not named back");
    assert!(refused(false, 0, 0.0, 3), "another bone count");
}

#[test]
fn a_vehicle_lattice_has_its_record_at_0xe8_restating_its_counts() {
    let (img, at, vehicle) = lattice_image(true, 0, 0.0);
    let read_back = read(&img, |v| lattice(v, at, 2, vehicle)).expect("the vehicle lattice reads");
    assert!(read_back.vehicle);
    // The head's +0x02 is 0 exactly where the record is set.
    assert!(read(&img, |v| lattice(v, at, 2, None)).is_err());
    let (mut img, at, vehicle) = lattice_image(true, 0, 0.0);
    let units = u64_at(&img.0, vehicle.expect("a record") + 0x10) as usize & 0x0FFF_FFFF;
    img.u32(units + 8, 13);
    assert!(read(&img, |v| lattice(v, at, 2, vehicle)).is_err());
}

#[test]
fn a_wheel_is_a_group_on_a_wheel_bone_and_one_with_no_child_is_refused() {
    let bones = [
        bone("chassis", [0.0, 0.0, 0.0, 1.0], [0.0; 3]),
        bone("Wheel_LF", [0.0, 0.0, 0.0, 1.0], [-0.9, 1.5, 0.0]),
        bone("wheelie_bar", [0.0, 0.0, 0.0, 1.0], [0.0; 3]),
        bone("wheel_rr", [0.0, 0.0, 0.0, 1.0], [0.9, -1.5, 0.0]),
    ];
    let groups = [
        group(0, 0..2),
        group(1, 2..4),
        group(2, 4..5),
        group(3, 5..6),
    ];
    let read = wheels(&groups, &bones).expect("the wheels read");
    assert_eq!(
        read,
        [Wheel { group: 1, child: 2 }, Wheel { group: 3, child: 5 }]
    );
    let empty = [group(0, 0..2), group(3, 2..2)];
    assert!(wheels(&empty, &bones).is_err());
}
