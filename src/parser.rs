use std::ops::Not;
use std::sync::OnceLock;

use chrono::{Duration, NaiveDate, NaiveTime};
use html_escape::decode_html_entities;
use scraper::{ElementRef, Html, Selector};

use crate::calendar::{Calendar, Event};

trait InspectNone {
    fn inspect_none(self, f: impl FnOnce()) -> Self;
}

impl<T> InspectNone for Option<T> {
    #[inline]
    fn inspect_none(self, f: impl FnOnce()) -> Self {
        self.or_else(|| {
            f();
            None
        })
    }
}

macro_rules! loc {
    () => {
        concat!(file!(), ":", line!(), ":", column!())
    };
}

macro_rules! trace_none {
    ($($arg:expr), *) => {
        || {
            if cfg!(debug_assertions) {
                eprintln!(concat!("[trace] none at ", loc!()));
                $(dbg!($arg);)*
            }
        }
    };
}

macro_rules! trace_err {
    ($($arg:expr), *) => {
        |err| {
            if cfg!(debug_assertions) {
                eprintln!("[trace] err at {}: {err}", loc!());
                $(dbg!($arg);)*
            }
        }
    };
}

macro_rules! select {
    ($element:expr, $query:expr) => {{
        static SELECTOR: OnceLock<Selector> = OnceLock::new();
        $element.select(SELECTOR.get_or_init(|| Selector::parse($query).unwrap()))
    }};
}

pub fn parse_calendar(s: &str, mut start_year: i32) -> Option<Calendar> {
    let html = Html::parse_document(s);
    let name = select!(html, "title")
        .next()
        .inspect_none(trace_none!())?
        .inner_html()
        .trim()
        .to_string();

    let mut events = Vec::new();
    for (idx, week_element) in select!(html, "div.calendar > table.week_table > tbody").enumerate()
    {
        let week_number_html = select!(week_element, "th.week_number")
            .next()
            .inspect_none(trace_none!())?
            .inner_html();

        let week_number = week_number_html
            .split(' ')
            .nth(1)
            .inspect_none(trace_none!(&week_number_html))?
            .parse::<usize>()
            .inspect_err(trace_err!())
            .ok()?;

        if week_number == 1 && idx > 0 {
            start_year += 1;
        }

        let mut week_events = parse_week(week_element, start_year).inspect_none(trace_none!())?;
        events.append(&mut week_events);
    }

    Some(Calendar { name, events })
}

fn parse_week(element: ElementRef, start_year: i32) -> Option<Vec<Event>> {
    let week_header = select!(element, "tr > td.week_header > nobr")
        .next()
        .inspect_none(trace_none!())?
        .inner_html();

    let mut day_month = week_header
        .split(' ')
        .nth(1)
        .inspect_none(trace_none!())?
        .trim_end_matches('.')
        .split('.');

    let start_day = day_month
        .next()
        .inspect_none(trace_none!())?
        .parse::<u32>()
        .inspect_err(trace_err!())
        .ok()?;

    let start_month = day_month
        .next()?
        .parse::<u32>()
        .inspect_err(trace_err!())
        .ok()?;

    let monday =
        NaiveDate::from_ymd_opt(start_year, start_month, start_day).inspect_none(trace_none!())?;

    let mut events = Vec::new();
    for row in select!(element, "tr").skip(1) {
        let mut day_index = 0;
        for column in select!(row, "td") {
            let class = column
                .value()
                .classes()
                .next()
                .inspect_none(trace_none!())?;

            if class.starts_with("week_separatorcell") {
                day_index += 1;
            }

            if class != "week_block" {
                continue;
            }

            let date = monday + Duration::try_days(day_index).inspect_none(trace_none!())?;
            events.push(parse_event(column, date).inspect_none(trace_none!())?);
        }
    }

    Some(events)
}

fn parse_event(element: ElementRef, date: NaiveDate) -> Option<Event> {
    // Sometimes there is an extra <span class="link"> wrapper around the content we're after.
    // We pick last element to ensure we have the innermost matched element.
    let details = select!(element, ":is(a, span.link)")
        .last()
        .inspect_none(trace_none!())?
        .inner_html();

    let mut details_split = details.split("<br>");

    let times_raw = details_split.next().inspect_none(trace_none!())?;
    let mut times_raw_split = times_raw.split("&nbsp;-");

    let start_time_raw = times_raw_split.next().inspect_none(trace_none!())?;
    let end_time_raw = times_raw_split.next().inspect_none(trace_none!())?;

    // Some genuises at DHBW find it a great idea to leave out the start and/or end time
    // to signify "full day" which is to be interpreted as "from 08:00 until 18:00".
    // The dash in the middle is always there though. For now.
    let start = if start_time_raw.is_empty() {
        NaiveTime::from_hms_opt(8, 0, 0).unwrap()
    } else {
        NaiveTime::parse_from_str(start_time_raw, "%H:%M")
            .inspect_err(trace_err!())
            .ok()?
    };
    let end = if end_time_raw.is_empty() {
        NaiveTime::from_hms_opt(18, 0, 0).unwrap()
    } else {
        NaiveTime::parse_from_str(end_time_raw, "%H:%M")
            .inspect_err(trace_err!())
            .ok()?
    };

    let title = details_split.next().inspect_none(trace_none!())?;
    let title = decode_html_entities(title).to_string();

    let resources = select!(element, "span.resource")
        .map(|location| decode_html_entities(&location.inner_html()).to_string())
        .collect::<Vec<_>>();
    let location = resources.last().cloned();
    let description = resources.is_empty().not().then(|| resources.join(", "));

    let persons = select!(element, "span.person")
        .map(|person| decode_html_entities(&person.inner_html()).to_string())
        .collect::<Vec<_>>();
    let organizer = persons.is_empty().not().then(|| persons.join(", "));

    Some(Event {
        date,
        start,
        end,
        title,
        location,
        organizer,
        description,
    })
}
