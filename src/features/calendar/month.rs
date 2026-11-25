use super::CalendarMessage;
use cosmic::{
    Element,
    iced::{Border, Color, Length, alignment::Vertical, widget::Id},
    theme::{Container, spacing},
    widget,
};
use time::{Date, Duration, Month, OffsetDateTime, Weekday};

use crate::{
    app::{AppModel, Message},
    features::{
        calendar_management::model::{CalendarKey, LocalCalendarMeta, calendar_color},
        event::model::{Event, EventTime},
    },
    fl,
    shared::{ids::EventId, storage::LocalStorage},
};

use super::{
    CalendarAnchor, CalendarSlot, EventSelection, LocalCalendar, accent_or_calendar_color,
    colored_event_card_style, display_time, event_button_style, event_target, slot_target,
};

const DAYS_PER_WEEK: usize = 7;
const EVENT_ROWS: usize = 2;
const EVENT_HEIGHT: f32 = 20.0;
const COMPACT_WIDTH: f32 = 520.0;
const WEEKDAY_ROW_HEIGHT: f32 = 28.0;

pub(super) struct Occurrence {
    id: EventId,
    pub(super) selected: bool,
    pub(super) editable: bool,
    pub(super) display_start: OffsetDateTime,
    pub(super) display_end: OffsetDateTime,
    pub(super) display_end_exclusive: OffsetDateTime,
    pub(super) start: OffsetDateTime,
    pub(super) first: Date,
    pub(super) last: Date,
    pub(super) time: Option<String>,
    pub(super) title: String,
    pub(super) location: Option<String>,
    pub(super) calendar: Option<String>,
    pub(super) color: Option<Color>,
}

impl Occurrence {
    fn spans_dates(&self) -> bool {
        self.first != self.last
    }

    pub(super) fn key(&self) -> EventSelection {
        (self.id, self.start)
    }

    pub(super) fn open(&self, anchor: CalendarAnchor) -> Message {
        Message::Calendar(CalendarMessage::CalendarEventClicked(self.key(), anchor))
    }
}

pub(super) fn occurrences(
    storage: &LocalStorage,
    app: &AppModel,
    first: Date,
    last: Date,
    zone: Option<chrono_tz::Tz>,
    selection: Option<EventSelection>,
) -> Vec<Occurrence> {
    let events = storage
        .all_events()
        .into_iter()
        .filter(|event| app.event_is_visible(storage, event));
    let mut items = occurrences_with_calendars(
        events.map(|event| {
            let calendar = match event.calendar {
                CalendarKey::Local(id) => storage.get_calendar(id),
                CalendarKey::Remote { .. } => None,
            };
            (event, calendar)
        }),
        first,
        last,
        zone,
    );
    for item in &mut items {
        item.selected = Some(item.key()) == selection;
        if let Some(Event { calendar: CalendarKey::Remote { account_id, calendar_id }, .. }) = storage.get_event(item.id) {
            item.calendar = storage.remote_calendars(*account_id).iter()
                .find(|calendar| calendar.id == *calendar_id)
                .map(|calendar| calendar.name.clone());
        }
    }
    items
}

