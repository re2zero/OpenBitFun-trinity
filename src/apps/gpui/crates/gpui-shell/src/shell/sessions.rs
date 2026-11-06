// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// One session list entry, composed (including localized metadata) when the
/// snapshot arrives so the render path never touches the i18n service.
#[derive(Clone)]
pub(crate) struct SessionRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(super) metadata: String,
    /// Persisted archive state (from the session metadata store, not a local
    /// flag): archived rows are hidden until the "show archived" toggle is on.
    pub(crate) archived: bool,
    /// Ordering keys for the sessions list projection (the web
    /// `compareWorkspaceNavSessions` reads the same two timestamps off the
    /// session record). Lineage children carry 0: they render under their
    /// parent, never in the ordering.
    pub(crate) created_at_ms: u64,
    pub(crate) last_active_at_ms: u64,
}

/// One other-workspace session group in the sidebar (web SessionsSection
/// renders every workspace's sessions as its own group). Populated by one
/// list request per recent workspace; the current workspace's rows stay in
/// [`ShellWindow::sessions`].
pub(super) struct OtherWorkspaceGroup {
    pub(super) name: String,
    pub(super) root_path: String,
    pub(super) rows: Vec<SessionRow>,
    pub(super) collapsed: bool,
    /// Per-group show-more fold (each group folds to the same level-0 page).
    pub(super) expanded: bool,
}

/// One workspace group on the settings archived page (G-SE30, web
/// `ArchivedSessionsConfig`): a collapsible header (label + count) over the
/// workspace's archived rows. Only workspaces with archived sessions get a
/// group; a workspace whose read failed contributes a failure line instead.
#[derive(Clone)]
pub(crate) struct ArchivedGroupView {
    /// The owning workspace, for the per-workspace restore/delete calls.
    pub(crate) workspace_id: String,
    /// The fold key and action target: the workspace root path.
    pub(crate) root_path: String,
    /// Header text, `name · path` (or just the path when the name is it).
    pub(crate) label: String,
    /// Newest first (the web sorts every collected entry by `lastActiveAt`).
    pub(crate) rows: Vec<ArchivedRowView>,
}

/// One archived session row on that page.
#[derive(Clone)]
pub(crate) struct ArchivedRowView {
    pub(crate) session_id: String,
    pub(crate) name: String,
    /// Unix ms; rendered through [`runtime::format_last_active`].
    pub(crate) last_active_at: u64,
}

/// The sessions section header row: the "Sessions" label plus the action
/// cluster (view toggle, filter menu, new session, archive filter, reload,
/// workspace menu; web MainNav's SectionHeader order). It lives in the
/// sidebar's fixed header slot — see `render_sidebar` for why the shell pins
/// it instead of the web's CSS-sticky row.
pub(super) fn session_section_header_row(
    id_prefix: SharedString,
    label: SharedString,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    archived_on: bool,
    // The list projection state is passed in rather than read back off the
    // window: the row is now rendered eagerly inside the shell's own render
    // pass, where reading the `ShellWindow` entity would panic (gpui forbids
    // read-while-updated).
    session_list: SessionListFilters,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    // Session-header actions (R1b): the three desktop header verbs the shell
    // has real semantics for — new session, archive filter, list reload — plus
    // the workspace menu (R1c*) whose first two entries are 打开项目 /
    // 新建项目. The view toggle and filter menu lead the cluster, the order
    // the web section header uses (GroupingToggle, FilterMenu, then the
    // workspace menu trigger).
    let id = id_prefix;
    let new_tx = tx.clone();
    let reload_tx = tx.clone();
    let archive_view = view.clone();
    let add_project_view = view.clone();
    let toggle_view = view.clone();
    let filter_view = view.clone();
    let text = i18n::current(cx);
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_1()
        .pt_1()
        .h_8()
        .px_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().sidebar_foreground.opacity(0.7))
                .child(label),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_0p5()
                // Grouping toggle (web WorkspaceSessionGroupingToggle):
                // grouped tree ↔ one flat list of every workspace.
                .child(
                    Button::new(SharedString::from(format!("{}-view-toggle", id)))
                        .icon(if session_list.flat {
                            IconName::List
                        } else {
                            IconName::ListTree
                        })
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(if session_list.flat {
                            text.sessions_view_grouped_tooltip.clone()
                        } else {
                            text.sessions_view_all_tooltip.clone()
                        })
                        .on_click(move |_, _, cx| {
                            if let Some(view) = toggle_view.upgrade() {
                                view.update(cx, |this, cx| {
                                    this.session_list.flat = !this.session_list.flat;
                                    cx.notify();
                                });
                            }
                        }),
                )
                // Filter menu (web WorkspaceSessionFilterMenu), the
                // button marked while a filter narrows the list.
                .child(
                    Button::new(SharedString::from(format!("{}-filter", id)))
                        .icon(IconName::Funnel)
                        .small()
                        .with_variant(if session_list.is_customized() {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .tooltip(text.sessions_filter_tooltip.clone())
                        .dropdown_menu_with_anchor(Anchor::BottomRight, {
                            let view = filter_view.clone();
                            move |menu, _window, cx| {
                                session_filter_menu_into(menu, view.clone(), cx)
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("{}-new", id)))
                        .icon(IconName::Plus)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(i18n::current(cx).sessions_new.clone())
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.create_session(new_tx.clone());
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("{}-archived", id)))
                        .icon(IconName::Archive)
                        .small()
                        .with_variant(if archived_on {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .tooltip(i18n::current(cx).sessions_show_archived.clone())
                        .on_click(move |_, _, cx| {
                            if let Some(view) = archive_view.upgrade() {
                                view.update(cx, |this, cx| {
                                    this.show_archived = !this.show_archived;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("{}-reload", id)))
                        .icon(IconName::RefreshCw)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(i18n::current(cx).sessions_reload.clone())
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.reload_sessions(reload_tx.clone());
                            }
                        }),
                )
                // The workspace menu trigger (web MainNav: the
                // sessions header's last action opens 打开项目 /
                // 新建项目 / the recent workspace list). Same
                // builder as the nav top bar and composer pill.
                .child(
                    Button::new(SharedString::from(format!("{}-add-project", id)))
                        .icon(IconName::FolderPlus)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(i18n::current(cx).sessions_add_group.clone())
                        .dropdown_menu_with_anchor(Anchor::BottomRight, {
                            let view = add_project_view.clone();
                            move |menu, _window, cx| workspace_menu_into(menu, view.clone(), cx)
                        }),
                ),
        )
        .into_any_element()
}

/// Per-row actions menu: rename, fork, compact (open session only), export,
/// archive toggle, delete. A Button (whose press stops propagation) keeps the
/// row's own click-to-open handler from firing when the menu trigger is used.
pub(super) fn session_row_menu(
    session_id: String,
    name: String,
    archived: bool,
    open: bool,
    view: WeakEntity<ShellWindow>,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) -> gpui_kit::AnyElement {
    let rename_action = {
        let view = view.clone();
        let session_id = session_id.clone();
        let name = name.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            open_rename_dialog(view.clone(), session_id.clone(), name.clone(), window, cx);
        }
    };
    let fork_action = {
        let session_id = session_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.fork_session(session_id.clone(), tx.clone());
            }
        }
    };
    let compact_action = {
        let session_id = session_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.start_compaction(session_id.clone(), tx.clone());
            }
        }
    };
    let export_action = {
        let view = view.clone();
        let session_id = session_id.clone();
        let name = name.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            open_export_dialog(view.clone(), session_id.clone(), name.clone(), window, cx);
        }
    };
    let archive_action = {
        let session_id = session_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.set_session_archived(session_id.clone(), !archived, tx.clone());
            }
        }
    };
    let delete_action = {
        let view = view.clone();
        let session_id = session_id.clone();
        let name = name.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            open_delete_dialog(view.clone(), session_id.clone(), name.clone(), window, cx);
        }
    };
    Button::new(SharedString::from(format!("session-menu-{session_id}")))
        .icon(IconName::Ellipsis)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
            let text = i18n::current(cx);
            let archive_label = if archived {
                text.session_unarchive.clone()
            } else {
                text.session_archive.clone()
            };
            let menu = menu
                .item(
                    PopupMenuItem::new(text.session_rename.clone()).on_click(rename_action.clone()),
                )
                .item(PopupMenuItem::new(text.session_fork.clone()).on_click(fork_action.clone()));
            let menu = if open {
                menu.item(
                    PopupMenuItem::new(text.session_compact.clone())
                        .on_click(compact_action.clone()),
                )
            } else {
                menu
            };
            menu.item(
                PopupMenuItem::new(text.session_export.clone()).on_click(export_action.clone()),
            )
            .item(PopupMenuItem::new(archive_label).on_click(archive_action.clone()))
            .item(PopupMenuItem::new(text.session_delete.clone()).on_click(delete_action.clone()))
        })
        .into_any_element()
}

