use std::f32::consts::{PI, TAU};

use glam::{Mat4, Vec3};
use or_io::gta5::{
    DrawableTarget, FragType, GtaDrawable, PgDictionary, PhBoundAny, PhBoundComposite,
    PhBoundGeometry, RmcDrawable,
};
use or_renderer::LineVertex;

const CIRCLE_SEGMENTS: usize = 32;

use crate::render::bound_meshes::{
    self, capsule_extent, cylinder_extent, disc_margin, geometry_positions, header_aabb,
    header_centroid,
};
use crate::state::capabilities::{self, GeometryRequest};
use crate::state::{AppState, DocPayload};
use crate::ui::panels::selection_ids::{
    ydd_selected_entry_index, yld_selected_cloth, ypt_drawable_for_selection, ypt_tag_of, YPT_MODEL,
};

const AXIS_X: [f32; 4] = [0.95, 0.30, 0.30, 1.0];
const AXIS_Y: [f32; 4] = [0.30, 0.85, 0.30, 1.0];
const AXIS_Z: [f32; 4] = [0.34, 0.61, 0.84, 1.0];
const GRID: [f32; 4] = [0.20, 0.20, 0.22, 1.0];
const MESH: [f32; 4] = [0.55, 0.78, 0.95, 0.85];
const BOUND_PRIMITIVE: [f32; 4] = [0.86, 0.45, 0.85, 1.0];
const BOUND_GEOMETRY: [f32; 4] = [0.95, 0.65, 0.30, 1.0];
const CLOTH_EDGE: [f32; 4] = [0.45, 0.85, 0.90, 0.90];
const CLOTH_CUSTOM_EDGE: [f32; 4] = [0.95, 0.75, 0.35, 0.95];
const CLOTH_PIN: [f32; 4] = [0.95, 0.35, 0.40, 1.0];
const CLOTH_BB: [f32; 4] = [0.55, 0.55, 0.60, 0.70];
const NAVMESH_OUTLINE: [f32; 4] = [0.85, 0.95, 0.70, 1.0];
const NAVMESH_SECTOR: [f32; 4] = [0.95, 0.75, 0.30, 0.80];

/// How far above its fill a cell's outlines are drawn.
///
/// ⚠ The fill draws after the lines and passes the depth test at equal depth. The depth buffer's
/// 24 bits step about 5 cm apart where a whole 128 m cell is framed from, and the step grows with
/// the square of the distance, so a fixed 5 cm lift hides every outline inside the fill.
fn outline_lift(mesh: &or_io::gta5::CNavMesh) -> Vec3 {
    let wide = crate::render::navmesh::extent(mesh)
        .map_or(0.0, |(min, max)| (max - min).truncate().max_element());
    Vec3::Z * (wide * 2e-3).max(0.05)
}

pub fn build_axes(half_extent: f32) -> Vec<LineVertex> {
    let h = half_extent.max(0.0);
    vec![
        LineVertex::new([-h, 0.0, 0.0], AXIS_X),
        LineVertex::new([h, 0.0, 0.0], AXIS_X),
        LineVertex::new([0.0, -h, 0.0], AXIS_Y),
        LineVertex::new([0.0, h, 0.0], AXIS_Y),
        LineVertex::new([0.0, 0.0, -h], AXIS_Z),
        LineVertex::new([0.0, 0.0, h], AXIS_Z),
    ]
}

pub fn build_grid(cells: i32, step: f32) -> Vec<LineVertex> {
    let extent = cells as f32 * step;
    let mut out = Vec::with_capacity(((2 * cells) * 2 * 2) as usize);
    for i in -cells..=cells {
        if i == 0 {
            continue;
        }
        let p = i as f32 * step;
        out.push(LineVertex::new([p, -extent, 0.0], GRID));
        out.push(LineVertex::new([p, extent, 0.0], GRID));
        out.push(LineVertex::new([-extent, p, 0.0], GRID));
        out.push(LineVertex::new([extent, p, 0.0], GRID));
    }
    out
}

/// The active document's lines: its wireframe where `wireframe`, and the bound its drawable embeds
/// while Collision is on.
pub fn document_lines(state: &AppState, wireframe: bool) -> Vec<LineVertex> {
    let mut lines = match wireframe {
        true => build_lines_for_active_document(state),
        false => Vec::new(),
    };
    if state.preview_scene.show_collision {
        lines.extend(
            embedded_bound(state)
                .map(build_bound_lines)
                .unwrap_or_default(),
        );
        let meshes = crate::render::gta6_drawable::collision_meshes(state);
        lines.extend(mesh_edges(&meshes, BOUND_GEOMETRY));
        lines.extend(crate::render::gta6_glass_panes::pane_lines(state));
    }
    lines
}

