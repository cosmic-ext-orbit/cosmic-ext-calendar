//! Desktop alert delivery shared by the Calendar window and the session worker.

use std::collections::HashSet;
use std::fs::{self, File, TryLockError};
use std::io;
use std::path::Path;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime, UtcOffset};

use crate::features::calendar::display_time;
use crate::features::calendar_management::model::CalendarKey;
use crate::features::event::model::{Event, EventTime};
use crate::fl;
use crate::shared::ids::EventId;
use crate::shared::storage::{data_dir, write_json_atomic, LocalStorage};

#[derive(Debug, Clone)]
pub enum NotificationMessage {
    CheckAlerts,
    AlertsChecked,
}

pub const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const GRACE: Duration = Duration::minutes(5);
const MAX_CATCH_UP: Duration = Duration::DAY;

type AlertKey = (EventId, OffsetDateTime, i64);

#[derive(Default, Deserialize, Serialize)]
struct DeliveryHistory {
    last_checked: Option<OffsetDateTime>,
    delivered: Vec<Delivery>,
}

#[derive(Deserialize, Serialize)]
struct Delivery {
    key: AlertKey,
    checked_at: OffsetDateTime,
}

impl DeliveryHistory {
    fn load(path: &Path) -> crate::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    fn since(&self, now: OffsetDateTime) -> OffsetDateTime {
        let grace_start = now.saturating_sub(GRACE);
        self.last_checked
            .unwrap_or(grace_start)
            .min(grace_start)
            .max(now.saturating_sub(MAX_CATCH_UP))
    }

    fn deliver<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a Event>,
        now: OffsetDateTime,
        mut send: impl FnMut(&DueAlert) -> bool,
    ) {
        let retention_start = now.saturating_sub(MAX_CATCH_UP);
        self.delivered
            .retain(|delivery| delivery.checked_at >= retention_start);
        let already_delivered = self.delivered.iter().map(|delivery| delivery.key).collect();
        let since = self.since(now);
        let alerts = due_alerts(events, since, now, already_delivered);
        let mut failed = false;
        for alert in alerts {
            if send(&alert) {
                self.delivered.push(Delivery {
                    key: alert.key,
                    checked_at: now,
                });
            } else {
                failed = true;
            }
        }
        self.last_checked = Some(if failed { since } else { now });
    }
}

struct DueAlert {
    key: AlertKey,
    summary: String,
    start: OffsetDateTime,
    trigger: OffsetDateTime,
    all_day: bool,
}

pub fn check_alerts(remote_events: &[Event]) -> crate::Result<()> {
    let root = data_dir()?;
    if !root.try_exists()? {
        return Ok(());
    }
    let lock = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("notification-delivery.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(()),
        Err(TryLockError::Error(err)) => return Err(err.into()),
    }

    let storage = LocalStorage::load()?;
    let path = root.join("notification-delivery.json");
    let mut history = DeliveryHistory::load(&path)?;
    let previous_checkpoint = history.last_checked;
    let local_events = storage.all_events().into_iter()
        .filter(|event| matches!(event.calendar, CalendarKey::Local(_)));
    history.deliver(local_events.chain(remote_events), OffsetDateTime::now_utc(), notify);
    if storage.load_error().is_some() {
        history.last_checked = previous_checkpoint;
    }
    write_json_atomic(&path, &history)
}

pub fn run_worker() -> crate::Result<()> {
    let root = data_dir()?;
    fs::create_dir_all(&root)?;
    let lock = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("notification-worker.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(()),
        Err(TryLockError::Error(err)) => return Err(err.into()),
    }
    loop {
        if let Err(err) = check_alerts(&[]) {
            tracing::warn!("could not check event alerts: {err}");
        }
        std::thread::sleep(CHECK_INTERVAL);
    }
}

fn due_alerts<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    since: OffsetDateTime,
    now: OffsetDateTime,
    mut seen: HashSet<AlertKey>,
) -> Vec<DueAlert> {
    let mut alerts = Vec::new();
    for event in events {
        let all_day = matches!(event.time, EventTime::AllDay { .. });
        let padding = if event.floating || all_day {
            Duration::DAY
        } else {
            Duration::ZERO
        };
        for reminder in &event.reminders {
            let minutes = reminder.minutes_before;
            let Some(seconds) = minutes.checked_mul(60).filter(|seconds| *seconds >= 0) else {
                continue;
            };
            let lead = Duration::seconds(seconds);
            let window = since.saturating_add(lead).saturating_sub(padding)
                ..now
                    .saturating_add(lead)
                    .saturating_add(padding + Duration::NANOSECOND);
            for occurrence in event.occurrences(window) {
                let Some(start) = notification_time(event, occurrence, all_day) else {
                    continue;
                };
                let trigger = start.saturating_sub(lead);
                let key = (event.id, occurrence, minutes);
                if (since..=now).contains(&trigger) && seen.insert(key) {
                    alerts.push(DueAlert {
                        key,
                        summary: event.summary.clone(),
                        start,
                        trigger,
                        all_day,
                    });
                }
            }
        }
    }
    alerts.sort_by_key(|alert| alert.trigger);
    alerts
}

