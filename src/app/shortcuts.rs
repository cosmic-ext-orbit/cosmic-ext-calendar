// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::sync::LazyLock;

use cosmic::iced::keyboard::{key::Named, Key, Modifiers};
use cosmic::iced::{event, keyboard, window, Event, Subscription};
use cosmic::prelude::*;
use cosmic::widget::{self, menu};
use cosmic::{Application, Element};
use time::Duration;

use super::{AppModel, ContextPage, MenuAction, Message};
use crate::features::accounts::AccountsAction;
use crate::features::calendar::{display_time, CalendarAction, CalendarMessage};
use crate::features::calendar_management::CalendarManagementMessage;
use crate::features::event::{model::EventTime, EventMessage};
use crate::features::ics::IcsMessage;
use crate::fl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Scope {
    Calendar,
    EventEditor,
    CalendarForm,
    Archive,
    Import,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Menu(MenuAction),
    NewEvent,
    Search,
    Refresh,
    CloseWindow,
    ToggleSidebar,
    Dismiss,
    CloseDialog,
    View(usize),
    Day(bool),
    Week(bool),
    Month(bool),
    Year(bool),
    Today,
    Agenda,
    Details,
    StepEvent(bool),
    Event(CalendarAction),
    SaveEvent,
    EventOptions,
    AllDay,
    SaveCalendar,
    ConfirmArchive,
    ConfirmImport,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Navigation,
    SelectedEvent,
    EventEditor,
    CalendarManagement,
    Import,
}

struct Binding {
    scope: Scope,
    section: Section,
    action: Action,
    key: menu::KeyBind,
}

