// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

impl ShellWindow {
    /// The scene top bar (web `SceneTopBar`, the desktop's right 45px chrome
    /// row): the tab strip leads — horizontally scrollable, so a full strip
    /// never covers the trailing actions — then the active session's chrome
    /// (usage, quick actions, the right resource panel toggle), then the
    /// window controls. The empty stretch between tabs and actions drags the
    /// window; double-click zooms, the contract the removed component
    /// `TitleBar` used to honor.
    pub(super) fn render_scene_top_bar(
        &self,
        window: &mut Window,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut tabs = div()
            .id("shell-top-tabs")
            .debug_selector(|| "shell-top-tabs".to_string())
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .overflow_x_scroll();
        for (index, kind) in self.tabs.kinds().iter().enumerate() {
            let active = self.tabs.active_index() == Some(index);
            let (icon, label, tab_id) = match kind {
                TabKind::Session(session_id) => (
                    IconName::Sparkles,
                    self.chat_for(session_id)
                        .map(|chat| chat.name.clone())
                        .unwrap_or_else(|| session_id.clone()),
                    format!("top-tab-session-{index}"),
                ),
                TabKind::Scene(scene) => {
                    let def = crate::tabs::builtin_scenes()
                        .into_iter()
                        .find(|row| row.id == *scene);
                    let (icon, label) = match scene {
                        // Plugin scenes carry icon and title in the
                        // contribution; the builtin table has no row.
                        SceneId::External(reference) => (
                            self.plugin_scene_icon(*reference),
                            self.plugin_scene_label(*reference, text),
                        ),
                        _ => (
                            def.map(|row| row.icon).unwrap_or(IconName::Square),
                            crate::tabs::scene_title(*scene, text)
                                .unwrap_or_default()
                                .to_string(),
                        ),
                    };
                    (
                        icon,
                        label,
                        format!(
                            "top-tab-scene-{}-{index}",
                            format!("{scene:?}").to_lowercase()
                        ),
                    )
                }
            };
            let close_label = text.tab_close.clone();
            tabs = tabs.child(
                div()
                    .id(tab_id)
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_2()
                    .pr_1()
                    .h_7()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .flex_shrink_0()
                    .max_w(px(220.))
                    .when(active, |chip| {
                        chip.bg(cx.theme().secondary)
                            .text_color(cx.theme().secondary_foreground)
                    })
                    .when(!active, |chip| {
                        chip.text_color(cx.theme().muted_foreground)
                            .hover(|style| style.bg(cx.theme().muted))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tabs.activate(index);
                        this.list_sync_reset(cx);
                        cx.notify();
                    }))
                    .child(icon)
                    .child(
                        div()
                            .min_w_0()
                            .line_clamp(1)
                            .child(SharedString::from(label)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("top-tab-close-{index}")))
                            .icon(IconName::X)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(close_label)
                            .disabled(!active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_top_tab(index, cx);
                            })),
                    ),
            );
        }

        // Session chrome rides the top bar only while a session tab is the
        // active one (web SceneChromeHost: the active scene contributes).
        let session_active = matches!(self.tabs.active_kind(), Some(TabKind::Session(_)));
        let panel_icon = if self.resource_panel.is_some() {
            IconName::PanelRightClose
        } else {
            IconName::PanelRightOpen
        };
        let panel_label = if self.resource_panel.is_some() {
            text.panel_close.clone()
        } else {
            text.panel_open.clone()
        };

        // Window controls: the same gating the component TitleBar applies —
        // none on macOS (traffic lights), none under server-side decorations
        // on Linux, and only the buttons the window manager can honor. The
        // cluster renders at the end of the bar; the divider is painted only
        // when the cluster exists, so it is folded into the element itself.

        div()
            .id("shell-scene-top-bar")
            .debug_selector(|| "shell-scene-top-bar".to_string())
            .h(px(TOP_BAR_HEIGHT))
            .flex_none()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().background)
            // The folded-nav floating strip owns the corner; keep the tabs
            // clear of it.
            .when(!self.sidebar_open, |bar| {
                bar.pl(px(NAV_COLLAPSED_STRIP_WIDTH))
            })
            .on_double_click(|_, window, _| window.zoom_window())
            .child(tabs)
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            // The terminal dock toggle lives on the window chrome (any scene
            // can dock the terminal); the footer keeps only the settings gear,
            // like the desktop.
            .child({
                let terminal_variant = if self.terminal_open {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Ghost
                };
                Button::new("top-bar-terminal")
                    .icon(IconName::SquareTerminal)
                    .small()
                    .with_variant(terminal_variant)
                    .tooltip(text.terminal_title.clone())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_terminal_panel(window, cx)),
                    )
            })
            .child(self.render_notification_button(&text, cx))
            .when(session_active, |bar| {
                bar.child(self.render_usage_button(cx))
                    .child(self.render_quick_actions_menu(text, cx))
                    .child(
                        Button::new("top-bar-resource-panel")
                            .icon(panel_icon)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(panel_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.resource_panel.is_some() {
                                    this.close_resource_panel(cx);
                                } else {
                                    // The page entity is created on first dock;
                                    // docking without it renders an empty body.
                                    this.ensure_resource_page(ResourcePanelKind::Files, window, cx);
                                    this.dock_resource_panel(ResourcePanelKind::Files, cx);
                                }
                            })),
                    )
            })
            .children(self.render_window_controls(window))
    }

    /// The notification center (G-X6, web `NotificationCenter` behind the
    /// title bar's bell): the unread turns the tray badge counts, listed by
    /// session. The shell keeps no general notification history, so the panel
    /// projects exactly the completion facts it has. Opening a row opens that
    /// session (which clears its unread count); the last entry clears them
    /// all. The dot mirrors the tray: red when a turn failed.
    pub(super) fn render_notification_button(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let unread: Vec<(String, String, u32, bool)> = self
            .unread_turns
            .iter()
            .map(|(session_id, (count, failed))| {
                let name = self
                    .chat_for(session_id)
                    .map(|chat| chat.name.clone())
                    .or_else(|| {
                        self.sessions
                            .iter()
                            .find(|row| &row.id == session_id)
                            .map(|row| row.name.clone())
                    })
                    .unwrap_or_else(|| session_id.clone());
                (session_id.clone(), name, *count, *failed)
            })
            .collect();
        let total: u32 = unread.iter().map(|(_, _, count, _)| *count).sum();
        let any_failed = unread.iter().any(|(_, _, _, failed)| *failed);
        let view = cx.entity().downgrade();
        let menu_view = cx.entity().downgrade();
        let button = Button::new("top-bar-notifications")
            .debug_selector(|| "top-bar-notifications".to_string())
            .icon(IconName::Bell)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.notifications_title.clone())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, cx| {
                let text = i18n::current(cx);
                let mut menu = menu.max_h(px(320.));
                if unread.is_empty() {
                    return menu
                        .item(PopupMenuItem::new(text.notifications_empty.clone()).disabled(true));
                }
                for (session_id, name, count, failed) in &unread {
                    let view = view.clone();
                    let session_id = session_id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(i18n::notifications_row(name, *count, cx))
                            .icon(if *failed {
                                IconName::ShieldAlert
                            } else {
                                IconName::Check
                            })
                            .on_click(move |_, window, cx| {
                                if let Some(shell) = view.upgrade() {
                                    let session_id = session_id.clone();
                                    shell.update(cx, |this, cx| {
                                        this.open_session_row(session_id, None, window, cx);
                                    });
                                }
                            }),
                    );
                }
                let menu_view = menu_view.clone();
                menu.separator().item(
                    PopupMenuItem::new(text.notifications_clear_all.clone()).on_click(
                        move |_, _, cx| {
                            if let Some(shell) = menu_view.upgrade() {
                                shell.update(cx, |this, cx| this.clear_all_unread(cx));
                            }
                        },
                    ),
                )
            });
        div()
            .relative()
            .child(button)
            .when(total > 0, |this| {
                this.child(
                    div()
                        .debug_selector(|| "notification-badge".to_string())
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(if any_failed {
                            cx.theme().danger
                        } else {
                            cx.theme().accent
                        }),
                )
            })
            .into_any_element()
    }

    /// Clears every unread completion (the notification center's "Clear All").
    pub(super) fn clear_all_unread(&mut self, cx: &mut Context<Self>) {
        if self.unread_turns.is_empty() {
            return;
        }
        tracing::info!(
            "Clearing {} unread turn notifications",
            self.unread_turns.len()
        );
        self.unread_turns.clear();
        self.sync_tray_unread();
        cx.notify();
    }

    /// The window control cluster (minimize / zoom / close), shaped after the
    /// component `TitleBar`'s private `WindowControls`: client-side
    /// decorations only, and only the buttons the platform supports. Close
    /// keeps the M22 semantics — quit goes through the event pump so the
    /// terminal PTYs shut down first, otherwise the window minimizes to the
    /// tray.
    pub(super) fn render_window_controls(
        &self,
        window: &mut Window,
    ) -> Option<gpui_kit::AnyElement> {
        if cfg!(target_os = "macos") || cfg!(target_family = "wasm") {
            return None;
        }
        #[cfg(target_os = "linux")]
        if !matches!(
            window.window_decorations(),
            gpui_kit::Decorations::Client { .. }
        ) {
            return None;
        }
        let supported = window.window_controls();
        let maximized = window.is_maximized();
        let quit_tx = self.event_tx.clone();
        Some(
            div()
                .id("window-controls")
                .flex()
                .items_center()
                .flex_shrink_0()
                .when(supported.minimize, |this| {
                    this.child(
                        Button::new("window-minimize")
                            .icon(IconName::Minus)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                })
                .when(supported.maximize, |this| {
                    this.child(
                        Button::new("window-zoom")
                            .icon(if maximized {
                                IconName::Copy
                            } else {
                                IconName::Square
                            })
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, _| window.zoom_window()),
                    )
                })
                .child(
                    Button::new("window-close")
                        .icon(IconName::X)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, window, _| {
                            if CLOSE_QUITS.load(std::sync::atomic::Ordering::Relaxed) {
                                let _ = quit_tx.send(RuntimeEvent::QuitRequested);
                            } else {
                                window.minimize_window();
                            }
                        }),
                )
                .into_any_element(),
        )
    }

    /// The nav column's top bar (web `NavBar`): the sidebar drawer toggle and
    /// the brand at the left, the rest a native drag area. The window
    /// controls sit on the scene top bar, matching the desktop split.
    pub(super) fn render_nav_top_bar(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let settings_nav_active =
            self.tabs.active_kind() == Some(&TabKind::Scene(SceneId::Settings));
        div()
            .id("shell-nav-top-bar")
            .debug_selector(|| "shell-nav-top-bar".to_string())
            .h(px(TOP_BAR_HEIGHT))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .on_double_click(|_, window, _| window.zoom_window())
            .child(
                Button::new("sidebar-toggle")
                    .icon(IconName::PanelLeftClose)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(text.sidebar_toggle.clone())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_open = false;
                        cx.notify();
                    })),
            )
            // The desktop NavBar: the panel toggle, the history arrows, then
            // the active scene nav's title. The brand wordmark stands in for
            // the title while the default (MainNav) navigation is showing.
            .when(!settings_nav_active, |this| {
                this.child(div().text_sm().font_medium().child("OpenBitFun"))
            })
            // The desktop NavBar arrows: tab-activation history (scene and
            // session tabs alike, entries keyed by kind so closes survive).
            .child(
                Button::new("nav-back")
                    .icon(IconName::ArrowLeft)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(!self.tabs.can_go_back())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tabs.go_back();
                        this.list_sync_reset(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("nav-forward")
                    .icon(IconName::ArrowRight)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(!self.tabs.can_go_forward())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tabs.go_forward();
                        this.list_sync_reset(cx);
                        cx.notify();
                    })),
            )
            .when(settings_nav_active, |this| {
                this.child(
                    div()
                        .id("nav-scene-title")
                        .debug_selector(|| "nav-scene-title".to_string())
                        .truncate()
                        .text_sm()
                        .font_medium()
                        .child(text.settings_title.clone()),
                )
            })
            // Workspace switching at the window chrome (the desktop keeps it
            // in the nav's workspace section and the composer strip pill).
            .child(self.render_workspace_button(text, cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
    }

    /// The docked terminal panel: header (new-tab menu, one chip per tab,
    /// close) above the active pane. Each pane owns focus, keyboard, wheel
    /// scrollback, and reflows to whatever size the panel gives it.
    pub(super) fn render_terminal_panel(
        &self,
        text: &ShellText,
        dock: TerminalDock,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let active_title = self
            .terminals
            .get(self.active_terminal)
            .map(|tab| {
                let fallback = tab.label.clone();
                tab.pane.read(cx).title_label(fallback)
            })
            .unwrap_or_else(|| text.terminal_title.clone().into());

        // New-tab menu: the default shell plus every configured profile, and
        // the settings entry that manages the list (labels resolve here, in
        // the menu builder, like the insights range menu; item clicks carry
        // the window so pane creation goes through the ordinary path).
        let profiles = settings::ShellConfig::current(cx).terminal.profiles.clone();
        let menu_view = cx.entity().downgrade();
        let new_tab_button = Button::new("terminal-new-tab")
            .icon(IconName::Plus)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.terminal_new_tab.clone())
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, cx| {
                let default_label = gpui_foundation::i18n::current(cx)
                    .terminal_new_tab_default
                    .clone();
                let manage_label = gpui_foundation::i18n::current(cx)
                    .terminal_profiles_manage
                    .clone();
                let mut popup = menu.item(PopupMenuItem::new(default_label).on_click({
                    let menu_view = menu_view.clone();
                    move |_, window, cx| {
                        if let Some(shell) = menu_view.upgrade() {
                            shell.update(cx, |this, cx| this.open_terminal_tab(None, window, cx));
                        }
                    }
                }));
                for profile in &profiles {
                    let profile_id = profile.id.clone();
                    let menu_view = menu_view.clone();
                    popup = popup.item(PopupMenuItem::new(profile.name.clone()).on_click(
                        move |_, window, cx| {
                            if let Some(shell) = menu_view.upgrade() {
                                shell.update(cx, |this, cx| {
                                    this.open_terminal_tab(Some(profile_id.clone()), window, cx)
                                });
                            }
                        },
                    ));
                }
                popup = popup.item(PopupMenuItem::new(manage_label).on_click({
                    let menu_view = menu_view.clone();
                    move |_, _window, cx| {
                        if let Some(shell) = menu_view.upgrade() {
                            shell.update(cx, |this, cx| this.open_settings_view(cx));
                        }
                    }
                }));
                popup
            });

        let mut tab_bar = div().flex().items_center().gap_1().min_w_0();
        for (index, tab) in self.terminals.iter().enumerate() {
            let active = index == self.active_terminal;
            let fallback = tab.label.clone();
            let label = tab.pane.read(cx).title_label(fallback);
            let close_view = cx.entity();
            // G-T2: a profile tab that ran a startup command carries the
            // marker (the web's badge); its tooltip is the command.
            let startup_command = tab.startup_command.clone();
            let label_hover = startup_command.clone();
            let mut tab_entry = div()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_0p5()
                .rounded(cx.theme().radius)
                .when(active, |style| style.bg(cx.theme().sidebar_accent))
                .when(startup_command.is_some(), |this| {
                    this.child(
                        gpui_kit::component::Icon::new(IconName::SquareTerminal)
                            .size_3()
                            .text_color(cx.theme().muted_foreground),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!("terminal-tab-select-{index}")))
                        .cursor_pointer()
                        .max_w_32()
                        .truncate()
                        .text_xs()
                        .text_color(if active {
                            cx.theme().foreground
                        } else {
                            cx.theme().muted_foreground
                        })
                        .when_some(label_hover, |this, command| {
                            this.tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(command.clone())
                                    .build(window, cx)
                            })
                        })
                        .on_click({
                            let this = cx.entity();
                            move |_, _, cx| {
                                this.update(cx, |this, cx| this.select_terminal_tab(index, cx));
                            }
                        })
                        .child(label),
                );
            // G-T2: reveal the tab's launch directory in the platform file
            // manager (the web's `reveal-directory` action; the pane tracks no
            // live cwd, so this is where the shell started the terminal).
            if let Some(directory) = tab.directory.clone() {
                tab_entry = tab_entry.child(
                    Button::new(SharedString::from(format!("terminal-tab-reveal-{index}")))
                        .debug_selector(move || format!("terminal-tab-reveal-{index}"))
                        .icon(IconName::FolderOpen)
                        .xsmall()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.terminal_reveal.clone())
                        .on_click(move |_, _, _cx| {
                            crate::settings::open_path_in_file_manager(std::path::Path::new(
                                &directory,
                            ));
                        }),
                );
            }
            tab_bar = tab_bar.child(
                tab_entry.child(
                    div()
                        .id(SharedString::from(format!("terminal-tab-close-{index}")))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            close_view.update(cx, |this, cx| this.close_terminal_tab(index, cx));
                        })
                        .child(
                            Icon::new(IconName::Close)
                                .size_3()
                                .text_color(cx.theme().muted_foreground),
                        ),
                ),
            );
        }

        div()
            .id("shell-terminal-dock")
            .debug_selector(|| "shell-terminal-dock".to_string())
            .v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            // The seam sits on the edge the divider is on: above the bottom
            // dock, left of the right one.
            .when(dock == TerminalDock::Bottom, |this| this.border_t_1())
            .when(dock == TerminalDock::Right, |this| this.border_l_1())
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(new_tab_button)
                    .child(
                        div()
                            .id("terminal-tabbar-scroll")
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .child(div().flex().items_center().gap_1().child(tab_bar)),
                    )
                    .when(dock == TerminalDock::Bottom, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .max_w_48()
                                .child(active_title),
                        )
                    })
                    .child(
                        Button::new("terminal-close")
                            .icon(IconName::X)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.terminal_close.clone())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_terminal_panel(window, cx)
                            })),
                    ),
            )
            .child(match self.terminals.get(self.active_terminal) {
                Some(tab) => div()
                    .flex_1()
                    .min_h_0()
                    .child(tab.pane.clone())
                    .into_any_element(),
                None => div().flex_1().min_h_0().into_any_element(),
            })
            .into_any_element()
    }

    /// The chat surface: session header, transcript, busy marker, composer.
    /// One per session tab; the session id comes from the active tab.
    pub(super) fn render_chat_column(
        &self,
        session_id: &str,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let chat_area = self.render_chat_area(session_id, text, cx);
        div()
            .debug_selector(|| "chat-column".to_string())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .v_flex()
            // External file drags land anywhere on the chat surface; the
            // highlight only marks the composer card as the drop target.
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths: Vec<PathBuf> = paths.0.to_vec();
                this.attach_image_paths(paths, cx);
            }))
            .child(self.render_chat_header(text, cx))
            .when_some(self.chat_error.clone(), |this, error| {
                this.child(
                    div()
                        .mx_4()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(chat_area)
            .when_some(self.busy_label(text), |this, label| {
                this.child(
                    div()
                        .w_full()
                        .max_w(px(READING_COLUMN_WIDTH))
                        .mx_auto()
                        .px_4()
                        .pb_1()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        // The streaming marker sweeps a highlight across the
                        // label; the component degrades to plain text under
                        // reduced motion.
                        .child(ShimmerText::new(label)),
                )
            })
            .child(self.render_composer(text, cx))
            .into_any_element()
    }

    /// The session tab's main area (R2c): the chat column, plus the right
    /// resource panel when one is docked — a horizontal split whose divider
    /// drags the panel width. The panel is a workspace-scoped preference, so
    /// it reappears on every session tab; with it hidden the chat column
    /// takes the full width. One `children` call: the group *replaces* its
    /// panel list (upstream 0.7.1), so the toggle between one and two panels
    /// is a full re-set — the same rule the terminal dock follows.
    pub(super) fn render_session_main(
        &self,
        session_id: &str,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(kind) = self.resource_panel else {
            return self.render_chat_column(session_id, text, cx);
        };
        let panel = self.render_resource_panel(kind, text, cx);
        // Maximized (G-F8, web `content-only`): the panel fills the scene body
        // and the chat column steps aside; the head's restore button is the
        // way back.
        if self.resource_panel_maximized {
            return panel;
        }
        let chat = self.render_chat_column(session_id, text, cx);
        let sized = || {
            resizable_panel()
                .size(self.panel_width)
                .size_range(px(MIN_RESOURCE_PANEL_WIDTH)..px(MAX_RESOURCE_PANEL_WIDTH))
                .child(panel)
        };
        let panels = match self.resource_panel_side {
            PanelSide::Right => [resizable_panel().child(chat), sized()],
            PanelSide::Left => [sized(), resizable_panel().child(chat)],
        };
        h_resizable("session-panel-split")
            .with_state(&self.panel_split_state())
            .children(panels)
            .into_any_element()
    }

    /// The docked resource panel: a slim header (title, pop-out, close) over
    /// the page body. The pages render unchanged — the same entities the main
    /// scene tabs mount, moved to the docked mount point.
    pub(super) fn render_resource_panel(
        &self,
        kind: ResourcePanelKind,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let title = match kind {
            ResourcePanelKind::Files => text.files_title.clone(),
            ResourcePanelKind::Git => text.git_title.clone(),
        };
        // The two pages are different entity types: each arm wraps its own
        // mount into the same panel-body shape.
        let body = match kind {
            ResourcePanelKind::Files => self.files.clone().map(|page| {
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(page)
                    .into_any_element()
            }),
            ResourcePanelKind::Git => self.git.clone().map(|panel| {
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(panel)
                    .into_any_element()
            }),
        };
        let maximize_label = if self.resource_panel_maximized {
            text.panel_restore.clone()
        } else {
            text.panel_maximize.clone()
        };
        let maximize_icon = if self.resource_panel_maximized {
            IconName::Minimize
        } else {
            IconName::Maximize
        };
        let head = div()
            .debug_selector(|| "resource-panel-head".to_string())
            .h_9()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().px_1().text_sm().font_medium().child(title))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        Button::new("resource-panel-maximize")
                            .icon(maximize_icon)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(maximize_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_resource_panel_maximized(cx);
                            })),
                    )
                    .child(
                        Button::new("resource-panel-swap")
                            .icon(IconName::Replace)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.panel_swap.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.swap_resource_panel_side(cx);
                            })),
                    )
                    .child(
                        Button::new("resource-panel-pop-out")
                            .icon(IconName::ExternalLink)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.panel_pop_out.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pop_out_resource_panel(cx);
                            })),
                    )
                    .child(
                        Button::new("resource-panel-close")
                            .icon(IconName::X)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.panel_close.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_resource_panel(cx);
                            })),
                    ),
            );
        div()
            .debug_selector(|| "resource-panel".to_string())
            .h_full()
            .min_w_0()
            .flex_1()
            .v_flex()
            .overflow_hidden()
            .when(self.resource_panel_side == PanelSide::Left, |this| {
                this.border_r_1()
            })
            .when(self.resource_panel_side == PanelSide::Right, |this| {
                this.border_l_1()
            })
            .border_color(cx.theme().border)
            .child(head)
            .child(match body {
                Some(body) => body,
                None => div().into_any_element(),
            })
            .into_any_element()
    }

    /// The tabless landing face (R2b, the web `WelcomeScene`): the brand line
    /// and slogan stand in for a session, and one input starts one —
    /// submitting creates the session and sends the text as its opening turn
    /// (`create_session_with_first_message`, so the tab opens before the
    /// reply stream starts). The composer card's visual language carries over
    /// so the hand-off to the session composer reads as one surface. The web
    /// typewriter cycles seven phrases; the shell pins the brand line
    /// ("把如果变成真。") — the web reduced-motion form — instead of ticking a
    /// timer through the whole shell every frame.
    pub(super) fn render_welcome_face(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let draft_value = self.draft.read(cx).value().to_string();
        let draft = self.draft.clone();
        let tx = self.event_tx.clone();
        // The welcome card shares the session composer's round send button
        // (web WelcomeScene: one primary circle with an up arrow).
        let send = Button::new("welcome-send")
            .icon(IconName::ArrowUp)
            .with_variant(ButtonVariant::Primary)
            .rounded(ButtonRounded::Size(px(999.)))
            .disabled(draft_value.trim().is_empty())
            .on_click(move |_, window, cx| {
                let body = draft.read(cx).value().to_string();
                if body.trim().is_empty() {
                    return;
                }
                draft.update(cx, |state, cx| state.set_value("", window, cx));
                if let Some(handle) = runtime::runtime_handle() {
                    handle.create_session_with_first_message(body, tx.clone());
                }
            });
        div()
            .debug_selector(|| "welcome-face".to_string())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .debug_selector(|| "welcome-brand".to_string())
                    .v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(text.welcome_title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(28.))
                            .text_color(cx.theme().muted_foreground)
                            .child(text.welcome_slogan.clone()),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "welcome-input-card".to_string())
                    .w_full()
                    .max_w(px(READING_COLUMN_WIDTH))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_2xl())
                    .shadow_sm()
                    .p_2()
                    .child(
                        Textarea::new(&self.draft)
                            .appearance(false)
                            .flex_1()
                            .h(px(COMPOSER_HEIGHT)),
                    )
                    .child(send),
            )
            .into_any_element()
    }

    pub(super) fn render_chat_header(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        match self.active_chat() {
            Some(chat) => {
                // The tab strip names the session and the top bar carries the
                // session chrome (usage, quick actions, right panel), so the
                // in-content header only exists while a hidden child view is
                // shown — the badge names what the pane shows and "back"
                // returns to the parent (M20). The busy label lives above the
                // composer (the shimmer line), not here.
                let Some(child) = chat.child_context.as_ref() else {
                    return div();
                };
                let review_label = child
                    .agent_type
                    .as_deref()
                    .and_then(review_activity_kind)
                    .map(|_| text.sidebar_review_running.clone());
                let label = match (child.kind, child.agent_type.as_deref()) {
                    (ChildKind::Subagent, Some(agent_type)) => {
                        let kind_label =
                            review_label.unwrap_or_else(|| text.subagent_badge.clone());
                        format!("{kind_label} · {agent_type}")
                    }
                    (ChildKind::Subagent, None) => text.subagent_badge.clone(),
                    (ChildKind::Btw, _) => text.btw_badge.clone(),
                };
                let back_label = text.child_back.clone();
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().selection)
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(
                        Button::new("chat-child-back")
                            .label(back_label)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.return_from_child(cx);
                            })),
                    )
            }
            None => div(),
        }
    }

    /// Header quick-actions menu (T2-10, the web SessionFilesBadge actions
    /// section): one entry per enabled `app.ai_experience.quick_actions`
    /// item. Clicking sends the action's prompt as a normal message (web
    /// sends through FlowChatManager.sendMessage with the same gating:
    /// nothing while the session is processing). Hidden when no action is
    /// enabled or no session is open.
    pub(super) fn render_quick_actions_menu(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
            return div().into_any_element();
        };
        let enabled: Vec<(String, String, IconName)> = settings::ShellConfig::try_current(cx)
            .map(|config| config.quick_actions)
            .unwrap_or_default()
            .iter()
            .filter(|action| action.enabled)
            .map(|action| {
                let (label, prompt) = resolve_quick_action_text(action, text);
                let icon = match action.id.as_str() {
                    "commit" => IconName::GitCommitHorizontal,
                    "create_pr" => IconName::GitPullRequest,
                    _ => IconName::Zap,
                };
                (label, prompt, icon)
            })
            .collect();
        if enabled.is_empty() {
            return div().into_any_element();
        }
        let busy = chat.busy || chat.submitting;
        let session_id = chat.session_id.clone();
        let tx = self.event_tx.clone();
        Button::new("chat-quick-actions")
            .icon(IconName::Zap)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.quick_action_title.clone())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                let mut menu = menu;
                for (label, prompt, icon) in &enabled {
                    let prompt = prompt.clone();
                    let session_id = session_id.clone();
                    let tx = tx.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .icon(icon.clone())
                            .disabled(busy)
                            .on_click(move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.send_message(
                                        session_id.clone(),
                                        prompt.clone(),
                                        Vec::new(),
                                        tx.clone(),
                                    );
                                }
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// The composer boost button (web "+" boost): a round ghost button whose
    /// menu carries the shell-capable subset of the web boost items — a new
    /// session and the global search palette. Image and file context keep
    /// their existing entrances (paste, drag, drop): the shell has no native
    /// file picker to back a menu entry, and a dead entry would be a fake.
    pub(super) fn render_composer_boost_menu(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let tx = self.event_tx.clone();
        let view = cx.entity().downgrade();
        let new_session_label = text.sessions_new.clone();
        let search_label = text.search_trigger.clone();
        Button::new("composer-boost")
            .icon(IconName::Plus)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .rounded(ButtonRounded::Size(px(999.)))
            .tooltip(text.sessions_new.clone())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                menu.item(
                    PopupMenuItem::new(new_session_label.clone())
                        .icon(IconName::Plus)
                        .on_click({
                            let tx = tx.clone();
                            move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.create_session(tx.clone());
                                }
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new(search_label.clone())
                        .icon(IconName::Search)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                if let Some(view) = view.upgrade() {
                                    view.update(cx, |shell, cx| {
                                        shell.open_search_overlay(window, cx)
                                    });
                                }
                            }
                        }),
                )
            })
            .into_any_element()
    }

    /// Header usage button: asks the runtime for the session usage report;
    /// the dialog opens when [`RuntimeEvent::SessionUsage`] arrives.
    pub(super) fn render_usage_button(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
            return div().into_any_element();
        };
        let session_id = chat.session_id.clone();
        let tx = self.event_tx.clone();
        let text = i18n::current(cx);
        Button::new("session-usage")
            .icon(IconName::ChartPie)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.usage_title.clone())
            .on_click(
                move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.session_usage(session_id.clone(), tx.clone());
                    }
                },
            )
            .into_any_element()
    }

    /// Model picker for the open session (R2a: moved from the chat header
    /// into the composer's action row, the web end-actions cluster). The
    /// trigger shows the current model and the menu offers every enabled
    /// configured model. The selection is applied through the runtime
    /// (persisted per session), so the trigger only changes after
    /// [`RuntimeEvent::SessionModelChanged`] confirms it.
    pub(super) fn render_model_picker(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
            return div().into_any_element();
        };
        let session_id = chat.session_id.clone();
        let current = chat.model_id.clone();
        let catalog = cx
            .try_global::<runtime::ModelCatalog>()
            .cloned()
            .unwrap_or_default();
        let label = match current.as_deref() {
            Some(id) => catalog
                .models
                .iter()
                .find(|model| model.id == id)
                .map(|model| model.label())
                .unwrap_or_else(|| i18n::model_alias_label(id, cx)),
            None => text.model_default.clone(),
        };
        let options: Vec<(String, String, bool)> = catalog
            .models
            .iter()
            .filter(|model| model.enabled)
            .map(|model| {
                (
                    model.id.clone(),
                    model.label(),
                    current.as_deref() == Some(model.id.as_str()),
                )
            })
            .collect();
        let empty = options.is_empty();
        let tx = self.event_tx.clone();
        Button::new("chat-model-picker")
            .label(label)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .dropdown_caret(true)
            .disabled(empty)
            .tooltip(if empty {
                text.model_picker_empty.clone()
            } else {
                text.model_picker.clone()
            })
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                let mut menu = menu;
                for (id, option_label, checked) in &options {
                    let id = id.clone();
                    let tx = tx.clone();
                    let session_id = session_id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(option_label.clone())
                            .checked(*checked)
                            .on_click(move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.update_session_model(
                                        session_id.clone(),
                                        id.clone(),
                                        tx.clone(),
                                    );
                                }
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
}
