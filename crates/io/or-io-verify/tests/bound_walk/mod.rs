//! What the typed API reads of a bound and everything it reaches, filed as
//! the tree files it: the `.ybn` suite's typed read, and a cloth's bounds.

use or_io::decl::{Described, Plain, Raw, Record, View, ViewError};
use or_io::desc::Image;
use or_io::gta5::bounds::{
    self, PhBound, PhBoundBVH, PhBoundComposite, PhBoundCylinder, PhBoundDisc, PhBoundGeometry,
    PhOptimizedBvh,
};
use or_io_verify::gates::{offset_of_run, Filed};

type E = ViewError;

pub(crate) fn put<R: Record + Described>(out: &mut Filed, v: &View<'_, R>) {
    out.structs.insert(
        (View::offset(v), R::name()),
        Plain::as_bytes(View::get(v)).to_vec(),
    );
}

fn run<R: Record + Described>(out: &mut Filed, run: or_io::decl::Run<'_, R>) {
    for v in run.views() {
        put(out, &v);
    }
}

pub(crate) fn bytes<T: Plain>(image: Image<'_>, out: &mut Filed, run: &[T]) {
    if let Some(at) = offset_of_run(image, run) {
        out.runs
            .insert(at, run.iter().flat_map(|v| v.as_bytes().to_vec()).collect());
    }
}

/// What the typed API reads of a bound and everything it reaches, in the shape [`file_tree`] files a tree.
pub(crate) fn walk(image: Image<'_>, b: View<'_, Raw<PhBound>>, out: &mut Filed) -> Result<(), E> {
    macro_rules! header_only {
        ($($t:ty),*) => {$(
            if let Some(v) = View::variant::<Raw<$t>>(&b)? {
                put(out, &v);
                return Ok(());
            }
        )*};
    }
    header_only!(
        bounds::PhBoundSphere,
        bounds::PhBoundBox,
        bounds::PhBoundCapsule,
        PhBoundDisc,
        PhBoundCylinder,
        bounds::PhBoundPlane
    );
    if let Some(v) = View::variant::<Raw<PhBoundGeometry>>(&b)? {
        put(out, &v);
        geometry(image, &v, out)?;
    } else if let Some(v) = View::variant::<Raw<PhBoundBVH>>(&b)? {
        put(out, &v);
        geometry(image, &v.base(), out)?;
        if let Some(t) = v.bvh()? {
            tree(&t, out)?;
        }
    } else if let Some(v) = View::variant::<Raw<PhBoundComposite>>(&b)? {
        put(out, &v);
        let children = v.bounds()?;
        for i in 0..children.len() {
            if let Some(child) = children.follow(i)? {
                walk(image, child, out)?;
            }
        }
        run(out, v.current_matrices()?);
        run(out, v.last_matrices()?);
        run(out, v.local_box_min_maxs()?);
        run(out, v.type_and_include_flags()?);
        run(out, v.owned_type_and_include_flags()?);
        if let Some(t) = v.bvh_structure()? {
            tree(&t, out)?;
        }
    } else {
        put(out, &b);
    }
    Ok(())
}

fn geometry(
    image: Image<'_>,
    g: &View<'_, Raw<PhBoundGeometry>>,
    out: &mut Filed,
) -> Result<(), E> {
    // ⚠ A run its count governs may be null, which a view refuses and the tree holds as nothing.
    if let Ok(r) = g.compressed_shrunk_vertices() {
        run(out, r);
    }
    run(out, g.polygons()?);
    run(out, g.compressed_vertices()?);
    if let Ok(r) = g.vertex_attribs() {
        run(out, r);
    }
    run(out, g.material_ids()?);
    if let Ok(r) = g.material_colors() {
        run(out, r);
    }
    if let Ok(r) = g.second_surface_vertex_displacements() {
        bytes(image, out, &r);
    }
    bytes(image, out, &g.poly_mat_index_list()?);
    Ok(())
}

fn tree(t: &View<'_, Raw<PhOptimizedBvh>>, out: &mut Filed) -> Result<(), E> {
    put(out, t);
    run(out, t.contiguous_nodes()?);
    run(out, t.subtree_headers()?);
    Ok(())
}
