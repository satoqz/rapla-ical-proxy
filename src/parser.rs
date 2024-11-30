use std::ops::Not;

use chrono::{Duration, NaiveDate, NaiveTime};
use once_cell::sync::Lazy;
use scraper::{ElementRef, Html, Selector};

use crate::structs::{Calendar, Event};

macro_rules! selector {
    ($query:expr) => {{
        static SELECTOR: Lazy<Selector> = Lazy::new(|| Selector::parse($query).unwrap());
        &SELECTOR
    }};
}

pub fn parse_calendar(s: &str, mut start_year: i32) -> Option<Calendar> {
    let html = Html::parse_document(s);
    let name = html
        .select(selector!("title"))
        .next()?
        .inner_html()
        .trim()
        .to_string();

    eprintln!("got to 1");

    let mut events = Vec::new();

    for (idx, week_element) in html
        .select(selector!("div.calendar > table.week_table > tbody"))
        .enumerate()
    {
        let week_number = week_element
            .select(selector!("th.week_number"))
            .next()?
            .inner_html()
            .split(' ')
            .nth(1)?
            .parse::<usize>()
            .ok()?;

        if week_number == 1 && idx > 0 {
            start_year += 1;
        }

        let mut week_events = parse_week(week_element, start_year)?;

        events.append(&mut week_events);
    }

    Some(Calendar { name, events })
}

fn parse_week(element: ElementRef, start_year: i32) -> Option<Vec<Event>> {
    let start_date_raw = element
        .select(selector!("tr > td.week_header > nobr"))
        .next()?
        .inner_html();

    let mut day_month = start_date_raw
        .split(' ')
        .nth(1)?
        .trim_end_matches('.')
        .split('.');

    let start_day = day_month.next()?.parse::<u32>().ok()?;
    let start_month = day_month.next()?.parse::<u32>().ok()?;

    let monday = NaiveDate::from_ymd_opt(start_year, start_month, start_day)?;

    let mut events = Vec::new();

    for row in element.select(selector!("tr")).skip(1) {
        let mut day_index = 0;

        for column in row.select(selector!("td")) {
            let class = column.value().classes().next()?;

            if class.starts_with("week_separatorcell") {
                day_index += 1;
            }

            if class != "week_block" {
                continue;
            }

            let date = monday + Duration::try_days(day_index)?;
            events.push(parse_event_details(column, date)?);
        }
    }

    Some(events)
}

fn parse_event_details(element: ElementRef, date: NaiveDate) -> Option<Event> {
    let details = element.select(selector!("a")).next()?.inner_html();
    let mut details_split = details.split("<br>");

    let times_raw = details_split.next()?;
    let mut times_raw_split = times_raw.split("&nbsp;-");

    let start = NaiveTime::parse_from_str(times_raw_split.next()?, "%H:%M").ok()?;
    let end = NaiveTime::parse_from_str(times_raw_split.next()?, "%H:%M").ok()?;

    let title = details_split.next()?.replace("&amp;", "&");

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
