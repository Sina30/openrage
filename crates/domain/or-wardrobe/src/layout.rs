//! Where a pack's files go in a resource, and what they are called.
//!
//! The layout is the one the staged packs share: the streamed files of a pack in `stream/<pack>/`,
//! a folder for each ped's components and one for its props, a folder for each slot, the metadata of
//! each ped beside them and the creature metadata in a folder of its own.

use crate::names::{Ped, Skin, Slot};

/// `mp_m_freemode_01_<collection>`, or `mp_m_freemode_01_p_<collection>` for the props; the ped's own drawables, which are in no collection, are `mp_m_freemode_01` and `mp_m_freemode_01_p`.
pub fn stem(ped: Ped, collection: &str, prop: bool) -> String {
    match (collection.is_empty(), prop) {
        (true, true) => format!("{}_p", ped.model()),
        (true, false) => ped.model().to_string(),
        (false, true) => format!("{}_p_{collection}", ped.model()),
        (false, false) => format!("{}_{collection}", ped.model()),
    }
}

/// The folder of a slot's files.
pub fn directory(pack: &str, ped: Ped, collection: &str, slot: Slot) -> String {
    format!(
        "stream/{pack}/{}/{}",
        stem(ped, collection, slot.is_prop()),
        slot.name()
    )
}

/// The file name of the model at `position`: `<stem>^jbib_012_u.ydd`, or `<stem>^p_head_005.ydd` for a prop.
pub fn model_name(ped: Ped, collection: &str, slot: Slot, position: u32, skin: Skin) -> String {
    let stem = stem(ped, collection, slot.is_prop());
    match slot.is_prop() {
        true => format!("{stem}^{}_{position:03}.ydd", slot.name()),
        false => format!("{stem}^{}_{position:03}_{}.ydd", slot.name(), skin.letter()),
    }
}

/// The file name of variation `variation` of the garment at `position`, lettered from `a`.
pub fn texture_name(
    ped: Ped,
    collection: &str,
    slot: Slot,
    position: u32,
    variation: usize,
    tint: &str,
) -> String {
    let stem = stem(ped, collection, slot.is_prop());
    let letter = char::from(b'a' + u8::try_from(variation).unwrap_or(25).min(25));
    match slot.is_prop() {
        true => format!("{stem}^{}_diff_{position:03}_{letter}.ytd", slot.name()),
        false => format!(
            "{stem}^{}_diff_{position:03}_{letter}_{tint}.ytd",
            slot.name()
        ),
    }
}

/// The file name of alternate `number` (from 1) of the model named `model`: `<stem>_1.ydd`.
pub fn alternate_name(model: &str, number: usize) -> String {
    let stem = model.strip_suffix(".ydd").unwrap_or(model);
    format!("{stem}_{number}.ydd")
}

/// The file name of the cloth of the model named `model`: the same name with `.yld`, beside it.
pub fn cloth_name(model: &str) -> String {
    let stem = model.strip_suffix(".ydd").unwrap_or(model);
    format!("{stem}.yld")
}

/// A ped's variation metadata.
pub fn variation_path(pack: &str, ped: Ped, collection: &str) -> String {
    format!("stream/{pack}/{}.ymt", stem(ped, collection, false))
}

/// The creature metadata both peds of the pack share.
pub fn creature_path(pack: &str) -> String {
    creature_path_of(pack, 0)
}

/// The name collection `part` files its creature metadata under: the pack's for the pack's own collection, `<pack>_<n>` after it.
pub fn creature_pack(pack: &str, part: u32) -> String {
    match part {
        0 => pack.to_string(),
        n => format!("{pack}_{n}"),
    }
}

/// The creature metadata of collection `part`, in the folder of the pack.
pub fn creature_path_of(pack: &str, part: u32) -> String {
    let name = creature_pack(pack, part);
    format!("stream/{pack}/creaturemetadata/mp_creaturemetadata_{name}.ymt")
}

/// The name a ped's shop metadata gives the creature metadata, which the game resolves without regard to case.
pub fn creature_name(pack: &str) -> String {
    format!("MP_CreatureMetadata_{pack}")
}

/// A ped's shop metadata, the file the manifest registers the collection with.
pub fn shop_path(ped: Ped, collection: &str) -> String {
    format!("data/{}.meta", stem(ped, collection, false))
}

