//! A `.ysc` opened read-only as its decompiled source and its listing.

use std::path::PathBuf;
use std::sync::Arc;

use super::DocPayload;

/// Where the app and `openrage-cli script download-natives` keep the downloaded natives database.
pub fn natives_dir() -> Option<PathBuf> {
    crate::state::prefs::config_root().map(|root| or_cli::natives_dir(&root))
}

/// The downloaded natives database, empty when none was downloaded or it does not read.
///
/// ponytail: read again for every script opened, a few megabytes of JSON;
/// cache it if opening scripts in bulk ever shows the cost.
fn downloaded_natives() -> or_io::gta5::script::natives::NativeDbSet {
    let Some(dir) = natives_dir() else {
        return Default::default();
    };
    or_cli::downloaded_natives(&dir).unwrap_or_else(|e| {
        tracing::warn!("{e}");
        Default::default()
    })
}

/// A `.ysc` as the source `openrage-cli script decompile` prints and the listing `script disasm` prints.
///
/// ⚠ Read-only: OpenRage cannot author bytecode, so the program is not kept.
#[derive(Clone, Default)]
pub struct ScriptDocument {
    /// The listing, one entry per line.
    pub listing: Vec<String>,
    pub source: ropey::Rope,
    /// Which natives list named this script's natives, or why none did.
    pub sentence: String,
    /// Whether a natives list named any of them.
    pub named: bool,
}

impl ScriptDocument {
    /// The source and listing of `program`, its natives named from the downloaded natives database.
    pub fn of(program: &or_io::gta5::script::ScrProgram) -> Result<Self, String> {
        Self::of_named(program, &downloaded_natives())
    }

    /// The source and listing of `program`, its natives named from `names` where a list names them.
    pub fn of_named(
        program: &or_io::gta5::script::ScrProgram,
        names: &or_io::gta5::script::natives::NativeDbSet,
    ) -> Result<Self, String> {
        let (db, sentence) = if names.is_empty() {
            (
                None,
                format!(
                    "{} Download the natives database from this inspector, then reopen the script.",
                    or_io::gta5::script::ScrProgram::native_names_are_unavailable()
                ),
            )
        } else {
            or_cli::script_provenance(program, names)
        };
        let (text, _clean) = or_cli::script_listing(program, db.as_ref(), &sentence, true)?;
        let source = or_cli::script_source(program, db.as_ref(), &sentence)?;
        Ok(Self {
            listing: text.lines().map(str::to_owned).collect(),
            source: ropey::Rope::from_str(&source),
            named: db.is_some(),
            sentence,
        })
    }

    pub fn into_payload(self) -> DocPayload {
        DocPayload::Registered {
            format: crate::state::format_handlers::FormatId::new(
                or_io::container::resource::ResourceType::Ysc,
                or_io::Generation::Gen8,
            ),
            data: Arc::new(self),
        }
    }

    pub fn in_payload(payload: &DocPayload) -> Option<&Self> {
        match payload {
            DocPayload::Registered { data, .. } => data.downcast_ref(),
            _ => None,
        }
    }
}
