use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use sentry::protocol::Map;
use sentry::Breadcrumb;
use serde_json::Value;

use crate::calendar::Calendar;
use crate::helpers;
use crate::resolver::UpstreamUrlExtension;

#[derive(Debug)]
struct Error {
    message: &'static str,
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Reqwest(reqwest::Error),
    Status(reqwest::StatusCode),
    Parse(crate::parser::Error),
}

impl Error {
    pub fn new(message: &'static str, kind: ErrorKind) -> Self {
        Self { message, kind }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Reqwest(err) => Some(err),
            ErrorKind::Parse(err) => Some(err),
            ErrorKind::Status(_) => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error {
    fn capture(self) -> Self {
        sentry::capture_error(&self);
        self
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self.kind {
            ErrorKind::Reqwest(_) => StatusCode::BAD_GATEWAY,
            ErrorKind::Parse(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorKind::Status(status) => status, // Propagate whatever issue they're having.
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
    router.route("/{*path}", get(handle_calendar))
}

async fn handle_calendar(
    Extension(upstream): Extension<UpstreamUrlExtension>,
) -> impl IntoResponse {
    breadcrumb("Sending request to Rapla", "http", {
        helpers::map!({ "method": "GET", "url": upstream.url })
    });

    let response = send_request(&upstream.url).await?;
    let status = response.status();

    breadcrumb("Got response from Rapla", "http", {
        helpers::map!({ "method": "GET", "url": upstream.url, "status_code": status.as_u16() })
    });

    if !status.is_success() {
        return Err(Error::new(
            "Upstream returned bad status code",
            ErrorKind::Status(status),
        ));
    }

    let html = response.text().await.map_err(|err| {
        Error::new(
            "Couldn't parse body returned by upstream",
            ErrorKind::Reqwest(err),
        )
        // I'd be curious to know if this ever occurs.
        .capture()
    })?;

    crate::parser::parse_calendar(&html, upstream.start_year).map_err(|err| {
        Error::new(
            "Couldn't parse HTML returned by upstream",
            ErrorKind::Parse(err),
        )
        // These are the important errors we really want to track.
        // Given that Rapla returned a successful status code for a set of well-formed
        // query parameters, we can be at least 90% certain that our parsing is broken
        // (or was broken, depending on how you see it).
        .capture()
    })
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

async fn send_request(url: &str) -> Result<reqwest::Response, Error> {
    let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .build()
        .map_err(|err| Error::new("Couldn't connect to upstream", ErrorKind::Reqwest(err)))?;

    let request = client
        .get(url)
        .build()
        .map_err(|err| Error::new("Couldn't connect to upstream", ErrorKind::Reqwest(err)))?;

    client
        .execute(request)
        .await
        .map_err(|err| Error::new("Request to upstream failed", ErrorKind::Reqwest(err)))
}
