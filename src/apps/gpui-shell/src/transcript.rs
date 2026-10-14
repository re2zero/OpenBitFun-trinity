//! Rich transcript reading aids: how one tool call reads as a row, and which
//! consecutive calls collapse into one "exploration and execution" summary.
//!
//! The web UI derives both from its tool cards
//! (`src/web-ui/src/flow_chat/tool-cards/**`) and its group policies
//! (`grouping/policies.ts`); the shell reads the same shapes out of the raw
//! tool input/result JSON once, at conversion time, so the render path only
//! picks an icon, a localized label and the strings collected here.
//!
//! Membership and wording are ported from the web policies on purpose: the
//! same session has to read the same way on both surfaces.

use serde_json::Value;

/// Tool family a row draws its icon and action label from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolKind {
    /// Shell launch (`ExecCommand`/`Bash`) and its process controls.
    Command,
    Read,
    Search,
    Web,
    Write,
    Edit,
    /// Read one tool's documentation (`GetToolSpec`).
    ToolSpec,
    /// Open a tool's parameter list through the deferred gateway.
    ToolCall,
    AgentSpawn,
    AgentWait,
    AgentList,
    AgentSend,
    AgentDelete,
    /// Anything without a family of its own; the row shows the tool name.
    Other,
}

/// One subagent an agent-family row refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentRef {
    /// The runtime alias (`agent_id`), the stable display identity.
    pub alias: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    /// Background task id (`bg_task_id`); pairs a wait row with its spawn.
    pub task_id: Option<String>,
    /// Raw run status the runtime reported (`running`/`failed`/`completed`…).
    pub status: Option<String>,
}

/// Secondary chip on a tool row. Counts and codes stay structured so the
/// renderer localizes them instead of the parser baking in English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolMeta {
    /// Raw value (model id, agent type, path, provider…).
    Text(String),
    ExitCode(i64),
    Lines(usize),
    Replacements(usize),
    Matches(usize),
    /// `total` subagents of which `running` are live.
    Agents {
        total: usize,
        running: usize,
    },
    Failed(usize),
}

/// Display facts for one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolFacts {
    pub kind: ToolKind,
    /// Primary payload: command, path, query, URL or agent alias.
    pub title: String,
    pub meta: Vec<ToolMeta>,
    /// Agent-family rows: the subagents the call touched.
    pub agents: Vec<AgentRef>,
    /// AgentWait rows: background task ids the call waited on, in input order.
    pub tasks: Vec<String>,
    /// Failure text (first line), shown under a failed row.
    pub error: Option<String>,
    /// Full text the row's copy action puts on the clipboard (the web
    /// ExecProcess card copies the whole command, not the truncated title).
    pub copy: Option<String>,
}

impl ToolFacts {
    fn new(kind: ToolKind) -> Self {
        Self {
            kind,
            title: String::new(),
            meta: Vec::new(),
            agents: Vec::new(),
            tasks: Vec::new(),
            error: None,
            copy: None,
        }
    }

    fn titled(kind: ToolKind, title: impl Into<String>) -> Self {
        let mut facts = Self::new(kind);
        facts.title = title.into();
        facts
    }

    /// Facts for an event that carried nothing (rejections, bare lifecycle
    /// pings): the row keeps whatever the earlier events already collected.
    pub(crate) fn empty() -> Self {
        Self::new(ToolKind::Other)
    }
}