/// Where the files of a pack go in a build of one kind, and what each is called there.
///
/// A FiveM resource has the layout of the free functions above. A pack of a singleplayer DLC has the
/// game's own: an archive for each ped's components and one for its props, a folder named for the
/// collection in each, and the files in it named without the collection (`jbib_012_u.ydd`), since
/// the folder says whose they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pack: String,
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// `stream/<pack>/` with a folder for each slot where `subfolders` says so, and `data/`.
    Resource { subfolders: bool },
    /// The tree of a `dlc.rpf`.
    Pack,
}

impl Paths {
    /// The layout of a FiveM resource.
    pub fn resource(pack: &str, subfolders: bool) -> Self {
        Self {
            pack: pack.to_string(),
            kind: Kind::Resource { subfolders },
        }
    }

    /// The layout of a singleplayer DLC pack, below the pack's own root.
    pub fn pack(pack: &str) -> Self {
        Self {
            pack: pack.to_string(),
            kind: Kind::Pack,
        }
    }

    pub fn is_pack(&self) -> bool {
        self.kind == Kind::Pack
    }

    /// The archive that holds a ped's components, or its props.
    fn archive(&self, ped: Ped, prop: bool) -> String {
        let who = match ped {
            Ped::Male => "male",
            Ped::Female => "female",
        };
        let props = if prop { "_p" } else { "" };
        format!("x64/models/cdimages/{}_{who}{props}.rpf", self.pack)
    }

    /// The folder of a slot's files.
    pub fn directory(&self, ped: Ped, collection: &str, slot: Slot) -> String {
        match self.kind {
            Kind::Resource { subfolders: true } => directory(&self.pack, ped, collection, slot),
            Kind::Resource { subfolders: false } => format!(
                "stream/{}/{}",
                self.pack,
                stem(ped, collection, slot.is_prop())
            ),
            Kind::Pack => format!(
                "{}/{}",
                self.archive(ped, slot.is_prop()),
                stem(ped, collection, slot.is_prop())
            ),
        }
    }

    /// The name a file of the pack has in its folder: `name` is the one the resource layout gives it.
    fn named(&self, ped: Ped, collection: &str, slot: Slot, name: String) -> String {
        match self.kind {
            Kind::Resource { .. } => name,
            Kind::Pack => {
                let prefix = format!("{}^", stem(ped, collection, slot.is_prop()));
                name.strip_prefix(&prefix).unwrap_or(&name).to_string()
            }
        }
    }

    pub fn model_name(
        &self,
        ped: Ped,
        collection: &str,
        slot: Slot,
        position: u32,
        skin: Skin,
    ) -> String {
        let name = model_name(ped, collection, slot, position, skin);
        self.named(ped, collection, slot, name)
    }

    pub fn texture_name(
        &self,
        ped: Ped,
        collection: &str,
        slot: Slot,
        position: u32,
        variation: usize,
        tint: &str,
    ) -> String {
        let name = texture_name(ped, collection, slot, position, variation, tint);
        self.named(ped, collection, slot, name)
    }

    /// A ped's variation metadata.
    pub fn variation_path(&self, ped: Ped, collection: &str) -> String {
        match self.kind {
            Kind::Resource { .. } => variation_path(&self.pack, ped, collection),
            Kind::Pack => format!(
                "{}/{}.ymt",
                self.archive(ped, false),
                stem(ped, collection, false)
            ),
        }
    }

    /// The creature metadata both peds of the pack share.
    pub fn creature_path(&self) -> String {
        self.creature_path_of(0)
    }

    /// The creature metadata of collection `part`, which both peds share.
    pub fn creature_path_of(&self, part: u32) -> String {
        match self.kind {
            Kind::Resource { .. } => creature_path_of(&self.pack, part),
            Kind::Pack => format!(
                "x64/anim/creaturemetadata.rpf/mp_creaturemetadata_{}.ymt",
                creature_pack(&self.pack, part)
            ),
        }
    }

    /// A ped's shop metadata, the file that registers the collection.
    pub fn shop_path(&self, ped: Ped, collection: &str) -> String {
        match self.kind {
            Kind::Resource { .. } => shop_path(ped, collection),
            Kind::Pack => format!("common/data/{}_shop.meta", stem(ped, collection, false)),
        }
    }

