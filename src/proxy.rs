use axum::extract::{Path, RawQuery};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::calendar::Calendar;
use crate::parser::{parse_calendar, ParseError};

#[derive(Serialize)]
struct ProxyError {
    /// Status code set when this is returned as a response.
    #[serde(skip)]
    status_code: StatusCode,

    /// Generic message describing the kind of error.
    message: &'static str,

    /// Specific error information.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    details: Option<ProxyErrorDetails>,

    /// What did upstream return so I don't have to check?
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream: Option<UpstreamInfo>,
}

#[derive(Clone, Serialize)]
struct UpstreamInfo {
    /// Transformed upstream URL.
    url: String,

    /// Status code we got from upstream.
    #[serde(skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ProxyErrorDetails {
    Err { details: String },
    Parse(ParseError),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        (self.status_code, Json(self)).into_response()
    }
}

impl IntoResponse for Calendar {
    fn into_response(self) -> axum::response::Response {
        (
            [("content-type", "text/calendar")],
            self.to_ics().to_string(),
        )
            .into_response()
    }
}

pub fn router(cache_config: Option<crate::cache::Config>) -> Router {
    let router = Router::new().route("/:calendar_path", get(handle_calendar));
    if let Some(cache_config) = cache_config {
        crate::cache::apply_middleware(router, cache_config)
    } else {
        router
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum CalendarQuery {
    V1 { key: String, salt: String },
    V2 { user: String, file: String },
}

async fn handle_calendar(
    Path(calendar_path): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> impl IntoResponse {
    let query = serde_urlencoded::from_str(&raw_query.unwrap_or_default()).map_err(|_| ProxyError {
        status_code: StatusCode::BAD_REQUEST,
        upstream: None,
        message: "bad query parameters: your URL either isn't a valid Rapla calendar URL or this service doesn't handle it yet",
        details: Some(ProxyErrorDetails::Err { details: "your URL needs to have either the 'key' and 'salt' parameters or the 'user' and 'file' parameters".into() }),
    })?;

    let (url, start_year) = generate_upstream_url(calendar_path, query);

    let upstream_response = reqwest::get(&url).await.map_err(|err| ProxyError {
        status_code: StatusCode::BAD_GATEWAY,
        upstream: Some(UpstreamInfo {
            url: url.clone(),
            status_code: None,
        }),
        message: "couldn't connect to upstream",
        details: Some(ProxyErrorDetails::Err {
            details: err.without_url().to_string(),
        }),
    })?;

    let upstream_status = upstream_response.status();
    let upstream_info = Some(UpstreamInfo {
        url: url.clone(),
        status_code: Some(upstream_status.as_u16()),
    });

    if !upstream_status.is_success() {
        return Err(ProxyError {
            status_code: upstream_status, // Propagate whatever issue they're having.
            upstream: upstream_info.clone(),
            message: "upstream returned bad status code",
            details: None,
        });
    }

    let html = upstream_response.text().await.map_err(|err| ProxyError {
        status_code: StatusCode::BAD_GATEWAY,
        upstream: upstream_info.clone(),
        message: "couldn't parse body returned by upstream",
        details: Some(ProxyErrorDetails::Err {
            details: err.without_url().to_string(),
        }),
    })?;

    parse_calendar(&html, start_year).map_err(|err| ProxyError {
        status_code: StatusCode::INTERNAL_SERVER_ERROR,
        upstream: upstream_info.clone(),
        message: "couldn't parse HTML returned by upstream",
        details: Some(ProxyErrorDetails::Parse(err)),
    })
}

fn generate_upstream_url(calendar_path: String, query: CalendarQuery) -> (String, i32) {
    // These don't need to be 100% accurate.
    const WEEKS_TWO_YEARS: usize = 104;
    const DAYS_ONE_YEAR: i64 = 365;

    let now = Utc::now();
    let year_ago = now - Duration::try_days(DAYS_ONE_YEAR).unwrap();

    const UPSTREAM: &str = "https://rapla.dhbw.de";
    let url = format!(
        "{UPSTREAM}/rapla/{calendar_path}?day={}&month={}&year={}&pages={WEEKS_TWO_YEARS}&{}",
        year_ago.day(),
        year_ago.month(),
        year_ago.year(),
        // There's no reason this should fail, we already parsed it in the first place.
        serde_urlencoded::to_string(query).unwrap(),
    );

    (url, year_ago.year())
}