fn occurrences_with_calendars<'a>(
    events: impl IntoIterator<Item = (&'a Event, Option<&'a LocalCalendarMeta>)>,
    first: Date,
    last: Date,
    zone: Option<chrono_tz::Tz>,
) -> Vec<Occurrence> {
    let utc_midnight = |date: Date| date.midnight().assume_utc();
    let mut items = Vec::new();

    for (event, calendar) in events {
        if calendar.is_some_and(|calendar| !calendar.visible) {
            continue;
        }

        let (duration, all_day) = match event.time {
            EventTime::AllDay { start, end } => (end - start, true),
            EventTime::Timed { start, end } => (end - start, false),
        };
        let duration = duration.max(Duration::ZERO);
        let lower = utc_midnight(first)
            .saturating_sub(duration)
            .saturating_sub(Duration::days(1));
        let upper = utc_midnight(last).saturating_add(Duration::days(2));

        for start in event.occurrences(lower..upper) {
            let display = |instant| {
                if all_day || event.floating {
                    instant
                } else {
                    display_time(instant, zone)
                }
            };
            let display_start = display(start);
            let end = start.saturating_add(duration);
            let inclusive_end = if duration.is_zero() {
                start
            } else {
                end.saturating_sub(Duration::NANOSECOND)
            };
            let display_end = display(inclusive_end);
            if display_start.date() > last || display_end.date() < first {
                continue;
            }

            items.push(Occurrence {
                id: event.id,
                selected: false,
                editable: matches!(event.calendar, CalendarKey::Local(_)),
                start,
                display_start,
                display_end,
                display_end_exclusive: display(end),
                first: display_start.date(),
                last: display_end.date(),
                time: (!all_day)
                    .then(|| format!("{:02}:{:02}", display_start.hour(), display_start.minute())),
                title: event.summary.clone(),
                location: event
                    .location
                    .as_deref()
                    .map(str::trim)
                    .filter(|location| !location.is_empty())
                    .map(str::to_owned),
                calendar: calendar.map(|calendar| calendar.name.clone()),
                color: event.color.as_deref().and_then(calendar_color).or_else(|| {
                    calendar
                        .and_then(|calendar| calendar.color.as_deref())
                        .and_then(calendar_color)
                }),
            });
        }
    }

    items.sort_by(|a, b| {
        (a.first, &a.time, a.start, &a.title, a.id)
            .cmp(&(b.first, &b.time, b.start, &b.title, b.id))
    });
    items
}

pub(super) fn month_view<'a>(
    calendar: LocalCalendar,
    storage: &LocalStorage,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let selected = calendar.selected_date.date();
    let today = calendar.current_date.date();
    let month_start = selected.replace_day(1).unwrap();
    let leading = month_start.weekday().number_days_from_monday() as usize;
    let first = month_start - Duration::days(leading as i64);
    let weeks =
        (leading + usize::from(selected.month().length(selected.year()))).div_ceil(DAYS_PER_WEEK);
    let last = first + Duration::days((weeks * DAYS_PER_WEEK - 1) as i64);
    let items = occurrences(
        storage,
        app,
        first,
        last,
        calendar.display_timezone,
        calendar.selected_event,
    );

    widget::responsive(move |size| {
        let compact = size.width < COMPACT_WIDTH;
        let padding = spacing().space_xxs;
        let grid_height = size.height - WEEKDAY_ROW_HEIGHT - 2.0 * f32::from(padding);
        let week_height = ((grid_height - (weeks - 1) as f32) / weeks as f32).max(if compact {
            52.0
        } else {
            91.0
        });
        let weekdays = widget::row::with_children(
            [
                Weekday::Monday,
                Weekday::Tuesday,
                Weekday::Wednesday,
                Weekday::Thursday,
                Weekday::Friday,
                Weekday::Saturday,
                Weekday::Sunday,
            ]
            .into_iter()
            .map(|day| {
                widget::container(widget::text::caption(weekday_name(day)))
                    .center_x(Length::Fill)
                    .center_y(WEEKDAY_ROW_HEIGHT)
                    .into()
            })
            .collect::<Vec<_>>(),
        );

        let mut grid = widget::column(Vec::new()).spacing(1);
        for week in 0..weeks {
            let start = first + Duration::days((week * DAYS_PER_WEEK) as i64);
            grid = grid.push(week_view(
                start,
                selected,
                today,
                &items,
                compact,
                week_height,
                app,
            ));
        }

        widget::column::with_children(vec![
            weekdays.into(),
            widget::scrollable(grid)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ])
        .spacing(padding)
        .padding([0, padding, padding, padding])
        .height(Length::Fill)
        .into()
    })
    .into()
}

#[derive(Clone, Copy)]
struct Segment<'a> {
    event: &'a Occurrence,
    first: usize,
    last: usize,
}

