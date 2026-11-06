// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

/// One open session in the chat pane; one per session tab (R1a multi-tab).
pub(super) struct ChatView {
    pub(super) session_id: String,
    pub(super) name: String,
    pub(super) agent_type: String,
    /// The session's current model selector; `None` means the runtime default.
    pub(super) model_id: Option<String>,
    pub(super) entries: Vec<ChatEntry>,
    /// Virtualized transcript state for this tab (the scroller moved here from
    /// the window when sessions became tabs: each tab follows its own scroll
    /// position, and background sessions skip live syncs while hidden).
    pub(super) scroller: Entity<MessageScrollerState>,
    /// A turn is executing (streaming or tool calls in flight).
    pub(super) busy: bool,
    /// A submission is in flight; the optimistic user entry awaits
    /// [`RuntimeEvent::TurnAccepted`].
    pub(super) submitting: bool,
    /// The pane shows a hidden child session (subagent run or `/btw` side
    /// question, M20): the header grows its badge and the "back to parent"
    /// entry point. `None` = a normal, user-visible session.
    pub(super) child_context: Option<ChildContext>,
    /// User prompts expanded past the three-line clamp (R2d), keyed by the
    /// content hash (`markdown_id_key`) so the flag survives transcript
    /// reloads that re-index entries.
    pub(super) expanded_user_texts: BTreeSet<u64>,
    /// Explicit open/closed choices for work runs (M24), keyed by the run's
    /// head tool key hash — same stability rule as the user bubbles. Absent
    /// means automatic: a collecting run stays open, a settled one folds.
    pub(super) expanded_groups: BTreeMap<u64, bool>,
    /// G-S1: per-turn wall-clock timing for the transcript footer. A
    /// transcript read fills every persisted turn; live turns add theirs as
    /// they start and settle.
    pub(super) turn_times: HashMap<String, runtime::TurnTimes>,
    /// The turn currently executing for this session (set from its start
    /// event), so the finish event can close the matching timing row.
    pub(super) active_turn_id: Option<String>,
}

/// Why the chat pane is showing a hidden child session (M20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildKind {
    /// A subagent run opened from its parent tool card.
    Subagent,
    /// A `/btw` side-question session.
    Btw,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildContext {
    pub kind: ChildKind,
    pub parent_session_id: String,
    /// Subagent type of a linked run (badge detail); `None` for `/btw`.
    pub agent_type: Option<String>,
    /// The coordinator dialog turn of a `/btw` child, the stop button's
    /// target; `None` for subagent runs, which cancel through the session
    /// loop.
    pub btw_turn_id: Option<String>,
}

/// One hidden subagent session linked to its parent tool call (M20). Kept on
/// the window so the mapping survives transcript reloads; tool cards are
/// re-annotated from here whenever a transcript (re)loads.
#[derive(Debug, Clone)]
pub(crate) struct SubagentLink {
    pub parent_session_id: String,
    pub tool_call_id: String,
    pub subagent_session_id: String,
    pub agent_type: Option<String>,
    /// Parent-scoped alias the run was launched with (`agent_id`), when the
    /// source reported one. Identity falls back to the child session id.
    pub agent_id: Option<String>,
}

/// Which review flavor a linked subagent belongs to (T2-6, the web review
/// badge): deep review runs report `DeepReview`, the plain review family
/// (worker/judge/fixer, `CodeReview`, and the legacy `Review*` ids) reports
/// its own type. Web derives this from a client-set session kind; the shell
/// classifies the agent type the runtime already reports on the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewActivityKind {
    Review,
    DeepReview,
}

pub(super) fn review_activity_kind(agent_type: &str) -> Option<ReviewActivityKind> {
    if agent_type == "DeepReview" {
        Some(ReviewActivityKind::DeepReview)
    } else if agent_type.starts_with("Review") || agent_type == "CodeReview" {
        Some(ReviewActivityKind::Review)
    } else {
        None
    }
}

/// A child session the user asked to open whose transcript is still loading
/// (`RuntimeEvent::SessionOpened` settles it into the chat's child context).
#[derive(Debug, Clone)]
pub(super) struct PendingChild {
    pub(super) child_session_id: String,
    pub(super) context: ChildContext,
}

