use std::collections::BTreeMap;
use std::fmt::Write;

use chrono::{DateTime, Duration, Offset, Utc};
use chrono_tz::{OffsetComponents, Tz};
use icalendar::{CalendarComponent, Component};

use crate::features::event::model::{Event, EventTime};

pub(super) fn export_zones(events: &[Event]) -> impl Iterator<Item = String> {
    let mut zones = BTreeMap::<_, (i32, i32)>::new();
    for event in events {
        let (Some(name), EventTime::Timed { start, end }) = (&event.timezone, &event.time) else {
            continue;
        };
        let Ok(zone) = name.parse::<Tz>() else {
            continue;
        };
        let first = start.year();
        let last = if event.is_recurring() {
            end.year().max(2100)
        } else {
            end.year()
        };
        zones
            .entry(zone.name())
            .and_modify(|years| {
                years.0 = years.0.min(first);
                years.1 = years.1.max(last);
            })
            .or_insert((first, last));
    }

    zones.into_iter().filter_map(|(name, (first, last))| {
        let zone = name.parse::<Tz>().ok()?;
        export_zone(zone, first, last)
    })
}

fn january_first(year: i32) -> Option<DateTime<Utc>> {
    Some(
        chrono::NaiveDate::from_ymd_opt(year, 1, 1)?
            .and_hms_opt(0, 0, 0)?
            .and_utc(),
    )
}

fn export_zone(zone: Tz, first: i32, last: i32) -> Option<String> {
    let mut component = format!("BEGIN:VTIMEZONE\r\nTZID:{}\r\n", zone.name());
    let initial = january_first(first - 1)?;
    let first = january_first(first.clamp(1800, 2100) - 1)?.max(initial);
    let last = january_first(last.clamp(1800, 2100) + 1)?;
    let mut sample = first;
    let mut previous = *sample.with_timezone(&zone).offset();
    let initial_offset = *initial.with_timezone(&zone).offset();
    write_observance(
        &mut component,
        initial,
        initial_offset.fix().local_minus_utc(),
        initial_offset,
    );
    while sample < last {
        let next = (sample + Duration::hours(12)).min(last);
        let offset = *next.with_timezone(&zone).offset();
        if offset != previous {
            let mut low = sample.timestamp();
            let mut high = next.timestamp();
            while high - low > 1 {
                let middle = low + (high - low) / 2;
                let instant = DateTime::from_timestamp(middle, 0)?;
                if *instant.with_timezone(&zone).offset() == previous {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let transition = DateTime::from_timestamp(high, 0)?;
            write_observance(
                &mut component,
                transition,
                previous.fix().local_minus_utc(),
                offset,
            );
            previous = offset;
        }
        sample = next;
    }
    component.push_str("END:VTIMEZONE\r\n");
    Some(component)
}

fn write_observance(
    component: &mut String,
    instant: DateTime<Utc>,
    before: i32,
    after: chrono_tz::TzOffset,
) {
    let kind = if after.dst_offset().is_zero() {
        "STANDARD"
    } else {
        "DAYLIGHT"
    };
    let local = (instant + Duration::seconds(i64::from(before))).naive_utc();
    write!(
        component,
        "BEGIN:{kind}\r\nDTSTART:{}\r\nTZOFFSETFROM:{}\r\nTZOFFSETTO:{}\r\nEND:{kind}\r\n",
        local.format("%Y%m%dT%H%M%S"),
        format_offset(before),
        format_offset(after.fix().local_minus_utc()),
    )
    .expect("writing to a String cannot fail");
}

fn format_offset(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let seconds = seconds.unsigned_abs();
    let base = format!("{sign}{:02}{:02}", seconds / 3600, seconds / 60 % 60);
    if seconds.is_multiple_of(60) {
        base
    } else {
        format!("{base}{:02}", seconds % 60)
    }
}

pub(super) fn import_aliases(components: &[CalendarComponent]) -> BTreeMap<String, Tz> {
    components
        .iter()
        .filter_map(|component| {
            let CalendarComponent::Other(zone) = component else {
                return None;
            };
            if zone.component_kind() != "VTIMEZONE" {
                return None;
            }
            let name = zone.property_value("TZID")?;
            let location = zone.property_value("X-LIC-LOCATION")?;
            Some((name.to_string(), location.parse().ok()?))
        })
        .collect()
}
