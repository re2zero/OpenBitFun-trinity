// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// Sidebar navigation destinations (R1b): the desktop top-scene rows in
/// NavPanel order. Files and git stay workbench panels opened through the
/// global search palette's quick actions; E4 batch ① re-homes only insights —
/// the report page is a standalone scene like the task board, so it gets the
/// fifth nav row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NavAction {
    Chat,
    Tasks,
    MiniApps,
    Extensions,
    Insights,
}

impl NavAction {
    pub(super) fn icon(self) -> IconName {
        match self {
            NavAction::Chat => IconName::Sparkles,
            NavAction::Tasks => IconName::Calendar,
            NavAction::MiniApps => IconName::LayoutDashboard,
            NavAction::Extensions => IconName::Puzzle,
            NavAction::Insights => IconName::ChartPie,
        }
    }

    /// Whether the row is the active surface. Sessions resolve through the
    /// tab bar (any open session tab makes Chat the active nav row); a scene
    /// row is active while its scene tab is the active tab.
    pub(super) fn is_active(self, tabs: &TabsState) -> bool {
        match self {
            NavAction::Chat => tabs
                .active_kind()
                .is_none_or(|kind| matches!(kind, TabKind::Session(_))),
            NavAction::Tasks => tabs.active_kind() == Some(&TabKind::Scene(SceneId::Tasks)),
            NavAction::MiniApps => tabs.active_kind() == Some(&TabKind::Scene(SceneId::MiniApps)),
            NavAction::Extensions => {
                tabs.active_kind() == Some(&TabKind::Scene(SceneId::Extensions))
            }
            NavAction::Insights => tabs.active_kind() == Some(&TabKind::Scene(SceneId::Insights)),
        }
    }
}

/// Branded empty state (M23): the component `Empty` layout with an icon in a
/// muted frame over a title and description. Shared by the chat, files, and
/// search surfaces so the treatment stays identical; theme resolution happens
/// at render time inside the component.
pub(crate) fn brand_empty(icon: IconName, title: &str, description: &str) -> gpui_kit::AnyElement {
    Empty::new()
        .header(
            EmptyHeader::new()
                .media(
                    EmptyMedia::new()
                        .with_variant(EmptyMediaVariant::Icon)
                        .child(Icon::new(icon)),
                )
                .title(EmptyTitle::new().child(title.to_string()))
                .description(EmptyDescription::new().child(description.to_string())),
        )
        .into_any_element()
}

/// Scene-level empty state (E4 batch ①): the branded empty laid on a quiet
/// card centered in the scene canvas (design components.html §空态), with an
/// optional action lane under the copy. The action must point at a real
/// surface — a scene with no honest destination renders without one rather
/// than faking a link.
pub(crate) fn brand_empty_card(
    icon: IconName,
    title: &str,
    description: &str,
    action: Option<gpui_kit::AnyElement>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(brand_empty(icon, title, description));
    if let Some(action) = action {
        body = body.child(action);
    }
    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .child(
            div()
                .debug_selector(|| "brand-empty-card".to_string())
                .w_full()
                .max_w(px(440.))
                .flex()
                .flex_col()
                .items_center()
                .bg(cx.theme().popover)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_2xl())
                .px_4()
                .py(px(40.))
                .child(body),
        )
        .into_any_element()
}

/// All nav rows in display order (desktop NavPanel top-scene order); a pure
/// projection so tests can pin the active-state wiring without a window.
pub(super) fn nav_actions() -> Vec<NavAction> {
    vec![
        NavAction::Chat,
        NavAction::Tasks,
        NavAction::MiniApps,
        NavAction::Extensions,
        NavAction::Insights,
    ]
}

/// Row label for a nav destination: the desktop nav nouns (智能助理/认知体/
/// 任务看板/小应用/扩展与兼容/洞察), not the scene titles the tabs show.
pub(super) fn nav_label<'a>(action: NavAction, text: &'a ShellText) -> &'a str {
    match action {
        NavAction::Chat => &text.nav_assistant,
        NavAction::Tasks => &text.nav_tasks,
        NavAction::MiniApps => &text.miniapps_title,
        NavAction::Extensions => &text.extensions_title,
        NavAction::Insights => &text.insights_title,
    }
}

