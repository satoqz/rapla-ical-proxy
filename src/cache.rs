use std::mem;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::response::Parts;
use axum::http::Uri;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use quick_cache::sync::Cache;
use quick_cache::Weighter;

#[derive(Clone)]
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
            "x-cache-age",
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

pub struct Config {
    pub ttl: Duration,
    pub max_size: u64,
}

struct MiddlewareState {
    cache: Cache<String, CachedResponse, CachedResponseWeighter>,
    ttl: Duration,
}

pub fn apply_middleware(router: Router, config: Config) -> Router {
    let capacity = config.max_size * 1024 * 1024; // Megabytes, weighter measures bytes
    let cache = Cache::with_weighter(100, capacity, CachedResponseWeighter);

    router.route_layer(middleware::from_fn_with_state(
        Arc::new(MiddlewareState {
            cache,
            ttl: config.ttl,
        }),
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
    let now = Instant::now();

    match state.cache.get(&key) {
        Some(cached) if (now - cached.timestamp) < state.ttl => return cached.into_response(),
        _ => {}
    }

    let response = next.run(request).await;
    let decomposed = decompose_response(response).await;

    // We're fine caching responses no matter the status. If things recover to normal automatically, just wait out the TTL.
    // If a fix needs to be pushed from our side, we're redeploying and thereby clearing the cache anyways.
    // Caching errored responses saves additional calls to upstream and parsing CPU time for paths that are most likely permanent fails anyways.
    state.cache.insert(key, decomposed.clone());
    Response::from_parts(decomposed.parts, Body::from(decomposed.body))
}