/// The workspace-group header's menu (web `WorkspaceItem`'s ⋯ popover, honest
/// subset): new session, the workspace's Files panel, copy path, reveal in the
/// file manager. Web's scheduled-jobs, project-permissions, port-forwarding,
/// batch session management and assistant reset/delete entries have no shell
/// surface and stay out — the parity doc records each as a deliberate
/// divergence. Both this menu and the header's ＋ follow the same
/// switch-then-act rule for another workspace's group.
pub(super) fn workspace_group_menu(
    group: Option<String>,
    view: WeakEntity<ShellWindow>,
) -> gpui_kit::AnyElement {
    let key = group.clone().unwrap_or_else(|| "current".to_string());
    let new_view = view.clone();
    let new_group = group.clone();
    let files_view = view.clone();
    let files_group = group.clone();
    let copy_view = view.clone();
    let copy_group = group.clone();
    let reveal_view = view.clone();
    let reveal_group = group.clone();
    Button::new(SharedString::from(format!("session-group-menu-{key}")))
        .icon(IconName::Ellipsis)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
            let text = i18n::current(cx);
            // The menu builder runs as `Fn`: each handler gets its own owned
            // clone of what it needs, never the closure's capture itself.
            let new_view = new_view.clone();
            let new_group = new_group.clone();
            let files_view = files_view.clone();
            let files_group = files_group.clone();
            let copy_view = copy_view.clone();
            let copy_group = copy_group.clone();
            let reveal_view = reveal_view.clone();
            let reveal_group = reveal_group.clone();
            menu.item(PopupMenuItem::new(text.sessions_new.clone()).on_click(
                move |_, window, cx| {
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
                },
            ))
            .item(
                PopupMenuItem::new(text.files_title.clone()).on_click(move |_, window, cx| {
                    if let Some(view) = files_view.upgrade() {
                        let group = files_group.clone();
                        view.update(cx, |this, cx| {
                            this.run_group_action(group, PendingGroupAction::OpenFiles, window, cx);
                        });
                    }
                }),
            )
            .separator()
            .item(
                PopupMenuItem::new(text.sessions_copy_path.clone()).on_click(move |_, _, cx| {
                    if let Some(view) = copy_view.upgrade() {
                        view.update(cx, |this, cx| {
                            this.copy_group_path(copy_group.as_deref(), cx)
                        });
                    }
                }),
            )
            .item(
                PopupMenuItem::new(text.sessions_reveal.clone()).on_click(move |_, _, cx| {
                    if let Some(view) = reveal_view.upgrade() {
                        view.update(cx, |this, _cx| {
                            this.reveal_group_path(reveal_group.as_deref())
                        });
                    }
                }),
            )
        })
        .into_any_element()
}

