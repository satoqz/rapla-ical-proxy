use std::fmt;

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use icalendar::Calendar;

use crate::query;

pub enum Error {
    Request(reqwest::Error),
    Parse,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match &self {
            Self::Request(err) if err.is_status() => "Upstream returned unexpected status code!",
            Self::Request(_) => "Failed to connect to upstream!",
            Self::Parse => "Failed to parse calendar!",
        };
        write!(f, "{message}")
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Error").field(&self.to_string()).finish()
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(err) => Some(err),
            Self::Parse => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::Request(value)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Request(err) if err.is_status() => {
                err.status().expect("error status should be set")
            } // Propagate whatever issue they're having.
            Self::Request(_) => StatusCode::BAD_GATEWAY,
            Self::Parse => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (
            status,
            [(header::CONTENT_TYPE, "text/plain")],
            self.to_string(),
        )
            .into_response()
    }
}

pub fn build_client() -> reqwest::Client {
    const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("reqwest client should build")
}

pub fn apply_routes(router: Router) -> Router {
    router.route("/{*path}", get(request_handler).with_state(build_client()))
}

async fn request_handler(
    State(client): State<reqwest::Client>,
    Extension(query): Extension<query::Extension>,
) -> Result<Response, Error> {
    let calendar = handle(&client, query).await?;
    Ok((
        [(header::CONTENT_TYPE, "text/calendar")],
        calendar.to_string(),
    )
        .into_response())
}

pub async fn handle(client: &reqwest::Client, query: query::Extension) -> Result<Calendar, Error> {
    let request = client.get(&query.url).build()?;
    let response = client.execute(request).await?.error_for_status()?;
    let html = response.text().await?;
    crate::parser::parse_calendar(&html, query.start_year).ok_or(Error::Parse)
}
