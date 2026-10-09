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

gpui_kit::assets::icon_assets!(ExtraIcons, [GitBranch, Languages, Shield, TextWrap, X]);

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
