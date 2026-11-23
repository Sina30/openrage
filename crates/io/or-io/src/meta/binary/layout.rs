use std::collections::BTreeMap;

use crate::meta::binary::pso::schema::PsoSchemaSection;
use crate::meta::binary::pso::sections::{PmapSection, PsoSection};
use crate::meta::binary::rbf::layout::RbfLayout;
use crate::meta::binary::rbm::header::RbmHeader;
use crate::meta::binary::rbm::schema::RbmModel;
use crate::meta::binary::BinaryFormat;

#[derive(Debug, Clone)]
pub struct MetaLayout {
    pub format: BinaryFormat,
    pub container: ContainerLayout,
}

impl MetaLayout {
    pub fn format(&self) -> BinaryFormat {
        self.format
    }
}

#[derive(Debug, Clone)]
pub enum ContainerLayout {
    Pso(PsoLayout),
    Rbm(Box<RbmLayout>),
    Rbf(RbfLayout),
}

#[derive(Debug, Clone)]
pub struct PsoLayout {
    pub schema: PsoSchemaSection,
    pub pmap: PmapSection,
    pub psin: Vec<u8>,
    pub opaque_sections: Vec<OpaqueSection>,
    pub section_order: Vec<PsoSection>,
}

#[derive(Debug, Clone)]
pub struct OpaqueSection {
    pub ident: PsoSection,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RbmLayout {
    pub header: RbmHeader,
    pub model: RbmModel,
    pub decompressed: Vec<u8>,
    /// The RSC7 header this document was opened under, carried so the writer
    /// can put the payload back beneath the same one.
    ///
    /// ⚠ The sizes and version alone cannot reproduce the flag bitfield, so
    /// without it an untouched `.ymt` stops round-tripping, and only RBM's
    /// identity gate would notice. `None` for a GTA VI headerless resource,
    /// whose header is in the archive's table.
    pub seal: Option<crate::container::resource::Rsc7Seal>,
    pub virtual_page_size: usize,
    pub physical_page_size: usize,
}

impl PsoLayout {
    pub fn struct_for(
        &self,
        name_hash: u32,
    ) -> Option<&crate::meta::binary::pso::schema::PsoStructInfo> {
        self.schema.struct_for(name_hash)
    }

    /// Whether the file's `CHKS` states GTA VI.
    pub fn is_gta6(&self) -> bool {
        self.opaque_sections.iter().any(|s| {
            s.ident == PsoSection::Chks
                && crate::meta::binary::pso::sections::chks_states_gta6(&s.bytes)
        })
    }

    /// Whether this is GTA VI's table form, which has `LSCH` where the others have `PMAP`.
    pub fn is_table_form(&self) -> bool {
        self.section_order.get(1)
            == Some(&PsoSection::Other(
                crate::meta::binary::pso::sections::LSCH_IDENT,
            ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SynthesisReport {
    pub synthesised: BTreeMap<String, usize>,
}

impl SynthesisReport {
    pub fn record(&mut self, type_name: &str) {
        *self.synthesised.entry(type_name.to_string()).or_insert(0) += 1;
    }

    pub fn is_empty(&self) -> bool {
        self.synthesised.is_empty()
    }
}
