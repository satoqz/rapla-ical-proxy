use std::fmt::{self, Display};
use std::ops::Not;

use chrono::{Duration, NaiveDate, NaiveTime};
use once_cell::sync::Lazy;
use scraper::{ElementRef, Html, Selector};

use crate::structs::{Calendar, Event};

pub struct ParseError {
    location: String,
    kind: ParseErrorKind,
}

pub enum ParseErrorKind {
    Generic(String),
    Select(&'static str),
}

impl Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ParseErrorKind::Generic(message) => {
                write!(f, "{message} (source location: {})", self.location)
            }
            ParseErrorKind::Select(query) => write!(
                f,
                "query `{query}` resulted in no elements (source location: {})",
                self.location
            ),
        }
    }
}

macro_rules! source_url {
    () => {
        format!(
            "{}/blob/{}/{}#L{}",
            env!("CARGO_PKG_REPOSITORY"),
            env!("GIT_COMMIT_HASH"),
            file!(),
            line!(),
        )
    };
}

macro_rules! generic_parse_error {
    ($message:expr) => {
        ParseError {
            location: source_url!(),
            kind: ParseErrorKind::Generic($message),
        }
    };
}

macro_rules! selector {
    ($query:expr) => {{
        static SELECTOR: Lazy<Selector> = Lazy::new(|| Selector::parse($query).unwrap());
        &SELECTOR
    }};
}

macro_rules! select_first {
    ($element:expr, $query:expr) => {
        $element.select(selector!($query)).next().ok_or(ParseError {
            location: source_url!(),
            kind: ParseErrorKind::Select($query),
        })
    };
}

pub fn parse_calendar(s: &str, mut start_year: i32) -> Result<Calendar, ParseError> {
    let html = Html::parse_document(s);
    let name = select_first!(html, "title")?
        .inner_html()
        .trim()
        .to_string();

    let mut events = Vec::new();
    for (idx, week_element) in html
        .select(selector!("div.calendar > table.week_table > tbody"))
        .enumerate()
    {
        let week_number = select_first!(week_element, "th.week_number")?
            .inner_html()
            .split(' ')
            .nth(1)
            .ok_or(generic_parse_error!("".into()))?
            .parse::<usize>()
            .map_err(|_| generic_parse_error!("".into()))?;

        if week_number == 1 && idx > 0 {
            start_year += 1;
        }

        let mut week_events = parse_week(week_element, start_year)?;
        events.append(&mut week_events);
    }

    Ok(Calendar { name, events })
}

fn parse_week(element: ElementRef, start_year: i32) -> Result<Vec<Event>, ParseError> {
    let start_date_raw = select_first!(element, "tr > td.week_header > nobr")?.inner_html();

    let mut day_month = start_date_raw
        .split(' ')
        .nth(1)
        .ok_or(generic_parse_error!("".into()))?
        .trim_end_matches('.')
        .split('.');

    let start_day = day_month
        .next()
        .ok_or(generic_parse_error!("".into()))?
        .parse::<u32>()
        .map_err(|_| generic_parse_error!("".into()))?;

    let start_month = day_month
        .next()
        .ok_or(generic_parse_error!("".into()))?
        .parse::<u32>()
        .map_err(|_| generic_parse_error!("".into()))?;

    let monday = NaiveDate::from_ymd_opt(start_year, start_month, start_day)
        .ok_or(generic_parse_error!("".into()))?;

    let mut events = Vec::new();
    for row in element.select(selector!("tr")).skip(1) {
        let mut day_index = 0;

        for column in row.select(selector!("td")) {
            let class = column
                .value()
                .classes()
                .next()
                .ok_or(generic_parse_error!("".into()))?;

            if class.starts_with("week_separatorcell") {
                day_index += 1;
            }

            if class != "week_block" {
                continue;
            }

            let date =
                monday + Duration::try_days(day_index).ok_or(generic_parse_error!("".into()))?;
            events.push(parse_event_details(column, date)?);
        }
    }

    Ok(events)
}

fn parse_event_details(element: ElementRef, date: NaiveDate) -> Result<Event, ParseError> {
    let details = select_first!(element, "a")?.inner_html();
    let mut details_split = details.split("<br>");

    let times_raw = details_split
        .next()
        .ok_or(generic_parse_error!("".into()))?;
    let mut times_raw_split = times_raw.split("&nbsp;-");

    let start = NaiveTime::parse_from_str(
        times_raw_split
            .next()
            .ok_or(generic_parse_error!("".into()))?,
        "%H:%M",
    )
    .map_err(|_| generic_parse_error!("".into()))?;
    let end = NaiveTime::parse_from_str(
        times_raw_split
            .next()
            .ok_or(generic_parse_error!("".into()))?,
        "%H:%M",
    )
    .map_err(|_| generic_parse_error!("".into()))?;

    let title = details_split
        .next()
        .ok_or(generic_parse_error!("".into()))?
        .replace("&amp;", "&");

    let resources = element
        .select(selector!("span.resource"))
        .map(|location| location.inner_html())
        .collect::<Vec<_>>();

    let location = resources.last().cloned();

    let persons = element
        .select(selector!("span.person"))
        .map(|person| person.inner_html())
        .collect::<Vec<_>>();

    let organizer = persons.is_empty().not().then(|| persons.join(", "));
    let description = resources.is_empty().not().then(|| resources.join(", "));

    Ok(Event {
        date,
        start,
        end,
        title,
        location,
        organizer,
        description,
    })
}
