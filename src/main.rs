mod bridge;
mod cli;
mod state;
mod update;

#[tokio::main]
async fn main() {
    // reqwest 0.13 ships without a rustls provider; ring keeps the build free of
    // aws-lc's cmake cross-compilation requirements.
    bridge::ensure_crypto_provider();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = cli::run(argv).await;
    std::process::exit(code);
}
