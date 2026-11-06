// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

impl ShellWindow {
    /// Shows the settings page in the main area. The nav column switches to
    /// the settings tree while the scene is active (web `SettingsNav`), and
    /// the page renders the selected `settings_page` entry in the centered
    /// column. M22 page data (global usage stats, web-search credential
    /// status) loads here so a page render never triggers fetches.
    pub(super) fn open_settings_view(&mut self, cx: &mut Context<Self>) {
        self.tabs.open(TabKind::Scene(SceneId::Settings));
        if self.usage_stats.is_none() && !self.usage_loading {
            self.usage_loading = true;
            if let Some(handle) = runtime::runtime_handle() {
                handle.fetch_usage_stats(self.usage_range.clone());
            }
        }
        if self.usage_activity.is_none() && !self.usage_activity_loading {
            self.usage_activity_loading = true;
            if let Some(handle) = runtime::runtime_handle() {
                handle.fetch_usage_activity();
            }
        }
        let (provider, http_auth_mode) = settings::ShellConfig::try_current(cx)
            .map(|config| (config.web_search_provider, config.web_search_http_auth_mode))
            .unwrap_or_default();
        if matches!(provider.as_str(), "exa_search_api" | "tavily")
            || (provider == "openbitfun_search_http"
                && settings::websearch_http_credential_required(&http_auth_mode))
        {
            if let Some(handle) = runtime::runtime_handle() {
                handle.load_web_search_credential_status(provider);
            }
        }
        cx.notify();
    }

    /// Selects the settings page the nav column and the content column show
    /// (web `openDestination`: the nav drives the page, the tab strip stays
    /// where it is; the page keeps its own scroll position).
    pub(crate) fn set_settings_page(
        &mut self,
        page: settings::SettingsEntry,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = page;
        // The archived page's projection spans every recent workspace, so it
        // is read when the page is selected (web loads on mount) and refreshed
        // after each mutation while it stays open.
        if page == settings::SettingsEntry::Archived {
            self.reload_archived_groups(cx);
        }
        // G-SE25: the automation page reads the Hook-import projection on
        // selection (web loads on mount); a plain read uses the catalog cache.
        if page == settings::SettingsEntry::Automation {
            settings::fetch_hook_imports(false, cx);
        }
        // E3-B2: the worktree management card reads the host catalog on
        // selection (web loads on mount); the in-flight guard coalesces
        // repeated selections, and the card's refresh action re-reads it.
        if page == settings::SettingsEntry::WorktreesGit {
            settings::fetch_worktree_projects(cx);
        }
        // E3-B5: the ACP agents page reads the client list, the requirement
        // probes, and the config document on selection (web loads on mount).
        if page == settings::SettingsEntry::ExternalAgents {
            if let Some(handle) = runtime::runtime_handle() {
                handle.reload_acp_snapshot();
                handle.load_acp_json_config();
            }
        }
        cx.notify();
    }

    /// Reloads the engine page's daemon configuration (G-SE14). The in-flight
    /// guard coalesces repeated selections; a stale offline banner clears when
    /// a fresh read starts so the page never shows two verdicts at once.

    /// G-SE13: flips the memory page's advanced block (web `expandAdvanced` /
    /// `collapseAdvanced`). Purely presentational, so it stays on the entity.
    pub(crate) fn toggle_memory_advanced(&mut self, cx: &mut Context<Self>) {
        settings::MemoryPageState::mutate(cx, |state| state.advanced_open = !state.advanced_open);
        cx.notify();
    }

