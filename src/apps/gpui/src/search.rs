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
//!
//! G-Q1 turns the palette into the desktop's federated shape: a query also
//! filters the action catalog into its own section, and entity hits from the
//! skill and agent catalogs join sessions and files as result sections.
//! Entities filter at render time from the gpui globals (no query round
//! trip), and a hit jumps to the owning settings scene.

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

/// Display cap per entity section (G-Q1). The palette is a flat card, so one
/// broad prefix (a single letter) must not flood it; the section says when it
/// stopped short.
const ENTITY_RESULT_CAP: usize = 8;

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

/// Case-insensitive substring match behind the G-Q1 actions and entity
/// sections; `to_lowercase` folds ASCII and leaves CJK queries intact.
fn query_matches(query: &str, haystack: &str) -> bool {
    haystack.to_lowercase().contains(&query.to_lowercase())
}

/// Filters the action catalog by label. An empty query matches nothing here —
/// the empty-query palette already lists every action in its quick row.
fn filter_actions(query: &str, text: &ShellText) -> Vec<SearchQuickAction> {
    if query.is_empty() {
        return Vec::new();
    }
    QUICK_ACTIONS
        .iter()
        .copied()
        .filter(|action| query_matches(query, action.label(text)))
        .collect()
}

/// Indices of rows whose name or description match the query, in source
/// order. Shared by the skill and agent sections.
fn filter_entities<T>(
    query: &str,
    rows: &[T],
    name: impl Fn(&T) -> &str,
    description: impl Fn(&T) -> &str,
) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            query_matches(query, name(row)) || query_matches(query, description(row))
        })
        .map(|(index, _)| index)
        .collect()
}