/// The edges of every triangle of `meshes`, in `colour`.
fn mesh_edges(meshes: &[crate::render::meshes::MeshUpload], colour: [f32; 4]) -> Vec<LineVertex> {
    let mut out = Vec::new();
    for mesh in meshes {
        let at = |i: u32| mesh.vertices.get(i as usize).map(|v| v.position);
        for tri in mesh.indices.as_chunks::<3>().0 {
            let (Some(a), Some(b), Some(c)) = (at(tri[0]), at(tri[1]), at(tri[2])) else {
                continue;
            };
            for (from, to) in [(a, b), (b, c), (c, a)] {
                out.push(LineVertex::new(from, colour));
                out.push(LineVertex::new(to, colour));
            }
        }
    }
    out
}

/// The bound the active document's drawable, or its dictionary's entry drawn, embeds.
pub(crate) fn embedded_bound(state: &AppState) -> Option<&PhBoundAny> {
    let doc = state.active_document()?;
    let entry = |sel| ydd_selected_entry_index(&doc.payload, sel);
    let index = state.selection.selected(doc.id).and_then(entry);
    let DocPayload::Resource(asset) = &doc.payload else {
        return None;
    };
    match asset {
        or_io::Asset::Drawable(d) => d.bound.as_deref(),
        or_io::Asset::DrawableGen9(d) => d.bound.as_deref(),
        or_io::Asset::DrawableDict(dict) => dict.entries.get(index?)?.as_deref()?.bound.as_deref(),
        or_io::Asset::DrawableDictGen9(dict) => {
            dict.entries.get(index?)?.as_deref()?.bound.as_deref()
        }
        _ => None,
    }
}

pub fn build_lines_for_active_document(state: &AppState) -> Vec<LineVertex> {
    let Some(doc) = state.active_document() else {
        return Vec::new();
    };
    let request = GeometryRequest {
        state,
        doc,
        selection: state.selection.selected(doc.id),
        entry: None,
    };
    capabilities::for_payload(&doc.payload)
        .and_then(|row| row.lines)
        .map(|build| build(&request))
        .unwrap_or_default()
}

pub(crate) mod rows {
    use super::*;

    pub(crate) fn ydr(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        let lod = req.state.preview_scene.lod;
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::Drawable(d)) => drawable_lines(&d.base, lod),
            DocPayload::Resource(or_io::Asset::DrawableGen9(d)) => drawable_lines(&d.base, lod),
            _ => Vec::new(),
        }
    }

    pub(crate) fn ydd(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        fn entry_lines<T: DrawableTarget>(
            dict: &PgDictionary<GtaDrawable<T>>,
            selected: Option<usize>,
            lod: usize,
        ) -> Vec<LineVertex> {
            selected
                .and_then(|i| dict.entries.get(i)?.as_deref())
                .map(|d| drawable_lines(&d.base, lod))
                .unwrap_or_default()
        }
        let selected = req
            .selection
            .and_then(|sel| ydd_selected_entry_index(&req.doc.payload, sel));
        let lod = req.state.preview_scene.lod;
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => {
                entry_lines(dict, selected, lod)
            }
            DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => {
                entry_lines(dict, selected, lod)
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn yft(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        let lod = req.state.preview_scene.lod;
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::Fragment(frag)) => fragment_lines(frag, lod),
            DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => fragment_lines(frag, lod),
            _ => Vec::new(),
        }
    }

    pub(crate) fn ybn(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::Bounds(bound)) => build_bound_lines(bound),
            _ => Vec::new(),
        }
    }

    pub(crate) fn yld(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        match req
            .selection
            .and_then(|sel| yld_selected_cloth(&req.doc.payload, sel))
        {
            Some(cloth) => build_cloth_lines(cloth),
            None => Vec::new(),
        }
    }

    pub(crate) fn ypt(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        build_ypt_lines(&req.doc.payload, req.selection, req.state.preview_scene.lod)
    }

    /// The edges of the triangles the model view draws of a scanned GTA VI body, a bound's child
    /// the Outline selects lit.
    pub(crate) fn gta6_drawable(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        use crate::render::gta6_drawable::{child_meshes, meshes};
        use or_io::gta6::headerless::Resource;
        let bound = match &req.doc.payload {
            DocPayload::Gta6Drawable(r) => match &**r {
                Resource::Bound(b) => Some(b),
                Resource::Cloth(c) => return crate::render::gta6_cloth::lines(c),
                _ => None,
            },
            _ => None,
        };
        let lit = bound.and_then(|b| crate::state::gta6_bound::selected_child(b, req.selection));
        match (bound, lit) {
            (Some(b), Some((lit, _))) => (b.children.iter().enumerate())
                .flat_map(|(i, child)| {
                    let colour = match i == lit {
                        true => crate::render::skeleton_lines::SELECTED,
                        false => MESH,
                    };
                    super::mesh_edges(&child_meshes(child), colour)
                })
                .collect(),
            _ => super::mesh_edges(&meshes(req), MESH),
        }
    }

    /// Each polygon's outline and each leaf sector's box.
    pub(crate) fn ynv(req: &GeometryRequest<'_>) -> Vec<LineVertex> {
        let DocPayload::Resource(or_io::Asset::NavMesh(mesh)) = &req.doc.payload else {
            return Vec::new();
        };
        let lift = super::outline_lift(mesh);
        let mut out = Vec::new();
        for corners in crate::render::navmesh::polygons(mesh) {
            for (i, a) in corners.iter().enumerate() {
                let b = corners[(i + 1) % corners.len()];
                push_line(&mut out, *a + lift, b + lift, NAVMESH_OUTLINE);
            }
        }
        // The polygon the Outline has selected, over its own outline
        if let Some(index) = req
            .selection
            .and_then(crate::ui::panels::selection_ids::nav_polygon_of)
        {
            let corners = crate::render::navmesh::polygons(mesh)
                .into_iter()
                .nth(index)
                .unwrap_or_default();
            for (i, a) in corners.iter().enumerate() {
                let b = corners[(i + 1) % corners.len()];
                push_line(
                    &mut out,
                    *a + lift * 2.0,
                    b + lift * 2.0,
                    crate::render::skeleton_lines::SELECTED,
                );
            }
        }
        for (mn, mx) in crate::render::navmesh::sectors(mesh) {
            push_aabb_lines(Mat4::IDENTITY, mn, mx, NAVMESH_SECTOR, &mut out);
        }
        out
    }
}