    /// G-SE13: reveals the memory root in the platform file manager (the web
    /// page's `openDirectory`, which creates the directory first, so the
    /// action works before the first memory was ever written). Fire and
    /// forget, so it needs no in-flight guard; a missing path manager is the
    /// only failure and surfaces as the page's error receipt.
    pub(crate) fn open_memory_directory(&mut self, cx: &mut Context<Self>) {
        match openbitfun_core::infrastructure::try_get_path_manager_arc() {
            Ok(manager) => {
                let dir = manager.memories_root_dir();
                if let Err(error) = std::fs::create_dir_all(&dir) {
                    tracing::warn!("Failed to create memory directory {:?}: {}", dir, error);
                }
                crate::settings::open_path_in_file_manager(&dir);
            }
            Err(error) => {
                tracing::warn!("Memory directory unavailable: {}", error);
                crate::settings::MemoryNotice::set(
                    Some((
                        false,
                        i18n::current(cx).memory_open_directory_failed.clone(),
                    )),
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// G-SE13: restores the memory settings document to its defaults. Guarded
    /// so the confirmation dialog cannot queue two resets.
    pub(crate) fn reset_memory_settings(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = runtime::runtime_handle() else {
            return;
        };
        let start = settings::MemoryPageState::mutate(cx, |state| {
            if state.busy {
                return false;
            }
            state.busy = true;
            true
        });
        if !start {
            return;
        }
        crate::settings::MemoryNotice::set(None, cx);
        handle.reset_memories_config(self.event_tx.clone());
        cx.notify();
    }

    /// G-SE13: permanently clears the memory database and workspace files.
    pub(crate) fn clear_memory_data(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = runtime::runtime_handle() else {
            return;
        };
        let start = settings::MemoryPageState::mutate(cx, |state| {
            if state.busy {
                return false;
            }
            state.busy = true;
            true
        });
        if !start {
            return;
        }
        crate::settings::MemoryNotice::set(None, cx);
        handle.clear_memory_data(self.event_tx.clone());
        cx.notify();
    }
}

impl ShellWindow {
    /// Runs one update check through the updater; the result lands back
    /// through the pump. Shared by the settings button and the tray menu.
    pub(super) fn start_update_check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.update_state,
            gpui_host::update::UpdateUiState::Checking
                | gpui_host::update::UpdateUiState::Installing
        ) {
            return;
        }
        self.update_state = gpui_host::update::UpdateUiState::Checking;
        cx.notify();
        let Some(handle) = runtime::runtime_handle() else {
            self.update_state = gpui_host::update::UpdateUiState::Failed {
                message: "runtime is not ready".to_string(),
            };
            return;
        };
        handle.tokio_handle().spawn(async move {
            let result = gpui_host::update::check_for_update(None)
                .await
                .map_err(|error| format!("{error:#}"));
            runtime::publish(RuntimeEvent::UpdateCheckFinished { result });
        });
    }

    /// Downloads and installs the offered release; the finished event carries
    /// the installed version and the page offers the restart.
    pub(super) fn start_update_install(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.update_state,
            gpui_host::update::UpdateUiState::Available { .. }
        ) {
            return;
        }
        self.update_state = gpui_host::update::UpdateUiState::Installing;
        cx.notify();
        let Some(handle) = runtime::runtime_handle() else {
            self.update_state = gpui_host::update::UpdateUiState::Failed {
                message: "runtime is not ready".to_string(),
            };
            return;
        };
        handle.tokio_handle().spawn(async move {
            let result = gpui_host::update::install_available_update(None)
                .await
                .map_err(|error| format!("{error:#}"));
            runtime::publish(RuntimeEvent::UpdateInstallFinished { result });
        });
    }
}

impl ShellWindow {
    /// Opens the changes view (creating the panel, which fetches the workspace
    /// diff snapshot) or returns to the chat when it is already showing.
    /// The chat's open session rides along: the worktree view's session
    /// isolation toggle addresses it (M19).
    /// Opens the changes resource (R2c): docked into the active session tab's
    /// right panel, or as a main scene tab when no session tab is active —
    /// same opening policy as Files. Creating the panel fetches the workspace
    /// diff snapshot. Idempotent: an open tab or docked panel just stays.
    /// The chat's active session rides along: the worktree view's session
    /// isolation toggle addresses it (M19).
    pub(super) fn toggle_git_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_resource(ResourcePanelKind::Git, window, cx);
    }

    /// The chat turn footer's snapshot entry point (R2d): opens the git
    /// resource the same way `toggle_resource` does but never closes it, then
    /// points the panel at the snapshots view for this workspace's sessions.
    pub(super) fn open_git_snapshots(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_git_panel(window, cx);
        self.set_git_view(crate::git::GitView::Snapshots, cx);
    }
}

impl ShellWindow {
    /// Points the git panel at a view without touching its mount; a no-op
    /// while the panel is closed (the settings worktrees redirect pairs this
    /// with [`Self::open_git_panel`], R3).
    pub(crate) fn set_git_view(&mut self, view: crate::git::GitView, cx: &mut Context<Self>) {
        if let Some(git) = self.git.clone() {
            git.update(cx, |panel, cx| panel.set_view(view, cx));
        }
    }

