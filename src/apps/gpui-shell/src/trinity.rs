//! Shell Trinity console: a thin client over the `trinityd` daemon RPC —
//! the same frame protocol the desktop host speaks (4-byte big-endian length
//! prefix + UTF-8 JSON), pointed at the same `~/.config/trinity/trinity.toml`
//! the daemon resolves itself.
//!
//! Connect-only by design: the shell never spawns, bundles, or shuts the
//! daemon down. A daemon started by the desktop host or by the user is an
//! independent service; when none is listening the console shows an explicit
//! offline state and every call degrades to an error string.
//!
//! The console surface mirrors the web scene's cognitive-state + memory face:
//! PSI state (emotion / focus / confidence / needs / memory counters), the
//! cognition-history trend, and the memory timeline (browse / search / add /
//! reinforce / forget). Cloud sync, awakening, and engine LLM config stay
//! web-scene features.

use std::sync::OnceLock;
use std::time::Duration;

use chrono::{Local, TimeZone};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::{div, prelude::*, px, App, SharedString};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};

const FRAME_HEADER_LEN: usize = 4;
const MAX_FRAME_LEN: usize = 10_000_000;
/// Upper bound for one exchange; a stalled daemon must not hang a fetch.
const RPC_TIMEOUT: Duration = Duration::from_secs(15);

/// Timeline page size (web `TIMELINE_LIMIT`).
pub(crate) const TIMELINE_LIMIT: i64 = 50;
/// History points pulled for the trend (web `HISTORY_LIMIT`).
const HISTORY_LIMIT: i64 = 120;

/// Port assumed when neither the override nor the config supplies one.
/// Matches the desktop host and the factory template.
pub(crate) const DEFAULT_DAEMON_PORT: u16 = 11656;
/// Key assumed when the config supplies none (a config without `[daemon]
/// keys` runs trinityd without authentication, so any value is accepted).
const DEFAULT_DAEMON_KEY: &str = "trinity-local-dev-key";

// ── Daemon endpoint resolution ──────────────────────────────────────

/// `~/.config/trinity/trinity.toml` — the file trinityd reads on its own.
fn user_config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("trinity").join("trinity.toml"))
}

fn read_daemon_table(path: &std::path::Path) -> Option<toml::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table.get("daemon")?.as_table().cloned()
}

fn read_daemon_port(path: &std::path::Path) -> Option<u16> {
    let port = read_daemon_table(path)?.get("port")?.as_integer()?;
    u16::try_from(port).ok()
}

fn read_daemon_key(path: &std::path::Path) -> Option<String> {
    read_daemon_table(path)?
        .get("keys")?
        .as_array()?
        .iter()
        .find_map(|key| key.as_str().map(str::to_owned))
}

/// Daemon listen port: `TRINITY_DAEMON_PORT`, then the user config, then the
/// factory default — the same resolution order as the desktop host.
pub(crate) fn daemon_port() -> u16 {
    std::env::var("TRINITY_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .or_else(|| user_config_path().and_then(|p| read_daemon_port(&p)))
        .unwrap_or(DEFAULT_DAEMON_PORT)
}

/// Daemon API key, resolved in the same order as [`daemon_port`].
pub(crate) fn daemon_key() -> String {
    std::env::var("TRINITY_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| user_config_path().and_then(|p| read_daemon_key(&p)))
        .unwrap_or_else(|| DEFAULT_DAEMON_KEY.to_string())
}

// ── Frame-protocol client ───────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct DaemonRequest {
    id: u64,
    method: String,
    #[serde(alias = "api_key")]
    key: Option<String>,
    #[serde(default)]
    params: Value,
}

fn encode_frame(value: &Value) -> Vec<u8> {
    let json = serde_json::to_vec(value).expect("serialize frame payload");
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + json.len());
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(&json);
    frame
}

async fn write_request_read_response(
    stream: &mut tokio::net::TcpStream,
    frame: &[u8],
) -> Result<Result<Value, String>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(frame)
        .await
        .map_err(|e| format!("write: {e}"))?;
    let mut header = [0u8; FRAME_HEADER_LEN];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|e| format!("read header: {e}"))?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        return Ok(Err(format!("invalid frame length: {len}")));
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("read body: {e}"))?;
    let v: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;

    if let Some(result) = v.get("result") {
        Ok(Ok(result.clone()))
    } else if let Some(err) = v.get("error") {
        Ok(Err(err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string()))
    } else {
        Ok(Err(format!("unexpected daemon response: {v}")))
    }
}

/// Reusable client: the first call connects, later calls reuse the stream.
/// A stream is reconnected only while a new one is being obtained; once a
/// request may have been written, a failure ends the call (a non-idempotent
/// method must never be silently re-executed).
#[derive(Debug)]
pub(crate) struct TrinityClient {
    port: u16,
    key: String,
    conn: tokio::sync::Mutex<Option<tokio::net::TcpStream>>,
}

impl TrinityClient {
    fn from_env() -> Self {
        Self {
            port: daemon_port(),
            key: daemon_key(),
            conn: tokio::sync::Mutex::new(None),
        }
    }

