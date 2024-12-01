use std::error::Error;
use std::fmt::Display;

use axum::extract::{Path, RawQuery};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Datelike, Duration, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::calendar::Calendar;
use crate::parser::{parse_calendar, ParseError};

#[derive(Debug, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
struct UpstreamInfo {
    /// Transformed upstream URL.
    url: String,

    /// Status code we got from upstream.
    #[serde(skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ProxyErrorDetails {
    Err { details: String },
    Parse(ParseError),
}

impl Error for ProxyError {}

impl Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(details) = &self.details {
            write!(f, ": {details}")?;
        }
        Ok(())
    }
}

impl Display for ProxyErrorDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Err { details } => write!(f, "{details}"),
            Self::Parse(err) => write!(f, "{err}"),
        }
    }
}

impl ProxyError {
    fn sentry_capture(self) -> Self {
        sentry::capture_error(&self);
        self
    }
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

    let router = if let Some(cache_config) = cache_config {
        crate::cache::apply_middleware(router, cache_config)
    } else {
        router
    };

    crate::logging::apply_middleware(router)
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

    let upstream_response = send_request(&url).await?;
    let upstream_status = upstream_response.status();

    let upstream_info = || {
        Some(UpstreamInfo {
            url: url.clone(),
            status_code: Some(upstream_status.as_u16()),
        })
    };

    if !upstream_status.is_success() {
        return Err(ProxyError {
            status_code: upstream_status, // Propagate whatever issue they're having.
            upstream: upstream_info(),
            message: "upstream returned bad status code",
            details: None,
        });
    }

    let html = upstream_response.text().await.map_err(|err| {
        ProxyError {
            status_code: StatusCode::BAD_GATEWAY,
            upstream: upstream_info(),
            message: "couldn't parse body returned by upstream",
            details: Some(ProxyErrorDetails::Err {
                details: err.without_url().to_string(),
            }),
        }
        // I'd be curious to know if this ever occurs.
        .sentry_capture()
    })?;

    parse_calendar(&html, start_year).map_err(|err| {
        ProxyError {
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
            upstream: upstream_info(),
            message: "couldn't parse HTML returned by upstream",
            details: Some(ProxyErrorDetails::Parse(err)),
        }
        // These are the important errors we really want to track.
        // Given that Rapla returned a successful status code for a set of well-formed
        // query parameters, we can be at least 90% certain that our parsing is broken
        // (or was broken, depending on how you see it).
        .sentry_capture()
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

async fn send_request(url: &str) -> Result<reqwest::Response, ProxyError> {
    let into_proxy_error = |err: reqwest::Error| ProxyError {
        status_code: StatusCode::BAD_GATEWAY,
        upstream: Some(UpstreamInfo {
            url: url.to_string(),
            status_code: None,
        }),
        message: "couldn't connect to upstream",
        details: Some(ProxyErrorDetails::Err {
            details: err.without_url().to_string(),
        }),
    };

    let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    let client = Client::builder()
        .user_agent(user_agent)
        .build()
        .map_err(into_proxy_error)?;

    let request = client.get(url).build().map_err(into_proxy_error)?;
    client.execute(request).await.map_err(into_proxy_error)
}
