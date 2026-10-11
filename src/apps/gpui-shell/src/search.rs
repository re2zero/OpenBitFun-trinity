//! Shell global search overlay (R1b): the sidebar's "search everything"
//! trigger (Ctrl K) opens a top-anchored palette that searches workspace file
//! content and persisted session content in one pass, mirroring the desktop's
//! global search.
//!
//! Files search runs through the core `WorkspaceSearchService` (the same
//! managed daemon the desktop hosts, with its ripgrep/scan fallbacks and the
//! same config gate — the palette degrades loudly when the feature or the
//! daemon is missing). Session search runs through the `ProductSearchPort`
//! the runtime handle already carries. Results cross into the shell through
//! the event pump like every other surface; a hit click jumps via the shell
//! weak handle (file hits open the files viewer, session hits open the
//! session tab) and closes the overlay — the same jump paths the former
//! search page used before R1b folded it into this overlay.
//!
//! The empty-query state lists quick actions — the shell-side stand-in for
//! the desktop product-action catalog, and since R1b the entry point for the
//! scenes whose nav rows were converged away (R2c re-homes files/git into the
//! session workbench).

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Enter, Escape, Input, InputState};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, StyledExt as _};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{div, prelude::*, px, App, Entity, SharedString, WeakEntity};
use openbitfun_core::service::search::WorkspaceSearchHit;
use openbitfun_runtime_ports::SessionSearchHit;
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};
use crate::shell::ShellWindow;

/// Request/display cap shared by both paths; the palette shows flat lists, so
/// a bounded payload keeps one broad query from flooding it.
pub(crate) const SEARCH_LIMIT: usize = 200;

/// Overlay panel geometry: desktop-palette-shaped (top-anchored card over a
/// window scrim).
const PANEL_WIDTH: f32 = 620.0;
const PANEL_TOP_INSET: f32 = 96.0;

/// State of the global search overlay; lives on the shell window and updates
/// only through [`RuntimeEvent`] projections (insights/tasks pattern).
#[derive(Debug, Default)]
pub(crate) struct SearchPageState {
    /// Flattened file-content rows (file + line order) for the last query.
    pub content: Vec<ContentMatchRow>,
    /// Session hits for the last query.
    pub sessions: Vec<SessionSearchHit>,
    pub truncated: bool,
    pub running: bool,
    /// Feature/daemon gate detail; `Some` blocks the files section.
    pub unavailable: Option<Option<String>>,
    pub error: Option<String>,
    /// The last executed query (drives the before-first-search state).
    pub query: String,
}

/// One display row of the file results: a match line with its file.
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

/// Palette quick actions (empty-query state). Terminal/Git/Files/Insights are
/// also the stopgap entry points for the scenes whose nav rows R1b removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchQuickAction {
    NewSession,
    Terminal,
    Files,
    Git,
    Insights,
    Settings,
}

/// Row order mirrors the desktop product-action catalog priorities (session
/// creation first, then surfaces).
pub(crate) const QUICK_ACTIONS: &[SearchQuickAction] = &[
    SearchQuickAction::NewSession,
    SearchQuickAction::Terminal,
    SearchQuickAction::Files,
    SearchQuickAction::Git,
    SearchQuickAction::Insights,
    SearchQuickAction::Settings,
];

impl SearchQuickAction {
    fn icon(self) -> IconName {
        match self {
            SearchQuickAction::NewSession => IconName::Plus,
            SearchQuickAction::Terminal => IconName::SquareTerminal,
            SearchQuickAction::Files => IconName::FolderOpen,
            SearchQuickAction::Git => IconName::GitBranch,
            SearchQuickAction::Insights => IconName::ChartPie,
            SearchQuickAction::Settings => IconName::Settings,
        }
    }

    fn label<'a>(self, text: &'a ShellText) -> &'a str {
        match self {
            SearchQuickAction::NewSession => &text.sessions_new,
            SearchQuickAction::Terminal => &text.terminal_title,
            SearchQuickAction::Files => &text.files_title,
            SearchQuickAction::Git => &text.git_title,
            SearchQuickAction::Insights => &text.insights_title,
            SearchQuickAction::Settings => &text.settings_title,
        }
    }
}

