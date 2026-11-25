//! The intentional `time` <-> `chrono` conversion boundary.
//!
//! The rest of the app uses the `time` crate. `rrule` (recurrence) and
//! `icalendar` (import/export, phase 4) are both `chrono`-based, so any
//! conversion between the two date/time libraries should happen only here.

use chrono::{DateTime, Offset, Utc};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};
use time::{Duration, OffsetDateTime, UtcOffset};

use super::model::{Event, EventTime};

/// Maximum occurrences generated per query, guarding against unbounded
/// RRULEs (e.g. no COUNT/UNTIL) combined with a huge query range.
const MAX_OCCURRENCES: u16 = 512;

fn to_chrono(dt: OffsetDateTime, zone: Tz) -> DateTime<Tz> {
    let utc = DateTime::<Utc>::from_timestamp(dt.unix_timestamp(), dt.nanosecond())
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
    utc.with_timezone(&zone)
}

fn from_chrono(dt: DateTime<Tz>) -> OffsetDateTime {
    let offset = UtcOffset::from_whole_seconds(dt.offset().fix().local_minus_utc())
        .unwrap_or(UtcOffset::UTC);
    (OffsetDateTime::from_unix_timestamp(dt.timestamp()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + Duration::nanoseconds(i64::from(dt.timestamp_subsec_nanos())))
    .to_offset(offset)
}

/// Follow split links to find the series that now owns an original slot.
pub(crate) fn resolve_series_occurrence<'a>(
    mut series: crate::shared::ids::EventId,
    mut occurrence: OffsetDateTime,
    events: impl Iterator<Item = &'a Event> + Clone,
) -> (crate::shared::ids::EventId, OffsetDateTime) {
    let mut visited = std::collections::HashSet::new();
    while visited.insert(series) {
        let Some(parent) = events.clone().find(|event| event.id == series) else {
            break;
        };
        let successor = events
            .clone()
            .filter_map(|event| {
                let (predecessor, split) = event.predecessor?;
                (predecessor == series && split <= occurrence).then_some((event, split))
            })
            .min_by_key(|(_, split)| *split);
        let Some((successor, split)) = successor else {
            break;
        };
        let Some(rebased) = successor.rebase_occurrence(parent, split, occurrence) else {
            break;
        };
        series = successor.id;
        occurrence = rebased;
    }
    (series, occurrence)
}