    /// Test constructor targeting an explicit port.
    #[cfg(test)]
    fn for_port(port: u16) -> Self {
        Self {
            port,
            key: DEFAULT_DAEMON_KEY.to_string(),
            conn: tokio::sync::Mutex::new(None),
        }
    }

    pub(crate) async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut guard = self.conn.lock().await;

        let req = DaemonRequest {
            id: 1,
            method: method.to_string(),
            key: Some(self.key.clone()),
            params,
        };
        let frame = encode_frame(&serde_json::to_value(&req).unwrap());

        let mut connect_attempt = 0u8;
        loop {
            let mut stream = match guard.take() {
                Some(s) => s,
                None => match tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await {
                    Ok(s) => s,
                    Err(_) if connect_attempt == 0 => {
                        connect_attempt = 1;
                        continue;
                    }
                    Err(e) => return Err(format!("connect trinityd :{}: {e}", self.port)),
                },
            };

            let result = match tokio::time::timeout(
                RPC_TIMEOUT,
                write_request_read_response(&mut stream, &frame),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(format!(
                    "trinityd '{method}' timed out after {RPC_TIMEOUT:?}"
                )),
            };

            match result {
                Ok(v) => {
                    *guard = Some(stream);
                    return v;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Process-global shared client (one pooled connection for all fetches).
pub(crate) fn client() -> &'static TrinityClient {
    static CLIENT: OnceLock<TrinityClient> = OnceLock::new();
    CLIENT.get_or_init(TrinityClient::from_env)
}

// ── Snapshot types + parsers ────────────────────────────────────────

/// Identity + awakening profile from the daemon `get_status` payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StatusInfo {
    pub awakened: bool,
    pub name: Option<String>,
    pub persona: Option<String>,
    pub user_name: Option<String>,
    pub birthday: Option<String>,
}

/// Live cognitive state from `get_cognitive_state`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PsiState {
    pub valence: Option<String>,
    pub focus: Option<String>,
    pub confidence: Option<f64>,
    /// The five drives in canonical display order, present ones first.
    pub needs: Vec<(String, f64)>,
    pub memory_nodes: Option<i64>,
    pub memory_triples: Option<i64>,
    /// G-Y6: the MindGraph vocabulary size the daemon reports alongside the
    /// node/triple counters (`memory.vocabulary_size`).
    pub memory_vocabulary: Option<i64>,
    pub cycle_count: Option<i64>,
}

/// One memory-timeline entry (daemon MindGraph convention: seconds).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MemoryEntry {
    pub id: Option<String>,
    pub content: Option<String>,
    pub kind: Option<String>,
    pub layer: Option<String>,
    pub strength: Option<f64>,
    pub timestamp: Option<i64>,
}

/// The five drives in the web scene's display order.
pub(crate) const NEED_KEYS: [&str; 5] = [
    "competence",
    "autonomy",
    "relatedness",
    "certainty",
    "growth",
];

pub(crate) fn parse_status(value: &Value) -> StatusInfo {
    let string_field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    StatusInfo {
        awakened: value
            .get("awakened")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        name: string_field("name"),
        persona: string_field("persona"),
        user_name: string_field("user_name"),
        birthday: string_field("birthday"),
    }
}

pub(crate) fn parse_psi(value: &Value) -> PsiState {
    let num = |key: &str| value.get(key).and_then(Value::as_f64);
    let int = |key: &str| value.get(key).and_then(Value::as_i64);
    let needs = value
        .get("needs")
        .and_then(Value::as_object)
        .map(|needs| {
            NEED_KEYS
                .iter()
                .filter_map(|key| {
                    needs
                        .get(*key)
                        .and_then(Value::as_f64)
                        .map(|v| (key.to_string(), v))
                })
                .collect()
        })
        .unwrap_or_default();
    PsiState {
        valence: value
            .get("emotion")
            .and_then(|e| e.get("valence"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        focus: value
            .get("focus")
            .and_then(Value::as_str)
            .map(str::to_owned),
        confidence: num("confidence"),
        needs,
        memory_nodes: value
            .get("memory")
            .and_then(|m| m.get("total_nodes"))
            .and_then(Value::as_i64),
        memory_triples: value
            .get("memory")
            .and_then(|m| m.get("total_triples"))
            .and_then(Value::as_i64),
        memory_vocabulary: value
            .get("memory")
            .and_then(|m| m.get("vocabulary_size"))
            .and_then(Value::as_i64),
        cycle_count: int("cycle_count"),
    }
}

/// The status-bar lamp phase (R1d, the web footer lamp shape): derived from
/// the daemon snapshot, never stored — an absent snapshot is offline, a
/// present one answers awake or dormant by its `awakened` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipPhase {
    Offline,
    Dormant,
    Awake,
}

pub(crate) fn chip_phase(awakened: Option<bool>) -> ChipPhase {
    match awakened {
        None => ChipPhase::Offline,
        Some(true) => ChipPhase::Awake,
        Some(false) => ChipPhase::Dormant,
    }
}

/// The awake lamp dot color family for an emotion valence (R1d): positive
/// feelings read green, negative ones red, everything else stays neutral.
pub(crate) fn emotion_dot_positive(valence: &Option<String>) -> bool {
    valence
        .as_deref()
        .is_some_and(|v| v.starts_with("positive"))
}

fn parse_memory_entry(value: &Value) -> MemoryEntry {
    MemoryEntry {
        id: value.get("id").and_then(Value::as_str).map(str::to_owned),
        content: value
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned),
        kind: value.get("kind").and_then(Value::as_str).map(str::to_owned),
        layer: value
            .get("layer")
            .and_then(Value::as_str)
            .map(str::to_owned),
        strength: value.get("strength").and_then(Value::as_f64),
        timestamp: value.get("timestamp").and_then(Value::as_i64),
    }
}