impl ChatView {
    pub(super) fn new(
        session_id: String,
        name: String,
        agent_type: String,
        model_id: Option<String>,
        entries: Vec<ChatEntry>,
        turn_times: HashMap<String, runtime::TurnTimes>,
        cx: &mut Context<ShellWindow>,
    ) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));
        Self {
            session_id,
            name,
            agent_type,
            model_id,
            entries,
            scroller,
            busy: false,
            submitting: false,
            child_context: None,
            expanded_user_texts: BTreeSet::new(),
            expanded_groups: BTreeMap::new(),
            turn_times,
            active_turn_id: None,
        }
    }

    pub(super) fn matches(&self, session_id: &str) -> bool {
        self.session_id == session_id
    }

    /// Streaming chunks accumulate into one assistant entry per model round;
    /// rounds arrive contiguously, so comparing against the tail is enough.
    /// Returns `true` when a new entry was pushed (as opposed to growing the
    /// tail entry in place), so the caller can sync the virtualized list.
    pub(super) fn append_chunk(&mut self, round_id: &str, text: &str) -> bool {
        if let Some(ChatEntry::Assistant {
            round_id: tail_round,
            text: tail_text,
        }) = self.entries.last_mut()
        {
            if *tail_round == round_id {
                tail_text.push_str(text);
                return false;
            }
        }
        self.entries.push(ChatEntry::Assistant {
            round_id: round_id.to_string(),
            text: text.to_string(),
        });
        true
    }

    /// Whether one work run is open. An explicit reader choice wins; without
    /// one a run that is still collecting stays open and folds by itself when
    /// the turn settles (web `getFlowGroupDisclosureChoice`).
    pub(super) fn group_expanded(&self, group: &transcript::ExploreGroup) -> bool {
        let key = group_head_key(&self.entries, group.head);
        self.expanded_groups.get(&key).copied().unwrap_or_else(|| {
            (self.busy || self.submitting) && group.head + group.len >= self.entries.len()
        })
    }

    /// Flips one run open or closed, returning its span so the caller can
    /// remeasure exactly those rows.
    pub(super) fn toggle_group(&mut self, group: &transcript::ExploreGroup) -> (usize, usize) {
        let key = group_head_key(&self.entries, group.head);
        let expanded = self.group_expanded(group);
        self.expanded_groups.insert(key, !expanded);
        (group.head, group.len)
    }

    /// Upserts the tool card keyed by tool id. Lifecycle events carry the
    /// facts unevenly — input on `Started`, counts on `Completed` — so an
    /// update merges into what the row already knows instead of replacing it.
    /// Returns the index of an existing card that changed in place, or `None`
    /// when the card was appended.
    pub(super) fn upsert_tool(&mut self, card: ToolCard) -> Option<usize> {
        let existing = self.entries.iter_mut().enumerate().find(
            |(_, entry)| matches!(entry, ChatEntry::Tool(existing) if existing.key == card.key),
        );
        if let Some((index, entry)) = existing {
            let ChatEntry::Tool(existing) = entry else {
                unreachable!("find matched a tool entry");
            };
            if !card.tool_name.is_empty() {
                existing.tool_name = card.tool_name;
            }
            existing.status = card.status;
            existing.facts = transcript::merge_facts(&existing.facts, &card.facts);
            if card.result_success.is_some() {
                existing.result_success = card.result_success;
            }
            if card.subagent_session.is_some() {
                existing.subagent_session = card.subagent_session;
            }
            existing.background = existing.background || card.background;
            Some(index)
        } else {
            self.entries.push(ChatEntry::Tool(card));
            None
        }
    }
}

/// Everything the per-turn fork menu and the subagent "view run" entry point
/// need; `None` in list harnesses that render entries without a live session
/// attached.
#[derive(Clone)]
pub(super) struct ChatForkContext {
    pub(super) session_id: String,
    pub(super) tx: mpsc::UnboundedSender<RuntimeEvent>,
    /// Weak handle back to the shell window: the subagent run entry point
    /// settles the pending child context before opening the child session.
    pub(super) view: gpui_kit::WeakEntity<ShellWindow>,
}

