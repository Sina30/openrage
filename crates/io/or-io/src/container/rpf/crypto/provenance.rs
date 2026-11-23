//! Where each class of key material came from, and what the bytes say when
//! none of it could be recovered.

use serde::{Deserialize, Serialize};

/// Where one class of key material came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KeyOrigin {
    #[default]
    Absent,
    /// Recovered from the user's own game executable by fingerprint search.
    Executable,
    /// Decoded from the key snapshot OpenRage bundles (D18), as builds before #202 also did.
    EmbeddedSnapshot,
    /// Decoded from a key snapshot the user supplied.
    SnapshotFile,
}

impl KeyOrigin {
    pub const fn label(self) -> &'static str {
        match self {
            KeyOrigin::Absent => "not recovered",
            KeyOrigin::Executable => "your game executable",
            KeyOrigin::EmbeddedSnapshot => "the key snapshot bundled with OpenRage",
            KeyOrigin::SnapshotFile => "your own key snapshot",
        }
    }

    /// `true` when the material came from a snapshot rather than from the
    /// executable in front of us, so nothing about the running install
    /// vouches for it.
    pub const fn is_snapshot(self) -> bool {
        matches!(self, KeyOrigin::EmbeddedSnapshot | KeyOrigin::SnapshotFile)
    }
}

/// The result of comparing a snapshot against the executable on the one field
/// both of them supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SnapshotAgreement {
    /// No snapshot was consulted, or only one source supplied the LUT, so
    /// there was nothing to compare.
    #[default]
    Unchecked,
    Agrees,
    /// The snapshot describes a build that is not the one in front of us.
    Disagrees,
}

/// Where each class of a `Gta5Keys` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KeyProvenance {
    pub aes_key: KeyOrigin,
    pub lut: KeyOrigin,
    pub ng_keys: KeyOrigin,
    pub ng_decrypt_tables: KeyOrigin,
    pub awc_key: KeyOrigin,
    pub snapshot_agreement: SnapshotAgreement,
}

impl KeyProvenance {
    pub fn all_from_executable() -> Self {
        Self {
            aes_key: KeyOrigin::Executable,
            lut: KeyOrigin::Executable,
            ng_keys: KeyOrigin::Executable,
            ng_decrypt_tables: KeyOrigin::Executable,
            awc_key: KeyOrigin::Absent,
            snapshot_agreement: SnapshotAgreement::Unchecked,
        }
    }

    /// One line per key class, for the CLI and the settings page.
    pub fn report(&self) -> Vec<String> {
        let mut lines = vec![
            format!("AES key: {}", self.aes_key.label()),
            format!("hash LUT: {}", self.lut.label()),
            format!("NG keys: {}", self.ng_keys.label()),
            format!("NG decrypt tables: {}", self.ng_decrypt_tables.label()),
            format!("AWC key: {}", self.awc_key.label()),
        ];
        match self.snapshot_agreement {
            SnapshotAgreement::Unchecked => {}
            SnapshotAgreement::Agrees => lines.push(
                "the snapshot's hash LUT matches the one in your executable, so it \
                 describes this build"
                    .into(),
            ),
            SnapshotAgreement::Disagrees => lines.push(
                "the snapshot's hash LUT does NOT match the one in your executable, so \
                 it describes a different build; its NG keys were refused"
                    .into(),
            ),
        }
        lines
    }
}

/// What the bytes say about an executable that yielded no AES key.
///
/// ⚠ Each variant is a claim about evidence the scan collected, never a guess
/// from the file name — `GTA5_Enhanced.exe` and `PlayGTAV.exe` sit in the same
/// folder and only one of them has ever held a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExeVerdict {
    NotAnExecutable,
    NotWindowsX64 {
        machine: u16,
    },
    /// A game executable whose AES key is no longer where the fingerprint
    /// expects it.
    KeyMaterialRotated,
    /// A Rockstar-built binary that is not the game: a launcher, the language
    /// selector, the uninstaller.
    RockstarButNotTheGame,
    NotARockstarBinary,
}

