fn main() {
    openbitfun_gpui_shell::shell::init_tracing();
    tracing::info!("Starting OpenBitFun GPUI shell");

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

    openbitfun_gpui_shell::shell::launch_main_window(tokio_handle);
}