/// Merges a lifecycle update into the stored facts. Input-side facts arrive
/// once (the `Started` params), result-side counts arrive later, and a later
/// event must never blank what the row already knows.
pub(crate) fn merge_facts(stored: &ToolFacts, update: &ToolFacts) -> ToolFacts {
    let mut merged = ToolFacts {
        // The side that carries a title owns the kind: an update with no
        // title is a lifecycle ping, not a new identity.
        kind: if update.title.is_empty() {
            stored.kind
        } else {
            update.kind
        },
        title: if update.title.is_empty() {
            stored.title.clone()
        } else {
            update.title.clone()
        },
        meta: stored.meta.clone(),
        agents: merge_agents(&stored.agents, &update.agents),
        tasks: if update.tasks.is_empty() {
            stored.tasks.clone()
        } else {
            update.tasks.clone()
        },
        error: update.error.clone().or_else(|| stored.error.clone()),
        copy: update.copy.clone().or_else(|| stored.copy.clone()),
    };
    for item in &update.meta {
        // Result-side counts repeat across retries; never double a chip.
        if !merged.meta.contains(item) {
            merged.meta.push(item.clone());
        }
    }
    merged
}

/// Agent lists combine when they describe the same calls (a spawn's input-side
/// alias against its result-side run state); otherwise the newer list wins.
fn merge_agents(stored: &[AgentRef], update: &[AgentRef]) -> Vec<AgentRef> {
    if update.is_empty() {
        return stored.to_vec();
    }
    if stored.len() != update.len() {
        return update.to_vec();
    }
    stored
        .iter()
        .zip(update)
        .map(|(stored, update)| AgentRef {
            alias: if update.alias.is_empty() {
                stored.alias.clone()
            } else {
                update.alias.clone()
            },
            agent_type: update
                .agent_type
                .clone()
                .or_else(|| stored.agent_type.clone()),
            model: update.model.clone().or_else(|| stored.model.clone()),
            task_id: update.task_id.clone().or_else(|| stored.task_id.clone()),
            status: update.status.clone().or_else(|| stored.status.clone()),
        })
        .collect()
}

/// Input key priority for the generic fallback title: the first present key
/// that carries an identifier the reader can act on.
const GENERIC_TITLE_KEYS: &[&str] = &[
    "file_path",
    "path",
    "cmd",
    "command",
    "query",
    "url",
    "pattern",
    "agent_id",
    "tool_name",
    "name",
    "id",
];

