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
    // docked changes panel (R2c) was the first smoke to render them.
    [
        Activity,
        Archive,
        BrainCircuit,
        ChartColumn,
        ListTodo,
        Check,
        Circle,
        CircleDot,
        GitBranch,
        GitCommitHorizontal,
        GitPullRequest,
        House,
        Languages,
        Laptop,
        Plug,
        Shield,
        ShieldAlert,
        Sparkles,
        Square,
        TextWrap,
        Upload,
        Webhook,
        X,
        Zap
    ]
);

/// Asset source registered for the shell window.
pub(crate) struct ShellAssets;

impl AssetSource for ShellAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
