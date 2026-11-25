// SPDX-License-Identifier: GPL-3.0-only

use crate::features::accounts::{self as accounts_feature, AccountsAction};
use crate::features::calendar::{
    CalendarAction, CalendarAnchor, CalendarMessage, CalendarSlot, EventSelection, LocalCalendar,
};
use crate::features::calendar_management::model::{CalendarKey, LocalCalendarMeta};
use crate::features::calendar_management::{
    state::CalendarForm, view as calendar_form, CalendarManagementMessage,
};
use crate::features::event::location::{
    photon_api_url, LocationSuggestions, PUBLIC_PHOTON_ENDPOINT,
};
use crate::features::event::model::{Event, EventTime};
use crate::features::event::state::EventEditorState;
use crate::features::event::EventMessage;
use crate::features::ics::IcsMessage;
use crate::fl;
use crate::config::{AppTheme, Config};
use crate::shared::ids::{CalendarId, EventId};
use crate::shared::storage::LocalStorage;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Length, Subscription, window};
use cosmic::prelude::*;
use cosmic::theme::spacing;
use cosmic::widget::{color_picker::ColorPickerUpdate, segmented_button::SingleSelect};
use cosmic::widget::{self, about::About, menu, nav_bar};
use cosmic::{cosmic_theme, theme};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

mod flags;
pub use flags::*;
mod settings;
mod shortcuts;
mod sidebar;
pub use settings::*;
mod update;

use crate::features::event::notifications::{NotificationMessage, CHECK_INTERVAL};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

fn about_info() -> About {
    let links = vec![
        (fl!("repository"), REPOSITORY.to_owned()),
        (
            fl!("about-releases"),
            concat!(env!("CARGO_PKG_REPOSITORY"), "/releases").to_owned(),
        ),
        (
            fl!("about-report-issue"),
            concat!(env!("CARGO_PKG_REPOSITORY"), "/issues").to_owned(),
        ),
    ];

    About::default()
        .name(fl!("app-title"))
        .comments(fl!("about-description"))
        .author("Eduardo Flores")
        .developers([("Eduardo Flores", "edfloreshz@gmail.com")])
        .icon(widget::icon::from_svg_bytes(APP_ICON))
        .version(env!("CARGO_PKG_VERSION"))
        .license(env!("CARGO_PKG_LICENSE"))
        .license_url("https://www.gnu.org/licenses/gpl-3.0.html")
        .links(links)
}

#[derive(Default)]
struct IcsImportState {
    path: Option<PathBuf>,
    calendar: Option<CalendarId>,
    busy: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalendarClick {
    Event(EventSelection),
    EmptySlot(CalendarSlot),
}

enum CalendarPopup {
    Details(EventSelection, Box<Event>, Option<String>),
}

struct OpenCalendarPopup {
    content: CalendarPopup,
    anchor: Option<CalendarAnchor>,
}

#[derive(Clone)]
enum CalendarClipboard {
    Copy(Event),
    Cut(EventSelection),
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    about: About,
    /// Display a context drawer with the designated page if defined.
    context_page: ContextPage,
    sidebar_before_agenda: Option<bool>,
    /// Main window width reported by COSMIC.
    window_width: f32,
    /// Contains items assigned to the nav bar panel.
    nav: nav_bar::Model,
    /// Contains items assigned to the tab bar.
    tabs: widget::segmented_button::Model<SingleSelect>,
    /// Key bindings for the application's menu bar.
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    // Configuration data that persists between application runs.
    config: Config,
    display_timezone_options: Vec<String>,
    ics_import: IcsImportState,
    // Calendar data that persists between application runs.
    calendar: LocalCalendar,
    calendar_popup: Option<OpenCalendarPopup>,
    calendar_clipboard: Option<CalendarClipboard>,
    calendar_click: Option<(CalendarClick, std::time::Instant)>,
    // Local calendar/event storage, persisted to disk as JSON.
    storage: Arc<Mutex<LocalStorage>>,
    toasts: widget::Toasts<Message>,
    storage_read_only: bool,
    calendar_to_archive: Option<(CalendarId, String)>,
    calendar_archive_busy: bool,
    // State of the event editor dialog, when open.
    editing_event: Option<EventEditorState>,
    event_notes: widget::text_editor::Content,
    event_locations: LocationSuggestions,
    location_lookup: Option<(uuid::Uuid, cosmic::iced::task::Handle)>,
    photon_endpoint_input: Option<String>,
    calendar_form: Option<CalendarForm>,
    // Account groups and the rows currently shown in the calendar sidebar.
    sidebar: sidebar::Sidebar,
    // Current text in the header search bar; non-empty replaces the main
    // view with search results.
    search_query: String,
    checking_alerts: bool,
    keep_running: bool,
    // Accounts-backed calendar sources. The embedded manager owns account
    // state; these fields are a local cache used by Settings and filtering.
    accounts: Vec<accounts_feature::Account>,
    accounts_error: Option<accounts_feature::Error>,
    accounts_checked: bool,
    watched_account_ids: Vec<uuid::Uuid>,
    active_accounts: BTreeSet<uuid::Uuid>,
    account_sync: HashMap<uuid::Uuid, accounts_feature::SyncStatus>,
    next_account_sync: u64,
    discovering_accounts: bool,
    account_auth: Option<accounts_feature::AuthRequest>,
    account_form: Option<accounts_feature::AccountForm>,
    account_action_error: Option<String>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    MenuSurface(cosmic::surface::Action),
    Shortcut(shortcuts::Action),
    RefreshSidebarAccount(nav_bar::Id),
    Quit,
    ToggleContextPage(ContextPage),
    LaunchUrl(String),
    CloseToast(widget::ToastId),
    MainWindowClosed,
    MainWindowOpened,
    Calendar(CalendarMessage),
    Event(EventMessage),
    CalendarManagement(CalendarManagementMessage),
    Ics(IcsMessage),
    Settings(SettingsMessage),
    Notifications(NotificationMessage),
    Accounts(AccountsAction),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = Flags;

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "dev.edfloreshz.Calendar";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Create a nav bar with three page items.
        let nav = nav_bar::Model::default();

        // Create a tab bar with three page items.
        let tabs = widget::segmented_button::Model::builder()
            .insert(|b| {
                b.text(fl!("month"))
                    .icon(widget::icon::from_name("office-calendar-symbolic"))
                    .data(Tab::Month)
                    .activate()
            })
            .insert(|b| {
                b.text(fl!("week"))
                    .icon(widget::icon::from_name("x-office-spreadsheet-symbolic"))
                    .data(Tab::Week)
            })
            .insert(|b| {
                b.text(fl!("day"))
                    .icon(widget::icon::from_name("calendar-go-today-symbolic"))
                    .data(Tab::Day)
            })
            .build();

