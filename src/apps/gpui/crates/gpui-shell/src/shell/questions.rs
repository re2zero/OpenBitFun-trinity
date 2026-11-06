// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// One pending approval: the runtime request plus display lines composed once
/// at arrival, so the dialog builder never touches the i18n service on the
/// render path (same pattern as [`SessionRow`] metadata).
#[derive(Clone)]
pub(super) struct PermissionPromptItem {
    pub(super) request: PermissionRequest,
    pub(super) resources_line: Option<String>,
    pub(super) project_line: String,
    pub(super) delegation_line: Option<String>,
    pub(super) risk_line: Option<String>,
}

impl PermissionPromptItem {
    pub(super) fn request_id(&self) -> &str {
        &self.request.request_id
    }
}

/// Which shell-owned modal currently owns the single dialog slot. The
/// component library's dialog stack closes strictly by LIFO and carries no
/// dialog id, so the shell keeps at most one modal open and hands the slot
/// over in priority order; two open modals would make a `close_dialog` meant
/// for one queue pop the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModalOwner {
    Permissions,
    Questions,
}

/// One pending `ask_user` batch, pre-parsed for rendering. Selection state
/// lives here rather than in the dialog builder, so a queue rebuild keeps the
/// user's in-progress answers when another question joins or leaves.
#[derive(Debug, Clone)]
pub(super) struct QuestionPromptItem {
    pub(super) tool_id: String,
    pub(super) session_id: String,
    /// Questions of this batch; empty when the payload could not be parsed
    /// (the dialog then offers Cancel only).
    pub(super) questions: Vec<PromptQuestion>,
    /// Selected option indices per question; single-select holds at most one.
    pub(super) selected: Vec<BTreeSet<usize>>,
}

/// One question of an `ask_user` batch as rendered by the dialog.
#[derive(Debug, Clone)]
pub(super) struct PromptQuestion {
    pub(super) header: String,
    pub(super) question: String,
    /// "Question n of m", resolved in the active locale when the item was
    /// composed; the render path never calls the i18n service.
    pub(super) progress_line: String,
    pub(super) options: Vec<PromptOption>,
    pub(super) multi_select: bool,
}

/// One selectable option of a question.
#[derive(Debug, Clone)]
pub(super) struct PromptOption {
    pub(super) label: String,
    pub(super) description: String,
}

impl QuestionPromptItem {
    /// True once every question has a selection; a malformed batch is never
    /// answerable and only offers Cancel.
    pub(super) fn is_answered(&self) -> bool {
        !self.questions.is_empty() && self.selected.iter().all(|set| !set.is_empty())
    }

    /// Answers in the wire shape the tool expects: question index as key,
    /// option labels as the value, an array for multi-select questions
    /// (`{"0": "A", "1": ["B", "C"]}` — same as the desktop card).
    pub(super) fn answers(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (index, question) in self.questions.iter().enumerate() {
            let selected = question
                .options
                .iter()
                .enumerate()
                .filter(|(option_index, _)| {
                    self.selected
                        .get(index)
                        .is_some_and(|set| set.contains(option_index))
                })
                .map(|(_, option)| option.label.clone())
                .collect::<Vec<_>>();
            if selected.is_empty() {
                continue;
            }
            if question.multi_select {
                map.insert(index.to_string(), serde_json::Value::from(selected));
            } else if let Some(label) = selected.first() {
                map.insert(index.to_string(), serde_json::Value::from(label.clone()));
            }
        }
        serde_json::Value::Object(map)
    }
}

/// Parses one mailbox payload into render-ready questions. The payload is the
/// tool input plus timing metadata; an unparsable or option-less batch yields
/// `None` so the dialog stays visible with Cancel instead of hiding a live
/// ask. `question_label` localizes the "n of m" heading at composition time,
/// keeping the i18n service off the render path.
pub(super) fn parse_question_payload(
    payload: &serde_json::Value,
    question_label: impl Fn(usize, usize) -> String,
) -> Option<Vec<PromptQuestion>> {
    let input: openbitfun_agent_runtime::user_questions::AskUserQuestionInput =
        serde_json::from_value(payload.clone()).ok()?;
    let total = input.questions.len();
    let questions = input
        .questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| PromptQuestion {
            header: question.header,
            question: question.question,
            progress_line: question_label(index + 1, total),
            options: question
                .options
                .into_iter()
                .map(|option| PromptOption {
                    label: option.label,
                    description: option.description,
                })
                .collect(),
            multi_select: question.multi_select,
        })
        .collect::<Vec<_>>();
    if questions.is_empty() || questions.iter().any(|question| question.options.is_empty()) {
        return None;
    }
    Some(questions)
}

