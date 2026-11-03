// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// Opens the rename dialog for one session. The input entity lives on the
/// shell (a dialog builder runs every frame and must not create state), and the
/// dialog is dismissable: unlike a permission ask, a rename nobody answers
/// blocks nothing.
pub(super) fn open_rename_dialog(
    view: WeakEntity<ShellWindow>,
    session_id: String,
    current_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let input = shell.read(cx).rename_input.clone();
    input.update(cx, |state, cx| state.set_value(current_name, window, cx));
    shell.update(cx, |this, cx| {
        this.rename_target = Some(session_id.clone());
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let name = shell
            .read(cx)
            .rename_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if name.is_empty() {
            // Keep the dialog open: an empty name would rename nothing the
            // user could recognize in the list.
            tracing::warn!("Ignoring empty session name");
            return;
        }
        let session_id = shell.read(cx).rename_target.clone().unwrap_or_default();
        if session_id.is_empty() {
            window.close_dialog(cx);
            return;
        }
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.rename_session(session_id, name, tx);
        }
        shell.update(cx, |this, cx| {
            this.rename_target = None;
            cx.notify();
        });
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |this, cx| {
                this.rename_target = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.session_rename_title.clone()))
            .child(div().w(px(360.)).child(Input::new(&input)))
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("rename-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("rename-confirm")
                            .label(text.session_action_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the "打开项目" dialog: the typed folder path is registered (or reused)
/// as a workspace record and becomes this shell's active workspace. The
/// dialog is dismissable; unlike an ask, nobody answering it blocks nothing.
///
/// The web UI picks this folder through the platform's native directory
/// chooser. The shell keeps the path field instead: the file-chooser route
/// runs on the platform's XDG-portal client (ashpd -> zbus), and a workspace
/// build resolves zbus with a tokio executor that has no reactor on the GPUI
/// thread (the desktop portal hazard documented for this repository), so the
/// chooser is not a call the shell can make safely yet.
/// `ponytail: path field now, native folder chooser once the shell carries an
/// async-io-only zbus build.`
pub(super) fn open_workspace_path_dialog(
    view: WeakEntity<ShellWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let input = shell.read(cx).workspace_input.clone();
    input.update(cx, |state, cx| state.set_value("", window, cx));
    let confirm_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let path = shell
            .read(cx)
            .workspace_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let text = i18n::current(cx);
        let error = match check_project_path(&path) {
            ProjectPathCheck::Ok => None,
            ProjectPathCheck::Empty => Some(text.new_project_error_parent.clone()),
            ProjectPathCheck::Missing => Some(text.new_project_error_missing.clone()),
        };
        if let Some(message) = error {
            shell.update(cx, |this, cx| {
                this.new_project_error = Some(message);
                cx.notify();
            });
            return;
        }
        shell.update(cx, |this, _cx| this.new_project_error = None);
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.switch_workspace(std::path::PathBuf::from(path), tx);
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let error = view
            .upgrade()
            .and_then(|shell| shell.read(cx).new_project_error.clone());
        let mut body = div().v_flex().gap_2().w(px(420.)).child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.workspace_open_path_description.clone()),
        );
        if let Some(error) = error {
            body = body.child(div().text_sm().text_color(cx.theme().danger).child(error));
        }
        dialog
            .title(div().child(text.workspace_open_project.clone()))
            .child(body.child(Input::new(&input)))
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("workspace-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("workspace-confirm")
                            .label(text.workspace_open_project.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the "新建项目" dialog (web `NewProjectDialog` with a typed parent
/// folder instead of the platform picker — see [`open_workspace_path_dialog`]
/// for why). Confirm creates the directory and opens it as this shell's
/// workspace, exactly like the web's `createDirectory` + `openWorkspace` pair.
pub(super) fn open_new_project_dialog(
    view: WeakEntity<ShellWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (parent_input, name_input) = {
        let shell = shell.read(cx);
        (
            shell.workspace_new_parent_input.clone(),
            shell.workspace_new_name_input.clone(),
        )
    };
    // Web `defaultParentPath` is the picker's start directory; with a typed
    // field the useful default is the active workspace's parent folder.
    let default_parent = runtime::runtime_handle()
        .and_then(|handle| {
            handle
                .workspace()
                .root_path
                .parent()
                .map(|p| p.to_path_buf())
        })
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    parent_input.update(cx, |state, cx| state.set_value(default_parent, window, cx));
    name_input.update(cx, |state, cx| state.set_value("", window, cx));
    shell.update(cx, |this, cx| {
        this.new_project_error = None;
        cx.notify();
    });

    let confirm_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let (parent, name) = {
            let shell = shell.read(cx);
            (
                shell
                    .workspace_new_parent_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string(),
                shell
                    .workspace_new_name_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string(),
            )
        };
        let text = i18n::current(cx);
        let error = if parent.is_empty() {
            Some(text.new_project_error_parent.clone())
        } else if name.is_empty() {
            Some(text.new_project_error_name.clone())
        } else if check_project_path(&parent) == ProjectPathCheck::Missing {
            // The web dialog picks the parent, so it is always an existing
            // folder; this one types it, and `create_dir_all`-style parent
            // creation would bury a mistyped project somewhere unintended.
            Some(text.new_project_error_missing.clone())
        } else {
            None
        };
        if let Some(message) = error {
            shell.update(cx, |this, cx| {
                this.new_project_error = Some(message);
                cx.notify();
            });
            return;
        }
        let target = new_project_path(&parent, &name);
        if let Err(message) = create_project_dir(&target) {
            tracing::error!("Failed to create project directory {target}: {message}");
            let message = format!("{}: {message}", text.new_project_error_create);
            shell.update(cx, |this, cx| {
                this.new_project_error = Some(message);
                cx.notify();
            });
            return;
        }
        tracing::info!("Created project directory {target}; switching workspace");
        shell.update(cx, |this, cx| {
            this.new_project_error = None;
            this.workspace_error = None;
            cx.notify();
        });
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.switch_workspace(std::path::PathBuf::from(target), tx);
        }
        window.close_dialog(cx);
    };
    let cancel_view = view.clone();
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |this, cx| {
                this.new_project_error = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let (error, preview) = view
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                let parent = shell
                    .workspace_new_parent_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let name = shell
                    .workspace_new_name_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let preview = (!parent.is_empty() && !name.is_empty())
                    .then(|| new_project_path(&parent, &name));
                (shell.new_project_error.clone(), preview)
            })
            .unwrap_or((None, None));
        let mut body = div()
            .v_flex()
            .gap_3()
            .w(px(420.))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.new_project_subtitle.clone()),
            )
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text.new_project_parent.clone()),
                    )
                    .child(Input::new(&parent_input)),
            )
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text.new_project_name.clone()),
                    )
                    .child(Input::new(&name_input)),
            );
        if let Some(preview) = preview {
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.new_project_full_path.clone())
                    .child(div().min_w_0().truncate().child(preview)),
            );
        }
        if let Some(error) = error {
            body = body.child(div().text_sm().text_color(cx.theme().danger).child(error));
        }
        dialog
            .title(div().child(text.new_project_title.clone()))
            .child(body)
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("new-project-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("new-project-confirm")
                            .label(text.new_project_create.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// The full path of a new project: the web UI's `${parent}/${name}` join with
/// backslashes normalized, so a pasted Windows path still lands on one
/// separator. Trailing separators on the parent are collapsed.
pub(super) fn new_project_path(parent: &str, name: &str) -> String {
    let parent = parent.trim().replace('\\', "/");
    let parent = parent.trim_end_matches('/');
    let name = name.trim().trim_matches('/');
    format!("{parent}/{name}")
}

/// Validation outcome for the 打开项目 field: the path must name an existing
/// directory (`switch_workspace` would otherwise fail later with a less
/// specific message). Missing and non-directory paths get the same refusal
/// the web's directory chooser gives by never offering them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectPathCheck {
    Ok,
    Empty,
    Missing,
}

pub(super) fn check_project_path(path: &str) -> ProjectPathCheck {
    if path.trim().is_empty() {
        return ProjectPathCheck::Empty;
    }
    if !std::path::Path::new(path).is_dir() {
        return ProjectPathCheck::Missing;
    }
    ProjectPathCheck::Ok
}

/// Creates the new project directory (the web `workspaceAPI.createDirectory`
/// step). Only the leaf is created: the dialog has already refused a parent
/// that is not an existing folder, and the parent here is typed rather than
/// picked, so `create_dir_all` would turn a typo into a nested project.
pub(super) fn create_project_dir(path: &str) -> Result<(), std::io::Error> {
    std::fs::create_dir(path)
}

/// Opens the delete confirmation for one session. Deleting is destructive and
/// has no undo, so it never happens straight from the row menu.
pub(super) fn open_export_dialog(
    view: WeakEntity<ShellWindow>,
    session_id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let input = shell.read(cx).export_input.clone();
    input.update(cx, |state, cx| {
        state.set_value(default_export_path(&name), window, cx)
    });
    shell.update(cx, |this, cx| {
        this.export_target = Some(session_id.clone());
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let path = shell
            .read(cx)
            .export_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if path.is_empty() {
            // Keep the dialog open: an empty path would write nowhere.
            tracing::warn!("Ignoring empty export path");
            return;
        }
        let session_id = shell.read(cx).export_target.clone().unwrap_or_default();
        if session_id.is_empty() {
            window.close_dialog(cx);
            return;
        }
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.export_session(session_id, PathBuf::from(&path), tx);
        }
        shell.update(cx, |this, cx| {
            this.export_target = None;
            cx.notify();
        });
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |this, cx| {
                this.export_target = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.export_title.clone()))
            .child(div().w(px(420.)).child(Input::new(&input)))
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("export-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("export-confirm")
                            .label(text.session_action_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Export prefill: the user's Downloads directory (HOME / USERPROFILE),
/// falling back to the current directory when neither is set.
pub(super) fn default_export_path(name: &str) -> String {
    let base = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(|home| PathBuf::from(home).join("Downloads"))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join(format!("openbitfun-{}.md", export_slug(name)))
        .display()
        .to_string()
}

/// File-name slug for an export: ASCII lowercase words joined by dashes; a
/// name with no ASCII word characters collapses to `session`.
pub(super) fn export_slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let compacted = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if compacted.is_empty() {
        "session".to_string()
    } else {
        compacted
    }
}

/// Key/value rows for the usage dialog: headline token counts, timing, then
/// one line per model the session called.
pub(super) fn usage_report_rows(
    report: &runtime::SessionUsageSummary,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let mut entries: Vec<(String, String)> = vec![
        (text.usage_turns.clone(), report.turn_count.to_string()),
        (
            text.usage_input_tokens.clone(),
            optional_tokens(report.input_tokens),
        ),
        (
            text.usage_output_tokens.clone(),
            optional_tokens(report.output_tokens),
        ),
        (
            text.usage_cached_tokens.clone(),
            optional_tokens(report.cached_tokens),
        ),
        (
            text.usage_total_tokens.clone(),
            optional_tokens(report.total_tokens),
        ),
        (
            text.usage_wall_time.clone(),
            optional_duration(report.wall_time_ms),
        ),
        (
            text.usage_active_time.clone(),
            optional_duration(report.active_turn_ms),
        ),
    ];
    if !report.models.is_empty() {
        for model in &report.models {
            entries.push((
                format!("{} · {}", text.usage_models.clone(), model.model_id),
                format!("{} × {}", model.calls, optional_tokens(model.total_tokens)),
            ));
        }
    }
    let mut rows = div().v_flex().gap_1().w(px(420.));
    for (label, value) in entries {
        rows = rows.child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .gap_2()
                .text_sm()
                .child(div().text_color(cx.theme().muted_foreground).child(label))
                .child(div().child(value)),
        );
    }
    rows
}

/// Compact duration for the usage dialog: seconds below a minute, then whole
/// minutes and seconds.
pub(super) fn optional_duration(ms: Option<u64>) -> String {
    let Some(ms) = ms else {
        return "—".to_string();
    };
    if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let total_seconds = ms / 1000;
        format!("{} m {} s", total_seconds / 60, total_seconds % 60)
    }
}

pub(super) fn optional_tokens(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "—".to_string())
}

pub(super) fn open_delete_dialog(
    view: WeakEntity<ShellWindow>,
    session_id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let text = i18n::current(cx);
        let confirm_id = session_id.clone();
        let confirm_view = view.clone();
        alert
            .confirm()
            .title(text.session_delete_title.clone())
            .description(
                div()
                    .v_flex()
                    .gap_1()
                    .child(name.clone())
                    .child(text.session_delete_warning.clone()),
            )
            .ok_text(text.session_delete_confirm.clone())
            .cancel_text(text.session_action_cancel.clone())
            .on_ok(move |_, _, cx| {
                if let Some(shell) = confirm_view.upgrade() {
                    let tx = shell.read(cx).event_tx.clone();
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.delete_session(confirm_id.clone(), tx);
                    }
                }
                true
            })
    });
}
