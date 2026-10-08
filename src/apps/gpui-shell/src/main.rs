mod i18n;
mod runtime;
mod theme;

use gpui::{
    div, prelude::*, px, size, App, Application, Bounds, Context, Entity, Global, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants},
    input::{Input, InputState},
    scroll::ScrollableElement,
    ActiveTheme, Root, Sizable, StyledExt,
};
use openbitfun_runtime_ports::AgentSessionSummary;
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::RuntimeEvent;
use crate::theme::{ThemePreset, DEFAULT_PRESET_ID};

/// App-global shell state: theme presets from the CLI palette plus the active
/// selection. Lives outside `ShellWindow` so button handlers can reach it.
struct ShellState {
    presets: Vec<ThemePreset>,
    active_preset: usize,
}

impl Global for ShellState {}

/// Lifecycle of the embedded runtime, mirrored into the status line.
#[derive(Clone)]
enum RuntimePhase {
    Starting,
    Ready,
    Failed(String),
}

/// One read-only session list entry, composed (including localized metadata)
/// when the snapshot arrives so the render path never touches the i18n service.
struct SessionRow {
    name: String,
    metadata: String,
}

struct ShellWindow {
    query: Entity<InputState>,
    draft: Entity<InputState>,
    phase: RuntimePhase,
    sessions: Vec<SessionRow>,
    list_error: Option<String>,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
}

impl ShellWindow {
    fn new(
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
        event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = i18n::current(cx);
        let query = cx.new(|cx| {
            let state = InputState::new(window, cx).placeholder(text.query_placeholder.clone());
            state.focus(window, cx);
            state
        });
        let draft = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(text.draft_placeholder.clone())
                .multi_line(true)
                .rows(6)
        });
        receive_runtime_events(event_rx, window, cx);
        Self {
            query,
            draft,
            phase: RuntimePhase::Starting,
            sessions: Vec::new(),
            list_error: None,
            event_tx,
        }
    }

    /// Applies one locale snapshot to the locale-sensitive input state.
    fn apply_locale(&mut self, text: &ShellText, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |state, cx| {
            state.set_placeholder(text.query_placeholder.clone(), window, cx)
        });
        self.draft.update(cx, |state, cx| {
            state.set_placeholder(text.draft_placeholder.clone(), window, cx)
        });
    }

    fn compose_row(&self, session: &AgentSessionSummary, cx: &App) -> SessionRow {
        let mut metadata = vec![session.agent_type.clone()];
        if let Some(model) = &session.model_id {
            metadata.push(model.clone());
        }
        metadata.push(i18n::turns_label(session.turn_count, cx));
        metadata.push(runtime::format_last_active(session.last_active_at_ms));
        SessionRow {
            name: session.session_name.clone(),
            metadata: metadata
                .iter()
                .filter(|part| !part.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · "),
        }
    }

    fn apply_runtime_event(
        &mut self,
        event: RuntimeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RuntimeEvent::Locale(locale) => {
                i18n::load_locale(locale, cx);
                let text = i18n::current(cx);
                self.apply_locale(&text, window, cx);
            }
            RuntimeEvent::Ready => {
                self.phase = RuntimePhase::Ready;
            }
            RuntimeEvent::Sessions(sessions) => {
                self.phase = RuntimePhase::Ready;
                self.list_error = None;
                self.sessions = sessions.iter().map(|s| self.compose_row(s, cx)).collect();
            }
            RuntimeEvent::SessionsFailed(error) => {
                self.list_error = Some(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::Failed(error) => {
                self.phase = RuntimePhase::Failed(i18n::failed_status(&error, cx));
            }
        }
        cx.notify();
    }

    fn render_status_line(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::Div {
        match &self.phase {
            RuntimePhase::Starting => div()
                .text_color(cx.theme().muted_foreground)
                .child(text.status_starting.clone()),
            RuntimePhase::Ready => {
                let workspace = runtime::runtime_handle()
                    .map(|handle| handle.workspace().name.clone())
                    .unwrap_or_default();
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", text.status_ready, workspace))
            }
            RuntimePhase::Failed(error) => div().text_color(cx.theme().danger).child(error.clone()),
        }
    }

    fn render_sessions(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut header = div()
            .id("sessions-header")
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.sessions_title.clone()),
            );
        if matches!(self.phase, RuntimePhase::Ready) {
            let tx = self.event_tx.clone();
            header = header.child(
                Button::new("reload-sessions")
                    .label(text.sessions_reload.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.reload_sessions(tx.clone());
                        }
                    }),
            );
        }

        let mut list = div().v_flex().id("sessions-list").gap_2().p_1();
        if self.sessions.is_empty() {
            list = list.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.sessions_empty.clone()),
            );
        }
        for row in &self.sessions {
            list = list.child(
                div()
                    .v_flex()
                    .gap_0p5()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(row.metadata.clone()),
                    ),
            );
        }

        let mut section = div()
            .v_flex()
            .id("sessions-section")
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(header);
        if let Some(error) = &self.list_error {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        section.child(list.overflow_y_scrollbar())
    }

    fn render_verification(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::Div {
        let query = self.query.clone();
        let draft = self.draft.clone();
        let language_button = Button::new("language")
            .label(text.locale.as_str())
            .on_click(move |_, window, cx| {
                let next = i18n::next_locale(i18n::current(cx).locale);
                i18n::load_locale(next, cx);
                let text = i18n::current(cx);
                query.update(cx, |state, cx| {
                    state.set_placeholder(text.query_placeholder.clone(), window, cx)
                });
                draft.update(cx, |state, cx| {
                    state.set_placeholder(text.draft_placeholder.clone(), window, cx)
                });
            });

        let ping_button = Button::new("ping")
            .label(text.ping.clone())
            .on_click(|_, _, _| {
                tracing::info!("Shell ping button clicked");
            });

        let active_preset = cx.global::<ShellState>().active_preset;
        let preset_labels: Vec<(&'static str, usize)> = cx
            .global::<ShellState>()
            .presets
            .iter()
            .enumerate()
            .map(|(index, preset)| (preset.label, index))
            .collect();

        let mut theme_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.theme.clone()),
            );
        for (label, index) in preset_labels {
            let variant = if index == active_preset {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Ghost
            };
            theme_row = theme_row.child(
                Button::new(SharedString::from(format!("preset-{label}")))
                    .label(label)
                    .with_variant(variant)
                    .on_click(move |_, _, cx| {
                        let preset = {
                            let state = cx.global_mut::<ShellState>();
                            state.active_preset = index;
                            state.presets[index].clone()
                        };
                        preset.apply(cx);
                    }),
            );
        }

        div()
            .v_flex()
            .gap_2()
            .child(Input::new(&self.query).w_full())
            .child(Input::new(&self.draft).w_full().h(px(140.)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(div().text_color(cx.theme().muted_foreground).child(format!(
                        "{}: {}",
                        text.language,
                        text.locale.as_str()
                    )))
                    .child(language_button)
                    .child(ping_button),
            )
            .child(theme_row)
    }
}