impl ShellWindow {
    /// The workspace switcher: a footer button whose menu lists this
    /// process's workspaces plus the shared core catalog, and an "open by
    /// path" entry that registers a record without touching the desktop's
    /// current/recent selection. The builder re-reads the list every frame, so
    /// a switch or a failure is visible without reopening the menu.
    pub(super) fn render_workspace_button(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity().downgrade();
        Button::new("footer-workspace")
            .icon(IconName::FolderOpen)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.workspace_switch.clone())
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _window, cx| {
                workspace_menu_into(menu, view.clone(), cx)
            })
    }

    /// The composer strip's workspace pill (web `ChatInputWorkspaceStrip`
    /// left rail): the workspace name opens the same recent-workspace menu as
    /// the chrome button, so switching stays reachable where the session
    /// names it (the web pill is clickable too).
    pub(super) fn render_workspace_pill(
        &self,
        label: String,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let view = cx.entity().downgrade();
        Button::new("composer-workspace-pill")
            .label(label)
            .xsmall()
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(cx.theme().muted)
                    .foreground(cx.theme().muted_foreground),
            )
            .rounded(ButtonRounded::Size(px(999.)))
            .tooltip(text.workspace_switch.clone())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
                workspace_menu_into(menu, view.clone(), cx)
            })
            .into_any_element()
    }
}

impl ShellWindow {
    /// The sidebar activity badges for one session row (T2-6, the web
    /// backgroundSubagentActivity and review badge pair): how many linked
    /// subagent turns are running behind the scenes, and whether a
    /// review-family subagent is among them. A subagent counts when its task
    /// tool started with `run_in_background`, or — for links whose `Started`
    /// event predates this process — when it has outlived the parent's turn.
    pub(super) fn session_activity_badges(
        &self,
        parent_session_id: &str,
    ) -> (usize, Option<ReviewActivityKind>) {
        let parent_live = self.live_sessions.contains(parent_session_id);
        let mut count = 0usize;
        let mut review = None;
        for link in self
            .subagent_links
            .iter()
            .filter(|link| link.parent_session_id == parent_session_id)
        {
            if !self.live_sessions.contains(&link.subagent_session_id) {
                continue;
            }
            let tool_key = SharedString::from(format!("tool-{}", link.tool_call_id));
            if !self.background_task_tools.contains(&tool_key) && parent_live {
                continue;
            }
            count += 1;
            if review.is_none() {
                review = link.agent_type.as_deref().and_then(review_activity_kind);
            }
        }
        (count, review)
    }
}

impl ShellWindow {
    /// Reloads the archived settings page's projection (G-SE30). Fails silent
    /// with the log when the runtime or the workspace catalog is not up yet:
    /// the page's own empty state covers that case. A request that arrives
    /// while a read is in flight is coalesced into one re-read when it lands.
    pub(crate) fn reload_archived_groups(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = runtime::runtime_handle() else {
            return;
        };
        if self.workspaces.is_empty() {
            return;
        }
        if self.archived_loading {
            self.archived_reload_dirty = true;
            return;
        }
        self.archived_loading = true;
        handle.load_archived_sessions(self.workspaces.clone(), self.event_tx.clone());
        cx.notify();
    }

    /// Records the archived page's in-flight action and its success receipt;
    /// the previous receipt clears so the page always shows the latest
    /// outcome.
    pub(crate) fn begin_archived_action(&mut self, success: String, cx: &mut Context<Self>) {
        self.archived_pending = Some(success);
        settings::ArchivedNotice::set(None, cx);
        cx.notify();
    }

    /// Flips one archived group's fold on the settings page.
    pub(crate) fn toggle_archived_group(&mut self, root_path: &str, cx: &mut Context<Self>) {
        if let Some(position) = self
            .archived_collapsed
            .iter()
            .position(|root| root == root_path)
        {
            self.archived_collapsed.remove(position);
        } else {
            self.archived_collapsed.push(root_path.to_string());
        }
        cx.notify();
    }
}

impl ShellWindow {
    /// The sessions tree as sidebar children (R1c): ONE child per workspace
    /// group — its header row, the visible session rows with their subagent
    /// children, the show-more fold, and the group-tail collapse row all fold
    /// *inside* that child. The child count therefore tracks the number of
    /// workspace groups, not the rows under them: folding a group (or any row
    /// list change) leaves it alone, which is what keeps `Sidebar`'s internal
    /// list from resetting the scroll offset to the top. Archived rows show
    /// only while the toggle is on; a failed list surfaces as the error line.
    pub(super) fn render_session_entries(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> Vec<SidebarSection> {
        let mut sections = Vec::new();
        if let Some(error) = &self.list_error {
            sections.push(SidebarSection::Item(SidebarEntry::Danger(
                error.clone().into(),
            )));
            return sections;
        }

        // One section per workspace group: the current workspace first (its
        // fold state lives on the window), then every other recent workspace
        // that reported sessions (web SessionsSection renders the same
        // per-workspace groups side by side). The flat projection is the web
        // `layout="flat"` swap (grouping `all`): every workspace's sessions in
        // the first child, the other group slots left as empty children so the
        // child count — and with it the sidebar list's scroll anchor — is the
        // same in both views.
        let current_name = runtime::runtime_handle()
            .map(|handle| handle.workspace().name)
            .unwrap_or_default();
        if self.session_list.flat {
            sections.push(SidebarSection::Rows(self.flat_entries(&current_name, cx)));
            for _ in &self.other_groups {
                sections.push(SidebarSection::Rows(Vec::new()));
            }
            return sections;
        }
        sections.push(SidebarSection::Rows(self.group_entries(
            &current_name,
            None,
            &self.sessions,
            self.sessions_group_collapsed,
            self.sessions_expanded,
            cx,
        )));
        for group in &self.other_groups {
            sections.push(SidebarSection::Rows(self.group_entries(
                &group.name,
                Some(group.root_path.clone()),
                &group.rows,
                group.collapsed,
                group.expanded,
                cx,
            )));
        }
        if sections.is_empty() {
            sections.push(SidebarSection::Item(SidebarEntry::Empty(
                text.sessions_empty.clone().into(),
            )));
        }
        sections
    }
}

impl ShellWindow {
    /// The per-row facts the list projection reads (archive state, live turn,
    /// unread completion) — one place so the grouped and flat projections
    /// filter identically.
    pub(super) fn row_facts(&self, row: &SessionRow) -> SessionRowFacts {
        SessionRowFacts {
            archived: row.archived,
            running: self.session_is_running(&row.id),
            unread: self.unread_turns.contains_key(&row.id),
        }
    }

