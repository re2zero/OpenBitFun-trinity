// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// Image formats the turn pipeline can decode reliably; anything else a
/// clipboard or drag offers is refused with a visible notice instead of
/// failing the turn later.
pub(super) const SUPPORTED_IMAGE_MIMES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

/// One image waiting to ride along with the next composer send. The bytes are
/// always held shell-side: the turn request carries a `data:` URL, so the
/// image survives any runtime topology (peer/remote runtimes never see a
/// controller-local path).
pub(super) struct PendingAttachment {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) mime: String,
    pub(super) data_url: String,
    /// Decoded bytes for the chip thumbnail.
    pub(super) preview: Arc<Image>,
}

impl PendingAttachment {
    pub(super) fn to_input_attachment(&self) -> AgentInputAttachment {
        AgentInputAttachment::image_context(
            self.id.clone(),
            None,
            Some(self.data_url.clone()),
            self.mime.clone(),
            Some(serde_json::json!({ "name": self.name })),
        )
    }
}

/// `image/png` for `.png`, etc.; `None` for anything outside the supported
/// set (checked against [`SUPPORTED_IMAGE_MIMES`]).
pub(super) fn image_format_for_extension(extension: &str) -> Option<ImageFormat> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        "bmp" => Some(ImageFormat::Bmp),
        _ => None,
    }
}

impl ShellWindow {
    /// Stages clipboard image bytes for the next send; unsupported formats
    /// surface a visible notice instead of failing the turn later.
    pub(super) fn attach_image_bytes(
        &mut self,
        format: ImageFormat,
        bytes: Vec<u8>,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mime = format.mime_type();
        if !SUPPORTED_IMAGE_MIMES.contains(&mime) {
            self.chat_error = Some(i18n::attachment_unsupported("clipboard image", cx));
            cx.notify();
            return;
        }
        let preview = Arc::new(Image::from_bytes(format, bytes.clone()));
        self.pending_attachments.push(PendingAttachment {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.unwrap_or_else(|| format!("clipboard.{}", format.extension())),
            mime: mime.to_string(),
            data_url: format!("data:{mime};base64,{}", BASE64.encode(bytes)),
            preview,
        });
        cx.notify();
    }

    /// Stages dropped / clipboard path entries. The bytes are read here so the
    /// turn payload is self-contained (data URL), which keeps the image
    /// readable on peer or remote runtimes that never see this filesystem.
    pub(super) fn attach_image_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "image".to_string());
            let unsupported = |cx: &Context<Self>| i18n::attachment_unsupported(&name, cx);
            let Some(format) = path
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(image_format_for_extension)
            else {
                self.chat_error = Some(unsupported(cx));
                cx.notify();
                continue;
            };
            match std::fs::read(&path) {
                Ok(bytes) => self.attach_image_bytes(format, bytes, Some(name), cx),
                Err(error) => {
                    tracing::warn!("Failed to read dropped image {}: {}", path.display(), error);
                    self.chat_error = Some(unsupported(cx));
                    cx.notify();
                }
            }
        }
    }

    pub(super) fn remove_attachment(&mut self, id: &str, cx: &mut Context<Self>) {
        self.pending_attachments.retain(|item| item.id != id);
        cx.notify();
    }

    /// The turn-request form of the staged images; the chips stay until the
    /// `TurnAccepted` echo so a failed submission can be retried as-is.
    pub(super) fn pending_attachment_payload(&self) -> Vec<AgentInputAttachment> {
        self.pending_attachments
            .iter()
            .map(PendingAttachment::to_input_attachment)
            .collect()
    }
}

impl ShellWindow {
    /// G-G1: stages composer text for the next paint. The git panel's
    /// not-a-repository card hands its init prompt through here; the composer
    /// consumes it in render, where the window `set_value` needs exists.
    pub(crate) fn prefill_composer(&mut self, draft: String, cx: &mut Context<Self>) {
        self.pending_composer_draft = Some(draft);
        cx.notify();
    }
}