    /// Opens the git resource with the resource opening policy minus the
    /// toggle: docked into the active session tab's right panel, or as a main
    /// scene tab when no session tab is active. Idempotent — an open tab or
    /// docked panel just stays, and the panel's current view is kept.
    pub(crate) fn open_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.tabs.active_kind(), Some(TabKind::Session(_))) {
            if self.resource_panel != Some(ResourcePanelKind::Git) {
                self.ensure_resource_page(ResourcePanelKind::Git, window, cx);
                self.dock_resource_panel(ResourcePanelKind::Git, cx);
            }
        } else {
            self.ensure_resource_page(ResourcePanelKind::Git, window, cx);
            self.set_resource_compact(ResourcePanelKind::Git, false, cx);
            self.tabs
                .open(TabKind::Scene(ResourcePanelKind::Git.scene()));
        }
        cx.notify();
    }

    /// Closes the top tab at `index`. A closing scene tab drops its page
    /// state where the old toggle did (the git panel refetches on reopen);
    /// a closing session tab drops that chat's transcript state.
    pub(super) fn close_top_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(kind) = self.tabs.kinds().get(index).cloned() else {
            return;
        };
        self.tabs.close(index);
        match kind {
            TabKind::Scene(SceneId::Git) => {
                // Drop the panel with the tab so the next open refetches.
                self.git = None;
            }
            TabKind::Session(session_id) => {
                self.chats.retain(|chat| !chat.matches(&session_id));
            }
            TabKind::Scene(_) => {}
        }
        self.list_sync_reset(cx);
        cx.notify();
    }
}

impl ShellWindow {
    /// Sidebar nav row activation (M23 nav rows, R1a tab semantics): scene
    /// rows open (and thereby activate) their scene tab — clicking an active
    /// row keeps it active instead of toggling back, matching the desktop.
    /// Chat activates the most recent session tab; with none open it rests on
    /// the empty state.
    pub(super) fn apply_nav(&mut self, action: NavAction, cx: &mut Context<Self>) {
        match action {
            NavAction::Chat => {
                if let Some(chat) = self.chats.last() {
                    let session_id = chat.session_id.clone();
                    self.tabs.open(TabKind::Session(session_id));
                } else {
                    self.tabs = TabsState::default();
                }
                cx.notify();
            }
            NavAction::Tasks => self.toggle_tasks_view(cx),
            NavAction::MiniApps => {
                self.tabs.open(TabKind::Scene(SceneId::MiniApps));
                cx.notify();
            }
            NavAction::Extensions => {
                self.tabs.open(TabKind::Scene(SceneId::Extensions));
                cx.notify();
            }
            NavAction::Insights => self.toggle_insights_view(cx),
        }
    }

    /// Insights page tab; the first open fetches the latest report list.
    pub(super) fn toggle_insights_view(&mut self, cx: &mut Context<Self>) {
        if !self.insights.loaded {
            if let Some(handle) = runtime::runtime_handle() {
                handle.reload_insights(self.event_tx.clone());
            }
        }
        self.tabs.open(TabKind::Scene(SceneId::Insights));
        cx.notify();
    }

    /// R1d status bar: one lamp prefetch when the runtime first reports
    /// ready, so the chip answers without the console page being opened.
    /// Scheduled-jobs page tab; the first open fetches the job snapshot.
    pub(super) fn toggle_tasks_view(&mut self, cx: &mut Context<Self>) {
        if !self.tasks.loaded {
            if let Some(handle) = runtime::runtime_handle() {
                handle.reload_cron_jobs(self.event_tx.clone());
            }
        }
        self.tabs.open(TabKind::Scene(SceneId::Tasks));
        cx.notify();
    }

    /// Files resource (R2c): with a session tab active the page docks into
    /// that tab's right panel; otherwise it opens as a main resource tab —
    /// the workbench opening policy's "no matching session tab" branch.
    /// Toggling the docked kind closed hides the panel again.
    pub(super) fn toggle_files_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_resource(ResourcePanelKind::Files, window, cx);
    }
}

