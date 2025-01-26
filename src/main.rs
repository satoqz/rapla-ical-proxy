mod cache;
mod calendar;
mod logging;
mod parser;
mod proxy;
mod resolver;

use std::io;
use std::net::SocketAddr;

use axum::Router;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::Duration;

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

    /// Time-to-live for cached responses (in seconds).
    #[arg(short = 't', long, env("RAPLA_CACHE_TTL"), default_value_t = 3600)]
    cache_ttl: u64,

    /// Maximum cache size in Megabytes. A value of 0 results in no caching.
    #[arg(short = 's', long, env("RAPLA_CACHE_MAX_SIZE"), default_value_t = 0)]
    cache_max_size: u64,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();

    eprintln!("Listening on address:    {}", args.address);
    eprintln!("Cache time to live:      {}s", args.cache_ttl);
    eprintln!("Cache max size:          {}mb", args.cache_max_size);

    let cache_config = crate::cache::Config {
        ttl: Duration::from_secs(args.cache_ttl),
        max_size: args.cache_max_size,
    };

    // Middlewares are layered, i.e. the later it is applied the earlier it is called.
    let router = Router::new();
    let router = crate::proxy::apply_routes(router);
    let router = crate::cache::apply_middleware(router, cache_config);
    let router = crate::resolver::apply_middleware(router);
    let router = crate::logging::apply_middleware(router);

    let listener = TcpListener::bind(args.address).await?;
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
