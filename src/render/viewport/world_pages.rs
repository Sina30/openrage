//! The large vertex and index buffers the world's models share, and where each mesh lies in them.
//!
//! Meshes of one page draw with the same two buffers bound, which is what lets the renderer send
//! runs of them to the device in one call.

use std::ops::Range;

use or_renderer::MeshVertex;

use super::budget::Budgets;

/// The bytes of a vertex and of an index.
const VERTEX: u64 = std::mem::size_of::<MeshVertex>() as u64;
const INDEX: u64 = std::mem::size_of::<u32>() as u64;

/// The free runs of a buffer, counted in what it holds, in order and never touching.
// ponytail: first fit of the fullest page; a page left with holes is emptied by moving its meshes out on the device (`Pages::moved`), never compacted in place.
#[derive(Debug, PartialEq, Eq)]
struct Free {
    runs: Vec<Range<u32>>,
}

impl Free {
    fn new(len: u32) -> Self {
        Self {
            runs: std::iter::once(0..len)
                .filter(|run| !run.is_empty())
                .collect(),
        }
    }

    /// Take `len` from the first run that holds as many; where they start.
    fn take(&mut self, len: u32) -> Option<u32> {
        let at = (self.runs.iter()).position(|run| run.end - run.start >= len)?;
        let start = self.runs[at].start;
        self.runs[at].start += len;
        if self.runs[at].is_empty() {
            self.runs.remove(at);
        }
        Some(start)
    }

    /// Give `run` back, joined to the free runs it touches.
    fn give(&mut self, run: Range<u32>) {
        if run.is_empty() {
            return;
        }
        let at = self.runs.partition_point(|free| free.start < run.start);
        let after_last = at > 0 && self.runs[at - 1].end == run.start;
        let before_next = at < self.runs.len() && self.runs[at].start == run.end;
        match (after_last, before_next) {
            (true, true) => {
                self.runs[at - 1].end = self.runs[at].end;
                self.runs.remove(at);
            }
            (true, false) => self.runs[at - 1].end = run.end,
            (false, true) => self.runs[at].start = run.start,
            (false, false) => self.runs.insert(at, run),
        }
    }

    /// Whether none of its `len` is taken.
    fn is_whole(&self, len: u32) -> bool {
        matches!(self.runs.as_slice(), [run] if *run == (0..len))
    }
}

/// Where a mesh lies: its page, its first vertex and index there, and how many of each it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Place {
    pub page: u32,
    pub base_vertex: u32,
    pub first_index: u32,
    pub vertices: u32,
    pub indices: u32,
}

impl Place {
    /// The bytes of the page it takes.
    pub(super) fn bytes(&self) -> u64 {
        u64::from(self.vertices) * VERTEX + u64::from(self.indices) * INDEX
    }
}

struct Page {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    free: (Free, Free),
    /// How many vertices and indices it holds, and how many of them its meshes take.
    size: (u32, u32),
    taken: (u32, u32),
}

impl Page {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let buffer = |label, bytes, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: usage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        Self {
            vertices: buffer(
                "world-page-vertices",
                u64::from(size.0) * VERTEX,
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                "world-page-indices",
                u64::from(size.1) * INDEX,
                wgpu::BufferUsages::INDEX,
            ),
            free: (Free::new(size.0), Free::new(size.1)),
            size,
            taken: (0, 0),
        }
    }

    /// The bytes its meshes take.
    fn used(&self) -> u64 {
        u64::from(self.taken.0) * VERTEX + u64::from(self.taken.1) * INDEX
    }

    /// Take room for `need` vertices and indices; where each starts, or `None` and nothing taken.
    fn take(&mut self, need: (u32, u32)) -> Option<(u32, u32)> {
        let vertices = self.free.0.take(need.0)?;
        let Some(indices) = self.free.1.take(need.1) else {
            self.free.0.give(vertices..vertices + need.0);
            return None;
        };
        self.taken = (self.taken.0 + need.0, self.taken.1 + need.1);
        Some((vertices, indices))
    }
}

/// The bytes of a new page's vertex buffer and of its index buffer: the whole of `page` shared as the meshes on the device share `taken`, the vertices' bytes and the indices', each at least an eighth; `page` as it is before any is taken.
// ⚠ Pages of a fixed share run out of one side while the other keeps holes no mesh can use, and those holes are device memory.
fn split(page: (u64, u64), taken: (u64, u64)) -> (u64, u64) {
    let (whole, of) = (page.0 + page.1, taken.0 + taken.1);
    if of == 0 {
        return page;
    }
    let share = (taken.0 as f64 / of as f64).clamp(0.125, 0.875);
    let vertices = (whole as f64 * share) as u64;
    (vertices, whole - vertices)
}

