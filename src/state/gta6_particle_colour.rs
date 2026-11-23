//! A GTA VI `ptxu_Colour`'s maximum in its GTA V form: enabled where its count of colours in use
//! enables it and the maximum holds a key, and lines for what is not known of the rest.

use or_io::decl::Bool8;
use or_io::gta6::headerless::{Behaviour, BehaviourFields, COLOUR_RGBA_KFPS};

/// The colours GTA V's record holds: a minimum and a maximum.
const GTA5_COLOURS: u32 = 2;

fn keyed_max(b: &Behaviour) -> bool {
    (b.kfps.iter()).any(|k| k.id == COLOUR_RGBA_KFPS[1] && !k.entries.is_empty())
}

/// GTA V's `rgbaMaxEnable` for `b`: its count enables the maximum and the maximum holds a key.
pub(crate) fn max_enable(b: &Behaviour) -> Bool8 {
    Bool8::new(b.fields.colour_max_enabled() && keyed_max(b))
}

/// What is not known of `b`'s colours past GTA V's two, or of an enabled empty maximum.
pub(super) fn lines(b: &Behaviour, lines: &mut Vec<String>) {
    let BehaviourFields::Colour { colours_used } = b.fields else {
        return;
    };
    if colours_used > GTA5_COLOURS {
        lines.push(tr!(
            "Its ptxu_Colour counts {count} colours in use; what the colours past the second draw is not known, so the preview draws its minimum and maximum alone.",
            count = colours_used,
        ));
    }
    if b.fields.colour_max_enabled() && !keyed_max(b) {
        lines.push(tr!("Its ptxu_Colour enables its maximum, which holds no key; what that draws is not known, so the preview draws its minimum alone.").to_string());
    }
}
