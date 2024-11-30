use std::fmt::{self, Display};

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{Datelike, Duration, Utc};
use serde::Deserialize;

use crate::parser::parse_calendar;
use crate::structs::Calendar;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CalendarQuery {
    V1 { key: String, salt: String },
    V2 { user: String, file: String },
}

impl Display for CalendarQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V1 { key, salt } => write!(f, "&key={key}&salt={salt}"),
            Self::V2 { user, file } => write!(f, "&user={user}&file={file}"),
        }
    }
}

enum Error {
    UpstreamConnection(reqwest::Error),
    UpstreamStatus(String, reqwest::StatusCode),
    UpstreamBody(String),
    Parse(String),
}

impl Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::UpstreamConnection(_) => StatusCode::BAD_GATEWAY,
            Self::UpstreamStatus(_, status) => {
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            Self::UpstreamBody(_) => StatusCode::BAD_GATEWAY,
            Self::Parse(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UpstreamConnection(err) => write!(
                f,
                "Could not connect to upstream at {}",
                err.url()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| String::from("<No URL found?>"))
            ),
            Error::UpstreamStatus(url, status) => {
                write!(
                    f,
                    "Upstream returned unsuccessful status code {status} at {url}",
                )
            }
            Error::UpstreamBody(url) => {
                write!(f, "Upstream  returned invalid body at {url}")
            }
            Error::Parse(url) => write!(f, "Upstream  returned HTML that did not parse at {url}"),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.status_code(),
            [("content-type", "text/plain")],
            self.to_string(),
        )
            .into_response()
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

async fn handle_calendar(
    Path(calendar_path): Path<String>,
    Query(query): Query<CalendarQuery>,
) -> impl IntoResponse {
    let (url, start_year) = generate_upstream_url(calendar_path, query);
    let html = fetch_html(&url).await?;
    parse_calendar(&html, start_year).ok_or_else(|| Error::Parse(url.clone()))
}

async fn fetch_html(url: &str) -> Result<String, Error> {
    let response = reqwest::get(url).await.map_err(Error::UpstreamConnection)?;
    let response_status = response.status();

    if !response_status.is_success() {
        return Err(Error::UpstreamStatus(url.into(), response_status));
    }

    response
        .text()
        .await
        .map_err(|_| Error::UpstreamBody(url.into()))
}

fn generate_upstream_url(calendar_path: String, query: CalendarQuery) -> (String, i32) {
    // these don't need to be 100% accurate
    const WEEKS_TWO_YEARS: usize = 104;
    const DAYS_ONE_YEAR: i64 = 365;

    let now = Utc::now();
    let year_ago = now - Duration::try_days(DAYS_ONE_YEAR).unwrap();

    const UPSTREAM: &str = "https://rapla.dhbw.de";
    let url = format!(
        "{UPSTREAM}/rapla/{calendar_path}?day={}&month={}&year={}&pages={WEEKS_TWO_YEARS}{query}",
        year_ago.day(),
        year_ago.month(),
        year_ago.year(),
    );

    (url, year_ago.year())
}