impl ShellWindow {
    /// The R2c resource entry point: docked right panel while a session tab
    /// is active, main scene tab otherwise — the workbench opening policy's
    /// "no matching session tab" branch. Docking keeps the page entity alive
    /// across the mount move and evicts the resource's main tab (the two
    /// forms are mutually exclusive); closing the panel remembers the dragged
    /// width, like the terminal dock remembers its height.
    pub(super) fn toggle_resource(
        &mut self,
        kind: ResourcePanelKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.tabs.active_kind(), Some(TabKind::Session(_))) {
            if self.resource_panel == Some(kind) {
                self.close_resource_panel(cx);
            } else {
                self.ensure_resource_page(kind, window, cx);
                self.dock_resource_panel(kind, cx);
            }
        } else {
            self.ensure_resource_page(kind, window, cx);
            self.set_resource_compact(kind, false, cx);
            self.tabs.open(TabKind::Scene(kind.scene()));
            cx.notify();
        }
    }

    /// Creates the page entity behind a resource if it does not exist yet.
    /// Files lives for the window (its tabs survive view switches); the git
    /// panel is recreated so the next open refetches the diff snapshot.
    pub(super) fn ensure_resource_page(
        &mut self,
        kind: ResourcePanelKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ResourcePanelKind::Files => {
                self.files
                    .get_or_insert_with(|| cx.new(crate::files::FilesPage::new));
            }
            ResourcePanelKind::Git => {
                if self.git.is_none() {
                    let active_session = self.active_chat().map(|chat| chat.session_id.clone());
                    // The panel hands its not-a-repository init prompt back to
                    // this window's composer (G-G1).
                    let shell = cx.entity().downgrade();
                    self.git = Some(
                        cx.new(move |cx| GitPanel::new(active_session, Some(shell), window, cx)),
                    );
                }
            }
        }
    }

    /// Docks `kind` into the session tab's right panel and closes its main
    /// tab without dropping the page entity — one resource, one mount point.
    /// The panel is the page's compact form (single column; a 360px panel
    /// cannot fit the two-column main-tab layout).
    pub(super) fn dock_resource_panel(&mut self, kind: ResourcePanelKind, cx: &mut Context<Self>) {
        self.set_resource_compact(kind, true, cx);
        self.resource_panel = Some(kind);
        self.tabs.remove(&TabKind::Scene(kind.scene()));
        cx.notify();
    }

    /// Flips a page between its docked (compact) and main tab (full) forms.
    pub(super) fn set_resource_compact(
        &mut self,
        kind: ResourcePanelKind,
        compact: bool,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ResourcePanelKind::Files => {
                if let Some(page) = &self.files {
                    page.update(cx, |page, cx| page.set_compact(compact, cx));
                }
            }
            ResourcePanelKind::Git => {
                if let Some(panel) = &self.git {
                    panel.update(cx, |panel, cx| panel.set_compact(compact, cx));
                }
            }
        }
    }

    /// Hides the right panel, remembering the dragged width so the next dock
    /// reopens at it (the terminal dock's height rule).
    pub(super) fn close_resource_panel(&mut self, cx: &mut Context<Self>) {
        let state = self.panel_split_state();
        if let Some(size) = state.read(cx).sizes().get(1) {
            self.panel_width =
                (*size).clamp(px(MIN_RESOURCE_PANEL_WIDTH), px(MAX_RESOURCE_PANEL_WIDTH));
        }
        self.resource_panel = None;
        self.resource_panel_maximized = false;
        cx.notify();
    }

    /// The split state of the current arrangement (one per side, G-F8).
    pub(super) fn panel_split_state(&self) -> Entity<ResizableState> {
        match self.resource_panel_side {
            PanelSide::Right => self.panel_split.clone(),
            PanelSide::Left => self.panel_split_left.clone(),
        }
    }

    /// Moves the resource panel to the other side of the chat column (G-F8,
    /// web `swapPanes`).
    pub(super) fn swap_resource_panel_side(&mut self, cx: &mut Context<Self>) {
        self.resource_panel_side = self.resource_panel_side.toggled();
        tracing::info!(
            "Resource panel docked to the {:?} of the chat column",
            self.resource_panel_side
        );
        cx.notify();
    }

    /// Maximizes the resource panel over the scene body, or restores the split
    /// (G-F8, web `toggleMaximized`).
    pub(super) fn toggle_resource_panel_maximized(&mut self, cx: &mut Context<Self>) {
        self.resource_panel_maximized = !self.resource_panel_maximized;
        cx.notify();
    }

    /// Pops the right panel out to its main scene tab (the web right-panel
    /// tab's pop-out): the panel hides, the tab opens and activates.
    pub(super) fn pop_out_resource_panel(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.resource_panel else {
            return;
        };
        self.resource_panel = None;
        self.resource_panel_maximized = false;
        self.set_resource_compact(kind, false, cx);
        self.tabs.open(TabKind::Scene(kind.scene()));
        cx.notify();
    }
}

