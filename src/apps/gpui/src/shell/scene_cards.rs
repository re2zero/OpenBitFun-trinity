// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// The quiet-card container scene pages render their sections on (style.md
/// §3): the settings-page card geometry — panel surface, hairline boundary,
/// the theme's large radius — with an arbitrary body instead of label/control
/// rows. Established by the tasks board (E4-b2), shared from E4-b3 on.
pub(crate) fn scene_card(anchor: &'static str, cx: &App) -> gpui_kit::Div {
    div()
        .debug_selector(move || format!("scene-card-{anchor}"))
        .w_full()
        .flex()
        .flex_col()
        .bg(cx.theme().group_box)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
}

/// A card head: title and optional muted description on the left, the extra
/// lane on the right (the `SettingsCard` head shape).
pub(crate) fn scene_card_head(
    title: String,
    description: Option<String>,
    extra: Option<gpui_kit::AnyElement>,
    cx: &App,
) -> gpui_kit::Div {
    div().px_4().pt_4().pb_3().child(
        div()
            .flex()
            .flex_row()
            .items_start()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .min_w_0()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(title),
                    )
                    .children(description.map(|text| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(text)
                    })),
            )
            .children(extra),
    )
}

/// A row inside a scene card: the settings-card row geometry (12px block,
/// hairline between rows) for list-shaped bodies.
pub(crate) fn scene_card_row(divider: bool, cx: &App) -> gpui_kit::Div {
    div().flex().flex_col().px_4().py_3().when(divider, |row| {
        row.border_t_1()
            .border_color(cx.theme().border.opacity(0.6))
    })
}
