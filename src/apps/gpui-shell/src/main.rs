mod i18n;
mod theme;

use gpui::{
    div, prelude::*, px, size, App, Application, Bounds, Context, Entity, Global, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants},
    input::{Input, InputState},
    ActiveTheme, Root,
};

use crate::i18n::ShellText;
use crate::theme::{ThemePreset, DEFAULT_PRESET_ID};

/// App-global shell state: theme presets from the CLI palette plus the active
/// selection. Lives outside `ShellWindow` so button handlers can reach it.
struct ShellState {
    presets: Vec<ThemePreset>,
    active_preset: usize,
}

impl Global for ShellState {}

struct ShellWindow {
    query: Entity<InputState>,
    draft: Entity<InputState>,
}

impl ShellWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        Self { query, draft }
    }
}

impl Render for ShellWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text: ShellText = i18n::current(cx);
        let active_preset = cx.global::<ShellState>().active_preset;
        let preset_labels: Vec<(&'static str, usize)> = cx
            .global::<ShellState>()
            .presets
            .iter()
            .enumerate()
            .map(|(index, preset)| (preset.label, index))
            .collect();

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
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().text_size(px(20.)).child("OpenBitFun GPUI Shell"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.subtitle.clone()),
            )
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

    Application::new().run(|cx: &mut App| {
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
                let view = cx.new(|cx| ShellWindow::new(window, cx));
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
