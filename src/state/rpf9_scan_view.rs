//! The view a body a scan of a GTA VI archive found opens in.

use or_io::container::rpf::{Rpf9BodyKind, Rpf9Content, Rpf9ScanFind, Rpf9Signature};

/// The view the app already has that a body opens in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Xml,
    Text,
    Pso,
    /// A run with four zero bytes first, which opens only where GTA VI's RBM reader reads it.
    Headerless,
    Movie,
    Audio,
    /// GTA VI's animation reader: a stored RSC8 of version 65, or a headerless clip root.
    Animation,
    /// A headerless resource with a signature, shown as the facts its reader reads.
    Facts(Rpf9Signature),
    /// A headerless drawable, drawn in the model view.
    Drawable,
    /// A nested archive whose names the scan placed, listed by a scan of its own.
    Archive,
    /// A GTA VI text table, in the view GTA V's text tables have.
    TextTable,
    /// GTA VI audio game data, listed as GTA V's `.rel` items are.
    AudioData,
    /// A body its head names: a height map or Scaleform movie in GTA V's view, else its facts.
    Identified(or_io::gta6::other_bodies::OtherBody),
    /// GTA VI's `SGD5` effect container, shown as the lines its reader reads.
    Effects,
    /// A body no reader fits, shown as its bytes under the reason.
    Bytes,
}

impl View {
    /// The extension of the path the body opens under.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Xml => "xml",
            Self::Text => "txt",
            Self::Pso => "pso",
            Self::Headerless => "rbm",
            Self::Movie => "bik",
            Self::Audio => "awc",
            Self::Animation => "anim",
            Self::Facts(_) => "rsc",
            Self::Drawable => "drawable",
            Self::Archive => "rpf",
            Self::TextTable => "rsc",
            Self::AudioData => "rel",
            Self::Identified(body) => crate::state::gta6_other_bodies::ext(body),
            Self::Effects => "rsc",
            Self::Bytes => "bin",
        }
    }
}

/// The view a body opens in, or why no view shows it.
pub fn view_of(find: &Rpf9ScanFind) -> Result<View, String> {
    match find.kind {
        Rpf9BodyKind::Zlib(run) => match run.content {
            Rpf9Content::Xml => Ok(View::Xml),
            Rpf9Content::Text => Ok(View::Text),
            Rpf9Content::Pso | Rpf9Content::Rbf => Ok(View::Pso),
            Rpf9Content::ResourceShaped | Rpf9Content::ClipRoot | Rpf9Content::TextTable
                if run.resource.is_none() =>
            {
                Err(not_a_start())
            }
            Rpf9Content::TextTable => Ok(View::TextTable),
            Rpf9Content::AudioData => Ok(View::AudioData),
            Rpf9Content::Identified(body) => crate::state::gta6_other_bodies::view(body),
            Rpf9Content::ResourceShaped => Ok(match run.signature {
                Some(s) if s.role().is_some_and(drawn) => View::Drawable,
                signature => signature.map_or(View::Headerless, View::Facts),
            }),
            Rpf9Content::ClipRoot => Ok(View::Animation),
            Rpf9Content::PipelineCache => Err(tr!(
                "GTA VI's pipeline cache: `openrage-cli rpf scan-open` states its header and record counts. Its code is native GPU code of a console, which nothing here runs, so no view shows it."
            )
            .to_string()),
            Rpf9Content::Other => Err(tr!(
                "This zlib body's first chunk inflates to neither text, a PSO document nor a resource's shape, so what it holds is not known and no view shows it."
            )
            .to_string()),
        },
        Rpf9BodyKind::Bink2 { .. } => Ok(View::Movie),
        Rpf9BodyKind::Awc { .. } => Ok(View::Audio),
        Rpf9BodyKind::StoredRsc8 {
            version: or_io::gta6::anim::RSC8_VERSION,
            ..
        } => Ok(View::Animation),
        Rpf9BodyKind::StoredRsc8 { version, extent } => Err(tr!(
            "An RSC8 resource stored with no key: version byte {version}, a 16-byte header and a virtual section of {size} bytes, and no physical section. The file does not name its type and OpenRage reads only version 65, GTA VI's animation resource, so no view shows it.",
            version = version,
            size = extent,
        )),
        Rpf9BodyKind::Archive(_) if find.offset == 0 => Err(tr!(
            "This archive's own header, whose facts are given in the archive's tab."
        )
        .to_string()),
        Rpf9BodyKind::Sgd5 { .. } => Ok(View::Effects),
        Rpf9BodyKind::Archive(_) => Err(tr!(
            "A GTA VI archive nested in this one. Its header states its table, but where it ends is in the encrypted table of the archive that holds it, and the scan placed no names of its own after its last entry, so it does not open on its own; the bodies found after it are listed under it."
        )
        .to_string()),
        Rpf9BodyKind::Names { archive } => Err(tr!(
            "The encrypted names of the archive at {archive}, which end it at {end}. OpenRage has no key for them, so no view shows them.",
            archive = format!("{archive:#X}"),
            end = format!("{:#X}", find.offset + find.span),
        )),
        Rpf9BodyKind::FirstEntry { .. } if crate::state::gta6_first_entry::ends(find) => {
            Err(crate::state::gta6_first_entry::banner(find))
        }
        Rpf9BodyKind::FirstEntry { .. } => Err(tr!(
            "The first entry after an archive's table, where its header puts it. Its bytes start no format the scan knows and where it ends is not known, so no view shows it."
        )
        .to_string()),
        Rpf9BodyKind::RawDeflate { .. } => Ok(View::Pso),
    }
}

/// Whether a headerless resource of `role` opens in the model view.
fn drawn(role: or_io::container::rpf::Rpf9Role) -> bool {
    use or_io::container::rpf::Rpf9Role;
    matches!(
        role,
        Rpf9Role::Drawable | Rpf9Role::DrawableWithBound | Rpf9Role::Bound
    )
}

/// Why a run with four zero bytes first that starts no resource does not open on its own.
pub(super) fn not_a_start() -> String {
    tr!("This run starts with four zero bytes, but no headerless resource starts here: its first bytes state no block map, or neither a zero slot nor a page start comes before it. A block of a resource opens with the resource it belongs to.").to_string()
}
