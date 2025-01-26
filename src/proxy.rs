use std::fmt;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};

use crate::calendar::Calendar;
use crate::resolver::UpstreamUrlExtension;

#[derive(Debug)]
enum Error {
    Request(reqwest::Error),
    Parse(crate::parser::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match &self {
            Self::Request(err) if err.is_status() => "Upstream returned unexpected status code",
            Self::Request(_) => "Can't connect to upstream",
            Self::Parse(_) => "Can't parse calendar",
        };
        write!(f, "{message}")
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Request(err) => err,
            Self::Parse(err) => err,
        })
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::Request(value)
    }
}

impl From<crate::parser::Error> for Error {
    fn from(value: crate::parser::Error) -> Self {
        Self::Parse(value)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Request(err) if err.is_status() => {
                err.status().expect("error status should be set")
            } // Propagate whatever issue they're having.
            Self::Request(_) => StatusCode::BAD_GATEWAY,
            Self::Parse(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
    const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("reqwest client should build");

    router.route("/{*path}", get(handle_calendar).with_state(client))
}

async fn handle_calendar(
    State(client): State<reqwest::Client>,
    Extension(upstream): Extension<UpstreamUrlExtension>,
) -> Result<Response, Error> {
    let request = client.get(&upstream.url).build()?;
    let response = client.execute(request).await?.error_for_status()?;
    let html = response.text().await?;
    Ok(crate::parser::parse_calendar(&html, upstream.start_year)?.into_response())
}
