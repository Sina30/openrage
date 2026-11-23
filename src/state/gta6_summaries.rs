//! What the status bar says of the part selected in a scanned GTA VI drawable or animation.

use super::DocPayload;

/// The drawable, dictionary entry or bound selected.
pub(super) fn gta6_drawable_summary(payload: &DocPayload, sel: u64) -> String {
    use or_io::gta6::headerless::Resource;
    let DocPayload::Gta6Drawable(resource) = payload else {
        return String::new();
    };
    let chosen = super::gta6_resource::drawable_of(resource, Some(sel));
    if let Some(said) = chosen.and_then(|d| super::gta6_selected::summary(&d.name, sel)) {
        return said;
    }
    if let Some(said) = crate::ui::panels::hierarchy::gta6_filters::summary(resource, sel) {
        return said;
    }
    match (&**resource, chosen) {
        (Resource::Dictionary(dict), Some(d)) => {
            use or_io::gta6::headerless::Entry;
            let at = (dict.entries.iter())
                .position(|(_, e)| matches!(e, Ok(Entry::Drawable(x)) if std::ptr::eq(&**x, d)));
            tr!(
                "Drawable: {label} ({at} of {total})",
                label = d.name,
                at = at.map_or(0, |i| i + 1),
                total = dict.entries.len()
            )
        }
        (Resource::Dictionary(_), None) => format!(
            "{} drawables",
            super::gta6_resource::dictionary_drawables(resource).count()
        ),
        (Resource::Bound(_), _) => tr!("Bound selected").to_string(),
        (_, Some(d)) => tr!("Drawable: {name}", name = d.name),
        _ => String::new(),
    }
}

/// The clip or animation selected, else how many the resource holds.
pub(super) fn gta6_anim_summary(payload: &DocPayload, sel: u64) -> String {
    use crate::ui::panels::inspector::{gta6_choice_count, gta6_choice_label};
    let DocPayload::Gta6Anim(resource) = payload else {
        return String::new();
    };
    let count = gta6_choice_count(resource);
    let animations: usize = resource.sets.iter().map(|s| s.animations.len()).sum();
    let chosen = usize::try_from(sel)
        .ok()
        .and_then(|i| gta6_choice_label(resource, i));
    match chosen {
        Some((label, seconds)) if !resource.clips.is_empty() => tr!(
            "Clip: {label} ({seconds}s of {clips} clips)",
            label = label,
            seconds = format!("{seconds:.2}"),
            clips = count
        ),
        Some((label, seconds)) => tr!(
            "Animation: {label} ({seconds}s of {count} animations)",
            label = label,
            seconds = format!("{seconds:.2}"),
            count = count
        ),
        None => tr!(
            "{clips} clips, {anim_dictionary} animations",
            clips = resource.clips.len(),
            anim_dictionary = animations
        ),
    }
}