fn field<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn count_at(input: &Value, key: &str) -> Option<usize> {
    input
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

/// First non-empty line, trimmed: list rows never render a multi-line title.
pub(crate) fn first_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// Builds the input-side facts for one tool call. `tool_name` is the
/// effective name (the deferred gateway already projects to the inner tool).
pub(crate) fn facts_from_input(tool_name: &str, input: &Value) -> ToolFacts {
    match tool_name {
        "ExecCommand" | "Bash" => {
            let mut facts = ToolFacts::titled(ToolKind::Command, "");
            if let Some(cmd) = field(input, "cmd").or_else(|| field(input, "command")) {
                facts.title = first_line(cmd);
                // The row shows one line; the copy action gets the whole
                // command, like the desktop ExecProcess card's copy button.
                facts.copy = Some(cmd.to_string());
            }
            facts
        }
        "ExecControl" => {
            let mut facts = ToolFacts::new(ToolKind::Command);
            if let Some(action) = field(input, "action") {
                facts.meta.push(ToolMeta::Text(action.to_string()));
            }
            if let Some(stdin) = field(input, "chars") {
                facts.title = first_line(stdin);
            }
            facts
        }
        "WriteStdin" => {
            let mut facts = ToolFacts::new(ToolKind::Command);
            if let Some(chars) = field(input, "chars") {
                facts.title = first_line(chars);
            }
            facts
        }
        "Read" | "view_image" | "analyze_image" | "LS" => {
            let mut facts = ToolFacts::titled(ToolKind::Read, path_field(input));
            // `Read` windows a large file with offset/limit; the row states
            // which slice the model actually saw (web ReadToolCard's range).
            if let (Some(offset), Some(limit)) =
                (count_at(input, "offset"), count_at(input, "limit"))
            {
                if limit > 0 {
                    facts
                        .meta
                        .push(ToolMeta::Text(format!("{}-{}", offset + 1, offset + limit)));
                }
            }
            facts
        }
        "Grep" | "Glob" | "WebSearch" => {
            let query = field(input, "pattern")
                .or_else(|| field(input, "query"))
                .unwrap_or_default();
            ToolFacts::titled(ToolKind::Search, query)
        }
        "WebFetch" => ToolFacts::titled(ToolKind::Web, field(input, "url").unwrap_or_default()),
        "Write" => ToolFacts::titled(ToolKind::Write, path_field(input)),
        "Edit" | "MultiEdit" | "ApplyPatch" | "Delete" => {
            ToolFacts::titled(ToolKind::Edit, path_field(input))
        }
        "GetToolSpec" => ToolFacts::titled(
            ToolKind::ToolSpec,
            field(input, "tool_name").unwrap_or_default(),
        ),
        "CallDeferredTool" => ToolFacts::titled(
            ToolKind::ToolCall,
            field(input, "tool_name").unwrap_or_default(),
        ),
        "AgentSpawn" | "Task" => {
            let mut facts = ToolFacts::new(ToolKind::AgentSpawn);
            let alias = field(input, "agent_id").unwrap_or_default().to_string();
            facts.agents.push(AgentRef {
                alias: alias.clone(),
                agent_type: field(input, "agent_type")
                    .or_else(|| field(input, "subagent_type"))
                    .map(ToOwned::to_owned),
                model: field(input, "model_id").map(ToOwned::to_owned),
                task_id: None,
                status: None,
            });
            facts.title = alias;
            if let Some(prompt) = field(input, "prompt") {
                facts.meta.push(ToolMeta::Text(facts_line_hint(prompt)));
            }
            facts
        }
        "AgentWait" => {
            let mut facts = ToolFacts::new(ToolKind::AgentWait);
            facts.tasks = string_list(
                input
                    .get("bg_task_ids")
                    .or_else(|| input.get("background_task_ids")),
            );
            facts
        }
        "AgentList" => ToolFacts::new(ToolKind::AgentList),
        "AgentSendInput" => {
            let mut facts = ToolFacts::new(ToolKind::AgentSend);
            let alias = field(input, "agent_id").unwrap_or_default().to_string();
            facts.title = alias.clone();
            facts.agents.push(AgentRef {
                alias,
                agent_type: None,
                model: None,
                task_id: None,
                status: None,
            });
            facts
        }
        "AgentDelete" => {
            let mut facts = ToolFacts::new(ToolKind::AgentDelete);
            let mut deleted: Vec<String> = Vec::new();
            if let Some(alias) = field(input, "agent_id") {
                deleted.push(alias.to_string());
            }
            if let Some(Value::Array(items)) = input.get("agent_ids") {
                deleted.extend(
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned),
                );
            }
            facts.title = deleted.join(", ");
            facts
        }
        _ => {
            let title = GENERIC_TITLE_KEYS
                .iter()
                .find_map(|key| field(input, key))
                .map(first_line)
                .unwrap_or_default();
            ToolFacts::titled(ToolKind::Other, title)
        }
    }
}