/// One transcript entry as a scroller row. Row insets and the inter-row gap
/// belong to the [`MessageScroller`] row wrapper plus the spacing the chat
/// area's row closure applies (R2d); the reading column is applied by that
/// closure around this item.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_chat_item(
    entry: &ChatEntry,
    text: &ShellText,
    fork: Option<&ChatForkContext>,
    identity: Option<&SubagentIdentity>,
    roster: &[SubagentIdentity],
    row: usize,
    user_expanded: bool,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    match entry {
        ChatEntry::User {
            text: body,
            turn_id: _,
        } => {
            // Content-hash key: the expansion flag survives transcript
            // reloads that re-index entries; the row index keeps the element
            // id unique when the same prompt text appears twice.
            let key = markdown_id_key(body);
            let column = div()
                .debug_selector(|| "user-bubble".to_string())
                .id(ElementId::Name(
                    format!("user-bubble-{row}-{:016x}", key).into(),
                ))
                .w_full()
                // A column wrapper on purpose: inside a row-flex `justify_end`
                // parent the Bubble lays out but never paints (gpui-pre 0.7.1
                // stack, reproduced 2026-10-09). The web fraction ships as a
                // definite pixel cap (`min(72%, 48rem)` at the 900px column):
                // a relative() max-width does not constrain the content inside
                // the virtualized row (measured 2026-10-12), so the shell owns
                // the cap and the right alignment on its own wrapper.
                .v_flex()
                .cursor_pointer()
                // Web `UserMessageItem`: three lines collapsed, click expands
                // (the toggle is free — the bubble has no other interaction).
                .on_click({
                    let view = fork.map(|fork| fork.view.clone());
                    let session_id = fork.map(|fork| fork.session_id.clone());
                    move |_, _, cx| {
                        if let (Some(view), Some(session_id)) = (&view, &session_id) {
                            let _ = view.update(cx, |shell, cx| {
                                if let Some(chat) = shell.chat_mut_for(session_id) {
                                    if chat.expanded_user_texts.contains(&key) {
                                        chat.expanded_user_texts.remove(&key);
                                    } else {
                                        chat.expanded_user_texts.insert(key);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }
                })
                .child(
                    div()
                        .debug_selector(|| "user-bubble-cap".to_string())
                        .self_end()
                        .max_w(px(USER_BUBBLE_MAX_WIDTH))
                        .min_w_0()
                        .child(
                            Bubble::new()
                                .with_variant(BubbleVariant::Muted)
                                // Web surface recipe (R2d): the component's
                                // default radius_2xl/padding read visibly
                                // airier than the web's 10px/10px/7px bubble.
                                .content(
                                    BubbleContent::new()
                                        .rounded(px(USER_BUBBLE_RADIUS))
                                        .px(px(USER_BUBBLE_PADDING_X))
                                        .py(px(USER_BUBBLE_PADDING_Y)),
                                )
                                .w_full()
                                .child(
                                    div()
                                        .when(!user_expanded, |this| this.line_clamp(3))
                                        .child(body.clone()),
                                ),
                        ),
                );
            column.into_any_element()
        }
        ChatEntry::FailedUser { text: body, error } => {
            // Web `UserMessageItem` failed phase: the prompt keeps its bubble
            // (danger-framed) above an alert row whose "continue editing"
            // refills the composer and retires the entry.
            let alert: SharedString = format!("{}: {error}", i18n::chat_send_failed(cx)).into();
            let continue_label: SharedString = i18n::chat_continue_editing(cx).into();
            let column = div()
                .w_full()
                .v_flex()
                .child(
                    div()
                        .debug_selector(|| "user-bubble-failed".to_string())
                        .self_end()
                        .max_w(px(USER_BUBBLE_MAX_WIDTH))
                        .min_w_0()
                        .border_1()
                        .border_color(cx.theme().danger)
                        .rounded(px(USER_BUBBLE_RADIUS))
                        .child(
                            Bubble::new()
                                .with_variant(BubbleVariant::Muted)
                                .content(
                                    BubbleContent::new()
                                        .rounded(px(USER_BUBBLE_RADIUS))
                                        .px(px(USER_BUBBLE_PADDING_X))
                                        .py(px(USER_BUBBLE_PADDING_Y)),
                                )
                                .w_full()
                                .child(div().line_clamp(3).child(body.clone())),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .mt(px(FLOW_ITEM_GAP))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(alert)
                        .when_some(fork, |this, fork| {
                            let view = fork.view.clone();
                            let session_id = fork.session_id.clone();
                            let body = body.clone();
                            this.child(
                                Button::new(SharedString::from(format!("failed-continue-{row}")))
                                    .label(continue_label)
                                    .small()
                                    .with_variant(ButtonVariant::Secondary)
                                    .on_click(move |_, _window, cx| {
                                        let _ = view.update(cx, |shell, cx| {
                                            shell.prefill_composer(body.clone(), cx);
                                            if let Some(chat) = shell.chat_mut_for(&session_id) {
                                                chat.entries.retain(|entry| {
                                                    !matches!(entry, ChatEntry::FailedUser { .. })
                                                });
                                            }
                                            cx.notify();
                                        });
                                    }),
                            )
                        }),
                );
            column.into_any_element()
        }
        ChatEntry::Assistant { text: body, .. } => {
            // The rich-render hooks the desktop MarkdownRenderer wires: links
            // route by target (external opener vs the shell's file viewer),
            // and fenced code gets a language label plus a copy action.
            let link_view = fork.map(|fork| fork.view.clone());
            let copy_label: SharedString = text.code_copy.clone().into();
            let copied_label: SharedString = text.code_copied.clone().into();
            div()
                .w_full()
                .min_w_0()
                .text_sm()
                .child(
                    TextView::markdown(
                        SharedString::from(format!("entry-{}-md", markdown_id_key(body))),
                        body.clone(),
                    )
                    .selectable(true)
                    .on_link_click(move |url, _event, _window, cx| {
                        let target = markdown::classify_link(url);
                        match target {
                            markdown::LinkTarget::External(url) => {
                                markdown::open_external(&url);
                            }
                            markdown::LinkTarget::File { path, line } => {
                                let Some(view) = link_view.as_ref().and_then(|view| view.upgrade())
                                else {
                                    return;
                                };
                                view.update(cx, |shell, cx| {
                                    shell.open_transcript_path(path, line, cx);
                                });
                            }
                            markdown::LinkTarget::Unsupported => {}
                        }
                    })
                    .code_block_actions(move |block, _window, cx| {
                        markdown::code_block_toolbar(
                            block,
                            copy_label.clone(),
                            copied_label.clone(),
                            cx,
                        )
                    }),
                )
                .into_any_element()
        }
        ChatEntry::Tool(card) => {
            // The "view run" entry point needs the shell handle; harnesses
            // that render without a fork render bare cards (M20).
            let run = fork.filter(|_| {
                card.subagent_session.is_some()
                    || identity.is_some_and(|identity| identity.subagent_session.is_some())
            });
            render_tool_card(card, identity, roster, run, fork, text, cx).into_any_element()
        }
        ChatEntry::Notice { text: body } => div()
            .w_full()
            .text_sm()
            .text_color(cx.theme().danger)
            .child(body.clone())
            .into_any_element(),
        ChatEntry::Info { text: body } => div()
            .w_full()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(body.clone())
            .into_any_element(),
    }
}

/// What the turn footer (R2d) shows under a finished turn's tail entry: the
/// fork boundary and the copy scopes derived from the turn's own rows. Zero
/// new runtime facts — everything comes from the transcript vec.
pub(super) struct TurnFooterPlan {
    /// The turn's user prompt id (fork boundary); `None` leaves the fork
    /// entry out (transcript head without a persisted prompt id).
    pub(super) turn_id: Option<String>,
    /// G-S1: the turn's completion meta when the transcript or the live turn
    /// events recorded it (web `buildModelRoundCompletionMeta`).
    pub(super) meta: Option<TurnMeta>,
    /// Whole-turn copy text (`User\n…` + every assistant round, web
    /// `formatTranscriptTurnText` shape); `None` when the turn has no
    /// assistant text and the copy entry hides.
    pub(super) copy_full: Option<String>,
    /// Result-scope copy: the final assistant round.
    pub(super) copy_result: Option<String>,
}

/// G-S1: one finished turn's footer meta — the completion clock and the
/// duration, which the web renders as `Completed HH:MM:SS` / `Duration 12s`
/// (the labels ride tooltips here; the values are the visible text).
pub(super) struct TurnMeta {
    pub(super) completed_at_ms: u64,
    pub(super) duration_ms: Option<u64>,
    pub(super) cancelled: bool,
}

/// The finished-turn tail test for row `index`: the entry must close a turn
/// (the next row is a user prompt, or the transcript ends) and that turn must
/// be complete — a tail row while `busy`/`submitting` is still streaming.
/// Notice/info/failed rows carry no actions of their own and never host the
/// footer.
pub(super) fn turn_footer_plan(
    entries: &[ChatEntry],
    index: usize,
    busy: bool,
    submitting: bool,
    text: &ShellText,
    turn_times: &HashMap<String, runtime::TurnTimes>,
) -> Option<TurnFooterPlan> {
    match entries.get(index)? {
        ChatEntry::User { .. }
        | ChatEntry::FailedUser { .. }
        | ChatEntry::Notice { .. }
        | ChatEntry::Info { .. } => {
            return None;
        }
        ChatEntry::Assistant { .. } | ChatEntry::Tool(_) => {}
    }
    match entries.get(index + 1) {
        Some(ChatEntry::User { .. }) => {}
        // Transcript tail: only a finished turn hosts a footer.
        None if busy || submitting => return None,
        None => {}
        Some(_) => return None,
    }
    // Backward scan to the turn's prompt: the first assistant met is the
    // final round (the copy result), the prompt supplies the fork boundary.
    let mut rounds: Vec<&str> = Vec::new();
    let mut turn: Option<(Option<String>, String)> = None;
    for entry in entries[..=index].iter().rev() {
        match entry {
            ChatEntry::User { text, turn_id } => {
                turn = Some((turn_id.clone(), text.clone()));
                break;
            }
            ChatEntry::Assistant { text, .. } => rounds.push(text),
            _ => {}
        }
    }
    let copy_result = rounds.first().map(|text| (*text).to_string());
    let copy_full = if rounds.is_empty() {
        None
    } else {
        let mut parts = Vec::new();
        if let Some((_, prompt)) = &turn {
            parts.push(format!("{}\n{}", text.turn_copy_user_label, prompt));
        }
        parts.extend(rounds.iter().rev().map(|text| (*text).to_string()));
        Some(parts.join("\n\n---\n\n"))
    };
    let turn_id = turn.map(|(turn_id, _)| turn_id).flatten();
    // G-S1: the meta needs the turn's timing, which a transcript read
    // (persisted message timestamps) or the live turn events supply. A turn
    // without a clock renders no meta, same as the web's absent `endTime`.
    let meta = turn_id
        .as_ref()
        .and_then(|turn_id| turn_times.get(turn_id))
        .filter(|times| !times.is_empty())
        .and_then(|times| {
            times.ended_at_ms.map(|completed_at_ms| TurnMeta {
                completed_at_ms,
                duration_ms: times.effective_duration_ms(),
                cancelled: times.cancelled,
            })
        });
    Some(TurnFooterPlan {
        turn_id,
        meta,
        copy_full,
        copy_result,
    })
}

/// The finished-turn footer (R2d, web `ModelRoundItem` metadata row): fork /
/// copy / snapshots, right-aligned under the turn's last entry. Every entry
/// hooks an existing capability — the per-turn fork menu, the clipboard, the
/// git snapshots view.
pub(super) fn turn_footer(
    plan: TurnFooterPlan,
    row: usize,
    fork: &ChatForkContext,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    // G-S1: the meta sits where the web puts it — a muted run left of the
    // action row (`margin-inline-end: auto` there, one flex row here).
    let mut footer = div()
        .debug_selector(|| "turn-footer".to_string())
        .mt_1()
        .flex()
        .items_center()
        .w_full()
        .gap_2();
    if let Some(meta) = &plan.meta {
        let mut meta_row = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        meta_row = meta_row.child(turn_meta_item(
            if meta.cancelled {
                text.turn_meta_stopped.clone()
            } else {
                text.turn_meta_completed.clone()
            },
            runtime::format_clock_time(meta.completed_at_ms),
        ));
        if let Some(duration_ms) = meta.duration_ms {
            meta_row = meta_row.child(turn_meta_item(
                text.turn_meta_duration.clone(),
                runtime::format_elapsed_duration(duration_ms),
            ));
        }
        footer = footer.child(meta_row);
    }
    let mut actions = div().flex().items_center().justify_end().gap_0p5();
    if let Some(turn_id) = plan.turn_id {
        actions = actions.child(turn_fork_menu(
            fork.session_id.clone(),
            turn_id,
            fork.tx.clone(),
            text,
        ));
    }
    if let (Some(result), Some(full)) = (plan.copy_result, plan.copy_full) {
        actions = actions.child(
            Button::new(ElementId::Name(format!("turn-copy-{row}").into()))
                .icon(IconName::Copy)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.turn_copy.clone())
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
                    let text = i18n::current(cx);
                    let full = full.clone();
                    let result = result.clone();
                    menu.item(PopupMenuItem::new(text.turn_copy_full.clone()).on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(full.clone()));
                        },
                    ))
                    .item(
                        PopupMenuItem::new(text.turn_copy_result.clone()).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(result.clone()));
                            },
                        ),
                    )
                }),
        );
    }
    let view = fork.view.clone();
    actions = actions.child(
        Button::new(ElementId::Name(format!("turn-snapshot-{row}").into()))
            .icon(IconName::Archive)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.git_snapshots_view.clone())
            .on_click(move |_, window, cx| {
                if let Some(view) = view.upgrade() {
                    let _ = view.update(cx, |shell, cx| shell.open_git_snapshots(window, cx));
                }
            }),
    );
    footer.child(actions).into_any_element()
}

