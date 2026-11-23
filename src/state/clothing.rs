//! A ped garment's texture variants: the `.ytd` files beside it that dress it.
//!
//! Ped clothing pairs a drawable to its textures by file name alone:
//! `<slot>_<index>_<suffix>.ydd` is dressed by `<slot>_diff_<index>_<variant>_<skin>.ytd`, one
//! file per variant the game's texture slider offers, and an add-on pack prefixes every file with
//! `<pack>^`.
//!
//! ⚠ The textures' own names pair nothing: an add-on pack's files carry stale ones (the variants
//! of one garment hold `2`, `1` and `15`), so a chosen variant's first texture is what the
//! viewport draws the garment's diffuse with, whatever name the material states.

use std::collections::HashMap;

use super::document::{DocPayload, DocumentId, RpfNodeId};
use super::texture_resolution::stem;
use super::{AppState, DocumentKind, RpfEntryBinding};

/// One dictionary that dresses a garment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClothingVariant {
    pub node: RpfNodeId,
    /// The file's name, as Files shows it.
    pub file: String,
    /// `a`, `b`, and on, as the game's texture slider counts them.
    pub variant: String,
    /// `uni`, or a race token; none for a prop.
    pub skin: Option<String>,
}

impl ClothingVariant {
    /// `a · uni`.
    pub fn label(&self) -> String {
        match &self.skin {
            Some(skin) => format!("{} \u{00B7} {skin}", self.variant),
            None => self.variant.clone(),
        }
    }
}

