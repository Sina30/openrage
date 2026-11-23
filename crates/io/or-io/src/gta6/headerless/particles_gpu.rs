//! The GPU buffers a sprite or trail particle rule points at: each buffer object's words and the
//! extent of its data in the physical section, whose contents no reader lays out.

use super::*;

const WHAT: &str = "particle GPU buffer";
/// A buffer object's size.
const OBJECT: usize = 0x50;
/// Where a rule points at a GPU buffer, and the two u16 words and bind flags every buffer there holds.
const SLOTS: [(usize, (u16, u16, u32)); 3] = [
    (0x1B8, (8, 8, 0x980)),
    (0x388, (8, 8, 0x980)),
    (0x390, (16, 4, 0x1600)),
];
/// The slot that may be null on any rule; the other two are null exactly on a model rule.
const OPTIONAL: usize = 0x1B8;
/// The slot of the two tables, and their bytes: 64 rows of four half floats.
const TABLES: (usize, usize) = (0x388, 0x200);
/// The slot whose data is a copy of the rule's floats at +0x1E0 to +0x1FB.
const FLOATS: (usize, usize) = (0x390, 0x1C);
/// The rule's place of the floats `FLOATS` copies.
const RULE_FLOATS: usize = 0x1E0;
/// The row length of the optional slot's data.
const ROW: usize = 128;

/// A rule's +0x388 data: two tables of 16 rows of four half floats, each as stored.
pub type GpuTables = [[[u16; 4]; 16]; 2];

/// One GPU buffer of a particle rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuBuffer {
    /// The rule's offset that points at it: 0x1B8, 0x388 or 0x390.
    pub slot: u16,
    /// Its object's place in the virtual section.
    pub at: u64,
    /// The u16 words at +0x0C and +0x0E of its object, and its bind flags at +0x10.
    pub words: (u16, u16, u32),
    /// Where its data starts in the physical section, and the data's length in bytes.
    pub data_at: u64,
    pub len: u64,
}

/// The GPU buffers `rule`, a rule of the list read from these sections, points at, in slot order.
pub fn read_particle_gpu_buffers(
    virtual_section: &[u8],
    physical: Option<&[u8]>,
    rule: &ParticleRule,
) -> Result<Vec<GpuBuffer>> {
    let (at, model) = (rule.at as usize, rule.draw_type == 1);
    sections(virtual_section, physical, |v, p| buffers(&v, &p, at, model))
}

/// The buffers the rule at `at` points at; a model rule points at none but the optional one.
fn buffers(v: &Bytes, p: &Bytes, at: usize, model: bool) -> Result<Vec<GpuBuffer>> {
    let mut out = Vec::new();
    for (slot, expected) in SLOTS {
        let Some(b) = v.ptr(WHAT, at + slot)? else {
            need(
                slot == OPTIONAL || model,
                "a sprite or trail rule does not point at its GPU buffers",
            )?;
            continue;
        };
        need(
            slot == OPTIONAL || !model,
            "a model rule points at a GPU buffer",
        )?;
        v.get(WHAT, b, OBJECT)?;
        let words = (
            v.u16(WHAT, b + 0x0C)?,
            v.u16(WHAT, b + 0x0E)?,
            v.u32(WHAT, b + 0x10)?,
        );
        need(
            words == expected,
            "a GPU buffer's words at +0x0C, +0x0E and +0x10 are not those measured at its slot",
        )?;
        // The length is stated in 4-byte words, whatever the word at +0x0C.
        let Some(len) = (v.u32(WHAT, b + 0x08)? as usize).checked_mul(4) else {
            return refuse("a GPU buffer's length does not fit");
        };
        let data = v.phys(WHAT, b + 0x18)?;
        p.get(WHAT, data, len)?;
        data_held(v, p, at, slot, data, len)?;
        out.push(GpuBuffer {
            slot: slot as u16,
            at: b as u64,
            words,
            data_at: data as u64,
            len: len as u64,
        });
    }
    Ok(out)
}