/// One G-S1 footer meta item (web `model-round-item__meta-item`): the value is
/// the visible text, the label rides its tooltip (the web keeps the label in
/// `aria-label`).
pub(super) fn turn_meta_item(label: String, value: String) -> gpui_kit::AnyElement {
    let shown = SharedString::from(value);
    let hover = SharedString::from(format!("{label}: {shown}"));
    div()
        .id(SharedString::from(format!("turn-meta-{shown}")))
        .flex_shrink_0()
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hover.clone()).build(window, cx)
        })
        .child(shown)
        .into_any_element()
}

/// Per-turn menu under a user prompt: fork including the selected turn, from
/// just before it, or roll the session back to the turn boundary (M19). Forks
/// open the copy when the runtime confirms; rollback reverts transcript and
/// workspace files in place and refreshes through the history-changed event.
pub(super) fn turn_fork_menu(
    session_id: String,
    turn_id: String,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
) -> gpui_kit::AnyElement {
    let at_action = {
        let session_id = session_id.clone();
        let turn_id = turn_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.fork_session_turn(session_id.clone(), turn_id.clone(), false, tx.clone());
            }
        }
    };
    let before_action = {
        let session_id = session_id.clone();
        let turn_id = turn_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.fork_session_turn(session_id.clone(), turn_id.clone(), true, tx.clone());
            }
        }
    };
    let rollback_action = {
        let session_id = session_id.clone();
        let turn_id = turn_id.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.rollback_session_to_turn(session_id.clone(), turn_id.clone(), tx.clone());
            }
        }
    };
    Button::new(SharedString::from(format!("turn-fork-{turn_id}")))
        .icon(IconName::GitBranch)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .tooltip(text.turn_menu.clone())
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
            let text = i18n::current(cx);
            menu.item(PopupMenuItem::new(text.fork_at_turn.clone()).on_click(at_action.clone()))
                .item(
                    PopupMenuItem::new(text.fork_before_turn.clone())
                        .on_click(before_action.clone()),
                )
                .separator()
                .item(
                    PopupMenuItem::new(text.rollback_at_turn.clone())
                        .on_click(rollback_action.clone()),
                )
        })
        .into_any_element()
}

/// Stable per-content markdown element id: transcript indices shift when
/// history reloads swap the entries vec, and a reused id pointing at new
/// content would briefly show the previous parse while the worker catches up.
pub(super) fn markdown_id_key(body: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(body, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}