static BINDINGS: LazyLock<Vec<Binding>> = LazyLock::new(|| {
    use menu::key_bind::Modifier::{self, Alt, Ctrl, Shift};
    use Action::*;
    use Named::*;
    use Section::{CalendarManagement, General, Navigation, SelectedEvent};

    let bind = |scope, section, action, modifiers: &[Modifier], key| Binding {
        scope,
        section,
        action,
        key: menu::KeyBind {
            modifiers: modifiers.to_vec(),
            key,
        },
    };
    let main =
        |section, action, modifiers, key| bind(Scope::Calendar, section, action, modifiers, key);
    let c = |key: &'static str| Key::Character(key.into());
    let k = Key::Named;
    vec![
        main(General, NewEvent, &[Ctrl], c("n")),
        main(
            General,
            Menu(MenuAction::NewCalendar),
            &[Ctrl, Shift],
            c("n"),
        ),
        main(General, Search, &[Ctrl], c("f")),
        main(General, Refresh, &[Ctrl], c("r")),
        main(General, Refresh, &[], k(F5)),
        main(General, Menu(MenuAction::Accounts), &[Ctrl, Shift], c("a")),
        main(General, Menu(MenuAction::ImportIcs), &[Ctrl, Shift], c("i")),
        main(General, Menu(MenuAction::ExportIcs), &[Ctrl, Shift], c("e")),
        main(
            General,
            Menu(MenuAction::ArchivedCalendars),
            &[Ctrl, Shift],
            c("h"),
        ),
        main(General, Menu(MenuAction::Settings), &[Ctrl], c(",")),
        main(General, Menu(MenuAction::Shortcuts), &[], k(F1)),
        main(General, Menu(MenuAction::About), &[Ctrl, Shift], c("b")),
        main(General, CloseWindow, &[Ctrl], c("w")),
        main(General, ToggleSidebar, &[Ctrl, Shift], c("l")),
        main(General, Menu(MenuAction::Quit), &[Ctrl], c("q")),
        main(General, Dismiss, &[], k(Escape)),
        main(Navigation, View(0), &[Alt], c("1")),
        main(Navigation, View(1), &[Alt], c("2")),
        main(Navigation, View(2), &[Alt], c("3")),
        main(Navigation, Day(false), &[Alt], k(ArrowLeft)),
        main(Navigation, Day(true), &[Alt], k(ArrowRight)),
        main(Navigation, Week(false), &[Alt], k(PageUp)),
        main(Navigation, Week(true), &[Alt], k(PageDown)),
        main(Navigation, Month(false), &[Ctrl], k(PageUp)),
        main(Navigation, Month(true), &[Ctrl], k(PageDown)),
        main(Navigation, Year(false), &[Ctrl, Shift], k(PageUp)),
        main(Navigation, Year(true), &[Ctrl, Shift], k(PageDown)),
        main(Navigation, Today, &[Ctrl], k(Home)),
        main(Navigation, Agenda, &[Ctrl], c("g")),
        main(SelectedEvent, StepEvent(false), &[Alt], k(ArrowUp)),
        main(SelectedEvent, StepEvent(true), &[Alt], k(ArrowDown)),
        main(SelectedEvent, Details, &[], k(Enter)),
        main(
            SelectedEvent,
            Event(CalendarAction::Edit),
            &[Ctrl],
            k(Enter),
        ),
        main(SelectedEvent, Event(CalendarAction::Edit), &[], k(F2)),
        main(SelectedEvent, Event(CalendarAction::Copy), &[Ctrl], c("c")),
        main(SelectedEvent, Event(CalendarAction::Cut), &[Ctrl], c("x")),
        main(SelectedEvent, Event(CalendarAction::Paste), &[Ctrl], c("v")),
        main(
            SelectedEvent,
            Event(CalendarAction::Duplicate),
            &[Ctrl],
            c("d"),
        ),
        main(SelectedEvent, Event(CalendarAction::Delete), &[], k(Delete)),
        bind(
            Scope::EventEditor,
            Section::EventEditor,
            SaveEvent,
            &[Ctrl],
            c("s"),
        ),
        bind(
            Scope::EventEditor,
            Section::EventEditor,
            SaveEvent,
            &[Ctrl],
            k(Enter),
        ),
        bind(
            Scope::EventEditor,
            Section::EventEditor,
            EventOptions,
            &[Ctrl, Shift],
            c("o"),
        ),
        bind(
            Scope::EventEditor,
            Section::EventEditor,
            AllDay,
            &[Ctrl, Shift],
            c("d"),
        ),
        bind(
            Scope::EventEditor,
            Section::EventEditor,
            CloseDialog,
            &[Ctrl],
            c("w"),
        ),
        bind(
            Scope::CalendarForm,
            CalendarManagement,
            SaveCalendar,
            &[Ctrl],
            c("s"),
        ),
        bind(
            Scope::CalendarForm,
            CalendarManagement,
            SaveCalendar,
            &[Ctrl],
            k(Enter),
        ),
        bind(
            Scope::CalendarForm,
            CalendarManagement,
            CloseDialog,
            &[Ctrl],
            c("w"),
        ),
        bind(
            Scope::Archive,
            CalendarManagement,
            ConfirmArchive,
            &[Ctrl],
            c("s"),
        ),
        bind(
            Scope::Archive,
            CalendarManagement,
            ConfirmArchive,
            &[Ctrl],
            k(Enter),
        ),
        bind(
            Scope::Import,
            Section::Import,
            ConfirmImport,
            &[Ctrl],
            c("s"),
        ),
        bind(
            Scope::Import,
            Section::Import,
            ConfirmImport,
            &[Ctrl],
            k(Enter),
        ),
        bind(
            Scope::Archive,
            CalendarManagement,
            CloseDialog,
            &[Ctrl],
            c("w"),
        ),
        bind(Scope::Import, Section::Import, CloseDialog, &[Ctrl], c("w")),
    ]
});

