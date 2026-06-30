use crate::color::blend;
use crate::color::is_light;
use crate::terminal_palette::StdoutColorLevel;
use crate::terminal_palette::best_color;
use crate::terminal_palette::default_bg;
use crate::terminal_palette::default_fg;
use crate::terminal_palette::rgb_color;
use crate::terminal_palette::stdout_color_level;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;

// AIMUX brand palette. A single electric-indigo accent, applied sparingly to
// the wordmark, the active model name, the spinner band, and the picker chrome.
/// Primary accent — electric indigo (#7C5CFF).
pub(crate) const AIMUX_ACCENT_RGB: (u8, u8, u8) = (124, 92, 255);
/// Secondary / dim accent — muted indigo (#5646A0) for chrome that must recede.
pub(crate) const AIMUX_ACCENT_DIM_RGB: (u8, u8, u8) = (86, 70, 160);
/// On-light fallback — darker indigo (#4A2FBD) for light terminals.
pub(crate) const AIMUX_ACCENT_LIGHT_RGB: (u8, u8, u8) = (74, 47, 189);

// Decorative table rules should remain visible without competing with cell content.
const TABLE_SEPARATOR_FG_ALPHA: f32 = 0.20;

/// Brand accent color (indigo), independent of selection state. Used by the
/// wordmark, the `»` brand glyph, and the active model name.
pub(crate) fn brand_accent() -> Color {
    if default_bg().is_some_and(is_light) {
        rgb_color(AIMUX_ACCENT_LIGHT_RGB)
    } else {
        rgb_color(AIMUX_ACCENT_RGB)
    }
}

/// Muted brand accent (dim indigo). Used for the card border, version tag,
/// `/model` hint, and picker group headers.
pub(crate) fn brand_dim() -> Color {
    rgb_color(AIMUX_ACCENT_DIM_RGB)
}

pub fn user_message_style() -> Style {
    user_message_style_for(default_bg())
}

pub fn proposed_plan_style() -> Style {
    proposed_plan_style_for(default_bg())
}

/// Returns a low-contrast rule style for separators within markdown tables.
pub(crate) fn table_separator_style() -> Style {
    table_separator_style_for(default_fg(), default_bg(), stdout_color_level())
}

/// Returns the shared accent style for active or selected TUI controls.
pub(crate) fn accent_style() -> Style {
    accent_style_for(default_bg())
}

/// Returns the style for a user-authored message using the provided terminal background.
pub fn user_message_style_for(terminal_bg: Option<(u8, u8, u8)>) -> Style {
    match terminal_bg {
        Some(bg) => Style::default().bg(user_message_bg(bg)),
        None => Style::default(),
    }
}

pub fn proposed_plan_style_for(terminal_bg: Option<(u8, u8, u8)>) -> Style {
    match terminal_bg {
        Some(bg) => Style::default().bg(proposed_plan_bg(bg)),
        None => Style::default(),
    }
}

/// Returns the shared accent style for the provided terminal background.
///
/// AIMUX uses a single electric-indigo accent. On truecolor terminals the exact
/// RGB is used; on lower color levels it degrades to the nearest palette violet
/// rather than falling back to a named cyan (the old Codex accent).
pub(crate) fn accent_style_for(terminal_bg: Option<(u8, u8, u8)>) -> Style {
    let accent_rgb = if terminal_bg.is_some_and(is_light) {
        AIMUX_ACCENT_LIGHT_RGB
    } else {
        AIMUX_ACCENT_RGB
    };
    match stdout_color_level() {
        StdoutColorLevel::TrueColor => Style::default().fg(rgb_color(accent_rgb)).bold(),
        _ => Style::default().fg(best_color(accent_rgb)).bold(),
    }
}

fn table_separator_style_for(
    terminal_fg: Option<(u8, u8, u8)>,
    terminal_bg: Option<(u8, u8, u8)>,
    color_level: StdoutColorLevel,
) -> Style {
    let (Some(fg), Some(bg)) = (terminal_fg, terminal_bg) else {
        return Style::default().dim();
    };
    let separator_rgb = blend(fg, bg, TABLE_SEPARATOR_FG_ALPHA);
    match color_level {
        StdoutColorLevel::TrueColor => Style::default().fg(rgb_color(separator_rgb)),
        StdoutColorLevel::Ansi256 => Style::default().fg(best_color(separator_rgb)),
        StdoutColorLevel::Ansi16 | StdoutColorLevel::Unknown => Style::default().dim(),
    }
}

#[allow(clippy::disallowed_methods)]
pub fn user_message_bg(terminal_bg: (u8, u8, u8)) -> Color {
    let (top, alpha) = if is_light(terminal_bg) {
        ((0, 0, 0), 0.04)
    } else {
        ((255, 255, 255), 0.12)
    };
    best_color(blend(top, terminal_bg, alpha))
}

#[allow(clippy::disallowed_methods)]
pub fn proposed_plan_bg(terminal_bg: (u8, u8, u8)) -> Color {
    user_message_bg(terminal_bg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Modifier;

    fn expected_accent_fg(rgb: (u8, u8, u8)) -> Option<Color> {
        match stdout_color_level() {
            StdoutColorLevel::TrueColor => Some(rgb_color(rgb)),
            _ => Some(best_color(rgb)),
        }
    }

    #[test]
    fn accent_style_uses_darker_indigo_on_light_backgrounds() {
        let style = accent_style_for(Some((255, 255, 255)));

        assert_eq!(style.fg, expected_accent_fg(AIMUX_ACCENT_LIGHT_RGB));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn accent_style_uses_indigo_on_dark_or_unknown_backgrounds() {
        let expected_fg = expected_accent_fg(AIMUX_ACCENT_RGB);

        assert_eq!(accent_style_for(Some((0, 0, 0))).fg, expected_fg);
        assert_eq!(accent_style_for(/*terminal_bg*/ None).fg, expected_fg);
    }

    #[test]
    fn table_separator_blends_toward_dark_background() {
        let style = table_separator_style_for(
            Some((255, 255, 255)),
            Some((0, 0, 0)),
            StdoutColorLevel::TrueColor,
        );

        assert_eq!(style.fg, Some(rgb_color((51, 51, 51))));
    }

    #[test]
    fn table_separator_blends_toward_light_background() {
        let style = table_separator_style_for(
            Some((0, 0, 0)),
            Some((255, 255, 255)),
            StdoutColorLevel::TrueColor,
        );

        assert_eq!(style.fg, Some(rgb_color((204, 204, 204))));
    }

    #[test]
    fn table_separator_dims_when_palette_aware_color_is_unavailable() {
        let expected = Style::default().dim();

        assert_eq!(
            table_separator_style_for(
                Some((255, 255, 255)),
                Some((0, 0, 0)),
                StdoutColorLevel::Ansi16,
            ),
            expected
        );
        assert_eq!(
            table_separator_style_for(
                /*terminal_fg*/ None,
                Some((0, 0, 0)),
                StdoutColorLevel::TrueColor,
            ),
            expected
        );
    }
}