/// What dresses each garment, and the variant it is drawn with.
#[derive(Default)]
pub struct Clothing {
    /// Looked for once per document; empty for a document that is no garment.
    variants: HashMap<DocumentId, Vec<ClothingVariant>>,
    chosen: HashMap<DocumentId, Chosen>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chosen {
    node: RpfNodeId,
    /// The dictionary's document, which the app opened for itself unless the user had it open.
    dictionary: DocumentId,
}

/// `(slot, index)` of a garment's stem: `pack^accs_026_u` is (`pack^accs`, `026`).
fn garment(stem: &str) -> Option<(String, String)> {
    let lower = stem.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('_').collect();
    let at = parts
        .iter()
        .rposition(|p| p.len() >= 3 && p.bytes().all(|b| b.is_ascii_digit()))?;
    // The index leads a suffix of at most one part: `u`, `r`, or none for a prop.
    if at == 0 || parts.len() - at > 2 {
        return None;
    }
    Some((parts[..at].join("_"), parts[at].to_string()))
}

/// `(variant, skin)` when `file` is a dictionary dressing the garment `slot` and `index` name.
fn variant_of(file: &str, slot: &str, index: &str) -> Option<(String, Option<String>)> {
    let (stem, ext) = file.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case("ytd") {
        return None;
    }
    let lower = stem.to_ascii_lowercase();
    let rest = lower
        .strip_prefix(slot)?
        .strip_prefix("_diff_")?
        .strip_prefix(index)?
        .strip_prefix('_')?;
    let mut parts = rest.split('_');
    let variant = parts.next()?;
    if !(1..=2).contains(&variant.len()) || !variant.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let skin = parts.next().map(str::to_string);
    if parts.next().is_some() || skin.as_deref() == Some("") {
        return None;
    }
    Some((variant.to_string(), skin))
}

/// The dictionaries among `names` that dress the garment `stem` names, in the order the game numbers them.
fn variants_among<'a>(
    stem: &str,
    names: impl Iterator<Item = (RpfNodeId, &'a str)>,
) -> Vec<ClothingVariant> {
    let Some((slot, index)) = garment(stem) else {
        return Vec::new();
    };
    let mut found: Vec<ClothingVariant> = names
        .filter_map(|(node, file)| {
            let (variant, skin) = variant_of(file, &slot, &index)?;
            Some(ClothingVariant {
                node,
                file: file.to_string(),
                variant,
                skin,
            })
        })
        .collect();
    found.sort_by(|a, b| {
        (a.variant.len(), &a.variant, &a.skin).cmp(&(b.variant.len(), &b.variant, &b.skin))
    });
    found
}

impl AppState {
    /// What dresses the garment `doc`, once it has been looked for.
    pub fn garment_variants(&self, doc: DocumentId) -> &[ClothingVariant] {
        self.clothing
            .variants
            .get(&doc)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The file `doc` is drawn with.
    pub fn garment_variant(&self, doc: DocumentId) -> Option<RpfNodeId> {
        self.clothing.chosen.get(&doc).map(|c| c.node)
    }

    /// The document of the dictionary `doc` is drawn with.
    pub fn garment_dictionary(&self, doc: DocumentId) -> Option<DocumentId> {
        self.clothing.chosen.get(&doc).map(|c| c.dictionary)
    }

    /// Look beside the active document for what dresses it, once, and draw it with the first; whether that changed what is drawn.
    pub fn ensure_garment_dressed(&mut self) -> bool {
        let Some(doc) = self.active_document() else {
            return false;
        };
        let id = doc.id;
        if self.clothing.variants.contains_key(&id) {
            return false;
        }
        let variants = self.variants_beside(id);
        let first = variants.first().map(|v| v.node);
        self.clothing.variants.insert(id, variants);
        let Some(first) = first else {
            return false;
        };
        match self.choose_garment_variant(id, first) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "a garment's first texture variant did not open");
                false
            }
        }
    }

    fn variants_beside(&self, doc: DocumentId) -> Vec<ClothingVariant> {
        let Some(doc) = self.document(doc) else {
            return Vec::new();
        };
        if !matches!(
            doc.kind,
            DocumentKind::Ydd | DocumentKind::Ydr | DocumentKind::Yft
        ) {
            return Vec::new();
        }
        let (Some(binding), Some(stem)) = (doc.rpf_entry_binding, stem(doc)) else {
            return Vec::new();
        };
        let Some(DocPayload::Rpf(rpf)) = self.document(binding.parent_doc_id).map(|d| &d.payload)
        else {
            return Vec::new();
        };
        let Some(folder) = rpf
            .session
            .node(binding.entry_id)
            .and_then(|n| n.parent)
            .and_then(|parent| rpf.session.node(parent))
        else {
            return Vec::new();
        };
        variants_among(
            &stem,
            folder.children.iter().filter_map(|id| {
                let node = rpf.session.node(*id)?;
                (!node.is_dir()).then_some((*id, node.name.as_str()))
            }),
        )
    }

    /// Draw the garment `doc` with the dictionary at `node`, among its variants.
    pub fn choose_garment_variant(
        &mut self,
        doc: DocumentId,
        node: RpfNodeId,
    ) -> Result<(), String> {
        if self.garment_variant(doc) == Some(node) {
            return Ok(());
        }
        let Some(binding) = self.document(doc).and_then(|d| d.rpf_entry_binding) else {
            return Err(tr!("That garment is not in a folder or an archive").to_string());
        };
        if !self.garment_variants(doc).iter().any(|v| v.node == node) {
            return Err(tr!("That file does not dress this garment").to_string());
        }
        let row = RpfEntryBinding {
            parent_doc_id: binding.parent_doc_id,
            entry_id: node,
        };
        let dictionary = match self.entry_document(row) {
            Some(open) => open,
            None => {
                let id = self.open_archive_entry_now(row)?;
                self.auto_opened_texture_docs.insert(id);
                id
            }
        };
        if !self
            .document(dictionary)
            .is_some_and(|d| d.payload.kind() == DocumentKind::Ytd)
        {
            self.release_garment_dictionary(dictionary);
            return Err(tr!("That file is not a texture dictionary").to_string());
        }
        let previous = self
            .clothing
            .chosen
            .insert(doc, Chosen { node, dictionary });
        if let Some(previous) = previous {
            self.release_garment_dictionary(previous.dictionary);
        }
        Ok(())
    }

    /// Close `dictionary` when the app opened it for a garment and no garment is drawn with it now.
    fn release_garment_dictionary(&mut self, dictionary: DocumentId) {
        let in_use = self
            .clothing
            .chosen
            .values()
            .any(|c| c.dictionary == dictionary);
        if !in_use && self.auto_opened_texture_docs.contains(&dictionary) {
            self.drop_document(dictionary);
        }
    }

    /// Forget what dresses `id`, and close what the app opened for it; `id` is a closed document or a dictionary.
    pub(super) fn forget_clothing(&mut self, id: DocumentId) {
        self.clothing.variants.remove(&id);
        if let Some(gone) = self.clothing.chosen.remove(&id) {
            self.release_garment_dictionary(gone.dictionary);
        }
        // A dictionary closed under a garment leaves it undressed, and looked for again.
        let orphaned: Vec<DocumentId> = self
            .clothing
            .chosen
            .iter()
            .filter(|(_, c)| c.dictionary == id)
            .map(|(garment, _)| *garment)
            .collect();
        for garment in orphaned {
            self.clothing.chosen.remove(&garment);
            self.clothing.variants.remove(&garment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes<'a>(files: &'a [&'a str]) -> impl Iterator<Item = (RpfNodeId, &'a str)> {
        files.iter().enumerate().map(|(i, f)| (i as RpfNodeId, *f))
    }

    #[test]
    fn a_garments_slot_and_index_come_from_its_stem() {
        assert_eq!(
            garment("mp_f_freemode_01_mp_f_unity_clothes^accs_026_u"),
            Some((
                "mp_f_freemode_01_mp_f_unity_clothes^accs".to_string(),
                "026".to_string()
            ))
        );
        assert_eq!(
            garment("Jbib_000_R"),
            Some(("jbib".to_string(), "000".to_string()))
        );
        assert_eq!(
            garment("p_head_000"),
            Some(("p_head".to_string(), "000".to_string())),
            "a prop has no suffix"
        );
    }

    #[test]
    fn a_name_that_is_no_garment_has_no_slot() {
        for stem in [
            "csb_sol",
            "prop_bench_01",
            "adder",
            "026_u",
            "a_b_c_001_d_e",
        ] {
            assert_eq!(garment(stem), None, "{stem}");
        }
    }

    #[test]
    fn a_garment_is_dressed_by_its_own_index_in_its_own_pack_and_nothing_else() {
        let pack = "mp_f_unity^";
        let files = [
            "mp_f_unity^accs_diff_026_b_uni.ytd",
            "mp_f_unity^accs_diff_026_a_uni.YTD",
            "mp_f_unity^accs_diff_026_aa_uni.ytd",
            "mp_f_unity^accs_diff_027_a_uni.ytd",
            "mp_f_unity^accs_diff_0260_a_uni.ytd",
            "other_pack^accs_diff_026_a_uni.ytd",
            "mp_f_unity^accs_diff_026_a_uni.ydd",
            "mp_f_unity^accs_diff_026_uni.ytd",
            "mp_f_unity^accs_diff_026_a_uni_extra.ytd",
            "mp_f_unity^accs_026_u.ytd",
        ];
        let found = variants_among(&format!("{pack}accs_026_u"), nodes(&files));
        assert_eq!(
            found.iter().map(|v| v.label()).collect::<Vec<_>>(),
            ["a \u{00B7} uni", "b \u{00B7} uni", "aa \u{00B7} uni"],
            "in the order the game numbers them"
        );
        assert_eq!(found[0].file, "mp_f_unity^accs_diff_026_a_uni.YTD");
    }

    #[test]
    fn a_prop_variant_has_no_skin() {
        let files = ["p_head_diff_000_a.ytd", "p_head_diff_000_b.ytd"];
        let found = variants_among("p_head_000", nodes(&files));
        assert_eq!(
            found.iter().map(|v| v.label()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }
}