/// One `Sidebar` content entry: a session row (root or indented child, R1c),
/// a navigation row, the "show more" fold row, the empty-state line, or the
/// list error line. One type because `Sidebar` children share one element
/// type; collapsed-state is inert because entries are always rendered.
#[derive(Clone)]
pub(super) enum SidebarEntry {
    Session {
        row: SessionRow,
        open: bool,
        /// The shell's single active row (web `is-active`): true only while
        /// this session is the active tab, so exactly one row highlights no
        /// matter how many session tabs are open.
        active: bool,
        /// Tree depth (R1c): 0 = workspace top-level session, 1 = subagent
        /// child indented under its root. Deeper lineage descendants stay
        /// hidden, matching the web nav's two-level projection.
        depth: u8,
        /// A turn is executing for this session (process-wide live set, so
        /// background sessions light up too).
        running: bool,
        /// The workspace root this row belongs to: `None` = the active
        /// workspace, `Some(root)` = another workspace's group. Opening a
        /// foreign row switches the workspace first (web `openMainSession`
        /// activates the session's owning workspace), because the runtime
        /// restores a transcript from the *active* workspace's store.
        group: Option<String>,
        /// The owning workspace's name while the flat projection is on (web
        /// flat rows carry their `workspaceScopes` name); `None` in the
        /// grouped tree, where the group header above the row says it.
        scope: Option<SharedString>,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
        /// The shell view, so the row menu can drive dialogs and actions
        /// (row rendering runs outside the view's own context).
        view: WeakEntity<ShellWindow>,
    },
    /// The workspace group row (R1c): chevron fold, workspace name, and the
    /// running dot when any visible session has a live turn. It is an entry,
    /// not a section, so a whole group — header, rows, folds — is ONE
    /// `Sidebar` child and the child count never changes with a fold.
    GroupHeader {
        name: SharedString,
        collapsed: bool,
        running: bool,
        /// Fold key: `None` = the active workspace, `Some(root_path)` = one
        /// of the other-workspace groups.
        group: Option<String>,
        view: WeakEntity<ShellWindow>,
    },
    Nav {
        action: NavAction,
        label: SharedString,
        active: bool,
        view: WeakEntity<ShellWindow>,
    },
    More {
        /// Hidden root sessions the expansion would reveal.
        remaining: usize,
        expanded: bool,
        /// Which group the fold toggles: `None` = current workspace,
        /// `Some(root_path)` = another workspace group.
        group: Option<String>,
        view: WeakEntity<ShellWindow>,
    },
    /// The desktop group-tail collapse row (收起): folds the whole group back
    /// to its header line.
    Collapse {
        /// Same group key as [`SidebarEntry::More`].
        group: Option<String>,
        view: WeakEntity<ShellWindow>,
    },
    Empty(SharedString),
    Danger(SharedString),
}

impl Collapsible for SidebarEntry {
    fn collapsed(self, _collapsed: bool) -> Self {
        self
    }

    fn is_collapsed(&self) -> bool {
        false
    }
}