    /// One workspace's rows after the show/status filters, sorted by the
    /// active ordering (web `matchesWorkspaceSessionView` →
    /// `compareWorkspaceNavSessions`).
    pub(super) fn visible_rows<'a>(&self, rows: &'a [SessionRow]) -> Vec<&'a SessionRow> {
        let mut visible: Vec<&SessionRow> = rows
            .iter()
            .filter(|row| {
                session_row_matches(self.row_facts(row), &self.session_list, self.show_archived)
            })
            .collect();
        visible
            .sort_by(|left, right| compare_session_rows(left, right, self.session_list.ordering));
        visible
    }

    /// One workspace group's entries, in render order: the group header
    /// (folder glyph, fold chevron, running dot, its action pair), the
    /// single-line session rows with their lineage children, the show-more
    /// fold, and the desktop group-tail collapse row. `group` routes fold
    /// toggles and group actions: `None` mutates the window's
    /// current-workspace state, `Some(root_path)` the matching other group.
    pub(super) fn group_entries(
        &self,
        name: &str,
        group: Option<String>,
        rows: &[SessionRow],
        collapsed: bool,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Vec<SidebarEntry> {
        let view = cx.entity().downgrade();
        let visible = self.visible_rows(rows);
        let group_running = visible.iter().any(|row| self.session_is_running(&row.id));
        let mut entries = vec![SidebarEntry::GroupHeader {
            name: SharedString::from(name.to_string()),
            collapsed,
            running: group_running,
            group: group.clone(),
            view: view.clone(),
        }];
        if collapsed {
            return entries;
        }
        // A filter that hides every row says so instead of leaving a blank
        // group (web `nav.sessions.viewMenu.noMatches`); with no filter active
        // an empty group is just an empty workspace and stays silent.
        if visible.is_empty() && self.session_list.is_customized() {
            entries.push(SidebarEntry::Empty(
                i18n::current(cx).sessions_no_matches.clone().into(),
            ));
            entries.push(SidebarEntry::Collapse { group, view });
            return entries;
        }

        // Collapsed shows the web nav's level-0 page; expanding reveals the
        // rest.
        let foldable = visible.len() > SESSIONS_COLLAPSED_VISIBLE;
        let (shown, more): (Vec<&SessionRow>, usize) = if expanded || !foldable {
            (visible, 0)
        } else {
            (
                visible[..SESSIONS_COLLAPSED_VISIBLE].to_vec(),
                visible.len() - SESSIONS_COLLAPSED_VISIBLE,
            )
        };

        let mut push_row = |row: SessionRow, depth: u8| {
            let open = self.chats.iter().any(|chat| chat.matches(&row.id));
            let active = self.tabs.active_kind() == Some(&TabKind::Session(row.id.clone()));
            let running = self.session_is_running(&row.id);
            entries.push(SidebarEntry::Session {
                row,
                open,
                active,
                depth,
                running,
                group: group.clone(),
                scope: None,
                tx: self.event_tx.clone(),
                view: view.clone(),
            });
        };
        for row in shown {
            let children = self.lineage_children(&row.id);
            push_row(row.clone(), 0);
            for child in children {
                push_row(child, 1);
            }
        }
        // The fold row only exists when it can do something: reveal hidden
        // roots, or re-fold an expansion that actually hid something. The
        // collapse row closes the whole group (the desktop group tail).
        if more > 0 || (expanded && foldable) {
            entries.push(SidebarEntry::More {
                remaining: more,
                expanded,
                group: group.clone(),
                view: view.clone(),
            });
        }
        entries.push(SidebarEntry::Collapse { group, view });
        entries
    }

    /// The flat projection (web `layout="flat"` + `workspaceScopes`): every
    /// workspace's sessions in one list, ordered globally, each root row
    /// carrying its workspace name — the same swap the web makes for grouping
    /// `all`. The display cap does not apply (the web comment: the flat
    /// aggregate "bypasses this display cap and streams every page in"), and
    /// rows keep their group so opening a foreign one still switches the
    /// workspace first.
    pub(super) fn flat_entries(
        &self,
        current_name: &str,
        cx: &mut Context<Self>,
    ) -> Vec<SidebarEntry> {
        let view = cx.entity().downgrade();
        let mut rows: Vec<(Option<String>, SharedString, SessionRow)> = self
            .sessions
            .iter()
            .map(|row| {
                (
                    None,
                    SharedString::from(current_name.to_string()),
                    row.clone(),
                )
            })
            .collect();
        for group in &self.other_groups {
            for row in &group.rows {
                rows.push((
                    Some(group.root_path.clone()),
                    SharedString::from(group.name.clone()),
                    row.clone(),
                ));
            }
        }
        rows.retain(|(_, _, row)| {
            session_row_matches(self.row_facts(row), &self.session_list, self.show_archived)
        });
        rows.sort_by(|left, right| {
            compare_session_rows(&left.2, &right.2, self.session_list.ordering)
        });
        if rows.is_empty() {
            return vec![SidebarEntry::Empty(
                i18n::current(cx).sessions_no_matches.clone().into(),
            )];
        }
        let mut entries = Vec::new();
        let mut push_row =
            |row: SessionRow, depth: u8, group: Option<String>, scope: Option<SharedString>| {
                let open = self.chats.iter().any(|chat| chat.matches(&row.id));
                let active = self.tabs.active_kind() == Some(&TabKind::Session(row.id.clone()));
                let running = self.session_is_running(&row.id);
                entries.push(SidebarEntry::Session {
                    row,
                    open,
                    active,
                    depth,
                    running,
                    group,
                    // The web flat row shows its workspace name; subagent
                    // children inherit their parent's position instead.
                    scope: if depth == 0 { scope } else { None },
                    tx: self.event_tx.clone(),
                    view: view.clone(),
                });
            };
        for (group, workspace_name, row) in rows {
            let children = self.lineage_children(&row.id);
            push_row(row, 0, group.clone(), Some(workspace_name));
            for child in children {
                push_row(child, 1, group.clone(), None);
            }
        }
        entries
    }

    /// Flips one workspace group's fold. `None` is the active workspace (the
    /// window's own flag), `Some(root_path)` one of the other groups.
    pub(super) fn toggle_session_group(&mut self, group: Option<&str>, cx: &mut Context<Self>) {
        match group {
            Some(path) => {
                if let Some(group) = self
                    .other_groups
                    .iter_mut()
                    .find(|group| group.root_path == path)
                {
                    group.collapsed = !group.collapsed;
                }
            }
            None => self.sessions_group_collapsed = !self.sessions_group_collapsed,
        }
        cx.notify();
    }

    /// Runs one action from a sidebar workspace group. A group that is the
    /// active workspace acts directly; a group that belongs to another
    /// workspace switches there first and leaves the action pending
    /// ([`PendingGroupAction`], settled by [`RuntimeEvent::WorkspaceSwitched`])
    /// — the order the web UI uses (`openMainSession` / `WorkspaceItem`
    /// activate the owning workspace, then act), and the reason a transcript
    /// is restored from the *active* workspace's store.
    pub(super) fn run_group_action(
        &mut self,
        group: Option<String>,
        action: PendingGroupAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = runtime::runtime_handle() else {
            return;
        };
        let current_root = {
            let workspace = handle.workspace();
            workspace.root_path.to_string_lossy().to_string()
        };
        match workspace_group_plan(group.as_deref(), &current_root) {
            WorkspaceGroupPlan::Act => match action {
                PendingGroupAction::OpenSession(session_id) => {
                    handle.open_session(session_id, self.event_tx.clone())
                }
                PendingGroupAction::NewSession => handle.create_session(self.event_tx.clone()),
                PendingGroupAction::OpenFiles => self.toggle_files_view(window, cx),
            },
            WorkspaceGroupPlan::SwitchThenAct { root } => {
                tracing::info!("Group action for workspace {root}: switching before acting");
                self.pending_group_action = Some(action);
                self.session_open_error = None;
                handle.switch_workspace(std::path::PathBuf::from(root), self.event_tx.clone());
            }
        }
        cx.notify();
    }

    /// Opens one sidebar session row (the row click's group action).
    pub(super) fn open_session_row(
        &mut self,
        session_id: String,
        group: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_group_action(
            group,
            PendingGroupAction::OpenSession(session_id),
            window,
            cx,
        );
    }

    /// One group's workspace root: the active workspace's for `None`, the
    /// recorded path for another group. `None` before the runtime is up.
    pub(super) fn group_root(&self, group: Option<&str>) -> Option<String> {
        match group {
            Some(root) => Some(root.to_string()),
            None => runtime::runtime_handle()
                .map(|handle| handle.workspace().root_path.to_string_lossy().to_string()),
        }
    }

    /// Copies one group's workspace root (web `nav.workspaces.actions.copyPath`).
    pub(super) fn copy_group_path(&self, group: Option<&str>, cx: &mut Context<Self>) {
        if let Some(root) = self.group_root(group) {
            cx.write_to_clipboard(ClipboardItem::new_string(root.clone()));
            tracing::info!("Copied workspace path {root}");
        }
    }

    /// Reveals one group's workspace root in the platform file manager (web
    /// `nav.workspaces.actions.reveal`; the web disables it for remote
    /// workspaces, which the shell does not have yet).
    pub(super) fn reveal_group_path(&self, group: Option<&str>) {
        if let Some(root) = self.group_root(group) {
            crate::settings::open_path_in_file_manager(std::path::Path::new(&root));
        }
    }

    /// Folds every workspace group and the current one (web
    /// `nav.sessions.viewMenu.collapseAll`, offered while grouped).
    pub(super) fn collapse_all_groups(&mut self, cx: &mut Context<Self>) {
        self.sessions_group_collapsed = true;
        for group in &mut self.other_groups {
            group.collapsed = true;
        }
        cx.notify();
    }

    /// A turn is executing for this session, whether its tab is open or not:
    /// the event pump reports every session's lifecycle process-wide.
    pub(super) fn session_is_running(&self, session_id: &str) -> bool {
        self.live_sessions.contains(session_id)
    }

    /// Direct subagent children of a session (R1c), as sidebar rows: the
    /// lineage snapshot rooted at the session, filtered to depth one —
    /// deeper descendants stay hidden, matching the web nav's two-level
    /// projection. Children keep their pre-order arrival order.
    pub(super) fn lineage_children(&self, root_session_id: &str) -> Vec<SessionRow> {
        let Some(snapshot) = self
            .lineages
            .iter()
            .find(|snapshot| snapshot.root_session_id == root_session_id)
        else {
            return Vec::new();
        };
        snapshot
            .sessions
            .iter()
            .filter(|entry| entry.parent_session_id.as_deref() == Some(root_session_id))
            .map(|entry| SessionRow {
                id: entry.session_id.clone(),
                name: entry.session_name.clone(),
                metadata: entry.subagent_type.clone().unwrap_or_default(),
                archived: false,
                // Children follow their parent's row; they never take part in
                // the list ordering.
                created_at_ms: 0,
                last_active_at_ms: 0,
            })
            .collect()
    }
}