/// Refines the facts with the call's result: counts, exit codes, run states.
pub(crate) fn apply_result(facts: &mut ToolFacts, tool_name: &str, result: &Value) {
    match tool_name {
        "ExecCommand" | "Bash" | "ExecControl" | "WriteStdin" => {
            if let Some(code) = result.get("exit_code").and_then(Value::as_i64) {
                facts.meta.push(ToolMeta::ExitCode(code));
            }
        }
        "Read" => {
            if let Some(lines) = count_at(result, "lines_read") {
                facts.meta.push(ToolMeta::Lines(lines));
            }
            if facts.title.is_empty() {
                if let Some(path) = field(result, "file_path") {
                    facts.title = path.to_string();
                }
            }
        }
        "LS" => {
            if let Some(total) = count_at(result, "total") {
                facts.meta.push(ToolMeta::Matches(total));
            }
        }
        "Edit" => {
            if let Some(matches) = count_at(result, "match_count") {
                facts.meta.push(ToolMeta::Replacements(matches));
            }
            if facts.title.is_empty() {
                if let Some(path) = field(result, "file_path") {
                    facts.title = path.to_string();
                }
            }
        }
        "Write" => {
            if let Some(lines) = count_at(result, "lines_written") {
                facts.meta.push(ToolMeta::Lines(lines));
            }
            if facts.title.is_empty() {
                if let Some(path) = field(result, "file_path") {
                    facts.title = path.to_string();
                }
            }
        }
        "Grep" | "Glob" => {
            if let Some(matches) =
                count_at(result, "total_matches").or_else(|| count_at(result, "match_count"))
            {
                facts.meta.push(ToolMeta::Matches(matches));
            }
        }
        "WebSearch" => {
            if let Some(matches) = count_at(result, "match_count") {
                facts.meta.push(ToolMeta::Matches(matches));
            }
        }
        "AgentSpawn" | "Task" => {
            if let Some(agent) = facts.agents.first_mut() {
                if let Some(task_id) = field(result, "bg_task_id") {
                    agent.task_id = Some(task_id.to_string());
                }
                agent.status = field(result, "status").map(ToOwned::to_owned);
                if agent.alias.is_empty() {
                    if let Some(alias) = field(result, "agent_id") {
                        agent.alias = alias.to_string();
                        facts.title = alias.to_string();
                    }
                }
            }
        }
        "AgentWait" => {
            let results = result.get("results").and_then(Value::as_array);
            let mut failed = 0usize;
            let mut agents: Vec<AgentRef> = Vec::new();
            for entry in results.into_iter().flatten() {
                let alias = field(entry, "agent_id").unwrap_or_default().to_string();
                let error = field(entry, "error").is_some();
                if error {
                    failed += 1;
                }
                agents.push(AgentRef {
                    alias,
                    agent_type: None,
                    model: None,
                    task_id: field(entry, "bg_task_id").map(ToOwned::to_owned),
                    status: Some(if error { "failed" } else { "completed" }.to_string()),
                });
            }
            if facts.tasks.is_empty() {
                facts.tasks = string_list(result.get("pending_bg_task_ids"));
            }
            facts.agents = agents;
            if failed > 0 {
                facts.meta.push(ToolMeta::Failed(failed));
            }
        }
        "AgentList" => {
            let agents = result.get("agents").and_then(Value::as_array);
            let mut total = 0usize;
            let mut running = 0usize;
            let mut collected: Vec<AgentRef> = Vec::new();
            for entry in agents.into_iter().flatten() {
                let status = field(entry, "status").unwrap_or_default().to_string();
                if status == "running" || status == "started" {
                    running += 1;
                }
                total += 1;
                collected.push(AgentRef {
                    alias: field(entry, "agent_id").unwrap_or_default().to_string(),
                    agent_type: None,
                    model: None,
                    task_id: None,
                    status: Some(status),
                });
            }
            if total > 0 {
                facts.meta.push(ToolMeta::Agents { total, running });
                facts.agents = collected;
            }
        }
        _ => {
            // A tool without a family of its own may still report a count
            // (the deferred gateway's lookups carry `match_count`).
            if let Some(matches) =
                count_at(result, "match_count").or_else(|| count_at(result, "total_matches"))
            {
                facts.meta.push(ToolMeta::Matches(matches));
            }
        }
    }
    if facts.title.is_empty() {
        if let Some(hint) = field(result, "path")
            .or_else(|| field(result, "file_path"))
            .or_else(|| field(result, "url"))
        {
            facts.title = hint.to_string();
        }
    }
}

/// Human hint for a spawn prompt: the first meaningful line, so a subagent
/// row can show what the run is for. Bounded — prompts are huge.
fn facts_line_hint(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default();
    crate::runtime::truncate_chars(line, 120)
}

