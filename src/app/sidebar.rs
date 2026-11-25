// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeSet, HashMap};

use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget::{self, menu, nav_bar};
use cosmic::{theme, Application, Element};
use uuid::Uuid;

use super::{AppModel, ContextPage, Message};
use crate::config::Config;
use crate::features::accounts::{Account, AccountsAction, SyncStatus};
use crate::features::calendar_management::{
    model::{calendar_color, CalendarKey},
    view::calendar_color_icon,
    CalendarManagementMessage,
};
use crate::fl;
use crate::shared::{ids::CalendarId, storage::LocalStorage};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Local,
    Account(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Group(Group),
    Calendar(CalendarKey),
}

#[derive(PartialEq, Eq)]
enum Icon {
    LocalCalendars,
    Account,
    AccountWarning,
    Calendar {
        color: Option<String>,
        visible: bool,
    },
}

#[derive(PartialEq, Eq)]
struct Row {
    item: Item,
    label: String,
    indent: u16,
    icon: Icon,
    refresh: bool,
    syncing: bool,
}

#[derive(Default)]
pub(super) struct Sidebar {
    collapsed: BTreeSet<Group>,
    rows: Vec<Row>,
}

impl Sidebar {
    fn group(
        &self,
        group: Group,
        label: String,
        status: Option<&SyncStatus>,
        rows: &mut Vec<Row>,
    ) -> bool {
        let failed = status.and_then(SyncStatus::error).is_some();
        let refresh = matches!(group, Group::Account(_))
            && !matches!(status, Some(SyncStatus::Running { .. }));
        let collapsed = self.collapsed.contains(&group);
        let icon = match group {
            Group::Local => Icon::LocalCalendars,
            Group::Account(_) if failed => Icon::AccountWarning,
            Group::Account(_) => Icon::Account,
        };
        rows.push(Row {
            item: Item::Group(group),
            label,
            indent: 0,
            icon,
            refresh,
            syncing: matches!(status, Some(SyncStatus::Running { .. })),
        });
        !collapsed && !failed
    }

    fn build_rows(
        &self,
        storage: &LocalStorage,
        accounts: &[Account],
        active_accounts: &BTreeSet<Uuid>,
        sync_status: &HashMap<Uuid, SyncStatus>,
        config: &Config,
    ) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.group(Group::Local, fl!("local-calendars"), None, &mut rows) {
            let mut local = storage.list_calendars();
            local.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
            rows.extend(local.into_iter().map(|calendar| Row {
                item: Item::Calendar(CalendarKey::Local(calendar.id)),
                label: calendar.name.clone(),
                indent: 1,
                refresh: false,
                syncing: false,
                icon: Icon::Calendar {
                    color: calendar.color.clone(),
                    visible: calendar.visible,
                },
            }));
        }

        let mut accounts: Vec<_> = accounts
            .iter()
            .filter(|account| active_accounts.contains(&account.id))
            .map(|account| {
                let label = account
                    .address()
                    .map_or_else(|| fl!("sidebar-online-account"), str::to_owned);
                (label, account.id)
            })
            .collect();
        accounts.sort();
        for (label, account_id) in accounts {
            if !self.group(
                Group::Account(account_id),
                label,
                sync_status.get(&account_id),
                &mut rows,
            ) {
                continue;
            }
            let mut calendars: Vec<_> = storage.remote_calendars(account_id).iter().collect();
            calendars.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
            rows.extend(calendars.into_iter().map(|calendar| Row {
                item: Item::Calendar(CalendarKey::Remote {
                    account_id,
                    calendar_id: calendar.id.clone(),
                }),
                label: calendar.name.clone(),
                indent: 1,
                refresh: false,
                syncing: false,
                icon: Icon::Calendar {
                    color: None,
                    visible: !config.remote_calendar_hidden(account_id, &calendar.id),
                },
            }));
        }
        rows
    }

    fn update_nav(&mut self, nav: &mut nav_bar::Model, rows: Vec<Row>) {
        if self.rows == rows {
            return;
        }
        let old_ids: Vec<_> = nav.iter().collect();
        for id in old_ids {
            nav.remove(id);
        }
        for row in &rows {
            let icon = match &row.icon {
                Icon::LocalCalendars => {
                    widget::icon::from_name("x-office-calendar-symbolic").into()
                }
                Icon::Account => widget::icon::from_name("user-info-symbolic").into(),
                Icon::AccountWarning => widget::icon::from_name("dialog-warning-symbolic").into(),
                Icon::Calendar { color, visible } => {
                    calendar_color_icon(color.as_deref().and_then(calendar_color), *visible)
                }
            };
            let icon = icon.icon().size(16);
            let icon = if row.syncing {
                widget::icon::from_raster_pixels(1, 1, &[0u8; 4][..])
                    .icon()
                    .size(16)
            } else {
                icon
            };
            let id = nav
                .insert()
                .indent(row.indent)
                .text(row.label.clone())
                .icon(icon)
                .data(row.item.clone())
                .id();
            nav.closable_set(id, row.refresh);
        }
        self.rows = rows;
    }
}