/// How many hits a section shows and how many it holds back.
fn entity_display_count(hit_count: usize) -> (usize, usize) {
    (
        hit_count.min(ENTITY_RESULT_CAP),
        hit_count.saturating_sub(ENTITY_RESULT_CAP),
    )
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

/// Below the query row: quick actions while the query is empty, else the
/// federated sections (actions first, then sessions, entities, and file
/// content) with notices and the empty state.
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

    // G-Q1: matching actions stay reachable from the palette while a query is
    // up, mirroring the desktop catalog's command rows.
    let actions = filter_actions(&state.query, text);
    if !actions.is_empty() {
        body = body.child(section_header(&text.settings_quick_actions_section, cx));
        for action in actions {
            body = body.child(quick_action_row(view.clone(), action, text, cx));
        }
    }

    if !state.sessions.is_empty() {
        body = body.child(section_header(&text.search_sessions_tab, cx));
        for (index, hit) in state.sessions.iter().enumerate() {
            body = body.child(session_hit_row(view.clone(), index, hit, tx, cx));
        }
    }

    // Entity sections filter the in-memory catalogs at render time; a catalog
    // that has not loaded yet simply contributes no rows.
    if let Some(catalog) = cx.try_global::<crate::manage::SkillCatalog>() {
        let hits = filter_entities(
            &state.query,
            &catalog.skills,
            |row| &row.name,
            |row| &row.description,
        );
        let (shown, held_back) = entity_display_count(hits.len());
        if shown > 0 {
            body = body.child(section_header(&text.manage_skills_title, cx));
            for &index in &hits[..shown] {
                body = body.child(skill_hit_row(view.clone(), &catalog.skills[index], cx));
            }
        }
        if held_back > 0 {
            body = body.child(overflow_row(held_back, text, cx));
        }
    }
    if cx.has_global::<crate::manage::AgentCatalog>() {
        let catalog = cx.global::<crate::manage::AgentCatalog>();
        let rows: Vec<&crate::manage::AgentCardRow> = catalog
            .modes
            .iter()
            .chain(catalog.subagents.iter())
            .collect();
        let hits = filter_entities(&state.query, &rows, |row| &row.name, |row| &row.description);
        let (shown, held_back) = entity_display_count(hits.len());
        if shown > 0 {
            body = body.child(section_header(&text.manage_agents_title, cx));
            for &index in &hits[..shown] {
                body = body.child(agent_hit_row(view.clone(), rows[index], cx));
            }
        }
        if held_back > 0 {
            body = body.child(overflow_row(held_back, text, cx));
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
        && !has_entity_sections(state, text, cx)
    {
        body = body.child(div().px_3().py_4().child(crate::shell::brand_empty(
            IconName::Search,
            &text.search_empty_title,
            &text.search_empty,
        )));
    }
    body.into_any_element()
}

/// Whether any G-Q1 section (filtered actions, skills, agents) has rows for
/// the current query; part of the "nothing anywhere" empty-state check.
fn has_entity_sections(state: &SearchPageState, text: &ShellText, cx: &App) -> bool {
    if !filter_actions(&state.query, text).is_empty() {
        return true;
    }
    if let Some(catalog) = cx.try_global::<crate::manage::SkillCatalog>() {
        if !filter_entities(
            &state.query,
            &catalog.skills,
            |row| &row.name,
            |row| &row.description,
        )
        .is_empty()
        {
            return true;
        }
    }
    if cx.has_global::<crate::manage::AgentCatalog>() {
        let catalog = cx.global::<crate::manage::AgentCatalog>();
        let rows: Vec<&crate::manage::AgentCardRow> = catalog
            .modes
            .iter()
            .chain(catalog.subagents.iter())
            .collect();
        if !filter_entities(&state.query, &rows, |row| &row.name, |row| &row.description).is_empty()
        {
            return true;
        }
    }
    false
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

/// One matched skill (G-Q1): jumps to the skills settings scene.
fn skill_hit_row(
    view: WeakEntity<ShellWindow>,
    row: &crate::manage::SkillRow,
    cx: &App,
) -> gpui_kit::AnyElement {
    entity_hit_row(
        view,
        format!("search-skill-{}", row.key),
        IconName::Sparkles,
        row.name.clone(),
        row.description.clone(),
        crate::settings::SettingsEntry::Skills,
        cx,
    )
}

/// One matched mode or subagent (G-Q1): jumps to the agents settings scene.
fn agent_hit_row(
    view: WeakEntity<ShellWindow>,
    row: &crate::manage::AgentCardRow,
    cx: &App,
) -> gpui_kit::AnyElement {
    entity_hit_row(
        view,
        format!("search-agent-{}", row.id),
        IconName::Bot,
        row.name.clone(),
        row.description.clone(),
        crate::settings::SettingsEntry::Agents,
        cx,
    )
}

/// Shared entity row shape: icon, name, muted description, click jumps to the
/// owning settings scene and closes the overlay.
#[allow(clippy::too_many_arguments)]
fn entity_hit_row(
    view: WeakEntity<ShellWindow>,
    id: String,
    icon: IconName,
    name: String,
    description: String,
    entry: crate::settings::SettingsEntry,
    cx: &App,
) -> gpui_kit::AnyElement {
    div()
        .id(SharedString::from(id))
        .cursor_pointer()
        .v_flex()
        .gap_0p5()
        .px_3()
        .py_1p5()
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .on_click(move |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |shell, cx| {
                    shell.open_settings_entry(entry, window, cx);
                });
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_x_2()
                .text_sm()
                .text_color(cx.theme().popover_foreground)
                .child(Icon::new(icon))
                .child(div().truncate().child(name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(description),
        )
        .into_any_element()
}

/// The per-section hold-back notice when a section shows only its first cap
/// of hits.
fn overflow_row(held_back: usize, text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    div()
        .px_3()
        .py_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from(format!(
            "+{} · {}",
            held_back, text.search_truncated
        )))
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

    #[test]
    fn query_matches_fold_case_but_keep_cjk() {
        assert!(query_matches("TER", "Terminal"));
        assert!(query_matches("term", "Open TERMINAL here"));
        assert!(query_matches("技能", "技能市场"));
        assert!(!query_matches("term", "Session"));
    }

    #[test]
    fn filter_actions_matches_labels_only() {
        let text = crate::i18n::fallback_text(Default::default());
        let hits = filter_actions("term", &text);
        assert_eq!(hits, vec![SearchQuickAction::Terminal]);

        let none = filter_actions("", &text);
        assert!(
            none.is_empty(),
            "an empty query matches nothing: the quick row lists all"
        );
    }

    #[test]
    fn filter_entities_match_name_or_description_in_order() {
        let rows = [
            ("alpha", "first tool"),
            ("beta", "runs TERMINAL jobs"),
            ("gamma", ""),
        ];
        let hits = filter_entities("term", &rows, |row| row.0, |row| row.1);
        assert_eq!(hits, vec![1], "only the description match, in source order");

        let by_name = filter_entities("ALPH", &rows, |row| row.0, |row| row.1);
        assert_eq!(by_name, vec![0]);

        let none = filter_entities("zed", &rows, |row| row.0, |row| row.1);
        assert!(none.is_empty());
    }

    #[test]
    fn entity_display_count_caps_and_reports_holdback() {
        assert_eq!(entity_display_count(3), (3, 0));
        assert_eq!(
            entity_display_count(ENTITY_RESULT_CAP),
            (ENTITY_RESULT_CAP, 0)
        );
        let (shown, held) = entity_display_count(ENTITY_RESULT_CAP + 5);
        assert_eq!(shown, ENTITY_RESULT_CAP);
        assert_eq!(held, 5);
    }

    #[test]
    fn skill_rows_filter_by_name_and_description() {
        let rows = vec![
            crate::manage::SkillRow {
                key: "user::reviewer".to_string(),
                name: "Reviewer".to_string(),
                description: "Reviews patches".to_string(),
                project_level: false,
                builtin: false,
                shadowed: false,
                disabled: false,
                group_key: None,
                source_id: String::new(),
                source_label: String::new(),
                path: "/skills/reviewer".to_string(),
                deletable: true,
            },
            crate::manage::SkillRow {
                key: "user::scribe".to_string(),
                name: "Scribe".to_string(),
                description: "Writes TERMINAL logs".to_string(),
                project_level: false,
                builtin: false,
                shadowed: false,
                disabled: false,
                group_key: None,
                source_id: String::new(),
                source_label: String::new(),
                path: "/skills/scribe".to_string(),
                deletable: true,
            },
        ];
        let hits = filter_entities("term", &rows, |row| &row.name, |row| &row.description);
        assert_eq!(hits, vec![1]);
    }
}
