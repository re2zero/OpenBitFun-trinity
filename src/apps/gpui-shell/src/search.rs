//! Shell global search page: workspace file content and persisted session
//! content in one panel.
//!
//! Files search runs through the core `WorkspaceSearchService` (the same
//! managed daemon the desktop hosts, with its ripgrep/scan fallbacks and the
//! same config gate — the page degrades loudly when the feature or the daemon
//! is missing). Session search runs through the `ProductSearchPort` the
//! runtime handle already carries. Results cross into the shell through the
//! event pump like every other page; clicking a hit jumps via the shell weak
//! handle: file hits open in the files viewer, session hits open the session
//! in the chat pane.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt as _};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{div, prelude::*, App, Entity, SharedString, WeakEntity};
use openbitfun_core::service::search::WorkspaceSearchHit;
use openbitfun_runtime_ports::SessionSearchHit;
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};
use crate::shell::ShellWindow;

/// Request/display cap shared by both tabs; the panels show flat lists, so a
/// bounded payload keeps one broad query from flooding them.
pub(crate) const SEARCH_LIMIT: usize = 200;

/// Which surface the panel searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SearchTab {
    #[default]
    Files,
    Sessions,
}

/// State of the global search page; lives on the shell window and updates
/// only through [`RuntimeEvent`] projections (insights/tasks pattern).
#[derive(Debug, Default)]
pub(crate) struct SearchPageState {
    pub tab: SearchTab,
    /// Flattened files-tab rows (file + line order) for the last query.
    pub content: Vec<ContentMatchRow>,
    /// Sessions-tab hits for the last query.
    pub sessions: Vec<SessionSearchHit>,
    pub truncated: bool,
    pub running: bool,
    /// Feature/daemon gate detail; `Some` blocks the results area.
    pub unavailable: Option<Option<String>>,
    pub error: Option<String>,
    /// The last executed query (drives the before-first-search state).
    pub query: String,
}

/// One display row of the files tab: a match line with its file.
#[derive(Debug, Clone)]
pub(crate) struct ContentMatchRow {
    pub path: String,
    pub line: usize,
    pub snippet: String,
}

/// Flattens grouped hits into display rows (file, then line order).
pub(crate) fn flatten_content_hits(
    hits: &[WorkspaceSearchHit],
    cap: usize,
) -> Vec<ContentMatchRow> {
    let mut rows = Vec::new();
    for hit in hits {
        for matching in &hit.matches {
            if rows.len() >= cap {
                return rows;
            }
            rows.push(ContentMatchRow {
                path: hit.path.clone(),
                line: matching.location.line,
                snippet: matching.snippet.clone(),
            });
        }
    }
    rows
}

/// The full page: tab switch, query row, and the active tab's results.
pub(crate) fn render_page(
    view: WeakEntity<ShellWindow>,
    state: &SearchPageState,
    input: &Entity<InputState>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut column = div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .text_color(cx.theme().foreground)
        .child(
            div()
                .text_lg()
                .text_color(cx.theme().foreground)
                .child(text.search_title.clone()),
        );

    let tabs = [
        (SearchTab::Files, text.search_files_tab.clone()),
        (SearchTab::Sessions, text.search_sessions_tab.clone()),
    ];
    let mut bar = div().flex().items_center().gap_1();
    for (tab, label) in tabs {
        let active = state.tab == tab;
        let switch_view = view.clone();
        bar = bar.child(
            Button::new(SharedString::from(format!(
                "search-tab-{}",
                tab_id_suffix(tab)
            )))
            .label(label)
            .small()
            .with_variant(if active {
                ButtonVariant::Secondary
            } else {
                ButtonVariant::Ghost
            })
            .on_click(move |_, _, cx| {
                switch_view
                    .update(cx, |shell, cx| {
                        shell.search.tab = tab;
                        cx.notify();
                    })
                    .ok();
            }),
        );
    }
    column = column.child(bar);

    let run_view = view.clone();
    let run_tx = tx.clone();
    column = column.child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().child(Input::new(input)))
            .child(
                Button::new("search-run")
                    .label(text.search_run.clone())
                    .small()
                    .with_variant(ButtonVariant::Primary)
                    .on_click(move |_, _, cx| {
                        run_view
                            .update(cx, |shell, cx| shell.run_search(&run_tx, cx))
                            .ok();
                    }),
            ),
    );

    if state.running {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.search_running.clone()),
        );
    }
    if let Some(error) = &state.error {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone()),
        );
    }

    column = column.child(render_results(view, state, tx, text, cx));
    column.into_any_element()
}

