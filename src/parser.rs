use std::fmt::{self, Display};
use std::ops::Not;

use chrono::{Duration, NaiveDate, NaiveTime, Timelike};
use html_escape::decode_html_entities;
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
                "query '{query}' didn't return any elements (source location: {})",
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
            parse_error!("couldn't find day and month in week header '{week_header}': missing second element after splitting by space")
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
        .map_err(|err| parse_error!("couldn't parse day in week header '{week_header}': {err}"))?;
    let start_month = day_month[1].parse::<u32>().map_err(|err| {
        parse_error!("couldn't parse month in week header '{week_header}': {err}")
    })?;
    let monday = NaiveDate::from_ymd_opt(start_year, start_month, start_day).ok_or(
        parse_error!("week start date '{start_day}.{start_month}.{start_year}' derived from week header '{week_header}' appears to be an invalid date"),
    )?;

    let mut events = Vec::new();
    for row in element.select(selector!("tr")).skip(1) {
        let mut day_index = 0;

        for column in row.select(selector!("td")) {
            let class = column
                .value()
                .classes()
                .next()
                .ok_or(parse_error!("expected 'td' element to have a class"))?;

            if class.starts_with("week_separatorcell") {
                day_index += 1;
            }
            if class != "week_block" {
                continue;
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

    let times_raw = details_split.next().ok_or(parse_error!(
        "couldn't find time range in event details '{details}'"
    ))?;
    let mut times_raw_split = times_raw.split("&nbsp;-");

    let mut start = NaiveTime::parse_from_str(
        times_raw_split
            .next()
            .ok_or(parse_error!("missing event start time in '{times_raw}'"))?,
        "%H:%M",
    )
    .map_err(|err| parse_error!("couldn't parse event start time from '{times_raw}': {err}"))?;

    let end_time_raw = times_raw_split
        .next()
        .ok_or(parse_error!("missing event end time in '{times_raw}'"))?;
    // Some genuises at DHBW find it a great idea to leave out the end time
    // to signify "full day" which is to be interpreted as "until 18:00".
    // THEY EVEN KEEP THE DASH AFTER THE START TIME AS BAIT :(
    let end = if end_time_raw.is_empty() {
        // The sheer idea of the above irritates me to much that I'll unwrap here
        NaiveTime::from_hms_opt(18, 0, 0).unwrap()
    } else {
        NaiveTime::parse_from_str(end_time_raw, "%H:%M").map_err(|err| {
            parse_error!("couldn't parse event end time from '{times_raw}': {err}")
        })?
    };
    // Also, they will set the start time to 00:00 when it's actually supposed to be 08:00.
    // At least that's how it's displayed on the website.
    if start.hour() == 0 && start.minute() == 0 {
        // Grr.
        start = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    }

    let title = details_split
        .next()
        .ok_or_else(|| parse_error!("couldn't find event title in '{details}'"))?;
    let title = decode_html_entities(title).to_string();

    let resources = element
        .select(selector!("span.resource"))
        .map(|location| decode_html_entities(&location.inner_html()).to_string())
        .collect::<Vec<_>>();
    let location = resources.last().cloned();
    let description = resources.is_empty().not().then(|| resources.join(", "));

    let persons = element
        .select(selector!("span.person"))
        .map(|person| decode_html_entities(&person.inner_html()).to_string())
        .collect::<Vec<_>>();
    let organizer = persons.is_empty().not().then(|| persons.join(", "));

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
