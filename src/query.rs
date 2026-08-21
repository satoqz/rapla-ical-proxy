use std::str::FromStr;

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum UpstreamQuery {
    V1 { key: String, salt: String },
    V2 { user: String, file: String },
}

#[derive(Debug, Deserialize)]
struct ProxyQuery {
    #[serde(flatten)]
    base: UpstreamQuery,
    page: Option<String>,
    cutoff_date: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
pub struct Extension {
    pub url: String,
    pub start_year: i32,
    pub cutoff_date: Option<NaiveDate>,
    pub filters: Vec<(String, String)>,
}

pub fn apply_middleware(router: Router) -> Router {
    router.route_layer(middleware::from_fn(query_middleware))
}

async fn query_middleware(mut request: Request, next: Next) -> Response {
    let Some(query) = Extension::from_request_uri(request.uri()) else {
        return (
            StatusCode::BAD_REQUEST,
            "Failed to parse query, check your request URL!",
        )
            .into_response();
    };

    request.extensions_mut().insert(query);
    next.run(request).await
}

impl Extension {
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

    fn from_simple_uri(uri: &Uri) -> Option<Self> {
        let host = uri.host().unwrap_or(Self::DEFAULT_HOST);
        if !Self::HOST_ALLOWLIST.contains(&host) {
            return None;
        }

        let query_raw = uri.query()?;
        let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query_raw).ok()?;

        let mut filters = Vec::new();
        for (key, value) in pairs {
            if key == "filter"
                && let Some((property, val)) = value.split_once(':')
            {
                filters.push((property.to_ascii_uppercase(), val.to_ascii_lowercase()));
            }
        }

        let query: ProxyQuery = serde_urlencoded::from_str(query_raw).ok()?;

        let mut page = query.page.or_else(|| {
            let path = uri.path();
            path.starts_with("/rapla/").then(|| {
                path.trim_start_matches("/rapla/")
                    .trim_end_matches('/')
                    .to_string()
            })
        })?;

        if page == "ical" {
            page = "calendar".into()
        }

        const WEEKS_TWO_YEARS: usize = 104;
        const DAYS_ONE_YEAR: i64 = 365;

        let start = Utc::now() - Duration::days(DAYS_ONE_YEAR);

        let url = format!(
            "https://{}/rapla/{}?day={}&month={}&year={}&pages={WEEKS_TWO_YEARS}&{}",
            host,
            page,
            start.day(),
            start.month(),
            start.year(),
            // There's no reason this should fail, we already parsed it in the first place.
            serde_urlencoded::to_string(query.base).unwrap()
        );

        Some(Extension {
            url,
            filters,
            cutoff_date: query.cutoff_date,
            start_year: start.year(),
        })
    }
}
