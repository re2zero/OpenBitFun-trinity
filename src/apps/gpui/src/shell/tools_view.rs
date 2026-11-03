// All items resolve through the parent `shell` scope (see shell.rs).

#![allow(unused_imports)]

use super::*;

impl ShellWindow {
    /// Re-annotates every open transcript's tool cards from the subagent link
    /// map (M20). Called on every transcript load and on each new link, so
    /// reloads never lose the "view run" entry points; session tabs all show
    /// links of their parent, so each open chat re-derives its own.
    /// Adds the missing links a restored transcript cannot carry: lineage
    /// entries record `parent_tool_call_id` for every subagent child, so the
    /// chat can resolve a spawn card to its child session (and therefore to
    /// the identity the run shows on the web UI). Live links win for a
    /// repeated call id — they also carry the link event's agent type.
    pub(super) fn seed_links_from_lineages(&mut self) {
        let mut additions: Vec<SubagentLink> = Vec::new();
        for chat in &self.chats {
            let Some(snapshot) = self
                .lineages
                .iter()
                .find(|snapshot| snapshot.root_session_id == chat.session_id)
            else {
                continue;
            };
            for entry in &snapshot.sessions {
                let (Some(parent), Some(tool_call_id)) = (
                    entry.parent_session_id.as_deref(),
                    entry.parent_tool_call_id.as_deref(),
                ) else {
                    continue;
                };
                if parent != chat.session_id || tool_call_id.is_empty() {
                    continue;
                }
                let known = self.subagent_links.iter().any(|link| {
                    link.parent_session_id == parent && link.tool_call_id == tool_call_id
                }) || additions.iter().any(|link| {
                    link.parent_session_id == parent && link.tool_call_id == tool_call_id
                });
                if known {
                    continue;
                }
                additions.push(SubagentLink {
                    parent_session_id: parent.to_string(),
                    tool_call_id: tool_call_id.to_string(),
                    subagent_session_id: entry.session_id.clone(),
                    agent_type: entry.subagent_type.clone(),
                    agent_id: entry.agent_id.clone(),
                });
            }
        }
        self.subagent_links.extend(additions);
    }

    /// The identity seed and run chips for one agent-family tool card: the
    /// linked child session id when known (live link or lineage), else the
    /// alias the call itself named. Same seeding rule as the web UI, so a run
    /// keeps one name and avatar across surfaces.
    pub(super) fn subagent_identity_for(&self, card: &ToolCard) -> Option<SubagentIdentity> {
        let alias = card.facts.agents.first().map(|agent| agent.alias.clone());
        let link = self
            .subagent_links
            .iter()
            .find(|link| link.tool_call_id == card.call_id);
        let seed = link
            .map(|link| link.subagent_session_id.clone())
            .filter(|seed| !seed.is_empty())
            .or_else(|| alias.clone())
            .filter(|seed| !seed.is_empty())?;
        let agent = card.facts.agents.first();
        let subagent_session = link.map(|link| link.subagent_session_id.clone());
        Some(SubagentIdentity {
            seed,
            alias: alias.filter(|alias| !alias.is_empty()),
            agent_type: agent
                .and_then(|agent| agent.agent_type.clone())
                .or_else(|| link.and_then(|link| link.agent_type.clone())),
            model: agent.and_then(|agent| agent.model.clone()),
            status: agent.and_then(|agent| agent.status.clone()),
            running: subagent_session
                .as_deref()
                .is_some_and(|session_id| self.live_sessions.contains(session_id)),
            subagent_session,
        })
    }