impl Action {
    fn label(self) -> String {
        match self {
            Self::Menu(action) => match action {
                MenuAction::NewCalendar => fl!("new-calendar"),
                MenuAction::ImportIcs => fl!("import-ics"),
                MenuAction::ExportIcs => fl!("export-ics"),
                MenuAction::ArchivedCalendars => fl!("archived-calendars"),
                MenuAction::Accounts => fl!("manage-accounts"),
                MenuAction::Settings => fl!("settings"),
                MenuAction::Shortcuts => fl!("keyboard-shortcuts"),
                MenuAction::About => fl!("about"),
                MenuAction::Quit => fl!("shortcut-quit"),
            },
            Self::NewEvent => fl!("create-event"),
            Self::Search => fl!("search"),
            Self::Refresh => fl!("shortcut-refresh"),
            Self::CloseWindow => fl!("shortcut-close-window"),
            Self::Dismiss => fl!("shortcut-dismiss"),
            Self::CloseDialog => fl!("shortcut-close-dialog"),
            Self::ToggleSidebar => fl!("shortcut-toggle-sidebar"),
            Self::View(0) => fl!("month"),
            Self::View(1) => fl!("week"),
            Self::View(_) => fl!("day"),
            Self::Day(false) => fl!("shortcut-previous-day"),
            Self::Day(true) => fl!("shortcut-next-day"),
            Self::Week(false) => fl!("shortcut-previous-week"),
            Self::Week(true) => fl!("shortcut-next-week"),
            Self::Month(false) => fl!("shortcut-previous-month"),
            Self::Month(true) => fl!("shortcut-next-month"),
            Self::Year(false) => fl!("shortcut-previous-year"),
            Self::Year(true) => fl!("shortcut-next-year"),
            Self::Today => fl!("today"),
            Self::Agenda => fl!("shortcut-month-agenda"),
            Self::Details => fl!("shortcut-event-details"),
            Self::StepEvent(false) => fl!("shortcut-previous-event"),
            Self::StepEvent(true) => fl!("shortcut-next-event"),
            Self::Event(action) => match action {
                CalendarAction::Edit => fl!("edit-event"),
                CalendarAction::Copy => fl!("calendar-copy"),
                CalendarAction::Cut => fl!("calendar-cut"),
                CalendarAction::Paste => fl!("calendar-paste"),
                CalendarAction::Duplicate => fl!("calendar-duplicate"),
                CalendarAction::Delete => fl!("delete"),
            },
            Self::SaveEvent => fl!("shortcut-save-event"),
            Self::EventOptions => fl!("event-more-options"),
            Self::AllDay => fl!("shortcut-toggle-all-day"),
            Self::SaveCalendar => fl!("shortcut-save-calendar"),
            Self::ConfirmArchive => fl!("shortcut-confirm-archive"),
            Self::ConfirmImport => fl!("shortcut-confirm-import"),
        }
    }
}

pub(super) fn menu_bindings() -> HashMap<menu::KeyBind, MenuAction> {
    BINDINGS
        .iter()
        .filter_map(|binding| match binding.action {
            Action::Menu(action) => Some((binding.key.clone(), action)),
            _ => None,
        })
        .collect()
}

pub(super) fn subscription(main_window: Option<window::Id>, scope: Scope) -> Subscription<Action> {
    event::listen_with(|event, status, window| match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            repeat,
            ..
        }) => Some((window, key, physical_key, modifiers, repeat, status)),
        _ => None,
    })
    .with((main_window, scope))
    .filter_map(
        |((main_window, scope), (window, key, physical, modifiers, repeat, status))| {
            if main_window != Some(window) {
                return None;
            }
            resolve(scope, &key, Some(&physical), modifiers, repeat, status)
        },
    )
}

fn resolve(
    scope: Scope,
    key: &Key,
    physical: Option<&keyboard::key::Physical>,
    modifiers: Modifiers,
    repeat: bool,
    status: event::Status,
) -> Option<Action> {
    if key == &Key::Named(Named::Escape) && modifiers.is_empty() {
        return (scope != Scope::Calendar && !repeat && status == event::Status::Captured)
            .then_some(Action::Dismiss);
    }
    if status == event::Status::Captured
        && (scope == Scope::Calendar || key == &Key::Named(Named::Enter))
    {
        return None;
    }
    BINDINGS.iter().find_map(|binding| {
        (binding.scope == scope
            && (!repeat
                || matches!(
                    binding.action,
                    Action::Day(_)
                        | Action::Week(_)
                        | Action::Month(_)
                        | Action::Year(_)
                        | Action::StepEvent(_)
                ))
            && binding.key.matches(modifiers, key, physical))
        .then_some(binding.action)
    })
}

pub(super) fn view() -> Element<'static, Message> {
    let headings = [
        (Section::General, fl!("shortcut-general")),
        (Section::Navigation, fl!("shortcut-navigation")),
        (Section::SelectedEvent, fl!("shortcut-selected-event")),
        (Section::EventEditor, fl!("shortcut-event-editor")),
        (
            Section::CalendarManagement,
            fl!("shortcut-calendar-management"),
        ),
        (Section::Import, fl!("import-ics")),
    ];
    let mut content = widget::column::with_capacity(headings.len()).spacing(12);
    for (group, title) in headings {
        let mut section = widget::settings::section().title(title);
        let mut rows: Vec<(Action, Vec<String>)> = Vec::new();
        for binding in BINDINGS.iter().filter(|binding| binding.section == group) {
            let key = binding.key.to_string();
            if let Some((_, keys)) = rows
                .iter_mut()
                .find(|(action, _)| *action == binding.action)
            {
                keys.push(key);
            } else {
                rows.push((binding.action, vec![key]));
            }
        }
        for (action, keys) in rows {
            section = section.add(
                widget::settings::item::builder(action.label())
                    .control(widget::text::caption(keys.join(" / "))),
            );
        }
        content = content.push(section);
    }
    content.into()
}

