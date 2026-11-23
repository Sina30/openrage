use super::capabilities;
use super::document::DocPayload;
use crate::ui::panels::selection_ids::{dict_entry_id, DOC_ROOT_SEL};

#[derive(Debug, Clone)]
pub struct DocumentModel {
    pub name: String,

    pub selection_id: u64,
}

pub enum ModelListing {
    Models(Vec<DocumentModel>),

    Nothing(&'static str),
}

const NO_MODELS: &str = "This file has no models";

pub fn models_in(payload: &DocPayload) -> ModelListing {
    match capabilities::for_payload(payload).and_then(|row| row.models) {
        Some(list) => list(payload),
        None => ModelListing::Nothing(NO_MODELS),
    }
}

fn entry_label(name: &str, hash: u32) -> String {
    if name.is_empty() {
        format!("#{hash:08X}")
    } else {
        name.to_string()
    }
}

pub(crate) mod rows {
    use super::*;

    pub(crate) fn ydd(payload: &DocPayload) -> ModelListing {
        fn entries<T: or_io::gta5::DrawableTarget>(
            dict: &or_io::gta5::PgDictionary<or_io::gta5::GtaDrawable<T>>,
        ) -> Vec<DocumentModel> {
            (dict.codes.iter().zip(dict.entries.iter()))
                .map(|(&hash, entry)| DocumentModel {
                    name: entry_label(entry.as_deref().map_or("", |d| d.name.as_str()), hash),
                    selection_id: dict_entry_id(hash),
                })
                .collect()
        }
        let models: Vec<DocumentModel> = match payload {
            DocPayload::Resource(or_io::Asset::DrawableDict(dict)) => entries(dict),
            DocPayload::Resource(or_io::Asset::DrawableDictGen9(dict)) => entries(dict),
            _ => return ModelListing::Nothing(NO_MODELS),
        };
        if models.is_empty() {
            return ModelListing::Nothing("This dictionary is empty");
        }
        ModelListing::Models(models)
    }

    pub(crate) fn ypt(payload: &DocPayload) -> ModelListing {
        fn models<T: or_io::gta5::DrawableTarget>(
            list: &or_io::gta5::PtxFxList<T>,
        ) -> Vec<DocumentModel> {
            or_particle::list::models(list)
                .map(|(hash, _)| {
                    let name = or_particle::list::model_info(list, hash)
                        .map(|d| d.name.clone())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| format!("#{hash:08X}"));
                    DocumentModel {
                        name,
                        selection_id: dict_entry_id(hash),
                    }
                })
                .collect()
        }
        let models = match payload {
            DocPayload::Resource(or_io::Asset::Particles(list)) => models(list),
            DocPayload::Resource(or_io::Asset::ParticlesGen9(list)) => models(list),
            _ => return ModelListing::Nothing(NO_MODELS),
        };
        if models.is_empty() {
            return ModelListing::Nothing("This particle file has no embedded models");
        }
        ModelListing::Models(models)
    }

    pub(crate) fn ydr(payload: &DocPayload) -> ModelListing {
        let name = match payload {
            DocPayload::Resource(or_io::Asset::Drawable(d)) => d.name.clone(),
            DocPayload::Resource(or_io::Asset::DrawableGen9(d)) => d.name.clone(),
            _ => return ModelListing::Nothing(NO_MODELS),
        };
        ModelListing::Models(vec![DocumentModel {
            name: if name.is_empty() {
                "(unnamed drawable)".to_string()
            } else {
                name
            },
            selection_id: DOC_ROOT_SEL,
        }])
    }

    pub(crate) fn yft(payload: &DocPayload) -> ModelListing {
        let (name, common) = match payload {
            DocPayload::Resource(or_io::Asset::Fragment(frag)) => {
                (&frag.tune_name, frag.common_drawable.is_some())
            }
            DocPayload::Resource(or_io::Asset::FragmentGen9(frag)) => {
                (&frag.tune_name, frag.common_drawable.is_some())
            }
            _ => return ModelListing::Nothing(NO_MODELS),
        };
        if !common {
            return ModelListing::Nothing("This fragment has no common drawable");
        }
        ModelListing::Models(vec![DocumentModel {
            name: if name.is_empty() {
                "(unnamed fragment)".to_string()
            } else {
                name.clone()
            },
            selection_id: DOC_ROOT_SEL,
        }])
    }

    /// A scanned GTA VI body's drawable, its own or its fragment's, else each of its dictionary's.
    pub(crate) fn gta6(payload: &DocPayload) -> ModelListing {
        use or_io::gta6::headerless::Resource;
        let DocPayload::Gta6Drawable(resource) = payload else {
            return ModelListing::Nothing(NO_MODELS);
        };
        let own = match &**resource {
            Resource::Drawable(d) => Some(&d.name),
            Resource::DrawableWithBound(r) => Some(&r.name),
            _ => None,
        };
        if let Some(name) = own {
            return ModelListing::Models(vec![DocumentModel {
                name: match name.is_empty() {
                    true => "(unnamed drawable)".to_string(),
                    false => name.clone(),
                },
                selection_id: DOC_ROOT_SEL,
            }]);
        }
        // No field of a cloth names it, and the archive's names are encrypted.
        if let Resource::Cloth(_) = &**resource {
            return ModelListing::Models(vec![DocumentModel {
                name: tr!("GTA VI Cloth").to_string(),
                selection_id: DOC_ROOT_SEL,
            }]);
        }
        let models: Vec<DocumentModel> =
            crate::state::gta6_resource::dictionary_drawables(resource)
                .map(|(code, d)| DocumentModel {
                    name: entry_label(&d.name, code),
                    selection_id: dict_entry_id(code),
                })
                .collect();
        if models.is_empty() {
            return ModelListing::Nothing(NO_MODELS);
        }
        ModelListing::Models(models)
    }