/// Normalize a memory-timeline payload into a flat entry list.
pub(crate) fn parse_memory_items(value: &Value) -> Vec<MemoryEntry> {
    value
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_memory_entry).collect())
        .unwrap_or_default()
}

/// Normalize a recall payload into a flat entry list.
pub(crate) fn parse_recall_items(value: &Value) -> Vec<MemoryEntry> {
    value
        .get("results")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_memory_entry).collect())
        .unwrap_or_default()
}

/// Numeric position of a valence on the engine's positive→negative axis;
/// unknown labels are dropped from the trend series.
pub(crate) fn valence_score(valence: &Option<String>) -> Option<f64> {
    match valence.as_deref() {
        Some("positive_high") => Some(1.0),
        Some("positive_mild") => Some(0.5),
        Some("neutral") => Some(0.0),
        Some("curious") => Some(0.3),
        Some("confused") => Some(-0.2),
        Some("negative_mild") => Some(-0.5),
        Some("negative_high") => Some(-1.0),
        _ => None,
    }
}

/// Trend series from a `cognition_history` payload.
pub(crate) fn parse_history(value: &Value) -> Vec<f64> {
    value
        .get("points")
        .and_then(Value::as_array)
        .map(|points| {
            points
                .iter()
                .filter_map(|point| {
                    let valence = point
                        .get("emotion")
                        .and_then(|e| e.get("valence"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    valence_score(&valence)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Needs at or below this level ask for attention.
pub(crate) const NEED_ATTENTION_THRESHOLD: f64 = 0.4;
/// Needs at or above this level read as satisfied.
pub(crate) const NEED_SATISFIED_THRESHOLD: f64 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NeedState {
    Low,
    Normal,
    High,
}

pub(crate) fn need_state(value: f64) -> NeedState {
    if value <= NEED_ATTENTION_THRESHOLD {
        NeedState::Low
    } else if value >= NEED_SATISFIED_THRESHOLD {
        NeedState::High
    } else {
        NeedState::Normal
    }
}

/// Compact `MM-DD HH:mm` rendering for a seconds timestamp.
pub(crate) fn format_memory_time(timestamp: i64) -> String {
    // Daemon sends seconds; guard against ms-level values anyway.
    let ms = if timestamp > 1_000_000_000_000 {
        timestamp
    } else {
        timestamp * 1000
    };
    let date = Local.timestamp_opt(
        ms.div_euclid(1000),
        (ms.rem_euclid(1000) * 1_000_000) as u32,
    );
    match date.single() {
        Some(date) => date.format("%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

// ── Page state ──────────────────────────────────────────────────────

/// Console state; lives on the shell window and is updated only through
/// [`RuntimeEvent`] projections (plus the small view-local toggles).
#[derive(Debug, Default)]
pub(crate) struct TrinityPageState {
    pub status: Option<(Box<PsiState>, Box<StatusInfo>)>,
    pub history: Vec<f64>,
    pub status_error: Option<String>,
    /// Status was fetched at least once (drives reload-on-open).
    pub loaded: bool,
    pub memories: Vec<MemoryEntry>,
    pub memories_exhausted: bool,
    pub memories_loading: bool,
    pub memories_loaded: bool,
    /// `Some` while a recall result set is shown, replacing the timeline.
    pub recall: Option<Vec<MemoryEntry>>,
    pub recall_busy: bool,
    /// Last failed action / load message, shown inline above the list.
    pub action_error: Option<String>,
    /// Add form open (view-local toggle).
    pub add_open: bool,
    /// Two-step forget: the armed entry id awaiting confirmation.
    pub forget_armed: Option<String>,
    /// G-Y5: the entry whose content is being corrected inline (web
    /// `editingId`); the shared edit textarea holds the draft.
    pub editing: Option<String>,
    /// G-Y5: the last correction verdict — `Some(true)` when the daemon
    /// applied it, `Some(false)` when the original memory was gone.
    pub correction: Option<bool>,
}

// ── Rendering ───────────────────────────────────────────────────────

/// The full console: status header, PSI card with trend, memory face.
pub(crate) fn render_page(
    state: &TrinityPageState,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    recall_input: &gpui_kit::Entity<InputState>,
    add_content: &gpui_kit::Entity<TextareaState>,
    add_project: &gpui_kit::Entity<InputState>,
    edit_content: &gpui_kit::Entity<TextareaState>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut column = div().w_full().h_full().flex().flex_col().gap_3().p_4();

    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_lg()
                .text_color(cx.theme().foreground)
                .child(text.trinity_title.clone()),
        );
    let mut actions = div().flex().items_center().gap_2();
    let refresh_tx = tx.clone();
    actions = actions.child(
        Button::new("trinity-refresh")
            .label(text.trinity_action_refresh.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click(move |_, _window, _cx| {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.trinity_refresh(refresh_tx.clone());
                    handle.trinity_load_memories(refresh_tx.clone(), None);
                }
            }),
    );
    header = header.child(actions);
    column = column.child(header);

    match &state.status {
        Some((psi, status)) => {
            column = column.child(status_card(status, text, cx));
            column = column.child(psi_card(state, psi, text, cx));
        }
        None => {
            let message = state.status_error.clone().unwrap_or_default();
            column = column.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().danger)
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(text.trinity_status_unavailable.clone())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(message),
                    ),
            );
        }
    }
    column = column.child(memory_section(
        state,
        view,
        recall_input,
        add_content,
        add_project,
        edit_content,
        tx,
        text,
        cx,
    ));
    column.into_any_element()
}

fn status_card(status: &StatusInfo, text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let phase_label = if status.awakened {
        text.trinity_phase_awake.clone()
    } else {
        text.trinity_phase_dormant.clone()
    };
    let identity = [
        (text.trinity_label_name.clone(), status.name.clone()),
        (text.trinity_label_persona.clone(), status.persona.clone()),
        (text.trinity_label_user.clone(), status.user_name.clone()),
        (text.trinity_label_birthday.clone(), status.birthday.clone()),
    ]
    .into_iter()
    .filter_map(|(label, value)| value.map(|value| format!("{label}: {value}")))
    .collect::<Vec<_>>()
    .join(" · ");

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(text.trinity_status_title.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .child(phase_label),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(identity),
        )
        .into_any_element()
}

fn psi_card(
    state: &TrinityPageState,
    psi: &PsiState,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut card = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border);

    let mut rows = div().flex().flex_row().flex_wrap().gap_4();
    rows = rows.child(fact_row(
        text.trinity_label_emotion.clone(),
        emotion_label(&psi.valence, text),
        cx,
    ));
    rows = rows.child(fact_row(
        text.trinity_label_focus.clone(),
        focus_label(&psi.focus, text),
        cx,
    ));
    if let Some(confidence) = psi.confidence {
        rows = rows.child(fact_row(
            text.trinity_label_confidence.clone(),
            format!("{:.0}%", confidence * 100.0),
            cx,
        ));
    }
    if let Some(count) = psi.cycle_count {
        rows = rows.child(fact_row(
            text.trinity_label_cycles.clone(),
            count.to_string(),
            cx,
        ));
    }
    card = card.child(rows);

    // Needs: five horizontal bars, low needs in the danger color.
    if !psi.needs.is_empty() {
        let mut needs = div().flex().flex_col().gap_1().w_full();
        for (key, value) in &psi.needs {
            needs = needs.child(need_bar(key, *value, text, cx));
        }
        card = card.child(needs);
    }

    let mut counters = div().flex().flex_row().gap_4();
    if let Some(nodes) = psi.memory_nodes {
        counters = counters.child(fact_row(
            text.trinity_label_memory_nodes.clone(),
            nodes.to_string(),
            cx,
        ));
    }
    if let Some(triples) = psi.memory_triples {
        counters = counters.child(fact_row(
            text.trinity_label_memory_triples.clone(),
            triples.to_string(),
            cx,
        ));
    }
    // G-Y6: the web card's third counter, in the same place the daemon's
    // memory snapshot provides it (absent when the daemon omits the field).
    if let Some(vocabulary) = psi.memory_vocabulary {
        counters = counters.child(fact_row(
            text.trinity_label_vocabulary.clone(),
            vocabulary.to_string(),
            cx,
        ));
    }
    card = card.child(counters);

    card = card.child(trend_strip(state, text, cx));
    card.into_any_element()
}

fn fact_row(label: String, value: String, cx: &App) -> gpui_kit::AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(value),
        )
        .into_any_element()
}

fn need_bar(key: &str, value: f64, text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let fill_color = match need_state(value) {
        NeedState::Low => cx.theme().danger,
        NeedState::Normal => cx.theme().primary,
        NeedState::High => cx.theme().accent,
    };
    let track_width = 160.0_f64;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(72.))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(need_label(key, text)),
        )
        .child(
            div()
                .w(px(track_width as f32))
                .h(px(6.))
                .rounded_full()
                .bg(cx.theme().muted)
                .child(
                    div()
                        .w(px((track_width * value.clamp(0.0, 1.0)).max(2.0) as f32))
                        .h(px(6.))
                        .rounded_full()
                        .bg(fill_color),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{:.0}%", value * 100.0)),
        )
        .into_any_element()
}

/// Inline trend strip: one mini column per history point, height by score.
fn trend_strip(state: &TrinityPageState, text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let mut strip = div().flex().flex_col().gap_1().w_full();
    strip = strip.child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text.trinity_trend_title.clone()),
    );
    if state.history.is_empty() {
        strip = strip.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("—"),
        );
        return strip.into_any_element();
    }
    let mut row = div().flex().flex_row().items_end().gap(px(1.)).h(px(28.));
    for score in state.history.iter().rev().take(120).rev() {
        let height = (8.0 + (score + 1.0) / 2.0 * 20.0).max(2.0) as f32;
        let color = if *score >= 0.0 {
            cx.theme().primary
        } else {
            cx.theme().danger
        };
        row = row.child(
            div()
                .w(px(3.))
                .h(px(height))
                .rounded(cx.theme().radius_tokens().sm)
                .bg(color),
        );
    }
    strip = strip.child(row);
    strip.into_any_element()
}

