//! System tray icon (StatusNotifierItem) for the shell, plus the
//! launch-at-login toggle.
//!
//! The tray publishes the same [`RuntimeEvent`]s the in-window controls use
//! (show window, check updates, quit) through the process event sender, so
//! the window pump stays the only place that touches gpui state. ksni drives
//! its own D-Bus connection on its own thread — it is deliberately never
//! constructed on the gpui main thread (the desktop's zbus-on-main-thread
//! hazard applies here too) — and a missing StatusNotifierWatcher degrades to
//! a warning: the shell keeps working without a tray.

use std::sync::OnceLock;

use gpui_kit::App;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip, Tray};

use openbitfun_core::service::i18n::LocaleId;

use crate::runtime::RuntimeEvent;

/// The product icon ships with the desktop packaging; the shell embeds the
/// 32×32 hicolor entry so a tray host always has pixels to show even without
/// an installed icon theme.
const ICON_PNG: &[u8] =
    include_bytes!("../../desktop/icons/hicolor/32x32/apps/openbitfun-desktop.png");

/// ksni handle parked once the tray is up, so a locale switch can re-resolve
/// the menu labels.
static TRAY: OnceLock<ksni::blocking::Handle<ShellTray>> = OnceLock::new();

/// Localized tray copy, resolved through the core i18n service before the
/// tray thread starts. Rebuilt by [`apply_locale`] on a locale switch.
#[derive(Debug, Clone)]
struct TrayLabels {
    show_window: String,
    check_updates: String,
    quit: String,
    tooltip: String,
}

impl TrayLabels {
    async fn resolve(locale: LocaleId) -> Self {
        let Some(service) = openbitfun_core::service::i18n::get_global_i18n_service().await else {
            tracing::warn!("i18n service unavailable, keeping English tray copy");
            return Self::english();
        };
        // The service is process-global; pin it to this locale so the keys
        // below resolve in the language the shell just switched to.
        let _ = service.set_locale(locale).await;
        Self {
            show_window: service.t("gpui-shell-tray-show").await,
            check_updates: service.t("gpui-shell-tray-check-updates").await,
            quit: service.t("gpui-shell-tray-quit").await,
            tooltip: service.t("gpui-shell-tray-tooltip").await,
        }
    }

    fn english() -> Self {
        Self {
            show_window: "Show OpenBitFun".to_string(),
            check_updates: "Check for updates".to_string(),
            quit: "Quit OpenBitFun".to_string(),
            tooltip: "OpenBitFun".to_string(),
        }
    }
}

struct ShellTray {
    event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
    labels: TrayLabels,
}

impl ShellTray {
    fn send(&self, event: RuntimeEvent) {
        let _ = self.event_tx.send(event);
    }
}

impl Tray for ShellTray {
    fn id(&self) -> String {
        "openbitfun-gpui-shell".to_string()
    }

    fn title(&self) -> String {
        // Product name, not translated copy.
        "OpenBitFun".to_string()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "OpenBitFun".to_string(),
            description: self.labels.tooltip.clone(),
            ..ToolTip::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        tray_icon().into_iter().collect()
    }

    /// Left click: show (and focus) the main window. There is no per-window
    /// hide in gpui, so the shell's tray behavior is activate-only and the
    /// close-button behavior decides what "closing" means.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(RuntimeEvent::TrayShowWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: self.labels.show_window.clone(),
                activate: Box::new(|tray: &mut Self| tray.send(RuntimeEvent::TrayShowWindow)),
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: self.labels.check_updates.clone(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send(RuntimeEvent::UpdateCheckRequested);
                }),
                ..StandardItem::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: self.labels.quit.clone(),
                activate: Box::new(|tray: &mut Self| tray.send(RuntimeEvent::QuitRequested)),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

/// Decodes the embedded brand PNG into the ARGB32 (network byte order)
/// pixmap the SNI spec carries. Decoded once; a decode failure leaves the
/// tray iconless rather than failing the shell.
fn tray_icon() -> Option<Icon> {
    static ARGB: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let data = ARGB.get_or_init(|| {
        let image::DynamicImage::ImageRgba8(rgba) =
            image::load_from_memory(ICON_PNG).expect("embedded tray icon decodes")
        else {
            return None;
        };
        let mut argb = Vec::with_capacity(rgba.as_raw().len());
        for pixel in rgba.pixels() {
            argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
        }
        Some(argb)
    });
    let data = data.as_ref()?;
    Some(Icon {
        width: 32,
        height: 32,
        data: data.clone(),
    })
}

