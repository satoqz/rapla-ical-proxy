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

macro_rules! parse_error {
    ($($arg:tt)*) => {
        ParseError {
            location: source_url!(),
            kind: ParseErrorKind::Generic(format!($($arg)*)),
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
            .ok_or_else(|| {
                parse_error!(
                    "malformed calendar week number in week #{}: missing second element after splitting by space",
                    idx + 1
                )
            })?
            .parse::<usize>()
            .map_err(|err| parse_error!("malformed calendar week number in week #{}: {err}", idx + 1))?;

        if week_number == 1 && idx > 0 {
            start_year += 1;
        }

        let mut week_events = parse_week(week_element, start_year)?;
        events.append(&mut week_events);
    }

    Ok(Calendar { name, events })
}

fn parse_week(element: ElementRef, start_year: i32) -> Result<Vec<Event>, ParseError> {
    let week_header = select_first!(element, "tr > td.week_header > nobr")?.inner_html();

    let day_month = week_header
        .split(' ')
        .nth(1)
        .ok_or_else(|| {
            parse_error!("failed to find day and month in week header '{week_header}': missing second element after splitting by space")
        })?
        .trim_end_matches('.')
        .split('.')
        .collect::<Vec<_>>();

    if day_month.len() != 2 {
        return Err(parse_error!(
            "expected day + month information in week header '{week_header}' to consist of two elements when splitting by dots")
        );
    }

    let start_day = day_month[0]
        .parse::<u32>()
        .map_err(|err| parse_error!("failed to parse day in week header '{week_header}': {err}"))?;
    let start_month = day_month[1].parse::<u32>().map_err(|err| {
        parse_error!("failed to parse month in week header '{week_header}': {err}")
    })?;
    let monday = NaiveDate::from_ymd_opt(start_year, start_month, start_day).ok_or(
        parse_error!("week start date '{start_day}.{start_month}.{start_year}' derived from week header '{week_header}' appears to be an invalid date"),
    )?;

    let mut events = Vec::new();
    for row in element.select(selector!("tr")).skip(1) {
        let mut day_index = 0;

        for column in row.select(selector!("td")) {
            match column.value().classes().next() {
                Some(class) if class.starts_with("week_separatorcell") => day_index += 1,
                Some(class) if class != "week_block" => continue,
                _ => {}
            }

            let date = monday + Duration::try_days(day_index).ok_or(parse_error!(""))?;
            events.push(parse_event_details(column, date)?);
        }
    }

    Ok(events)
}

fn parse_event_details(element: ElementRef, date: NaiveDate) -> Result<Event, ParseError> {
    let details = select_first!(element, "a")?.inner_html();
    let mut details_split = details.split("<br>");

    let times_raw = details_split.next().ok_or(parse_error!(""))?;
    let mut times_raw_split = times_raw.split("&nbsp;-");

    let start = NaiveTime::parse_from_str(times_raw_split.next().ok_or(parse_error!(""))?, "%H:%M")
        .map_err(|_| parse_error!(""))?;
    let end = NaiveTime::parse_from_str(times_raw_split.next().ok_or(parse_error!(""))?, "%H:%M")
        .map_err(|_| parse_error!(""))?;

    let title = details_split
        .next()
        .ok_or(parse_error!(""))?
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
