// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;
use std::time::Duration;

use accounts::Service;
use reqwest::{Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize};
use time::{Date, Duration as TimeDuration, OffsetDateTime};
use uuid::Uuid;

use super::{Account, Calendar, Error, Snapshot, manager};
use crate::features::{
    calendar_management::model::{CalendarKey, Source},
    event::model::{Event, EventKind, EventTime, ExternalRef, Reminder},
};
use crate::shared::ids::EventId;

const GRAPH: &str = "https://graph.microsoft.com/v1.0/";
const MAX_PAGE_BYTES: usize = 16 * 1024 * 1024;

pub(super) async fn sync(account: &Account) -> Result<Snapshot, Error> {
    let manager = manager().await?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| Error::Failed)?;
    let now = OffsetDateTime::now_utc();
    for refresh in [false, true] {
        let token = manager
            .access_token(account.id, Service::Calendar, refresh)
            .await?;
        let result = fetch(&client, &token, account, now).await;
        if !refresh && matches!(result, Err(Error::NeedsReauth)) {
            continue;
        }
        return result;
    }
    unreachable!("the refreshed token attempt returns its result")
}

#[derive(Deserialize)]
struct Page<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    next: Option<String>,
}

#[derive(Deserialize)]
struct GraphCalendar {
    id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDateTime {
    date_time: String,
    time_zone: String,
}

impl GraphDateTime {
    fn date(&self) -> Result<Date, Error> {
        use chrono::Datelike;

        let date = self.date_time.get(..10).ok_or(Error::Failed)?;
        let date =
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| Error::Failed)?;
        let month = time::Month::try_from(date.month() as u8).map_err(|_| Error::Failed)?;
        Date::from_calendar_date(date.year(), month, date.day() as u8).map_err(|_| Error::Failed)
    }

    fn in_utc(&self) -> Result<OffsetDateTime, Error> {
        // Graph returns UTC wall times without an offset when UTC is requested.
        let parsed = match chrono::DateTime::parse_from_rfc3339(&self.date_time) {
            Ok(date) => date.to_utc(),
            Err(_) if self.time_zone.eq_ignore_ascii_case("UTC") => {
                chrono::NaiveDateTime::parse_from_str(&self.date_time, "%Y-%m-%dT%H:%M:%S%.f")
                    .map_err(|_| Error::Failed)?
                    .and_utc()
            }
            Err(_) => return Err(Error::Failed),
        };
        let nanos = i128::from(parsed.timestamp()) * 1_000_000_000
            + i128::from(parsed.timestamp_subsec_nanos());
        OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|_| Error::Failed)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphEvent {
    id: String,
    subject: Option<String>,
    body_preview: Option<String>,
    location: Option<GraphLocation>,
    start: Option<GraphDateTime>,
    end: Option<GraphDateTime>,
    #[serde(default)]
    is_all_day: bool,
    #[serde(default)]
    is_cancelled: bool,
    #[serde(default)]
    is_reminder_on: bool,
    reminder_minutes_before_start: Option<i64>,
    i_cal_u_id: Option<String>,
    #[serde(rename = "@odata.etag")]
    etag: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphLocation {
    display_name: String,
}

async fn fetch(
    client: &Client,
    token: &str,
    account: &Account,
    now: OffsetDateTime,
) -> Result<Snapshot, Error> {
    let base = Url::parse(GRAPH).expect("valid Graph base URL");
    let calendars_url = base.join("me/calendars").expect("valid calendar list URL");
    let calendars: Vec<GraphCalendar> = pages(client, token, &base, calendars_url).await?;
    let mut snapshot = Snapshot::default();
    let mut seen_calendars = HashSet::new();

    // calendarView expands series and returns exceptions within a bounded range.
    let start = (now - TimeDuration::days(365)).date();
    let end = (now + TimeDuration::days(366)).date();
    for calendar in calendars {
        if calendar.id.is_empty() || !seen_calendars.insert(calendar.id.clone()) {
            continue;
        }
        let mut url = base.join("me/calendars/").expect("valid calendar base URL");
        url.path_segments_mut()
            .expect("Graph URL supports path segments")
            .pop_if_empty()
            .push(&calendar.id)
            .push("calendarView");
        url.query_pairs_mut()
            .append_pair("startDateTime", &format!("{start}T00:00:00Z"))
            .append_pair("endDateTime", &format!("{end}T00:00:00Z"))
            .append_pair("$top", "100")
            .append_pair("$select", "id,subject,bodyPreview,location,start,end,isAllDay,isCancelled,isReminderOn,reminderMinutesBeforeStart,iCalUId");
        let events: Vec<GraphEvent> = pages(client, token, &base, url).await?;
        let mut seen_events = HashSet::new();
        for event in events {
            if !event.is_cancelled && seen_events.insert(event.id.clone()) {
                snapshot
                    .events
                    .push(event.into_event(account, &calendar.id, now)?);
            }
        }
        snapshot.calendars.push(Calendar {
            id: calendar.id,
            name: calendar.name,
            sync_token: None,
        });
    }
    snapshot
        .calendars
        .sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    Ok(snapshot)
}

async fn pages<T: DeserializeOwned>(
    client: &Client,
    token: &str,
    base: &Url,
    mut url: Url,
) -> Result<Vec<T>, Error> {
    let mut entries = Vec::new();
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(url.clone()) {
            return Err(Error::Failed);
        }
        let mut response = client
            .get(url)
            .bearer_auth(token)
            .header("Prefer", "outlook.timezone=\"UTC\", IdType=\"ImmutableId\"")
            .send()
            .await
            .map_err(|_| Error::Failed)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => return Err(Error::NeedsReauth),
            StatusCode::FORBIDDEN => return Err(Error::MicrosoftAccessDenied),
            _ => return Err(Error::Failed),
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Failed)? {
            if bytes.len() + chunk.len() > MAX_PAGE_BYTES {
                return Err(Error::Failed);
            }
            bytes.extend_from_slice(&chunk);
        }
        let page: Page<T> = serde_json::from_slice(&bytes).map_err(|_| Error::Failed)?;
        entries.extend(page.value);
        let Some(next) = page.next else {
            return Ok(entries);
        };
        url = next_page(base, &next)?;
    }
}