fn memory_section(
    state: &TrinityPageState,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    recall_input: &gpui_kit::Entity<InputState>,
    add_content: &gpui_kit::Entity<TextareaState>,
    add_project: &gpui_kit::Entity<InputState>,
    edit_content: &gpui_kit::Entity<TextareaState>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut section = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border);

    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(text.trinity_memory_title.clone()),
        );
    let mut actions = div().flex().items_center().gap_2();

    // Search input + submit.
    let search_tx = tx.clone();
    let search_entity = recall_input.clone();
    let search_view = view.clone();
    actions = actions.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(div().w(px(220.)).child(Input::new(&search_entity).small()))
            .child(
                Button::new("trinity-recall")
                    .label(text.trinity_memory_search.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _window, cx| {
                        let query = search_entity.read(cx).value().trim().to_string();
                        let Some(handle) = runtime::runtime_handle() else {
                            return;
                        };
                        if query.is_empty() {
                            if let Some(shell) = search_view.upgrade() {
                                shell.update(cx, |shell, cx| {
                                    shell.trinity.recall = None;
                                    shell.trinity.recall_busy = false;
                                    cx.notify();
                                });
                            }
                            return;
                        }
                        if let Some(shell) = search_view.upgrade() {
                            shell.update(cx, |shell, cx| {
                                shell.trinity.recall_busy = true;
                                cx.notify();
                            });
                        }
                        handle.trinity_recall(search_tx.clone(), query);
                    }),
            ),
    );

    // Add-memory toggle + inline form.
    let add_view = view.clone();
    actions = actions.child(
        Button::new("trinity-add-toggle")
            .label(text.trinity_memory_add.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click(move |_, _window, cx| {
                if let Some(shell) = add_view.upgrade() {
                    shell.update(cx, |shell, cx| {
                        shell.trinity.add_open = !shell.trinity.add_open;
                        cx.notify();
                    });
                }
            }),
    );
    header = header.child(actions);
    section = section.child(header);

    if let Some(message) = &state.action_error {
        section = section.child(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(format!("{}: {message}", text.trinity_action_failed)),
        );
    }

    // G-Y5: the correction verdict. The web raises a toast; the shell keeps
    // the line until the next correction or the next action starts.
    if let Some(applied) = state.correction {
        let (label, tone) = if applied {
            (text.trinity_memory_edit_applied.clone(), cx.theme().success)
        } else {
            (text.trinity_memory_edit_missing.clone(), cx.theme().danger)
        };
        section = section.child(
            div()
                .text_xs()
                .text_color(tone)
                .debug_selector(|| "trinity-correction-notice".to_string())
                .child(label),
        );
    }

    if state.add_open {
        let submit_tx = tx.clone();
        let content_entity = add_content.clone();
        let project_entity = add_project.clone();
        let cancel_view = view.clone();
        let submit_view = view.clone();
        section = section.child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .child(div().child(Textarea::new(&content_entity).small()))
                .child(div().w(px(280.)).child(Input::new(&project_entity).small()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("trinity-add-cancel")
                                .label(text.session_action_cancel.clone())
                                .small()
                                .with_variant(ButtonVariant::Ghost)
                                .on_click(move |_, _window, cx| {
                                    if let Some(shell) = cancel_view.upgrade() {
                                        shell.update(cx, |shell, cx| {
                                            shell.trinity.add_open = false;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("trinity-add-submit")
                                .label(text.trinity_memory_submit.clone())
                                .small()
                                .with_variant(ButtonVariant::Primary)
                                .on_click(move |_, window, cx| {
                                    let content =
                                        content_entity.read(cx).value().trim().to_string();
                                    if content.is_empty() {
                                        return;
                                    }
                                    let project =
                                        project_entity.read(cx).value().trim().to_string();
                                    let project = (!project.is_empty()).then_some(project);
                                    content_entity
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                    project_entity
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                    if let Some(shell) = submit_view.upgrade() {
                                        shell.update(cx, |shell, cx| {
                                            shell.trinity.add_open = false;
                                            shell.trinity.action_error = None;
                                            cx.notify();
                                        });
                                    }
                                    if let Some(handle) = runtime::runtime_handle() {
                                        handle.trinity_memorize(
                                            submit_tx.clone(),
                                            content,
                                            project,
                                            None,
                                        );
                                    }
                                }),
                        ),
                ),
        );
    }

    // Recall results replace the timeline while present.
    let entries_busy = state.recall_busy;
    let entries: &[MemoryEntry] = match &state.recall {
        Some(results) => results,
        None => &state.memories,
    };

    if entries.is_empty() {
        let empty_label = if state.recall.is_some() {
            text.trinity_memory_no_results.clone()
        } else if entries_busy || !state.memories_loaded {
            text.trinity_loading.clone()
        } else {
            text.trinity_memory_empty.clone()
        };
        section = section.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(empty_label),
        );
        return section.into_any_element();
    }

    let mut list = div().flex().flex_col().gap_1().w_full();
    for entry in entries {
        list = list.child(memory_row(
            state,
            view.clone(),
            edit_content,
            entry,
            tx,
            text,
            cx,
        ));
    }
    section = section.child(list);

    // Load-earlier only makes sense for the timeline, not recall results.
    if state.recall.is_none() {
        let load_tx = tx.clone();
        let load_view = view.clone();
        let label = if state.memories_exhausted {
            text.trinity_memory_no_more.clone()
        } else {
            text.trinity_memory_load_earlier.clone()
        };
        let exhausted = state.memories_exhausted;
        let loading = state.memories_loading;
        section = section.child(
            div().flex().flex_row().justify_center().child(
                Button::new("trinity-load-earlier")
                    .label(label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(exhausted || loading)
                    .on_click(move |_, _window, cx| {
                        if exhausted || loading {
                            return;
                        }
                        let Some(shell) = load_view.upgrade() else {
                            return;
                        };
                        shell.update(cx, |shell, cx| {
                            let before = shell
                                .trinity
                                .memories
                                .last()
                                .and_then(|last| last.timestamp);
                            shell.trinity.memories_loading = true;
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.trinity_load_memories(load_tx.clone(), before);
                            }
                            cx.notify();
                        });
                    }),
            ),
        );
    }

    section.into_any_element()
}

fn memory_row(
    state: &TrinityPageState,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    edit_content: &gpui_kit::Entity<TextareaState>,
    entry: &MemoryEntry,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let time = entry.timestamp.map(format_memory_time).unwrap_or_default();
    let meta = [entry.kind.clone(), entry.layer.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("/");

    let mut row = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_0p5()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border);

    let mut head = div().flex().flex_row().items_center().gap_2();
    if !time.is_empty() {
        head = head.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(time),
        );
    }
    if !meta.is_empty() {
        head = head.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(meta),
        );
    }
    if let Some(strength) = entry.strength {
        head = head.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{:.0}%", strength * 100.0)),
        );
    }

    // Two-step forget (web semantics; the shell has no timer, so the armed
    // state persists until clicked again, another entry is armed, or the
    // list reloads).
    let Some(entry_id) = entry.id.clone() else {
        row = row.child(head).child(content_line(entry, cx));
        return row.into_any_element();
    };

    // G-Y5: the corrected row replaces the content line with the shared edit
    // textarea and swaps the row actions for Save / Cancel, the way the web
    // row keeps its chrome and only the body becomes editable.
    if state.editing.as_deref() == Some(entry_id.as_str()) {
        let save_view = view.clone();
        let save_id = entry_id.clone();
        let save_content = edit_content.clone();
        let save_tx = tx.clone();
        let cancel_view = view.clone();
        let cancel_content = edit_content.clone();
        row = row.child(head).child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(Textarea::new(edit_content).small())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!(
                                "trinity-edit-cancel-{entry_id}"
                            )))
                            .label(text.session_action_cancel.clone())
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(move |_, window, cx| {
                                cancel_content
                                    .update(cx, |state, cx| state.set_value("", window, cx));
                                if let Some(shell) = cancel_view.upgrade() {
                                    shell.update(cx, |shell, cx| {
                                        shell.trinity.editing = None;
                                        cx.notify();
                                    });
                                }
                            }),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "trinity-edit-save-{entry_id}"
                            )))
                            .label(text.trinity_memory_edit_save.clone())
                            .small()
                            .with_variant(ButtonVariant::Primary)
                            .on_click(move |_, window, cx| {
                                let content = save_content.read(cx).value().trim().to_string();
                                if content.is_empty() {
                                    return;
                                }
                                save_content
                                    .update(cx, |state, cx| state.set_value("", window, cx));
                                if let Some(shell) = save_view.upgrade() {
                                    shell.update(cx, |shell, cx| {
                                        shell.trinity.editing = None;
                                        shell.trinity.correction = None;
                                        shell.trinity.action_error = None;
                                        cx.notify();
                                    });
                                }
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.trinity_memorize(
                                        save_tx.clone(),
                                        content,
                                        None,
                                        Some(save_id.clone()),
                                    );
                                }
                            }),
                        ),
                ),
        );
        return row.into_any_element();
    }

    let armed = state.forget_armed.as_deref() == Some(entry_id.as_str());

    let arm_view = view.clone();
    let arm_id = entry_id.clone();
    let forget_tx = tx.clone();
    let forget_id = entry_id.clone();
    head = head.child(
        Button::new(SharedString::from(format!("trinity-forget-{entry_id}")))
            .label(if armed {
                text.trinity_memory_forget_confirm.clone()
            } else {
                text.trinity_memory_forget.clone()
            })
            .small()
            .with_variant(if armed {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Ghost
            })
            .on_click(move |_, _window, cx| {
                let Some(shell) = arm_view.upgrade() else {
                    return;
                };
                if !armed {
                    shell.update(cx, |shell, cx| {
                        shell.trinity.forget_armed = Some(arm_id.clone());
                        cx.notify();
                    });
                    return;
                }
                shell.update(cx, |shell, cx| {
                    shell.trinity.forget_armed = None;
                    shell.trinity.action_error = None;
                    cx.notify();
                });
                if let Some(handle) = runtime::runtime_handle() {
                    handle.trinity_forget(forget_tx.clone(), forget_id.clone());
                }
            }),
    );

    let reinforce_tx = tx.clone();
    let reinforce_id = entry_id.clone();
    head = head.child(
        Button::new(SharedString::from(format!("trinity-reinforce-{entry_id}")))
            .label(text.trinity_memory_reinforce.clone())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(move |_, _window, _cx| {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.trinity_reinforce(reinforce_tx.clone(), reinforce_id.clone());
                }
            }),
    );

    // G-Y5 entry point: opens the inline editor with the current content
    // loaded, so Save submits the correction with `target_id` (web
    // `trinity_memorize { content, target_id }`).
    let edit_view = view.clone();
    let edit_id = entry_id.clone();
    let edit_content_entity = edit_content.clone();
    let edit_seed = entry.content.clone().unwrap_or_default();
    head = head.child(
        Button::new(SharedString::from(format!("trinity-edit-{entry_id}")))
            .label(text.trinity_memory_edit.clone())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(move |_, window, cx| {
                edit_content_entity.update(cx, |state, cx| {
                    state.set_value(edit_seed.clone(), window, cx)
                });
                if let Some(shell) = edit_view.upgrade() {
                    shell.update(cx, |shell, cx| {
                        shell.trinity.editing = Some(edit_id.clone());
                        shell.trinity.forget_armed = None;
                        shell.trinity.correction = None;
                        cx.notify();
                    });
                }
            }),
    );

    row = row.child(head).child(content_line(entry, cx));
    row.into_any_element()
}

