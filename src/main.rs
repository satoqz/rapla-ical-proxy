mod cache;
mod calendar;
mod cli;
mod parser;
mod proxy;

use std::env;
use std::io;

use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = crate::cli::parse(env::args().skip(1).collect());

    let router = Router::new().nest(
        "/rapla",
        crate::proxy::router(args.cache_enable.then_some(crate::cache::Config {
            ttl: args.cache_ttl,
            max_size: args.cache_max_size,
        })),
    );

    let listener = TcpListener::bind(args.address).await?;

    eprintln!("Listening on address:    {}", args.address);
    eprintln!("Caching enabled:         {}", args.cache_enable);
    eprintln!("Cache time to live:      {}s", args.cache_ttl.as_secs());
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
