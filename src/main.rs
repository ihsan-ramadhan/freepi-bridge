mod bridge;
mod autostart;
mod cli;
mod config;
mod state;
mod update;

fn main() {
    // reqwest 0.13 ships without a rustls provider; ring keeps the build free of
    // aws-lc's cmake cross-compilation requirements.
    bridge::ensure_crypto_provider();

    config::apply();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = runtime.block_on(cli::run(argv));
    std::process::exit(code);
}
