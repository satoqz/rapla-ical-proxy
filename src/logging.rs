use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;

pub fn apply_middleware(router: Router) -> Router {
    router.route_layer(middleware::from_fn_with_state(
        Arc::new(AtomicU64::new(0)),
        logging_middleware,
    ))
}

async fn logging_middleware(
    State(request_counter): State<Arc<AtomicU64>>,
    request: Request,
    next: Next,
) -> Response {
    let request_id = request_counter.fetch_add(1, Ordering::Relaxed);
    let request_url = request.uri().to_string();

    let start_time = Instant::now();
    let response = next.run(request).await;

    let json = serde_json::json!({
        "request_id": request_id,
        "status_code": response.status().as_u16(),
        "cached": response.headers().get("x-cache-age").is_some(),
        "processing_time": Instant::now().duration_since(start_time).as_secs_f64(),
        "url": request_url,
    });

    let Ok(mut buf) = serde_json::to_vec(&json) else {
        return response;
    };

    buf.push(b'\n');
    let _ = io::stderr().lock().write_all(&buf);

    response
}
