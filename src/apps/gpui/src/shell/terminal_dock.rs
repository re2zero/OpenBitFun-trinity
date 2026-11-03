// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// Which edge the terminal panel docks to (G-T1): the shared
/// `terminal.terminal_panel_position` preference. The web hosts the `right`
/// position inside its aux pane; the shell's aux pane is the files/changes
/// panel, so the terminal docks as the scene body's own right column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalDock {
    Bottom,
    Right,
}

impl TerminalDock {
    /// Resolves the shared value; anything but `right` (including the empty
    /// string an older config may carry) keeps the shell's bottom dock.
    pub(super) fn from_shared(raw: &str) -> Self {
        if raw == "right" {
            Self::Right
        } else {
            Self::Bottom
        }
    }

    /// The size tier a panel of this extent sits in (web
    /// `getPanelDisplayMode`).
    pub(super) fn tier(self, extent: Pixels) -> TerminalTier {
        let extent = f32::from(extent);
        if extent < TERMINAL_COMPACT_THRESHOLD {
            TerminalTier::Compact
        } else if extent >= TERMINAL_EXPANDED_THRESHOLD {
            TerminalTier::Expanded
        } else {
            TerminalTier::Comfortable
        }
    }
}

/// Which side of the chat column the resource panel docks to (G-F8, web
/// `SplitView`'s `secondarySide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    /// The other side — what the panel head's swap action selects.
    pub(super) fn toggled(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// The terminal panel's display tiers the divider's double click cycles
/// through (web `PanelDisplayMode` collapsed/compact/comfortable/expanded;
/// `collapsed` is the closed panel, which the cycle leaves to the close
/// button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalTier {
    Compact,
    Comfortable,
    Expanded,
}

impl TerminalTier {
    /// The next tier a double click selects (web `getNextMode`: compact and
    /// collapsed go to comfortable, comfortable to expanded, expanded back to
    /// comfortable).
    pub(super) fn next(self) -> Self {
        match self {
            Self::Compact => Self::Comfortable,
            Self::Comfortable => Self::Expanded,
            Self::Expanded => Self::Comfortable,
        }
    }

    pub(super) fn extent(self) -> Pixels {
        px(match self {
            Self::Compact => TERMINAL_TIER_COMPACT,
            Self::Comfortable => TERMINAL_TIER_COMFORTABLE,
            Self::Expanded => TERMINAL_TIER_EXPANDED,
        })
    }
}

/// Bounds of the session's right resource panel (R2c), dragged by the divider
/// left of it; `DEFAULT` is the pre-drag width.
pub(super) const MIN_RESOURCE_PANEL_WIDTH: f32 = 240.;
pub(super) const MAX_RESOURCE_PANEL_WIDTH: f32 = 720.;
pub(super) const DEFAULT_RESOURCE_PANEL_WIDTH: f32 = 360.;

/// One terminal panel tab (M17): a pane owning its PTY plus the fallback
/// tab label (a profile's name, or the localized default-shell label; the
/// pane's OSC title wins once the shell sets one).
#[derive(Clone)]
pub(super) struct TerminalTab {
    pub(super) pane: Entity<TerminalPane>,
    pub(super) label: SharedString,
    /// G-T2: the launch directory — the target of the tab's "show in file
    /// manager" action (the pane tracks no live cwd, so the reveal uses where
    /// the shell started it).
    pub(super) directory: Option<String>,
    /// G-T2: the startup command a profile tab ran (the web's badge); `None`
    /// for a plain default-shell tab.
    pub(super) startup_command: Option<String>,
}