impl ShellWindow {
    /// The navigation sidebar (R1b, desktop NavPanel shape): the "search
    /// everything" trigger in the header, the three top-scene nav rows, the
    /// sessions section (titled header with new/archive/reload/workspace-menu
    /// actions above the live list), and the runtime status line in the
    /// footer. R1d moved the footer tool buttons to the window-bottom global
    /// status bar. Each workspace group is one child of the sidebar list (see
    /// [`Self::group_entries`]); only the group count moves that list's item
    /// count, so nothing a click can fold resets the scroll position.
    pub(super) fn render_sidebar(
        &self,
        text: &ShellText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Sidebar<SidebarSection> {
        // Fixed sidebar chrome: the search trigger, the scene navigation
        // rows, and the sessions section header all live in the component's
        // header slot, so only the workspace groups scroll. The web keeps the
        // same header reachable with `position: sticky` inside its scroll
        // container (`StickySectionHeader`) and lets the navigation rows
        // scroll away above it; gpui has no sticky positioning and the
        // component exposes no scroll accessor (its `ListState` is keyed by
        // the full element-id stack, so it cannot be read from outside), so
        // the shell pins this block instead — the header and its actions stay
        // reachable, the scroll-away navigation is the recorded divergence.
        let header = div().w_full().v_flex().child(
            div()
                .id("search-everything")
                .debug_selector(|| "search-everything".to_string())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_search_overlay(window, cx);
                }))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_x_2()
                .h_8()
                .px_2()
                .rounded(cx.theme().radius)
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .hover(|style| style.bg(cx.theme().sidebar_accent))
                .child(Icon::new(IconName::Search).small())
                .child(div().flex_1().truncate().child(text.search_trigger.clone()))
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_tokens().sm)
                        .child("Ctrl K"),
                ),
        );
        // Nav rows above the sessions section (web-ui NavPanel order: top
        // scenes above the session list), rendered in place of the list.
        let nav_entries: Vec<SidebarEntry> = nav_actions()
            .into_iter()
            .map(|action| SidebarEntry::Nav {
                action,
                label: SharedString::from(nav_label(action, text)),
                active: action.is_active(&self.tabs),
                view: cx.entity().downgrade(),
            })
            .collect();
        let mut nav_rows: Vec<gpui_kit::AnyElement> = Vec::with_capacity(nav_entries.len());
        for (ix, entry) in nav_entries.into_iter().enumerate() {
            // Per-row id: gpui rejects duplicate ids inside one frame.
            let id = ElementId::Name(SharedString::from(format!("nav-row-{ix}")));
            nav_rows.push(entry.render(id, window, cx).into_any_element());
        }
        let header = header.children(nav_rows).child(session_section_header_row(
            SharedString::from("shell-sidebar-sessions-header"),
            SharedString::from(text.sessions_title.clone()),
            self.event_tx.clone(),
            self.show_archived,
            self.session_list,
            cx.entity().downgrade(),
            cx,
        ));

        Sidebar::new(SIDEBAR_ID)
            .w(px(SESSION_PANE_WIDTH))
            .header(header)
            .children(self.render_session_entries(text, cx))
            // No component footer: the runtime status line and the tool
            // buttons moved to the nav column's own footer row (the web
            // PersistentFooterActions shape), which the collapse folds away
            // together with the column.
            .footer(SidebarFooter::new())
    }

    /// The sidebar footer (web `PersistentFooterActions`): the user slot and
    /// the user slot on the left, the tool buttons on the right —
    /// terminal dock toggle, workspace switcher, settings. The runtime status
    /// line rides above the row only while it has something to say (starting,
    /// failed, or a workspace error); the all-clear "ready" line the old
    /// window-bottom status bar carried is ambient noise the desktop footer
    /// does not repeat.
    pub(super) fn render_sidebar_footer(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let left = div().flex().items_center().gap_3().min_w_0().child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .max_w(px(160.))
                .truncate()
                .child(SharedString::from(self.os_user.clone())),
        );
        let status_line = match &self.phase {
            RuntimePhase::Starting => {
                Some((text.status_starting.clone(), cx.theme().muted_foreground))
            }
            RuntimePhase::Failed(error) => Some((error.clone(), cx.theme().danger)),
            RuntimePhase::Ready => self
                .session_open_error
                .clone()
                .or_else(|| self.workspace_error.clone())
                .map(|error| (error, cx.theme().danger)),
        };
        div()
            .id("shell-sidebar-footer")
            .debug_selector(|| "shell-sidebar-footer".to_string())
            .w_full()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .when_some(status_line, |this, (line, color)| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(color)
                        .min_w_0()
                        .truncate()
                        .child(line),
                )
            })
            .child(
                // The desktop footer (PersistentFooterActions): identity and
                // the lamp on the left, the settings gear alone on the right.
                // The terminal toggle moved to the scene top bar; workspace
                // switching lives in the nav top bar button and the composer
                // strip pill.
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .min_w_0()
                    .child(left)
                    .child(self.render_settings_button(text, cx)),
            )
    }
}

