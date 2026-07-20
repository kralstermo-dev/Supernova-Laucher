use std::rc::Rc;

use gpui::{App, Hsla, SharedString, hsla};
use gpui_component::{Colorize, Theme, theme::ThemeConfigColors};

use crate::interface_config::{ColorPreset, InterfaceConfig};

/// Overwrites all 9 custom color slots from a saved preset, then recomputes.
pub fn apply_preset(preset: &ColorPreset, cx: &mut App) {
    let cfg = InterfaceConfig::get_mut(cx);
    cfg.custom_accent_color = preset.accent.clone();
    cfg.custom_background_color = preset.background.clone();
    cfg.custom_secondary_color = preset.secondary.clone();
    cfg.custom_text_color = preset.text.clone();
    cfg.custom_border_color = preset.border.clone();
    cfg.custom_danger_color = preset.danger.clone();
    cfg.custom_success_color = preset.success.clone();
    cfg.custom_warning_color = preset.warning.clone();
    cfg.custom_info_color = preset.info.clone();

    reapply_custom_colors(cx);
}

/// Captures the current 9 custom color slots into a new named preset.
pub fn capture_preset(name: SharedString, cx: &App) -> ColorPreset {
    let cfg = InterfaceConfig::get(cx);
    ColorPreset {
        name,
        accent: cfg.custom_accent_color.clone(),
        background: cfg.custom_background_color.clone(),
        secondary: cfg.custom_secondary_color.clone(),
        text: cfg.custom_text_color.clone(),
        border: cfg.custom_border_color.clone(),
        danger: cfg.custom_danger_color.clone(),
        success: cfg.custom_success_color.clone(),
        warning: cfg.custom_warning_color.clone(),
        info: cfg.custom_info_color.clone(),
    }
}

