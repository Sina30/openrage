//! Layout descriptions: the pack model, type plans, records, and the image they read.

pub mod analyse;
pub mod export;
pub mod image;
pub mod name;
pub mod pack;
pub mod plan;
pub mod record;
pub mod repr;
pub mod value;
pub mod view;

pub use analyse::{layout_of, Block, Confidence, Extent, Layout, NoStride, Site, Space};
pub use image::{Elements, Image, ImageMut, PagingModel, Slots, Unresolvable};
pub use name::{FieldName, TypeName};
pub use pack::PackSelector;
pub use pack::{prettify_name, Pack, PackEnum, PackField, PackType, PackVariant};
pub use plan::{Kind, PlanError, PlanField, PlanScalar, Plans, Rec, RecMut, Slot, TypePlan};
pub use record::{is_packable, FieldSite, FieldValue, Record, RecordError, RecordRef, RecordRun};
pub use repr::{
    AddressSpace, BitRange, CountSource, FieldRepr, IntWidth, Pointee, Scalar, AT_ARRAY_SIZE,
    POINTER_SIZE,
};
pub use value::{
    elements_from, empty as empty_value, read as read_value, BlobCodecs, BlobReader, BlobSource,
    BySite, ScalarValue, Value, ValueError, MAX_DEPTH,
};
pub use view::{
    BindError, Cursor, CursorMut, Disagreement, EnumField, Field, FromField, Incoherence, ToField,
    View, WriteError, WrongRepr,
};