    /// Indexes the transcript's subagent launches by background task id and
    /// by alias, so wait/list rows can show the same identity their spawn did.
    pub(super) fn agent_index(&self, session_id: &str, entries: &[ChatEntry]) -> AgentIndex {
        let mut index = AgentIndex {
            by_task: Vec::new(),
            by_alias: Vec::new(),
        };
        // The link map knows every run this session launched, spawn card in
        // the loaded window or not: the alias resolves to the child session.
        for link in self
            .subagent_links
            .iter()
            .filter(|link| link.parent_session_id == session_id)
        {
            let Some(alias) = link.agent_id.clone().filter(|alias| !alias.is_empty()) else {
                continue;
            };
            if index.by_alias.iter().any(|(known, _)| *known == alias) {
                continue;
            }
            index.by_alias.push((
                alias.clone(),
                SubagentIdentity {
                    seed: link.subagent_session_id.clone(),
                    alias: Some(alias),
                    agent_type: link.agent_type.clone(),
                    model: None,
                    status: None,
                    running: self.live_sessions.contains(&link.subagent_session_id),
                    subagent_session: Some(link.subagent_session_id.clone()),
                },
            ));
        }
        for entry in entries {
            let ChatEntry::Tool(card) = entry else {
                continue;
            };
            if card.facts.kind != ToolKind::AgentSpawn {
                continue;
            }
            let Some(identity) = self.subagent_identity_for(card) else {
                continue;
            };
            let Some(agent) = card.facts.agents.first() else {
                continue;
            };
            if let Some(task_id) = agent.task_id.as_ref().filter(|id| !id.is_empty()) {
                index.by_task.push((task_id.clone(), identity.clone()));
            }
            if !agent.alias.is_empty() {
                index.by_alias.push((agent.alias.clone(), identity));
            }
        }
        index
    }

    pub(super) fn apply_subagent_links(&mut self) {
        // A restored transcript has no live link events: lineage entries carry
        // `parent_tool_call_id`, the same mapping and the only source of a
        // subagent's identity seed (its child session id). Seeding here (not
        // only on the lineage event) covers the ordering where a session opens
        // after its lineage snapshot already landed.
        self.seed_links_from_lineages();
        for chat in &mut self.chats {
            for link in self
                .subagent_links
                .iter()
                .filter(|link| link.parent_session_id == chat.session_id)
            {
                let tool_key = format!("tool-{}", link.tool_call_id);
                for entry in &mut chat.entries {
                    if let ChatEntry::Tool(card) = entry {
                        if card.key.as_ref() == tool_key {
                            card.subagent_session = Some(link.subagent_session_id.clone());
                        }
                    }
                }
            }
        }
    }
}

impl ShellWindow {
    /// Opens a hidden subagent session as the detail view (M20): the chat
    /// pane switches to the child transcript with a "back to parent" header
    /// entry point once the load settles.
    pub(super) fn open_subagent_run(
        &mut self,
        subagent_session_id: String,
        parent_session_id: String,
        cx: &mut Context<Self>,
    ) {
        // The link records the subagent type; the badge shows it when known.
        let agent_type = self
            .subagent_links
            .iter()
            .find(|link| link.subagent_session_id == subagent_session_id)
            .and_then(|link| link.agent_type.clone());
        self.pending_child = Some(PendingChild {
            child_session_id: subagent_session_id.clone(),
            context: ChildContext {
                kind: ChildKind::Subagent,
                parent_session_id,
                agent_type,
                btw_turn_id: None,
            },
        });
        if let Some(handle) = runtime::runtime_handle() {
            handle.open_session(subagent_session_id, self.event_tx.clone());
        }
        cx.notify();
    }

    /// Returns from a child view (subagent run / /btw) to its parent session.
    pub(super) fn return_from_child(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat() else {
            return;
        };
        let Some(child) = chat.child_context.as_ref() else {
            return;
        };
        let parent_session_id = child.parent_session_id.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.open_session(parent_session_id, self.event_tx.clone());
        }
        cx.notify();
    }
}

/// The settings button keeps a raised variant while its page is showing.
pub(super) fn active_variant_settings(tabs: &TabsState) -> ButtonVariant {
    if tabs.active_kind() == Some(&TabKind::Scene(SceneId::Settings)) {
        ButtonVariant::Secondary
    } else {
        ButtonVariant::Ghost
    }
}

pub(super) fn tool_status_label<'a>(status: ToolStatus, text: &'a ShellText) -> &'a str {
    match status {
        ToolStatus::Running => &text.tool_running,
        ToolStatus::WaitingConfirm => &text.tool_waiting,
        ToolStatus::Done => &text.tool_done,
        ToolStatus::Failed => &text.tool_failed,
        ToolStatus::Cancelled => &text.tool_cancelled,
    }
}

/// What a subagent-family row shows about one run: the identity seed its name
/// and avatar derive from (the child session id when known, else the agent
/// alias), plus the run's chips.
#[derive(Clone)]
pub(super) struct SubagentIdentity {
    pub(super) seed: String,
    pub(super) alias: Option<String>,
    pub(super) agent_type: Option<String>,
    pub(super) model: Option<String>,
    /// Launch-time status the runtime reported for the run (`started`,
    /// `failed`, …). Not a live signal on its own — see `running`.
    pub(super) status: Option<String>,
    /// The linked child session currently has a turn in flight, per the
    /// process-wide session lifecycle. This is what a status chip may claim:
    /// a historical transcript must not show a finished run as running.
    pub(super) running: bool,
    pub(super) subagent_session: Option<String>,
}

