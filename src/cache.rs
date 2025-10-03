use std::mem;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::response::Parts;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::{Extension, Router};
use moka::future::Cache;
use tokio::time::{Duration, Instant};

use crate::resolver::UpstreamUrlExtension;

const CACHE_AGE_HEADER: &str = "X-Cache-Age";

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

pub fn apply_middleware(router: Router, (ttl, max_capacity): (Duration, u64)) -> Router {
    let cache = Cache::builder()
        .time_to_live(ttl)
        .max_capacity(max_capacity * 1024 * 1024) // Megabytes, weigher measures bytes
        .weigher(|url: &String, response: &CachedResponse| {
            (mem::size_of::<CachedResponse>()
                .saturating_add(url.len())
                .saturating_add(response.body.len()))
            .max(1)
            .try_into()
            .unwrap_or(u32::MAX)
        })
        .build();

    router.route_layer(middleware::from_fn_with_state(
        Arc::new(cache),
        cache_middleware,
    ))
}

async fn cache_middleware(
    State(cache): State<Arc<Cache<String, CachedResponse>>>,
    Extension(upstream): Extension<UpstreamUrlExtension>,
    request: Request,
    next: Next,
) -> Response {
    let mut cache_hit = true;

    let cached = cache
        .get_with(upstream.url, async {
            cache_hit = false;
            // Cache responses no matter their status. Caching errored responses
            // saves additional calls to upstream and parsing CPU time for paths
            // that are often permanent fails anyways. The only trade-off is
            // that temporary errors driven by upstream will take the full time
            // to live to recover from, even if upstream recovers earlier.
            let response = next.run(request).await;
            decompose_response(response).await
        })
        .await;

    let mut response = Response::from_parts(cached.parts, Body::from(cached.body));

    if cache_hit {
        let age = cached.timestamp.elapsed().as_secs().to_string();
        response.headers_mut().insert(
            CACHE_AGE_HEADER,
            age.parse().expect("header value did not parse"),
        );
    }

    response
}
