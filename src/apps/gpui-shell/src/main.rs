use gpui::{
    div, prelude::*, px, App, Application, Context, Entity, SharedString, TitlebarOptions, Window,
    WindowOptions,
};
use gpui_component::{
    button::Button,
    input::{Input, InputState},
    ActiveTheme, Root,
};

struct ShellWindow {
    query: Entity<InputState>,
    draft: Entity<InputState>,
}

impl ShellWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            let state =
                InputState::new(window, cx).placeholder("IME single-line input (preedit here)");
            state.focus(window, cx);
            state
        });
        let draft = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("IME multi-line input (compose across lines)")
                .multi_line(true)
                .rows(6)
        });
        Self { query, draft }
    }
}

impl Render for ShellWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child("OpenBitFun GPUI Shell - M1 IME verification")
            .child(Input::new(&self.query).w_full())
            .child(Input::new(&self.draft).w_full().h(px(140.)))
            .child(Button::new("ping").label("Ping").on_click(|_, _, _| {
                tracing::info!("Shell ping button clicked");
            }))
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

        let open_result = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("OpenBitFun GPUI Shell")),
                    ..Default::default()
                }),
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
