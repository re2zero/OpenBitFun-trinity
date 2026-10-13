//! Shell asset source: the component library's default icon bundle plus the
//! icons the shell references that the default bundle does not embed.
//!
//! `gpui-kit` ships the full Lucide catalog but only embeds a curated default
//! set (`default-icons.txt`, 106 icons); every other `IconName` resolves to a
//! missing asset and logs an error at render time. The shell needs a handful
//! outside that set, so it composes them in the same way the upstream
//! `extra_assets` example does.

use std::borrow::Cow;

use gpui_kit::AssetSource;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    // Sparkles is outside the component default bundle (the skills page and
    // the M23 chat/nav entries use it); House/Webhook/ChartColumn/Activity
    // (settings nav) and Plug (manage pages) likewise. Without these the SVG
    // resolves missing at render time. GitCommitHorizontal/GitPullRequest/Zap
    // are the quick-actions menu glyphs (T2-10); Lucide renamed `commit` to
    // `git-commit-horizontal`. Laptop/ShieldAlert are the composer strip's
    // local-execution fact and full-access permission ramp (R2a).
    // ListTodo/Upload are the git header's stage-all and push buttons; the
    // docked changes panel (R2c) was the first smoke to render them. Trash is
    // the worktree row's remove button (git.rs) — first rendered by the R3
    // settings worktrees redirect landing on the Worktree view.
    // LayoutDashboard is the plugin-contributed scene's tab glyph (R7b
    // fixture; contribution icon names resolve through the closed icon set).
    // Puzzle is the extensions nav row (the desktop 扩展与兼容 scene).
    // FilePlus/Pencil/Wrench/Users/Route are the rich transcript's tool-family
    // and group glyphs (M24).
    [
        Activity,
        Archive,
        BrainCircuit,
        ChartColumn,
        LayoutDashboard,
        ListTodo,
        Check,
        Circle,
        CircleDot,
        FilePlus,
        GitBranch,
        GitCommitHorizontal,
        GitPullRequest,
        House,
        Languages,
        Laptop,
        Pencil,
        Plug,
        Puzzle,
        Route,
        Shield,
        ShieldAlert,
        Sparkles,
        Square,
        TextWrap,
        Trash,
        Upload,
        Users,
        Webhook,
        Wrench,
        X,
        Zap
    ]
);

/// Subagent avatar artwork, embedded so a packaged shell renders the same
/// identities as the web UI (which bundles these same twenty files from
/// `src/web-ui/src/flow_chat/assets/subagent-avatars`). Which file a run uses
/// is a hash of its child session id — see [`crate::subagent_identity`].
const SUBAGENT_AVATARS: [(&str, &[u8]); 20] = [
    (
        "subagent-avatars/robot-01.svg",
        include_bytes!("../assets/subagent-avatars/robot-01.svg"),
    ),
    (
        "subagent-avatars/robot-02.svg",
        include_bytes!("../assets/subagent-avatars/robot-02.svg"),
    ),
    (
        "subagent-avatars/robot-03.svg",
        include_bytes!("../assets/subagent-avatars/robot-03.svg"),
    ),
    (
        "subagent-avatars/robot-04.svg",
        include_bytes!("../assets/subagent-avatars/robot-04.svg"),
    ),
    (
        "subagent-avatars/robot-05.svg",
        include_bytes!("../assets/subagent-avatars/robot-05.svg"),
    ),
    (
        "subagent-avatars/robot-06.svg",
        include_bytes!("../assets/subagent-avatars/robot-06.svg"),
    ),
    (
        "subagent-avatars/robot-07.svg",
        include_bytes!("../assets/subagent-avatars/robot-07.svg"),
    ),
    (
        "subagent-avatars/robot-08.svg",
        include_bytes!("../assets/subagent-avatars/robot-08.svg"),
    ),
    (
        "subagent-avatars/robot-09.svg",
        include_bytes!("../assets/subagent-avatars/robot-09.svg"),
    ),
    (
        "subagent-avatars/robot-10.svg",
        include_bytes!("../assets/subagent-avatars/robot-10.svg"),
    ),
    (
        "subagent-avatars/robot-11.svg",
        include_bytes!("../assets/subagent-avatars/robot-11.svg"),
    ),
    (
        "subagent-avatars/robot-12.svg",
        include_bytes!("../assets/subagent-avatars/robot-12.svg"),
    ),
    (
        "subagent-avatars/robot-13.svg",
        include_bytes!("../assets/subagent-avatars/robot-13.svg"),
    ),
    (
        "subagent-avatars/robot-14.svg",
        include_bytes!("../assets/subagent-avatars/robot-14.svg"),
    ),
    (
        "subagent-avatars/robot-15.svg",
        include_bytes!("../assets/subagent-avatars/robot-15.svg"),
    ),
    (
        "subagent-avatars/robot-16.svg",
        include_bytes!("../assets/subagent-avatars/robot-16.svg"),
    ),
    (
        "subagent-avatars/robot-17.svg",
        include_bytes!("../assets/subagent-avatars/robot-17.svg"),
    ),
    (
        "subagent-avatars/robot-18.svg",
        include_bytes!("../assets/subagent-avatars/robot-18.svg"),
    ),
    (
        "subagent-avatars/robot-19.svg",
        include_bytes!("../assets/subagent-avatars/robot-19.svg"),
    ),
    (
        "subagent-avatars/robot-20.svg",
        include_bytes!("../assets/subagent-avatars/robot-20.svg"),
    ),
];

/// Asset source registered for the shell window.
pub(crate) struct ShellAssets;

impl AssetSource for ShellAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = SUBAGENT_AVATARS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(path))
        {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.extend(
            SUBAGENT_AVATARS
                .iter()
                .map(|(name, _)| (*name).into())
                .filter(|name: &gpui_kit::SharedString| name.starts_with(path)),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
