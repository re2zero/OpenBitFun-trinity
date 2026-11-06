//! Leaf foundations of the OpenBitFun GPUI shell: i18n, theming, embedded
//! assets, and the Ely bridge. Nothing here depends on the window shell or the
//! embedded runtime, so every other shell crate may build on it.

pub mod assets;
pub mod ely_support;
pub mod highlight;
pub mod i18n;
pub mod subagent_identity;
pub mod theme;
pub mod theme_openbitfun;
