mod cache;
mod calendar;
mod logging;
mod parser;
mod proxy;
mod resolver;

use std::env::{self, VarError};
use std::fmt::Display;
use std::net::SocketAddr;
use std::str::FromStr;

use axum::Router;
use tokio::net::TcpListener;
use tokio::time::Duration;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Single-shot debug mode for parser development.
    #[cfg(debug_assertions)]
    if let Some(uri) = getenv("RAPLA_DEBUG") {
        use crate::proxy::{build_client, handle};
        use crate::resolver::UpstreamUrlComponents;

        let calendar = handle(
            &build_client(),
            UpstreamUrlComponents::from_request_uri(&uri)
                .expect("couldn't resolve upstream")
                .generate_url(),
        )
        .await
        .expect("couldn't handle request");

        eprintln!("{calendar:#?}");

        return Ok(());
    }

    let address =
        getenv("RAPLA_ADDRESS").unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8080)));

    let cache_ttl = Duration::from_secs(getenv("RAPLA_CACHE_TTL").unwrap_or(3600));
    let cache_capacity = getenv("RAPLA_CACHE_MAX_SIZE").unwrap_or(0);

    // Middlewares are layered, i.e. the later it is applied the earlier it is called.
    let router = Router::new();
    let router = crate::proxy::apply_routes(router);
    let router = crate::cache::apply_middleware(router, (cache_ttl, cache_capacity));
    let router = crate::resolver::apply_middleware(router);
    let router = crate::logging::apply_middleware(router);

    let listener = TcpListener::bind(address).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

fn getenv<T: FromStr>(key: &str) -> Option<T>
where
    T::Err: Display,
{
    use std::process;

    let val = match env::var(key) {
        Ok(val) => val,
        Err(VarError::NotPresent) => return None,
        Err(err) => {
            eprintln!("Invalid ${key}: {err}");
            process::exit(1);
        }
    };

    Some(T::from_str(&val).unwrap_or_else(|err| {
        eprintln!("Invalid ${key}: {err}");
        process::exit(1);
    }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl-c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
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