/// Builds one render-ready item from a mailbox question.
pub(super) fn compose_question_item(
    question: PendingUserQuestion,
    question_label: impl Fn(usize, usize) -> String,
) -> QuestionPromptItem {
    let questions = parse_question_payload(&question.questions, question_label);
    let selected = vec![BTreeSet::new(); questions.as_ref().map_or(0, Vec::len)];
    QuestionPromptItem {
        tool_id: question.tool_id,
        session_id: question.session_id,
        questions: questions.unwrap_or_default(),
        selected,
    }
}

/// Rebuilds the question queue from a mailbox snapshot. Existing items are
/// carried over whole so the user's in-progress selections survive an
/// unrelated question arriving or leaving; only new tool ids are parsed.
pub(super) fn rebuild_question_queue(
    existing: &[QuestionPromptItem],
    incoming: Vec<PendingUserQuestion>,
    question_label: impl Fn(usize, usize) -> String,
) -> Vec<QuestionPromptItem> {
    incoming
        .into_iter()
        .map(|question| {
            match existing
                .iter()
                .find(|item| item.tool_id == question.tool_id)
            {
                Some(item) => item.clone(),
                None => compose_question_item(question, &question_label),
            }
        })
        .collect()
}

/// Which modal should own the dialog slot next; `None` when both queues are
/// empty. Approvals take the slot first: they gate a tool call that is
/// otherwise parked, while an attended question waits without a deadline.
pub(super) fn next_modal_target(
    pending_permissions: usize,
    pending_questions: usize,
) -> Option<ModalOwner> {
    if pending_permissions > 0 {
        Some(ModalOwner::Permissions)
    } else if pending_questions > 0 {
        Some(ModalOwner::Questions)
    } else {
        None
    }
}

impl ShellWindow {
    /// Maps the stored permission mode onto the strip badge: icon ramp (web
    /// `PERMISSION_MODE_ICONS` — the shield gains a mark as confirmation is
    /// given up), label, whether the icon takes the danger ink of the
    /// full-access step, and the mode description the trigger shows.
    pub(super) fn permission_badge(
        mode: &str,
        text: &ShellText,
    ) -> (IconName, String, bool, String) {
        match mode {
            "full_access" => (
                IconName::ShieldAlert,
                text.permission_full_access.clone(),
                true,
                text.permission_full_access_desc.clone(),
            ),
            "auto" => (
                IconName::ShieldCheck,
                text.permission_auto.clone(),
                false,
                text.permission_auto_desc.clone(),
            ),
            _ => (
                IconName::Shield,
                text.permission_ask.clone(),
                false,
                text.permission_ask_desc.clone(),
            ),
        }
    }