fn build_cloth_lines(cloth: &or_io::gta5::CharacterCloth) -> Vec<LineVertex> {
    let mut out = Vec::new();
    for (_, verlet) in crate::ui::panels::hierarchy::cloth_levels(cloth) {
        extend_cloth_verlet_lines(&mut out, verlet);
    }
    if let Some(bound) = cloth.bound_composite.as_ref() {
        walk_composite(bound, Mat4::IDENTITY, &mut out);
    }
    out
}

fn extend_cloth_verlet_lines(out: &mut Vec<LineVertex>, verlet: &or_io::gta5::PhVerletCloth) {
    let positions = &verlet.cloth_data.vertex_positions;
    if positions.is_empty() {
        return;
    }

    let to_v3 = |i: u16| -> Option<Vec3> {
        let v = positions.get(i as usize)?;
        Some(Vec3::new(v.x, v.y, v.z))
    };

    for edge in &verlet.edge_data {
        let [a, b] = edge.vert_indices;
        if let (Some(a), Some(b)) = (to_v3(a), to_v3(b)) {
            push_line(out, a, b, CLOTH_EDGE);
        }
    }
    for edge in &verlet.custom_edge_data {
        let [a, b] = edge.vert_indices;
        if let (Some(a), Some(b)) = (to_v3(a), to_v3(b)) {
            push_line(out, a, b, CLOTH_CUSTOM_EDGE);
        }
    }

    let pin_count = usize::try_from(verlet.num_pinned_verts).unwrap_or(0);
    let mut extents = Vec3::ZERO;
    for v in positions {
        extents = extents.max(Vec3::new(v.x.abs(), v.y.abs(), v.z.abs()));
    }
    let mark = (extents.length() * 0.02).clamp(0.005, 0.05);
    for v in positions.iter().take(pin_count) {
        let p = Vec3::new(v.x, v.y, v.z);
        push_line(out, p - Vec3::X * mark, p + Vec3::X * mark, CLOTH_PIN);
        push_line(out, p - Vec3::Y * mark, p + Vec3::Y * mark, CLOTH_PIN);
        push_line(out, p - Vec3::Z * mark, p + Vec3::Z * mark, CLOTH_PIN);
    }

    let mn = Vec3::new(verlet.bb_min.x, verlet.bb_min.y, verlet.bb_min.z);
    let mx = Vec3::new(verlet.bb_max.x, verlet.bb_max.y, verlet.bb_max.z);
    if mn.is_finite() && mx.is_finite() && (mx - mn).length_squared() > 1e-8 {
        push_aabb_lines(Mat4::IDENTITY, mn, mx, CLOTH_BB, out);
    }
}

fn build_ypt_lines(payload: &DocPayload, selection: Option<u64>, lod: usize) -> Vec<LineVertex> {
    let Some(sel) = selection else {
        return Vec::new();
    };
    if ypt_tag_of(sel) != YPT_MODEL {
        return Vec::new();
    }
    match payload {
        DocPayload::Resource(or_io::Asset::Particles(list)) => {
            ypt_drawable_for_selection(list, sel).map_or_else(Vec::new, |d| drawable_lines(d, lod))
        }
        DocPayload::Resource(or_io::Asset::ParticlesGen9(list)) => {
            ypt_drawable_for_selection(list, sel).map_or_else(Vec::new, |d| drawable_lines(d, lod))
        }
        _ => Vec::new(),
    }
}