fn tab_id_suffix(tab: SearchTab) -> &'static str {
    match tab {
        SearchTab::Files => "files",
        SearchTab::Sessions => "sessions",
    }
}

fn render_results(
    view: WeakEntity<ShellWindow>,
    state: &SearchPageState,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    if state.query.is_empty() {
        return div().flex_1().into_any_element();
    }
    if let Some(detail) = &state.unavailable {
        let mut notice = div()
            .flex_1()
            .v_flex()
            .gap_1()
            .p_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.search_unavailable.clone());
        if let Some(hint) = detail {
            notice = notice.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint.clone()),
            );
        }
        return notice.into_any_element();
    }

    match state.tab {
        SearchTab::Files => {
            if state.content.is_empty() {
                return empty_results(text);
            }
            let rows = state.content.clone();
            let mut list = div()
                .id("search-content-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .v_flex()
                .gap_1();
            for (index, row) in rows.iter().enumerate() {
                let path = row.path.clone();
                let jump = view.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("search-hit-{index}")))
                        .cursor_pointer()
                        .v_flex()
                        .gap_0p5()
                        .p_2()
                        .rounded(cx.theme().radius)
                        .hover(|style| style.bg(cx.theme().sidebar_accent))
                        .on_click(move |_, _, cx| {
                            jump.update(cx, |shell, cx| {
                                shell.open_file_in_files_view(path.clone(), cx)
                            })
                            .ok();
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().primary)
                                .truncate()
                                .child(format!("{}:{}", row.path, row.line)),
                        )
                        .child(div().text_sm().truncate().child(row.snippet.clone())),
                );
            }
            let mut section = div().flex_1().min_h_0().v_flex().gap_1().child(list);
            if state.truncated {
                section = section.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.search_truncated.clone()),
                );
            }
            section.into_any_element()
        }
        SearchTab::Sessions => {
            if state.sessions.is_empty() {
                return empty_results(text);
            }
            let hits = state.sessions.clone();
            let mut list = div()
                .id("search-session-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .v_flex()
                .gap_1();
            for (index, hit) in hits.iter().enumerate() {
                let session_id = hit.session_id.clone();
                let run_tx = tx.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("search-session-{index}")))
                        .cursor_pointer()
                        .v_flex()
                        .gap_0p5()
                        .p_2()
                        .rounded(cx.theme().radius)
                        .hover(|style| style.bg(cx.theme().sidebar_accent))
                        .on_click(move |_, _, _cx| {
                            // Opening the session emits SessionOpened, whose
                            // handler moves the shell to the chat pane.
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.open_session(session_id.clone(), run_tx.clone());
                            }
                        })
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(hit.session_title.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(hit.snippet.clone()),
                        ),
                );
            }
            let mut section = div().flex_1().min_h_0().v_flex().gap_1().child(list);
            if state.truncated {
                section = section.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.search_truncated.clone()),
                );
            }
            section.into_any_element()
        }
    }
}

fn empty_results(text: &ShellText) -> gpui_kit::AnyElement {
    crate::shell::brand_empty(
        IconName::Search,
        &text.search_empty_title,
        &text.search_empty,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbitfun_core::service::search::{WorkspaceSearchMatch, WorkspaceSearchMatchLocation};

    fn hit(path: &str, lines: &[usize]) -> WorkspaceSearchHit {
        WorkspaceSearchHit {
            path: path.to_string(),
            matches: lines
                .iter()
                .map(|line| WorkspaceSearchMatch {
                    location: WorkspaceSearchMatchLocation {
                        line: *line,
                        column: 0,
                    },
                    snippet: format!("snippet-{line}"),
                    matched_text: "query".to_string(),
                })
                .collect(),
            lines: Vec::new(),
        }
    }

    #[test]
    fn content_hits_flatten_in_order_and_cap() {
        let hits = vec![hit("/w/a.rs", &[1, 5]), hit("/w/b.md", &[3])];
        let rows = flatten_content_hits(&hits, SEARCH_LIMIT);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].path, "/w/a.rs");
        assert_eq!(rows[0].line, 1);
        assert_eq!(rows[1].line, 5);
        assert_eq!(rows[2].path, "/w/b.md");

        let capped = flatten_content_hits(&hits, 2);
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn empty_hit_groups_produce_no_rows() {
        let hits = vec![hit("/w/empty.rs", &[])];
        assert!(flatten_content_hits(&hits, SEARCH_LIMIT).is_empty());
    }
}