    /// Opens the modal approval dialog bound to the head of the pending
    /// queue. The builder re-reads the queue every frame, so answering one
    /// request reveals the next without reopening; the dialog closes on the
    /// resolving [`RuntimeEvent::PermissionResolved`] echo instead of in the
    /// button handlers, keeping the surface a pure projection of the pending
    /// set. Every dismissal path without a decision (Escape, backdrop press,
    /// close icon) is disabled: an unanswered ask parks its turn, so the only
    /// ways out are the three replies.
    pub(super) fn open_permission_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let Some(view) = view.upgrade() else {
                return dialog;
            };
            let Some(item) = view.read(cx).pending_permissions.first().cloned() else {
                return dialog;
            };
            let text = i18n::current(cx);
            dialog
                .title(
                    div()
                        .text_color(cx.theme().warning)
                        .child(text.permission_title.clone()),
                )
                .overlay_closable(false)
                .keyboard(false)
                .close_button(false)
                .child(permission_body(&item, cx))
                .footer(permission_footer(&item, &text, answer_permission))
        });
        self.modal_owner = Some(ModalOwner::Permissions);
    }

    /// Opens the modal question dialog bound to the head of the pending
    /// queue. Like the approval dialog the builder re-reads the queue every
    /// frame, so answering one batch reveals the next without reopening, and
    /// the dialog closes when the mailbox rebuild drops the last question
    /// rather than in the button handlers. Every dismissal path without a
    /// decision (Escape, backdrop press, close icon) is disabled: the
    /// question parks its turn and has no deadline left once the shell has
    /// shown it, so the only ways out are Submit and Cancel.
    pub(super) fn open_question_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let Some(view) = view.upgrade() else {
                return dialog;
            };
            let Some(item) = view.read(cx).pending_questions.first().cloned() else {
                return dialog;
            };
            let text = i18n::current(cx);
            dialog
                .title(div().child(text.question_title.clone()))
                .overlay_closable(false)
                .keyboard(false)
                .close_button(false)
                .child(question_body(&view.downgrade(), &item, &text, cx))
                .footer(question_footer(
                    &item,
                    &text,
                    submit_question,
                    cancel_question,
                ))
        });
        self.modal_owner = Some(ModalOwner::Questions);
    }

    /// Opens the next waiting modal unless one is already open. Approvals
    /// take the slot first: they gate a tool call that is otherwise parked,
    /// while an attended question waits on the mailbox without a deadline.
    pub(super) fn open_next_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal_owner.is_some() {
            return;
        }
        match next_modal_target(self.pending_permissions.len(), self.pending_questions.len()) {
            Some(ModalOwner::Permissions) => self.open_permission_dialog(window, cx),
            Some(ModalOwner::Questions) => self.open_question_dialog(window, cx),
            None => {}
        }
    }

    /// Releases the dialog slot once `owner`'s queue drained, then hands it
    /// to the next waiting queue. No-op while the other queue owns the slot.
    pub(super) fn release_modal(
        &mut self,
        owner: ModalOwner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal_owner != Some(owner) {
            return;
        }
        window.close_dialog(cx);
        self.modal_owner = None;
        self.open_next_modal(window, cx);
    }

    /// Rebuilds the question queue from a mailbox snapshot. Existing items are
    /// carried over so selections survive an unrelated question arriving or
    /// leaving; only new tool ids are parsed.
    pub(super) fn apply_questions_changed(
        &mut self,
        questions: Vec<PendingUserQuestion>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_questions =
            rebuild_question_queue(&self.pending_questions, questions, |index, total| {
                i18n::question_progress(index, total, cx)
            });
        if self.pending_questions.is_empty() {
            self.release_modal(ModalOwner::Questions, window, cx);
        } else {
            self.open_next_modal(window, cx);
        }
    }

    /// Toggles one option of the dialog's head item: single-select replaces
    /// the selection, multi-select adds or removes it.
    pub(super) fn toggle_question_option(
        &mut self,
        tool_id: &str,
        question_index: usize,
        option_index: usize,
    ) {
        let Some(item) = self
            .pending_questions
            .iter_mut()
            .find(|item| item.tool_id == tool_id)
        else {
            return;
        };
        let Some(question) = item.questions.get(question_index) else {
            return;
        };
        let multi_select = question.multi_select;
        let Some(selected) = item.selected.get_mut(question_index) else {
            return;
        };
        toggle_option_selection(selected, option_index, multi_select);
    }
}

/// Applies one option click: single-select replaces the selection,
/// multi-select adds or removes the option.
pub(super) fn toggle_option_selection(
    selected: &mut BTreeSet<usize>,
    option_index: usize,
    multi_select: bool,
) {
    if multi_select {
        if !selected.remove(&option_index) {
            selected.insert(option_index);
        }
    } else {
        selected.clear();
        selected.insert(option_index);
    }
}

/// Forwards one user reply to the embedded runtime. The queue and dialog
/// update on the broadcast echo, not here, so a failed answer leaves the
/// prompt standing (the request is still pending).
pub(super) fn answer_permission(request_id: String, reply: PermissionReply) {
    if let Some(handle) = runtime::runtime_handle() {
        handle.respond_permission(request_id, reply);
    }
}