/// Spawn cards indexed for the rows that follow them: a wait row names
/// background task ids, a list row names aliases, and both resolve to the
/// identity their spawn already established.
pub(super) struct AgentIndex {
    pub(super) by_task: Vec<(String, SubagentIdentity)>,
    pub(super) by_alias: Vec<(String, SubagentIdentity)>,
}

impl AgentIndex {
    /// The chips one agent-family row shows. Wait rows resolve their task ids;
    /// list rows carry their own roster (the result named every agent), which
    /// links to a launch when one happened in this transcript.
    pub(super) fn roster_for(&self, card: &ToolCard) -> Vec<SubagentIdentity> {
        let mut roster: Vec<SubagentIdentity> = Vec::new();
        for task in &card.facts.tasks {
            if let Some((_, identity)) = self.by_task.iter().find(|(id, _)| id == task) {
                roster.push(identity.clone());
            }
        }
        for agent in &card.facts.agents {
            if agent.alias.is_empty()
                || roster
                    .iter()
                    .any(|entry| entry.alias.as_deref() == Some(agent.alias.as_str()))
            {
                continue;
            }
            if let Some((_, identity)) = self
                .by_alias
                .iter()
                .find(|(alias, _)| *alias == agent.alias)
            {
                // The launch's own card (when present) carries the model and
                // run status; the link-derived entry is the fallback.
                roster.push(identity.clone());
            } else {
                // A run this transcript never launched (or one reported only
                // by id): keep its alias and status, no artwork claim.
                roster.push(SubagentIdentity {
                    seed: agent.alias.clone(),
                    alias: Some(agent.alias.clone()),
                    agent_type: agent.agent_type.clone(),
                    model: agent.model.clone(),
                    status: agent.status.clone(),
                    running: false,
                    subagent_session: None,
                });
            }
        }
        roster
    }
}

/// The icon a tool family renders in front of its label.
pub(super) fn tool_kind_icon(kind: ToolKind) -> IconName {
    match kind {
        ToolKind::Command => IconName::SquareTerminal,
        ToolKind::Read => IconName::FileText,
        ToolKind::Search => IconName::Search,
        ToolKind::Web => IconName::Globe,
        ToolKind::Write => IconName::FilePlus,
        ToolKind::Edit => IconName::Pencil,
        ToolKind::ToolSpec => IconName::BookOpen,
        ToolKind::ToolCall => IconName::Wrench,
        ToolKind::AgentSpawn
        | ToolKind::AgentWait
        | ToolKind::AgentList
        | ToolKind::AgentSend
        | ToolKind::AgentDelete => IconName::Bot,
        ToolKind::Other => IconName::Wrench,
    }
}

/// The localized action label for a tool family (M24): what the call does,
/// before its payload. The web tool cards carry the same per-family wording.
pub(super) fn tool_action_label(kind: ToolKind, text: &ShellText) -> Option<&str> {
    let label = match kind {
        ToolKind::Command => &text.tool_action_command,
        ToolKind::Read => &text.tool_action_read,
        ToolKind::Search => &text.tool_action_search,
        ToolKind::Web => &text.tool_action_web,
        ToolKind::Write => &text.tool_action_write,
        ToolKind::Edit => &text.tool_action_edit,
        ToolKind::ToolSpec => &text.tool_action_tool_spec,
        ToolKind::ToolCall => &text.tool_action_tool_call,
        ToolKind::AgentSpawn => &text.tool_action_agent_spawn,
        ToolKind::AgentWait => &text.tool_action_agent_wait,
        ToolKind::AgentList => &text.tool_action_agent_list,
        ToolKind::AgentSend => &text.tool_action_agent_send,
        ToolKind::AgentDelete => &text.tool_action_agent_delete,
        // A tool without a family names itself; a label prefix would only
        // repeat the tool name.
        ToolKind::Other => return None,
    };
    Some(label)
}

