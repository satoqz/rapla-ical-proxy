use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};

use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use axum_extra::headers::UserAgent;
use axum_extra::TypedHeader;
use chrono::{Datelike, Duration, Utc};
use reqwest::{Client, Error as ReqwestError, StatusCode};
use sentry::protocol::Map;
use sentry::Breadcrumb;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calendar::Calendar;
use crate::helpers;
use crate::parser::{parse_calendar, ParseError};

#[derive(Debug)]
struct ProxyError {
    message: &'static str,
    kind: ProxyErrorKind,
}

#[derive(Debug)]
enum ProxyErrorKind {
    Reqwest(ReqwestError),
    Status(StatusCode),
    Parse(ParseError),
}

impl ProxyError {
    pub fn new(message: &'static str, kind: ProxyErrorKind) -> Self {
        Self { message, kind }
    }
}

impl Error for ProxyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ProxyErrorKind::Reqwest(err) => Some(err),
            ProxyErrorKind::Parse(err) => Some(err),
            ProxyErrorKind::Status(_) => None,
        }
    }
}

impl Display for ProxyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.message)
    }
}

impl ProxyError {
    fn capture(self) -> Self {
        sentry::capture_error(&self);
        self
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let status = match self.kind {
            ProxyErrorKind::Reqwest(_) => StatusCode::BAD_GATEWAY,
            ProxyErrorKind::Parse(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyErrorKind::Status(status) => status, // Propagate whatever issue they're having.
        };

        (
            status,
            [("content-type", "text/plain")],
            format!("Error: {self}"),
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

pub fn apply_routes(router: Router) -> Router {
    router.route("/rapla/:calendar_path", get(handle_calendar))
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum CalendarQuery {
    V1 { key: String, salt: String },
    V2 { user: String, file: String },
}

fn breadcrumb(message: &str, ty: &str, data: Map<String, Value>) {
    sentry::add_breadcrumb(Breadcrumb {
        ty: ty.into(),
        category: Some("proxy".into()),
        message: Some(message.into()),
        data,
        ..Default::default()
    });
}

async fn handle_calendar(
    Path(calendar_path): Path<String>,
    Query(query): Query<CalendarQuery>,
    TypedHeader(user_agent): TypedHeader<UserAgent>,
) -> impl IntoResponse {
    breadcrumb("Incoming request", "default", {
        let mut map = Map::from_iter(match serde_json::to_value(&query).unwrap() {
            Value::Object(obj) => obj.into_iter(),
            _ => unreachable!(),
        });
        map.insert("user_agent".into(), user_agent.as_str().into());
        map
    });

    let (url, start_year) = generate_upstream_url(calendar_path, query);
    breadcrumb("Sending request to Rapla", "http", {
        helpers::map!({ "method": "GET", "url": url })
    });

    let response = send_request(&url).await?;
    let status = response.status();
    breadcrumb("Got response from Rapla", "http", {
        helpers::map!({ "method": "GET", "url": url, "status_code": status.as_u16() })
    });

    if !status.is_success() {
        return Err(ProxyError::new(
            "Upstream returned bad status code",
            ProxyErrorKind::Status(status),
        ));
    }

    let html = response.text().await.map_err(|err| {
        ProxyError::new(
            "Couldn't parse body returned by upstream",
            ProxyErrorKind::Reqwest(err),
        )
        // I'd be curious to know if this ever occurs.
        .capture()
    })?;

    parse_calendar(&html, start_year).map_err(|err| {
        ProxyError::new(
            "Couldn't parse HTML returned by upstream",
            ProxyErrorKind::Parse(err),
        )
        // These are the important errors we really want to track.
        // Given that Rapla returned a successful status code for a set of well-formed
        // query parameters, we can be at least 90% certain that our parsing is broken
        // (or was broken, depending on how you see it).
        .capture()
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
    let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    let client = Client::builder()
        .user_agent(user_agent)
        .build()
        .map_err(|err| {
            ProxyError::new("Couldn't connect to upstream", ProxyErrorKind::Reqwest(err))
        })?;

    let request = client.get(url).build().map_err(|err| {
        ProxyError::new("Couldn't connect to upstream", ProxyErrorKind::Reqwest(err))
    })?;

    client
        .execute(request)
        .await
        .map_err(|err| ProxyError::new("Request to upstream failed", ProxyErrorKind::Reqwest(err)))
}