/// Forwards the dialog's submitted answers to the embedded runtime. The queue
/// advances on the mailbox rebuild, not here.
/// Splits a `/btw <question>` composer input into its question. `None` when
/// the text is not the command ("/btwfoo" is just a message); `Some("")`
/// when the command is there but the question is missing.
pub(super) fn btw_question(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix("/btw")?;
    if rest.is_empty() {
        Some("")
    } else if rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

pub(super) fn submit_question(tool_id: String, answers: serde_json::Value) {
    if let Some(handle) = runtime::runtime_handle() {
        handle.respond_user_question(tool_id, answers);
    }
}

/// Declines one question on the user's behalf; the tool reports the
/// cancellation back to the model.
pub(super) fn cancel_question(session_id: String, tool_id: String) {
    if let Some(handle) = runtime::runtime_handle() {
        handle.cancel_user_question(session_id, tool_id);
    }
}

/// Dialog body for one question batch: every question of the batch with its
/// options, scrollable so a long batch stays inside the dialog.
pub(super) fn question_body(
    view: &WeakEntity<ShellWindow>,
    item: &QuestionPromptItem,
    text: &ShellText,
    cx: &App,
) -> impl IntoElement {
    let mut body = div()
        .v_flex()
        .gap_4()
        .w(px(520.))
        .max_h(px(420.))
        .overflow_y_scrollbar()
        .text_sm();
    if item.questions.is_empty() {
        return body.child(
            div()
                .text_color(cx.theme().warning)
                .child(text.question_unreadable.clone()),
        );
    }
    for (index, question) in item.questions.iter().enumerate() {
        let heading = if question.header.trim().is_empty() {
            question.progress_line.clone()
        } else {
            format!("{} · {}", question.header, question.progress_line)
        };
        let mut options = div().v_flex().gap_2();
        for (option_index, option) in question.options.iter().enumerate() {
            let selected = item
                .selected
                .get(index)
                .is_some_and(|set| set.contains(&option_index));
            options = options.child(question_option_row(
                view.clone(),
                item.tool_id.clone(),
                index,
                option_index,
                option,
                question.multi_select,
                selected,
                cx,
            ));
        }
        body = body.child(
            div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(heading),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().foreground)
                                .child(question.question.clone()),
                        )
                        .when(question.multi_select, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.question_multi.clone()),
                            )
                        }),
                )
                .child(options),
        );
    }
    body
}

/// One clickable option row. Selection lives on the shell item, so the click
/// updates that item and the dialog's next frame shows the new state.
#[allow(clippy::too_many_arguments)]
pub(super) fn question_option_row(
    view: WeakEntity<ShellWindow>,
    tool_id: String,
    question_index: usize,
    option_index: usize,
    option: &PromptOption,
    multi_select: bool,
    selected: bool,
    cx: &App,
) -> impl IntoElement {
    let marker = match (multi_select, selected) {
        (true, true) => IconName::Check,
        (true, false) => IconName::Square,
        (false, true) => IconName::CircleDot,
        (false, false) => IconName::Circle,
    };
    div()
        .id(SharedString::from(format!(
            "question-option-{tool_id}-{question_index}-{option_index}"
        )))
        .cursor_pointer()
        .h_flex()
        .items_start()
        .gap_2()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if selected {
            cx.theme().primary
        } else {
            cx.theme().border
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().muted))
        })
        .child(Icon::new(marker).size_4().text_color(if selected {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        }))
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .min_w_0()
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .child(option.label.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(option.description.clone()),
                ),
        )
        .on_click(move |_, _, cx| {
            if let Some(shell) = view.upgrade() {
                shell.update(cx, |this, cx| {
                    this.toggle_question_option(&tool_id, question_index, option_index);
                    cx.notify();
                });
            }
        })
}

/// Dialog footer: Cancel (declines the question) and Submit, disabled until
/// every question has a selection (same affordance as the composer's send
/// button). The callbacks are injected so tests can observe the decisions
/// without a runtime.
pub(super) fn question_footer(
    item: &QuestionPromptItem,
    text: &ShellText,
    on_submit: impl Fn(String, serde_json::Value) + Clone + 'static,
    on_cancel: impl Fn(String, String) + Clone + 'static,
) -> gpui_kit::Div {
    let submit_tool_id = item.tool_id.clone();
    let answers_item = item.clone();
    let cancel_session_id = item.session_id.clone();
    let cancel_tool_id = item.tool_id.clone();
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            Button::new("question-cancel")
                .label(text.question_cancel.clone())
                .with_variant(ButtonVariant::Ghost)
                .on_click({
                    let on_cancel = on_cancel.clone();
                    move |_, _, _| {
                        on_cancel(cancel_session_id.clone(), cancel_tool_id.clone());
                    }
                }),
        )
        .child(
            Button::new("question-submit")
                .label(text.question_submit.clone())
                .with_variant(ButtonVariant::Primary)
                .disabled(!item.is_answered())
                .on_click({
                    let on_submit = on_submit.clone();
                    move |_, _, _| {
                        on_submit(submit_tool_id.clone(), answers_item.answers());
                    }
                }),
        )
}

