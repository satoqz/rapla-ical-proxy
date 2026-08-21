use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use chrono::NaiveDate;
use icalendar::{Calendar, CalendarComponent, Component};

use crate::query;

pub fn apply_middleware(router: Router) -> Router {
    router.route_layer(middleware::from_fn(filter_middleware))
}

async fn filter_middleware(
    Extension(query): Extension<query::Extension>,
    request: Request,
    next: Next,
) -> Response {
    let response = next.run(request).await;

    let is_calendar = response.status() == StatusCode::OK
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/calendar"));

    if !is_calendar {
        return response;
    }

    if query.filters.is_empty() && query.cutoff_date.is_none() {
        return response;
    }

    let (parts, body) = response.into_parts();
    let data = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("response size is bigger than max usize");

    // SAFETY: The body returned by the proxy layer is created from a `String`,
    // so we know it is valid UTF-8.
    let Ok(mut calendar) = unsafe { std::str::from_utf8_unchecked(&data) }.parse::<Calendar>()
    else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to re-parse calendar for filtering!",
        )
            .into_response();
    };

    if let Some(date) = query.cutoff_date {
        apply_cutoff_date(&mut calendar, date);
    }

    if !query.filters.is_empty() {
        apply_ical_filters(&mut calendar, &query.filters);
    }

    Response::from_parts(parts, Body::from(calendar.to_string()))
}

/// Filter calendar events by trimming the start of the calendar up until the
/// given cutoff date.
fn apply_cutoff_date(calendar: &mut Calendar, cutoff: NaiveDate) {
    calendar.components.retain(|component| match component {
        CalendarComponent::Event(event) => event
            .get_start()
            .map(|start| start.date_naive() >= cutoff)
            .unwrap_or(true),
        _ => true,
    });
}

/// Filter calendar events by matching ical property values.
/// Each entry is a `(property, value)` pair parsed from `filter=PROPERTY:value`.
fn apply_ical_filters(calendar: &mut Calendar, filters: &[(String, String)]) {
    calendar.components.retain(|component| match component {
        CalendarComponent::Event(event) => filters.iter().any(|(property, value)| {
            event
                .property_value(property)
                .map(|v| v.to_ascii_lowercase().contains(value.as_str()))
                .unwrap_or(false)
        }),
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use icalendar::{Event, EventLike, Todo};

    fn event(summary: &str) -> Event {
        Event::new().summary(summary).done()
    }

    fn event_on(summary: &str, date: NaiveDate) -> Event {
        Event::new()
            .summary(summary)
            .starts(date.and_hms_opt(8, 0, 0).unwrap())
            .done()
    }

    fn calendar_with(events: Vec<Event>) -> Calendar {
        let mut calendar = Calendar::new();
        for event in events {
            calendar.push(event);
        }
        calendar
    }

    fn summaries(calendar: &Calendar) -> Vec<&str> {
        calendar
            .components
            .iter()
            .filter_map(|component| match component {
                CalendarComponent::Event(event) => event.property_value("SUMMARY"),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn keeps_matching_event_summary() {
        let mut calendar = calendar_with(vec![event("Linux Fundamentals"), event("Math")]);

        apply_ical_filters(&mut calendar, &[("SUMMARY".into(), "linux".into())]);

        assert_eq!(summaries(&calendar), ["Linux Fundamentals"]);
    }

    #[test]
    fn keeps_multiple_matching_event_summaries() {
        let mut calendar = calendar_with(vec![event("Linux"), event("Software Engineering")]);

        apply_ical_filters(
            &mut calendar,
            &[
                ("SUMMARY".into(), "linux".into()),
                ("SUMMARY".into(), "engineering".into()),
            ],
        );

        assert_eq!(summaries(&calendar), ["Linux", "Software Engineering"]);
    }

    #[test]
    fn drops_events_before_cutoff() {
        let cutoff = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();

        let mut calendar = calendar_with(vec![
            event_on("Old", NaiveDate::from_ymd_opt(2026, 5, 31).unwrap()),
            event_on("Boundary", cutoff),
            event_on("New", NaiveDate::from_ymd_opt(2026, 6, 2).unwrap()),
        ]);

        apply_cutoff_date(&mut calendar, cutoff);

        assert_eq!(summaries(&calendar), ["Boundary", "New"]);
    }

    #[test]
    fn keeps_events_without_start_and_other_components() {
        let cutoff = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();

        let mut calendar = calendar_with(vec![event("No Start")]);
        calendar.push(Todo::new().summary("Todo").done());

        apply_cutoff_date(&mut calendar, cutoff);

        assert_eq!(summaries(&calendar), ["No Start"]);
        assert_eq!(calendar.components.len(), 2);
    }
}
