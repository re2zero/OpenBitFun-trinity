//! Registry-driven top tabs (R1a): the window's main area is a tab bar of
//! session tabs and scene tabs, mirroring the web-ui desktop `SceneBar`.
//!
//! The scene table is the structural seam for plugin-contributed surfaces
//! (R7): builtin scenes register through the same [`SceneDef`] rows a future
//! plugin contribution will use, so the shell never hardcodes a scene list in
//! its chrome. Rendering stays a shell-side match over [`SceneId`]; a plugin
//! scene (declared UI schema) will dispatch through its own arm once the R7
//! contract lands.

use gpui_kit::assets::IconName;

use crate::i18n::ShellText;

/// A registry-driven scene identity. Builtin ids are static; the R7 plugin
/// contract will extend this table with contributed scene rows rather than
/// growing the match arms (rendering then dispatches on the contribution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SceneId {
    Settings,
    Git,
    Insights,
    Tasks,
    Files,
    Trinity,
}

/// A registry-driven scene: one row of the scene table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SceneDef {
    pub id: SceneId,
    pub icon: IconName,
    /// Whether the scene owns a sidebar nav row. R1b converged the nav to the
    /// desktop top-scene shape (assistant / trinity / task board), so only
    /// workbench scenes that still lack their R2c home keep rows out — they
    /// open through the global search overlay's quick actions instead, the
    /// same programmatic path the desktop uses. The table rows outlive nav
    /// membership.
    pub in_nav: bool,
}

/// Built-in scenes. Settings is reachable from the footer gear and the scene
/// tab it opens, not from a nav row; Files/Git/Insights move to their desktop
/// homes in R2c and until then open via the search overlay's quick actions.
pub(crate) fn builtin_scenes() -> Vec<SceneDef> {
    use SceneId::*;
    vec![
        SceneDef {
            id: Git,
            icon: IconName::GitBranch,
            in_nav: false,
        },
        SceneDef {
            id: Files,
            icon: IconName::FolderOpen,
            in_nav: false,
        },
        SceneDef {
            id: Insights,
            icon: IconName::ChartPie,
            in_nav: false,
        },
        SceneDef {
            id: Tasks,
            icon: IconName::Calendar,
            in_nav: true,
        },
        SceneDef {
            id: Trinity,
            icon: IconName::BrainCircuit,
            in_nav: true,
        },
        SceneDef {
            id: Settings,
            icon: IconName::Settings,
            in_nav: false,
        },
    ]
}

/// What one top tab holds. Session tabs are per-session (several can be open
/// at once, like the desktop); scene tabs are singletons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabKind {
    Session(String),
    Scene(SceneId),
}

/// The window's open tabs plus the active index. Pure state, so the
/// activation/close rules are unit-testable without a window.
#[derive(Debug, Default)]
pub(crate) struct TabsState {
    tabs: Vec<TabKind>,
    active: Option<usize>,
}

impl TabsState {
    /// Opens the tab (activating the existing one when already present) and
    /// returns its index. Every open activates.
    pub(crate) fn open(&mut self, kind: TabKind) -> usize {
        if let Some(index) = self.index_of(&kind) {
            self.active = Some(index);
            return index;
        }
        self.tabs.push(kind);
        let index = self.tabs.len() - 1;
        self.active = Some(index);
        index
    }