/// One tool row: status glyph, family icon, action label, payload, chips. The
/// payload is the only text that reads like data (a command, a path), so it
/// stays on the foreground color while the label and chips stay muted.
pub(super) fn render_tool_row(
    card: &ToolCard,
    run: Option<&ChatForkContext>,
    fork: Option<&ChatForkContext>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let mut row = div().v_flex().gap_0p5().min_w_0().py_0p5();
    // Web `FlowChatToolCard.module.css`: a cancelled/rejected card dims
    // instead of changing its glyph.
    if card.status == ToolStatus::Cancelled {
        row = row.opacity(0.6);
    }
    let title = if card.facts.title.is_empty() {
        card.tool_name.clone()
    } else {
        card.facts.title.clone()
    };
    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(tool_leading_glyph(card, cx))
        .when_some(tool_action_label(card.facts.kind, text), |this, label| {
            this.child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_string()),
            )
        })
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(title.clone()),
        );
    // Content actions (web `AmbientToolCardHeader` `contentActions`, revealed
    // on hover there and always shown here): a file-family row opens its path
    // in the viewer, a command row copies the whole command.
    if let (Some(fork), false) = (fork, title.is_empty()) {
        if tool_row_opens_path(card) {
            let view = fork.view.clone();
            let path = title.clone();
            header = header.child(
                Button::new(SharedString::from(format!("{}-open", card.key)))
                    .debug_selector(|| "tool-row-open".to_string())
                    .icon(IconName::ArrowUpRight)
                    .xsmall()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(text.tool_open_details.clone())
                    .on_click(move |_, _, cx| {
                        let Some(view) = view.upgrade() else {
                            return;
                        };
                        let path = path.clone();
                        view.update(cx, |shell, cx| {
                            shell.open_transcript_path(path, None, cx);
                        });
                    }),
            );
        }
    }
    if let Some(copy) = &card.facts.copy {
        let copy = copy.clone();
        let copied = markdown::tool_row_copied(card.key.as_ref(), cx);
        let copy_key = card.key.clone();
        header = header.child(
            Button::new(SharedString::from(format!("{}-copy", card.key)))
                .debug_selector(|| "tool-row-copy".to_string())
                .icon(if copied {
                    IconName::Check
                } else {
                    IconName::Copy
                })
                .xsmall()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(if copied {
                    text.tool_copied.clone()
                } else {
                    text.tool_copy.clone()
                })
                .on_click(move |_, _, cx| {
                    markdown::copy_tool_row(&copy_key, &copy, cx);
                }),
        );
    }
    let mut row = row.child(header.children(tool_meta_chips(card, cx)));
    if let Some(error) = &card.facts.error {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .truncate()
                .child(error.clone()),
        );
    }
    // A linked hidden subagent session (M20): the card grows the entry point
    // that opens the child transcript as the detail view.
    if let (Some(subagent_session_id), Some(fork)) = (&card.subagent_session, run) {
        let child = subagent_session_id.clone();
        let parent = fork.session_id.clone();
        let view = fork.view.clone();
        let label = text.subagent_open.clone();
        row = row.child(
            div().flex().justify_end().child(
                Button::new(SharedString::from(format!("{}-view-run", card.key)))
                    .label(label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |shell, cx| {
                                shell.open_subagent_run(child.clone(), parent.clone(), cx);
                            });
                        }
                    }),
            ),
        );
    }
    row
}

/// The leading glyph of a tool row: the web ambient card swaps the family
/// icon for the status glyph while work is in flight and on failure (the
/// processing dots / xmark of `ToolCardStatusSlot`); a completed, waiting or
/// cancelled row keeps its family icon. Approvals tint that icon as the
/// card's confirmation colour does on the desktop.
pub(super) fn tool_leading_glyph(card: &ToolCard, cx: &App) -> gpui_kit::AnyElement {
    match card.status {
        ToolStatus::Running => gpui_kit::component::spinner::Spinner::new()
            .xsmall()
            .color(cx.theme().muted_foreground)
            .into_any_element(),
        ToolStatus::Failed => Icon::new(IconName::X)
            .with_size(px(14.))
            .text_color(cx.theme().danger)
            .into_any_element(),
        ToolStatus::WaitingConfirm => Icon::new(tool_kind_icon(card.facts.kind))
            .with_size(px(14.))
            .text_color(cx.theme().warning)
            .into_any_element(),
        ToolStatus::Done | ToolStatus::Cancelled => Icon::new(tool_kind_icon(card.facts.kind))
            .with_size(px(14.))
            .text_color(cx.theme().muted_foreground)
            .into_any_element(),
    }
}

