use glam::Vec3;
use or_io::gta5::{DrawableTarget, FragType, GtaDrawable, PgDictionary, PhBound, RmcDrawable};

use crate::state::{AppState, DocPayload};

pub fn compute_active_doc_bounds(state: &AppState) -> Option<(Vec3, f32)> {
    let doc = state.active_document()?;
    let request = crate::state::capabilities::GeometryRequest {
        state,
        doc,
        selection: state.selection.selected(doc.id),
        entry: None,
    };
    let row = crate::state::capabilities::for_payload(&doc.payload)?;
    (row.bounds?)(&request)
}

/// What Frame selection frames: the selected model or geometry, else what Frame all frames.
pub fn compute_selection_bounds(state: &AppState) -> Option<(Vec3, f32)> {
    selected_part_sphere(state).or_else(|| compute_active_doc_bounds(state))
}

/// The model or geometry the active selection names, by the box its model ships for it.
fn selected_part_sphere(state: &AppState) -> Option<(Vec3, f32)> {
    use crate::ui::panels::selection_ids::{
        dict_entry_unpack_part, is_dict_entry_selection, section_of, unpack_lod_sub_index,
        unpack_selection, ypt_drawable_for_selection, SECT_LOD, YPT_MODEL_KIND_LOD,
    };
    let doc = state.active_document()?;
    let sel = state.selection.selected(doc.id)?;
    let DocPayload::Resource(asset) = &doc.payload else {
        return None;
    };
    // A drawable's own tree names a part by its section; a dictionary's, a
    // fragment's and a particle list's name it under an entry's hash.
    if section_of(sel) == SECT_LOD {
        let (lod, model, geometry) = unpack_selection(sel);
        let part = (lod.into(), model.into(), geometry.map(|g| g as usize));
        return match asset {
            or_io::Asset::Drawable(d) => part_sphere(&d.base, part),
            or_io::Asset::DrawableGen9(d) => part_sphere(&d.base, part),
            _ => None,
        };
    }
    let (hash, kind, sub) = dict_entry_unpack_part(sel);
    if !is_dict_entry_selection(sel) || kind != YPT_MODEL_KIND_LOD {
        return None;
    }
    let (lod, model, geometry) = unpack_lod_sub_index(sub);
    let part = (lod.into(), model.into(), geometry.map(usize::from));
    match asset {
        or_io::Asset::DrawableDict(dict) => part_sphere(&entry(dict, hash)?.base, part),
        or_io::Asset::DrawableDictGen9(dict) => part_sphere(&entry(dict, hash)?.base, part),
        or_io::Asset::Fragment(frag) => part_sphere(&frag.common_drawable.as_deref()?.base, part),
        or_io::Asset::FragmentGen9(frag) => {
            part_sphere(&frag.common_drawable.as_deref()?.base, part)
        }
        or_io::Asset::Particles(list) => part_sphere(ypt_drawable_for_selection(list, sel)?, part),
        or_io::Asset::ParticlesGen9(list) => {
            part_sphere(ypt_drawable_for_selection(list, sel)?, part)
        }
        _ => None,
    }
}

fn entry<T: DrawableTarget>(
    dict: &PgDictionary<GtaDrawable<T>>,
    hash: u32,
) -> Option<&GtaDrawable<T>> {
    let at = dict.codes.iter().position(|code| *code == hash)?;
    dict.entries.get(at)?.as_deref()
}

/// The box model `model` of level `lod` ships for itself, or for one of its geometries.
fn part_sphere<T: DrawableTarget>(
    drawable: &RmcDrawable<T>,
    (lod, model, geometry): (usize, usize, Option<usize>),
) -> Option<(Vec3, f32)> {
    let model = drawable.lod_group.lods.get(lod)?.models.get(model)?;
    // Slot 0 is the model's own box: the union above one geometry, the one geometry's otherwise.
    let slot = match geometry {
        Some(index) => or_io::codec::mesh::aabb_slot_of(model.geometries.len(), index)?,
        None => 0,
    };
    let b = model.aabbs.get(slot)?;
    let (centre, radius) = or_io::codec::mesh::Aabb {
        min: [b.min.x, b.min.y, b.min.z],
        max: [b.max.x, b.max.y, b.max.z],
    }
    .bounding_sphere();
    let centre = Vec3::from(centre);
    (centre.is_finite() && radius.is_finite() && radius > 0.0).then_some((centre, radius))
}

pub(crate) mod bound_rows {
    use super::*;
    use crate::state::capabilities::GeometryRequest;