impl ShellWindow {
    /// The settings entry: theme and language moved into the settings page
    /// (M8), so the footer keeps a single button.
    pub(super) fn render_settings_button(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new("sidebar-settings")
            .icon(IconName::Settings)
            .small()
            .with_variant(active_variant_settings(&self.tabs))
            .tooltip(text.settings_open.clone())
            .on_click(cx.listener(|this, _, _, cx| this.open_settings_view(cx)))
    }
}

/// The shared workspace menu body (web MainNav workspace menu + the nav top
/// bar's workspace button): the two project verbs first — 打开项目, 新建项目 —
/// then the recent workspace list with the active one checked. Used by the
/// nav-top-bar button, the composer strip pill, and the sessions header's
/// FolderPlus trigger so every entry stays one menu.
pub(super) fn workspace_menu_into(
    menu: gpui_kit::component::menu::PopupMenu,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::component::menu::PopupMenu {
    let Some(shell) = view.upgrade() else {
        return menu;
    };
    let (choices, error, tx) = {
        let shell = shell.read(cx);
        (
            shell.workspaces.clone(),
            shell.workspace_error.clone(),
            shell.event_tx.clone(),
        )
    };
    let active_id = runtime::runtime_handle().map(|handle| handle.workspace().id);
    let text = i18n::current(cx);
    let mut menu = menu;
    if let Some(error) = error {
        menu = menu.item(PopupMenuItem::new(error).disabled(true));
    }
    let open_view = view.clone();
    let new_view = view.clone();
    menu = menu
        .item(
            PopupMenuItem::new(text.workspace_open_project.clone()).on_click(
                move |_, window, cx| {
                    open_workspace_path_dialog(open_view.clone(), window, cx);
                },
            ),
        )
        .item(
            PopupMenuItem::new(text.workspace_new_project.clone()).on_click(
                move |_, window, cx| {
                    open_new_project_dialog(new_view.clone(), window, cx);
                },
            ),
        )
        .separator();
    for choice in choices {
        let label = format!("{} · {}", choice.name, choice.root_path);
        let item = if active_id.as_deref() == Some(choice.id.as_str()) {
            PopupMenuItem::new(label).checked(true)
        } else {
            PopupMenuItem::new(label)
        };
        let root = choice.root_path;
        let tx = tx.clone();
        menu = menu.item(item.on_click(move |_, _, _| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.switch_workspace(std::path::PathBuf::from(&root), tx.clone());
            }
        }));
    }
    menu
}