/// The path-ish identifier of a file tool input.
fn path_field(input: &Value) -> String {
    field(input, "file_path")
        .or_else(|| field(input, "path"))
        .unwrap_or_default()
        .to_string()
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(Value::String(item)) if !item.trim().is_empty() => vec![item.trim().to_string()],
        _ => Vec::new(),
    }
}

/// Whether a result counts as a failure for grouping: the web explore policy
/// excludes calls whose result reported `success === false`.
pub(crate) fn result_flagged_failure(facts: &ToolFacts, result_success: Option<bool>) -> bool {
    result_success == Some(false) || facts.error.is_some()
}

/// One maximal run of routine exploration/execution calls (web
/// `flowGroupPolicies.explore`): reads, searches, and shell activity that sit
/// next to each other collapse into a single summary row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExploreGroup {
    /// Index of the run's first entry in the transcript.
    pub head: usize,
    pub len: usize,
    pub stats: ExploreStats,
}

/// Counts shown on the group row, mirroring the web summary parts
/// (`exploreRegion.readLabel` / `searchLabel`, `shellGroup.commands`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExploreStats {
    pub reads: usize,
    pub searches: usize,
    pub commands: usize,
    pub failed: usize,
    pub running: usize,
}

/// Read-only exploration tools that collapse when they succeeded (web
/// `COLLAPSIBLE_TOOL_NAMES`).
fn is_collapsible_read(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "Read" | "LS" | "Grep" | "Glob" | "WebSearch" | "WebFetch" | "view_image"
    )
}

/// Shell activity: always collapsible, failures included (web
/// `isShellActivity` / `shellTools`).
fn is_shell_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "ExecCommand" | "Bash" | "WriteStdin" | "ExecControl"
    )
}

fn is_search_tool(tool_name: &str) -> bool {
    matches!(tool_name, "Grep" | "Glob" | "WebSearch")
}