/// A wireframe of the level of detail the viewport draws of a drawable for the level `lod`.
fn drawable_lines<T: DrawableTarget>(drawable: &RmcDrawable<T>, lod: usize) -> Vec<LineVertex> {
    use or_io::codec::mesh::VertexSource;
    let mut out = Vec::new();
    let level = crate::render::meshes::level_drawn(drawable, lod);
    for geom in drawable.lod_group.lods[level]
        .models
        .iter()
        .flat_map(|m| &m.geometries)
    {
        let vb = &*geom.vertex_buffer;
        extend_with_triangles(
            &mut out,
            vb.vertex_data(),
            vb.vertex_stride(),
            vb.vertex_declared(),
            T::indices(&geom.index_buffer),
        );
    }
    out
}

/// A fragment's common drawable, and the bound of its highest physics level.
fn fragment_lines<T: DrawableTarget>(frag: &FragType<T>, lod: usize) -> Vec<LineVertex> {
    let mut lines = frag
        .common_drawable
        .as_deref()
        .map(|d| drawable_lines(&d.base, lod))
        .unwrap_or_default();
    if let Some(bound) = physics_bound(frag) {
        lines.extend(build_bound_lines(bound));
    }
    lines
}

/// The bound of a fragment's highest physics level, which Wireframe draws beside its mesh.
pub(crate) fn physics_bound<T: DrawableTarget>(frag: &FragType<T>) -> Option<&PhBoundAny> {
    (frag.physics_lod_group.as_deref()?.high_lod.as_deref()?)
        .composite_bounds
        .as_deref()
}

/// A wireframe of the triangles `indices` draws over the positions leading each `stride` bytes of `data`.
fn extend_with_triangles(
    out: &mut Vec<LineVertex>,
    data: &[u8],
    stride: usize,
    vertex_count: usize,
    indices: &[u16],
) {
    if stride < 12 {
        return;
    }

    let positions = decode_positions(data, stride, vertex_count);
    if positions.is_empty() {
        return;
    }

    out.reserve(indices.len() * 2);
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        let a = positions[i0];
        let b = positions[i1];
        let c = positions[i2];
        out.push(LineVertex::new(a, MESH));
        out.push(LineVertex::new(b, MESH));
        out.push(LineVertex::new(b, MESH));
        out.push(LineVertex::new(c, MESH));
        out.push(LineVertex::new(c, MESH));
        out.push(LineVertex::new(a, MESH));
    }
}

fn decode_positions(data: &[u8], stride: usize, vertex_count: usize) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let off = i * stride;
        if off + 12 > data.len() {
            break;
        }
        let bytes = &data[off..off + 12];
        let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let y = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let z = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        if x.is_finite() && y.is_finite() && z.is_finite() {
            out.push([x, y, z]);
        } else {
            out.push([0.0, 0.0, 0.0]);
        }
    }
    out
}

pub fn build_bound_lines(bound: &PhBoundAny) -> Vec<LineVertex> {
    let mut out = Vec::new();
    walk_bound(bound, Mat4::IDENTITY, &mut out);
    out
}

/// The lines each leaf of a bound draws, by the number its row has in the Outline, which counts the
/// nodes the bound holds in order, a composite before its children.
pub(crate) fn bound_node_lines(bound: &PhBoundAny) -> Vec<(u64, Vec<LineVertex>)> {
    fn walk(
        bound: &PhBoundAny,
        transform: Mat4,
        next: &mut u64,
        out: &mut Vec<(u64, Vec<LineVertex>)>,
    ) {
        let node = *next;
        *next += 1;
        if let PhBoundAny::PhBoundComposite(c) = bound {
            for (child, local) in composite_children(c) {
                walk(child, transform * local, next, out);
            }
            return;
        }
        let mut lines = Vec::new();
        walk_bound(bound, transform, &mut lines);
        out.push((node, lines));
    }
    let mut out = Vec::new();
    walk(bound, Mat4::IDENTITY, &mut 0, &mut out);
    out
}

/// A composite's bounds that are there, each with the matrix that places it.
pub(crate) fn composite_children(
    c: &PhBoundComposite,
) -> impl Iterator<Item = (&PhBoundAny, Mat4)> {
    c.bounds.iter().enumerate().filter_map(|(i, child)| {
        let local = (c.current_matrices.get(i))
            .map(bound_meshes::mat34_to_mat4)
            .unwrap_or(Mat4::IDENTITY);
        Some((child.as_deref()?, local))
    })
}

fn walk_composite(c: &PhBoundComposite, transform: Mat4, out: &mut Vec<LineVertex>) {
    for (child, local) in composite_children(c) {
        walk_bound(child, transform * local, out);
    }
}