impl ShellWindow {
    /// Global search overlay (R1b): opens the palette, clears the previous
    /// query state, and focuses the query input so typing starts immediately.
    pub(crate) fn open_search_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::Focusable as _;
        self.search_overlay_open = true;
        self.search = crate::search::SearchPageState::default();
        self.search_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        let focus = self.search_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Closes the palette and drops the query state, so a stale result set
    /// never lingers behind the next open. Focus blurs: the query input is
    /// about to unmount, and a dangling focus would swallow keystrokes
    /// (the render pass then parks focus back on the root).
    pub(crate) fn close_search_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_overlay_open {
            return;
        }
        self.search_overlay_open = false;
        self.search = crate::search::SearchPageState::default();
        window.blur(cx);
        cx.notify();
    }

    /// Runs the palette search (R1b): both paths at once, like the desktop's
    /// federated global search. Empty queries and in-flight runs are no-ops
    /// so the palette never floods or double-fires.
    pub(crate) fn run_search(
        &mut self,
        tx: &mpsc::UnboundedSender<RuntimeEvent>,
        cx: &mut Context<Self>,
    ) {
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() || self.search.running {
            return;
        }
        let Some(handle) = runtime::runtime_handle() else {
            self.search.error = Some("runtime not ready".to_string());
            cx.notify();
            return;
        };
        self.search.running = true;
        self.search.error = None;
        self.search.unavailable = None;
        self.search.query = query.clone();
        handle.search_workspace_content(query.clone(), tx.clone());
        handle.search_sessions(query, tx.clone());
        cx.notify();
    }

    /// Dispatches a palette quick action and closes the overlay. Terminal
    /// needs the window (dock toggle drags a splitter); the rest are tab
    /// opens.
    pub(crate) fn run_search_quick_action(
        &mut self,
        action: crate::search::SearchQuickAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::search::SearchQuickAction as Q;
        self.close_search_overlay(window, cx);
        match action {
            Q::NewSession => {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.create_session(self.event_tx.clone());
                }
            }
            Q::Terminal => self.toggle_terminal_panel(window, cx),
            Q::Git => self.toggle_git_view(window, cx),
            Q::Files => self.toggle_files_view(window, cx),
            Q::Insights => self.toggle_insights_view(cx),
            Q::Settings => self.open_settings_view(cx),
        }
    }

    /// Jumps from a palette entity hit (G-Q1) to its settings scene. Closing
    /// the overlay first keeps the focus chain identical to quick actions.
    pub(crate) fn open_settings_entry(
        &mut self,
        entry: crate::settings::SettingsEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_search_overlay(window, cx);
        self.open_settings_view(cx);
        self.set_settings_page(entry, cx);
    }
}

impl ShellWindow {
    /// Jumps from a search hit to the files viewer with the file open — the
    /// same opening policy as the toggles (R2c): the active session tab's
    /// right panel, or the main Files tab when no session tab is active.
    pub(crate) fn open_file_in_files_view(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_file_in_files_view_at(path, None, cx);
    }

    /// A transcript file link (`#L42` / `:12-14` line specs included):
    /// relative paths resolve against the active workspace root, then open
    /// under the files viewer's usual opening policy.
    pub(crate) fn open_transcript_path(
        &mut self,
        path: String,
        line: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let root = runtime::runtime_handle()
            .map(|handle| handle.workspace().root_path.to_string_lossy().to_string())
            .unwrap_or_default();
        let resolved = markdown::resolve_path(&root, &path);
        if resolved.is_empty() {
            return;
        }
        self.open_file_in_files_view_at(resolved, line, cx);
    }

    /// Like [`Self::open_file_in_files_view`], revealing a 1-based line when
    /// the link carried one.
    pub(crate) fn open_file_in_files_view_at(
        &mut self,
        path: String,
        line: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let page = self
            .files
            .get_or_insert_with(|| cx.new(crate::files::FilesPage::new));
        page.update(cx, |page, cx| page.open_path_at(path, line, cx));
        if matches!(self.tabs.active_kind(), Some(TabKind::Session(_))) {
            self.dock_resource_panel(ResourcePanelKind::Files, cx);
        } else {
            self.set_resource_compact(ResourcePanelKind::Files, false, cx);
            self.tabs.open(TabKind::Scene(SceneId::Files));
            cx.notify();
        }
    }
}