fn week_view<'a>(
    first: Date,
    selected: Date,
    today: Date,
    items: &[Occurrence],
    compact: bool,
    height: f32,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let last = first + Duration::days((DAYS_PER_WEEK - 1) as i64);
    let week_items: Vec<_> = items
        .iter()
        .filter(|item| item.first <= last && item.last >= first)
        .collect();
    let mut lanes: [Vec<Segment<'_>>; EVENT_ROWS] = Default::default();
    let mut occupied = [[false; DAYS_PER_WEEK]; EVENT_ROWS];
    let mut counts = [0; DAYS_PER_WEEK];
    let mut hidden = [0; DAYS_PER_WEEK];

    for &event in &week_items {
        let first_column = (event.first.max(first) - first).whole_days() as usize;
        let last_column = (event.last.min(last) - first).whole_days() as usize;
        for count in &mut counts[first_column..=last_column] {
            *count += 1;
        }
    }

    if !compact {
        for event in week_items
            .iter()
            .copied()
            .filter(|event| event.spans_dates())
            .chain(
                week_items
                    .iter()
                    .copied()
                    .filter(|event| !event.spans_dates()),
            )
        {
            let first_column = (event.first.max(first) - first).whole_days() as usize;
            let last_column = (event.last.min(last) - first).whole_days() as usize;
            let columns = first_column..=last_column;
            if let Some(lane) = occupied
                .iter()
                .position(|lane| !lane[columns.clone()].contains(&true))
            {
                occupied[lane][columns].fill(true);
                lanes[lane].push(Segment {
                    event,
                    first: first_column,
                    last: last_column,
                });
            } else {
                for count in &mut hidden[columns] {
                    *count += 1;
                }
            }
        }
    }

    let mut backgrounds = widget::row::with_capacity(DAYS_PER_WEEK).height(height);
    let mut dates = widget::row::with_capacity(DAYS_PER_WEEK);
    let mut overflow = widget::row::with_capacity(DAYS_PER_WEEK);

    for (day, (count, hidden)) in counts.into_iter().zip(hidden).enumerate() {
        let date = first + Duration::days(day as i64);
        let outside = date.month() != selected.month();
        backgrounds = backgrounds.push(slot_target(
            app,
            widget::container(widget::space::vertical())
                .width(Length::Fill)
                .height(Length::Fill)
                .class(Container::custom(move |theme| {
                    let cosmic = theme.cosmic();
                    let surface = theme.current_container();
                    let base: Color = surface.base.into();
                    let ink: Color = surface.on.into();
                    let tint = if date == selected {
                        blend(base, cosmic.accent_text_color().into(), 0.13)
                    } else if outside || day >= 5 {
                        blend(base, ink, 0.035)
                    } else {
                        base
                    };
                    cosmic::iced::widget::container::Style {
                        background: Some(tint.into()),
                        border: Border {
                            color: blend(base, ink, 0.12),
                            width: 1.0,
                            ..Border::default()
                        },
                        ..Default::default()
                    }
                })),
            CalendarSlot::MonthDay(date),
            time::UtcOffset::UTC,
        ));

        let number = widget::text::body(date.day().to_string()).class(if outside {
            cosmic::style::Text::Custom(|theme| {
                let surface = theme.current_container();
                cosmic::iced::widget::text::Style {
                    color: Some(blend(surface.on.into(), surface.base.into(), 0.32)),
                    ..Default::default()
                }
            })
        } else if date == today {
            cosmic::style::Text::Accent
        } else {
            cosmic::style::Text::Default
        });
        let number = widget::container(number).center_x(25).center_y(23);
        let number = if date == today {
            number.class(Container::custom(|theme| {
                cosmic::iced::widget::container::Style {
                    border: Border {
                        color: theme.cosmic().accent_text_color().into(),
                        radius: 6.0.into(),
                        width: 1.0,
                    },
                    ..Default::default()
                }
            }))
        } else {
            number
        };
        dates = dates.push(
            widget::container(number)
                .height(26)
                .align_y(Vertical::Center)
                .padding([0, 5])
                .width(Length::Fill),
        );

        let overflow_count = if compact { count } else { hidden };
        let control: Element<'_, Message> = if overflow_count > 0 {
            let label = if compact {
                fl!("month-compact-event-count", count = overflow_count)
            } else {
                fl!("month-more-events", count = overflow_count)
            };
            widget::button::text(label)
                .id(day_id(date))
                .name(fl!(
                    "month-day-accessible",
                    date = agenda_date(date),
                    count = count
                ))
                .width(Length::Fill)
                .height(EVENT_HEIGHT)
                .padding([0, 4])
                .class(cosmic::style::Button::Text)
                .on_press(Message::Calendar(CalendarMessage::ShowDayAgenda(date)))
                .into()
        } else {
            widget::space::horizontal().width(Length::Fill).into()
        };
        let control = if let Some(anchor) = app.compact_month_anchor(date, compact) {
            app.attach_calendar_popup(control, Some(anchor))
        } else {
            control
        };
        overflow = overflow.push(
            widget::container(control)
                .padding([0, 3])
                .width(Length::Fill),
        );
    }

    let mut foreground = widget::column(Vec::new()).push(dates);
    for lane in lanes.iter_mut().filter(|_| !compact) {
        lane.sort_by_key(|segment| segment.first);
        let mut row = widget::row(Vec::new()).height(EVENT_HEIGHT);
        let mut cursor = 0;
        for segment in lane {
            if cursor < segment.first {
                row = row.push(
                    widget::space::horizontal()
                        .width(Length::FillPortion((segment.first - cursor) as u16)),
                );
            }
            row = row.push(
                widget::container(event_button(
                    segment.event,
                    first + Duration::days(segment.first as i64),
                    app,
                ))
                .padding([0, 3])
                .width(Length::FillPortion(
                    (segment.last - segment.first + 1) as u16,
                )),
            );
            cursor = segment.last + 1;
        }
        if cursor < DAYS_PER_WEEK {
            row = row.push(
                widget::space::horizontal()
                    .width(Length::FillPortion((DAYS_PER_WEEK - cursor) as u16)),
            );
        }
        foreground = foreground.push(row);
    }
    foreground = foreground.push(overflow);

    cosmic::iced::widget::stack(vec![backgrounds.into(), foreground.into()])
        .width(Length::Fill)
        .height(height)
        .into()
}