/// Composes the display lines for one arriving request. Everything the dialog
/// shows is resolved here against the active locale, mirroring the CLI prompt
/// content (`src/apps/cli/src/ui/permission.rs`); the render path stays free
/// of i18n service calls.
pub(super) fn compose_permission_item(
    request: PermissionRequest,
    text: &ShellText,
    cx: &App,
) -> PermissionPromptItem {
    let resources_line = if request.resources.is_empty() {
        None
    } else {
        let resources = request
            .resources
            .iter()
            .map(|resource| runtime::truncate_chars(resource, 80))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("{}: {resources}", text.permission_resources))
    };
    let project = request
        .project_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(&request.project_id);
    let project_line = format!("{}: {}", text.permission_project, project);
    let delegation_line = request.delegation.as_ref().map(|delegation| {
        format!(
            "{}: {} · {}",
            i18n::permission_subagent_label(&delegation.subagent_type, cx),
            delegation.subagent_type,
            request.session_id
        )
    });
    let risk_line = request
        .display_metadata
        .get("riskDescription")
        .or_else(|| request.display_metadata.get("risk"))
        .and_then(serde_json::Value::as_str)
        .filter(|risk| !risk.trim().is_empty())
        .map(|risk| {
            format!(
                "{}: {}",
                text.permission_risk,
                runtime::truncate_chars(risk, 100)
            )
        });
    PermissionPromptItem {
        resources_line,
        project_line,
        delegation_line,
        risk_line,
        request,
    }
}

/// Dialog body: the action as the headline, then the composed context lines.
pub(super) fn permission_body(item: &PermissionPromptItem, cx: &App) -> gpui_kit::Div {
    let mut body = div()
        .v_flex()
        .gap_1p5()
        .text_sm()
        .child(
            div()
                .text_color(cx.theme().foreground)
                .truncate()
                .child(item.request.action.clone()),
        )
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(item.project_line.clone()),
        );
    if let Some(line) = &item.resources_line {
        body = body.child(
            div()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(line.clone()),
        );
    }
    if let Some(line) = &item.delegation_line {
        body = body.child(
            div()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(line.clone()),
        );
    }
    if let Some(line) = &item.risk_line {
        body = body.child(
            div()
                .text_color(cx.theme().warning)
                .truncate()
                .child(line.clone()),
        );
    }
    body
}

/// Dialog footer: Deny / Always allow / Allow once. The reply callback is
/// injected so tests can observe the decisions without a runtime.
pub(super) fn permission_footer(
    item: &PermissionPromptItem,
    text: &ShellText,
    on_reply: impl Fn(String, PermissionReply) + Clone + 'static,
) -> gpui_kit::Div {
    let deny_id = item.request_id().to_string();
    let always_id = item.request_id().to_string();
    let once_id = item.request_id().to_string();
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            Button::new("permission-deny")
                .label(text.permission_deny.clone())
                .with_variant(ButtonVariant::Danger)
                .on_click({
                    let on_reply = on_reply.clone();
                    move |_, _, _| {
                        on_reply(deny_id.clone(), PermissionReply::Reject { feedback: None });
                    }
                }),
        )
        .child(
            Button::new("permission-always")
                .label(text.permission_allow_always.clone())
                .on_click({
                    let on_reply = on_reply.clone();
                    move |_, _, _| {
                        on_reply(always_id.clone(), PermissionReply::Always);
                    }
                }),
        )
        .child(
            Button::new("permission-once")
                .label(text.permission_allow_once.clone())
                .with_variant(ButtonVariant::Primary)
                .on_click(move |_, _, _| {
                    on_reply(once_id.clone(), PermissionReply::Once);
                }),
        )
}