/// Spawns the tray service. Called once from the runtime bootstrap after the
/// config (and with it the i18n service) is up; the initial D-Bus connect
/// runs on a throwaway thread so neither the gpui main thread nor a tokio
/// worker ever owns the zbus context.
pub(crate) async fn spawn(
    locale: LocaleId,
    event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
) {
    let labels = TrayLabels::resolve(locale).await;
    std::thread::spawn(move || {
        let tray = ShellTray { event_tx, labels };
        match tray.spawn() {
            Ok(handle) => {
                let _ = TRAY.set(handle);
                tracing::info!("Tray icon registered");
            }
            Err(error) => {
                tracing::warn!("Tray icon unavailable ({}); the shell keeps running", error);
            }
        }
    });
}

/// Re-resolves the tray labels after a locale switch. A tray that never came
/// up is a no-op.
pub(crate) fn apply_locale(locale: LocaleId, cx: &App) {
    let Some(handle) = TRAY.get() else {
        return;
    };
    let labels = cx
        .foreground_executor()
        .block_on(TrayLabels::resolve(locale));
    handle.update(|tray| tray.labels = labels);
}

/// Launch-at-login: the same user-level autostart entry the desktop manages
/// (`~/.config/autostart` on Linux), under the shell's own entry name so the
/// two hosts never fight over one file. `None` means the platform backend
/// refused (unreadable executable, unsupported desktop); the toggle then
/// reports failure instead of pretending.
pub(crate) mod autostart {
    const APP_NAME: &str = "openbitfun-gpui-shell";

    pub(crate) fn backend() -> Result<auto_launch::AutoLaunch, String> {
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot resolve the shell executable: {error}"))?;
        auto_launch::AutoLaunchBuilder::new()
            .set_app_name(APP_NAME)
            .set_app_path(exe.display().to_string().as_str())
            .build()
            .map_err(|error| format!("autostart backend unavailable: {error}"))
    }

    pub(crate) fn is_enabled() -> Option<bool> {
        match backend() {
            Ok(launch) => launch.is_enabled().ok(),
            Err(reason) => {
                tracing::warn!("Cannot read launch-at-login state: {reason}");
                None
            }
        }
    }

    pub(crate) fn set_enabled(enable: bool) -> Result<(), String> {
        let launch = backend()?;
        let result = if enable {
            launch.enable()
        } else {
            launch.disable()
        };
        result.map_err(|error| format!("failed to write the autostart entry: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray() -> ShellTray {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        ShellTray {
            event_tx: tx,
            labels: TrayLabels::english(),
        }
    }

    #[test]
    fn tray_menu_carries_the_three_actions() {
        let tray = tray();
        let menu = tray.menu();
        assert_eq!(menu.len(), 4, "show, check updates, separator, quit");
        // The labels resolve through the English fallback when the i18n
        // service is unavailable; the tray must still be buildable.
        assert_eq!(tray.labels.show_window, "Show OpenBitFun");
        assert_eq!(tray.id(), "openbitfun-gpui-shell");
        assert_eq!(tray.title(), "OpenBitFun");
    }

    #[test]
    fn tray_activation_publishes_window_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tray = ShellTray {
            event_tx: tx,
            labels: TrayLabels::english(),
        };
        // Left click shows the window.
        Tray::activate(&mut tray, 0, 0);
        assert!(matches!(rx.try_recv(), Ok(RuntimeEvent::TrayShowWindow)));
        // The menu items ride the same pump; drive them through the tray
        // state the callbacks receive.
        tray.send(RuntimeEvent::UpdateCheckRequested);
        tray.send(RuntimeEvent::QuitRequested);
        assert!(matches!(
            rx.try_recv(),
            Ok(RuntimeEvent::UpdateCheckRequested)
        ));
        assert!(matches!(rx.try_recv(), Ok(RuntimeEvent::QuitRequested)));
    }

    #[test]
    fn tray_icon_decodes_the_embedded_brand_png() {
        let icon = tray_icon().expect("the embedded 32x32 hicolor icon decodes");
        assert_eq!(icon.width, 32);
        assert_eq!(icon.height, 32);
        assert_eq!(icon.data.len(), 32 * 32 * 4, "ARGB32, four bytes per pixel");
    }

    #[test]
    fn autostart_backend_builds_without_touching_the_platform() {
        // Building the backend only resolves the executable; it must not
        // read or write the autostart directory.
        let _ = crate::tray::autostart::backend().expect("the backend builds");
    }
}
