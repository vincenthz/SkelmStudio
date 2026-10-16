//! Small formatting helpers and shared bits of UI.

use std::time::{Duration, SystemTime};

use gpui::{AnyElement, App, IntoElement, ParentElement, SharedString, Styled, div, prelude::FluentBuilder as _, px};
use gpui_component::StyledExt as _;
use gpui_component::{ActiveTheme as _, Icon, h_flex};

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn human_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}K", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

pub fn human_duration(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h", secs / 3600),
        86400..2_592_000 => format!("{}d", secs / 86400),
        2_592_000..31_536_000 => format!("{}mo", secs / 2_592_000),
        _ => format!("{}y", secs / 31_536_000),
    }
}

pub fn ago(time: Option<SystemTime>) -> String {
    match time.and_then(|t| SystemTime::now().duration_since(t).ok()) {
        Some(d) => format!("{} ago", human_duration(d)),
        None => "—".into(),
    }
}

/// `2025-06-01T12:00:00.000Z` → `2025-06-01`
pub fn date_of(iso: &str) -> String {
    iso.chars().take(10).collect()
}

/// A small uppercase section heading
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// A label / value line of a details panel
pub fn info_row(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap_3()
        .justify_between()
        .text_sm()
        .child(
            div()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(div().min_w_0().truncate().child(value.into()))
        .into_any_element()
}

/// A small rounded tag
pub fn chip(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .px_1p5()
        .py_0p5()
        .rounded(px(4.))
        .text_xs()
        .bg(cx.theme().secondary)
        .text_color(cx.theme().secondary_foreground)
        .child(text.into())
        .into_any_element()
}

/// An icon followed by a muted label
pub fn icon_label(
    icon: impl Into<Icon>,
    text: impl Into<SharedString>,
    muted: bool,
    cx: &App,
) -> AnyElement {
    h_flex()
        .gap_1()
        .items_center()
        .when(muted, |d| d.text_color(cx.theme().muted_foreground))
        .child(icon.into().size_3p5())
        .child(text.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(4_683_000_000), "4.68 GB");
        assert_eq!(human_size(397_000_000), "397 MB");
        assert_eq!(human_count(1_234_567), "1.2M");
    }
}
