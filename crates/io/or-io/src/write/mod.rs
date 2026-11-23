//! A resource read, patched and rebuilt from a description.

pub mod build;
pub mod canvas;
pub mod compare;
pub mod edit;
pub mod patch;

pub use build::{
    build, build_into, build_into_mapping_with, build_into_reporting, build_into_reporting_with,
    build_into_with, BlobHeap, BlobWriter, BlobWriters, BuildError, BuildReport, Built, Carried,
    Dropped, Owner, Placement, StructAt, StructMap,
};
pub use canvas::{coverage, coverage_with, Canvas, Inherited};
pub use patch::{patch, Edit, PatchError};

pub use crate::desc::value::{
    read, read_using, read_with, BlobCodecs, BlobReader, BlobSource, ElementTypes,
};