/// The sessions header's filter menu (web `WorkspaceSessionFilterMenu`
/// restricted to the segments the shell has data for): ordering
/// updated/created/name, show all/unread, the running status filter, reset,
/// and the two whole-list verbs (collapse all while grouped, mark all read).
/// Web's worktree/environment/source segments, its needs-attention value and
/// its per-segment submenus stay out — one flat menu with section labels
/// instead; the parity doc records the divergences.
pub(super) fn session_filter_menu_into(
    menu: gpui_kit::component::menu::PopupMenu,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::component::menu::PopupMenu {
    let Some(shell) = view.upgrade() else {
        return menu;
    };
    let (filters, tx) = {
        let shell = shell.read(cx);
        (shell.session_list, shell.event_tx.clone())
    };
    let text = i18n::current(cx);
    let mut menu = menu;

    // Ordering: radio semantics — one ordering is active at a time.
    menu = menu.item(PopupMenuItem::label(text.sessions_ordering.clone()));
    for (ordering, label) in [
        (
            SessionOrdering::Updated,
            text.sessions_order_updated.clone(),
        ),
        (
            SessionOrdering::Created,
            text.sessions_order_created.clone(),
        ),
        (SessionOrdering::Name, text.sessions_order_name.clone()),
    ] {
        let view = view.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .checked(filters.ordering == ordering)
                .on_click(move |_, _, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |this, cx| {
                            this.session_list.ordering = ordering;
                            cx.notify();
                        });
                    }
                }),
        );
    }

    // Show: all / unread (the window's unread-completion map).
    menu = menu.item(PopupMenuItem::label(text.sessions_show.clone()));
    for (unread_only, label) in [
        (false, text.sessions_show_all.clone()),
        (true, text.sessions_show_unread.clone()),
    ] {
        let view = view.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .checked(filters.unread_only == unread_only)
                .on_click(move |_, _, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |this, cx| {
                            this.session_list.unread_only = unread_only;
                            cx.notify();
                        });
                    }
                }),
        );
    }

    // Filters: the one status the shell can derive is `running` (checked =
    // shown, the web multi-select's semantics).
    menu = menu.item(PopupMenuItem::label(text.sessions_filters.clone()));
    menu = menu.item(
        PopupMenuItem::new(text.sessions_status_running.clone())
            .checked(!filters.hide_running)
            .on_click({
                let view = view.clone();
                move |_, _, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |this, cx| {
                            this.session_list.hide_running = !this.session_list.hide_running;
                            cx.notify();
                        });
                    }
                }
            }),
    );
    menu = menu.item(
        PopupMenuItem::new(text.sessions_reset_filters.clone()).on_click({
            let view = view.clone();
            move |_, _, cx| {
                if let Some(shell) = view.upgrade() {
                    shell.update(cx, |this, cx| {
                        // Web `resetFilters` restores the filter section only,
                        // not ordering or show: the running filter clears and
                        // archived sessions hide again.
                        this.session_list.hide_running = false;
                        this.show_archived = false;
                        cx.notify();
                    });
                }
            }
        }),
    );

    menu = menu.separator();
    if !filters.flat {
        menu = menu.item(
            PopupMenuItem::new(text.sessions_collapse_all.clone()).on_click({
                let view = view.clone();
                move |_, _, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |this, cx| this.collapse_all_groups(cx));
                    }
                }
            }),
        );
    }
    menu.item(
        PopupMenuItem::new(text.sessions_mark_all_read.clone()).on_click(move |_, _, _| {
            let _ = tx.send(RuntimeEvent::TrayMarkAllRead);
        }),
    )
}

/// Ordering choices the sessions list offers (web `WorkspaceSessionOrdering`
/// minus `status`: the shell has no error/attention/completed session state to
/// order by — see the parity doc's deliberate divergences).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionOrdering {
    Updated,
    Created,
    Name,
}

