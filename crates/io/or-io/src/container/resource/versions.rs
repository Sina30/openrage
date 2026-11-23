//! The RSC7 version each format carries, as observed in retail files.
//!
//! ⚠ **A measured table, not a derivation.** Every value is a version the
//! installs themselves carry, stated as a literal. Measured by
//! `openrage-cli versions` — which reads the 16-byte header of every resource
//! file of every registered type through the VFS — over a retail Legacy install
//! (316,589 resource files) and a retail Enhanced install (317,951), both
//! with 0 unreadable and 0 non-RSC7 files. The file counts below are that
//! run's.
//!
//! | Format | Legacy | Enhanced |
//! |---|---|---|
//! | `.ybn` | 43 × 15,130 | 43 × 14,695 |
//! | `.ybd` | 43 × 1, **42 × 2, 40 × 1** | 43 × 1 |
//! | `.ycd` | 46 × 25,174, **43 × 4** | 46 × 26,320 |
//! | `.ydd` | 165 × 23,373 | 159 × 23,506 |
//! | `.ydr` | 165 × 86,576, 164 × 13 | 159 × 84,616, 154 × 12, 165 × 3 |
//! | `.yed` | 25 × 242 | 25 × 241 |
//! | `.yfd` | 4 × 4 | 4 × 3 |
//! | `.yft` | 162 × 60,776, 160 × 66 | 171 × 61,516 |
//! | `.yld` | 8 × 61 | 8 × 61 |
//! | `.ynd` | 1 × 1,521 | 1 × 1,027 |
//! | `.ynv` | 2 × 10,583 | 2 × 10,583 |
//! | `.ypt` | 68 × 1,246 | 71 × 1,577 |
//! | `.ysc` | 10 × 8 | 10 × 710 |
//! | `.ytd` | 13 × 91,808 | 5 × 93,076, 13 × 3, **2 × 1** |
//! | `.ypm` | none | none |
//!
//! A version in bold is observed and deliberately not registered, so the seal
//! refuses it before a reader can misread it.

pub const YDR_FILE_VERSION: u32 = 165;

pub const YDD_FILE_VERSION: u32 = 165;

pub const YTD_FILE_VERSION: u32 = 13;

pub const YFT_FILE_VERSION: u32 = 162;

pub const YPT_FILE_VERSION: u32 = 68;

pub const YCD_FILE_VERSION: u32 = 46;

pub const YBN_FILE_VERSION: u32 = 43;

/// ⚠ **A `.ybd` is a dictionary of exactly the bounds a `.ybn` holds, so the
/// two share a version and only 43 is described.** A retail Legacy install
/// ships four `.ybd` at three versions — one at 43, two at 42 and one at 40 —
/// and nothing states what a bound looked like at 42 or 40. The registry lists
/// 43 alone, so the older three are refused by the seal before a reader sees
/// them: a dictionary reads at any version and it is the *bounds inside it*
/// that would be misread.
pub const YBD_FILE_VERSION: u32 = 43;

pub const YLD_FILE_VERSION: u32 = 8;

pub const YED_FILE_VERSION: u32 = 25;

pub const YFD_FILE_VERSION: u32 = 4;

/// ⚠ **Unmeasured: the one row no file vouches for.** Neither retail install
/// ships a `.ypm`, nothing here reads or writes one, and no public reference
/// states its version. The value is the one earlier builds registered.
pub const YPM_FILE_VERSION: u32 = 63;

/// ⚠ **Measured by a different census.** The header census above sees `.ysc`
/// only at 10; `ysc_census::what_a_script_container_holds` counts 1,154 at 12
/// in each install, and 12 is what the script disassembler is written against.
pub const YSC_FILE_VERSION: u32 = 12;

/// ⚠ **A `.ysc`'s version does not say which edition shipped it.** Both
/// numbers occur in both installs, and the script header is the same shape at
/// each — so this is a Gen8 alternate like `.yft`'s 160, not a Gen9 variant.
/// Measured by `ysc_census::what_a_script_container_holds` over two retail
/// installs: Legacy is 1,154 at version 12 and 4 at version 10, Enhanced is
/// 1,154 and 710.
pub const YSC_FILE_VERSION_GEN8_ALT: u32 = 10;

pub const YNV_FILE_VERSION: u32 = 2;

pub const YND_FILE_VERSION: u32 = 1;

pub const YVR_FILE_VERSION: u32 = 1;

pub const YWR_FILE_VERSION: u32 = 1;

pub const YFT_FILE_VERSION_GEN8_ALT: u32 = 160;

pub const YDR_FILE_VERSION_GEN8_ALT: u32 = 164;

/// An older clip-dictionary revision: 4 files in a retail Legacy install carry
/// 43 rather than 46 — `des_bridge.ycd`, `des_fib_ceiling2.ycd`,
/// `des_hosp_ceil.ycd`, `des_setpiece.ycd`, the clip dictionaries belonging to
/// the same destruction assets as [`YDR_FILE_VERSION_GEN8_ALT`].
///
/// ⚠ **Deliberately not registered for read.** Their payload decompresses —
/// they carry the same 8-byte prefix as those drawables, and skipping it yields exactly the
/// page-flag size — but the *structures inside* are not the v46 layout. Read as
/// v46 they produce garbage that a lenient parse hides: `des_bridge.ycd` yields
/// a property declaring 55,817 attributes of which 0 parse, and a block stream
/// whose header describes a payload that is not there.
///
/// Registering it would convert a clean rejection into plausible nonsense.
/// Reading these four files needs the v43 layout worked out.
pub const YCD_FILE_VERSION_GEN8_ALT: u32 = 43;

pub const YTD_FILE_VERSION_GEN9: u32 = 5;

pub const YDR_FILE_VERSION_GEN9: u32 = 159;

pub const YDR_FILE_VERSION_GEN9_ALT: u32 = 154;

pub const YDD_FILE_VERSION_GEN9: u32 = 159;

pub const YFT_FILE_VERSION_GEN9: u32 = 171;

pub const YPT_FILE_VERSION_GEN9: u32 = 71;
