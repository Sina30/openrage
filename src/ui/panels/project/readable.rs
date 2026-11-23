//! The small pieces the power windows share: a line of a dry run or a review as a person reads it, and the filled button of the action a window exists for.

use crate::ui::theme;

/// `line` with its arrows drawn as arrows.
pub(super) fn arrows(line: &str) -> String {
    line.replace(" -> ", " → ")
}

/// The button of the one action a window is for, filled so that it reads apart from Cancel.
pub(super) fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(theme::ON_ACCENT.get()))
        .fill(theme::ACCENT.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_says_what_it_was_and_what_it_is_with_a_real_arrow() {
        assert_eq!(arrows("tags: none -> winter"), "tags: none → winter");
        assert_eq!(arrows("no change"), "no change");
    }
}
