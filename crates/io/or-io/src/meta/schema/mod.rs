pub mod library;
pub mod normalize;
pub mod parser;
pub mod types;

pub use library::{ResolvedStruct, SchemaLibrary};
pub use normalize::{
    consistent_struct_short_name, convert_datatype_to_friendly_name, normalize_member_name,
};
pub use parser::{parse_psc, SchemaItem};
pub use types::*;

pub fn build_library<I, S, B>(files: I) -> Result<SchemaLibrary, String>
where
    I: IntoIterator<Item = (S, B)>,
    S: AsRef<str>,
    B: AsRef<[u8]>,
{
    let mut lib = SchemaLibrary::new();
    for (path, bytes) in files {
        let items = parse_psc(bytes.as_ref(), path.as_ref())?;
        for item in items {
            match item {
                SchemaItem::Struct(s) => lib.insert_struct(s),
                SchemaItem::Enum(e) => lib.insert_enum(e),
                SchemaItem::Skipped => {}
            }
        }
    }
    Ok(lib)
}
