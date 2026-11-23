//! `.ysc` — a compiled script program, read by hand as ordinary code.
//!
//! An RSC7 resource holding one `scrProgram`: paged bytecode, a static block,
//! a table of the natives the script calls, and a paged string heap. Nothing
//! here is declared or pack-bound: the declared form waits for a staged
//! corpus.
//!
//! ⚠ **A native call reaches the engine by 64-bit hash, and a name needs data
//! this project does not ship.** [`ScrProgram::native_hash`] undoes the
//! per-slot rotation, so the number is free; [`natives::NativeDb`] maps it to
//! a name from a list the user supplies, and a hash no list names stays a hash.
//!
//! ⚠ **A `.ysc`'s RSC7 version does not say which edition shipped it.** Both
//! 12 and 10 occur in both installs and the header is the same shape at each,
//! so the reader dispatches on neither — see
//! [`YSC_FILE_VERSION_GEN8_ALT`](crate::container::resource::YSC_FILE_VERSION_GEN8_ALT).
//!
//! ⚠ **No writer, deliberately.** Nothing in OpenRage can author bytecode, so
//! a writer would exist only to round-trip bytes a reader had just read.

pub mod decompile;
pub mod disasm;
pub mod natives;
pub mod opcode;
mod program;

pub use program::{ScrProgram, CODE_PAGE_SIZE};