        let mut display_timezone_options = chrono_tz::TZ_VARIANTS
            .iter()
            .map(|zone| zone.name().to_owned())
            .collect::<Vec<_>>();
        display_timezone_options.sort_unstable();
        display_timezone_options.insert(0, fl!("display-timezone-system"));

        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            about: about_info(),
            context_page: ContextPage::default(),
            sidebar_before_agenda: None,
            window_width: 0.0,
            nav,
            tabs,
            key_binds: shortcuts::menu_bindings(),
            // Optional configuration file for an application.
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    }
                })
                .unwrap_or_default(),
            display_timezone_options,
            ics_import: IcsImportState::default(),
            calendar: LocalCalendar::default(),
            calendar_popup: None,
            calendar_clipboard: None,
            calendar_click: None,
            storage: Arc::new(Mutex::new(LocalStorage::load().unwrap_or_else(|err| {
                tracing::error!("failed to load local storage: {err}");
                LocalStorage::unavailable(err.to_string())
            }))),
            toasts: widget::Toasts::new(Message::CloseToast),
            storage_read_only: false,
            calendar_to_archive: None,
            calendar_archive_busy: false,
            editing_event: None,
            event_notes: widget::text_editor::Content::new(),
            event_locations: LocationSuggestions::new([], ""),
            location_lookup: None,
            photon_endpoint_input: None,
            calendar_form: None,
            sidebar: sidebar::Sidebar::default(),
            search_query: String::new(),
            checking_alerts: false,
            keep_running: flags.keep_running,
            accounts: Vec::new(),
            accounts_error: None,
            accounts_checked: false,
            watched_account_ids: Vec::new(),
            active_accounts: BTreeSet::new(),
            account_sync: HashMap::new(),
            next_account_sync: 0,
            discovering_accounts: false,
            account_auth: None,
            account_form: None,
            account_action_error: None,
        };

        let mut tasks = Vec::new();
        // Seed a default calendar only if the store has no calendar records.
        {
            let mut storage = app.storage.lock().unwrap();
            if let Some(error) = storage.load_error() {
                app.storage_read_only = true;
                let toast = widget::Toast::new(fl!("calendar-storage-load-error", error = error))
                    .duration(widget::toaster::Duration::Long);
                tasks.push(app.toasts.push(toast).map(cosmic::action::app));
            } else if !storage.has_calendars() {
                let personal = LocalCalendarMeta {
                    id: crate::shared::ids::CalendarId::new(),
                    name: fl!("personal-calendar"),
                    description: None,
                    color: None,
                    visible: true,
                    archived: false,
                };
                if let Err(err) = storage.create_calendar(personal) {
                    let toast =
                        widget::Toast::new(fl!("default-calendar-error", error = err.to_string()));
                    tasks.push(app.toasts.push(toast).map(cosmic::action::app));
                    app.calendar_form = Some(CalendarForm::new(None, String::new(), None));
                }
            }
        }

        app.refresh_calendar_nav();

        if app.keep_running {
            tasks.push(app.open_main_window());
        } else {
            tasks.push(app.update_title());
        }
        tasks.push(cosmic::task::message(Message::Notifications(
            NotificationMessage::CheckAlerts,
        )));
        tasks.push(cosmic::task::message(Message::Accounts(
            AccountsAction::Discover,
        )));
        tasks.push(cosmic::command::set_theme(app.config.app_theme.theme()));

        app.refresh_display_timezone();
        app.calendar.set_today();

        (app, Task::batch(tasks))
    }

    /// Elements to pack at the start of the header bar.
    fn header_start<'a>(&'a self) -> Vec<Element<'a, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            widget::RcElementWrapper::new(
                widget::button::icon(widget::icon::from_name("open-menu-symbolic"))
                    .padding([4, 12])
                    .class(theme::Button::MenuRoot)
                    .name(fl!("app-menu"))
                    .into(),
            ),
            menu::items(
                &self.key_binds,
                vec![
                    menu::Item::Button(fl!("new-calendar"), None, MenuAction::NewCalendar),
                    menu::Item::Button(fl!("import-ics"), None, MenuAction::ImportIcs),
                    menu::Item::Button(fl!("export-ics"), None, MenuAction::ExportIcs),
                    menu::Item::Button(
                        fl!("archived-calendars"),
                        None,
                        MenuAction::ArchivedCalendars,
                    ),
                    menu::Item::Button(fl!("settings"), None, MenuAction::Settings),
                    menu::Item::Divider,
                    menu::Item::Button(fl!("keyboard-shortcuts"), None, MenuAction::Shortcuts),
                    menu::Item::Button(fl!("about"), None, MenuAction::About),
                    menu::Item::Divider,
                    menu::Item::Button(fl!("shortcut-quit"), None, MenuAction::Quit),
                ],
            ),
        )])
        .item_height(menu::ItemHeight::Dynamic(40))
        .item_width(menu::ItemWidth::Uniform(320))
        .spacing(spacing().space_xxxs.into())
        .window_id_maybe(self.core.main_window_id())
        .on_surface_action(Message::MenuSurface);

        vec![menu_bar.into()]
    }

    fn header_center<'a>(&'a self) -> Vec<Element<'a, Self::Message>> {
        let width = if self.window_width > 0.0 {
            (self.window_width / self.core.scale_factor() - 260.0).clamp(100.0, 240.0)
        } else {
            240.0
        };
        vec![widget::search_input(fl!("search"), &self.search_query)
            .id(shortcuts::search_id())
            .on_input(|value| Message::Calendar(CalendarMessage::SearchChanged(value)))
            .on_clear(Message::Calendar(CalendarMessage::SearchChanged(
                String::new(),
            )))
            .width(Length::Fixed(width))
            .into()]
    }

    fn header_end<'a>(&'a self) -> Vec<Element<'a, Self::Message>> {
        let create_event = widget::button::icon(widget::icon::from_name("list-add-symbolic"))
            .name(fl!("create-event"))
            .on_press(self.new_event_message());

        vec![
            widget::tooltip(
                create_event,
                widget::text(fl!("create-event")),
                widget::tooltip::Position::Bottom,
            )
            .into(),
        ]
    }

    fn footer<'a>(&'a self) -> Option<Element<'a, Self::Message>> {
        let days = widget::row(vec![
            widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::Calendar(CalendarMessage::NavigatePreviousDay))
                .into(),
            widget::dropdown(
                self.calendar
                    .days()
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<String>>(),
                self.calendar
                    .days()
                    .iter()
                    .position(|m| *m == self.calendar.selected_date.day()),
                |value| Message::Calendar(CalendarMessage::SelectDay(value)),
            )
            .into(),
            widget::button::icon(widget::icon::from_name("go-next-symbolic"))
                .on_press(Message::Calendar(CalendarMessage::NavigateNextDay))
                .into(),
        ])
        .align_y(Vertical::Center)
        .spacing(spacing().space_xxs);

        let months = widget::row(vec![
            widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .tooltip(fl!("previous-month"))
                .name(fl!("previous-month"))
                .on_press(Message::Calendar(CalendarMessage::NavigatePreviousMonth))
                .into(),
            widget::dropdown(
                self.calendar
                    .months()
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<String>>(),
                self.calendar
                    .months()
                    .iter()
                    .position(|m| *m == self.calendar.selected_date.month()),
                |value| Message::Calendar(CalendarMessage::SelectMonth(value)),
            )
            .into(),
            widget::button::icon(widget::icon::from_name("go-next-symbolic"))
                .tooltip(fl!("next-month"))
                .name(fl!("next-month"))
                .on_press(Message::Calendar(CalendarMessage::NavigateNextMonth))
                .into(),
        ])
        .align_y(Vertical::Center)
        .spacing(spacing().space_xxs);

        let years = widget::row(vec![
            widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::Calendar(CalendarMessage::NavigatePreviousYear))
                .into(),
            widget::dropdown(
                self.calendar
                    .years()
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<String>>(),
                self.calendar
                    .years()
                    .iter()
                    .position(|y| *y == self.calendar.selected_date.year()),
                |value| Message::Calendar(CalendarMessage::SelectYear(value)),
            )
            .into(),
            widget::button::icon(widget::icon::from_name("go-next-symbolic"))
                .on_press(Message::Calendar(CalendarMessage::NavigateNextYear))
                .into(),
        ])
        .align_y(Vertical::Center)
        .spacing(spacing().space_xxs);

        let today_icon = widget::icon::from_svg_bytes(
            include_bytes!("../resources/icons/calendar-today-symbolic.svg").as_slice(),
        )
        .symbolic(true);
        let today_width = f32::from(spacing().space_l);
        let today = widget::button::icon(today_icon)
            .tooltip(fl!("today"))
            .name(fl!("today"))
            .width(today_width)
            .class(cosmic::style::Button::NavToggle)
            .on_press_maybe(
                (!self.calendar.today())
                    .then_some(Message::Calendar(CalendarMessage::NavigateToday)),
            );
        let navigation = widget::flex_row(vec![days.into(), months.into(), years.into()])
            .width(Length::Fill)
            .justify_content(widget::JustifyContent::Center)
            .justify_items(Alignment::Center)
            .spacing(spacing().space_xs);

        let navigation = widget::container(navigation)
            .padding([0.0, today_width + f32::from(spacing().space_xxs)])
            .width(Length::Fill);
        let actions = widget::row(vec![widget::space::horizontal().into(), today.into()])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Vertical::Center);
        let footer = cosmic::iced::widget::stack(vec![navigation.into(), actions.into()])
            .width(Length::Fill)
            .height(Length::Shrink);

        Some(
            widget::container(footer)
            .class(cosmic::theme::Container::Card)
            .padding(spacing().space_xxxs)
            .width(Length::Fill)
            .into(),
        )
    }

    /// Enables the COSMIC application to create a nav bar with this model.
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        self.sidebar_before_agenda.is_none().then_some(&self.nav)
    }

    fn nav_bar(&self) -> Option<Element<'_, cosmic::Action<Message>>> {
        if !self.core.nav_bar_active() {
            return None;
        }
        self.nav_model()?;
        Some(self.sidebar_view())
    }

    /// Display a context drawer if the context page is requested.
    fn context_drawer<'a>(&'a self) -> Option<context_drawer::ContextDrawer<'a, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::DayAgenda => {
                let storage = self.storage.lock().unwrap();
                context_drawer::context_drawer(
                    self.calendar.day_agenda(&storage, self),
                    Message::Calendar(CalendarMessage::CloseDayAgenda),
                )
                .title(fl!("month-day-agenda"))
            }
            ContextPage::Settings => context_drawer::context_drawer(
                self.settings_view(),
                Message::ToggleContextPage(ContextPage::Settings),
            )
            .title(fl!("settings")),
            ContextPage::IcsImport => context_drawer::context_drawer(
                self.ics_import_view(),
                Message::ToggleContextPage(ContextPage::IcsImport),
            )
            .title(fl!("import-ics")),
            ContextPage::Shortcuts => context_drawer::context_drawer(
                shortcuts::view(),
                Message::ToggleContextPage(ContextPage::Shortcuts),
            )
            .title(fl!("keyboard-shortcuts")),
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_owned()),
                Message::ToggleContextPage(ContextPage::About),
            )
            .title(fl!("about")),
            ContextPage::ArchivedCalendars => context_drawer::context_drawer(
                self.archived_calendars_view(),
                Message::ToggleContextPage(ContextPage::ArchivedCalendars),
            )
            .title(fl!("archived-calendars")),
        })
    }

    /// Displays calendar creation, calendar deletion confirmation, or the event editor.
    fn dialog<'a>(&'a self) -> Option<Element<'a, Self::Message>> {
        if let Some((_, name)) = &self.calendar_to_archive {
            return Some(
                widget::dialog::dialog()
                    .title(fl!("delete-calendar"))
                    .body(fl!("archive-calendar-confirmation", name = name.as_str()))
                    .primary_action(widget::button::destructive(fl!("delete")).on_press_maybe(
                        (!self.calendar_archive_busy).then_some(Message::CalendarManagement(
                            CalendarManagementMessage::ConfirmArchiveCalendar,
                        )),
                    ))
                    .secondary_action(widget::button::standard(fl!("cancel")).on_press_maybe(
                        (!self.calendar_archive_busy).then_some(Message::CalendarManagement(
                            CalendarManagementMessage::CancelArchiveCalendar,
                        )),
                    ))
                    .width(Length::Fixed(420.0))
                    .into(),
            );
        }
        if let Some(form) = &self.calendar_form {
            let (title, action) = if form.id.is_some() {
                (fl!("edit-calendar"), fl!("save"))
            } else {
                (fl!("new-calendar"), fl!("create"))
            };
            return Some(widget::toaster(
                &self.toasts,
                widget::dialog::dialog()
                    .title(title)
                    .control(widget::scrollable(calendar_form::view(form)).width(Length::Fill))
                    .primary_action(widget::button::suggested(action).on_press_maybe(
                        form.can_save().then_some(Message::CalendarManagement(
                            CalendarManagementMessage::SaveCalendarForm,
                        )),
                    ))
                    .secondary_action(widget::button::standard(fl!("cancel")).on_press_maybe(
                        (!form.writing).then_some(Message::CalendarManagement(
                            CalendarManagementMessage::CloseCalendarForm,
                        )),
                    ))
                    .width(Length::Fixed(if form.picker.get_is_active() {
                        620.0
                    } else {
                        380.0
                    })),
            ));
        }
        self.editing_event
            .as_ref()
            .map(|_| widget::toaster(&self.toasts, self.event_editor_view()))
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// Application events will be processed through the view. Any messages emitted by
    /// events received by widgets will be passed to the update method.
    fn view<'a>(&'a self) -> Element<'a, Self::Message> {
        let tabs = widget::segmented_button::horizontal(&self.tabs)
            .padding(2)
            .button_spacing(spacing().space_xxs)
            .button_alignment(cosmic::iced::Alignment::Center)
            .on_activate(|value| Message::Calendar(CalendarMessage::TabSelected(value)))
            .into();

        let active_tab = if !self.search_query.is_empty() {
            self.search_results_view()
        } else {
            match self.tabs.active_data::<Tab>() {
                Some(Tab::Month) => self
                    .calendar
                    .month_view(&self.storage.lock().unwrap(), self),
                Some(tab @ (Tab::Week | Tab::Day)) => {
                    let storage = self.storage.lock().unwrap();
                    if matches!(tab, Tab::Week) {
                        self.calendar.week_view(&storage, self).into()
                    } else {
                        self.calendar.day_view(&storage, self).into()
                    }
                }
                None => widget::text::title1("Welcome")
                    .apply(widget::container)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Center)
                    .align_y(Vertical::Center)
                    .into(),
            }
        };

        let content = widget::column(vec![tabs, active_tab]).spacing(spacing().space_xxs);
        let calendar = self.attach_calendar_popup(content, None);
        if self.editing_event.is_some() || self.calendar_form.is_some() {
            calendar
        } else {
            widget::toaster(&self.toasts, calendar)
        }
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-running async tasks running in the background which
    /// emit messages to the application through a channel. They are started at the
    /// beginning of the application, and persist through its lifetime.
    fn subscription(&self) -> Subscription<Self::Message> {
        let alerts_tick = cosmic::iced::time::every(CHECK_INTERVAL)
            .map(|_| Message::Notifications(NotificationMessage::CheckAlerts));

        let config_updates = self
            .core()
            .watch_config::<Config>(Self::APP_ID)
            .map(|update| Message::Settings(SettingsMessage::UpdateConfig(update.config)));

        let keyboard_shortcuts =
            shortcuts::subscription(self.core.main_window_id(), self.shortcut_scope())
                .map(Message::Shortcut);
        let accounts =
            accounts_feature::subscription::subscription(self.watched_account_ids.clone());
        Subscription::batch([alerts_tick, config_updates, keyboard_shortcuts, accounts])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Shortcut(action) => return self.update_shortcut(action),
            Message::RefreshSidebarAccount(id) => return self.refresh_sidebar_account(id),
            Message::Quit => return cosmic::iced::exit(),
            Message::Calendar(message) => return self.update_calendar(message),
            Message::Event(message) => return self.update_event(message),
            Message::CalendarManagement(message) => {
                return self.update_calendar_management(message)
            }
            Message::Ics(message) => return self.update_ics(message),
            Message::Settings(message) => return self.update_settings(message),
            Message::Notifications(message) => return self.update_notifications(message),
            Message::Accounts(action) => return self.update_accounts(action),
            Message::MenuSurface(action) => return cosmic::surface::surface_task(action),
            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page && self.core.window.show_context {
                    if !self.close_day_agenda() {
                        self.core.set_show_context(false);
                    }
                } else if context_page == ContextPage::DayAgenda {
                    self.open_day_agenda();
                } else {
                    self.show_context_page(context_page);
                }
            }
            Message::LaunchUrl(url) => match open::that_detached(&url) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("failed to open {url:?}: {err}");
                }
            },
            Message::CloseToast(id) => self.toasts.remove(id),
            Message::MainWindowClosed => {
                self.core.set_main_window_id(None);
                self.calendar_popup = None;
            }
            Message::MainWindowOpened => return self.update_title(),
        }
        self.update_drawer_layout();
        Task::none()
    }

    fn system_theme_update(
        &mut self,
        _keys: &[&'static str],
        _new_theme: &cosmic_theme::Theme,
    ) -> Task<cosmic::Action<Message>> {
        cosmic::command::set_theme(self.config.app_theme.theme())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        (self.keep_running && self.core.main_window_is(id)).then_some(Message::MainWindowClosed)
    }

    fn dbus_activation(
        &mut self,
        _message: cosmic::dbus_activation::Message,
    ) -> Task<cosmic::Action<Message>> {
        if let Some(id) = self.core.main_window_id() {
            return window::gain_focus(id);
        }
        self.open_main_window()
    }

    fn on_window_resize(&mut self, id: cosmic::iced::window::Id, width: f32, _height: f32) {
        if self.core.main_window_is(id) {
            self.window_width = width;
            self.update_drawer_layout();
        }
    }

    fn on_escape(&mut self) -> Task<cosmic::Action<Self::Message>> {
        if self.editing_event.is_some() {
            return self.update(Message::Event(EventMessage::EscapeEventEditor));
        }
        if self.calendar_to_archive.is_some() {
            return cosmic::task::message(Message::CalendarManagement(
                CalendarManagementMessage::CancelArchiveCalendar,
            ));
        }
        if let Some(form) = &self.calendar_form {
            let message = if form.picker.get_is_active() {
                Message::CalendarManagement(CalendarManagementMessage::CalendarColorPicker(
                    ColorPickerUpdate::Cancel,
                ))
            } else {
                Message::CalendarManagement(CalendarManagementMessage::CloseCalendarForm)
            };
            return cosmic::task::message(message);
        }
        if self.account_form.is_some() {
            return cosmic::task::message(Message::Accounts(AccountsAction::CancelForm));
        }
        if let Some(request) = self.account_auth.as_ref() {
            return cosmic::task::message(Message::Accounts(AccountsAction::CancelAuth(
                request.state.clone(),
            )));
        }
        if self.calendar_popup.take().is_some() {
            return Task::none();
        }
        if self.core.window.show_context {
            return match self.context_page {
                ContextPage::DayAgenda => {
                    cosmic::task::message(Message::Calendar(CalendarMessage::CloseDayAgenda))
                }
                page => cosmic::task::message(Message::ToggleContextPage(page)),
            };
        }
        if !self.search_query.is_empty() {
            return cosmic::task::message(Message::Calendar(CalendarMessage::SearchChanged(
                String::new(),
            )));
        }
        Task::none()
    }

    /// Called when a nav item is selected.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.update_sidebar(id)
    }
}