/// Refused where the data at `data` of the rule at `at`'s `slot` breaks what that slot holds.
fn data_held(v: &Bytes, p: &Bytes, at: usize, slot: usize, data: usize, len: usize) -> Result<()> {
    match slot {
        _ if slot == FLOATS.0 => need(
            len >= FLOATS.1
                && p.get(WHAT, data, FLOATS.1)? == v.get(WHAT, at + RULE_FLOATS, FLOATS.1)?,
            "a rule's +0x390 GPU buffer does not hold its floats at +0x1E0 to +0x1FB",
        ),
        _ if slot == TABLES.0 => {
            need(
                len == TABLES.1,
                "a rule's +0x388 GPU buffer is not the 512 bytes of its two tables",
            )?;
            let t = p.get(WHAT, data, TABLES.1)?;
            need(
                t[0x80..0x100]
                    .iter()
                    .chain(&t[0x180..0x200])
                    .all(|b| *b == 0),
                "a rule's +0x388 GPU buffer sets rows 16 to 31 or 48 to 63",
            )
        }
        _ => need(
            len.is_multiple_of(ROW),
            "a rule's +0x1B8 GPU buffer is not a whole number of 128-byte rows",
        ),
    }
}

/// The two tables a rule's +0x388 `buffer` holds, rows 0 to 15 and 32 to 47 of its data, raw.
pub fn read_particle_gpu_tables(physical: &[u8], buffer: &GpuBuffer) -> Result<GpuTables> {
    need(
        usize::from(buffer.slot) == TABLES.0,
        "only a rule's +0x388 GPU buffer holds the tables",
    )?;
    need(
        buffer.len == TABLES.1 as u64,
        "a rule's +0x388 GPU buffer is not the 512 bytes of its two tables",
    )?;
    let at = usize::try_from(buffer.data_at).unwrap_or(usize::MAX);
    let end = at.saturating_add(TABLES.1);
    let Some(data) = physical.get(at..end) else {
        return refuse("a GPU buffer's tables are past the physical section's end");
    };
    Ok(std::array::from_fn(|t| {
        std::array::from_fn(|row| {
            let r = 0x100 * t + 8 * row;
            std::array::from_fn(|lane| {
                u16::from_le_bytes([data[r + 2 * lane], data[r + 2 * lane + 1]])
            })
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE_LEN: usize = 0x400;

    /// Where the data of the i-th buffer starts in the physical section.
    const DATA_STEP: usize = 0x200;
    /// The words of one 128-byte row.
    const ROW_WORDS: u32 = 32;

    /// A virtual section holding a rule at 0 and, for each of `slots`, a buffer object of its
    /// measured words over `words` 4-byte words of the physical section, `DATA_STEP` apart; the
    /// +0x388 object over its tables' 128.
    fn image(slots: &[usize], words: u32) -> Vec<u8> {
        let mut v = vec![0u8; RULE_LEN + OBJECT * SLOTS.len()];
        for (i, &slot) in slots.iter().enumerate() {
            let words = match slot == TABLES.0 {
                true => (TABLES.1 / 4) as u32,
                false => words,
            };
            let (_, (a, b, flags)) = SLOTS.iter().find(|(s, _)| *s == slot).copied().unwrap();
            let object = RULE_LEN + OBJECT * i;
            v[slot..slot + 8].copy_from_slice(&(0x5000_0000u64 + object as u64).to_le_bytes());
            v[object + 8..object + 0xC].copy_from_slice(&words.to_le_bytes());
            v[object + 0xC..object + 0xE].copy_from_slice(&a.to_le_bytes());
            v[object + 0xE..object + 0x10].copy_from_slice(&b.to_le_bytes());
            v[object + 0x10..object + 0x14].copy_from_slice(&flags.to_le_bytes());
            let data = 0x6000_0000u64 + (DATA_STEP * i) as u64;
            v[object + 0x18..object + 0x20].copy_from_slice(&data.to_le_bytes());
        }
        v
    }

    const PHYSICAL: [u8; 4 * DATA_STEP] = [0; 4 * DATA_STEP];

    fn read_over(v: &[u8], p: &[u8], model: bool) -> Result<Vec<GpuBuffer>> {
        sections(v, Some(p), |v, p| buffers(&v, &p, 0, model))
    }

    fn read(v: &[u8], model: bool) -> Result<Vec<GpuBuffer>> {
        read_over(v, &PHYSICAL[..], model)
    }

    #[test]
    fn a_sprite_rule_s_buffers_are_read_with_their_data_in_the_physical_section() {
        let v = image(&[0x388, 0x390, 0x1B8], ROW_WORDS);
        let found = read(&v, false).expect("the buffers read");
        let slots: Vec<u16> = found.iter().map(|b| b.slot).collect();
        assert_eq!(slots, [0x1B8, 0x388, 0x390], "in slot order");
        let by_slot = |slot: u16| found.iter().find(|b| b.slot == slot).copied().unwrap();
        assert_eq!(by_slot(0x388).at, RULE_LEN as u64);
        assert_eq!(by_slot(0x390).words, (16, 4, 0x1600));
        assert_eq!(by_slot(0x390).data_at, DATA_STEP as u64);
        assert_eq!(by_slot(0x1B8).data_at, 2 * DATA_STEP as u64);
        assert_eq!(by_slot(0x388).len, TABLES.1 as u64);
        assert!((found.iter())
            .filter(|b| b.slot != 0x388)
            .all(|b| b.len == 4 * u64::from(ROW_WORDS)));
        // The optional slot may be null.
        let v = image(&[0x388, 0x390], ROW_WORDS);
        assert_eq!(read(&v, false).map(|b| b.len()).ok(), Some(2));
    }

    #[test]
    fn a_buffer_that_breaks_a_measured_rule_is_refused() {
        // A sprite rule missing one of its two buffers, and a model rule that points at them.
        assert!(read(&image(&[0x388], ROW_WORDS), false).is_err());
        assert!(read(&image(&[0x388, 0x390], ROW_WORDS), true).is_err());
        assert_eq!(read(&image(&[], ROW_WORDS), true).ok(), Some(Vec::new()));
        // Data past the physical section, and a physical section not read.
        assert!(read(&image(&[0x388, 0x390], 0x400), false).is_err());
        let mut v = image(&[0x388, 0x390], ROW_WORDS);
        let unread = sections(&v, None, |v, p| buffers(&v, &p, 0, false));
        assert!(unread.is_err());
        // Bind flags other than the slot's.
        v[RULE_LEN + 0x10] ^= 1;
        assert!(read(&v, false).is_err());
    }

    #[test]
    fn a_buffers_data_is_refused_where_it_breaks_what_its_slot_holds() {
        let v = image(&[0x388, 0x390, 0x1B8], ROW_WORDS);
        assert!(read(&v, false).is_ok());
        // A rule float the +0x390 copy does not hold.
        let mut floats = v.clone();
        floats[RULE_FLOATS + FLOATS.1 - 1] = 1;
        assert!(read(&floats, false).is_err());
        let mut copied = PHYSICAL.to_vec();
        copied[DATA_STEP + FLOATS.1 - 1] = 1;
        assert!(
            read_over(&floats, &copied, false).is_ok(),
            "the copy follows"
        );
        // A +0x388 row past the two tables set.
        for row in [16, 31, 48, 63] {
            let mut p = PHYSICAL.to_vec();
            p[8 * row] = 1;
            assert!(read_over(&v, &p, false).is_err(), "row {row}");
        }
        // A +0x1B8 buffer of a part row.
        let short = image(&[0x388, 0x390, 0x1B8], ROW_WORDS - 1);
        assert!(read(&short, false).is_err());
        // A +0x388 buffer shorter than its tables, and a +0x390 one shorter than its floats.
        for (object, words) in [(RULE_LEN, 127u32), (RULE_LEN + OBJECT, 6)] {
            let mut cut = v.clone();
            cut[object + 8..object + 0xC].copy_from_slice(&words.to_le_bytes());
            assert!(read(&cut, false).is_err(), "{words} words");
        }
    }

    #[test]
    fn a_rules_tables_are_rows_0_to_15_and_32_to_47_of_its_0x388_data() {
        let v = image(&[0x388, 0x390], ROW_WORDS);
        let mut p = PHYSICAL.to_vec();
        p[0..2].copy_from_slice(&0x3C00u16.to_le_bytes());
        p[8 * 15 + 6..8 * 15 + 8].copy_from_slice(&0x4000u16.to_le_bytes());
        p[8 * 32 + 2..8 * 32 + 4].copy_from_slice(&0x4200u16.to_le_bytes());
        let found = read_over(&v, &p, false).expect("the buffers read");
        let tables = found.iter().find(|b| b.slot == 0x388).expect("its tables");
        let held = read_particle_gpu_tables(&p, tables).expect("they read");
        assert_eq!(held[0][0][0], 0x3C00);
        assert_eq!(held[0][15][3], 0x4000);
        assert_eq!(held[1][0][1], 0x4200);
        let floats = found.iter().find(|b| b.slot == 0x390).expect("its floats");
        assert!(read_particle_gpu_tables(&p, floats).is_err());
        assert!(read_particle_gpu_tables(&p[..0x100], tables).is_err());
    }
}