/// The sessions list projection (web `WorkspaceSessionViewPreferences` for the
/// segments the shell has real data for). Window state: web persists the same
/// preferences per workspace in localStorage, the shell keeps them for the
/// window (parity doc §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SessionListFilters {
    /// grouping: `false` = the grouped workspace tree, `true` = one flat list
    /// of every workspace's sessions (web `grouped` / `all`).
    pub(super) flat: bool,
    pub(super) ordering: SessionOrdering,
    /// show: only sessions with unread completions (the window's unread map).
    pub(super) unread_only: bool,
    /// status: hide the sessions with a live turn.
    pub(super) hide_running: bool,
}

impl Default for SessionListFilters {
    fn default() -> Self {
        Self {
            flat: false,
            ordering: SessionOrdering::Updated,
            unread_only: false,
            hide_running: false,
        }
    }
}

impl SessionListFilters {
    /// The filter button marks itself active while anything narrows the list
    /// (web `isCustomized`); grouping alone does not count.
    pub(super) fn is_customized(&self) -> bool {
        self.unread_only || self.hide_running
    }
}

/// Per-row facts the projection reads beyond the row record itself, built by
/// the window so the pipeline below stays a pure function.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SessionRowFacts {
    pub(super) archived: bool,
    pub(super) running: bool,
    pub(super) unread: bool,
}

/// Web `matchesWorkspaceSessionView` restricted to the segments the shell has
/// data for: the archive toggle, show-unread, and the running status filter.
pub(super) fn session_row_matches(
    facts: SessionRowFacts,
    filters: &SessionListFilters,
    show_archived: bool,
) -> bool {
    if facts.archived && !show_archived {
        return false;
    }
    if filters.unread_only && !facts.unread {
        return false;
    }
    if filters.hide_running && facts.running {
        return false;
    }
    true
}

/// Web `compareWorkspaceNavSessions` for the orderings the shell supports.
/// Ties fall back to created-desc then session id, so the order is total and
/// stable across reloads.
pub(super) fn compare_session_rows(
    left: &SessionRow,
    right: &SessionRow,
    ordering: SessionOrdering,
) -> std::cmp::Ordering {
    let primary = match ordering {
        SessionOrdering::Updated => right.last_active_at_ms.cmp(&left.last_active_at_ms),
        SessionOrdering::Created => right.created_at_ms.cmp(&left.created_at_ms),
        // The web compare is locale- and number-aware
        // (`localeCompare(..., { numeric, sensitivity: 'base' })`); the shell
        // compares case-folded names — same ordering for ASCII/Latin titles,
        // no numeric collation.
        SessionOrdering::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    };
    primary
        .then_with(|| right.created_at_ms.cmp(&left.created_at_ms))
        .then_with(|| left.id.cmp(&right.id))
}

/// What a sidebar group action should do, given its workspace group
/// (`None` = the active workspace) and the active workspace's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkspaceGroupPlan {
    /// The group is the active workspace: act straight away.
    Act,
    /// Another workspace: switch there first, then act.
    SwitchThenAct { root: String },
}

/// Projects one archived-sessions read into the page's group list (G-SE30):
/// rows newest first, groups ordered by their newest member — the web sorts
/// every collected entry by `lastActiveAt` descending and groups in that
/// order, so a workspace with the newest archived session leads. Workspaces
/// with no archived rows render no group at all. A failed read contributes a
/// failure line (web `loadFailures`); on a background refresh its previous
/// rows stay visible (web `loadArchived`).
pub(super) fn compose_archived_groups(
    groups: Vec<runtime::ArchivedWorkspaceGroup>,
    previous: &[ArchivedGroupView],
    fresh: bool,
    cx: &App,
) -> (Vec<ArchivedGroupView>, Vec<String>) {
    let mut composed: Vec<ArchivedGroupView> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for group in groups {
        let label = if group.name == group.root_path {
            group.root_path.clone()
        } else {
            format!("{} · {}", group.name, group.root_path)
        };
        match group.rows {
            Ok(mut rows) => {
                if rows.is_empty() {
                    continue;
                }
                rows.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
                composed.push(ArchivedGroupView {
                    workspace_id: group.workspace_id,
                    root_path: group.root_path,
                    label,
                    rows: rows
                        .into_iter()
                        .map(|row| ArchivedRowView {
                            session_id: row.session_id,
                            name: row.name,
                            last_active_at: row.last_active_at,
                        })
                        .collect(),
                });
            }
            Err(_) => {
                failures.push(gpui_foundation::i18n::archived_load_failed(
                    &group.name,
                    &group.root_path,
                    cx,
                ));
                if !fresh {
                    if let Some(kept) = previous
                        .iter()
                        .find(|view| view.root_path == group.root_path)
                    {
                        composed.push(kept.clone());
                    }
                }
            }
        }
    }
    composed.sort_by(|a, b| {
        let a_newest = a.rows.first().map(|row| row.last_active_at).unwrap_or(0);
        let b_newest = b.rows.first().map(|row| row.last_active_at).unwrap_or(0);
        b_newest.cmp(&a_newest)
    });
    (composed, failures)
}

pub(super) fn workspace_group_plan(group: Option<&str>, current_root: &str) -> WorkspaceGroupPlan {
    match group {
        Some(root) if root != current_root => WorkspaceGroupPlan::SwitchThenAct {
            root: root.to_string(),
        },
        _ => WorkspaceGroupPlan::Act,
    }
}

/// One sidebar group action waiting for its workspace to become active — the
/// switch-then-act rule, settled on [`RuntimeEvent::WorkspaceSwitched`].
/// Session rows carry [`PendingGroupAction::OpenSession`]; the group header's
/// new-session and files actions carry the other two. All three follow the
/// web, which activates a workspace before acting inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PendingGroupAction {
    /// Open this session (the row click's switch-then-open).
    OpenSession(String),
    /// Create a session in the workspace (web `nav.workspaces.actions.newSession`).
    NewSession,
    /// Show the workspace's Files panel (the group card's folder action).
    OpenFiles,
}