/// `IMAGE_FILE_MACHINE_AMD64`.
const MACHINE_AMD64: u16 = 0x8664;

/// The first archive the game mounts, and the marker that separates the game
/// from everything shipped beside it.
///
/// ⚠ The hash LUT cannot do this job. Measured over both retail installs, all
/// fourteen executables: the LUT is in every Rockstar-built binary including
/// `PlayGTAV.exe`, `GTAVLauncher.exe` and `uninstall.exe`, so a diagnosis
/// resting on it calls every launcher a patched game. This string is in all
/// six game executables and none of the eight others.
const GAME_MARKER: &[u8] = b"common.rpf";

impl ExeVerdict {
    /// Decide from the bytes, given whether the hash LUT fingerprint matched.
    pub fn of(image: &[u8], lut_found: bool) -> Self {
        match pe_machine(image) {
            None => ExeVerdict::NotAnExecutable,
            Some(MACHINE_AMD64) => {
                if contains(image, GAME_MARKER) {
                    ExeVerdict::KeyMaterialRotated
                } else if lut_found {
                    ExeVerdict::RockstarButNotTheGame
                } else {
                    ExeVerdict::NotARockstarBinary
                }
            }
            Some(machine) => ExeVerdict::NotWindowsX64 { machine },
        }
    }

    pub fn cause(self) -> String {
        match self {
            ExeVerdict::NotAnExecutable => "this file is not a Windows executable".into(),
            ExeVerdict::NotWindowsX64 { machine } => format!(
                "this is a Windows executable for machine 0x{machine:04X}, not the x64 \
                 build OpenRage reads keys from"
            ),
            ExeVerdict::KeyMaterialRotated => {
                "this is a GTA V game executable, but its AES key is no longer where the \
                 fingerprint expects it — a patch has rotated or moved it"
                    .into()
            }
            ExeVerdict::RockstarButNotTheGame => {
                "this is a Rockstar-built binary, but not the game: it never mounts an \
                 archive, so it has never carried an archive key"
                    .into()
            }
            ExeVerdict::NotARockstarBinary => {
                "this is a Windows x64 executable with no Rockstar key material in it at \
                 all"
                .into()
            }
        }
    }