    pub(crate) fn ydr(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::Drawable(d)) => drawable_world_sphere(&d.base),
            DocPayload::Resource(or_io::Asset::DrawableGen9(d)) => drawable_world_sphere(&d.base),
            _ => None,
        }
    }

    pub(crate) fn ydd(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        use crate::ui::panels::selection_ids::ydd_selected_entry_index;

        fn entry<T: DrawableTarget + crate::render::meshes::Material>(
            dict: &PgDictionary<GtaDrawable<T>>,
            index: usize,
        ) -> Option<(Vec3, f32)> {
            drawable_world_sphere(&dict.entries.get(index)?.as_deref()?.base)
        }
        let index = req
            .selection
            .and_then(|sel| ydd_selected_entry_index(&req.doc.payload, sel))?;
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => entry(dict, index),
            DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => entry(dict, index),
            _ => None,
        }
    }

    pub(crate) fn yft(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        fn sphere<T: DrawableTarget + crate::render::meshes::Material>(
            frag: &FragType<T>,
        ) -> Option<(Vec3, f32)> {
            let c = &frag.bounding_sphere;
            let common = frag.common_drawable.as_deref();
            common
                .and_then(|d| drawable_world_sphere(&d.base))
                .or_else(|| {
                    (c.w.is_finite() && c.w > 0.0).then_some((Vec3::new(c.x, c.y, c.z), c.w))
                })
        }
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::Fragment(frag)) => sphere(frag),
            DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => sphere(frag),
            _ => None,
        }
    }

    pub(crate) fn ybn(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        match &req.doc.payload {
            DocPayload::Resource(or_io::Asset::Bounds(bound)) => {
                bound_world_sphere(crate::render::bound_meshes::header(bound))
            }
            _ => None,
        }
    }

    pub(crate) fn ynv(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        let DocPayload::Resource(or_io::Asset::NavMesh(mesh)) = &req.doc.payload else {
            return None;
        };
        let (mn, mx) = crate::render::navmesh::extent(mesh)?;
        let radius = ((mx - mn) * 0.5).length();
        (mn.is_finite() && mx.is_finite() && radius > 0.0).then_some(((mn + mx) * 0.5, radius))
    }

    pub(crate) fn yld(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        use crate::ui::panels::selection_ids::yld_selected_cloth;

        req.selection
            .and_then(|sel| yld_selected_cloth(&req.doc.payload, sel))
            .and_then(cloth_world_sphere)
    }

    pub(crate) fn ypt(req: &GeometryRequest<'_>) -> Option<(Vec3, f32)> {
        ypt_world_sphere(&req.doc.payload, req.selection, req.state.preview_scene.lod)
    }
}

fn cloth_world_sphere(cloth: &or_io::gta5::CharacterCloth) -> Option<(Vec3, f32)> {
    if let Some(verlet) = cloth
        .cloth_controller
        .as_ref()
        .and_then(|c| crate::ui::panels::hierarchy::primary_cloth(c))
    {
        let mn = Vec3::new(verlet.bb_min.x, verlet.bb_min.y, verlet.bb_min.z);
        let mx = Vec3::new(verlet.bb_max.x, verlet.bb_max.y, verlet.bb_max.z);
        if mn.is_finite() && mx.is_finite() && (mx - mn).length_squared() > 1e-8 {
            let centre = (mn + mx) * 0.5;
            let radius = ((mx - mn) * 0.5).length();
            if radius.is_finite() && radius > 0.0 {
                return Some((centre, radius));
            }
        }
        let positions = &verlet.cloth_data.vertex_positions;
        if !positions.is_empty() {
            let mut mn = Vec3::splat(f32::INFINITY);
            let mut mx = Vec3::splat(f32::NEG_INFINITY);
            for v in positions {
                let p = Vec3::new(v.x, v.y, v.z);
                mn = mn.min(p);
                mx = mx.max(p);
            }
            if mn.is_finite() && mx.is_finite() {
                let centre = (mn + mx) * 0.5;
                let radius = ((mx - mn) * 0.5).length().max(0.01);
                return Some((centre, radius));
            }
        }
    }
    cloth
        .bound_composite
        .as_ref()
        .and_then(|c| bound_world_sphere(&c.base))
}

fn ypt_world_sphere(
    payload: &DocPayload,
    selection: Option<u64>,
    lod: usize,
) -> Option<(Vec3, f32)> {
    use crate::ui::panels::selection_ids::{ypt_drawable_for_selection, ypt_tag_of, YPT_MODEL};
    let sel = selection?;
    if ypt_tag_of(sel) != YPT_MODEL {
        return None;
    }
    match payload {
        DocPayload::Resource(or_io::Asset::Particles(list)) => {
            model_sphere(ypt_drawable_for_selection(list, sel)?, lod)
        }
        DocPayload::Resource(or_io::Asset::ParticlesGen9(list)) => {
            model_sphere(ypt_drawable_for_selection(list, sel)?, lod)
        }
        _ => None,
    }
}