/// The full-screen overlay: a window scrim with a top-anchored palette card.
/// Rendered as the last child of the window root, so it paints above the
/// sidebar, main area, and docked terminal; `occlude` keeps clicks from
/// reaching the surfaces below while it is open.
pub(crate) fn render_overlay(
    view: WeakEntity<ShellWindow>,
    state: &SearchPageState,
    input: &Entity<InputState>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let backdrop_view = view.clone();
    let run_view = view.clone();
    let run_tx = tx.clone();
    let close_view = view.clone();
    div()
        .id("search-overlay")
        .debug_selector(|| "search-overlay".to_string())
        .absolute()
        .inset_0()
        .occlude()
        .bg(cx.theme().overlay)
        // Backdrop press closes; the palette card stops the propagation of its
        // own presses so clicks inside don't dismiss (gpui bubbles mouse
        // events up the tree, same mechanism as the tab-chip close button).
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
            if let Some(view) = backdrop_view.upgrade() {
                view.update(cx, |shell, cx| shell.close_search_overlay(window, cx));
            }
        })
        // Enter and Escape arrive as actions dispatched by the focused input
        // (gpui-component search-panel pattern): Enter runs the query, Escape
        // closes the palette.
        .on_action(move |_: &Enter, _, cx| {
            run_view
                .update(cx, |shell, cx| shell.run_search(&run_tx, cx))
                .ok();
        })
        .on_action(move |_: &Escape, window, cx| {
            close_view
                .update(cx, |shell, cx| shell.close_search_overlay(window, cx))
                .ok();
        })
        .child(
            div()
                .id("search-overlay-panel")
                .debug_selector(|| "search-overlay-panel".to_string())
                .absolute()
                .top(px(PANEL_TOP_INSET))
                .left(px(0.0))
                .right(px(0.0))
                .flex()
                .justify_center()
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .w(px(PANEL_WIDTH))
                        .max_w_full()
                        .v_flex()
                        .bg(cx.theme().popover)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_lg)
                        .shadow_lg()
                        .overflow_hidden()
                        .child(query_row(view.clone(), state, input, tx, text, cx))
                        .child(render_body(view, state, tx, text, cx)),
                ),
        )
        .into_any_element()
}

/// The query row: the input plus the run button (Enter runs too).
fn query_row(
    view: WeakEntity<ShellWindow>,
    state: &SearchPageState,
    input: &Entity<InputState>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut row = div()
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().flex_1().min_w_0().child(Input::new(input)));
    if state.running {
        row = row.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.search_running.clone()),
        );
    } else {
        let run_view = view.clone();
        let run_tx = tx.clone();
        row = row.child(
            Button::new("search-overlay-run")
                .label(text.search_run.clone())
                .small()
                .with_variant(ButtonVariant::Primary)
                .on_click(move |_, _, cx| {
                    run_view
                        .update(cx, |shell, cx| shell.run_search(&run_tx, cx))
                        .ok();
                }),
        );
    }
    row.into_any_element()
}

