use std::collections::HashMap;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use super::{atomic, index::EventIndex, migration, paths};
use crate::features::accounts::{Calendar, Snapshot};
use crate::features::calendar_management::model::{
    CalendarKey, LocalCalendarMeta, CURRENT_CALENDAR_SCHEMA_VERSION,
};
use crate::features::event::model::{Event, CURRENT_EVENT_SCHEMA_VERSION};
use crate::features::event::resolve_series_occurrence;
use crate::shared::ids::{CalendarId, EventId};
use crate::Result;

/// The local, file-backed store for calendars and events created in this
/// app.
pub struct LocalStorage {
    root: PathBuf,
    load_error: Option<String>,
    index: EventIndex,
    event_files: HashMap<EventId, PathBuf>,
    duplicate_files: HashMap<EventId, Vec<PathBuf>>,
    calendars: HashMap<CalendarId, LocalCalendarMeta>,
    remote_calendars: HashMap<Uuid, Vec<Calendar>>,
}

impl LocalStorage {
    pub fn unavailable(error: String) -> Self {
        Self {
            root: PathBuf::new(),
            load_error: Some(error),
            index: EventIndex::default(),
            event_files: HashMap::new(),
            duplicate_files: HashMap::new(),
            calendars: HashMap::new(),
            remote_calendars: HashMap::new(),
        }
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    fn ensure_writable(&self) -> Result<()> {
        match self.load_error() {
            Some(error) => Err(crate::Error::Storage(error.into())),
            None => Ok(()),
        }
    }

    pub fn load() -> Result<Self> {
        Self::load_from(paths::data_dir()?)
    }

    fn load_from(root: PathBuf) -> Result<Self> {
        let mut load_error = None;
        let mut calendars = HashMap::new();
        for entry in read_dir_json(&root.join("calendars"))? {
            match load_calendar_file(&entry) {
                Ok(meta) => {
                    calendars.insert(meta.id, meta);
                }
                Err(err) => record_load_error(&mut load_error, &entry, err),
            }
        }

        let mut index = EventIndex::default();
        let mut event_files = HashMap::new();
        let mut duplicate_files: HashMap<EventId, Vec<PathBuf>> = HashMap::new();
        for calendar_dir in read_dir_if_present(&root.join("events"))?
            .into_iter()
            .flatten()
        {
            let calendar_dir = calendar_dir?;
            if !std::fs::metadata(calendar_dir.path())?.is_dir() {
                continue;
            }
            let path = calendar_dir.path();
            for entry in read_dir_json(&path)? {
                match load_event_file(&entry) {
                    Ok(event) => {
                        if let Some(previous) = index.get(event.id) {
                            if (
                                previous.sequence,
                                previous.updated_at,
                                &event_files[&event.id],
                            ) >= (event.sequence, event.updated_at, &entry)
                            {
                                duplicate_files.entry(event.id).or_default().push(entry);
                                continue;
                            }
                        }
                        if let Some(old_path) = event_files.insert(event.id, entry) {
                            duplicate_files.entry(event.id).or_default().push(old_path);
                        }
                        index.upsert(event);
                    }
                    Err(err) => record_load_error(&mut load_error, &entry, err),
                }
            }
        }

        let mut storage = Self {
            root,
            load_error,
            index,
            event_files,
            duplicate_files,
            calendars,
            remote_calendars: HashMap::new(),
        };
        let splits: Vec<_> = storage
            .index
            .all_records()
            .filter_map(|event| event.predecessor)
            .collect();
        for (series, occurrence) in splits {
            storage.split_series(series, occurrence);
        }
        let overrides: Vec<_> = storage
            .index
            .all_records()
            .filter_map(|event| Some((event.series_id?, event.recurrence_id?)))
            .collect();
        for (series, occurrence) in overrides {
            storage.suppress_occurrence(series, occurrence);
        }
        Ok(storage)
    }

    pub fn has_calendars(&self) -> bool {
        !self.calendars.is_empty()
    }

    pub fn list_calendars(&self) -> Vec<&LocalCalendarMeta> {
        self.calendars
            .values()
            .filter(|meta| !meta.archived)
            .collect()
    }

    pub fn get_calendar(&self, id: CalendarId) -> Option<&LocalCalendarMeta> {
        self.calendars.get(&id).filter(|meta| !meta.archived)
    }

    pub fn create_calendar(&mut self, meta: LocalCalendarMeta) -> Result<()> {
        self.ensure_writable()?;
        let path = paths::calendar_path(&self.root, &meta.id);
        let wrapped = migration::wrap(CURRENT_CALENDAR_SCHEMA_VERSION, &meta)?;
        atomic::write_json_atomic(&path, &wrapped)?;
        self.calendars.insert(meta.id, meta);
        Ok(())
    }

    pub fn update_calendar(&mut self, meta: LocalCalendarMeta) -> Result<()> {
        self.create_calendar(meta)
    }

    pub fn archived_calendars(&self) -> Vec<&LocalCalendarMeta> {
        self.calendars
            .values()
            .filter(|meta| meta.archived)
            .collect()
    }

    pub fn archive_calendar(&mut self, id: CalendarId) -> Result<()> {
        let mut meta = self
            .get_calendar(id)
            .cloned()
            .ok_or_else(|| crate::Error::Storage("active calendar not found".into()))?;
        meta.archived = true;
        self.update_calendar(meta)
    }

    pub fn restore_calendar(&mut self, id: CalendarId) -> Result<()> {
        let mut meta = self
            .calendars
            .get(&id)
            .filter(|meta| meta.archived)
            .cloned()
            .ok_or_else(|| crate::Error::Storage("archived calendar not found".into()))?;
        meta.archived = false;
        self.update_calendar(meta)
    }

    fn event_is_active(&self, event: &Event) -> bool {
        match event.calendar {
            CalendarKey::Local(id) => !self.calendars.get(&id).is_some_and(|meta| meta.archived),
            CalendarKey::Remote { .. } => true,
        }
    }

    pub fn replace_remote_snapshot(&mut self, account_id: Uuid, snapshot: Snapshot) {
        self.clear_remote_snapshot(account_id);
        let overrides: Vec<_> = snapshot
            .events
            .iter()
            .filter_map(|event| Some((event.series_id?, event.recurrence_id?)))
            .collect();
        for event in snapshot.events {
            self.index.upsert(event);
        }
        for (series, occurrence) in overrides {
            self.suppress_occurrence(series, occurrence);
        }
        self.remote_calendars.insert(account_id, snapshot.calendars);
    }

    pub fn clear_remote_snapshot(&mut self, account_id: Uuid) {
        self.remote_calendars.remove(&account_id);
        let ids: Vec<_> = self.index.all_records().filter_map(|event| {
            matches!(event.calendar, CalendarKey::Remote { account_id: id, .. } if id == account_id)
                .then_some(event.id)
        }).collect();
        for id in ids {
            self.index.remove(id);
        }
    }

    pub fn remote_calendars(&self, account_id: Uuid) -> &[Calendar] {
        self.remote_calendars
            .get(&account_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn remote_sync_seed(&self, account_id: Uuid) -> Snapshot {
        let calendars: Vec<_> = self
            .remote_calendars(account_id)
            .iter()
            .filter(|calendar| calendar.sync_token.is_some())
            .cloned()
            .collect();
        let ids: std::collections::HashSet<_> =
            calendars.iter().map(|calendar| &calendar.id).collect();
        let events = self
            .index
            .all_records()
            .filter(|event| {
                matches!(&event.calendar, CalendarKey::Remote { account_id: id, calendar_id }
                if *id == account_id && ids.contains(calendar_id))
            })
            .cloned()
            .collect();
        Snapshot { calendars, events }
    }

    pub fn get_event(&self, id: EventId) -> Option<&Event> {
        self.index
            .get(id)
            .filter(|event| self.event_is_active(event))
    }

    pub fn contains_event_id(&self, id: EventId) -> bool {
        self.index.get(id).is_some()
    }

    /// Events in active calendars, regardless of date.
    pub fn all_events(&self) -> Vec<&Event> {
        self.index
            .all()
            .filter(|event| self.event_is_active(event))
            .collect()
    }

    /// Include truncated predecessors so the exporter can resolve moved
    /// exceptions even when the old series has no visible occurrences.
    pub fn events_for_export(&self) -> impl Iterator<Item = &Event> {
        self.index
            .all()
            .chain(self.index.all_records().filter(|event| {
                event.split_before.is_some()
                    && event
                        .recurrence_cutoff()
                        .is_some_and(|end| end <= event.first_possible_occurrence())
            }))
            .filter(|event| {
                matches!(event.calendar, CalendarKey::Local(_)) && self.event_is_active(event)
            })
    }

    pub fn search(&self, query: &str) -> Vec<&Event> {
        self.index
            .search(query)
            .into_iter()
            .filter(|event| self.event_is_active(event))
            .collect()
    }

    pub fn create_event(&mut self, event: Event) -> Result<()> {
        self.ensure_writable()?;
        if matches!(event.calendar, CalendarKey::Remote { .. })
            || self
                .index
                .get(event.id)
                .is_some_and(|existing| matches!(existing.calendar, CalendarKey::Remote { .. }))
        {
            return Err(crate::Error::Storage("remote events are read-only".into()));
        }
        if self
            .index
            .get(event.id)
            .is_some_and(|existing| !self.event_is_active(existing))
        {
            return Err(crate::Error::Storage(
                "event belongs to an archived calendar".into(),
            ));
        }
        if let CalendarKey::Local(calendar_id) = &event.calendar {
            let already_assigned = self
                .index
                .get(event.id)
                .is_some_and(|existing| existing.calendar == event.calendar);
            if !already_assigned && self.get_calendar(*calendar_id).is_none() {
                return Err(crate::Error::Storage(
                    "destination calendar no longer exists".into(),
                ));
            }
            let path = self
                .event_files
                .get(&event.id)
                .cloned()
                .unwrap_or_else(|| paths::event_path(&self.root, calendar_id, &event.id));
            let wrapped = migration::wrap(CURRENT_EVENT_SCHEMA_VERSION, &event)?;
            atomic::write_json_atomic(&path, &wrapped)?;
            self.event_files.insert(event.id, path);
        }
        let replacement = event.series_id.zip(event.recurrence_id);
        let split = event.predecessor;
        self.index.upsert(event);
        if let Some((series, occurrence)) = split {
            self.split_series(series, occurrence);
        }
        if let Some((series, occurrence)) = replacement {
            self.suppress_occurrence(series, occurrence);
        }
        Ok(())
    }

    pub fn update_event(&mut self, event: Event) -> Result<()> {
        self.create_event(event)
    }

    fn split_series(&mut self, series: EventId, occurrence: OffsetDateTime) {
        if let Some(mut parent) = self.index.get(series).cloned() {
            parent.split_before = Some(
                parent
                    .split_before
                    .map_or(occurrence, |cutoff| cutoff.min(occurrence)),
            );
            self.index.upsert(parent);
        }
    }

    fn suppress_occurrence(&mut self, series: EventId, occurrence: OffsetDateTime) {
        let (series, occurrence) =
            resolve_series_occurrence(series, occurrence, self.index.all_records());
        if let Some(mut parent) = self.index.get(series).cloned() {
            if !parent.exdates.contains(&occurrence) {
                parent.exdates.push(occurrence);
                self.index.upsert(parent);
            }
        }
    }

    pub fn delete_event(&mut self, id: EventId) -> Result<()> {
        self.ensure_writable()?;
        let Some(event) = self.get_event(id) else {
            return Err(crate::Error::Storage("active event not found".into()));
        };
        if matches!(event.calendar, CalendarKey::Remote { .. }) {
            return Err(crate::Error::Storage("remote events are read-only".into()));
        }
        let mut successors = std::collections::HashSet::from([id]);
        loop {
            let children: Vec<_> = self
                .index
                .all_records()
                .filter(|event| {
                    event
                        .predecessor
                        .is_some_and(|(parent, _)| successors.contains(&parent))
                })
                .map(|event| event.id)
                .collect();
            let count = successors.len();
            successors.extend(children);
            if successors.len() == count {
                break;
            }
        }
        let overrides: Vec<_> = self
            .index
            .all_records()
            .filter_map(|event| {
                let (series, occurrence) = event.series_id.zip(event.recurrence_id)?;
                let (owner, slot) =
                    resolve_series_occurrence(series, occurrence, self.index.all_records());
                (owner == id || (series != owner && successors.contains(&owner))).then_some((
                    event.clone(),
                    owner,
                    slot,
                ))
            })
            .collect();
        if overrides
            .iter()
            .any(|(event, _, _)| !self.event_is_active(event))
        {
            return Err(crate::Error::Storage(
                "restore the calendar containing the related occurrence before deleting this event"
                    .into(),
            ));
        }
        if let Some((series, occurrence)) = self.index.get(id).and_then(|event| event.predecessor) {
            if let Some(mut parent) = self.index.get(series).cloned() {
                parent.repeat_before = Some(
                    parent
                        .repeat_before
                        .map_or(occurrence, |cutoff| cutoff.min(occurrence)),
                );
                self.update_event(parent)?;
            }
        }
        if let Some((series, occurrence)) = self
            .index
            .get(id)
            .and_then(|event| event.series_id.zip(event.recurrence_id))
        {
            let (owner, slot) =
                resolve_series_occurrence(series, occurrence, self.index.all_records());
            if let Some(mut parent) = self.index.get(owner).cloned() {
                if !parent.exdates.contains(&slot) {
                    parent.exdates.push(slot);
                }
                self.update_event(parent)?;
            }
        }
        for (mut replacement, owner, slot) in overrides {
            if owner == id {
                self.remove_event_file(replacement.id)?;
            } else {
                replacement.series_id = Some(owner);
                replacement.recurrence_id = Some(slot);
                replacement.ics_uid = self
                    .index
                    .get(owner)
                    .and_then(|parent| parent.ics_uid.clone());
                self.update_event(replacement)?;
            }
        }
        self.remove_event_file(id)
    }

    fn remove_event_file(&mut self, id: EventId) -> Result<()> {
        for path in self
            .duplicate_files
            .get(&id)
            .into_iter()
            .flatten()
            .chain(self.event_files.get(&id))
        {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        self.duplicate_files.remove(&id);
        self.event_files.remove(&id);
        self.index.remove(id);
        Ok(())
    }
}

fn record_load_error(first_error: &mut Option<String>, path: &Path, error: crate::Error) {
    let message = format!("{}: {error}", path.display());
    warn!("failed to load calendar data: {message}");
    first_error.get_or_insert(message);
}

fn read_dir_if_present(dir: &Path) -> Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(Some(entries)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(crate::Error::Storage(format!("{}: {err}", dir.display()))),
    }
}

fn read_dir_json(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in read_dir_if_present(dir)?.into_iter().flatten() {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn load_calendar_file(path: &std::path::Path) -> Result<LocalCalendarMeta> {
    let bytes = std::fs::read(path)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    migration::unwrap_current(raw, CURRENT_CALENDAR_SCHEMA_VERSION, "LocalCalendarMeta")
}

fn load_event_file(path: &std::path::Path) -> Result<Event> {
    let bytes = std::fs::read(path)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    migration::unwrap_current(raw, CURRENT_EVENT_SCHEMA_VERSION, "Event")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archiving_the_only_calendar_preserves_the_empty_active_list_on_reload() -> Result<()> {
        let root = std::env::temp_dir().join(format!("cosmic-calendar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root)?;
        let result = (|| {
            let mut storage = LocalStorage::load_from(root.clone())?;
            assert!(!storage.has_calendars());

            let id = CalendarId::new();
            storage.create_calendar(LocalCalendarMeta {
                id,
                name: "Personal".into(),
                description: None,
                color: None,
                visible: true,
                archived: false,
            })?;
            storage.archive_calendar(id)?;

            let mut storage = LocalStorage::load_from(root.clone())?;
            assert!(storage.has_calendars());
            assert!(storage.list_calendars().is_empty());
            assert_eq!(storage.archived_calendars()[0].id, id);

            storage.restore_calendar(id)?;
            assert!(storage.get_calendar(id).is_some());
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }
}
