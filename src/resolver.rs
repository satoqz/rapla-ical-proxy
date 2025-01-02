use std::str::FromStr;

use axum::extract::Request;
use axum::http::Uri;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum RaplaBaseQuery {
    V1 { key: String, salt: String },
    V2 { user: String, file: String },
}

#[derive(Debug, Clone, Deserialize)]
struct RaplaQueryWithPage {
    #[serde(flatten)]
    base: RaplaBaseQuery,
    page: Option<String>,
}

#[derive(Debug, Clone)]
struct UpstreamUrlComponents {
    host: String,
    page: String,
    query: RaplaBaseQuery,
}

impl UpstreamUrlComponents {
    fn try_from_request(request: &Request) -> Option<Self> {
        // Try either:
        //  1. The request URL itself (e.g. https://rapla.satoqz.net/rapla/calendar).
        //  2. The request path, treating it as a URL (e.g. https://rapla.satoqz.net/https://rapla.dhbw.de/rapla/calendar).
        Self::try_from_uri(request.uri()).or_else(|| {
            let path_and_query = request.uri().path_and_query()?;
            let uri = Uri::from_str(path_and_query.as_str().trim_start_matches('/')).ok()?;
            Self::try_from_uri(&uri)
        })
    }

    const DEFAULT_HOST: &'static str = "rapla.dhbw.de";
    const HOST_ALLOWLIST: [&'static str; 2] = ["rapla.dhbw.de", "rapla-ravensburg.dhbw.de"];

    fn try_from_uri(uri: &Uri) -> Option<Self> {
        let host = match uri.host() {
            Some(host) if Self::HOST_ALLOWLIST.contains(&host) => host,
            Some(_) => return None,
            None => Self::DEFAULT_HOST,
        };

        let query: RaplaQueryWithPage = serde_urlencoded::from_str(uri.query()?).ok()?;
        let page = match query.page {
            Some(page) => page,
            None => {
                let path = uri.path();
                if path.starts_with("/rapla/") {
                    path.trim_start_matches("/rapla/").to_string()
                } else {
                    return None;
                }
            }
        };

        Some(UpstreamUrlComponents {
            host: host.to_string(),
            page,
            query: query.base,
        })
    }

    fn generate_url(self) -> String {
        // These don't need to be 100% accurate.
        const WEEKS_TWO_YEARS: usize = 104;
        const DAYS_ONE_YEAR: i64 = 365;

        let now = Utc::now();
        let year_ago = now - Duration::try_days(DAYS_ONE_YEAR).unwrap();

        format!(
            "https://{}/rapla/{}?day={}&month={}&year={}&pages={WEEKS_TWO_YEARS}&{}",
            self.host,
            self.page,
            year_ago.day(),
            year_ago.month(),
            year_ago.year(),
            // There's no reason this should fail, we already parsed it in the first place.
            serde_urlencoded::to_string(self.query).unwrap()
        )
    }
}

#[derive(Debug, Clone)]
pub struct UpstreamUrlExtension(pub String);

pub fn apply_middleware(router: Router) -> Router {
    router.route_layer(middleware::from_fn(resolver_middleware))
}

async fn resolver_middleware(mut request: Request, next: Next) -> Response {
    match UpstreamUrlComponents::try_from_request(&request) {
        Some(components) => request
            .extensions_mut()
            .insert(UpstreamUrlExtension(components.generate_url())),
        None => return "oh no".into_response(),
    };

    next.run(request).await
}