impl AppModel {
    pub(super) fn sidebar_view(&self) -> Element<'_, cosmic::Action<Message>> {
        const ROW_HEIGHT: u16 = 32;
        let spacing = theme::spacing();
        let row_spacing = spacing.space_xxs;
        let left_padding = spacing.space_s;
        let nav = widget::segmented_button::vertical(&self.nav)
            .on_activate(|id| cosmic::Action::Cosmic(cosmic::app::Action::NavBar(id)))
            .on_context(|id| cosmic::Action::Cosmic(cosmic::app::Action::NavBarContext(id)))
            .context_menu(Some(self.sidebar_context_menu()))
            .close_icon(
                widget::icon::from_name("view-refresh-symbolic")
                    .size(16)
                    .icon(),
            )
            .on_close(|id| cosmic::Action::App(Message::RefreshSidebarAccount(id)))
            .window_id_maybe(self.core.main_window_id())
            .on_surface_action(|action| cosmic::Action::App(Message::MenuSurface(action)))
            .button_height(ROW_HEIGHT)
            .button_padding([left_padding, row_spacing, left_padding, row_spacing])
            .button_spacing(row_spacing)
            .spacing(row_spacing)
            .style(theme::SegmentedButton::NavBar);

        let mut indicators =
            widget::column::with_capacity(self.sidebar.rows.len()).spacing(row_spacing);
        let height = Length::Fixed(f32::from(ROW_HEIGHT));
        for row in &self.sidebar.rows {
            let indicator: Element<'_, cosmic::Action<Message>> = if row.syncing {
                widget::container(widget::indeterminate_circular().size(16.0))
                    .padding([0, 0, 0, left_padding])
                    .center_y(height)
                    .into()
            } else {
                widget::space::vertical().height(height).into()
            };
            indicators = indicators.push(indicator);
        }

        let content = cosmic::iced::widget::stack(vec![nav.into(), indicators.into()]);
        let nav = widget::container(
            widget::scrollable(widget::container(content).padding(row_spacing))
                .class(cosmic::style::iced::Scrollable::Minimal)
                .height(Length::Fill),
        )
        .width(Length::Shrink)
        .height(Length::Fill)
        .class(theme::Container::custom(widget::nav_bar::nav_bar_style));
        if self.core.is_condensed() {
            nav.into()
        } else {
            nav.max_width(280).into()
        }
    }

    pub(super) fn refresh_calendar_nav(&mut self) {
        let rows = self.sidebar.build_rows(
            &self.storage.lock().unwrap(),
            &self.accounts,
            &self.active_accounts,
            &self.account_sync,
            &self.config,
        );
        self.sidebar.update_nav(&mut self.nav, rows);
    }

    pub(super) fn update_sidebar(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Message>> {
        match self.nav.data::<Item>(id).cloned() {
            Some(Item::Group(Group::Account(account_id)))
                if self
                    .account_sync_status(account_id)
                    .and_then(SyncStatus::error)
                    .is_some() =>
            {
                self.show_context_page(ContextPage::Settings);
                Task::none()
            }
            Some(Item::Group(group)) => {
                if !self.sidebar.collapsed.remove(&group) {
                    self.sidebar.collapsed.insert(group);
                }
                self.refresh_calendar_nav();
                Task::none()
            }
            Some(Item::Calendar(CalendarKey::Local(calendar_id))) => {
                let visible = self
                    .storage
                    .lock()
                    .unwrap()
                    .get_calendar(calendar_id)
                    .is_some_and(|calendar| !calendar.visible);
                self.update(Message::CalendarManagement(
                    CalendarManagementMessage::ToggleCalendarVisibility(calendar_id, visible),
                ))
            }
            Some(Item::Calendar(CalendarKey::Remote {
                account_id,
                calendar_id,
            })) => {
                let mut hidden = self.config.hidden_remote_calendars.clone();
                let calendars = hidden.entry(account_id).or_default();
                if !calendars.remove(&calendar_id) {
                    calendars.insert(calendar_id);
                }
                if calendars.is_empty() {
                    hidden.remove(&account_id);
                }
                let mut config = self.config.clone();
                match cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                    .and_then(|context| config.set_hidden_remote_calendars(&context, hidden))
                {
                    Ok(_) => {
                        self.config = config;
                        self.refresh_calendar_visibility();
                        Task::none()
                    }
                    Err(error) => {
                        self.toast(fl!("calendar-visibility-error", error = error.to_string()))
                    }
                }
            }
            None => Task::none(),
        }
    }

    pub(super) fn refresh_sidebar_account(
        &mut self,
        id: nav_bar::Id,
    ) -> Task<cosmic::Action<Message>> {
        match self.nav.data::<Item>(id) {
            Some(Item::Group(Group::Account(account_id))) if self.nav.is_closable(id) => {
                self.update(Message::Accounts(AccountsAction::Sync(*account_id)))
            }
            _ => Task::none(),
        }
    }

    pub(super) fn sidebar_context_menu(&self) -> Vec<menu::Tree<cosmic::Action<Message>>> {
        menu::nav_context(
            &HashMap::new(),
            self.nav
                .iter()
                .map(|id| match self.nav.data::<Item>(id) {
                    Some(Item::Calendar(CalendarKey::Local(id))) if !self.storage_read_only => {
                        vec![
                            menu::Item::Button(
                                fl!("rename"),
                                None,
                                MenuAction::RenameCalendar(*id),
                            ),
                            menu::Item::Button(
                                fl!("delete-calendar"),
                                None,
                                MenuAction::DeleteCalendar(*id),
                            ),
                        ]
                    }
                    Some(Item::Group(Group::Local)) if !self.storage_read_only => {
                        vec![menu::Item::Button(
                            fl!("new-calendar"),
                            None,
                            MenuAction::NewCalendar,
                        )]
                    }
                    _ => Vec::new(),
                })
                .collect(),
        )
    }

    pub(super) fn refresh_calendar_visibility(&mut self) {
        let storage = self.storage.lock().unwrap();
        if let Some((id, _)) = self.calendar.selected_event {
            if !storage
                .get_event(id)
                .is_some_and(|event| self.event_is_visible(&storage, event))
            {
                self.calendar.selected_event = None;
                self.calendar_click = None;
                self.calendar_popup = None;
            }
        }
        drop(storage);
        self.refresh_calendar_nav();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuAction {
    NewCalendar,
    RenameCalendar(CalendarId),
    DeleteCalendar(CalendarId),
}

impl menu::action::MenuAction for MenuAction {
    type Message = cosmic::Action<Message>;

    fn message(&self) -> Self::Message {
        use CalendarManagementMessage as Calendar;
        cosmic::Action::App(match *self {
            Self::NewCalendar => Message::CalendarManagement(Calendar::OpenNewCalendarForm),
            Self::RenameCalendar(id) => {
                Message::CalendarManagement(Calendar::StartEditCalendar(id))
            }
            Self::DeleteCalendar(id) => Message::CalendarManagement(Calendar::DeleteCalendar(id)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::accounts::{Calendar, Snapshot};

    #[test]
    fn lists_accounts_directly_and_keeps_visibility_per_account() {
        let account = |provider: &str| Account {
            id: Uuid::new_v4(),
            email: (provider == "google").then(|| "person@example.test".into()),
            provider: provider.into(),
            server: (provider == "custom-caldav").then(|| "dav.example.test".into()),
        };
        let accounts = [
            account("google"),
            account("google"),
            account("custom-caldav"),
            account("microsoft"),
        ];
        let [first, second, caldav, inactive] = accounts.each_ref().map(|account| account.id);
        let mut storage = LocalStorage::unavailable("test".into());
        let collection = "https://example.test/calendars/personal/";
        for account in &accounts {
            storage.replace_remote_snapshot(
                account.id,
                Snapshot {
                    calendars: vec![Calendar {
                        id: collection.into(),
                        name: "Personal".into(),
                        sync_token: None,
                    }],
                    events: Vec::new(),
                },
            );
        }

        let mut config = Config::default();
        config
            .hidden_remote_calendars
            .insert(first, BTreeSet::from([collection.into()]));
        let active = BTreeSet::from([first, second, caldav]);
        let mut sidebar = Sidebar::default();
        let rows = sidebar.build_rows(&storage, &accounts, &active, &HashMap::new(), &config);
        let remote: Vec<_> = rows
            .iter()
            .filter_map(|row| match (&row.item, &row.icon) {
                (
                    Item::Calendar(CalendarKey::Remote { account_id, .. }),
                    Icon::Calendar { visible, .. },
                ) => Some((*account_id, *visible)),
                _ => None,
            })
            .collect();
        let mut google = [(first, false), (second, true)];
        google.sort_by_key(|(id, _)| *id);
        assert_eq!(remote, [(caldav, true), google[0], google[1]]);
        assert_eq!(rows.len(), 7);
        assert!(matches!(rows[0].icon, Icon::LocalCalendars));
        assert!(matches!(rows[1].icon, Icon::Account));
        assert_eq!(rows[1].item, Item::Group(Group::Account(caldav)));
        assert_eq!(rows[1].label, "dav.example.test");
        assert_eq!(rows[3].label, "person@example.test");
        assert_eq!(rows[1].indent, 0);
        assert_eq!(rows[2].indent, 1);
        assert!(!rows
            .iter()
            .any(|row| row.item == Item::Group(Group::Account(inactive))));

        sidebar.collapsed.insert(Group::Account(first));
        let rows = sidebar.build_rows(&storage, &accounts, &active, &HashMap::new(), &config);
        assert!(!rows.iter().any(|row| matches!(
            row.item,
            Item::Calendar(CalendarKey::Remote { account_id, .. }) if account_id == first
        )));
        assert!(rows.iter().any(|row| matches!(
            row.item,
            Item::Calendar(CalendarKey::Remote { account_id, .. }) if account_id == second
        )));
    }

    #[test]
    fn failed_accounts_can_retry_without_showing_calendars() {
        use crate::features::accounts::Error;
        use std::time::Instant;

        let id = Uuid::new_v4();
        let accounts = [Account {
            id,
            email: Some("test@example.test".into()),
            provider: "google".into(),
            server: None,
        }];
        let mut storage = LocalStorage::unavailable("test".into());
        storage.replace_remote_snapshot(
            id,
            Snapshot {
                calendars: vec![Calendar {
                    id: "collection".into(),
                    name: "Personal".into(),
                    sync_token: None,
                }],
                events: Vec::new(),
            },
        );
        let mut sidebar = Sidebar::default();
        let mut nav = nav_bar::Model::default();
        let active = BTreeSet::from([id]);
        let config = Config::default();
        for (status, refresh) in [
            (
                SyncStatus::Finished(Instant::now(), Some(Error::Failed)),
                true,
            ),
            (
                SyncStatus::Running {
                    request_id: 1,
                    previous_error: Some(Error::Failed),
                },
                false,
            ),
        ] {
            let rows = sidebar.build_rows(
                &storage,
                &accounts,
                &active,
                &HashMap::from([(id, status)]),
                &config,
            );
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[1].icon, Icon::AccountWarning));
            assert_eq!(rows[1].syncing, !refresh);
            sidebar.update_nav(&mut nav, rows);
            let account = nav.iter().nth(1).unwrap();
            assert!(nav.is_enabled(account));
            assert_eq!(nav.is_closable(account), refresh);
        }

        let status = HashMap::from([(id, SyncStatus::Finished(Instant::now(), None))]);
        let rows = sidebar.build_rows(&storage, &accounts, &active, &status, &config);
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[1].icon, Icon::Account));
        sidebar.update_nav(&mut nav, rows);
        assert!(nav.is_enabled(nav.iter().nth(1).unwrap()));
    }
}
