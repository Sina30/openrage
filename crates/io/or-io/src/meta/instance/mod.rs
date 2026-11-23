pub mod parser;
pub mod schemaless;
pub mod value;
pub mod writer;

pub use parser::parse_meta;
pub use schemaless::parse_schemaless;
pub use value::{HashedString, MetaInstance, UnknownNode, Value};
pub use writer::write_meta;

use serde::{Deserialize, Serialize};

use crate::meta::binary::{
    detect_binary_format, parse_meta_binary_full, write_meta_binary, BinaryFormat, MetaLayout,
};
use crate::meta::diagnostics::Diagnostics;
use crate::meta::error::MetaError;
use crate::meta::schema::SchemaLibrary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceFormat {
    Xml,
    Pso,
    Rbm,
    Rbf,
    /// A binary streaming request list, which [`crate::meta::srl`] reads.
    Srl,
}

impl SourceFormat {
    pub fn label(&self) -> &'static str {
        match self {
            SourceFormat::Xml => "XML",
            SourceFormat::Pso => BinaryFormat::Pso.label(),
            SourceFormat::Rbm => BinaryFormat::Rbm.label(),
            SourceFormat::Rbf => BinaryFormat::Rbf.label(),
            SourceFormat::Srl => "SRL",
        }
    }

    pub fn is_binary(&self) -> bool {
        !matches!(self, SourceFormat::Xml)
    }

    pub fn from_binary(b: BinaryFormat) -> Self {
        match b {
            BinaryFormat::Pso => SourceFormat::Pso,
            BinaryFormat::Rbm => SourceFormat::Rbm,
            BinaryFormat::Rbf => SourceFormat::Rbf,
        }
    }

    pub fn to_binary(self) -> Option<BinaryFormat> {
        match self {
            SourceFormat::Xml | SourceFormat::Srl => None,
            SourceFormat::Pso => Some(BinaryFormat::Pso),
            SourceFormat::Rbm => Some(BinaryFormat::Rbm),
            SourceFormat::Rbf => Some(BinaryFormat::Rbf),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetaDocument {
    pub instance: MetaInstance,
    pub raw_bytes: Vec<u8>,
    pub layout: Option<MetaLayout>,
    pub revision: u64,
    pub source_format: SourceFormat,
}

impl MetaDocument {
    pub fn parse(library: &SchemaLibrary, bytes: Vec<u8>) -> crate::meta::Result<Self> {
        let (instance, layout, source_format) = match detect_binary_format(&bytes) {
            Some(format) => {
                let mut diags = Diagnostics::lenient();
                let (instance, layout) = parse_meta_binary_full(library, &bytes, &mut diags)?;
                (instance, Some(layout), SourceFormat::from_binary(format))
            }
            None => match crate::meta::srl::read(&bytes) {
                Ok(instance) => (instance, None, SourceFormat::Srl),
                Err(_) => (parse_xml(library, &bytes)?, None, SourceFormat::Xml),
            },
        };
        Ok(Self {
            instance,
            raw_bytes: bytes,
            layout,
            revision: 0,
            source_format,
        })
    }

    pub fn to_bytes(&self) -> crate::meta::Result<Vec<u8>> {
        match self.source_format {
            SourceFormat::Xml => Ok(write_meta(&self.instance)?.into_bytes()),
            SourceFormat::Srl => crate::meta::srl::write(&self.instance),
            other => {
                let format = other
                    .to_binary()
                    .expect("non-XML SourceFormat must map to BinaryFormat");
                write_meta_binary(format, self.layout.as_ref(), &self.instance)
            }
        }
    }
}

/// XML read through `library`, or as the tree it states when `library` lacks one of its types.
fn parse_xml(library: &SchemaLibrary, bytes: &[u8]) -> crate::meta::Result<MetaInstance> {
    match parse_meta(library, bytes) {
        Err(MetaError::UnknownType(name)) => parse_schemaless(bytes).map_err(|why| {
            MetaError::Other(format!(
                "schema type `{name}` not found in library, and read without one: {why}"
            ))
        }),
        read => read,
    }
}