fn walk_bound(bound: &PhBoundAny, transform: Mat4, out: &mut Vec<LineVertex>) {
    let colour = BOUND_PRIMITIVE;
    match bound {
        PhBoundAny::PhBoundGeometry(g) => extend_with_polyhedron(g, transform, out),
        PhBoundAny::PhBoundBVH(b) => extend_with_polyhedron(&b.base, transform, out),
        PhBoundAny::PhBoundComposite(c) => walk_composite(c, transform, out),
        PhBoundAny::PhBoundSphere(b) => push_sphere_lines(
            transform,
            header_centroid(&b.base),
            b.base.radius_around_centroid,
            colour,
            out,
        ),
        PhBoundAny::PhBoundBox(b) => {
            let (mn, mx) = header_aabb(&b.base);
            push_aabb_lines(transform, mn, mx, colour, out);
        }
        PhBoundAny::PhBoundCapsule(b) => {
            let (radius, half_length) = capsule_extent(&b.base);
            push_capsule_lines(
                transform,
                header_centroid(&b.base),
                radius,
                half_length,
                colour,
                out,
            );
        }
        PhBoundAny::PhBoundCylinder(b) => {
            let (radius, half_height) = cylinder_extent(&b.base);
            push_cylinder_lines(
                transform,
                header_aabb(&b.base),
                radius,
                half_height,
                colour,
                out,
            );
        }
        PhBoundAny::PhBoundDisc(b) => push_disc_lines(
            transform,
            header_aabb(&b.base),
            b.base.radius_around_centroid,
            disc_margin(&b.base),
            colour,
            out,
        ),
        PhBoundAny::PhBoundPlane(b) => {
            let (mn, mx) = header_aabb(&b.base);
            push_aabb_lines(transform, mn, mx, colour, out);
        }
        PhBoundAny::Base(_) => {}
    }
}

fn extend_with_polyhedron(geom: &PhBoundGeometry, transform: Mat4, out: &mut Vec<LineVertex>) {
    let positions = geometry_positions(geom, transform);
    out.reserve(geom.polygons.len() * 6);
    for poly in &geom.polygons {
        let Some((named, radius)) = geom.polygon_vertices(poly) else {
            continue;
        };
        let corners: Option<Vec<Vec3>> = named.iter().map(|i| positions.get(*i).copied()).collect();
        if let Some(corners) = corners {
            push_polygon_lines(out, poly.primitive(), &corners, radius, BOUND_GEOMETRY);
        }
    }
}

/// One polygon of a polyhedron: a triangle, or a primitive `kind` names over its corners.
fn push_polygon_lines(
    out: &mut Vec<LineVertex>,
    kind: u32,
    corners: &[Vec3],
    radius: f32,
    colour: [f32; 4],
) {
    match (kind, corners) {
        (0, &[a, b, c]) => {
            push_line(out, a, b, colour);
            push_line(out, b, c, colour);
            push_line(out, c, a, colour);
        }
        (1, &[centre]) => push_polygon_sphere_lines(out, centre, radius, colour),
        (2, &[a, b]) => push_polygon_capsule_lines(out, a, b, radius, colour),
        (4, &[a, b]) => push_polygon_cylinder_lines(out, a, b, radius, colour),
        (3, &[a, b, c, d]) => push_polygon_box_lines(out, [a, b, c, d], colour),
        _ => {}
    }
}

fn push_polygon_sphere_lines(
    out: &mut Vec<LineVertex>,
    centre: Vec3,
    radius: f32,
    colour: [f32; 4],
) {
    if radius <= 0.0 {
        return;
    }
    push_circle(
        out,
        Mat4::IDENTITY,
        centre,
        Vec3::X,
        Vec3::Y,
        radius,
        colour,
    );
    push_circle(
        out,
        Mat4::IDENTITY,
        centre,
        Vec3::Y,
        Vec3::Z,
        radius,
        colour,
    );
    push_circle(
        out,
        Mat4::IDENTITY,
        centre,
        Vec3::X,
        Vec3::Z,
        radius,
        colour,
    );
}

fn push_polygon_capsule_lines(
    out: &mut Vec<LineVertex>,
    endpoint_a: Vec3,
    endpoint_b: Vec3,
    radius: f32,
    colour: [f32; 4],
) {
    if radius <= 0.0 {
        return;
    }
    let axis = endpoint_b - endpoint_a;
    if axis.length_squared() == 0.0 {
        push_polygon_sphere_lines(out, endpoint_a, radius, colour);
        return;
    }
    let y = axis.normalize();
    let helper = if y.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let x = helper.cross(y).normalize_or_zero();
    let z = y.cross(x).normalize_or_zero();
    push_circle(out, Mat4::IDENTITY, endpoint_a, x, z, radius, colour);
    push_circle(out, Mat4::IDENTITY, endpoint_b, x, z, radius, colour);
    let sides = [x, -x, z, -z];
    for axis_off in sides {
        push_line(
            out,
            endpoint_a + axis_off * radius,
            endpoint_b + axis_off * radius,
            colour,
        );
    }
}