fn next_page(base: &Url, next: &str) -> Result<Url, Error> {
    let url = Url::parse(next).map_err(|_| Error::Failed)?;
    if url.origin() != base.origin()
        || !url.path().starts_with(base.path())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Failed);
    }
    Ok(url)
}

impl GraphEvent {
    fn into_event(
        self,
        account: &Account,
        calendar_id: &str,
        now: OffsetDateTime,
    ) -> Result<Event, Error> {
        if self.id.is_empty() {
            return Err(Error::Failed);
        }
        let start = self.start.ok_or(Error::Failed)?;
        let end = self.end.ok_or(Error::Failed)?;
        let time = if self.is_all_day {
            // Keep the calendar dates Graph supplies. An offset conversion
            // can move midnight to the preceding day.
            let (start, end) = (start.date()?, end.date()?);
            if end <= start {
                return Err(Error::Failed);
            }
            EventTime::AllDay { start, end }
        } else {
            let (start, end) = (start.in_utc()?, end.in_utc()?);
            if end < start {
                return Err(Error::Failed);
            }
            EventTime::Timed { start, end }
        };
        let id = EventId(Uuid::new_v5(
            &account.id,
            format!("{calendar_id}\n{}", self.id).as_bytes(),
        ));
        Ok(Event {
            id,
            calendar: CalendarKey::Remote {
                account_id: account.id,
                calendar_id: calendar_id.to_owned(),
            },
            kind: EventKind::Event,
            summary: self.subject.unwrap_or_default(),
            description: self.body_preview.filter(|text| !text.is_empty()),
            location: self
                .location
                .map(|location| location.display_name)
                .filter(|text| !text.is_empty()),
            time,
            timezone: None,
            floating: false,
            color: None,
            rrule: None,
            repeat_before: None,
            predecessor: None,
            split_before: None,
            exdates: Vec::new(),
            rdates: Vec::new(),
            recurrence_id: None,
            series_id: None,
            ics_uid: self.i_cal_u_id,
            attendees: Vec::new(),
            reminders: self
                .reminder_minutes_before_start
                .filter(|minutes| self.is_reminder_on && *minutes >= 0)
                .map(|minutes_before| Reminder { minutes_before })
                .into_iter()
                .collect(),
            source: Source::Synced {
                provider: account.provider.clone(),
                account_id: account.id,
            },
            external: Some(ExternalRef {
                account_id: account.id,
                provider: account.provider.clone(),
                calendar_id: calendar_id.to_owned(),
                provider_event_id: self.id,
                etag: self.etag,
                last_synced: Some(now),
            }),
            created_at: now,
            updated_at: now,
            sequence: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    #[test]
    fn only_follows_graph_paging_links() {
        let base = Url::parse(GRAPH).unwrap();
        assert!(next_page(
            &base,
            "https://graph.microsoft.com/v1.0/me/calendars?$skiptoken=x"
        )
        .is_ok());
        for url in [
            "https://other.example/v1.0/me/calendars",
            "http://graph.microsoft.com/v1.0/me/calendars",
            "https://graph.microsoft.com/beta/me/calendars",
            "https://user@graph.microsoft.com/v1.0/me/calendars",
        ] {
            assert!(next_page(&base, url).is_err(), "{url}");
        }
    }

    #[test]
    fn imports_graph_occurrences_and_preserves_remote_identity() {
        let account = Account {
            id: Uuid::new_v4(),
            email: None,
            provider: "microsoft".into(),
            server: None,
        };
        let payload = br#"{
            "value": [
                {
                    "id": "instance",
                    "subject": "Weekly meeting",
                    "start": {"dateTime":"2026-06-01T13:15:00.0000000","timeZone":"UTC"},
                    "end": {"dateTime":"2026-06-01T14:15:00.0000000","timeZone":"UTC"},
                    "isReminderOn": true,
                    "reminderMinutesBeforeStart": 5
                },
                {
                    "id": "all-day",
                    "start": {"dateTime":"2026-06-01T00:00:00","timeZone":"UTC"},
                    "end": {"dateTime":"2026-06-03T00:00:00","timeZone":"UTC"},
                    "isAllDay": true
                }
            ],
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/calendarView?$skiptoken=x"
        }"#;
        let first = Date::from_calendar_date(2026, Month::June, 1).unwrap();
        let now = first.midnight().assume_utc();
        let parse = |calendar| {
            let page: Page<GraphEvent> = serde_json::from_slice(payload).unwrap();
            assert!(page.next.is_some());
            page.value
                .into_iter()
                .map(|event| event.into_event(&account, calendar, now).unwrap())
                .collect::<Vec<_>>()
        };
        let events = parse("primary");
        assert_eq!(events[0].id, parse("primary")[0].id);
        assert_ne!(events[0].id, parse("other")[0].id);
        assert!(matches!(events[0].time, EventTime::Timed { start, end }
            if start == now + TimeDuration::minutes(795) && end == start + TimeDuration::hours(1)));
        assert_eq!(events[0].reminders[0].minutes_before, 5);
        assert!(matches!(events[1].time, EventTime::AllDay { start, end }
            if start == first && end == first + TimeDuration::days(2)));
    }
}