pub(super) fn search_id() -> widget::Id {
    widget::Id::new("calendar-search")
}

impl AppModel {
    pub(super) fn shortcut_scope(&self) -> Scope {
        if self.editing_event.is_some() {
            Scope::EventEditor
        } else if self.calendar_to_archive.is_some() {
            Scope::Archive
        } else if self.calendar_form.is_some() {
            Scope::CalendarForm
        } else if self.core.window.show_context && self.context_page == ContextPage::IcsImport {
            Scope::Import
        } else {
            Scope::Calendar
        }
    }

    pub(super) fn update_shortcut(&mut self, action: Action) -> Task<cosmic::Action<Message>> {
        use menu::action::MenuAction as _;
        use Action::*;

        let date = self.calendar.selected_date;
        let message = match action {
            Menu(action) => action.message(),
            NewEvent => self.new_event_message(),
            Search => return widget::text_input::focus(search_id()),
            Refresh => {
                let ids: Vec<_> = self.active_accounts.iter().copied().collect();
                let mut tasks: Vec<_> = ids.into_iter().map(|id| self.sync_account(id)).collect();
                tasks.push(self.update(Message::Accounts(AccountsAction::Discover)));
                return Task::batch(tasks);
            }
            CloseWindow => {
                return self.core.main_window_id().map_or_else(Task::none, |id| {
                    Task::batch([self.update(Message::MainWindowClosed), window::close(id)])
                });
            }
            Dismiss | CloseDialog => return self.on_escape(),
            ToggleSidebar => {
                if self.core.window.show_context && self.context_page == ContextPage::DayAgenda {
                    self.close_day_agenda();
                }
                self.core.nav_bar_set_toggled(!self.core.nav_bar_active());
                return Task::none();
            }
            View(index) => {
                let Some(tab) = self.tabs.iter().nth(index) else {
                    return Task::none();
                };
                Message::Calendar(CalendarMessage::TabSelected(tab))
            }
            Day(false) => Message::Calendar(CalendarMessage::NavigatePreviousDay),
            Day(true) => Message::Calendar(CalendarMessage::NavigateNextDay),
            Week(next) => Message::Calendar(CalendarMessage::SelectDate(
                date.saturating_add(Duration::weeks(if next { 1 } else { -1 })),
            )),
            Month(false) => Message::Calendar(CalendarMessage::NavigatePreviousMonth),
            Month(true) => Message::Calendar(CalendarMessage::NavigateNextMonth),
            Year(false) => Message::Calendar(CalendarMessage::NavigatePreviousYear),
            Year(true) => Message::Calendar(CalendarMessage::NavigateNextYear),
            Today => Message::Calendar(CalendarMessage::NavigateToday),
            Agenda => {
                if !self.show_month() {
                    return Task::none();
                }
                Message::Calendar(CalendarMessage::ShowDayAgenda(date.date()))
            }
            StepEvent(next) => {
                if !self.search_query.is_empty() {
                    return Task::none();
                }
                let events = self
                    .calendar
                    .selected_day_events(&self.storage.lock().unwrap(), self);
                if let Some(first) = events.first().copied() {
                    let current = self
                        .calendar
                        .selected_event
                        .and_then(|selection| events.iter().position(|event| *event == selection));
                    let target = match (current, next) {
                        (Some(index), true) => events.get(index + 1).copied().unwrap_or(first),
                        (Some(index), false) if index > 0 => events[index - 1],
                        (_, true) => first,
                        (_, false) => events[events.len() - 1],
                    };
                    self.calendar.selected_event = Some(target);
                    self.calendar.selected_slot = None;
                    self.calendar_popup = None;
                    self.calendar_click = None;
                }
                return Task::none();
            }
            Details => {
                if let Some(selection) = self.calendar.selected_event {
                    self.open_event_details(selection, None);
                }
                return Task::none();
            }
            Event(action) => {
                let (selection, target) = if action == CalendarAction::Paste {
                    (
                        None,
                        self.calendar
                            .selected_slot
                            .map_or(date, |slot| slot.start(date.offset())),
                    )
                } else {
                    let Some((id, start)) = self.calendar.selected_event else {
                        return Task::none();
                    };
                    let storage = self.storage.lock().unwrap();
                    let Some(event) = storage
                        .get_event(id)
                        .filter(|event| self.event_is_visible(&storage, event))
                    else {
                        return Task::none();
                    };
                    if matches!(
                        action,
                        CalendarAction::Edit | CalendarAction::Cut | CalendarAction::Delete
                    ) && matches!(
                        event.calendar,
                        crate::features::calendar_management::model::CalendarKey::Remote { .. }
                    ) {
                        return Task::none();
                    }
                    let start_on_calendar =
                        if event.floating || matches!(event.time, EventTime::AllDay { .. }) {
                            start
                        } else {
                            display_time(start, self.calendar.display_timezone)
                        };
                    (Some((id, start)), start_on_calendar)
                };
                Message::Calendar(CalendarMessage::EventAction(action, selection, target))
            }
            SaveEvent => Message::Event(EventMessage::SaveEvent),
            EventOptions => Message::Event(EventMessage::ToggleEventOptions),
            AllDay => {
                let Some(state) = self.editing_event.as_ref().filter(|state| !state.writing) else {
                    return Task::none();
                };
                Message::Event(EventMessage::EventEditorAllDayToggled(!state.all_day))
            }
            SaveCalendar => {
                Message::CalendarManagement(CalendarManagementMessage::SaveCalendarForm)
            }
            ConfirmArchive => {
                Message::CalendarManagement(CalendarManagementMessage::ConfirmArchiveCalendar)
            }
            ConfirmImport => Message::Ics(IcsMessage::ConfirmIcsImport),
        };
        self.update(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event::Status::{Captured, Ignored};

    #[test]
    fn captured_or_repeated_delete_does_not_delete_an_event() {
        let delete = |scope, repeat, status| {
            resolve(
                scope,
                &Key::Named(Named::Delete),
                None,
                Modifiers::empty(),
                repeat,
                status,
            )
        };
        assert_eq!(delete(Scope::Calendar, false, Captured), None);
        assert_eq!(delete(Scope::Calendar, true, Ignored), None);
        assert_eq!(delete(Scope::EventEditor, false, Ignored), None);
        assert_eq!(
            delete(Scope::Calendar, false, Ignored),
            Some(Action::Event(CalendarAction::Delete))
        );
    }

    #[test]
    fn save_shortcuts_follow_the_open_dialog() {
        for (scope, expected) in [
            (Scope::Calendar, None),
            (Scope::EventEditor, Some(Action::SaveEvent)),
            (Scope::CalendarForm, Some(Action::SaveCalendar)),
            (Scope::Archive, Some(Action::ConfirmArchive)),
            (Scope::Import, Some(Action::ConfirmImport)),
        ] {
            let save = |key, status| resolve(scope, &key, None, Modifiers::CTRL, false, status);
            assert_eq!(save(Key::Character("s".into()), Captured), expected);
            assert_eq!(save(Key::Named(Named::Enter), Captured), None);
            if scope != Scope::Calendar {
                assert_eq!(save(Key::Named(Named::Enter), Ignored), expected);
            }
        }
    }

    #[test]
    fn ignored_escape_is_left_to_the_application() {
        let escape = |status| {
            resolve(
                Scope::EventEditor,
                &Key::Named(Named::Escape),
                None,
                Modifiers::empty(),
                false,
                status,
            )
        };
        assert_eq!(escape(Ignored), None);
        assert_eq!(escape(Captured), Some(Action::Dismiss));
    }

    #[test]
    fn preserves_calendar_navigation_and_exposes_help_in_the_menu() {
        assert_eq!(
            resolve(
                Scope::Calendar,
                &Key::Named(Named::PageUp),
                None,
                Modifiers::CTRL,
                true,
                Ignored
            ),
            Some(Action::Month(false)),
        );
        let help = menu::KeyBind {
            modifiers: vec![],
            key: Key::Named(Named::F1),
        };
        assert_eq!(menu_bindings().get(&help), Some(&MenuAction::Shortcuts));
        assert_eq!(
            resolve(
                Scope::Calendar,
                &help.key,
                None,
                Modifiers::empty(),
                false,
                Ignored
            ),
            Some(Action::Menu(MenuAction::Shortcuts))
        );
    }

    #[test]
    fn bindings_are_unique_within_each_scope() {
        let mut seen = std::collections::HashSet::new();
        for binding in BINDINGS.iter() {
            assert!(
                seen.insert((binding.scope, &binding.key)),
                "duplicate shortcut: {}",
                binding.key
            );
        }
    }
}