fn content_line(entry: &MemoryEntry, cx: &App) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(entry.content.clone().unwrap_or_default())
        .into_any_element()
}

// ── Label mapping (web trinityDisplay.ts) ───────────────────────────

fn emotion_label(valence: &Option<String>, text: &ShellText) -> String {
    match valence.as_deref() {
        Some("positive_high") => text.trinity_emotion_positive_high.clone(),
        Some("positive_mild") => text.trinity_emotion_positive_mild.clone(),
        Some("curious") => text.trinity_emotion_curious.clone(),
        Some("confused") => text.trinity_emotion_confused.clone(),
        Some("negative_mild") => text.trinity_emotion_negative_mild.clone(),
        Some("negative_high") => text.trinity_emotion_negative_high.clone(),
        _ => text.trinity_emotion_neutral.clone(),
    }
}

fn focus_label(focus: &Option<String>, text: &ShellText) -> String {
    match focus.as_deref() {
        Some("respond") => text.trinity_focus_respond.clone(),
        Some("reflect") => text.trinity_focus_reflect.clone(),
        Some("explore") => text.trinity_focus_explore.clone(),
        Some("plan") => text.trinity_focus_plan.clone(),
        _ => text.trinity_focus_idle.clone(),
    }
}

fn need_label(key: &str, text: &ShellText) -> String {
    match key {
        "competence" => text.trinity_need_competence.clone(),
        "autonomy" => text.trinity_need_autonomy.clone(),
        "relatedness" => text.trinity_need_relatedness.clone(),
        "certainty" => text.trinity_need_certainty.clone(),
        "growth" => text.trinity_need_growth.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_config(tag: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openbitfun-gpui-trinity-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test dir");
        let path = dir.join("trinity.toml");
        std::fs::write(&path, content).expect("test config");
        path
    }

    #[test]
    fn reads_daemon_port_and_key_from_user_config() {
        let path = temp_config(
            "read",
            "[daemon]\nenabled = true\nport = 12345\nkeys = [\"k1\", \"k2\"]\n",
        );
        assert_eq!(read_daemon_port(&path), Some(12345));
        assert_eq!(read_daemon_key(&path), Some("k1".to_string()));
        let empty = temp_config("empty", "[other]\nx = 1\n");
        assert_eq!(read_daemon_port(&empty), None);
        assert_eq!(read_daemon_key(&empty), None);
    }

    #[test]
    fn valence_scores_follow_the_engine_axis() {
        assert_eq!(valence_score(&Some("positive_high".into())), Some(1.0));
        assert_eq!(valence_score(&Some("positive_mild".into())), Some(0.5));
        assert_eq!(valence_score(&Some("neutral".into())), Some(0.0));
        assert_eq!(valence_score(&Some("curious".into())), Some(0.3));
        assert_eq!(valence_score(&Some("confused".into())), Some(-0.2));
        assert_eq!(valence_score(&Some("negative_mild".into())), Some(-0.5));
        assert_eq!(valence_score(&Some("negative_high".into())), Some(-1.0));
        assert_eq!(valence_score(&Some("unknown".into())), None);
        assert_eq!(valence_score(&None), None);
    }

    #[test]
    fn need_states_band_by_threshold() {
        assert_eq!(need_state(0.4), NeedState::Low);
        assert_eq!(need_state(0.39), NeedState::Low);
        assert_eq!(need_state(0.41), NeedState::Normal);
        assert_eq!(need_state(0.79), NeedState::Normal);
        assert_eq!(need_state(0.8), NeedState::High);
    }

    #[test]
    fn parses_psi_status_and_memory_payloads() {
        let psi = parse_psi(&json!({
            "emotion": {"valence": "positive_mild", "arousal": 0.8},
            "focus": "respond",
            "confidence": 0.96,
            "needs": {"competence": 1.0, "relatedness": 0.3, "growth": 0.6},
            "memory": {"total_nodes": 42, "total_triples": 7, "vocabulary_size": 133},
            "cycle_count": 11
        }));
        assert_eq!(psi.valence.as_deref(), Some("positive_mild"));
        assert_eq!(psi.focus.as_deref(), Some("respond"));
        assert_eq!(psi.confidence, Some(0.96));
        // Canonical order, present needs only.
        assert_eq!(
            psi.needs,
            vec![
                ("competence".to_string(), 1.0),
                ("relatedness".to_string(), 0.3),
                ("growth".to_string(), 0.6),
            ]
        );
        assert_eq!(psi.memory_nodes, Some(42));
        assert_eq!(psi.memory_triples, Some(7));
        // G-Y6: the third memory-scale counter, absent when the daemon omits it.
        assert_eq!(psi.memory_vocabulary, Some(133));
        assert_eq!(
            parse_psi(&json!({"memory": {"total_nodes": 1}})).memory_vocabulary,
            None
        );
        assert_eq!(psi.cycle_count, Some(11));

        let status = parse_status(&json!({
            "awakened": true, "name": "银月", "persona": "sage",
            "user_name": "公子", "birthday": "2026-10-11"
        }));
        assert!(status.awakened);
        assert_eq!(status.name.as_deref(), Some("银月"));

        let items = parse_memory_items(&json!({"items": [
            {"id": "m1", "content": "remember this", "kind": "note",
             "layer": "episodic", "strength": 0.7, "timestamp": 1760000000}
        ]}));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("m1"));
        assert_eq!(items[0].timestamp, Some(1760000000));

        let recall = parse_recall_items(&json!({"results": [
            {"id": "r1", "content": "hit"}
        ]}));
        assert_eq!(recall.len(), 1);

        let history = parse_history(&json!({"points": [
            {"emotion": {"valence": "positive_high"}},
            {"emotion": {"valence": "negative_mild"}},
            {"emotion": {"valence": "mystery"}}
        ]}));
        assert_eq!(history, vec![1.0, -0.5]);
    }

    #[test]
    fn chip_phase_reads_the_daemon_snapshot() {
        assert_eq!(chip_phase(None), ChipPhase::Offline);
        assert_eq!(chip_phase(Some(false)), ChipPhase::Dormant);
        assert_eq!(chip_phase(Some(true)), ChipPhase::Awake);
    }

    #[test]
    fn emotion_dot_splits_on_valence_family() {
        assert!(emotion_dot_positive(&Some("positive_mild".to_string())));
        assert!(emotion_dot_positive(&Some("positive_high".to_string())));
        assert!(!emotion_dot_positive(&Some("negative_high".to_string())));
        assert!(!emotion_dot_positive(&Some("curious".to_string())));
        assert!(!emotion_dot_positive(&None));
    }

    #[test]
    fn memory_time_renders_month_day_hour_minute() {
        use chrono::TimeZone;
        let ts = Local
            .with_ymd_and_hms(2026, 10, 11, 9, 5, 0)
            .single()
            .expect("valid local time")
            .timestamp();
        assert_eq!(format_memory_time(ts), "10-11 09:05");
        // Millisecond-level values are guarded back to seconds.
        assert_eq!(format_memory_time(ts * 1000), "10-11 09:05");
    }

    /// Happy path: a fake daemon answers one framed request; the client
    /// parses `result` and pools the stream (the second call reuses it).
    #[tokio::test]
    async fn call_parses_result_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut header = [0u8; 4];
            stream.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header) as usize;
            let mut buf = vec![0u8; len];
            stream.read_exact(&mut buf).await.unwrap();
            let req: Value = serde_json::from_slice(&buf).unwrap();
            assert_eq!(req["method"], "get_status");
            assert_eq!(req["key"], "trinity-local-dev-key");
            let response = json!({"id": 1, "result": {"awakened": true}});
            let body = serde_json::to_vec(&response).unwrap();
            let mut frame = Vec::new();
            frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
            frame.extend_from_slice(&body);
            stream.write_all(&frame).await.unwrap();
        });

        let client = TrinityClient::for_port(port);
        let result = client.call("get_status", json!({})).await.unwrap();
        assert_eq!(result["awakened"], true);
        server.await.unwrap();
    }

    /// An `error` frame surfaces the daemon message as the call error.
    #[tokio::test]
    async fn call_surfaces_daemon_errors() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut header = [0u8; 4];
            stream.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header) as usize;
            let mut buf = vec![0u8; len];
            stream.read_exact(&mut buf).await.unwrap();
            let response = json!({"id": 1, "error": {"message": "not awakened"}});
            let body = serde_json::to_vec(&response).unwrap();
            let mut frame = Vec::new();
            frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
            frame.extend_from_slice(&body);
            stream.write_all(&frame).await.unwrap();
        });

        let client = TrinityClient::for_port(port);
        let error = client.call("memorize", json!({})).await.unwrap_err();
        assert_eq!(error, "not awakened");
        server.await.unwrap();
    }
}
