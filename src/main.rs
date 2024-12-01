mod cache;
mod calendar;
mod logging;
mod parser;
mod proxy;

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::signal;

#[derive(Parser)]
struct Args {
    /// Socket address (IP and port) to listen on.
    #[arg(
        short,
        long,
        env("RAPLA_ADDRESS"),
        default_value_t = SocketAddr::from(([127, 0, 0, 1], 8080))
    )]
    address: SocketAddr,

    /// Enable caching of parsed calendars.
    #[arg(short = 'c', long, env("RAPLA_CACHE"))]
    cache: bool,

    /// Time-to-live for cached responses (in seconds).
    #[arg(short = 't', long, env("RAPLA_CACHE_TTL"), default_value_t = 3600)]
    cache_ttl: u64,

    /// Maximum cache size in Megabytes.
    #[arg(short = 's', long, env("RAPLA_CACHE_MAX_SIZE"), default_value_t = 50)]
    cache_max_size: u64,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();

    let router = Router::new().nest(
        "/rapla",
        crate::proxy::router(args.cache.then_some(crate::cache::Config {
            ttl: Duration::from_secs(args.cache_ttl),
            max_size: args.cache_max_size,
        })),
    );

    let listener = TcpListener::bind(args.address).await?;

    eprintln!("Listening on address:    {}", args.address);
    eprintln!("Caching enabled:         {}", args.cache);
    eprintln!("Cache time to live:      {}s", args.cache_ttl);
    eprintln!("Cache max size:          {}mb", args.cache_max_size);

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install ctrl-c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