impl Render for ShellWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text: ShellText = i18n::current(cx);

        div()
            .size_full()
            .v_flex()
            .gap_3()
            .p_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().text_size(px(20.)).child("OpenBitFun GPUI Shell"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.subtitle.clone()),
            )
            .child(self.render_status_line(&text, cx))
            .child(self.render_sessions(&text, cx))
            .child(self.render_verification(&text, cx))
    }
}

/// Bridges runtime events into the view: the tokio side of the shell pushes
/// [`RuntimeEvent`]s, this loop applies them on the UI thread.
fn receive_runtime_events(
    mut event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
    window: &Window,
    cx: &mut Context<ShellWindow>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(event) = event_rx.recv().await {
            let alive = cx
                .update(|window, cx| {
                    this.update(cx, |this, cx| this.apply_runtime_event(event, window, cx))
                        .is_ok()
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        }
    })
    .detach();
}

fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stdout)
        .try_init();
}

fn main() {
    init_tracing();
    tracing::info!("Starting OpenBitFun GPUI shell");

    // Process-lifetime tokio runtime: core services are async (config, i18n,
    // runtime bootstrap, session store) while the GPUI main loop is not.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let tokio_handle = tokio_runtime.handle().clone();
    std::mem::forget(tokio_runtime);

    let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
    // The view keeps its own sender for the session reload action.
    let view_event_tx = event_tx.clone();

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);

        let presets = theme::load_presets();
        let active_preset = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active_preset) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset,
        });

        i18n::init(cx);

        runtime::spawn_bootstrap(tokio_handle, event_tx);

        let open_result = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("OpenBitFun GPUI Shell")),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: gpui::point(px(80.), px(80.)),
                    size: size(px(1000.), px(680.)),
                })),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| ShellWindow::new(view_event_tx, event_rx, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );

        match open_result {
            Ok(_) => {
                tracing::info!("Main window opened");
                cx.activate(true);
            }
            Err(err) => {
                tracing::error!("Failed to open main window: {}", err);
            }
        }
    });
}