impl ShellWindow {
    /// Opens or closes the built-in terminal panel. First open creates the
    /// default tab (and its PTY); close shuts every tab's PTY down and drops
    /// the panes.
    pub(super) fn toggle_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_open {
            self.terminal_open = false;
            // Remember the dragged extent so the next open docks at it instead
            // of the default; the second slot is the terminal panel.
            self.remember_terminal_extent(cx);
            self.close_all_terminal_tabs(cx);
        } else {
            self.open_terminal_panel(window, cx);
        }
        cx.notify();
    }

    /// Shuts every tab's PTY down and drops the panes (panel close, workspace
    /// switch).
    pub(super) fn close_all_terminal_tabs(&mut self, cx: &mut Context<Self>) {
        for tab in self.terminals.drain(..) {
            tab.pane.update(cx, |pane, _| pane.close());
        }
        self.active_terminal = 0;
    }

    /// Where the terminal docks (G-T1): the shared
    /// `terminal.terminal_panel_position` preference the desktop and web
    /// write; a missing config keeps the shell's bottom dock.
    pub(super) fn terminal_dock(&self, cx: &App) -> TerminalDock {
        settings::ShellConfig::try_current(cx)
            .map(|config| TerminalDock::from_shared(&config.terminal.panel_position))
            .unwrap_or(TerminalDock::Bottom)
    }

    /// The docked panel's split state for the active dock.
    pub(super) fn terminal_dock_state(&self, cx: &App) -> Entity<ResizableState> {
        match self.terminal_dock(cx) {
            TerminalDock::Bottom => self.dock.clone(),
            TerminalDock::Right => self.dock_h.clone(),
        }
    }

    /// Remembers the docked panel's current extent so the next open lands on
    /// it (each dock keeps its own).
    pub(super) fn remember_terminal_extent(&mut self, cx: &Context<Self>) {
        let dock = self.terminal_dock(cx);
        let state = self.terminal_dock_state(cx);
        if let Some(size) = state.read(cx).sizes().get(1) {
            let size = (*size).clamp(px(MIN_TERMINAL_HEIGHT), px(MAX_TERMINAL_HEIGHT));
            match dock {
                TerminalDock::Bottom => self.terminal_height = size,
                TerminalDock::Right => self.terminal_width = size,
            }
        }
    }

    /// Cycles the docked terminal through the size tiers — the divider's
    /// double click (web `handleBottomDoubleClick` → `getNextMode`). Compact
    /// and comfortable go up one tier, expanded comes back to comfortable.
    pub(super) fn cycle_terminal_size_tier(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.terminal_open {
            return;
        }
        let dock = self.terminal_dock(cx);
        let state = self.terminal_dock_state(cx);
        let fallback = match dock {
            TerminalDock::Bottom => self.terminal_height,
            TerminalDock::Right => self.terminal_width,
        };
        let extent = state.read(cx).sizes().get(1).copied().unwrap_or(fallback);
        let target = dock.tier(extent).next().extent();
        state.update(cx, |state, cx| state.resize_panel(1, target, window, cx));
        match dock {
            TerminalDock::Bottom => self.terminal_height = target,
            TerminalDock::Right => self.terminal_width = target,
        }
        tracing::info!("Terminal panel tier -> {target:?}");
        cx.notify();
    }

    /// Opens the terminal panel, creating the default tab (and its PTY) on
    /// first use.
    pub(super) fn open_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_open = true;
        if self.terminals.is_empty() {
            let tab = self.spawn_terminal_tab(None, window, cx);
            self.terminals.push(tab);
            self.active_terminal = 0;
        }
        cx.notify();
    }

    /// One pane launch: `None` means the default shell in the active
    /// workspace; a profile overrides the working directory and types its
    /// startup command once the PTY is up.
    pub(super) fn spawn_terminal_tab(
        &mut self,
        profile: Option<openbitfun_core::service::config::types::TerminalProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TerminalTab {
        let tokio = runtime::runtime_handle()
            .map(|handle| handle.tokio_handle())
            .expect("runtime handle ready when terminal opens");
        // The pane starts from the shared terminal settings: the configured
        // shell command and scrollback are read here, font preferences are
        // also re-applied on every render below.
        let terminal = settings::ShellConfig::try_current(cx)
            .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale))
            .terminal;
        // The PTY starts in the profile's directory, else the active
        // workspace, so the terminal follows a workspace switch instead of
        // the process launch directory.
        let start_dir = profile
            .as_ref()
            .filter(|p| !p.working_directory.trim().is_empty())
            .map(|p| p.working_directory.clone())
            .or_else(|| {
                runtime::runtime_handle()
                    .map(|handle| handle.workspace().root_path.to_string_lossy().to_string())
            });
        let initial_command = profile
            .as_ref()
            .map(|p| p.startup_command.clone())
            .filter(|command| !command.trim().is_empty());
        let label = profile
            .as_ref()
            .map(|p| SharedString::from(p.name.clone()))
            .unwrap_or_else(|| i18n::current(cx).terminal_new_tab_default.clone().into());
        let directory = start_dir.clone();
        let startup_command = initial_command.clone();
        let pane = cx
            .new(|cx| TerminalPane::new(tokio, &terminal, start_dir, initial_command, window, cx));
        TerminalTab {
            pane,
            label,
            directory,
            startup_command,
        }
    }

    /// Opens a new panel tab: the default shell when `profile_id` is `None`,
    /// else the configured profile with its directory and startup command.
    pub(super) fn open_terminal_tab(
        &mut self,
        profile_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile = profile_id.and_then(|id| {
            settings::ShellConfig::current(cx)
                .terminal
                .profiles
                .into_iter()
                .find(|p| p.id == id)
        });
        let tab = self.spawn_terminal_tab(profile, window, cx);
        self.terminals.push(tab);
        self.active_terminal = self.terminals.len() - 1;
        self.terminal_open = true;
        cx.notify();
    }

    /// Closes one tab: its PTY is shut down and the active index stays on a
    /// sensible neighbor. Closing the last tab closes the whole panel (the
    /// dragged dock height is remembered, same as the panel toggle).
    pub(super) fn close_terminal_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.terminals.len() {
            return;
        }
        let tab = self.terminals.remove(index);
        tab.pane.update(cx, |pane, _| pane.close());
        if self.terminals.is_empty() {
            self.remember_terminal_extent(cx);
            self.terminal_open = false;
            self.active_terminal = 0;
        } else {
            if self.active_terminal >= self.terminals.len() {
                self.active_terminal = self.terminals.len() - 1;
            } else if index < self.active_terminal {
                self.active_terminal -= 1;
            }
        }
        cx.notify();
    }

    /// Focuses one tab.
    pub(super) fn select_terminal_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.terminals.len() {
            self.active_terminal = index;
            cx.notify();
        }
    }
}