impl AppModel {
    fn toast(&mut self, message: String) -> Task<cosmic::Action<Message>> {
        self.toasts
            .push(widget::Toast::new(message))
            .map(cosmic::action::app)
    }

    fn new_event_message(&self) -> Message {
        let selected_slot = self.calendar.selected_slot.or_else(|| {
            self.show_month()
                .then(|| CalendarSlot::MonthDay(self.calendar.selected_date.date()))
        });
        selected_slot.map_or(
            Message::Event(EventMessage::AddEvent(self.calendar.current_date)),
            |slot| self.create_event_at_slot(slot),
        )
    }

    fn create_event_at_slot(&self, slot: CalendarSlot) -> Message {
        Message::Event(EventMessage::OpenEventEditor(
            None,
            slot.start(self.calendar.selected_date.offset()),
            matches!(slot, CalendarSlot::AllDay(_)),
        ))
    }

    fn scroll_calendar_to_now(&self) -> Task<cosmic::Action<Message>> {
        if self.search_query.is_empty() {
            match self.tabs.active_data::<Tab>() {
                Some(Tab::Week) => return self.calendar.scroll_to_now(true),
                Some(Tab::Day) => return self.calendar.scroll_to_now(false),
                _ => {}
            }
        }
        Task::none()
    }

    fn calendar_double_click(&mut self, target: CalendarClick) -> bool {
        let now = std::time::Instant::now();
        let double_click = self.calendar_click.is_some_and(|(previous, at)| {
            target == previous && now.duration_since(at) <= std::time::Duration::from_millis(400)
        });
        self.calendar_click = (!double_click).then_some((target, now));
        double_click
    }

