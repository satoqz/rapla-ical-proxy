use std::str::FromStr;

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
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
    cutoff_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpstreamUrlComponents {
    host: String,
    page: String,
    query: RaplaBaseQuery,
    cutoff_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpstreamUrlExtension {
    pub url: String,
    pub start_year: i32,
}

pub fn apply_middleware(router: Router) -> Router {
    router.route_layer(middleware::from_fn(resolver_middleware))
}

async fn resolver_middleware(mut request: Request, next: Next) -> Response {
    let Some(mut components) = UpstreamUrlComponents::from_request_uri(request.uri()) else {
        return (
            StatusCode::BAD_REQUEST,
            "Error: Could not determine upstream URL, check your request URL",
        )
            .into_response();
    };

    if components.page == "ical" {
        components.page = "calendar".into()
    }

    request.extensions_mut().insert(components.generate_url());
    next.run(request).await
}

impl UpstreamUrlComponents {
    const DEFAULT_HOST: &str = "rapla.dhbw.de";
    const HOST_ALLOWLIST: &[&str] = &[Self::DEFAULT_HOST];
    // TODO: Allow access to the Ravensburg instance once it supports the pages query parameter.
    // const HOST_ALLOWLIST: &[&str] = &[Self::DEFAULT_HOST, "rapla-ravensburg.dhbw.de"];

    pub fn from_request_uri(uri: &Uri) -> Option<Self> {
        // Try either:
        //  1. The request path, treating it as a URL (e.g. https://rapla.satoqz.net/https://rapla.dhbw.de/rapla/calendar).
        //  2. The request URL itself (e.g. https://rapla.satoqz.net/rapla/calendar).
        // Order matters!!!
        let uri_in_path = uri
            .path_and_query()
            .map(|path| path.as_str().trim_start_matches('/'))
            .and_then(|path| Uri::from_str(path).ok());

        uri_in_path
            .as_ref()
            .and_then(Self::from_simple_uri)
            .or_else(|| Self::from_simple_uri(uri))
    }

    pub fn from_simple_uri(uri: &Uri) -> Option<Self> {
        let host = uri.host().unwrap_or(Self::DEFAULT_HOST);
        if !Self::HOST_ALLOWLIST.contains(&host) {
            return None;
        }

        let query: RaplaQueryWithPage = serde_urlencoded::from_str(uri.query()?).ok()?;
        let page = query.page.or_else(|| {
            let path = uri.path();
            path.starts_with("/rapla/").then(|| {
                path.trim_start_matches("/rapla/")
                    .trim_end_matches('/')
                    .to_string()
            })
        })?;

        Some(UpstreamUrlComponents {
            host: host.to_string(),
            page,
            query: query.base,
            cutoff_date: query.cutoff_date,
        })
    }

    pub fn generate_url(self) -> UpstreamUrlExtension {
        // These don't need to be 100% accurate.
        const WEEKS_TWO_YEARS: usize = 104;
        const DAYS_ONE_YEAR: i64 = 365;

        // Parse cutoff_date if provided, otherwise use year_ago
        let cutoff = self
            .cutoff_date
            .and_then(|date_str| {
                chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc())
                    .ok()
            })
            .unwrap_or(Utc::now() - Duration::try_days(DAYS_ONE_YEAR).unwrap());

        let url = format!(
            "https://{}/rapla/{}?day={}&month={}&year={}&pages={WEEKS_TWO_YEARS}&{}",
            self.host,
            self.page,
            cutoff.day(),
            cutoff.month(),
            cutoff.year(),
            // There's no reason this should fail, we already parsed it in the first place.
            serde_urlencoded::to_string(self.query).unwrap()
        );

        UpstreamUrlExtension {
            url,
            start_year: cutoff.year(),
        }
    }
}