    /// Closes the tab at `index`; the activation moves to the right neighbor
    /// (same position) or, at the tail, to the left neighbor. Returns the
    /// removed kind when the index was valid.
    pub(crate) fn close(&mut self, index: usize) -> Option<TabKind> {
        if index >= self.tabs.len() {
            return None;
        }
        let removed = self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = None;
        } else if let Some(active) = self.active {
            if active == index {
                self.active = Some(active.min(self.tabs.len() - 1));
            } else if active > index {
                self.active = Some(active - 1);
            }
        }
        Some(removed)
    }

    pub(crate) fn activate(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = Some(index);
        }
    }

    pub(crate) fn index_of(&self, kind: &TabKind) -> Option<usize> {
        self.tabs.iter().position(|tab| tab == kind)
    }

    pub(crate) fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub(crate) fn active_kind(&self) -> Option<&TabKind> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    pub(crate) fn kinds(&self) -> &[TabKind] {
        &self.tabs
    }

    /// Whether the active tab is the given session.
    pub(crate) fn is_active_session(&self, session_id: &str) -> bool {
        matches!(self.active_kind(), Some(TabKind::Session(id)) if id == session_id)
    }

    /// The session id of the active tab, when it is a session tab.
    pub(crate) fn active_session_id(&self) -> Option<&str> {
        match self.active_kind() {
            Some(TabKind::Session(id)) => Some(id),
            _ => None,
        }
    }
}

/// Tab label resolution happens at render time from already-composed strings
/// (the render path never touches the i18n service).
pub(crate) fn scene_title(id: SceneId, text: &ShellText) -> &str {
    match id {
        SceneId::Settings => &text.settings_title,
        SceneId::Git => &text.git_title,
        SceneId::Insights => &text.insights_title,
        SceneId::Tasks => &text.tasks_title,
        SceneId::Files => &text.files_title,
        SceneId::Trinity => &text.trinity_title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> TabKind {
        TabKind::Session(id.to_string())
    }

    #[test]
    fn open_activates_and_deduplicates() {
        let mut tabs = TabsState::default();
        assert_eq!(tabs.open(session("a")), 0);
        assert_eq!(tabs.open(TabKind::Scene(SceneId::Git)), 1);
        // Re-opening an existing tab activates it without duplicating it.
        assert_eq!(tabs.open(session("a")), 0);
        assert_eq!(tabs.kinds().len(), 2);
        assert_eq!(tabs.active_session_id(), Some("a"));
    }

    #[test]
    fn close_moves_activation_to_the_right_neighbor_then_left() {
        let mut tabs = TabsState::default();
        for id in ["a", "b", "c", "d"] {
            tabs.open(session(id));
        }
        // Closing an inactive tab keeps the active tab (d), index shifted.
        tabs.close(0);
        assert_eq!(tabs.active_session_id(), Some("d"));
        // Closing the active tail tab lands on the left neighbor (c).
        tabs.close(2);
        assert_eq!(tabs.active_session_id(), Some("c"));
        // Draining closes down to the empty state.
        tabs.close(1);
        assert_eq!(tabs.active_session_id(), Some("b"));
        tabs.close(0);
        assert_eq!(tabs.active_index(), None);
        assert!(tabs.kinds().is_empty());
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_index_aligned() {
        let mut tabs = TabsState::default();
        for id in ["a", "b", "c"] {
            tabs.open(session(id));
        }
        tabs.activate(0);
        tabs.close(1);
        assert_eq!(tabs.active_session_id(), Some("a"));
        assert_eq!(tabs.kinds(), &[session("a"), session("c")]);
    }

    #[test]
    fn scene_tabs_are_singletons() {
        let mut tabs = TabsState::default();
        tabs.open(TabKind::Scene(SceneId::Trinity));
        tabs.open(session("a"));
        tabs.open(TabKind::Scene(SceneId::Trinity));
        assert_eq!(tabs.kinds().len(), 2);
        assert_eq!(tabs.active_kind(), Some(&TabKind::Scene(SceneId::Trinity)));
    }

    #[test]
    fn builtin_scenes_cover_every_scene_id() {
        // The scene table is the single source of scene metadata; every
        // SceneId variant must have exactly one row so titles and icons
        // always resolve.
        let rows = builtin_scenes();
        let expected = [
            SceneId::Settings,
            SceneId::Git,
            SceneId::Insights,
            SceneId::Tasks,
            SceneId::Files,
            SceneId::Trinity,
        ];
        for id in expected {
            assert_eq!(rows.iter().filter(|row| row.id == id).count(), 1);
        }
        assert_eq!(rows.len(), expected.len());
    }
}
