use std::mem;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::response::Parts;
use axum::http::Uri;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use quick_cache::sync::Cache;
use quick_cache::Weighter;
use tokio::task;
use tokio::time::{self, Duration, Instant};

const CACHE_AGE_HEADER: &str = "x-cache-age";

#[derive(Debug, Clone)]
struct CachedResponse {
    parts: Parts,
    body: Bytes,
    timestamp: Instant,
}

async fn decompose_response(response: Response) -> CachedResponse {
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("response size is bigger than max usize");

    CachedResponse {
        parts,
        body: bytes,
        timestamp: Instant::now(),
    }
}

impl IntoResponse for CachedResponse {
    fn into_response(self) -> Response {
        let mut response = Response::from_parts(self.parts, Body::from(self.body));

        let age = self.timestamp.elapsed().as_secs().to_string();
        let headers = response.headers_mut();
        headers.insert(
            CACHE_AGE_HEADER,
            age.parse().expect("header value did not parse"),
        );

        response
    }
}

#[derive(Clone)]
struct CachedResponseWeighter;

impl Weighter<String, CachedResponse> for CachedResponseWeighter {
    fn weight(&self, key: &String, val: &CachedResponse) -> u64 {
        // Rough estimate of response size in bytes. Ensure weight is at least 1.
        (mem::size_of::<CachedResponse>() as u64 + key.len() as u64 + val.body.len() as u64).max(1)
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub ttl: Duration,
    pub max_size: u64,
}

#[derive(Debug)]
struct MiddlewareState {
    cache: Cache<String, CachedResponse, CachedResponseWeighter>,
    config: Config,
}

pub fn apply_middleware(router: Router, config: Config) -> Router {
    let capacity = config.max_size * 1024 * 1024; // Megabytes, weighter measures bytes.
    let cache = Cache::with_weighter(100, capacity, CachedResponseWeighter);

    router.route_layer(middleware::from_fn_with_state(
        Arc::new(MiddlewareState { cache, config }),
        cache_middleware,
    ))
}

async fn cache_middleware(
    State(state): State<Arc<MiddlewareState>>,
    uri: Uri,
    request: Request,
    next: Next,
) -> Response {
    let key = uri.to_string();

    let placeholder = match state.cache.get_value_or_guard_async(&key).await {
        Ok(cached) => return cached.into_response(),
        Err(placeholder) => placeholder,
    };

    let response = next.run(request).await;
    if state.config.max_size == 0 {
        return response;
    }

    // We're fine caching responses no matter the status. If things recover to normal automatically, just wait out the TTL.
    // If a fix needs to be pushed from our side, we're redeploying and thereby clearing the cache anyways.
    // Caching errored responses saves additional calls to upstream and parsing CPU time for paths that are most likely permanent fails anyways.

    // "Returns Err if the placeholder isn't in the cache anymore.
    // A placeholder can be removed as a result of a remove call or a non-placeholder insert with the same key."
    // This is the only place that we ever insert (locked by key). Additionally, the whole reason we're here
    // is that remove was called, and that we're about to schedule a new remove call.
    // TLDR; the unwrap should be fine.
    let decomposed = decompose_response(response).await;
    placeholder.insert(decomposed.clone()).unwrap();

    // Remove key from the cache once the TTL is expired.
    // Alternatively we could choose to do this whenever a value is fetched from
    // the cache and we notice that it has expired, but that encomplicates using
    // quick_cache's get_value_or_guard functionality and has other tradeoffs.
    task::spawn(async move {
        time::sleep(state.config.ttl).await;
        let now = Instant::now();
        // Ensure that we're removing only what was inserted above.
        // The cache could have evicted the entry itself because it got too large,
        // and a newer entry might already be in place. We don't want to remove that.
        if decomposed.timestamp + state.config.ttl <= now {
            state.cache.remove(&key);
        }
    });

    Response::from_parts(decomposed.parts, Body::from(decomposed.body))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::routing;
    use axum::Router;
    use tokio::net::TcpListener;
    use tokio::time::{self, Duration};

    use super::{apply_middleware, Config, CACHE_AGE_HEADER};

    async fn setup_listener() -> (TcpListener, String) {
        let addr = SocketAddr::from(([127, 0, 0, 1], 0));
        let listener = TcpListener::bind(addr).await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        (listener, base_url)
    }

    fn setup_basic_router() -> Router {
        Router::new().route("/{path}", routing::get(|| async { "Hello, World!" }))
    }

    #[tokio::test]
    async fn test_server_connection() {
        let (listener, url) = setup_listener().await;
        tokio::select! {
            result = axum::serve(listener, setup_basic_router()) => { result.unwrap(); }
            result = reqwest::get(url) => { result.unwrap(); }
        };
    }

    #[tokio::test]
    async fn test_cache_middleware() {
        let ttl = Duration::from_secs(3600);
        let config = Config { ttl, max_size: 100 };

        let router = apply_middleware(setup_basic_router(), config);
        let (listener, base_url) = setup_listener().await;

        let fuzzer = async {
            let response = reqwest::get(format!("{base_url}/test")).await.unwrap();
            assert!(response.headers().get(CACHE_AGE_HEADER).is_none());

            let response = reqwest::get(format!("{base_url}/test")).await.unwrap();
            assert!(response.headers().get(CACHE_AGE_HEADER).is_some());

            time::pause();
            time::advance(ttl).await;

            let response = reqwest::get(format!("{base_url}/test")).await.unwrap();
            assert!(response.headers().get(CACHE_AGE_HEADER).is_none());
        };

        tokio::select! {
            result = axum::serve(listener, router) => result.unwrap(),
            _ = fuzzer => {},
        };
    }
}