impl SidebarItem for SidebarEntry {
    fn render(
        self,
        id: impl Into<ElementId>,
        _window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let _id = id.into();
        match self {
            SidebarEntry::Session {
                row,
                open,
                active,
                depth,
                running,
                group,
                scope,
                tx,
                view,
            } => {
                let SessionRow {
                    id,
                    name,
                    metadata,
                    archived,
                    ..
                } = row;
                let open_id = id.clone();
                let open_group = group.clone();
                let open_view = view.clone();
                let menu = session_row_menu(
                    id.clone(),
                    name.clone(),
                    archived,
                    open,
                    view.clone(),
                    tx.clone(),
                );
                // Background subagent activity (T2-6, the web
                // backgroundSubagentActivity + review badge pair): a count
                // pill while hidden subagent turns run, and the review label
                // when one of them is a review-family run.
                let (background_count, review_kind) = view
                    .upgrade()
                    .map(|shell| shell.read(cx).session_activity_badges(&id))
                    .unwrap_or((0, None));
                let text = i18n::current(cx);
                let activity_badges = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .flex_shrink_0()
                    .when_some(review_kind, |this, _kind| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().accent)
                                .child(text.sidebar_review_running.clone()),
                        )
                    })
                    .when(background_count > 0, |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_0p5()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(Icon::new(IconName::Bot))
                                .child(background_count.to_string()),
                        )
                    });
                div()
                    .id(SharedString::from(format!("session-{id}")))
                    .on_click(move |_, window, cx| {
                        // The click lands on the window (not on a bare
                        // runtime call) so a foreign-group row can switch the
                        // workspace and open afterwards.
                        if let Some(view) = open_view.upgrade() {
                            let session_id = open_id.clone();
                            let group = open_group.clone();
                            view.update(cx, |this, cx| {
                                this.open_session_row(session_id, group, window, cx);
                            });
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .px_2()
                    .h(px(30.))
                    // Subagent children (R1c) indent one step under their
                    // root; the web nav nests one level the same way.
                    .when(depth > 0, |this| this.ml_4())
                    .rounded(cx.theme().radius)
                    // Web SessionsSection final layer: hover lights the
                    // neutral surface, the ACTIVE row the selection surface —
                    // two different tokens, and only one active row per
                    // window, so a hovered row beside a merely-open tab never
                    // reads as two selections.
                    .when(active, |this| this.bg(cx.theme().selection))
                    .when(!active, |this| {
                        this.hover(|style| style.bg(cx.theme().muted))
                    })
                    // While the flat projection is on, each root row says
                    // which workspace it belongs to (web flat rows render the
                    // same name inline), because no group header is above it.
                    .when_some(scope, |this, scope| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .max_w(px(96.))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(scope),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(cx.theme().sidebar_foreground)
                            .truncate()
                            .when(archived, |this| this.opacity(0.6))
                            .child(name),
                    )
                    // Subagent children keep their kind as an inline badge
                    // (the web row's StatusPill); top-level workspace meta
                    // stays off the row — the desktop list is single-line.
                    .when(depth > 0 && !metadata.is_empty(), |this| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .px_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(metadata),
                        )
                    })
                    .child(activity_badges)
                    .when(running, |this| {
                        this.child(
                            div()
                                .size_1p5()
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(cx.theme().success),
                        )
                    })
                    .child(menu)
                    .into_any_element()
            }
            SidebarEntry::GroupHeader {
                name,
                collapsed,
                running,
                group,
                view,
            } => {
                let text = i18n::current(cx);
                let new_view = view.clone();
                let new_group = group.clone();
                let menu = workspace_group_menu(group.clone(), view.clone());
                let fold_view = view.clone();
                let fold_group = group.clone();
                div()
                    .id(SharedString::from(format!(
                        "session-group-{}",
                        group.clone().unwrap_or_else(|| "current".to_string())
                    )))
                    .on_click(move |_, _, cx| {
                        if let Some(view) = fold_view.upgrade() {
                            view.update(cx, |this, cx| {
                                this.toggle_session_group(fold_group.as_deref(), cx);
                            });
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .hover(|style| style.bg(cx.theme().muted))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .small(),
                    )
                    // The desktop workspace group carries a folder glyph (the
                    // person icon is the standalone-agent group, which the shell
                    // has no data for).
                    .child(Icon::new(IconName::Folder).small())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_medium()
                            .truncate()
                            .child(name),
                    )
                    .when(running, |this| {
                        this.child(
                            div()
                                .size_1p5()
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(cx.theme().success),
                        )
                    })
                    // Group actions (web `WorkspaceItem`'s hover cluster: + new
                    // session, folder, ⋯). The web reveals them on hover; the
                    // shell keeps them visible like the session rows' menu button
                    // (this gpui has no parent-hover-gated children). Button
                    // presses stop propagation, so neither triggers the fold.
                    .child(
                        Button::new(SharedString::from(format!(
                            "session-group-new-{}",
                            group.clone().unwrap_or_else(|| "current".to_string())
                        )))
                        .icon(IconName::Plus)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.sessions_new.clone())
                        .on_click(move |_, window, cx| {
                            // The header's own click folds the group. A plain
                            // Button does not stop the click (only an open
                            // dropdown trigger does), so the action stops it
                            // here — otherwise every new session would fold
                            // the group it was started from.
                            cx.stop_propagation();
                            if let Some(view) = new_view.upgrade() {
                                let group = new_group.clone();
                                view.update(cx, |this, cx| {
                                    this.run_group_action(
                                        group,
                                        PendingGroupAction::NewSession,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }),
                    )
                    .child(menu)
                    .into_any_element()
            }
            SidebarEntry::Nav {
                action,
                label,
                active,
                view,
            } => {
                let id = SharedString::from(format!("nav-{:?}", action));
                div()
                    .id(id)
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| this.apply_nav(action, cx));
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_x_2()
                    .min_w_0()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .when(active, |this| {
                        this.font_medium()
                            .bg(cx.theme().selection)
                            .text_color(cx.theme().sidebar_accent_foreground)
                    })
                    .when(!active, |this| {
                        this.hover(|style| style.bg(cx.theme().muted))
                    })
                    .child(Icon::new(action.icon()))
                    .child(div().truncate().child(label))
                    .into_any_element()
            }
            SidebarEntry::More {
                remaining,
                expanded,
                group,
                view,
            } => {
                let text = i18n::current(cx);
                let label = if expanded {
                    text.sessions_show_less.clone()
                } else {
                    text.sessions_show_more.clone()
                };
                let chevron = if expanded {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                };
                div()
                    .id(SharedString::from(format!(
                        "sessions-show-more-{}",
                        group.clone().unwrap_or_else(|| "current".to_string())
                    )))
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| {
                                match &group {
                                    Some(path) => {
                                        if let Some(group) = this
                                            .other_groups
                                            .iter_mut()
                                            .find(|group| group.root_path == *path)
                                        {
                                            group.expanded = !group.expanded;
                                        }
                                    }
                                    None => this.sessions_expanded = !this.sessions_expanded,
                                }
                                cx.notify();
                            });
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|style| style.bg(cx.theme().muted))
                    .child(Icon::new(chevron).small())
                    .child(label)
                    .when(!expanded && remaining > 0, |this| {
                        this.child(
                            div()
                                .px_1()
                                .text_xs()
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded(cx.theme().radius_tokens().sm)
                                .child(format!("+{remaining}")),
                        )
                    })
                    .into_any_element()
            }
            SidebarEntry::Collapse { group, view } => {
                let text = i18n::current(cx);
                div()
                    .id(SharedString::from(format!(
                        "sessions-collapse-{}",
                        group.clone().unwrap_or_else(|| "current".to_string())
                    )))
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| {
                                match &group {
                                    Some(path) => {
                                        if let Some(group) = this
                                            .other_groups
                                            .iter_mut()
                                            .find(|group| group.root_path == *path)
                                        {
                                            group.collapsed = true;
                                        }
                                    }
                                    None => this.sessions_group_collapsed = true,
                                }
                                cx.notify();
                            });
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|style| style.bg(cx.theme().muted))
                    .child(text.sessions_collapse.clone())
                    .child(Icon::new(IconName::ChevronUp).small())
                    .into_any_element()
            }
            SidebarEntry::Empty(text) => div()
                .flex()
                .flex_row()
                .items_start()
                .gap_x_2()
                .p_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Inbox))
                .child(text)
                .into_any_element(),
            SidebarEntry::Danger(text) => div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(text)
                .into_any_element(),
        }
    }
}