/// Recomputes the launcher's full color set from the active base theme plus
/// whichever custom color slots the user has set (accent, background, secondary,
/// text, border), and applies the result in a single pass. Any slot left empty
/// in `InterfaceConfig` falls through to the base theme's own color for it.
pub fn reapply_custom_colors(cx: &mut App) {
    let theme = Theme::global(cx);
    let base = if theme.mode.is_dark() {
        theme.dark_theme.clone()
    } else {
        theme.light_theme.clone()
    };

    let mut colors = base.colors.clone();
    let cfg = InterfaceConfig::get(cx);

    if let Some(color) = parse_hex(&cfg.custom_accent_color) {
        apply_accent(&mut colors, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_background_color) {
        apply_background(&mut colors, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_secondary_color) {
        apply_secondary(&mut colors, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_text_color) {
        apply_text(&mut colors, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_border_color) {
        apply_border(&mut colors, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_danger_color) {
        apply_status(&mut colors, StatusKind::Danger, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_success_color) {
        apply_status(&mut colors, StatusKind::Success, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_warning_color) {
        apply_status(&mut colors, StatusKind::Warning, color);
    }
    if let Some(color) = parse_hex(&cfg.custom_info_color) {
        apply_status(&mut colors, StatusKind::Info, color);
    }

    let mut new_config = (*base).clone();
    new_config.colors = colors;

    Theme::global_mut(cx).apply_config(&Rc::new(new_config));

    // Applying a new global Theme doesn't automatically repaint already-drawn
    // windows (this happens most noticeably on startup, since the themes folder
    // is scanned on a background task and finishes after the first frame is
    // painted) — explicitly ask every window to redraw so the change shows
    // immediately instead of waiting for some unrelated interaction.
    cx.refresh_windows();
}

/// Convenience used right after a single color picker changes: saves the hex
/// into the given config field via the caller, then recomputes everything.
pub fn apply_and_save(cx: &mut App) {
    reapply_custom_colors(cx);
}

fn parse_hex(hex: &SharedString) -> Option<Hsla> {
    let hex = hex.trim();
    if hex.is_empty() {
        return None;
    }
    Hsla::parse_hex(hex).ok()
}

fn contrast_foreground(color: Hsla) -> Hsla {
    if color.l > 0.6 {
        hsla(0.0, 0.0, 0.0, 1.0)
    } else {
        hsla(0.0, 0.0, 1.0, 1.0)
    }
}

fn apply_accent(colors: &mut ThemeConfigColors, color: Hsla) {
    let hover = color.lighten(0.08);
    let active = color.darken(0.10);
    let wash = color.opacity(0.16);
    let foreground = contrast_foreground(color);

    colors.primary = Some(color.to_hex().into());
    colors.primary_hover = Some(hover.to_hex().into());
    colors.primary_active = Some(active.to_hex().into());
    colors.primary_foreground = Some(foreground.to_hex().into());

    colors.button_primary = Some(color.to_hex().into());
    colors.button_primary_hover = Some(hover.to_hex().into());
    colors.button_primary_active = Some(active.to_hex().into());
    colors.button_primary_foreground = Some(foreground.to_hex().into());

    colors.ring = Some(color.to_hex().into());
    colors.caret = Some(color.to_hex().into());

    colors.link = Some(color.to_hex().into());
    colors.link_hover = Some(hover.to_hex().into());
    colors.link_active = Some(active.to_hex().into());

    colors.selection = Some(wash.to_hex().into());
    colors.list_active = Some(wash.to_hex().into());
    colors.list_active_border = Some(color.to_hex().into());

    colors.switch = Some(color.to_hex().into());
    colors.slider_bar = Some(color.to_hex().into());
    colors.progress_bar = Some(color.to_hex().into());

    colors.sidebar_primary = Some(color.to_hex().into());
    colors.sidebar_primary_foreground = Some(foreground.to_hex().into());
    colors.sidebar_accent = Some(wash.to_hex().into());
    colors.sidebar_accent_foreground = Some(foreground.to_hex().into());
}

fn apply_background(colors: &mut ThemeConfigColors, color: Hsla) {
    let hex: SharedString = color.to_hex().into();

    colors.background = Some(hex.clone());
    colors.popover = Some(hex.clone());
    colors.sidebar = Some(hex.clone());
    colors.table = Some(hex.clone());
    colors.list = Some(hex.clone());
    colors.tab_bar = Some(hex.clone());
    colors.title_bar = Some(hex);
}

fn apply_secondary(colors: &mut ThemeConfigColors, color: Hsla) {
    let hover = color.lighten(0.06);
    let active = color.darken(0.06);
    let foreground = contrast_foreground(color);

    colors.secondary = Some(color.to_hex().into());
    colors.secondary_hover = Some(hover.to_hex().into());
    colors.secondary_active = Some(active.to_hex().into());
    colors.secondary_foreground = Some(foreground.to_hex().into());

    colors.muted = Some(color.to_hex().into());
    colors.muted_foreground = Some(foreground.to_hex().into());
}

fn apply_text(colors: &mut ThemeConfigColors, color: Hsla) {
    let hex: SharedString = color.to_hex().into();

    colors.foreground = Some(hex.clone());
    colors.popover_foreground = Some(hex.clone());
    colors.sidebar_foreground = Some(hex.clone());
    colors.tab_foreground = Some(hex);
}

fn apply_border(colors: &mut ThemeConfigColors, color: Hsla) {
    let hex: SharedString = color.to_hex().into();

    colors.border = Some(hex.clone());
    colors.input = Some(hex.clone());
    colors.sidebar_border = Some(hex.clone());
    colors.title_bar_border = Some(hex);
}

pub enum StatusKind {
    Danger,
    Success,
    Warning,
    Info,
}

fn apply_status(colors: &mut ThemeConfigColors, kind: StatusKind, color: Hsla) {
    let hover = color.lighten(0.06);
    let active = color.darken(0.08);
    let foreground = contrast_foreground(color);

    let (base, hover_slot, active_slot, foreground_slot) = match kind {
        StatusKind::Danger => (
            &mut colors.danger,
            &mut colors.danger_hover,
            &mut colors.danger_active,
            &mut colors.danger_foreground,
        ),
        StatusKind::Success => (
            &mut colors.success,
            &mut colors.success_hover,
            &mut colors.success_active,
            &mut colors.success_foreground,
        ),
        StatusKind::Warning => (
            &mut colors.warning,
            &mut colors.warning_hover,
            &mut colors.warning_active,
            &mut colors.warning_foreground,
        ),
        StatusKind::Info => (
            &mut colors.info,
            &mut colors.info_hover,
            &mut colors.info_active,
            &mut colors.info_foreground,
        ),
    };

    *base = Some(color.to_hex().into());
    *hover_slot = Some(hover.to_hex().into());
    *active_slot = Some(active.to_hex().into());
    *foreground_slot = Some(foreground.to_hex().into());
}