    pub fn remedy(self) -> &'static str {
        match self {
            ExeVerdict::NotAnExecutable
            | ExeVerdict::NotWindowsX64 { .. }
            | ExeVerdict::NotARockstarBinary => {
                "Point this at GTA5.exe (Legacy) or GTA5_Enhanced.exe (Enhanced) in the \
                 install folder."
            }
            ExeVerdict::KeyMaterialRotated => {
                "OpenRage cannot recover a key it has never seen: the fingerprint search \
                 tolerates the key moving, not the key changing. Wait for a build whose \
                 fingerprints cover this patch."
            }
            ExeVerdict::RockstarButNotTheGame => {
                "PlayGTAV.exe, GTAVLauncher.exe, GTAVLanguageSelect.exe and uninstall.exe \
                 all look like this. Point this at GTA5.exe (Legacy) or \
                 GTA5_Enhanced.exe (Enhanced) instead."
            }
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The `IMAGE_FILE_HEADER` machine field, or `None` when this is not a PE.
fn pe_machine(image: &[u8]) -> Option<u16> {
    if image.len() < 0x40 || &image[..2] != b"MZ" {
        return None;
    }
    let e_lfanew = u32::from_le_bytes(image[0x3C..0x40].try_into().ok()?) as usize;
    let machine_at = e_lfanew.checked_add(6)?;
    if machine_at > image.len() || &image.get(e_lfanew..e_lfanew + 4)? != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(
        image[e_lfanew + 4..e_lfanew + 6].try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE far smaller than anything a linker emits: enough header
    /// for the machine field and nothing else.
    fn pe_image(machine: u16) -> Vec<u8> {
        let mut image = vec![0u8; 0x100];
        image[..2].copy_from_slice(b"MZ");
        image[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        image[0x80..0x84].copy_from_slice(b"PE\0\0");
        image[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        image
    }

    fn game_image() -> Vec<u8> {
        let mut image = pe_image(MACHINE_AMD64);
        image.extend_from_slice(GAME_MARKER);
        image
    }

    #[test]
    fn a_verdict_names_a_different_cause_for_each_kind_of_wrong_file() {
        let x64 = pe_image(MACHINE_AMD64);
        let causes = [
            ExeVerdict::of(b"not an exe at all, just some bytes here", false),
            ExeVerdict::of(&pe_image(0x01C4), false),
            ExeVerdict::of(&game_image(), true),
            ExeVerdict::of(&x64, true),
            ExeVerdict::of(&x64, false),
        ];
        assert_eq!(causes[0], ExeVerdict::NotAnExecutable);
        assert_eq!(causes[1], ExeVerdict::NotWindowsX64 { machine: 0x01C4 });
        assert_eq!(causes[2], ExeVerdict::KeyMaterialRotated);
        assert_eq!(causes[3], ExeVerdict::RockstarButNotTheGame);
        assert_eq!(causes[4], ExeVerdict::NotARockstarBinary);

        let rendered: Vec<String> = causes.iter().map(|v| v.cause()).collect();
        for (i, a) in rendered.iter().enumerate() {
            for b in rendered.iter().skip(i + 1) {
                assert_ne!(a, b, "two verdicts render the same sentence");
            }
        }
    }

    /// ⚠ The LUT is in every Rockstar binary, so a game executable is
    /// recognised by the archive it mounts and not by the table it shares
    /// with its own launcher.
    #[test]
    fn a_launcher_is_not_called_a_patched_game_just_because_it_shares_the_lut() {
        assert_eq!(
            ExeVerdict::of(&pe_image(MACHINE_AMD64), true),
            ExeVerdict::RockstarButNotTheGame
        );
        // The game marker outranks the LUT in both directions: a game
        // executable stripped of the LUT is still a game executable.
        assert_eq!(
            ExeVerdict::of(&game_image(), false),
            ExeVerdict::KeyMaterialRotated
        );
    }

    #[test]
    fn a_truncated_or_lying_pe_header_is_not_an_executable() {
        // `MZ` with e_lfanew pointing past the end.
        let mut lying = vec![0u8; 0x40];
        lying[..2].copy_from_slice(b"MZ");
        lying[0x3C..0x40].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
        assert_eq!(ExeVerdict::of(&lying, false), ExeVerdict::NotAnExecutable);

        // `MZ` with e_lfanew pointing at something that is not `PE\0\0`.
        let mut no_pe = pe_image(MACHINE_AMD64);
        no_pe[0x80..0x84].copy_from_slice(b"NE\0\0");
        assert_eq!(ExeVerdict::of(&no_pe, false), ExeVerdict::NotAnExecutable);

        assert_eq!(ExeVerdict::of(b"MZ", false), ExeVerdict::NotAnExecutable);
        assert_eq!(ExeVerdict::of(&[], false), ExeVerdict::NotAnExecutable);
    }

    #[test]
    fn a_report_names_every_class_and_flags_a_disagreeing_snapshot() {
        let mixed = KeyProvenance {
            aes_key: KeyOrigin::Executable,
            lut: KeyOrigin::Executable,
            ng_keys: KeyOrigin::EmbeddedSnapshot,
            ng_decrypt_tables: KeyOrigin::EmbeddedSnapshot,
            awc_key: KeyOrigin::EmbeddedSnapshot,
            snapshot_agreement: SnapshotAgreement::Agrees,
        };
        let report = mixed.report();
        assert_eq!(report.len(), 6);
        assert!(report[0].contains("your game executable"));
        assert!(report[2].contains("bundled with OpenRage"));
        assert!(report[4].contains("bundled with OpenRage"));
        assert!(report[5].contains("matches"));

        let refused = KeyProvenance {
            ng_keys: KeyOrigin::Absent,
            ng_decrypt_tables: KeyOrigin::Absent,
            snapshot_agreement: SnapshotAgreement::Disagrees,
            ..mixed
        };
        let report = refused.report();
        assert!(report[2].contains("not recovered"));
        assert!(report[5].contains("refused"));
    }
}