/// One `Sidebar` child. A workspace group is ONE child holding its header,
/// its session rows, and its fold pair, so folding a group never changes how
/// many children the sidebar has — `Sidebar`'s internal `list` resets (and
/// jumps to the top) whenever the child count changes. Only the number of
/// workspace groups (and the error/empty line) moves the count. One type
/// because `Sidebar` children share one element type. Nothing collapses at
/// this level: the component re-applies its own icon-rail state each frame.
#[derive(Clone)]
pub(super) enum SidebarSection {
    Rows(Vec<SidebarEntry>),
    Item(SidebarEntry),
}

impl Collapsible for SidebarSection {
    fn collapsed(self, _collapsed: bool) -> Self {
        // The sidebar re-applies its own icon-rail state to every group on
        // each frame (`Sidebar` render calls `.collapsed(layout
        // .icon_collapsed)`); every child is always-rendered.
        self
    }

    fn is_collapsed(&self) -> bool {
        false
    }
}

impl SidebarItem for SidebarSection {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        match self {
            SidebarSection::Rows(rows) => {
                let id = id.into();
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows.into_iter().enumerate().map(|(ix, row)| {
                        let row_id = SharedString::from(format!("{}-{}", id, ix));
                        row.render(row_id, window, cx).into_any_element()
                    }))
                    .into_any_element()
            }
            SidebarSection::Item(entry) => entry.render(id, window, cx).into_any_element(),
        }
    }
}

/// The sidebar component's element id.
pub(super) const SIDEBAR_ID: &str = "shell-sidebar";