    /// The alternate variations of the pack.
    pub fn alternate_path(&self) -> String {
        match self.kind {
            Kind::Resource { .. } => "data/pedalternatevariations.meta".to_string(),
            Kind::Pack => format!("common/data/{}_pedalternatevariations.meta", self.pack),
        }
    }

    /// The first person alternates of the pack.
    pub fn first_person_path(&self) -> String {
        match self.kind {
            Kind::Resource { .. } => "data/pedfirstpersonalternates.meta".to_string(),
            Kind::Pack => format!("common/data/{}_pedfirstpersonalternates.meta", self.pack),
        }
    }

    /// Where a file the manifest registered with the shop is shipped.
    pub fn carried_path(&self, carried: &crate::model::Carried) -> String {
        let name = carried.file.rsplit('/').next().unwrap_or(&carried.file);
        match self.kind {
            Kind::Resource { .. } => format!("data/{name}"),
            Kind::Pack => format!("common/data/{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Component, Prop};

    const JBIB: Slot = Slot::Component(Component::Jbib);

    #[test]
    fn a_component_is_named_as_the_staged_pack_names_it() {
        let c = "mp_m_unity_clothes";
        assert_eq!(
            model_name(Ped::Male, c, JBIB, 12, Skin::Universal),
            "mp_m_freemode_01_mp_m_unity_clothes^jbib_012_u.ydd"
        );
        assert_eq!(
            texture_name(Ped::Male, c, JBIB, 12, 2, "uni"),
            "mp_m_freemode_01_mp_m_unity_clothes^jbib_diff_012_c_uni.ytd"
        );
        // The cloth of a drawable is the file of its model with `.yld`, beside it.
        assert_eq!(
            cloth_name(&model_name(Ped::Male, c, JBIB, 12, Skin::Universal)),
            "mp_m_freemode_01_mp_m_unity_clothes^jbib_012_u.yld"
        );
        assert_eq!(
            directory("unity", Ped::Male, c, JBIB),
            "stream/unity/mp_m_freemode_01_mp_m_unity_clothes/jbib"
        );
        assert_eq!(
            variation_path("unity", Ped::Male, c),
            "stream/unity/mp_m_freemode_01_mp_m_unity_clothes.ymt"
        );
    }

    #[test]
    fn a_prop_is_named_with_the_p_and_has_no_skin_or_tint() {
        let head = Slot::Prop(Prop::Head);
        let c = "mp_f_unity_clothes";
        assert_eq!(
            model_name(Ped::Female, c, head, 5, Skin::Race),
            "mp_f_freemode_01_p_mp_f_unity_clothes^p_head_005.ydd"
        );
        assert_eq!(
            texture_name(Ped::Female, c, head, 5, 1, "whi"),
            "mp_f_freemode_01_p_mp_f_unity_clothes^p_head_diff_005_b.ytd"
        );
        assert_eq!(
            directory("unity", Ped::Female, c, head),
            "stream/unity/mp_f_freemode_01_p_mp_f_unity_clothes/p_head"
        );
    }

    #[test]
    fn the_names_the_layout_writes_are_the_names_the_resolver_reads() {
        use crate::resolve::{resolve, Role};
        let c = "mp_m_unity_clothes";
        for slot in Slot::all() {
            let model = resolve(&model_name(Ped::Male, c, slot, 7, Skin::Race));
            assert_eq!(
                (model.ped, model.slot, model.position, model.role),
                (Some(Ped::Male), Some(slot), Some(7), Some(Role::Model))
            );
            assert_eq!(model.collection.as_deref(), Some(c));
            assert_eq!(model.skin.is_some(), !slot.is_prop());
            let cloth = resolve(&cloth_name(&model_name(Ped::Male, c, slot, 7, Skin::Race)));
            assert_eq!(
                (cloth.role, cloth.slot, cloth.position, cloth.skin),
                (Some(Role::Cloth), Some(slot), Some(7), model.skin)
            );
            let texture = resolve(&texture_name(Ped::Male, c, slot, 7, 3, "whi"));
            assert_eq!(
                (texture.slot, texture.position, texture.variant),
                (Some(slot), Some(7), Some(3))
            );
            assert_eq!(texture.tint.as_deref(), (!slot.is_prop()).then_some("whi"));
        }
    }
}