/// The dock divider's appearance (G-T1): the kit's own handle plus the web's
/// double-click size cycle and its "drag to resize | double-click to switch
/// mode | current: X" tooltip (`layout.resizer.title`). The kit renderer draws
/// the visible part only — the hit band, cursor and drag stay with the handle —
/// so the cycle rides a wrapper that covers the same box.
pub(super) fn terminal_dock_handle_appearance(
    view: gpui_kit::WeakEntity<ShellWindow>,
    dock: TerminalDock,
    tier: TerminalTier,
    text: &ShellText,
) -> gpui_kit::base::ResizeHandleRenderer {
    let inner = gpui_kit::component::resize_handle_appearance();
    let text = text.clone();
    Rc::new(move |handle, window, cx| {
        let element = inner(handle, window, cx)?;
        let view = view.clone();
        let text = text.clone();
        let title = i18n::terminal_resizer_title(&i18n::terminal_tier_label(tier, &text), cx);
        Some(
            div()
                .id(SharedString::from(format!(
                    "shell-terminal-dock-handle-{dock:?}"
                )))
                .absolute()
                .inset_0()
                .on_click(move |event, window, cx| {
                    // The web cycles tiers on a double click; gpui reports the
                    // count on the ordinary click event.
                    if event.click_count() < 2 {
                        return;
                    }
                    let Some(view) = view.upgrade() else {
                        return;
                    };
                    view.update(cx, |this, cx| this.cycle_terminal_size_tier(window, cx));
                })
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                })
                .child(element)
                .into_any_element(),
        )
    })
}
