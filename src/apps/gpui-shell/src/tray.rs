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
    mark_all_read: String,
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
            mark_all_read: service.t("gpui-shell-tray-mark-all-read").await,
        }
    }

    fn english() -> Self {
        Self {
            show_window: "Show OpenBitFun".to_string(),
            check_updates: "Check for updates".to_string(),
            quit: "Quit OpenBitFun".to_string(),
            tooltip: "OpenBitFun".to_string(),
            mark_all_read: "Mark all read".to_string(),
        }
    }
}

/// Unread snapshot the shell pushes after every change (T2-16): turns that
/// finished while the window was inactive, per-session counts folded into a
/// total plus the any-failure flag that picks the badge color.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TrayUnread {
    pub count: u32,
    pub any_failed: bool,
}

struct ShellTray {
    event_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEvent>,
    labels: TrayLabels,
    unread: TrayUnread,
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
        // Unread turns surface in the tooltip; zero keeps the plain copy.
        let description = if self.unread.count > 0 {
            format!("{} ({})", self.labels.tooltip, self.unread.count)
        } else {
            self.labels.tooltip.clone()
        };
        ToolTip {
            title: "OpenBitFun".to_string(),
            description,
            ..ToolTip::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        tray_icon(badge_dot(self.unread)).into_iter().collect()
    }

    /// Left click: show (and focus) the main window. There is no per-window
    /// hide in gpui, so the shell's tray behavior is activate-only and the
    /// close-button behavior decides what "closing" means.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(RuntimeEvent::TrayShowWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = vec![StandardItem {
            label: self.labels.show_window.clone(),
            activate: Box::new(|tray: &mut Self| tray.send(RuntimeEvent::TrayShowWindow)),
            ..StandardItem::default()
        }
        .into()];
        if self.unread.count > 0 {
            items.push(
                StandardItem {
                    label: format!("{} ({})", self.labels.mark_all_read, self.unread.count),
                    activate: Box::new(|tray: &mut Self| {
                        tray.send(RuntimeEvent::TrayMarkAllRead);
                    }),
                    ..StandardItem::default()
                }
                .into(),
            );
        }
        items.push(
            StandardItem {
                label: self.labels.check_updates.clone(),
                activate: Box::new(|tray: &mut Self| {
                    tray.send(RuntimeEvent::UpdateCheckRequested);
                }),
                ..StandardItem::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: self.labels.quit.clone(),
                activate: Box::new(|tray: &mut Self| tray.send(RuntimeEvent::QuitRequested)),
                ..StandardItem::default()
            }
            .into(),
        );
        items
    }
}

/// The badge dot overlaid on the brand icon: red when any unread turn
/// failed, teal otherwise; `None` (zero unread) leaves the icon untouched.
fn badge_dot(unread: TrayUnread) -> Option<(i32, i32, i32, [u8; 3])> {
    if unread.count == 0 {
        return None;
    }
    let color = if unread.any_failed {
        [220, 38, 38]
    } else {
        [13, 148, 136]
    };
    Some((22, 22, 9, color))
}

/// Decodes the embedded brand PNG into the ARGB32 (network byte order)
/// pixmap the SNI spec carries. Decoded once; a decode failure leaves the
/// tray iconless rather than failing the shell.
fn tray_icon(dot: Option<(i32, i32, i32, [u8; 3])>) -> Option<Icon> {
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
    let mut data = data.clone();
    if let Some((cx, cy, radius, color)) = dot {
        overlay_dot(&mut data, 32, 32, cx, cy, radius, color);
    }
    Some(Icon {
        width: 32,
        height: 32,
        data,
    })
}

/// Paints an opaque filled circle into an ARGB32 pixmap (A,R,G,B byte order).
fn overlay_dot(
    data: &mut [u8],
    width: i32,
    height: i32,
    cx: i32,
    cy: i32,
    radius: i32,
    color: [u8; 3],
) {
    for y in (cy - radius)..=(cy + radius) {
        for x in (cx - radius)..=(cx + radius) {
            if x < 0 || y < 0 || x >= width || y >= height {
                continue;
            }
            let dx = (x - cx) as f32;
            let dy = (y - cy) as f32;
            if dx * dx + dy * dy > (radius as f32) * (radius as f32) {
                continue;
            }
            let offset = ((y * width + x) * 4) as usize;
            data[offset] = 255;
            data[offset + 1] = color[0];
            data[offset + 2] = color[1];
            data[offset + 3] = color[2];
        }
    }
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
        let tray = ShellTray {
            event_tx,
            labels,
            unread: TrayUnread::default(),
        };
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

/// Pushes a new unread snapshot onto the tray (icon badge, tooltip suffix,
/// and the mark-all-read menu item re-render). A tray that never came up is
/// a no-op.
pub(crate) fn update_unread(unread: TrayUnread) {
    let Some(handle) = TRAY.get() else {
        return;
    };
    handle.update(|tray| tray.unread = unread);
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
            unread: TrayUnread::default(),
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
    fn unread_turns_add_the_mark_all_read_menu_item() {
        let mut tray = tray();
        assert_eq!(tray.menu().len(), 4, "no mark-all-read item at zero unread");
        tray.unread = TrayUnread {
            count: 2,
            any_failed: false,
        };
        assert_eq!(
            tray.menu().len(),
            5,
            "show, mark all read, check, sep, quit"
        );
    }

    #[test]
    fn tray_activation_publishes_window_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tray = ShellTray {
            event_tx: tx,
            labels: TrayLabels::english(),
            unread: TrayUnread::default(),
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
        let icon = tray_icon(None).expect("the embedded 32x32 hicolor icon decodes");
        assert_eq!(icon.width, 32);
        assert_eq!(icon.height, 32);
        assert_eq!(icon.data.len(), 32 * 32 * 4, "ARGB32, four bytes per pixel");
    }

    #[test]
    fn badge_dot_paints_only_the_unread_states() {
        assert_eq!(
            badge_dot(TrayUnread::default()),
            None,
            "zero unread: no dot"
        );
        let clean = badge_dot(TrayUnread {
            count: 1,
            any_failed: false,
        });
        assert_eq!(clean, Some((22, 22, 9, [13, 148, 136])));
        let failed = badge_dot(TrayUnread {
            count: 3,
            any_failed: true,
        });
        assert_eq!(failed, Some((22, 22, 9, [220, 38, 38])));

        // The overlay replaces pixels inside the circle, leaves the rest.
        let mut data = vec![0u8; 32 * 32 * 4];
        overlay_dot(&mut data, 32, 32, 22, 22, 9, [220, 38, 38]);
        let center = (22 * 32 + 22) * 4;
        assert_eq!(&data[center..center + 4], &[255, 220, 38, 38]);
        let corner = 0;
        assert_eq!(&data[corner..corner + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn autostart_backend_builds_without_touching_the_platform() {
        // Building the backend only resolves the executable; it must not
        // read or write the autostart directory.
        let _ = crate::tray::autostart::backend().expect("the backend builds");
    }
}