/// The explore grouping decision for one tool row, from the facts the
/// conversion already extracted. `None` = a visible row of its own.
fn group_member(card: &crate::runtime::ToolCard) -> Option<MemberKind> {
    use crate::runtime::ToolStatus;

    if card.status == ToolStatus::WaitingConfirm || card.status == ToolStatus::Cancelled {
        return None;
    }
    let name = card.tool_name.as_str();
    if is_shell_tool(name) {
        return Some(MemberKind::Command);
    }
    if card.status != ToolStatus::Done || result_flagged_failure(&card.facts, card.result_success) {
        return None;
    }
    if !is_collapsible_read(name) {
        return None;
    }
    if is_search_tool(name) {
        Some(MemberKind::Search)
    } else {
        Some(MemberKind::Read)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberKind {
    Read,
    Search,
    Command,
}

impl MemberKind {
    fn counted(self, stats: &mut ExploreStats) {
        match self {
            MemberKind::Read => stats.reads += 1,
            MemberKind::Search => stats.searches += 1,
            MemberKind::Command => stats.commands += 1,
        }
    }
}

/// Group membership for a whole transcript: `member_of[i] = Some(group)` when
/// entry `i` belongs to a collapsed run.
#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptGroups {
    groups: Vec<ExploreGroup>,
    member_of: Vec<Option<usize>>,
}

impl TranscriptGroups {
    pub(crate) fn build(entries: &[crate::runtime::ChatEntry]) -> Self {
        use crate::runtime::{ChatEntry, ToolStatus};

        let mut groups: Vec<ExploreGroup> = Vec::new();
        let mut member_of: Vec<Option<usize>> = vec![None; entries.len()];
        // Index of the run still accepting members; `None` closes it. Prose,
        // failures and any non-tool entry are hard boundaries — the web reader
        // never merges across them.
        let mut open: Option<usize> = None;
        for (index, entry) in entries.iter().enumerate() {
            let ChatEntry::Tool(card) = entry else {
                open = None;
                continue;
            };
            let Some(kind) = group_member(card) else {
                open = None;
                continue;
            };
            let group_index = *open.get_or_insert_with(|| {
                groups.push(ExploreGroup {
                    head: index,
                    len: 0,
                    stats: ExploreStats::default(),
                });
                groups.len() - 1
            });
            let group = &mut groups[group_index];
            group.len += 1;
            kind.counted(&mut group.stats);
            match card.status {
                ToolStatus::Failed => group.stats.failed += 1,
                ToolStatus::Running => group.stats.running += 1,
                _ => {}
            }
            member_of[index] = Some(group_index);
        }
        Self { groups, member_of }
    }

    /// The group entry `index` belongs to, when it is a collapsed-run member.
    pub(crate) fn group_of(&self, index: usize) -> Option<&ExploreGroup> {
        self.member_of
            .get(index)
            .copied()
            .flatten()
            .and_then(|group| self.groups.get(group))
    }

    /// Whether `index` opens its group (the row hosts the summary chip).
    pub(crate) fn is_head(&self, index: usize) -> bool {
        self.group_of(index)
            .is_some_and(|group| group.head == index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ChatEntry, ToolCard, ToolStatus};

    fn card(tool_name: &str, status: ToolStatus, facts: ToolFacts) -> ToolCard {
        ToolCard {
            key: format!("tool-{tool_name}").into(),
            call_id: tool_name.to_string(),
            tool_name: tool_name.to_string(),
            status,
            facts,
            result_success: None,
            subagent_session: None,
            background: false,
        }
    }

    fn read_card(path: &str) -> ChatEntry {
        ChatEntry::Tool(card(
            "Read",
            ToolStatus::Done,
            facts_from_input("Read", &serde_json::json!({ "file_path": path })),
        ))
    }

    fn command_card(cmd: &str) -> ChatEntry {
        ChatEntry::Tool(card(
            "ExecCommand",
            ToolStatus::Done,
            facts_from_input("ExecCommand", &serde_json::json!({ "cmd": cmd })),
        ))
    }

    fn prose(text: &str) -> ChatEntry {
        ChatEntry::Assistant {
            round_id: "r".into(),
            text: text.into(),
        }
    }

    #[test]
    fn command_facts_carry_the_first_line_and_exit_code() {
        let mut facts = facts_from_input(
            "ExecCommand",
            &serde_json::json!({ "cmd": "ls .relay/\necho done" }),
        );
        assert_eq!(facts.kind, ToolKind::Command);
        assert_eq!(facts.title, "ls .relay/");
        assert_eq!(facts.copy.as_deref(), Some("ls .relay/\necho done"));
        apply_result(
            &mut facts,
            "ExecCommand",
            &serde_json::json!({ "exit_code": 2, "output": "boom" }),
        );
        assert_eq!(facts.meta, vec![ToolMeta::ExitCode(2)]);
    }

    #[test]
    fn read_facts_window_the_offset_and_count_lines() {
        let mut facts = facts_from_input(
            "Read",
            &serde_json::json!({ "file_path": "src/main.rs", "offset": 100, "limit": 50 }),
        );
        assert_eq!(facts.title, "src/main.rs");
        // 1-based, inclusive window end, like the web Read card's range line.
        assert_eq!(facts.meta, vec![ToolMeta::Text("101-150".into())]);
        apply_result(&mut facts, "Read", &serde_json::json!({ "lines_read": 50 }));
        assert_eq!(facts.meta.len(), 2);
        assert_eq!(facts.meta[1], ToolMeta::Lines(50));
    }

    #[test]
    fn edit_and_search_facts_report_their_counts() {
        let mut edit = facts_from_input(
            "Edit",
            &serde_json::json!({ "file_path": "a.rs", "old_string": "x", "new_string": "y" }),
        );
        apply_result(&mut edit, "Edit", &serde_json::json!({ "match_count": 3 }));
        assert_eq!(edit.kind, ToolKind::Edit);
        assert_eq!(edit.title, "a.rs");
        assert_eq!(edit.meta, vec![ToolMeta::Replacements(3)]);

        let mut grep = facts_from_input(
            "Grep",
            &serde_json::json!({ "pattern": "fn render", "path": "src" }),
        );
        apply_result(
            &mut grep,
            "Grep",
            &serde_json::json!({ "total_matches": 12 }),
        );
        assert_eq!(grep.kind, ToolKind::Search);
        assert_eq!(grep.title, "fn render");
        assert_eq!(grep.meta, vec![ToolMeta::Matches(12)]);
    }

    #[test]
    fn agent_spawn_facts_name_the_run_and_its_model() {
        let facts = facts_from_input(
            "AgentSpawn",
            &serde_json::json!({
                "agent_id": "parser-review",
                "agent_type": "SwarmWorker",
                "model_id": "glm-5.3",
                "prompt": "# Task\n\nPort the parser slice",
            }),
        );
        assert_eq!(facts.kind, ToolKind::AgentSpawn);
        assert_eq!(facts.title, "parser-review");
        assert_eq!(facts.agents.len(), 1);
        assert_eq!(facts.agents[0].agent_type.as_deref(), Some("SwarmWorker"));
        assert_eq!(facts.agents[0].model.as_deref(), Some("glm-5.3"));
        // The prompt hint skips markdown headings and stays short.
        assert_eq!(
            facts.meta,
            vec![ToolMeta::Text("Port the parser slice".into())]
        );
    }

    #[test]
    fn agent_wait_facts_report_targets_and_failures() {
        let mut facts = facts_from_input(
            "AgentWait",
            &serde_json::json!({ "bg_task_ids": ["a_bg1", "b_bg1"], "timeout_seconds": 60 }),
        );
        assert_eq!(facts.kind, ToolKind::AgentWait);
        assert_eq!(facts.tasks, vec!["a_bg1", "b_bg1"]);
        apply_result(
            &mut facts,
            "AgentWait",
            &serde_json::json!({
                "pending_bg_task_ids": [],
                "results": [
                    { "agent_id": "a", "bg_task_id": "a_bg1", "error": "provider unreachable" },
                    { "agent_id": "b", "bg_task_id": "b_bg1", "content": "done" },
                ],
            }),
        );
        assert_eq!(facts.meta, vec![ToolMeta::Failed(1)]);
        assert_eq!(facts.agents.len(), 2);
        assert_eq!(facts.agents[0].status.as_deref(), Some("failed"));
        assert_eq!(facts.agents[1].status.as_deref(), Some("completed"));
    }

    #[test]
    fn agent_list_facts_count_the_roster() {
        let mut facts = facts_from_input("AgentList", &serde_json::Value::Null);
        apply_result(
            &mut facts,
            "AgentList",
            &serde_json::json!({
                "agents": [
                    { "agent_id": "a", "status": "failed" },
                    { "agent_id": "b", "status": "running" },
                    { "agent_id": "c", "status": "running" },
                ],
            }),
        );
        assert_eq!(facts.kind, ToolKind::AgentList);
        assert_eq!(
            facts.meta,
            vec![ToolMeta::Agents {
                total: 3,
                running: 2
            }]
        );
    }

    #[test]
    fn deferred_and_spec_tools_keep_their_own_labels() {
        let spec = facts_from_input(
            "GetToolSpec",
            &serde_json::json!({ "tool_name": "ListModels" }),
        );
        assert_eq!(spec.kind, ToolKind::ToolSpec);
        assert_eq!(spec.title, "ListModels");

        let call = facts_from_input(
            "CallDeferredTool",
            &serde_json::json!({ "tool_name": "ListModels", "args": {} }),
        );
        assert_eq!(call.kind, ToolKind::ToolCall);
        assert_eq!(call.title, "ListModels");
    }

    #[test]
    fn unknown_tools_fall_back_to_an_identifier() {
        let mut facts =
            facts_from_input("trinity_memorize", &serde_json::json!({ "content": "…" }));
        assert_eq!(facts.kind, ToolKind::Other);
        assert!(facts.title.is_empty());
        // Result-side facts backfill the row when the input had no id.
        apply_result(
            &mut facts,
            "trinity_memorize",
            &serde_json::json!({ "status": "ok", "path": "/tmp/memory.json" }),
        );
        assert_eq!(facts.title, "/tmp/memory.json");
    }

    #[test]
    fn merge_never_blanks_what_the_row_already_knows() {
        let stored = facts_from_input(
            "AgentSpawn",
            &serde_json::json!({ "agent_id": "run-a", "agent_type": "SwarmWorker" }),
        );
        let update = ToolFacts::empty();
        let merged = merge_facts(&stored, &update);
        assert_eq!(merged.kind, ToolKind::AgentSpawn);
        assert_eq!(merged.title, "run-a");
        assert_eq!(merged.agents.len(), 1);
    }

    #[test]
    fn groups_span_routine_calls_and_break_on_prose() {
        let entries = vec![
            read_card("a.rs"),
            command_card("cargo fmt"),
            prose("I formatted the tree."),
            read_card("b.rs"),
        ];
        let groups = TranscriptGroups::build(&entries);
        let first = groups.group_of(0).expect("read joins a run");
        assert_eq!(first.head, 0);
        assert_eq!(first.len, 2);
        assert_eq!(first.stats.reads, 1);
        assert_eq!(first.stats.commands, 1);
        assert!(groups.is_head(0));
        assert!(!groups.is_head(1));
        // Prose closes the run; the trailing read opens its own.
        assert!(groups.group_of(2).is_none());
        assert_eq!(groups.group_of(3).expect("own run").len, 1);
    }

    #[test]
    fn group_membership_matches_the_web_policies() {
        // A failed read breaks out of the run (web explore policy excludes
        // unsuccessful collapsible tools); a failed shell command joins it.
        let failed_read = ChatEntry::Tool(ToolCard {
            result_success: Some(false),
            ..card(
                "Read",
                ToolStatus::Done,
                facts_from_input("Read", &serde_json::json!({ "file_path": "x" })),
            )
        });
        let failed_command = ChatEntry::Tool(card(
            "ExecCommand",
            ToolStatus::Failed,
            facts_from_input("ExecCommand", &serde_json::json!({ "cmd": "false" })),
        ));
        let entries = vec![
            read_card("a.rs"),
            failed_read,
            failed_command,
            read_card("b.rs"),
        ];
        let groups = TranscriptGroups::build(&entries);
        assert_eq!(groups.group_of(0).expect("head run").len, 1);
        assert!(groups.group_of(1).is_none(), "failed read stays visible");
        // An ineligible row also splits the run, so the shell failure and the
        // trailing read form the next one — same boundaries the web reader
        // applies (`joinedFlowGroupCategory` ends a run at any other item).
        let command_run = groups.group_of(2).expect("shell failure joins");
        assert_eq!(command_run.stats.failed, 1);
        assert_eq!(command_run.stats.commands, 1);
        assert_eq!(command_run.len, 2);
        assert_eq!(groups.group_of(3).map(|group| group.head), Some(2));
    }

    #[test]
    fn pending_and_cancelled_calls_never_join_a_run() {
        let waiting = ChatEntry::Tool(card(
            "ExecCommand",
            ToolStatus::WaitingConfirm,
            facts_from_input("ExecCommand", &serde_json::json!({ "cmd": "rm -rf" })),
        ));
        let cancelled = ChatEntry::Tool(card(
            "Read",
            ToolStatus::Cancelled,
            facts_from_input("Read", &serde_json::json!({ "file_path": "x" })),
        ));
        let entries = vec![waiting, cancelled];
        let groups = TranscriptGroups::build(&entries);
        assert!(groups.group_of(0).is_none());
        assert!(groups.group_of(1).is_none());
    }
}