/// How many vertices and indices a new page holds for a mesh of `need`: a page's own count, or the
/// mesh's where it is larger; `None` where the device makes no buffer that large.
fn page_size(page: (u64, u64), most: u64, need: (u32, u32)) -> Option<(u32, u32)> {
    let side = |bytes: u64, unit: u64, need: u32| {
        let count = (bytes / unit).max(u64::from(need));
        // A draw names its first vertex by a signed number.
        (count * unit <= most && count <= i32::MAX as u64).then_some(count as u32)
    };
    Some((side(page.0, VERTEX, need.0)?, side(page.1, INDEX, need.1)?))
}

/// The pages models are written to, each let go when the last mesh in it is.
#[derive(Default)]
pub(super) struct Pages {
    pages: Vec<Option<Page>>,
}

impl Pages {
    /// Write a mesh to the first page with room for it, or to a new one; `None` for a mesh of
    /// nothing or one larger than the device makes a buffer.
    pub(super) fn put(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        budgets: &Budgets,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> Option<Place> {
        let need = (
            u32::try_from(vertices.len()).ok()?,
            u32::try_from(indices.len()).ok()?,
        );
        if need.0 == 0 || need.1 == 0 {
            return None;
        }
        let mut found = self.taken(need, None);
        if found.is_none() {
            let taken = (self.pages.iter().flatten()).fold((0, 0), |(v, i), page| {
                let (vertices, indices) = page.taken;
                (
                    v + u64::from(vertices) * VERTEX,
                    i + u64::from(indices) * INDEX,
                )
            });
            let size = page_size(split(budgets.page, taken), budgets.most_buffer, need)?;
            let mut page = Page::new(device, size);
            let (vertex, index) = page.take(need)?;
            let at = match self.pages.iter().position(Option::is_none) {
                Some(at) => at,
                None => {
                    self.pages.push(None);
                    self.pages.len() - 1
                }
            };
            self.pages[at] = Some(page);
            found = Some((at, vertex, index));
        }
        let (at, base_vertex, first_index) = found?;
        let page = self.pages[at].as_ref()?;
        queue.write_buffer(
            &page.vertices,
            u64::from(base_vertex) * VERTEX,
            bytemuck::cast_slice(vertices),
        );
        queue.write_buffer(
            &page.indices,
            u64::from(first_index) * INDEX,
            bytemuck::cast_slice(indices),
        );
        Some(Place {
            page: at as u32,
            base_vertex,
            first_index,
            vertices: need.0,
            indices: need.1,
        })
    }

    /// Give back the room of a mesh, and let its page go where that was the last in it.
    pub(super) fn free(&mut self, place: Place) {
        let Some(slot) = self.pages.get_mut(place.page as usize) else {
            return;
        };
        let Some(page) = slot else {
            return;
        };
        (page.free.0).give(place.base_vertex..place.base_vertex + place.vertices);
        (page.free.1).give(place.first_index..place.first_index + place.indices);
        let taken = &mut page.taken;
        *taken = (
            taken.0.saturating_sub(place.vertices),
            taken.1.saturating_sub(place.indices),
        );
        if page.free.0.is_whole(page.size.0) && page.free.1.is_whole(page.size.1) {
            *slot = None;
        }
    }

    /// The vertex buffer and the index buffer of the page `page`.
    pub(super) fn buffers(&self, page: u32) -> Option<(&wgpu::Buffer, &wgpu::Buffer)> {
        let page = self.pages.get(page as usize)?.as_ref()?;
        Some((&page.vertices, &page.indices))
    }

    /// Move the mesh at `place` into another page with room by a copy `encoder` records, and give its room back; the new place, or `None` where no other page has room.
    pub(super) fn moved(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        place: Place,
    ) -> Option<Place> {
        let (from_vertices, from_indices) = {
            let (v, i) = self.buffers(place.page)?;
            (v.clone(), i.clone())
        };
        let need = (place.vertices, place.indices);
        let (at, base_vertex, first_index) = self.taken(need, Some(place.page as usize))?;
        let to = self.pages[at].as_ref()?;
        let at_vertex = |first: u32| u64::from(first) * VERTEX;
        let at_index = |first: u32| u64::from(first) * INDEX;
        encoder.copy_buffer_to_buffer(
            &from_vertices,
            at_vertex(place.base_vertex),
            &to.vertices,
            at_vertex(base_vertex),
            at_vertex(need.0),
        );
        encoder.copy_buffer_to_buffer(
            &from_indices,
            at_index(place.first_index),
            &to.indices,
            at_index(first_index),
            at_index(need.1),
        );
        self.free(place);
        Some(Place {
            page: at as u32,
            base_vertex,
            first_index,
            ..place
        })
    }

    /// Room for `need` in the fullest page that has it, other than `not`: the page and where its vertices and indices start.
    fn taken(&mut self, need: (u32, u32), not: Option<usize>) -> Option<(usize, u32, u32)> {
        // The fullest page with room first, so that the emptiest empties as its models leave.
        let mut order: Vec<(u64, usize)> = (self.pages.iter().enumerate())
            .filter(|(at, _)| Some(*at) != not)
            .filter_map(|(at, page)| Some((page.as_ref()?.used(), at)))
            .collect();
        order.sort_unstable_by(|a, b| b.cmp(a));
        order.into_iter().find_map(|(_, at)| {
            let (vertex, index) = self.pages[at].as_mut()?.take(need)?;
            Some((at, vertex, index))
        })
    }

    /// The page whose meshes take the fewest bytes, where the pages together hold more than `most` bytes of device memory.
    pub(super) fn emptiest(&self, most: u64) -> Option<u32> {
        if self.held().1 <= most {
            return None;
        }
        let pages = self.pages.iter().enumerate();
        let held = pages.filter_map(|(at, page)| Some((at as u32, page.as_ref()?.used())));
        held.min_by_key(|(_, used)| *used).map(|(at, _)| at)
    }

    /// How many pages there are, and the bytes of device memory they hold.
    pub(super) fn held(&self) -> (usize, u64) {
        let pages = self.pages.iter().flatten();
        let bytes = |page: &Page| page.vertices.size() + page.indices.size();
        (pages.clone().count(), pages.map(bytes).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_is_taken_from_the_first_run_that_holds_it() {
        let mut free = Free::new(100);
        assert_eq!(free.take(30), Some(0));
        assert_eq!(free.take(30), Some(30));
        assert_eq!(free.take(50), None);
        assert_eq!(free.take(40), Some(60));
        assert_eq!(free.take(1), None);
        // A hole given back is the first run again, and takes what fits in it.
        free.give(0..30);
        assert_eq!(free.take(31), None);
        assert_eq!(free.take(10), Some(0));
        assert_eq!(free.take(20), Some(10));
        assert_eq!(Free::new(0).take(1), None);
    }

    #[test]
    fn room_given_back_joins_the_free_room_it_touches() {
        let mut free = Free::new(100);
        let taken: Vec<u32> = (0..5).filter_map(|_| free.take(20)).collect();
        assert_eq!(taken, [0, 20, 40, 60, 80]);
        assert!(!free.is_whole(100));
        free.give(20..40);
        free.give(60..80);
        assert_eq!(free.runs, [20..40, 60..80]);
        // Between two free runs: the three are one.
        free.give(40..60);
        assert_eq!(free.runs, std::iter::once(20..80).collect::<Vec<_>>());
        free.give(80..100);
        assert_eq!(free.runs, std::iter::once(20..100).collect::<Vec<_>>());
        free.give(0..20);
        assert!(free.is_whole(100));
        // Nothing given back changes nothing.
        free.give(50..50);
        assert!(free.is_whole(100));
    }

    #[test]
    fn a_new_page_is_shared_between_vertices_and_indices_as_the_meshes_on_the_device_are() {
        let page = (192 << 20, 64 << 20);
        assert_eq!(split(page, (0, 0)), page);
        // Meshes of seven parts of vertices to one of indices: a page shared the same way, whole.
        let (vertices, indices) = split(page, (7_000, 1_000));
        assert_eq!(vertices + indices, 256 << 20);
        assert_eq!(vertices, 224 << 20);
        // Never less than an eighth either side.
        assert_eq!(split(page, (1, 0)).1, 32 << 20);
        assert_eq!(split(page, (0, 1)).0, 32 << 20);
    }

    #[test]
    fn a_mesh_larger_than_a_page_takes_a_page_of_its_own_size() {
        let page = (1000 * VERTEX, 3000 * INDEX);
        let most = 1 << 20;
        assert_eq!(page_size(page, most, (10, 30)), Some((1000, 3000)));
        assert_eq!(page_size(page, most, (5000, 30)), Some((5000, 3000)));
        // More vertices than the device makes a buffer for.
        let too_many = (most / VERTEX + 1) as u32;
        assert_eq!(page_size(page, most, (too_many, 30)), None);
    }

    #[test]
    fn a_mesh_takes_the_bytes_of_its_vertices_and_indices() {
        let place = Place {
            page: 0,
            base_vertex: 7,
            first_index: 9,
            vertices: 10,
            indices: 30,
        };
        assert_eq!(place.bytes(), 10 * VERTEX + 30 * INDEX);
    }
}