/// The sphere that holds what a particle model draws at level `lod`.
///
/// ⚠ Not the bounds sphere the model ships, which some models outgrow: a camera framed on it
/// lands inside them.
fn model_sphere<T: crate::render::meshes::Material>(
    model: &RmcDrawable<T>,
    lod: usize,
) -> Option<(Vec3, f32)> {
    use crate::render::meshes::{drawable_parts, level_drawn};
    let parts = drawable_parts(model, level_drawn(model, lod), None);
    let uploads: Vec<_> = parts.into_iter().map(|(_, upload)| upload).collect();
    crate::render::thumbnails::bounding_sphere(&uploads)
}

fn drawable_world_sphere<T: DrawableTarget + crate::render::meshes::Material>(
    drawable: &RmcDrawable<T>,
) -> Option<(Vec3, f32)> {
    let s = &drawable.lod_group.bounds_sphere.center_and_radius;
    let radius = s.w;
    let header = (radius.is_finite() && radius > 0.0).then(|| (Vec3::new(s.x, s.y, s.z), radius));
    framing_sphere(header, model_sphere(drawable, 0))
}

/// The sphere to frame a drawable by, given the one it ships and the one its vertices fill: the shipped one while it holds the centre of what is drawn, else the drawn one.
///
/// ⚠ A map's water mesh is authored in the map's own frame and ships a sphere about the origin, so a camera framed on that looks at nothing, 90 metres from the mesh.
fn framing_sphere(header: Option<(Vec3, f32)>, drawn: Option<(Vec3, f32)>) -> Option<(Vec3, f32)> {
    match (header, drawn) {
        (Some((centre, radius)), Some((at, _))) if (centre - at).length() <= radius => header,
        (_, Some(drawn)) => Some(drawn),
        (header, None) => header,
    }
}

fn bound_world_sphere(header: &PhBound) -> Option<(Vec3, f32)> {
    let (mn, mx) = crate::render::bound_meshes::header_aabb(header);
    if !mn.is_finite() || !mx.is_finite() {
        return None;
    }
    let centre = (mn + mx) * 0.5;
    let radius = ((mx - mn) * 0.5).length();
    if radius.is_finite() && radius > 0.0 {
        Some((centre, radius))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_sphere_a_drawable_ships_frames_it_while_it_holds_what_is_drawn_and_the_drawn_one_frames_it_otherwise(
    ) {
        let shipped = (Vec3::new(0.2, 0.1, 0.0), 16.0);
        // A mesh in the middle of its sphere, and one a hundred metres from it
        let near = (Vec3::new(3.0, 0.0, 1.0), 5.0);
        let far = (Vec3::new(-63.0, 64.0, 36.0), 16.2);
        assert_eq!(framing_sphere(Some(shipped), Some(near)), Some(shipped));
        assert_eq!(framing_sphere(Some(shipped), Some(far)), Some(far));
        // Nothing drawn leaves the shipped one, and nothing shipped the drawn one
        assert_eq!(framing_sphere(Some(shipped), None), Some(shipped));
        assert_eq!(framing_sphere(None, Some(far)), Some(far));
        assert_eq!(framing_sphere(None, None), None);
    }

    fn fixture(name: &str) -> PathBuf {
        or_test_support::shared_corpus_root(env!("CARGO_MANIFEST_DIR")).join(name)
    }
    #[test]
    fn bvh_ybn_reports_world_space_bounds() {
        let path = fixture("canyonriver01_traincrash_1.ybn");
        if !path.exists() {
            or_test_support::skip_corpus(
                "bvh_ybn_reports_world_space_bounds",
                format_args!("fixture missing at {}", path.display()),
            );
            return;
        }
        let data = std::fs::read(&path).expect("read fixture");
        let bound =
            <or_io::gta5::PhBoundAny as or_io::gta5::File>::from_bytes(&data).expect("parse ybn");
        let (centre, radius) =
            bound_world_sphere(crate::render::bound_meshes::header(&bound)).expect("bounds");
        assert!(radius > 1.0, "radius too small: {radius}");
        assert!(
            centre.length() > 100.0,
            "expected world-space coordinates, got {centre:?}"
        );
    }

    #[test]
    fn an_enhanced_drawable_reports_a_framing_sphere() {
        let Some(path) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
            .target(or_test_support::TARGET_GEN9)
            .find()
            .into_iter()
            .next()
        else {
            or_test_support::skip_corpus(
                "an_enhanced_drawable_reports_a_framing_sphere",
                "no Enhanced .ydr fixture",
            );
            return;
        };
        let data = std::fs::read(&path).expect("read fixture");
        let drawable =
            <or_io::gta5::GtaDrawable<or_io::decl::Gen9> as or_io::gta5::File>::from_bytes(&data)
                .expect("read an Enhanced drawable");

        let (_, radius) = drawable_world_sphere(&drawable.base)
            .expect("an Enhanced drawable must report a framing sphere");
        assert!(
            radius.is_finite() && radius > 0.0,
            "radius {radius} is not something the camera can frame"
        );
    }
}