fn event_button<'a>(event: &Occurrence, date: Date, app: &'a AppModel) -> Element<'a, Message> {
    use cosmic::iced::advanced::text::Wrapping;

    let band = event.spans_dates() || event.time.is_none();
    let time = event.time.clone().unwrap_or_else(|| fl!("all-day"));
    let title = event.title.clone();
    let mut content = widget::row(Vec::new())
        .push(color_dot(event.color))
        .spacing(4)
        .align_y(Vertical::Center);
    if !band && event.time.is_some() {
        content = content.push(widget::text::caption(time.clone()).wrapping(Wrapping::None));
    }
    content = content.push(widget::text::caption(title.clone()).wrapping(Wrapping::None));
    let label = format!("{time} · {title}");
    let anchor = CalendarAnchor::Month(date, event.key());
    let button = widget::button::custom(content)
        .name(label)
        .padding([0, 4])
        .height(EVENT_HEIGHT)
        .width(Length::Fill)
        .class(colored_event_card_style(event, band))
        .on_press(event.open(anchor));
    widget::container(event_target(app, button, event, anchor))
        .width(Length::Fill)
        .clip(true)
        .into()
}

fn color_dot<'a>(color: Option<Color>) -> Element<'a, Message> {
    widget::container(widget::space::horizontal())
        .width(6)
        .height(6)
        .class(Container::custom(move |theme| {
            cosmic::iced::widget::container::Style {
                background: Some(accent_or_calendar_color(theme, color).into()),
                border: Border {
                    radius: 3.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

pub(crate) fn day_id(date: Date) -> Id {
    Id::new(format!("month-day-{date}"))
}

pub(super) fn agenda<'a>(
    date: Date,
    storage: &LocalStorage,
    zone: Option<chrono_tz::Tz>,
    selection: Option<EventSelection>,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let items = occurrences(storage, app, date, date, zone, selection);
    let mut content = widget::column(Vec::new())
        .spacing(spacing().space_xxs)
        .push(widget::text::heading(agenda_date(date)));
    if items.is_empty() {
        content = content.push(widget::text::body(fl!("month-no-events")));
    } else {
        for event in items {
            let time = event.time.clone().unwrap_or_else(|| fl!("all-day"));
            let metadata = match &event.calendar {
                Some(calendar) => format!("{time} · {calendar}"),
                None => time,
            };
            let accessible_label = format!("{} · {metadata}", event.title);
            let row = widget::column(Vec::new())
                .push(widget::text::body(event.title.clone()))
                .push(widget::text::caption(metadata))
                .spacing(2)
                .width(Length::Fill);
            let entry = widget::row(vec![color_dot(event.color), row.into()])
                .spacing(spacing().space_xxs)
                .align_y(Vertical::Center);
            let anchor = CalendarAnchor::Agenda(date, event.key());
            content = content.push(event_target(
                app,
                widget::button::custom(entry)
                    .width(Length::Fill)
                    .padding(spacing().space_xs)
                    .class(event_button_style(
                        event.selected,
                        cosmic::style::Button::Text,
                        cosmic::style::Button::Standard,
                    ))
                    .name(accessible_label)
                    .on_press(event.open(anchor)),
                &event,
                anchor,
            ));
        }
    }
    content.into()
}

pub(super) fn blend(base: Color, accent: Color, amount: f32) -> Color {
    Color::from_rgb(
        base.r + (accent.r - base.r) * amount,
        base.g + (accent.g - base.g) * amount,
        base.b + (accent.b - base.b) * amount,
    )
}

pub(crate) fn agenda_date(date: Date) -> String {
    fl!(
        "month-agenda-date",
        weekday = weekday_name(date.weekday()),
        month = month_name(date.month()),
        day = date.day()
    )
}

pub(crate) fn weekday_name(day: Weekday) -> String {
    match day {
        Weekday::Monday => fl!("weekday-mon"),
        Weekday::Tuesday => fl!("weekday-tue"),
        Weekday::Wednesday => fl!("weekday-wed"),
        Weekday::Thursday => fl!("weekday-thu"),
        Weekday::Friday => fl!("weekday-fri"),
        Weekday::Saturday => fl!("weekday-sat"),
        Weekday::Sunday => fl!("weekday-sun"),
    }
}

fn month_name(month: Month) -> String {
    match month {
        Month::January => fl!("month-jan"),
        Month::February => fl!("month-feb"),
        Month::March => fl!("month-mar"),
        Month::April => fl!("month-apr"),
        Month::May => fl!("month-may"),
        Month::June => fl!("month-jun"),
        Month::July => fl!("month-jul"),
        Month::August => fl!("month-aug"),
        Month::September => fl!("month-sep"),
        Month::October => fl!("month-oct"),
        Month::November => fl!("month-nov"),
        Month::December => fl!("month-dec"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{features::event::state::EventEditorState, shared::ids::CalendarId};

    #[test]
    fn includes_ongoing_all_day_events_until_the_exclusive_end() {
        let first = Date::from_calendar_date(2026, Month::October, 26).unwrap();
        let last = first + Duration::days(1);
        let start = first - Duration::days(75);
        let mut calendar = LocalCalendarMeta {
            id: CalendarId::new(),
            name: "Travel".into(),
            description: None,
            color: Some("#3caa86".into()),
            visible: true,
            archived: false,
        };
        let mut event = EventEditorState::new(
            CalendarKey::Local(calendar.id),
            start.midnight().assume_utc(),
            false,
            String::new(),
        )
        .build_event(None);
        event.time = EventTime::AllDay { start, end: last };

        let items = occurrences_with_calendars([(&event, Some(&calendar))], first, last, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].first, start);
        assert_eq!(items[0].last, first);
        assert_eq!(items[0].time, None);
        assert_eq!(items[0].color, Some(Color::from_rgb8(60, 170, 134)));
        assert!(
            occurrences_with_calendars([(&event, Some(&calendar))], last, last, None).is_empty()
        );

        calendar.visible = false;
        assert!(
            occurrences_with_calendars([(&event, Some(&calendar))], first, first, None).is_empty()
        );
    }

    #[test]
    fn month_navigation_wraps_year_and_clamps_short_months() {
        let start = Date::from_calendar_date(2023, Month::December, 31).unwrap();
        let mut calendar = LocalCalendar::new(start.midnight().assume_utc());
        calendar.next_month().unwrap();
        assert_eq!(
            calendar.selected_date.to_calendar_date(),
            (2024, Month::January, 31)
        );
        calendar.next_month().unwrap();
        assert_eq!(
            calendar.selected_date.to_calendar_date(),
            (2024, Month::February, 29)
        );
        calendar.previous_month().unwrap();
        calendar.previous_month().unwrap();
        assert_eq!(
            calendar.selected_date.to_calendar_date(),
            (2023, Month::December, 29)
        );
    }
}