/// Whether a tool row carries the web `arrow-up-right` open action: the
/// path-bearing families (Read/Write/Edit) open their file in the viewer,
/// like the desktop ReadFile/FileOperation cards.
pub(super) fn tool_row_opens_path(card: &ToolCard) -> bool {
    matches!(
        card.facts.kind,
        ToolKind::Read | ToolKind::Write | ToolKind::Edit
    )
}

/// The structured chips on a tool row: exit codes, line and match counts,
/// subagent tallies. Numbers are formatted per locale at render time.
pub(super) fn tool_meta_chips(card: &ToolCard, cx: &App) -> Vec<gpui_kit::AnyElement> {
    card.facts
        .meta
        .iter()
        .map(|meta| {
            let label = match meta {
                transcript::ToolMeta::Text(value) => value.clone(),
                transcript::ToolMeta::ExitCode(code) => i18n::tool_meta_exit_code(*code, cx),
                transcript::ToolMeta::Lines(count) => i18n::tool_meta_lines(*count, cx),
                transcript::ToolMeta::Replacements(count) => {
                    i18n::tool_meta_replacements(*count, cx)
                }
                transcript::ToolMeta::Matches(count) => i18n::tool_meta_matches(*count, cx),
                transcript::ToolMeta::Agents { total, running } => {
                    i18n::tool_meta_agents(*total, *running, cx)
                }
                transcript::ToolMeta::Failed(count) => i18n::tool_meta_failed(*count, cx),
            };
            let danger = matches!(meta, transcript::ToolMeta::Failed(_));
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(if danger {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                })
                .child(label)
                .into_any_element()
        })
        .collect()
}

/// A one-line chip naming one subagent run: avatar, localized name and — for
/// roster rows — its run status. The name and artwork come from the same
/// seeded pools the web UI uses, so a run reads identically on both surfaces.
pub(super) fn subagent_chip(
    identity: &SubagentIdentity,
    show_status: bool,
    cx: &App,
) -> gpui_kit::Div {
    let text = i18n::current(cx);
    let name = subagent_identity::display_name(&identity.seed, &text);
    let mut chip = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .flex_shrink_0()
        .child(
            Icon::default()
                .path(subagent_identity::avatar_path(&identity.seed))
                .with_size(px(18.))
                .flex_shrink_0(),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().foreground)
                .child(name),
        );
    if let Some(agent_type) = identity
        .agent_type
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        chip = chip.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(agent_type.clone()),
        );
    }
    if let Some(model) = identity.model.as_ref().filter(|value| !value.is_empty()) {
        chip = chip.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(model.clone()),
        );
    }
    if show_status {
        if let Some((status, color)) = agent_run_status(identity, &text, cx) {
            chip = chip.child(div().text_xs().text_color(color).child(status));
        }
    }
    chip
}

/// The status a subagent row may honestly claim. A live turn on the child
/// session is the only "running" evidence; a recorded terminal status
/// (`failed`, `cancelled`) stays readable forever.
pub(super) fn agent_run_status(
    identity: &SubagentIdentity,
    text: &ShellText,
    cx: &App,
) -> Option<(String, gpui_kit::Hsla)> {
    if identity.running {
        return Some((text.tool_running.clone(), cx.theme().success));
    }
    match identity.status.as_deref()? {
        "failed" | "error" => Some((text.tool_failed.clone(), cx.theme().danger)),
        "cancelled" | "canceled" | "stopped" => {
            Some((text.tool_cancelled.clone(), cx.theme().muted_foreground))
        }
        _ => None,
    }
}

/// The status text for an agent card: the run's own status when there is one,
/// else the tool lifecycle (a spawn still being approved, for instance).
pub(super) fn agent_card_status(
    card: &ToolCard,
    identity: &SubagentIdentity,
    text: &ShellText,
    cx: &App,
) -> Option<(String, gpui_kit::Hsla)> {
    if let Some(status) = agent_run_status(identity, text, cx) {
        return Some(status);
    }
    match card.status {
        ToolStatus::Done => None,
        ToolStatus::Failed => Some((text.tool_failed.clone(), cx.theme().danger)),
        ToolStatus::Cancelled => Some((text.tool_cancelled.clone(), cx.theme().danger)),
        ToolStatus::Running => Some((text.tool_running.clone(), cx.theme().warning)),
        ToolStatus::WaitingConfirm => Some((text.tool_waiting.clone(), cx.theme().warning)),
    }
}

