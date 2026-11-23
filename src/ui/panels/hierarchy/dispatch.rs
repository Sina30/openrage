//! The two tables [`crate::state::capabilities`] holds function pointers
//! into: which drawer paints a document's outline, and whether it has one
//! worth an expander.

use super::*;

pub(crate) mod outline {
    use super::*;

    pub(crate) fn ypt(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_ypt(ui, state, doc_id);
    }

    pub(crate) fn ytd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        match state.document(doc_id).map(|d| &d.payload) {
            Some(DocPayload::Resource(or_io::Asset::TextureDict(_))) => draw_ytd(ui, state, doc_id),
            Some(DocPayload::Resource(or_io::Asset::TextureDictGen9(_))) => {
                draw_ytd_gen9(ui, state, doc_id)
            }
            _ => {}
        }
    }

    pub(crate) fn ydr(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        match state.document(doc_id).map(|d| &d.payload) {
            Some(DocPayload::Resource(or_io::Asset::Drawable(_))) => draw_ydr(ui, state, doc_id),
            Some(DocPayload::Resource(or_io::Asset::DrawableGen9(_))) => {
                draw_ydr_gen9(ui, state, doc_id)
            }
            _ => {}
        }
    }

    pub(crate) fn ydd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        match state.document(doc_id).map(|d| &d.payload) {
            Some(DocPayload::Resource(or_io::Asset::DrawableDict(_))) => {
                draw_ydd(ui, state, doc_id)
            }
            Some(DocPayload::Resource(or_io::Asset::DrawableDictGen9(_))) => {
                draw_ydd_gen9(ui, state, doc_id)
            }
            _ => {}
        }
    }

    pub(crate) fn yft(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_yft(ui, state, doc_id);
    }

    pub(crate) fn ybn(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_ybn(ui, state, doc_id);
    }

    pub(crate) fn yld(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_yld(ui, state, doc_id);
    }

    pub(crate) fn ycd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_ycd(ui, state, doc_id);
    }

    pub(crate) fn yed(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        yed::draw_yed(ui, state, doc_id);
    }

    pub(crate) fn yfd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        yfd::draw_yfd(ui, state, doc_id);
    }

    pub(crate) fn ynd(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        ynd::draw_ynd(ui, state, doc_id);
    }

    pub(crate) fn ynv(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        ynv::draw_ynv(ui, state, doc_id);
    }

    pub(crate) fn ypdb(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        ypdb::draw_ypdb(ui, state, doc_id);
    }

    pub(crate) fn fxc(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_fxc(ui, state, doc_id);
    }

    pub(crate) fn rpf(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_rpf(ui, state, doc_id);
    }

    pub(crate) fn ttpf(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        draw_ttpf(ui, state, doc_id);
    }

    pub(crate) fn gta6_drawable(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        use or_io::gta6::headerless::Resource;
        let bound = matches!(
            state.document(doc_id).map(|d| &d.payload),
            Some(DocPayload::Gta6Drawable(r)) if matches!(&**r, Resource::Bound(_))
        );
        match bound {
            true => super::gta6_bound::draw_gta6_bound(ui, state, doc_id),
            false => super::super::gta6_drawable::draw_gta6_drawable(ui, state, doc_id),
        }
    }

    pub(crate) fn gta6_anim(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
        gta6_anim::draw_gta6_anim(ui, state, doc_id);
    }
}

pub(crate) mod children {
    use super::*;

    pub(crate) fn always(_: &DocPayload) -> bool {
        true
    }

    pub(crate) fn ttpf(payload: &DocPayload) -> bool {
        matches!(payload, DocPayload::Ttpf(ttpf) if !ttpf.pack.entries.is_empty())
    }

    /// A scanned GTA VI drawable, a dictionary of drawables or a bound, whose parts are rows.
    pub(crate) fn gta6_drawable(payload: &DocPayload) -> bool {
        use or_io::gta6::headerless::Resource;
        match payload {
            DocPayload::Gta6Drawable(r) => match &**r {
                Resource::Drawable(_) | Resource::DrawableWithBound(_) | Resource::Bound(_) => true,
                r => {
                    crate::state::gta6_resource::dictionary_drawables(r)
                        .next()
                        .is_some()
                        || super::super::gta6_filters::holds_filters(r)
                }
            },
            _ => false,
        }
    }

    pub(crate) fn gta6_anim(payload: &DocPayload) -> bool {
        matches!(payload, DocPayload::Gta6Anim(r) if crate::ui::panels::inspector::gta6_choice_count(r) > 0)
    }

    pub(crate) fn ypt(payload: &DocPayload) -> bool {
        use crate::state::particle_target::{on_particle_target, ParticleTarget};
        use or_particle::list;
        on_particle_target!(payload, T => T::list(payload).is_some_and(|asset| {
            list::effect_rules(asset).next().is_some()
                || list::emitter_rules(asset).next().is_some()
                || list::particle_rules(asset).next().is_some()
                || (asset.texture_dict.as_ref()).is_some_and(|d| !d.entries.is_empty())
                || list::models(asset).next().is_some()
        }), else false)
    }

    pub(crate) fn ytd(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Resource(or_io::Asset::TextureDict(td)) => {
                td.entries.iter().any(Option::is_some)
            }
            DocPayload::Resource(or_io::Asset::TextureDictGen9(td)) => {
                td.entries.iter().any(Option::is_some)
            }
            _ => false,
        }
    }

    pub(crate) fn ydd(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => !dict.entries.is_empty(),
            DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => !dict.entries.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn yld(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Resource(or_io::Asset::ClothDict(dict)) => !dict.entries.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn yed(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Resource(or_io::Asset::ExpressionDict(dict)) => !dict.entries.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn yfd(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Resource(or_io::Asset::FrameFilterDict(dict)) => !dict.entries.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn ypdb(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Ypdb(db) => !db.samples.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn fxc(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Fxc(d) => !d.effect.variables.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn ycd(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Resource(or_io::Asset::Clips(dict)) => {
                or_anim::clip::clips(dict).next().is_some()
            }
            _ => false,
        }
    }

    pub(crate) fn rpf(payload: &DocPayload) -> bool {
        match payload {
            DocPayload::Rpf(rpf) => {
                let root_id = rpf.session.root_id();
                !rpf.session.children_of(root_id).is_empty()
            }
            _ => false,
        }
    }
}