impl Event {
    fn rule_for_expansion<'a>(&self, rule: &'a str) -> std::borrow::Cow<'a, str> {
        if !self.floating && !matches!(self.time, EventTime::AllDay { .. }) {
            return std::borrow::Cow::Borrowed(rule);
        }
        let fields = rule
            .split(';')
            .map(|field| match field.strip_prefix("UNTIL=") {
                Some(date) if date.len() == 8 => format!("UNTIL={date}T235959Z"),
                Some(date) if date.len() == 15 => format!("UNTIL={date}Z"),
                _ => field.to_string(),
            })
            .collect::<Vec<_>>();
        std::borrow::Cow::Owned(fields.join(";"))
    }

    pub fn is_recurring(&self) -> bool {
        self.rrule.is_some() || !self.rdates.is_empty()
    }

    /// Returns the occurrence start times of this event that fall within
    /// `range`. Non-recurring events yield at most their own start time;
    /// recurring events (`rrule.is_some()`) are expanded via the `rrule`
    /// crate, honoring `exdates`.
    pub fn occurrences(&self, range: std::ops::Range<OffsetDateTime>) -> Vec<OffsetDateTime> {
        let start = self.start();
        let end = self
            .recurrence_cutoff()
            .map_or(range.end, |end| range.end.min(end));
        let range = range.start..end;
        if range.is_empty() {
            return Vec::new();
        }

        let zone = self.recurrence_zone();
        let to_rule_time = |date| to_chrono(date, zone);
        let dt_start = to_rule_time(start);
        let mut dates: Vec<_> = self
            .rdates
            .iter()
            .copied()
            .filter(|date| range.contains(date))
            .collect();

        if (self.rrule.is_none() || !self.rdates.is_empty()) && range.contains(&start) {
            dates.push(start);
        }
        if let Some(rrule_str) = self.rrule.as_deref() {
            let rule = self
                .rule_for_expansion(rrule_str)
                .parse::<RRule<Unvalidated>>()
                .and_then(|rule| rule.validate(dt_start));
            match rule {
                Ok(rule) => {
                    let set = self
                        .exdates
                        .iter()
                        .fold(RRuleSet::new(dt_start).rrule(rule), |set, date| {
                            set.exdate(to_rule_time(*date))
                        });
                    dates.extend(
                        set.after(to_rule_time(range.start))
                            .before(to_rule_time(range.end))
                            .all(MAX_OCCURRENCES)
                            .dates
                            .into_iter()
                            .map(from_chrono),
                    );
                }
                Err(err) => {
                    tracing::warn!("invalid RRULE {rrule_str:?} on event {:?}: {err}", self.id)
                }
            }
        }
        dates.retain(|date| range.contains(date) && !self.exdates.contains(date));
        dates.sort_unstable();
        dates.dedup();
        dates
    }

    pub fn first_possible_occurrence(&self) -> OffsetDateTime {
        self.rdates
            .iter()
            .copied()
            .fold(self.start(), OffsetDateTime::min)
    }

    pub fn recurrence_cutoff(&self) -> Option<OffsetDateTime> {
        self.repeat_before
            .into_iter()
            .chain(self.split_before)
            .min()
    }

    fn recurrence_zone(&self) -> Tz {
        match self.time {
            EventTime::Timed { .. } => self
                .timezone
                .as_deref()
                .and_then(|name| name.parse::<chrono_tz::Tz>().ok())
                .map(Tz::from),
            EventTime::AllDay { .. } => None,
        }
        .unwrap_or(Tz::UTC)
    }

    /// Move a series boundary or exclusion by the change in wall-clock time
    /// between the selected occurrence and the successor's new start.
    pub fn rebase_occurrence(
        &self,
        original: &Event,
        split: OffsetDateTime,
        occurrence: OffsetDateTime,
    ) -> Option<OffsetDateTime> {
        let old_zone = original.recurrence_zone();
        let new_zone = self.recurrence_zone();
        let old_anchor = to_chrono(split, old_zone).naive_local();
        let new_anchor = to_chrono(self.start(), new_zone).naive_local();
        let old_wall = to_chrono(occurrence, old_zone).naive_local();
        let new_wall = old_wall.checked_add_signed(new_anchor - old_anchor)?;
        let local = new_wall.and_local_timezone(new_zone);
        let date = local.earliest()?;
        let preferred = to_chrono(occurrence, new_zone).offset().fix();
        let date = local
            .latest()
            .filter(|date| date.offset().fix() == preferred)
            .unwrap_or(date);
        Some(from_chrono(date))
    }

    pub fn start_following_series(
        &mut self,
        parent: &Event,
        split: OffsetDateTime,
    ) -> crate::Result<()> {
        let invalid_split =
            || crate::Error::Storage("selected occurrence no longer belongs to this series".into());
        if parent.recurrence_cutoff().is_some_and(|end| split >= end)
            || parent.exdates.contains(&split)
        {
            return Err(invalid_split());
        }

        self.exdates = parent
            .exdates
            .iter()
            .copied()
            .filter(|date| *date >= split)
            .filter_map(|date| self.rebase_occurrence(parent, split, date))
            .collect();
        self.rdates = parent
            .rdates
            .iter()
            .copied()
            .filter(|date| *date >= split)
            .filter_map(|date| self.rebase_occurrence(parent, split, date))
            .filter(|date| *date != self.start())
            .collect();
        self.repeat_before = parent.recurrence_cutoff().and_then(|date| {
            self.rebase_occurrence(parent, split, date)
                .or_else(|| Some(date.saturating_add(self.start() - split)))
        });

        if self.rrule == parent.rrule {
            if let Some(rule) = self.rrule.as_mut() {
                if let Some(count) = rule
                    .split(';')
                    .find_map(|field| field.strip_prefix("COUNT=")?.parse::<u32>().ok())
                {
                    let previous = parent
                        .recurrence_slots_before(split)
                        .ok_or_else(invalid_split)?;
                    let remaining = count
                        .checked_sub(previous)
                        .filter(|count| *count > 0)
                        .ok_or_else(invalid_split)?;
                    let updated = rule
                        .split(';')
                        .map(|field| {
                            if field.starts_with("COUNT=") {
                                format!("COUNT={remaining}")
                            } else {
                                field.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    *rule = updated;
                }
            }
        }

        self.id = crate::shared::ids::EventId::new();
        self.ics_uid = None;
        self.predecessor = Some((parent.id, split));
        self.split_before = None;
        Ok(())
    }

    /// Counts the original slots before a split, including excluded slots.
    /// RFC COUNT limits generated dates before EXDATE is applied.
    pub fn recurrence_slots_before(&self, cutoff: OffsetDateTime) -> Option<u32> {
        let zone = self.recurrence_zone();
        let start = to_chrono(self.start(), zone);
        let rule = self
            .rule_for_expansion(self.rrule.as_deref()?)
            .parse::<RRule<Unvalidated>>()
            .ok()?
            .validate(start)
            .ok()?;
        let set = RRuleSet::new(start).rrule(rule).limit();
        let cutoff = to_chrono(cutoff, zone);
        const MAX_SPLIT_SLOTS: usize = 1_000_000;
        let slots = (&set)
            .into_iter()
            .take_while(|date| *date < cutoff)
            .take(MAX_SPLIT_SLOTS)
            .count();
        (slots < MAX_SPLIT_SLOTS).then_some(slots as u32)
    }

    /// The start instant of this event, normalizing all-day events to
    /// midnight UTC.
    pub fn start(&self) -> OffsetDateTime {
        match &self.time {
            super::model::EventTime::Timed { start, .. } => *start,
            super::model::EventTime::AllDay { start, .. } => {
                start.midnight().assume_offset(time::UtcOffset::UTC)
            }
        }
    }
}
