use gpui::{
    div, prelude::*, App, Application, Context, SharedString, TitlebarOptions, Window,
    WindowOptions,
};
use gpui_component::{button::Button, ActiveTheme, Root};

struct ShellWindow;

impl Render for ShellWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child("OpenBitFun GPUI Shell")
            .child("M0 skeleton window: the GPUI frontend is alive.")
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
                let view = cx.new(|_| ShellWindow);
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