    fn open_event_details(&mut self, key: EventSelection, anchor: Option<CalendarAnchor>) {
        let storage = self.storage.lock().unwrap();
        self.calendar_popup = storage.get_event(key.0).map(|event| {
            let calendar = match event.calendar {
                CalendarKey::Local(id) => storage
                    .get_calendar(id)
                    .map(|calendar| calendar.name.clone()),
                CalendarKey::Remote { account_id, ref calendar_id } => storage
                    .remote_calendars(account_id)
                    .iter()
                    .find(|calendar| calendar.id == *calendar_id)
                    .map(|calendar| calendar.name.clone()),
            };
            OpenCalendarPopup {
                content: CalendarPopup::Details(key, Box::new(event.clone()), calendar),
                anchor,
            }
        });
    }

    pub(crate) fn attach_calendar_popup<'a>(
        &'a self,
        content: impl Into<Element<'a, Message>>,
        anchor: Option<CalendarAnchor>,
    ) -> Element<'a, Message> {
        let Some(popup) = self
            .calendar_popup
            .as_ref()
            .filter(|popup| popup.anchor == anchor)
        else {
            return content.into();
        };
        let position = if anchor.is_some() {
            widget::popover::Position::Bottom
        } else {
            widget::popover::Position::Center
        };
        widget::popover::popover(content)
            .position(position)
            .popup(self.calendar_popup_view(&popup.content))
            .on_close(Message::Calendar(CalendarMessage::CloseCalendarPopup))
            .into()
    }

    pub(crate) fn calendar_can_paste(&self) -> bool {
        self.calendar_clipboard.is_some()
    }

    pub(crate) fn compact_month_anchor(
        &self,
        date: time::Date,
        compact: bool,
    ) -> Option<CalendarAnchor> {
        if compact {
            if let Some(CalendarAnchor::Month(day, key)) =
                self.calendar_popup.as_ref().and_then(|popup| popup.anchor)
            {
                if date == day {
                    return Some(CalendarAnchor::Month(date, key));
                }
            }
        }
        None
    }

    fn calendar_popup_view(&self, popup: &CalendarPopup) -> Element<'_, Message> {
        let mut content = widget::column(Vec::new()).spacing(spacing().space_xxs);
        match popup {
            CalendarPopup::Details(key, event, calendar) => {
                let start = key.1;
                let when = match event.time {
                    EventTime::AllDay {
                        start: original,
                        end,
                    } => {
                        let first = start.date();
                        let last = first
                            .saturating_add((end - original).saturating_sub(time::Duration::DAY));
                        if last > first {
                            format!("{first} – {last} · {}", fl!("all-day"))
                        } else {
                            format!("{first} · {}", fl!("all-day"))
                        }
                    }
                    EventTime::Timed {
                        start: original,
                        end,
                    } => {
                        let display = |instant| {
                            if event.floating {
                                instant
                            } else {
                                crate::features::calendar::display_time(
                                    instant,
                                    self.calendar.display_timezone,
                                )
                            }
                        };
                        let first = display(start);
                        let last = display(start.saturating_add(end - original));
                        let clock = |date: OffsetDateTime| {
                            format!("{:02}:{:02}", date.hour(), date.minute())
                        };
                        if first == last {
                            format!("{} · {}", first.date(), clock(first))
                        } else if first.date() == last.date() {
                            format!("{} · {}–{}", first.date(), clock(first), clock(last))
                        } else {
                            format!(
                                "{} {} – {} {}",
                                first.date(),
                                clock(first),
                                last.date(),
                                clock(last)
                            )
                        }
                    }
                };
                content = content
                    .push(widget::text::title4(event.summary.clone()))
                    .push(widget::text::body(when));
                for value in [&event.location, &event.description].into_iter().flatten() {
                    content = content.push(widget::text::body(value.clone()));
                }
                if let Some(calendar) = calendar {
                    content = content.push(widget::text::caption(calendar.clone()));
                }
                if matches!(event.calendar, CalendarKey::Local(_)) {
                    content = content.push(widget::button::suggested(fl!("edit-event")).on_press(
                        Message::Calendar(CalendarMessage::EventAction(
                            CalendarAction::Edit,
                            Some(*key),
                            start,
                        )),
                    ));
                }
            }
        }
        widget::container(content)
            .width(Length::Fixed(240.0))
            .padding(spacing().space_s)
            .class(cosmic::theme::Container::Dialog(false))
            .into()
    }

    fn paste_occurrence(
        &self,
        source: CalendarClipboard,
        date: OffsetDateTime,
    ) -> crate::Result<EventSelection> {
        use chrono::TimeZone;
        let mut storage = self.storage.lock().unwrap();
        let (mut event, cut) = match source {
            CalendarClipboard::Copy(event) => (event, None),
            CalendarClipboard::Cut(key) => {
                let event = storage
                    .get_event(key.0)
                    .cloned()
                    .ok_or_else(|| crate::Error::Storage("event no longer exists".into()))?;
                if event.is_recurring()
                    && event
                        .occurrences(key.1..key.1.saturating_add(time::Duration::SECOND))
                        .first()
                        != Some(&key.1)
                {
                    return Err(crate::Error::Storage("occurrence no longer exists".into()));
                }
                (event, Some(key))
            }
        };
        if matches!(event.calendar, CalendarKey::Remote { .. }) {
            let destination = storage.list_calendars().into_iter()
                .min_by(|a, b| (!a.visible, &a.name, a.id).cmp(&(!b.visible, &b.name, b.id)))
                .ok_or_else(|| crate::Error::Storage("destination calendar no longer exists".into()))?;
            event.calendar = CalendarKey::Local(destination.id);
            event.source = crate::features::calendar_management::model::Source::Local;
        }
        let recurring = event.is_recurring();
        let wall = date.replace_offset(time::UtcOffset::UTC);
        let start = match event.time {
            EventTime::AllDay { start, end } => {
                let date = date.date();
                event.time = EventTime::AllDay {
                    start: date,
                    end: date.saturating_add(end - start),
                };
                date.midnight().assume_utc()
            }
            EventTime::Timed { start, end } => {
                let destination = if event.floating {
                    wall
                } else {
                    let naive =
                        chrono::DateTime::from_timestamp(wall.unix_timestamp(), wall.nanosecond())
                            .ok_or_else(|| {
                                crate::Error::Storage("invalid destination date".into())
                            })?
                            .naive_utc();
                    let offset = match self.calendar.display_timezone {
                        Some(zone) => zone.from_local_datetime(&naive).earliest().map(|date| {
                            use chrono::Offset;
                            date.offset().fix().local_minus_utc()
                        }),
                        None => chrono::Local
                            .from_local_datetime(&naive)
                            .earliest()
                            .map(|date| date.offset().local_minus_utc()),
                    }
                    .and_then(|offset| time::UtcOffset::from_whole_seconds(offset).ok())
                    .ok_or_else(|| crate::Error::Storage("invalid destination time".into()))?;
                    wall.replace_offset(offset)
                };
                event.time = EventTime::Timed {
                    start: destination,
                    end: destination.saturating_add(end - start),
                };
                destination
            }
        };
        let now = OffsetDateTime::now_utc();
        event.updated_at = now;
        if cut.is_none() || recurring {
            event.id = EventId::new();
            event.created_at = now;
            event.sequence = 0;
            event.rrule = None;
            event.exdates.clear();
            event.rdates.clear();
            event.repeat_before = None;
            event.split_before = None;
            event.predecessor = None;
            event.series_id = cut.map(|key| key.0);
            event.recurrence_id = cut.map(|key| key.1);
            if cut.is_none() {
                event.ics_uid = None;
                event.external = None;
            }
        } else {
            event.sequence = event.sequence.saturating_add(1);
        }
        let key = (event.id, start);
        storage.create_event(event)?;
        Ok(key)
    }

    fn event_is_editable(&self, id: EventId) -> bool {
        self.storage.lock().unwrap().get_event(id)
            .is_some_and(|event| matches!(event.calendar, CalendarKey::Local(_)))
    }

    fn begin_event_deletion(&mut self, id: EventId) -> bool {
        if !self.event_is_editable(id) {
            return false;
        }
        let Some(state) = self
            .editing_event
            .as_mut()
            .filter(|state| !state.writing && state.event_id == Some(id))
        else {
            return false;
        };
        state.writing = true;
        self.location_lookup = None;
        true
    }

    fn show_context_page(&mut self, page: ContextPage) {
        self.close_day_agenda();
        self.context_page = page;
        self.update_drawer_layout();
        self.core.set_show_context(true);
    }

    fn open_day_agenda(&mut self) {
        if self.sidebar_before_agenda.is_none() {
            self.sidebar_before_agenda = Some(self.core.nav_bar_active());
            self.core.nav_bar_set_toggled(false);
        }
        self.context_page = ContextPage::DayAgenda;
        self.update_drawer_layout();
        self.core.set_show_context(true);
    }

    fn close_day_agenda(&mut self) -> bool {
        let Some(was_open) = self.sidebar_before_agenda.take() else {
            return false;
        };
        self.core.set_show_context(false);
        self.core.nav_bar_set_toggled(was_open);
        true
    }

    fn update_drawer_layout(&mut self) {
        // Below this width the native drawer would squeeze the seven day columns.
        let overlay = self.context_page != ContextPage::DayAgenda
            || self.window_width / self.core.scale_factor() < 1_000.0;
        if self.core.window.context_is_overlay != overlay {
            self.core.window.context_is_overlay = overlay;
            self.core.set_show_context(self.core.window.show_context);
        }
    }

    fn show_month(&self) -> bool {
        matches!(self.tabs.active_data::<Tab>(), Some(Tab::Month)) && self.search_query.is_empty()
    }

    fn ics_import_view(&self) -> Element<'_, Message> {
        if self.ics_import.busy {
            return widget::text::body(fl!("ics-import-working")).into();
        }
        let storage = self.storage.lock().unwrap();
        let mut calendars = storage.list_calendars();
        calendars.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
        if calendars.is_empty() {
            return widget::text::body(fl!("ics-import-no-calendar")).into();
        }
        let selected = calendars
            .iter()
            .position(|calendar| Some(calendar.id) == self.ics_import.calendar);
        let labels = calendars
            .iter()
            .map(|calendar| calendar.name.clone())
            .collect::<Vec<_>>();
        let ids = calendars
            .iter()
            .map(|calendar| calendar.id)
            .collect::<Vec<_>>();
        let file = self
            .ics_import
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        widget::column(Vec::new())
            .spacing(spacing().space_s)
            .push(widget::text::body(file))
            .push(widget::settings::item(
                fl!("ics-import-destination"),
                widget::dropdown(labels, selected, move |index| {
                    Message::Ics(IcsMessage::SelectImportCalendar(ids[index]))
                }),
            ))
            .push(
                widget::button::suggested(fl!("ics-import-confirm"))
                    .on_press_maybe(selected.map(|_| Message::Ics(IcsMessage::ConfirmIcsImport))),
            )
            .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let selected_zone = self.display_timezone_options[1..]
            .iter()
            .position(|zone| zone == &self.config.display_timezone)
            .map_or(0, |index| index + 1);
        let zone_dropdown = widget::dropdown(
            self.display_timezone_options.as_slice(),
            Some(selected_zone),
            |value| Message::Settings(SettingsMessage::SetDisplayTimezone(value)),
        );
        let section = widget::settings::section()
            .title(fl!("settings-date-and-time"))
            .add(
                widget::settings::item::builder(fl!("display-timezone"))
                    .control(widget::container(zone_dropdown).max_width(210)),
            );

        let appearance = widget::settings::section().title(fl!("appearance")).add(
            widget::settings::item::builder(fl!("theme")).control(widget::dropdown(
                vec![
                    fl!("theme-match-desktop"),
                    fl!("theme-dark"),
                    fl!("theme-light"),
                ],
                AppTheme::OPTIONS
                    .iter()
                    .position(|&theme| theme == self.config.app_theme),
                |index| Message::Settings(SettingsMessage::SetTheme(AppTheme::OPTIONS[index])),
            )),
        );
        let online_enabled = !self.config.photon_endpoint.is_empty();
        let endpoint = self.photon_endpoint_input();
        let valid_endpoint = photon_api_url(endpoint);
        let can_save = valid_endpoint.as_ref().is_some_and(|url| url.as_str() != self.config.photon_endpoint);
        let location_settings = widget::settings::section()
            .title(fl!("location-suggestions"))
            .add(widget::text::body(fl!("online-location-description")))
            .add(
                widget::text_input(fl!("photon-endpoint"), endpoint).on_input(|value| {
                    Message::Settings(SettingsMessage::PhotonEndpointChanged(value))
                }),
            )
            .add_maybe(
                (valid_endpoint.is_none())
                    .then(|| widget::text::caption(fl!("photon-endpoint-invalid"))),
            )
            .add(
                widget::row(vec![
                    widget::button::suggested(if online_enabled {
                        fl!("online-location-update")
                    } else {
                        fl!("online-location-enable")
                    })
                    .on_press_maybe(
                        can_save.then_some(Message::Settings(SettingsMessage::SavePhotonEndpoint)),
                    )
                    .into(),
                    if online_enabled {
                        widget::button::standard(fl!("online-location-disable"))
                            .on_press(Message::Settings(SettingsMessage::DisableOnlineLocations))
                            .into()
                    } else {
                        widget::button::link(fl!("online-location-about"))
                            .on_press(Message::LaunchUrl(
                                "https://github.com/komoot/photon#demo-server".into(),
                            ))
                            .into()
                    },
                ])
                .spacing(spacing().space_xs),
            );
        let accounts_section = accounts_feature::view::settings_view(self);
        widget::settings::view_column(vec![accounts_section, appearance.into(), section.into(), location_settings.into()]).into()
    }

    pub(crate) fn accounts(&self) -> &[accounts_feature::Account] {
        &self.accounts
    }

    pub(crate) fn accounts_checked(&self) -> bool {
        self.accounts_checked
    }

    pub(crate) fn accounts_error(&self) -> Option<&accounts_feature::Error> {
        self.accounts_error.as_ref()
    }

    pub(crate) fn account_locally_enabled(&self, id: uuid::Uuid) -> bool {
        !self.config.disabled_accounts.contains(&id)
    }

    pub(crate) fn account_auth(&self) -> Option<&accounts_feature::AuthRequest> {
        self.account_auth.as_ref()
    }

    pub(crate) fn account_form(&self) -> Option<&accounts_feature::AccountForm> {
        self.account_form.as_ref()
    }

    pub(crate) fn account_action_error(&self) -> Option<&str> {
        self.account_action_error.as_deref()
    }

    fn remote_calendar_is_visible(&self, account_id: uuid::Uuid, calendar_id: &str) -> bool {
        self.active_accounts.contains(&account_id)
            && !self.config.remote_calendar_hidden(account_id, calendar_id)
    }

    pub(crate) fn event_is_visible(&self, storage: &LocalStorage, event: &Event) -> bool {
        match &event.calendar {
            CalendarKey::Local(id) => storage
                .get_calendar(*id)
                .is_none_or(|calendar| calendar.visible),
            CalendarKey::Remote {
                account_id,
                calendar_id,
            } => self.remote_calendar_is_visible(*account_id, calendar_id),
        }
    }

    fn refresh_active_accounts(&mut self) {
        let active_accounts: BTreeSet<_> = self
            .accounts
            .iter()
            .map(|account| account.id)
            .filter(|id| self.account_locally_enabled(*id))
            .collect();
        for id in self.active_accounts.difference(&active_accounts) {
            self.account_sync.remove(id);
            self.storage.lock().unwrap().clear_remote_snapshot(*id);
        }
        if self.active_accounts != active_accounts {
            self.calendar_popup = None;
        }
        self.active_accounts = active_accounts;
        self.refresh_calendar_visibility();
    }

    pub(crate) fn account_sync_status(&self, id: uuid::Uuid) -> Option<&accounts_feature::SyncStatus> {
        self.account_sync.get(&id)
    }

    pub(crate) fn account_has_calendars(&self, id: uuid::Uuid) -> bool {
        !self.storage.lock().unwrap().remote_calendars(id).is_empty()
    }

    fn sync_active_accounts(&mut self) -> Task<cosmic::Action<Message>> {
        use accounts_feature::{Error, SyncStatus};
        let ids: Vec<_> = self.active_accounts.iter().copied().filter(|id| {
            match self.account_sync.get(id) {
                None => true,
                Some(SyncStatus::Finished(
                    at,
                    None | Some(Error::Failed | Error::StorageUnavailable),
                )) => {
                    at.elapsed() >= std::time::Duration::from_secs(300)
                }
                _ => false,
            }
        }).collect();
        Task::batch(ids.into_iter().map(|id| self.sync_account(id)).collect::<Vec<_>>())
    }

    fn sync_account(&mut self, id: uuid::Uuid) -> Task<cosmic::Action<Message>> {
        use accounts_feature::SyncStatus;
        if !self.active_accounts.contains(&id)
            || matches!(self.account_sync.get(&id), Some(SyncStatus::Running { .. }))
        {
            return Task::none();
        }
        let Some(account) = self.accounts.iter().find(|account| account.id == id).cloned() else {
            return Task::none();
        };
        self.next_account_sync += 1;
        let request_id = self.next_account_sync;
        let previous_error = self.account_sync.get(&id).and_then(SyncStatus::error);
        self.account_sync.insert(
            id,
            SyncStatus::Running {
                request_id,
                previous_error,
            },
        );
        self.refresh_calendar_nav();
        let previous = self.storage.lock().unwrap().remote_sync_seed(id);
        Task::perform(async move { accounts_feature::sync(&account, previous).await }, move |result| {
            cosmic::action::app(Message::Accounts(AccountsAction::Synced(id, request_id, result)))
        })
    }

    fn photon_endpoint_input(&self) -> &str {
        self.photon_endpoint_input.as_deref().unwrap_or_else(|| {
            if self.config.photon_endpoint.is_empty() {
                PUBLIC_PHOTON_ENDPOINT
            } else {
                &self.config.photon_endpoint
            }
        })
    }

    fn clear_online_locations(&mut self) {
        self.location_lookup = None;
        if let Some(state) = &self.editing_event {
            self.event_locations.clear_online(&state.location);
        }
    }

    fn refresh_display_timezone(&mut self) {
        self.calendar.display_timezone = self
            .config
            .display_timezone
            .parse()
            .ok()
            .or_else(|| iana_time_zone::get_timezone().ok()?.parse().ok());
        self.calendar.current_date = crate::features::calendar::display_time(
            OffsetDateTime::now_utc(),
            self.calendar.display_timezone,
        );
    }

    /// The event creation/editing form for the context drawer.
    pub fn event_editor_view<'a>(&'a self) -> Element<'a, Message> {
        match &self.editing_event {
            Some(state) => {
                let storage = self.storage.lock().unwrap();
                let mut calendars = storage.list_calendars();
                calendars.sort_by(|a, b| a.name.cmp(&b.name));
                crate::features::event::view::view(state, &calendars, &self.event_notes, &self.event_locations)
            }
            None => widget::text::body("").into(),
        }
    }

    fn archived_calendars_view(&self) -> Element<'_, Message> {
        let storage = self.storage.lock().unwrap();
        let mut calendars = storage.archived_calendars();
        calendars.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
        if calendars.is_empty() {
            return widget::text::body(fl!("no-archived-calendars")).into();
        }
        let mut content = widget::column(Vec::new()).spacing(spacing().space_s);
        for calendar in calendars {
            content = content.push(
                widget::row(Vec::new())
                    .spacing(spacing().space_s)
                    .push(widget::text::body(calendar.name.clone()).width(Length::Fill))
                    .push(
                        widget::button::standard(fl!("restore-calendar")).on_press_maybe(
                            (!self.calendar_archive_busy && !self.storage_read_only).then_some(
                                Message::CalendarManagement(
                                    CalendarManagementMessage::RestoreCalendar(calendar.id),
                                ),
                            ),
                        ),
                    ),
            );
        }
        widget::scrollable(content).into()
    }

    /// The list of events matching `self.search_query`, shown in place of
    /// the month/week/day grids while a search is active.
    fn search_results_view<'a>(&'a self) -> Element<'a, Message> {
        let storage = self.storage.lock().unwrap();
        let results: Vec<_> = storage
            .search(&self.search_query)
            .into_iter()
            .filter(|event| self.event_is_visible(&storage, event))
            .collect();

        if results.is_empty() {
            return widget::text::body(fl!("no-results"))
                .apply(widget::container)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .into();
        }

        let mut list = widget::column(Vec::new())
            .spacing(spacing().space_xs)
            .width(Length::Fill);

        for event in results {
            let id = event.id;
            let start = event.start();
            let label = format!("{} — {}", start.date(), event.summary);
            list = list.push(
                widget::button::text(label)
                    .width(Length::Fill)
                    .class(cosmic::style::Button::Standard)
                    .on_press(Message::Calendar(CalendarMessage::SelectSearchResult(
                        id, start,
                    ))),
            );
        }

        widget::scrollable(list)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn open_main_window(&mut self) -> Task<cosmic::Action<Message>> {
        let mut settings = window::Settings {
            min_size: Some(MIN_WINDOW_SIZE),
            decorations: false,
            transparent: true,
            ..window::Settings::default()
        };
        #[cfg(target_os = "linux")]
        {
            settings.platform_specific.application_id = <Self as cosmic::Application>::APP_ID.to_owned();
        }

        let (id, open) = window::open(settings);
        self.core.set_main_window_id(Some(id));
        open.map(|_| cosmic::action::app(Message::MainWindowOpened))
    }

    /// Updates the header and window titles.
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }
}

/// The tab to display in the application.
pub enum Tab {
    Month,
    Week,
    Day,
}

/// The context page to display in the context drawer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    ArchivedCalendars,
    DayAgenda,
    Settings,
    IcsImport,
    Shortcuts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    NewCalendar,
    About,
    Settings,
    Shortcuts,
    Quit,
    ImportIcs,
    ExportIcs,
    ArchivedCalendars,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::NewCalendar => {
                Message::CalendarManagement(CalendarManagementMessage::OpenNewCalendarForm)
            }
            MenuAction::Settings => Message::ToggleContextPage(ContextPage::Settings),
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
            MenuAction::Shortcuts => Message::ToggleContextPage(ContextPage::Shortcuts),
            MenuAction::Quit => Message::Quit,
            MenuAction::ImportIcs => Message::Ics(IcsMessage::ImportIcs),
            MenuAction::ExportIcs => Message::Ics(IcsMessage::ExportIcs),
            MenuAction::ArchivedCalendars => {
                Message::ToggleContextPage(ContextPage::ArchivedCalendars)
            }
        }
    }
}
