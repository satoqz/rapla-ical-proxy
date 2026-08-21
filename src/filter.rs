use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use icalendar::{Calendar, CalendarComponent, Component};

use crate::resolver::UpstreamUrlExtension;

pub fn apply_middleware(router: Router) -> Router {
    router.route_layer(middleware::from_fn(filter_middleware))
}

async fn filter_middleware(
    Extension(upstream): Extension<UpstreamUrlExtension>,
    request: Request,
    next: Next,
) -> Response {
    if upstream.filters.is_empty() {
        return next.run(request).await;
    }

    let response = next.run(request).await;
    let status = response.status();
    let is_calendar = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/calendar"));

    if status != StatusCode::OK || !is_calendar {
        return response;
    }

    let (parts, body) = response.into_parts();
    let data = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("response size is bigger than max usize");

    match filter_ics(&data, &upstream.filters) {
        Ok(filtered) => Response::from_parts(parts, Body::from(filtered)),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

/// Filter calendar events by matching a property value.
/// Each entry is a `(property, value)` pair parsed from `filter=PROPERTY:value`.
fn filter_ics(data: &[u8], filters: &[(String, String)]) -> Result<Vec<u8>, String> {
    let filters: Vec<(String, String)> = filters
        .iter()
        .map(|(prop, val)| (prop.to_ascii_uppercase(), val.to_ascii_lowercase()))
        .collect();

    let text = std::str::from_utf8(data).map_err(|e| format!("invalid utf-8: {}", e))?;
    let mut calendar: Calendar = text
        .parse()
        .map_err(|e: String| format!("failed to parse calendar: {}", e))?;

    calendar.components.retain(|component| match component {
        CalendarComponent::Event(event) => filters.iter().any(|(property, value)| {
            event
                .property_value(property)
                .map(|v| v.to_ascii_lowercase().contains(value.as_str()))
                .unwrap_or(false)
        }),
        _ => true,
    });

    Ok(calendar.to_string().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::filter_ics;

    #[test]
    fn keeps_matching_event_summary() {
        let ics = b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Linux Fundamentals\nEND:VEVENT\nBEGIN:VEVENT\nSUMMARY:Math\nEND:VEVENT\nEND:VCALENDAR\n";

        let filtered =
            String::from_utf8(filter_ics(ics, &[("SUMMARY".into(), "Linux".into())]).unwrap())
                .unwrap();

        assert!(filtered.contains("SUMMARY:Linux Fundamentals"));
        assert!(!filtered.contains("SUMMARY:Math"));
    }

    #[test]
    fn keeps_multiple_matching_event_summaries() {
        let ics = b"BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Linux\nEND:VEVENT\nBEGIN:VEVENT\nSUMMARY:Software Engineering\nEND:VEVENT\nEND:VCALENDAR\n";

        let filtered = String::from_utf8(
            filter_ics(
                ics,
                &[
                    ("SUMMARY".into(), "Linux".into()),
                    ("SUMMARY".into(), "Engineering".into()),
                ],
            )
            .unwrap(),
        )
        .unwrap();

        assert!(filtered.contains("SUMMARY:Linux"));
        assert!(filtered.contains("SUMMARY:Software Engineering"));
    }
}