impl ShellWindow {
    /// The virtualized transcript: gpui's native `list` element keeps measured
    /// per-item heights in a sum tree and only renders the visible range plus
    /// overdraw, so a 1k-message transcript neither re-parses off-screen
    /// markdown nor re-renders it per frame. Bottom alignment pins the view to
    /// the newest content until the user scrolls up.
    pub(super) fn render_chat_area(
        &self,
        session_id: &str,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.chat_for(session_id) else {
            return brand_empty(IconName::Sparkles, &text.chat_empty_title, &text.chat_empty);
        };
        let scroller = chat.scroller.clone();
        let session_id = session_id.to_string();
        let this = cx.entity();
        let jump_label = text.chat_jump_latest.clone();
        // Per-frame transcript plan: which consecutive calls collapse into one
        // work run (M24), and the spawn cards a wait/list row resolves its
        // subagent chips against. Derived, never stored — both are pure
        // functions of the entries.
        let groups = transcript::TranscriptGroups::build(&chat.entries);
        let agents = self.agent_index(&chat.session_id, &chat.entries);
        let text = text.clone();
        // The scroller's uniform inter-row gap stands in for a CSS `gap`;
        // transcript rows own their rhythm instead (turn boundary 32px,
        // in-turn 8/12px below), so the wrapper's bottom padding zeroes out.
        let mut row_style = StyleRefinement::default();
        row_style.padding.bottom = Some(px(0.).into());
        MessageScroller::new("transcript", scroller, move |index, _window, cx| {
            let snapshot = this.read(cx);
            let chat = snapshot.chat_for(&session_id);
            let entry = chat.and_then(|chat| chat.entries.get(index)).cloned();
            let group = groups.group_of(index);
            let expanded = match (group, chat) {
                (Some(group), Some(chat)) => chat.group_expanded(group),
                _ => false,
            };
            // Rows inside a collapsed run keep their entry indices (the list
            // counts entries) but render empty, so the run reads as its
            // summary chip alone.
            if group.is_some() && !expanded && !groups.is_head(index) {
                return div().into_any_element();
            }
            let footer_plan = chat.and_then(|chat| {
                turn_footer_plan(
                    &chat.entries,
                    index,
                    chat.busy,
                    chat.submitting,
                    &text,
                    &chat.turn_times,
                )
            });
            let user_expanded = match (&entry, chat) {
                (Some(ChatEntry::User { text: body, .. }), Some(chat)) => {
                    chat.expanded_user_texts.contains(&markdown_id_key(body))
                }
                _ => false,
            };
            let fork = chat.map(|chat| ChatForkContext {
                session_id: chat.session_id.clone(),
                tx: snapshot.event_tx.clone(),
                view: this.downgrade(),
            });
            let is_user = matches!(
                entry,
                Some(ChatEntry::User { .. } | ChatEntry::FailedUser { .. })
            );
            match entry {
                Some(entry) => {
                    let mut row = div()
                        .debug_selector(|| "chat-row".to_string())
                        .w_full()
                        .max_w(px(READING_COLUMN_WIDTH))
                        .mx_auto();
                    if is_user {
                        // The turn boundary opens above every prompt but the
                        // transcript's first row; the prompt hugs its answer.
                        if index > 0 {
                            row = row.mt(px(TURN_GAP));
                        }
                        row = row.mb(px(FLOW_ITEM_GAP));
                    } else {
                        row = row.mb(px(TURN_RHYTHM_GAP));
                    }
                    // A run's head hosts its summary chip; its own row joins
                    // the run when open.
                    if let Some(group) = group.filter(|group| group.head == index) {
                        let key = match &entry {
                            ChatEntry::Tool(card) => markdown_id_key(&card.key),
                            _ => markdown_id_key(&format!("row-{index}")),
                        };
                        row = row.child(render_group_chip(group, expanded, key, fork.as_ref(), cx));
                        if !expanded {
                            // The chip stands in for the whole run, footer
                            // included: attach the turn footer the hidden tail
                            // would have carried.
                            let tail = group.head + group.len - 1;
                            let plan = chat.and_then(|chat| {
                                turn_footer_plan(
                                    &chat.entries,
                                    tail,
                                    chat.busy,
                                    chat.submitting,
                                    &text,
                                    &chat.turn_times,
                                )
                            });
                            if let (Some(plan), Some(fork)) = (plan, fork.as_ref()) {
                                row = row.child(turn_footer(plan, tail, fork, &text, cx));
                            }
                            return row.into_any_element();
                        }
                    }
                    let (identity, roster) = match &entry {
                        ChatEntry::Tool(card) => (
                            snapshot.subagent_identity_for(card),
                            agents.roster_for(card),
                        ),
                        _ => (None, Vec::new()),
                    };
                    row = row.child(render_chat_item(
                        &entry,
                        &text,
                        fork.as_ref(),
                        identity.as_ref(),
                        &roster,
                        index,
                        user_expanded,
                        cx,
                    ));
                    if let (Some(plan), Some(fork)) = (footer_plan, fork.as_ref()) {
                        row = row.child(turn_footer(plan, index, fork, &text, cx));
                    }
                    row.into_any_element()
                }
                // Scroller item counts are synced on the same mutation
                // that grows `entries`, so an out-of-range index cannot
                // persist.
                None => div().into_any_element(),
            }
        })
        .with_row_style(row_style)
        .with_jump_button_label(jump_label)
        .with_bottom_fade(cx.theme().background)
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    /// Streaming indicator rendered as a fixed row between the transcript and
    /// the composer (list items are reserved for transcript entries).
    pub(super) fn busy_label(&self, text: &ShellText) -> Option<String> {
        let chat = self.active_chat()?;
        if chat.submitting {
            Some(text.chat_submitting.clone())
        } else if chat.busy {
            Some(text.chat_busy.clone())
        } else {
            None
        }
    }

    /// One staged image chip: thumbnail, name, remove button.
    pub(super) fn render_attachment_chip(
        &self,
        item: &PendingAttachment,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let view = cx.entity().downgrade();
        let id = item.id.clone();
        let remove = Button::new(ElementId::Name(
            format!("attachment-remove-{}", item.id).into(),
        ))
        .icon(IconName::X)
        .xsmall()
        .on_click(move |_, _, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.remove_attachment(&id, cx));
            }
        });
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .py_0p5()
            .rounded(cx.theme().radius)
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                img(item.preview.clone())
                    .size(px(24.))
                    .rounded(cx.theme().radius_tokens().sm)
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .max_w(px(140.))
                    .text_xs()
                    .truncate()
                    .child(item.name.clone()),
            )
            .child(remove)
            .into_any_element()
    }

    /// Staged-image strip above the input row; `None` keeps the composer at
    /// its usual height.
    pub(super) fn render_attachment_strip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        if self.pending_attachments.is_empty() {
            return None;
        }
        let chips: Vec<gpui_kit::AnyElement> = self
            .pending_attachments
            .iter()
            .map(|item| self.render_attachment_chip(item, cx))
            .collect();
        Some(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .w_full()
                .children(chips)
                .into_any_element(),
        )
    }

    pub(super) fn render_composer(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
            return div().into_any_element();
        };
        let busy = chat.busy;
        let submitting = chat.submitting;
        let session_id = chat.session_id.clone();
        let has_attachments = !self.pending_attachments.is_empty();

        let draft_value = self.draft.read(cx).value().to_string();
        let draft_for_send = self.draft.clone();
        let tx_for_send = self.event_tx.clone();
        let session_for_send = session_id.clone();
        let view_for_send = cx.entity().downgrade();

        let action_button = if busy {
            // A `/btw` side-question turn lives in the coordinator as a
            // dialog turn (the D68 direct-call pipeline), not in the agent
            // runtime's session loop: the stop must target the recorded
            // dialog turn, or it would be a silent no-op.
            let btw_cancel = chat
                .child_context
                .as_ref()
                .filter(|child| child.kind == ChildKind::Btw)
                .and_then(|child| child.btw_turn_id.clone())
                .map(|turn_id| (session_id.clone(), turn_id));
            // Web cancel shape: a round button holding a small square. The
            // tooltip rides a wrapper that goes away with the button instead of
            // riding `Button::tooltip` (W-8 ⑤): the kit's managed tooltip is a
            // window-level overlay whose only hide path is the trigger's
            // hover-end, so a turn that settles under a resting pointer unmounts
            // the trigger before that event can fire and the popup sticks on
            // screen. gpui's own tooltip is re-registered by its owner's paint
            // every frame, so it disappears together with its owner.
            let stop_label = text.chat_stop.clone();
            div()
                .flex()
                .id("chat-stop-tooltip-anchor")
                .debug_selector(|| "chat-stop-tooltip-anchor".to_string())
                .tooltip(move |window, cx| {
                    let label = stop_label.clone();
                    // `element` rather than `new`: the popup is the same
                    // styled box around the same plain string, and the
                    // selector is the only handle the W-8 ⑤ regression test
                    // has on a native tooltip (the kit's popup owns no
                    // selector of its own).
                    gpui_kit::component::tooltip::Tooltip::element(move |_, _| {
                        div()
                            .debug_selector(|| "chat-stop-tooltip".to_string())
                            .child(label.clone())
                    })
                    .build(window, cx)
                })
                .child(
                    Button::new("chat-stop")
                        .icon(IconName::Square)
                        .with_variant(ButtonVariant::Danger)
                        .rounded(ButtonRounded::Size(px(999.)))
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                match &btw_cancel {
                                    Some((child_session_id, turn_id)) => {
                                        handle.cancel_btw(child_session_id.clone(), turn_id.clone())
                                    }
                                    None => handle.cancel_turn(session_id.clone()),
                                }
                            }
                        }),
                )
                .into_any_element()
        } else {
            // Web send shape: a round primary button holding an up arrow.
            Button::new("chat-send")
                .icon(IconName::ArrowUp)
                .with_variant(ButtonVariant::Primary)
                .rounded(ButtonRounded::Size(px(999.)))
                .disabled(submitting || (draft_value.trim().is_empty() && !has_attachments))
                .on_click(move |_, window, cx| {
                    let body = draft_for_send.read(cx).value().to_string();
                    let trimmed = body.trim();
                    if trimmed.is_empty() && !has_attachments {
                        return;
                    }
                    // `/btw <question>` routes to a side-question child
                    // session (M20); attachments stay behind — the child
                    // forks the parent context instead of carrying media.
                    if let Some(question) = btw_question(&body) {
                        if question.is_empty() {
                            if let Some(view) = view_for_send.upgrade() {
                                view.update(cx, |shell, cx| {
                                    shell.chat_error = Some(i18n::btw_empty(cx));
                                });
                            }
                            return;
                        }
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.start_btw(
                                session_for_send.clone(),
                                question.to_string(),
                                tx_for_send.clone(),
                            );
                        }
                        draft_for_send.update(cx, |state, cx| {
                            state.set_value("", window, cx);
                        });
                        return;
                    }
                    let attachments = view_for_send
                        .upgrade()
                        .map(|view| view.read(cx).pending_attachment_payload())
                        .unwrap_or_default();
                    // A new submission retires a previous failed preview:
                    // the slot is single, exactly like the web's.
                    if let Some(view) = view_for_send.upgrade() {
                        view.update(cx, |shell, cx| {
                            if let Some(chat) = shell.chat_mut_for(&session_for_send) {
                                let before = chat.entries.len();
                                chat.entries
                                    .retain(|entry| !matches!(entry, ChatEntry::FailedUser { .. }));
                                if chat.entries.len() != before {
                                    cx.notify();
                                }
                            }
                        });
                    }
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.send_message(
                            session_for_send.clone(),
                            body,
                            attachments,
                            tx_for_send.clone(),
                        );
                    }
                    draft_for_send.update(cx, |state, cx| {
                        state.set_value("", window, cx);
                    });
                })
                .into_any_element()
        };

        let strip = self.render_attachment_strip(cx);
        let paste_view = cx.entity().downgrade();

        div()
            .w_full()
            .max_w(px(READING_COLUMN_WIDTH))
            .mx_auto()
            .px_4()
            .pb_3()
            // Context strip above the card (R2a, the web ChatComposer
            // contextBar): where the session runs and what the next turn may
            // do, as a bare muted track under the capsule.
            .child(self.render_composer_context_strip(text, cx))
            .child(
                div()
                    .debug_selector(|| "composer-card".to_string())
                    .flex()
                    .flex_col()
                    .gap_2()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_2xl())
                    .shadow_sm()
                    .p_2()
                    .when_some(strip, |this, strip| this.child(strip))
                    .child(
                        div().flex().flex_row().child(
                            Textarea::new(&self.draft)
                                .appearance(false)
                                .flex_1()
                                .h(px(COMPOSER_HEIGHT))
                                // Image and file pastes become staged
                                // attachments; text falls through to the
                                // input's own insertion.
                                .on_paste(move |item, _window, cx| {
                                    let Some(view) = paste_view.upgrade() else {
                                        return false;
                                    };
                                    let mut taken = false;
                                    view.update(cx, |this, cx| {
                                        for entry in item.entries() {
                                            match entry {
                                                ClipboardEntry::Image(image) => {
                                                    this.attach_image_bytes(
                                                        image.format,
                                                        image.bytes.clone(),
                                                        None,
                                                        cx,
                                                    );
                                                    taken = true;
                                                    break;
                                                }
                                                ClipboardEntry::ExternalPaths(paths) => {
                                                    let paths: Vec<PathBuf> = paths.0.to_vec();
                                                    this.attach_image_paths(paths, cx);
                                                    taken = true;
                                                    break;
                                                }
                                                ClipboardEntry::String(_) => {}
                                            }
                                        }
                                    });
                                    taken
                                }),
                        ),
                    )
                    // Action row under the input (web expanded layout): the
                    // boost cluster on the left, the model picker and the
                    // round send button share the right cluster.
                    .child(
                        div()
                            .debug_selector(|| "composer-actions-row".to_string())
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(self.render_composer_boost_menu(text, cx))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .debug_selector(|| "composer-model-picker".to_string())
                                            .child(self.render_model_picker(text, cx)),
                                    )
                                    .child(action_button),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The composer context strip (R2a, the web `ChatInputWorkspaceStrip`):
    /// left rail names where the session runs — workspace and mode — and the
    /// right rail states the next turn's contract — local execution and the
    /// permission preset. Every value is a read-only fact projected from an
    /// existing snapshot (workspace handle, chat agent type, shell config);
    /// the strip adds no controls, so editing stays with its owner surface
    /// (workspace switching in the status bar, the preset on the settings
    /// permission page).
    pub(super) fn render_composer_context_strip(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let workspace = runtime::runtime_handle()
            .map(|handle| handle.workspace().name)
            .unwrap_or_default();
        let mode = self
            .active_chat()
            .map(|chat| chat.agent_type.clone())
            .unwrap_or_default();
        let permission_config = settings::ShellConfig::try_current(cx);
        // The web `show_permission_mode_control` preference hides the pill;
        // the settings page owns the switch, so the strip reads the same
        // document rather than inventing a second source.
        let show_permission_badge = permission_config
            .as_ref()
            .map(|config| {
                settings::object_bool(&config.flow_chat, "show_permission_mode_control", true)
            })
            .unwrap_or(true);
        let (permission_icon, permission_label, permission_full_access, permission_tooltip) =
            match permission_config.as_ref() {
                Some(config) => {
                    Self::permission_badge(settings::permission_mode_value(config), text)
                }
                None => (
                    IconName::Shield,
                    text.permission_ask.clone(),
                    false,
                    text.permission_ask_desc.clone(),
                ),
            };

        // Built before the `fact`/`divider` closures borrow `cx`: the pill
        // mounts the shared workspace menu, which needs `&mut Context`.
        let has_workspace = !workspace.is_empty();
        let has_mode = !mode.is_empty();
        let workspace_pill =
            has_workspace.then(|| self.render_workspace_pill(workspace.clone(), text, cx));

        let fact = |label: String| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(div().truncate().max_w(px(180.)).child(label))
        };
        let divider = || div().w_px().h_3().flex_shrink_0().bg(cx.theme().border);

        div()
            .debug_selector(|| "composer-context-strip".to_string())
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_2()
            .pb_1()
            .min_h(px(20.))
            // Left rail: where this session runs — the workspace pill opens
            // the recent-workspace menu (web left-rail pill is clickable);
            // the mode stays a read-only fact (switching has no shell runtime
            // path yet).
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .when_some(workspace_pill, |this, pill| this.child(pill))
                    .when(has_workspace && has_mode, |this| this.child(divider()))
                    .when(has_mode, |this| this.child(fact(mode))),
            )
            // Right rail: what the next turn may do.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Laptop).small())
                            .child(text.composer_local.clone()),
                    )
                    .when(show_permission_badge, |this| {
                        this.child(
                            // A pill button, not a bare fact: the web trigger
                            // carries the mode description as its tooltip and
                            // its menu ends in "open default settings" — the
                            // click opens the settings scene that owns the
                            // preset. No mode switching here: the shell's
                            // preset surface stays on the settings permission
                            // page.
                            div()
                                .debug_selector(|| "composer-permission-badge".to_string())
                                .child(
                                    Button::new("composer-permission-badge-btn")
                                        .xsmall()
                                        .custom(
                                            ButtonCustomVariant::new(cx)
                                                .color(cx.theme().sidebar_accent)
                                                .foreground(cx.theme().muted_foreground),
                                        )
                                        .rounded(ButtonRounded::Size(px(999.)))
                                        .font_medium()
                                        .icon(Icon::new(permission_icon).small().text_color(
                                            if permission_full_access {
                                                cx.theme().danger
                                            } else {
                                                cx.theme().success
                                            },
                                        ))
                                        .label(permission_label)
                                        .tooltip(permission_tooltip)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_settings_view(cx)
                                        })),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Stored label/prompt of the built-in quick actions in the shared config
/// defaults (English; `config-contracts` writes the same values). The
/// localized copy replaces them only while the stored text still equals
/// these defaults — a customized action keeps its stored text verbatim.
pub(crate) const QUICK_ACTION_COMMIT_DEFAULTS: (&str, &str) =
    ("Commit", "Commit all current code changes");
pub(crate) const QUICK_ACTION_CREATE_PR_DEFAULTS: (&str, &str) =
    ("Create PR", "Create a Pull Request for the current branch");

/// One stored-vs-default text of a built-in quick action (web
/// `resolveQuickActionText` resolves label and prompt independently).
pub(super) fn builtin_quick_action_text(stored: &str, default: &str, localized: &str) -> String {
    if stored == default {
        localized.to_string()
    } else {
        stored.to_string()
    }
}

/// Resolves a quick action's display label and the prompt a click sends
/// (web `resolveQuickActionText` port): built-in `commit`/`create_pr`
/// localize the shared English defaults, everything else stays verbatim.
pub(crate) fn resolve_quick_action_text(
    action: &openbitfun_core::service::config::types::AiExperienceQuickAction,
    text: &ShellText,
) -> (String, String) {
    match action.id.as_str() {
        "commit" => (
            builtin_quick_action_text(
                &action.label,
                QUICK_ACTION_COMMIT_DEFAULTS.0,
                &text.quick_action_commit_label,
            ),
            builtin_quick_action_text(
                &action.prompt,
                QUICK_ACTION_COMMIT_DEFAULTS.1,
                &text.quick_action_commit_prompt,
            ),
        ),
        "create_pr" => (
            builtin_quick_action_text(
                &action.label,
                QUICK_ACTION_CREATE_PR_DEFAULTS.0,
                &text.quick_action_create_pr_label,
            ),
            builtin_quick_action_text(
                &action.prompt,
                QUICK_ACTION_CREATE_PR_DEFAULTS.1,
                &text.quick_action_create_pr_prompt,
            ),
        ),
        _ => (action.label.clone(), action.prompt.clone()),
    }
}
