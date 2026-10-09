use openbitfun_gpui_shell::cli::{self, ProcessCommand};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = cli::parse_process_args(args.iter().map(String::as_str));
    if let Some(exit_code) = cli::answer_process_command(&command) {
        std::process::exit(exit_code);
    }

    openbitfun_gpui_shell::shell::init_tracing();

    // Process-lifetime tokio runtime: core services are async (config, i18n,
    // runtime bootstrap, session store) while the GPUI main loop is not.
    // Agent execution exceeds the default worker stack in debug builds, same
    // as the CLI (`src/apps/cli/src/main.rs` uses 16MiB for the same reason);
    // runtime threads do not inherit the spawning thread's stack size.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let tokio_handle = tokio_runtime.handle().clone();
    std::mem::forget(tokio_runtime);

    // Update commands answer and exit before the embedded runtime is booted:
    // packagers and update tooling drive the binary, not its window. Their
    // stdout stays readable for scripts (the "starting the shell" line below
    // is part of launching the window, not of answering these flags).
    if matches!(
        command,
        ProcessCommand::CheckUpdate { .. } | ProcessCommand::SelfUpdate { .. }
    ) {
        if let Some(exit_code) =
            openbitfun_gpui_shell::update::answer_update_command(&command, &tokio_handle)
        {
            std::process::exit(exit_code);
        }
    }

    // Single-instance gate sits after the flag answers on purpose: update
    // commands must work while another shell instance is running. Only a
    // second window launch is refused.
    if std::env::var("OPENBITFUN_GPUI_ALLOW_MULTI").as_deref() != Ok("1") {
        match openbitfun_gpui_shell::instance::acquire_instance_lock() {
            Ok(lock) => {
                // Held until process exit; the guard itself is not needed.
                std::mem::forget(lock);
            }
            Err(running) => {
                let message = format!(
                    "OpenBitFun GPUI shell is already running (pid {}). Close it first, or set OPENBITFUN_GPUI_ALLOW_MULTI=1 to allow parallel instances.",
                    running.pid
                );
                println!("{message}");
                eprintln!("{message}");
                std::process::exit(3);
            }
        }
    }

    tracing::info!("Starting OpenBitFun GPUI shell");
    openbitfun_gpui_shell::shell::launch_main_window(tokio_handle);
}