/// Below the query row: quick actions while the query is empty, else the two
/// result sections (sessions first, like the desktop palette's people-first
/// grouping) with notices and the empty state.
fn render_body(
    view: WeakEntity<ShellWindow>,
    state: &SearchPageState,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut body = div()
        .id("search-overlay-results")
        .debug_selector(|| "search-overlay-results".to_string())
        .flex_1()
        .min_h_0()
        .max_h(px(420.0))
        .overflow_y_scroll()
        .v_flex()
        .py_1();

    if let Some(error) = &state.error {
        body = body.child(
            div()
                .px_3()
                .py_1()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone()),
        );
    }

    if state.query.is_empty() {
        for action in QUICK_ACTIONS {
            body = body.child(quick_action_row(view.clone(), *action, text, cx));
        }
        return body.into_any_element();
    }

    if !state.sessions.is_empty() {
        body = body.child(section_header(&text.search_sessions_tab, cx));
        for (index, hit) in state.sessions.iter().enumerate() {
            body = body.child(session_hit_row(view.clone(), index, hit, tx, cx));
        }
    }
    if let Some(detail) = &state.unavailable {
        let mut notice = div()
            .px_3()
            .py_1()
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
        body = body.child(notice);
    } else if !state.content.is_empty() {
        body = body.child(section_header(&text.search_files_tab, cx));
        for (index, row) in state.content.iter().enumerate() {
            body = body.child(content_hit_row(view.clone(), index, row, cx));
        }
    }
    if state.truncated {
        body = body.child(
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text.search_truncated.clone()),
        );
    }
    if !state.running
        && state.sessions.is_empty()
        && state.content.is_empty()
        && state.unavailable.is_none()
        && state.error.is_none()
    {
        body = body.child(div().px_3().py_4().child(crate::shell::brand_empty(
            IconName::Search,
            &text.search_empty_title,
            &text.search_empty,
        )));
    }
    body.into_any_element()
}

fn section_header(label: &str, cx: &App) -> gpui_kit::AnyElement {
    div()
        .px_3()
        .pt_2()
        .pb_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(label.to_string())
        .into_any_element()
}

fn quick_action_row(
    view: WeakEntity<ShellWindow>,
    action: SearchQuickAction,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let label = action.label(text).to_string();
    div()
        .id(SharedString::from(format!("search-action-{:?}", action)))
        .debug_selector(move || format!("search-action-{:?}", action))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_x_2()
        .px_3()
        .py_1p5()
        .text_sm()
        .text_color(cx.theme().popover_foreground)
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .on_click(move |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |shell, cx| {
                    shell.run_search_quick_action(action, window, cx)
                });
            }
        })
        .child(Icon::new(action.icon()))
        .child(label)
        .into_any_element()
}

fn session_hit_row(
    view: WeakEntity<ShellWindow>,
    index: usize,
    hit: &SessionSearchHit,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let session_id = hit.session_id.clone();
    let open_tx = tx.clone();
    div()
        .id(SharedString::from(format!("search-session-{index}")))
        .cursor_pointer()
        .v_flex()
        .gap_0p5()
        .px_3()
        .py_1p5()
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .on_click(move |_, window, cx| {
            // Opening the session emits SessionOpened, whose handler opens
            // (or activates) the session tab; the palette then closes.
            if let Some(handle) = runtime::runtime_handle() {
                handle.open_session(session_id.clone(), open_tx.clone());
            }
            if let Some(view) = view.upgrade() {
                view.update(cx, |shell, cx| shell.close_search_overlay(window, cx));
            }
        })
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().popover_foreground)
                .truncate()
                .child(hit.session_title.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(hit.snippet.clone()),
        )
        .into_any_element()
}

fn content_hit_row(
    view: WeakEntity<ShellWindow>,
    index: usize,
    row: &ContentMatchRow,
    cx: &App,
) -> gpui_kit::AnyElement {
    let path = row.path.clone();
    div()
        .id(SharedString::from(format!("search-hit-{index}")))
        .cursor_pointer()
        .v_flex()
        .gap_0p5()
        .px_3()
        .py_1p5()
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .on_click(move |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |shell, cx| {
                    shell.open_file_in_files_view(path.clone(), cx);
                    shell.close_search_overlay(window, cx);
                });
            }
        })
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().primary)
                .truncate()
                .child(SharedString::from(format!("{}:{}", row.path, row.line))),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().popover_foreground)
                .truncate()
                .child(row.snippet.clone()),
        )
        .into_any_element()
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

    #[test]
    fn quick_actions_carry_icons_and_labels() {
        let text = crate::i18n::fallback_text(Default::default());
        for action in QUICK_ACTIONS {
            let _ = action.icon();
            assert!(!action.label(&text).is_empty());
        }
    }
}