    pub(crate) fn yld(payload: &DocPayload) -> ModelListing {
        let DocPayload::Resource(or_io::Asset::ClothDict(dict)) = payload else {
            return ModelListing::Nothing(NO_MODELS);
        };
        let models: Vec<DocumentModel> = dict
            .codes
            .iter()
            .zip(dict.entries.iter())
            .map(|(&hash, entry)| DocumentModel {
                name: entry_label(
                    entry
                        .as_deref()
                        .map_or("", crate::ui::panels::hierarchy::cloth_name),
                    hash,
                ),
                selection_id: dict_entry_id(hash),
            })
            .collect();
        if models.is_empty() {
            return ModelListing::Nothing("This dictionary is empty");
        }
        ModelListing::Models(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_format_with_no_models_says_so() {
        assert!(matches!(
            models_in(&DocPayload::Resource(or_io::Asset::Clips(
                Default::default()
            ))),
            ModelListing::Nothing(_)
        ));
        assert!(matches!(
            models_in(&DocPayload::Resource(or_io::Asset::TextureDict(
                Default::default()
            ))),
            ModelListing::Nothing(_)
        ));
    }

    #[test]
    fn an_empty_dictionary_says_so() {
        assert!(matches!(
            models_in(&DocPayload::Resource(or_io::Asset::DrawableDict(
                Default::default()
            ))),
            ModelListing::Nothing(_)
        ));
    }

    #[test]
    fn a_blank_entry_name_falls_back_to_its_hash() {
        assert_eq!(entry_label("", 0xDEAD_BEEF), "#DEADBEEF");
        assert_eq!(entry_label("jbib_000", 0xDEAD_BEEF), "jbib_000");
    }

    fn gta6_drawable(name: &str) -> or_io::gta6::headerless::Drawable {
        or_io::gta6::headerless::Drawable {
            name: name.to_string(),
            box_min: [0.0; 3],
            box_max: [1.0; 3],
            shaders: Vec::new(),
            levels: [None, None, None, None],
            lod_distances: [0.0; 4],
            bound: None,
            embedded: None,
            extra_skeletons: Vec::new(),
        }
    }

    #[test]
    fn a_gta_vi_body_lists_its_drawable_or_each_of_its_dictionarys_and_each_tile_draws_its_own() {
        use or_io::gta6::headerless::{Dictionary, Entry, Resource};
        use std::sync::Arc;

        let single = DocPayload::Gta6Drawable(Arc::new(Resource::Drawable(gta6_drawable("lone"))));
        let ModelListing::Models(models) = rows::gta6(&single) else {
            panic!("a drawable is a model");
        };
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "lone");
        assert_eq!(models[0].selection_id, DOC_ROOT_SEL);

        let (first, second) = (0x0A0B_0C0Du32, 0x7F6E_5D4Cu32);
        let dict = Resource::Dictionary(Dictionary {
            entries: vec![
                (
                    first,
                    Ok(Entry::Drawable(Box::new(gta6_drawable("head_000")))),
                ),
                (0x1234_5678, Ok(Entry::Word64(9))),
                (second, Ok(Entry::Drawable(Box::new(gta6_drawable(""))))),
            ],
        });
        let payload = DocPayload::Gta6Drawable(Arc::new(dict));
        let ModelListing::Models(models) = models_in(&payload) else {
            panic!("a dictionary of drawables lists them");
        };
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["head_000", entry_label("", second).as_str()]);
        let DocPayload::Gta6Drawable(resource) = &payload else {
            unreachable!()
        };
        for (model, code) in models.iter().zip([first, second]) {
            assert_eq!(model.selection_id, dict_entry_id(code));
            use crate::state::gta6_resource::{dictionary_drawables, drawable_of};
            let drawn = drawable_of(resource, Some(model.selection_id));
            let wanted = dictionary_drawables(resource)
                .find(|(c, _)| *c == code)
                .map(|(_, d)| d as *const _);
            assert_eq!(
                drawn.map(|d| d as *const _),
                wanted,
                "tile {code:#X} draws another entry"
            );
        }

        let empty = DocPayload::Gta6Drawable(Arc::new(Resource::Dictionary(Dictionary {
            entries: vec![(1, Ok(Entry::Word64(2)))],
        })));
        assert!(matches!(models_in(&empty), ModelListing::Nothing(_)));
    }

    #[test]
    fn a_gta_vi_cloth_is_one_tile_of_the_whole_body() {
        use or_io::gta6::headerless::{Cloth, ClothKind, Resource};
        let cloth = Resource::Cloth(Cloth {
            flag: 0,
            bytes: [0, 0],
            owner: 0,
            record: None,
            kind: ClothKind::Edges {
                vertices: vec![[0.0; 4], [1.0, 0.0, 0.0, 1.0]],
                edges: Vec::new(),
            },
        });
        let payload = DocPayload::Gta6Drawable(std::sync::Arc::new(cloth));
        let ModelListing::Models(models) = models_in(&payload) else {
            panic!("a cloth is a model");
        };
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].selection_id, DOC_ROOT_SEL);
    }

    #[test]
    fn a_tile_id_is_the_hierarchys_id() {
        let hash = 0x1234_5678u32;
        assert_eq!(
            dict_entry_id(hash),
            crate::ui::panels::selection_ids::ypt_model_id(hash),
            "the model gallery and the outline share one encoding"
        );
    }
}