/// One subagent run as a card (M24, web `AgentControlToolCard` shape): the
/// run's identity, its type and model, its status, the task hint, and the
/// "view run" entry point when the child transcript is reachable.
pub(super) fn render_agent_card(
    card: &ToolCard,
    identity: &SubagentIdentity,
    run: Option<&ChatForkContext>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    // Type and model read as chips beside the name; the prompt hint (the one
    // free-text meta a spawn carries) becomes the card's subtitle line.
    let mut chips: Vec<gpui_kit::AnyElement> = Vec::new();
    if let Some(agent_type) = identity
        .agent_type
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        chips.push(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(agent_type.clone())
                .into_any_element(),
        );
    }
    if let Some(model) = identity.model.as_ref().filter(|value| !value.is_empty()) {
        chips.push(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(model.clone())
                .into_any_element(),
        );
    }
    if let Some((status, color)) = agent_card_status(card, identity, text, cx) {
        chips.push(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(color)
                .child(status)
                .into_any_element(),
        );
    }
    let hint = card.facts.meta.iter().find_map(|meta| match meta {
        transcript::ToolMeta::Text(value) => Some(value.clone()),
        _ => None,
    });
    let mut row = div()
        .v_flex()
        .gap_1()
        .min_w_0()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .px_2()
        .py_1p5()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    Icon::default()
                        .path(subagent_identity::avatar_path(&identity.seed))
                        .with_size(px(24.))
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(subagent_identity::display_name(&identity.seed, text)),
                )
                .when_some(identity.alias.clone(), |this, alias| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(alias),
                    )
                })
                .children(chips),
        )
        .when_some(hint, |this, hint| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(hint),
            )
        });
    if let (Some(subagent_session_id), Some(fork)) = (&identity.subagent_session, run) {
        let child = subagent_session_id.clone();
        let parent = fork.session_id.clone();
        let view = fork.view.clone();
        let label = text.subagent_open.clone();
        row = row.child(
            div().flex().justify_end().child(
                Button::new(SharedString::from(format!("{}-view-run", card.key)))
                    .label(label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |shell, cx| {
                                shell.open_subagent_run(child.clone(), parent.clone(), cx);
                            });
                        }
                    }),
            ),
        );
    }
    row
}

/// One agent-family tool call as a transcript row: the action row plus the
/// runs it touched. `roster` resolves the aliases a wait/list call names.
pub(super) fn render_agent_row(
    card: &ToolCard,
    identity: Option<&SubagentIdentity>,
    roster: &[SubagentIdentity],
    run: Option<&ChatForkContext>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    // A spawn call IS its run: render the identity card.
    if card.facts.kind == ToolKind::AgentSpawn {
        if let Some(identity) = identity {
            return render_agent_card(card, identity, run, text, cx);
        }
    }
    let status_color = tool_status_color(card.status, cx);
    let mut row = div().v_flex().gap_1().min_w_0().py_0p5().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(
                Icon::new(tool_kind_icon(card.facts.kind))
                    .with_size(px(14.))
                    .text_color(cx.theme().muted_foreground),
            )
            .when_some(tool_action_label(card.facts.kind, text), |this, label| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.to_string()),
                )
            })
            .children(tool_meta_chips(card, cx))
            .when(card.status != ToolStatus::Done, |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(status_color)
                        .child(tool_status_label(card.status, text).to_string()),
                )
            }),
    );
    if !roster.is_empty() {
        row = row.child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_x_3()
                .gap_y_1()
                .children(
                    roster
                        .iter()
                        .map(|identity| subagent_chip(identity, true, cx).into_any_element()),
                ),
        );
    }
    if let Some(error) = &card.facts.error {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .truncate()
                .child(error.clone()),
        );
    }
    if let (Some(subagent_session_id), Some(fork)) = (&card.subagent_session, run) {
        let child = subagent_session_id.clone();
        let parent = fork.session_id.clone();
        let view = fork.view.clone();
        let label = text.subagent_open.clone();
        row = row.child(
            div().flex().justify_end().child(
                Button::new(SharedString::from(format!("{}-view-run", card.key)))
                    .label(label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |shell, cx| {
                                shell.open_subagent_run(child.clone(), parent.clone(), cx);
                            });
                        }
                    }),
            ),
        );
    }
    row
}

