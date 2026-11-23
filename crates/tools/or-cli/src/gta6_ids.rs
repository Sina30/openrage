//! What `rpf scan-open` prints of the table the word at 0x50 of a headerless RBM header points
//! at: its entries as UUIDs where every one has a UUID's version and variant, else as bytes.

use or_io::meta::binary::rbm::gta6_ids::{id_table, uuid_text};

/// The most entries printed of one table.
const SHOWN: usize = 32;

/// The table of `virtual_section`'s word at 0x50, or why it is not read; nothing where it is zero.
pub(crate) fn print(virtual_section: &[u8]) {
    let table = match id_table(virtual_section) {
        Ok(Some(table)) => table,
        Ok(None) => return,
        Err(e) => {
            println!("  ids           not read: {e}");
            return;
        }
    };
    let n = table.entries.len();
    let uuids = table.reads_as_uuids();
    println!(
        "  ids           {n} entries at {:#X}, carried as read; {}; what object each names is not known",
        table.at,
        match uuids {
            true => "every one has a version 1 or 4 UUID's version and variant, so they read as UUIDs",
            false => "not every one has a UUID's version and variant, and the bytes of those that do not have no known structure",
        }
    );
    for (i, e) in table.entries.iter().take(SHOWN).enumerate() {
        let text = match uuids {
            true => uuid_text(e),
            false => e.iter().map(|b| format!("{b:02X}")).collect(),
        };
        println!("    {i:>4}  {text}");
    }
    if n > SHOWN {
        println!("    ... and {} more", n - SHOWN);
    }
}