fn notification_time(
    event: &Event,
    occurrence: OffsetDateTime,
    all_day: bool,
) -> Option<OffsetDateTime> {
    if !event.floating && !all_day {
        return Some(display_time(occurrence, None));
    }

    let wall =
        chrono::DateTime::from_timestamp(occurrence.unix_timestamp(), occurrence.nanosecond())?
            .naive_utc();
    let offset = chrono::Local
        .from_local_datetime(&wall)
        .earliest()?
        .offset()
        .local_minus_utc();
    Some(occurrence.replace_offset(UtcOffset::from_whole_seconds(offset).ok()?))
}

fn notify(alert: &DueAlert) -> bool {
    let start = alert.start;
    let date = crate::features::calendar::month::agenda_date(start.date());
    let body = if alert.all_day {
        format!("{} · {date}", fl!("all-day"))
    } else {
        format!("{:02}:{:02} · {date}", start.hour(), start.minute())
    };
    let summary = if alert.summary.trim().is_empty() {
        fl!("notification-untitled-event")
    } else {
        alert.summary.clone()
    };
    let mut notification = notify_rust::Notification::new();
    notification
        .appname(&fl!("app-title"))
        .summary(&summary)
        .body(&body)
        .icon("office-calendar-symbolic");

    #[cfg(all(unix, not(target_os = "macos")))]
    notification.hint(notify_rust::Hint::DesktopEntry(
        "dev.edfloreshz.Calendar".into(),
    ));

    match notification.show() {
        Ok(_) => true,
        Err(err) => {
            tracing::warn!("failed to send event alert: {err}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::calendar_management::model::CalendarKey;
    use crate::features::event::model::Reminder;
    use crate::features::event::state::EventEditorState;
    use crate::shared::ids::CalendarId;

    fn event(start: OffsetDateTime, lead_times: &[i64]) -> Event {
        let mut event = EventEditorState::new(
            CalendarKey::Local(CalendarId::new()),
            start,
            false,
            String::new(),
        )
        .build_event(None);
        event.reminders = lead_times
            .iter()
            .map(|&minutes_before| Reminder { minutes_before })
            .collect();
        event
    }

    #[test]
    fn recurrence_and_multiple_alerts_are_delivered_at_their_own_times() {
        let first = OffsetDateTime::UNIX_EPOCH + Duration::hours(12);
        let mut recurring = event(first, &[0, 15, 15]);
        recurring.rrule = Some("FREQ=DAILY;COUNT=3".into());
        let occurrence = first + Duration::DAY;
        let alerts = due_alerts(
            [&recurring],
            occurrence - Duration::minutes(15),
            occurrence,
            HashSet::new(),
        );
        assert_eq!(
            alerts
                .iter()
                .map(|alert| (alert.key.1, alert.key.2))
                .collect::<Vec<_>>(),
            [(occurrence, 15), (occurrence, 0)]
        );
    }

    #[test]
    fn checkpoint_catches_up_and_persisted_successes_are_not_repeated() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(2);
        let item = event(now - Duration::minutes(10), &[0, 15]);
        let expired = event(now - MAX_CATCH_UP - Duration::seconds(1), &[0]);
        let mut history = DeliveryHistory {
            last_checked: Some(now - Duration::days(2)),
            ..DeliveryHistory::default()
        };
        let mut first_pass = Vec::new();
        history.deliver([&item, &expired], now, |alert| {
            first_pass.push(alert.key.2);
            alert.key.2 == 15
        });
        assert_eq!(first_pass, [15, 0]);

        let bytes = serde_json::to_vec(&history).unwrap();
        let mut restarted: DeliveryHistory = serde_json::from_slice(&bytes).unwrap();
        let mut retried = Vec::new();
        restarted.deliver([&item], now + Duration::seconds(30), |alert| {
            retried.push(alert.key.2);
            true
        });
        assert_eq!(retried, [0]);

        restarted.deliver([&item], now + Duration::minutes(1), |_| {
            panic!("a delivered alert was sent again")
        });
    }
}