fn push_polygon_cylinder_lines(
    out: &mut Vec<LineVertex>,
    endpoint_a: Vec3,
    endpoint_b: Vec3,
    radius: f32,
    colour: [f32; 4],
) {
    push_polygon_capsule_lines(out, endpoint_a, endpoint_b, radius, colour);
}

fn push_polygon_box_lines(out: &mut Vec<LineVertex>, corners4: [Vec3; 4], colour: [f32; 4]) {
    let p1 = corners4[0];
    let p2 = corners4[1];
    let p3 = corners4[2];
    let p4 = corners4[3];
    let a1 = ((p3 + p4) - (p1 + p2)) * 0.5;
    let corner = p1;
    let edge1 = a1;
    let edge2 = (p3 - a1) - p1;
    let edge3 = (p4 - a1) - p1;

    let mut c = [Vec3::ZERO; 8];
    for (idx, slot) in c.iter_mut().enumerate() {
        let i = (idx & 1) as f32;
        let j = ((idx >> 1) & 1) as f32;
        let k = ((idx >> 2) & 1) as f32;
        *slot = corner + edge1 * i + edge2 * j + edge3 * k;
    }

    let edges: [(usize, usize); 12] = [
        (0, 1),
        (1, 3),
        (3, 2),
        (2, 0),
        (4, 5),
        (5, 7),
        (7, 6),
        (6, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    for (a, b) in edges {
        push_line(out, c[a], c[b], colour);
    }
}

fn push_line(out: &mut Vec<LineVertex>, a: Vec3, b: Vec3, colour: [f32; 4]) {
    out.push(LineVertex::new([a.x, a.y, a.z], colour));
    out.push(LineVertex::new([b.x, b.y, b.z], colour));
}

fn push_circle(
    out: &mut Vec<LineVertex>,
    transform: Mat4,
    centre: Vec3,
    axis_a: Vec3,
    axis_b: Vec3,
    radius: f32,
    colour: [f32; 4],
) {
    if radius <= 0.0 || !radius.is_finite() {
        return;
    }
    out.reserve(CIRCLE_SEGMENTS * 2);
    let mut prev = transform.transform_point3(centre + axis_a * radius);
    for i in 1..=CIRCLE_SEGMENTS {
        let theta = (i as f32) * TAU / CIRCLE_SEGMENTS as f32;
        let local = centre + axis_a * (radius * theta.cos()) + axis_b * (radius * theta.sin());
        let next = transform.transform_point3(local);
        push_line(out, prev, next, colour);
        prev = next;
    }
}

#[allow(clippy::too_many_arguments)]
fn push_arc(
    out: &mut Vec<LineVertex>,
    transform: Mat4,
    centre: Vec3,
    axis_a: Vec3,
    axis_b: Vec3,
    radius: f32,
    start: f32,
    end: f32,
    colour: [f32; 4],
) {
    if radius <= 0.0 || !radius.is_finite() {
        return;
    }
    let segs = (CIRCLE_SEGMENTS / 2).max(2);
    out.reserve(segs * 2);
    let step = (end - start) / segs as f32;
    let mut prev = {
        let local = centre + axis_a * (radius * start.cos()) + axis_b * (radius * start.sin());
        transform.transform_point3(local)
    };
    for i in 1..=segs {
        let theta = start + step * i as f32;
        let local = centre + axis_a * (radius * theta.cos()) + axis_b * (radius * theta.sin());
        let next = transform.transform_point3(local);
        push_line(out, prev, next, colour);
        prev = next;
    }
}

fn push_sphere_lines(
    transform: Mat4,
    centre: Vec3,
    radius: f32,
    colour: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    push_circle(out, transform, centre, Vec3::X, Vec3::Y, radius, colour);
    push_circle(out, transform, centre, Vec3::Y, Vec3::Z, radius, colour);
    push_circle(out, transform, centre, Vec3::X, Vec3::Z, radius, colour);
}

fn push_capsule_lines(
    transform: Mat4,
    centre: Vec3,
    radius: f32,
    half_length: f32,
    colour: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    if radius <= 0.0 {
        return;
    }
    let top = centre + Vec3::Y * half_length;
    let bottom = centre - Vec3::Y * half_length;

    push_circle(out, transform, top, Vec3::X, Vec3::Z, radius, colour);
    push_circle(out, transform, bottom, Vec3::X, Vec3::Z, radius, colour);

    push_arc(
        out,
        transform,
        top,
        Vec3::X,
        Vec3::Y,
        radius,
        0.0,
        PI,
        colour,
    );
    push_arc(
        out,
        transform,
        top,
        Vec3::Z,
        Vec3::Y,
        radius,
        0.0,
        PI,
        colour,
    );
    push_arc(
        out,
        transform,
        bottom,
        Vec3::X,
        Vec3::Y,
        radius,
        PI,
        TAU,
        colour,
    );
    push_arc(
        out,
        transform,
        bottom,
        Vec3::Z,
        Vec3::Y,
        radius,
        PI,
        TAU,
        colour,
    );

    let sides = [Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z];
    for axis in sides {
        let a = transform.transform_point3(top + axis * radius);
        let b = transform.transform_point3(bottom + axis * radius);
        push_line(out, a, b, colour);
    }
}

fn push_cylinder_lines(
    transform: Mat4,
    (mn, mx): (Vec3, Vec3),
    radius: f32,
    half_height: f32,
    colour: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    if radius <= 0.0 || half_height <= 0.0 {
        return;
    }

    let centre = (mn + mx) * 0.5;
    let top = centre + Vec3::Y * half_height;
    let bottom = centre - Vec3::Y * half_height;

    push_circle(out, transform, top, Vec3::X, Vec3::Z, radius, colour);
    push_circle(out, transform, bottom, Vec3::X, Vec3::Z, radius, colour);

    let sides = [Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z];
    for axis in sides {
        let a = transform.transform_point3(top + axis * radius);
        let b = transform.transform_point3(bottom + axis * radius);
        push_line(out, a, b, colour);
    }
}

/// A disc flat along X: `radius_around_centroid` or half its box, `margin` thick either side.
fn push_disc_lines(
    transform: Mat4,
    (mn, mx): (Vec3, Vec3),
    radius_around_centroid: f32,
    margin: f32,
    colour: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    let extent = mx - mn;

    let radius = if radius_around_centroid > 0.0 {
        radius_around_centroid
    } else {
        extent.y.max(extent.z) * 0.5
    };
    let margin = margin.max(0.0);
    if radius <= 0.0 {
        return;
    }

    let centre = (mn + mx) * 0.5;
    let face_pos = centre + Vec3::X * margin;
    let face_neg = centre - Vec3::X * margin;
    push_circle(out, transform, face_pos, Vec3::Y, Vec3::Z, radius, colour);
    if margin > 0.0 {
        push_circle(out, transform, face_neg, Vec3::Y, Vec3::Z, radius, colour);
        let sides = [Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        for axis in sides {
            let a = transform.transform_point3(face_pos + axis * radius);
            let b = transform.transform_point3(face_neg + axis * radius);
            push_line(out, a, b, colour);
        }
    }
}

fn push_aabb_lines(
    transform: Mat4,
    mn: Vec3,
    mx: Vec3,
    colour: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    let corners = [
        Vec3::new(mn.x, mn.y, mn.z),
        Vec3::new(mx.x, mn.y, mn.z),
        Vec3::new(mx.x, mx.y, mn.z),
        Vec3::new(mn.x, mx.y, mn.z),
        Vec3::new(mn.x, mn.y, mx.z),
        Vec3::new(mx.x, mn.y, mx.z),
        Vec3::new(mx.x, mx.y, mx.z),
        Vec3::new(mn.x, mx.y, mx.z),
    ]
    .map(|c| transform.transform_point3(c));

    let edges: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    out.reserve(edges.len() * 2);
    for (a, b) in edges {
        push_line(out, corners[a], corners[b], colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bound_meshes::shapes::{composite, cylinder, disc, sphere};
    use or_io::gta5::File as _;

    fn fixture_dir() -> std::path::PathBuf {
        or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn an_enhanced_drawable_draws_a_wireframe() {
        let Some(path) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
            .target(or_test_support::TARGET_GEN9)
            .find()
            .into_iter()
            .next()
        else {
            or_test_support::skip_corpus(
                "an_enhanced_drawable_draws_a_wireframe",
                "no Enhanced .ydr fixture",
            );
            return;
        };
        let data = std::fs::read(&path).expect("read fixture");
        let handler = crate::state::payload_handlers::handler_for(
            or_io::container::resource::ResourceType::Ydr,
            or_io::Generation::Gen9,
        )
        .expect("the Enhanced .ydr row");
        let payload = (handler.parse)(&path, &data)
            .unwrap_or_else(|e| panic!("{}: does not open with no pack: {e}", path.display()));
        let DocPayload::Resource(or_io::Asset::DrawableGen9(drawable)) = &payload else {
            panic!("{}: opened as {:?}", path.display(), payload.kind());
        };

        let lines = drawable_lines(&drawable.base, 0);
        assert!(
            !lines.is_empty(),
            "{}: an Enhanced drawable produced no wireframe, so its vertex \
             payload is not reaching `VertexSource::vertex_data`",
            path.display()
        );
        assert!(
            lines.len().is_multiple_of(2),
            "line vertices come in pairs; got {}",
            lines.len()
        );
    }

    #[test]
    fn sphere_primitive_draws_great_circles_not_a_box() {
        let lines = build_bound_lines(&sphere([0.0; 3], 2.5));
        assert!(!lines.is_empty(), "sphere produced no lines");
        for v in &lines {
            let r = (v.position[0].powi(2) + v.position[1].powi(2) + v.position[2].powi(2)).sqrt();
            assert!(
                (r - 2.5).abs() < 1e-3,
                "vertex {:?} not on sphere of r=2.5 (got r={})",
                v.position,
                r
            );
        }
    }

    /// Saved and reopened with no pack: the owned bound's description is built in.
    fn round_trip(bound: &PhBoundAny) -> PhBoundAny {
        let bytes = bound.to_bytes().expect("save ybn");
        PhBoundAny::from_bytes(&bytes).expect("load ybn")
    }

    #[test]
    fn cylinder_survives_ybn_roundtrip() {
        let rt = round_trip(&cylinder([1.0, 2.0, 3.0], 1.5, 4.0));
        let PhBoundAny::PhBoundCylinder(c) = &rt else {
            panic!("expected a cylinder, got {rt:?}");
        };
        let (radius, half_height) = cylinder_extent(&c.base);
        assert!(
            (radius - 1.5).abs() < 1e-5 && (half_height - 2.0).abs() < 1e-5,
            "cylinder extent lost on roundtrip: radius {radius}, half height {half_height}"
        );
        let lines = build_bound_lines(&rt);
        assert!(!lines.is_empty(), "roundtripped cylinder produced no lines");
    }

    #[test]
    fn disc_with_zero_margin_falls_back_to_its_box() {
        let mut bound = disc([0.0; 3], 2.0, 0.05);
        if let PhBoundAny::PhBoundDisc(d) = &mut bound {
            d.base.bounding_box_max_xyz_margin_w.w = 0.0;
            d.base.radius_around_centroid = 0.0;
        }
        let lines = build_bound_lines(&bound);
        assert!(
            !lines.is_empty(),
            "disc with zeroed margin and radius produced no lines"
        );
    }

    #[test]
    fn cylinder_in_composite_renders() {
        let lines = build_bound_lines(&composite(vec![cylinder([0.0; 3], 1.5, 4.0)]));
        assert!(
            !lines.is_empty(),
            "cylinder inside composite produced no lines"
        );
    }

    #[test]
    fn a_whole_cell_s_outlines_lift_clear_of_its_fill_in_the_depth_buffer() {
        use or_io::gta5::{CNavMesh, CNavMeshQuadTree, Vector3};
        let corner = |x: f32, y: f32| Vector3 {
            x,
            y,
            ..Default::default()
        };
        let cell = CNavMesh {
            quad_tree: Some(Box::new(CNavMeshQuadTree {
                mins: corner(-12288.0, -9856.0),
                maxs: corner(-12160.0, -9728.0),
                ..Default::default()
            })),
            ..Default::default()
        };
        // One step of a 24-bit reverse-Z depth buffer from the camera's near plane, twice the
        // cell's width away.
        let near = or_renderer::OrbitCamera::default().near;
        let distance = 2.0 * 128.0f32;
        let step = distance * distance / (near * (1u32 << 24) as f32);
        let lift = super::outline_lift(&cell).z;
        assert!(
            lift > 2.0 * step,
            "a lift of {lift} m against a step of {step} m"
        );
    }

    #[test]
    fn cylinder_primitive_draws_circular_caps() {
        let lines = build_bound_lines(&cylinder([0.0; 3], 1.5, 4.0));
        assert!(!lines.is_empty(), "cylinder produced no lines");
        let on_cap_ring = lines.iter().any(|v| {
            let [x, _, z] = v.position;
            let r = (x * x + z * z).sqrt();
            (r - 1.5).abs() < 1e-3 && x.abs() < 1.5 - 1e-3 && z.abs() < 1.5 - 1e-3
        });
        assert!(on_cap_ring, "cylinder wireframe missing curved cap ring");
    }

    #[test]
    fn disc_primitive_draws_a_ring_not_a_box() {
        let lines = build_bound_lines(&disc([0.0; 3], 2.0, 0.05));
        assert!(!lines.is_empty(), "disc produced no lines");
        let on_ring = lines.iter().any(|v| {
            let [_, y, z] = v.position;
            let r = (y * y + z * z).sqrt();
            (r - 2.0).abs() < 1e-3 && y.abs() < 2.0 - 1e-3 && z.abs() < 2.0 - 1e-3
        });
        assert!(
            on_ring,
            "disc wireframe missing curved ring in the YZ plane"
        );
    }

    #[test]
    fn bvh_bounds_emit_lines() {
        let path = fixture_dir().join("canyonriver01_traincrash_1.ybn");
        if !path.exists() {
            or_test_support::skip_corpus(
                "bvh_bounds_emit_lines",
                format_args!("fixture missing at {}", path.display()),
            );
            return;
        }
        let data = std::fs::read(&path).expect("read fixture");
        let bound = PhBoundAny::from_bytes(&data).expect("parse ybn");
        let lines = build_bound_lines(&bound);
        assert!(
            !lines.is_empty(),
            "BVH bound produced 0 line vertices ({})",
            crate::ui::panels::hierarchy::bound_type_name(&bound)
        );
    }
}