/// The transcript row for one tool call: family rows, agent cards, and the
/// failure/status affordances they carry.
/// Stable key for one work run: the head call's own key. Transcript reloads
/// re-index rows but keep tool ids, so the reader's disclosure survives.
pub(super) fn group_head_key(entries: &[ChatEntry], head: usize) -> u64 {
    match entries.get(head) {
        Some(ChatEntry::Tool(card)) => markdown_id_key(&card.key),
        _ => markdown_id_key(&format!("row-{head}")),
    }
}

pub(super) fn render_tool_card(
    card: &ToolCard,
    identity: Option<&SubagentIdentity>,
    roster: &[SubagentIdentity],
    // `run` is the fork context restricted to rows that expose a linked
    // subagent run; `fork` is the chat's own context, present whenever a
    // shell window renders the row — the content actions (open file) need it
    // even without a run link.
    run: Option<&ChatForkContext>,
    fork: Option<&ChatForkContext>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    match card.facts.kind {
        ToolKind::AgentSpawn
        | ToolKind::AgentWait
        | ToolKind::AgentList
        | ToolKind::AgentSend
        | ToolKind::AgentDelete => render_agent_row(card, identity, roster, run, text, cx),
        _ => render_tool_row(card, run, fork, text, cx),
    }
}

/// The collapsed work run's row (M24, web `workspaceGroup.summary`): one chip
/// naming how many read/search/shell steps the reader skipped, with the
/// category breakdown beside it. Clicking toggles the run open.
pub(super) fn render_group_chip(
    group: &transcript::ExploreGroup,
    expanded: bool,
    key: u64,
    fork: Option<&ChatForkContext>,
    cx: &App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let mut parts: Vec<String> = Vec::new();
    if group.stats.reads > 0 {
        parts.push(i18n::transcript_group_reads(group.stats.reads, cx));
    }
    if group.stats.searches > 0 {
        parts.push(i18n::transcript_group_searches(group.stats.searches, cx));
    }
    if group.stats.commands > 0 {
        parts.push(i18n::transcript_group_commands(group.stats.commands, cx));
    }
    let mut chips = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground);
    if !parts.is_empty() {
        chips = chips.child(div().child(parts.join(" · ")));
    }
    if group.stats.failed > 0 {
        chips = chips.child(
            div()
                .text_color(cx.theme().danger)
                .child(i18n::tool_meta_failed(group.stats.failed, cx)),
        );
    }
    let toggle = fork.cloned();
    let head = group.head;
    div()
        .debug_selector(|| "transcript-group".to_string())
        .id(ElementId::Name(format!("transcript-group-{key}").into()))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .min_w_0()
        .py_1()
        .cursor_pointer()
        .rounded(cx.theme().radius)
        .on_click(move |_, _, cx| {
            let Some(fork) = toggle.as_ref() else {
                return;
            };
            let Some(view) = fork.view.upgrade() else {
                return;
            };
            let session_id = fork.session_id.clone();
            view.update(cx, |shell, cx| {
                let span = shell.chat_mut_for(&session_id).and_then(|chat| {
                    let plan = transcript::TranscriptGroups::build(&chat.entries);
                    let group = plan
                        .group_of(head)
                        .filter(|group| group.head == head)
                        .cloned()?;
                    Some((chat.scroller.clone(), chat.toggle_group(&group)))
                });
                if let Some((scroller, (first, len))) = span {
                    // The run's rows change height together: remeasure the
                    // whole span, not just the chip.
                    scroller.update(cx, |scroller, cx| {
                        scroller.remeasure_items(first..first + len, cx);
                    });
                }
                cx.notify();
            });
        })
        .child(
            Icon::new(IconName::Route)
                .with_size(px(14.))
                .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(i18n::transcript_group_summary(group.len, cx)),
        )
        .child(chips)
        .child(
            Icon::new(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .with_size(px(14.))
            .text_color(cx.theme().muted_foreground),
        )
}

/// The status color ramp shared by tool rows, agent cards and group chips.
pub(super) fn tool_status_color(status: ToolStatus, cx: &App) -> gpui_kit::Hsla {
    match status {
        ToolStatus::Running | ToolStatus::WaitingConfirm => cx.theme().warning,
        ToolStatus::Done => cx.theme().success,
        ToolStatus::Failed | ToolStatus::Cancelled => cx.theme().danger,
    }
}
