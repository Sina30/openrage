//! Each task, run as the built binary against a repository or workspace it
//! makes for itself. One test binary, so the helpers are shared and linked once.

mod common;
mod encoding;
mod fmt;
mod hooks;
mod janitor;
mod provenance;
