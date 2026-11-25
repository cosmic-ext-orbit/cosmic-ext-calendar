use super::EventMessage;
use std::borrow::Cow;

use cosmic::{Element, iced::Length, theme::spacing, widget};

use crate::app::Message;
use crate::features::calendar::month::{agenda_date, weekday_name};
use crate::features::calendar_management::{
    model::{CalendarKey, LocalCalendarMeta, calendar_color},
    view::calendar_color_icon,
};
use crate::features::event::location::LocationSuggestions;
use crate::features::event::recurrence::{RecurrenceEnd, RecurrenceFreq};
use crate::features::event::state::{
    ALERT_PRESETS_MINUTES, AlertDraft, EventDateField, EventEditorState, RecurrenceScope,
    clock_label,
};
use crate::fl;

pub fn title_id() -> widget::Id {
    widget::Id::new("event-title")
}

fn date_control(
    state: &EventEditorState,
    field: EventDateField,
    date: time::Date,
) -> Element<'_, Message> {
    let button = widget::button::standard(fl!(
        "event-date-label",
        date = agenda_date(date),
        year = date.year().to_string()
    ))
    .leading_icon(widget::icon::from_name("office-calendar-symbolic"))
    .on_press(Message::Event(EventMessage::OpenEventDatePicker(field)));
    let Some((_, model)) = state
        .date_picker
        .as_ref()
        .filter(|(open, _)| *open == field)
    else {
        return button.into();
    };

    let picker = widget::calendar(
        model,
        |value| Message::Event(EventMessage::SelectEventDate(value)),
        || Message::Event(EventMessage::BrowseEventDatePicker(-1)),
        || Message::Event(EventMessage::BrowseEventDatePicker(1)),
        jiff::civil::Weekday::Monday,
    );
    widget::popover::popover(button)
        .position(widget::popover::Position::Bottom)
        .popup(widget::container(picker).class(cosmic::theme::Container::Dialog(false)))
        .on_close(Message::Event(EventMessage::CloseEventDatePicker))
        .into()
}

fn schedule_row(state: &EventEditorState, is_start: bool) -> Element<'_, Message> {
    let spacing = spacing();
    let (field, endpoint, input, presets, label) = if is_start {
        (
            EventDateField::Start,
            state.start,
            &state.start_clock_input,
            &state.start_clock_presets,
            fl!("starts"),
        )
    } else {
        (
            EventDateField::End,
            state.end,
            &state.end_clock_input,
            &state.end_clock_presets,
            fl!("ends"),
        )
    };
    let mut controls = vec![date_control(state, field, endpoint.date())];
    if !state.all_day {
        let value = input.clone().unwrap_or_else(|| clock_label(endpoint));
        let combo_box = widget::combo_box(presets, &value, Some(&value), move |value| {
            Message::Event(EventMessage::EventClockChanged(is_start, value))
        })
        .on_input(move |value| Message::Event(EventMessage::EventClockChanged(is_start, value)))
        .on_close(Message::Event(EventMessage::CloseEventClockSearch(
            is_start,
        )))
        .menu_height(240)
        .padding([spacing.space_xxs, spacing.space_xs])
        .font(cosmic::font::default())
        .size(14)
        .width(Length::Fill);
        let mut menu_theme = cosmic::theme::active();
        menu_theme.transparent = false;
        let measure = if value.chars().count() < 5 {
            "00:00".to_owned()
        } else {
            value
        };
        let measurement = widget::container(
            cosmic::iced::widget::text(measure)
                .font(cosmic::font::default())
                .size(14)
                .wrapping(cosmic::iced::widget::text::Wrapping::None)
                .class(cosmic::iced::Color::TRANSPARENT),
        )
        .padding([spacing.space_xxs, spacing.space_xs]);
        controls.push(
            cosmic::iced::widget::stack(vec![
                measurement.into(),
                cosmic::iced::widget::themer(Some(menu_theme), combo_box).into(),
            ])
            .width(Length::Shrink)
            .height(Length::Shrink)
            .into(),
        );
    }
    widget::settings::item(
        label,
        widget::flex_row(controls)
            .align_items(cosmic::iced::Alignment::Center)
            .spacing(spacing.space_xs),
    )
    .into()
}

fn text_input_editor_style(
    theme: &cosmic::Theme,
    status: widget::text_editor::Status,
) -> widget::text_editor::Style {
    use widget::text_editor::Status;
    use widget::text_input::{Style, StyleSheet};

    let style = Style::Default;
    let appearance = match status {
        Status::Active => StyleSheet::active(theme, &style),
        Status::Hovered => StyleSheet::hovered(theme, &style),
        Status::Focused { .. } => StyleSheet::focused(theme, &style),
        Status::Disabled => StyleSheet::disabled(theme, &style),
    };
    widget::text_editor::Style {
        background: appearance.background,
        border: cosmic::iced::Border {
            color: appearance.border_color,
            width: appearance.border_width,
            radius: appearance.border_radius,
        },
        placeholder: appearance.placeholder_color,
        value: appearance
            .text_color
            .unwrap_or_else(|| theme.current_container().on.into()),
        selection: appearance.selected_fill,
    }
}

/// The "Repeat" settings section. Only the frequency row is always shown;
/// interval/weekday/end-condition rows are added as the form deepens, so a
/// one-off event never has to look at recurrence controls at all.
fn recurrence_section<'a>(state: &'a EventEditorState) -> widget::settings::Section<'a, Message> {
    let recurrence_labels: Vec<String> = RecurrenceFreq::ALL
        .iter()
        .map(|freq| freq.label().to_string())
        .collect();
    let recurrence_selected = RecurrenceFreq::ALL
        .iter()
        .position(|freq| *freq == state.recurrence.freq);

    let mut section = widget::settings::section().add(widget::settings::item(
        fl!("recurrence"),
        widget::dropdown(recurrence_labels, recurrence_selected, |value| {
            Message::Event(EventMessage::EventEditorRecurrenceChanged(value))
        }),
    ));

    if state.recurrence.freq == RecurrenceFreq::None {
        return section;
    }

    let interval_labels: Vec<String> = (1..=30).map(|n| n.to_string()).collect();
    let interval_selected = Some((state.recurrence.interval.max(1) - 1) as usize);
    section = section.add(widget::settings::item(
        fl!("every"),
        widget::dropdown(interval_labels, interval_selected, |value| {
            Message::Event(EventMessage::EventEditorRecurrenceIntervalChanged(value))
        }),
    ));

    if state.recurrence.freq == RecurrenceFreq::Weekly {
        let mut weekdays = widget::row(Vec::new()).spacing(spacing().space_xxs);
        for (idx, day) in [
            time::Weekday::Monday,
            time::Weekday::Tuesday,
            time::Weekday::Wednesday,
            time::Weekday::Thursday,
            time::Weekday::Friday,
            time::Weekday::Saturday,
            time::Weekday::Sunday,
        ]
        .into_iter()
        .enumerate()
        {
            let on = state.recurrence.by_weekday[idx];
            weekdays = weekdays.push(
                widget::button::custom(widget::text::body(weekday_name(day)))
                    .class(if on {
                        cosmic::style::Button::Suggested
                    } else {
                        cosmic::style::Button::Standard
                    })
                    .on_press(Message::Event(
                        EventMessage::EventEditorRecurrenceWeekdayToggled(idx, !on),
                    )),
            );
        }
        section = section.add(widget::settings::item(fl!("on-days"), weekdays));
    }

    let end_labels: Vec<String> = vec![fl!("end-never"), fl!("end-until"), fl!("end-count")];
    let end_selected = Some(match state.recurrence.end {
        RecurrenceEnd::Never => 0,
        RecurrenceEnd::Until(_) => 1,
        RecurrenceEnd::Count(_) => 2,
    });
    section = section.add(widget::settings::item(
        fl!("recurrence-ends"),
        widget::dropdown(end_labels, end_selected, |value| {
            Message::Event(EventMessage::EventEditorRecurrenceEndChanged(value))
        }),
    ));

    match state.recurrence.end {
        RecurrenceEnd::Until(until) => {
            section = section.add(widget::settings::item(
                fl!("end-until"),
                date_control(state, EventDateField::Until, until),
            ));
        }
        RecurrenceEnd::Count(count) => {
            let count_labels: Vec<String> = (1..=30).map(|n| n.to_string()).collect();
            let count_selected = Some((count.max(1) - 1) as usize);
            let count_row = widget::row(Vec::new())
                .spacing(spacing().space_xs)
                .align_y(cosmic::iced::alignment::Vertical::Center)
                .push(widget::dropdown(count_labels, count_selected, |value| {
                    Message::Event(EventMessage::EventEditorRecurrenceCountChanged(value))
                }))
                .push(widget::text::body(fl!("occurrences")));
            section = section.add(widget::settings::item(fl!("end-count"), count_row));
        }
        RecurrenceEnd::Never => {}
    }

    section
}

fn alert_label(minutes: i64) -> String {
    if minutes == 0 {
        fl!("event-alert-at-start")
    } else if minutes % 1440 == 0 {
        let days = minutes / 1440;
        fl!("event-alert-days", count = days)
    } else if minutes % 60 == 0 {
        let hours = minutes / 60;
        fl!("event-alert-hours", count = hours)
    } else {
        fl!("event-alert-minutes", count = minutes)
    }
}

fn alert_row_label(number: usize) -> String {
    if number == 1 {
        return fl!("event-alert-first");
    }
    let suffix = match (number % 100, number % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    fl!(
        "event-alert-numbered",
        number = number.to_string(),
        suffix = suffix
    )
}

fn alerts_section(state: &EventEditorState) -> widget::settings::Section<'_, Message> {
    let mut section = widget::settings::section().title(fl!("event-alerts"));
    let add_row = state
        .alerts
        .last()
        .is_none_or(|alert| alert.minutes().is_some());
    let used: std::collections::HashSet<_> = state
        .alerts
        .iter()
        .filter_map(AlertDraft::minutes)
        .collect();
    for row in 0..state.alerts.len() + usize::from(add_row) {
        let alert = state.alerts.get(row);
        let current = alert.and_then(AlertDraft::minutes);
        let presets: Vec<_> = ALERT_PRESETS_MINUTES
            .iter()
            .copied()
            .filter(|minutes| current == Some(*minutes) || !used.contains(minutes))
            .collect();
        let selected = match alert {
            None => Some(0),
            Some(AlertDraft::Preset(minutes)) => presets
                .iter()
                .position(|option| option == minutes)
                .map(|index| index + 1),
            Some(AlertDraft::Custom(_)) => Some(presets.len() + 1),
        };
        let mut labels = vec![fl!("event-alert-none")];
        labels.extend(presets.iter().copied().map(alert_label));
        labels.push(fl!("event-alert-custom"));

        let mut controls = widget::row(Vec::new())
            .align_y(cosmic::iced::Alignment::Center)
            .spacing(spacing().space_xs);
        if let Some(AlertDraft::Custom(input)) = alert {
            controls = controls.push(
                widget::text_input(fl!("event-alert-custom-minutes"), input)
                    .on_input(move |value| {
                        Message::Event(EventMessage::EditCustomEventAlert(row, value))
                    })
                    .width(136),
            );
        }
        controls = controls.push(widget::dropdown(labels, selected, move |index| {
            if index == 0 {
                Message::Event(EventMessage::RemoveEventAlert(row))
            } else {
                Message::Event(EventMessage::SetEventAlert(
                    row,
                    presets.get(index - 1).copied(),
                ))
            }
        }));
        section = section
            .add(widget::settings::item::builder(alert_row_label(row + 1)).control(controls));
    }
    if let Some(error) = state.alert_error() {
        section = section.add(widget::text::body(error));
    }
    section
}

pub fn view<'a>(
    state: &'a EventEditorState,
    calendars: &[&LocalCalendarMeta],
    notes: &'a widget::text_editor::Content,
    locations: &'a LocationSuggestions,
) -> Element<'a, Message> {
    if state.writing {
        return widget::dialog::dialog()
            .title(fl!("event-writing"))
            .width(Length::Fixed(640.0))
            .into();
    }

    let calendar_labels: Vec<String> = calendars.iter().map(|meta| meta.name.clone()).collect();
    let calendar_selected = calendars
        .iter()
        .position(|meta| matches!(&state.calendar, CalendarKey::Local(id) if *id == meta.id));

    let calendar_icons = calendars
        .iter()
        .map(|meta| calendar_color_icon(meta.color.as_deref().and_then(calendar_color), true))
        .collect();
    let title = widget::settings::section().add(
        widget::editable_input(
            fl!("event-title"),
            &state.summary,
            state.editing_title,
            |value| Message::Event(EventMessage::ToggleEventTitleEditing(value)),
        )
        .id(title_id())
        .on_input(|value| Message::Event(EventMessage::EventEditorSummaryChanged(value)))
        .width(Length::Fill),
    );
    let mut when = widget::settings::section()
        .add(
            widget::settings::item::builder(fl!("all-day")).toggler(state.all_day, |value| {
                Message::Event(EventMessage::EventEditorAllDayToggled(value))
            }),
        )
        .add(schedule_row(state, true))
        .add(schedule_row(state, false))
        .add(widget::settings::item(
            fl!("event-calendar"),
            widget::dropdown(calendar_labels, calendar_selected, |value| {
                Message::Event(EventMessage::EventEditorCalendarChanged(value))
            })
            .icons(Cow::Owned(calendar_icons)),
        ));
    let validation_error = state.validation_error();
    if state.alert_error().is_none() {
        if let Some(error) = &validation_error {
            when = when.add(widget::text::body(error.clone()));
        }
    }

    let location_field = widget::combo_box(
        &locations.choices,
        &fl!("location"),
        Some(&state.location),
        |value| Message::Event(EventMessage::SelectEventLocation(value)),
    )
    .on_input(|value| Message::Event(EventMessage::EventEditorLocationChanged(value)))
    .on_close(Message::Event(EventMessage::CloseEventLocationSuggestions))
    .menu_height(240)
    .width(Length::Fill);
    let mut location_theme = cosmic::theme::active();
    location_theme.transparent = false;
    let mut details = widget::settings::section().add(cosmic::iced::widget::themer(
        Some(location_theme),
        location_field,
    ));
    if locations
        .online
        .as_ref()
        .is_some_and(|online| online.has_results())
    {
        details = details.add(
            widget::button::link(fl!("online-location-attribution"))
                .padding(0)
                .on_press(Message::LaunchUrl(
                    "https://www.openstreetmap.org/copyright".into(),
                )),
        );
    } else if locations.online_failed {
        details = details.add(widget::text::caption(fl!("online-location-unavailable")));
    }
    let details = details.add(
        widget::text_editor(notes)
            .placeholder(fl!("event-notes"))
            .on_action(|value| Message::Event(EventMessage::EventEditorNotesChanged(value)))
            .height(88)
            .padding(spacing().space_xxs)
            .class(cosmic::theme::iced::TextEditor::Custom(Box::new(
                text_input_editor_style,
            ))),
    );

    let mut options = widget::settings::section().add(
        widget::settings::item::builder(fl!("event-more-options"))
            .toggler(state.more_options, |_| {
                Message::Event(EventMessage::ToggleEventOptions)
            }),
    );
    if state.more_options {
        if !state.all_day {
            options = options.add(
                widget::settings::item::builder(fl!("event-floating-time"))
                    .toggler(state.floating, |value| {
                        Message::Event(EventMessage::EventEditorFloatingToggled(value))
                    }),
            );
            if !state.floating {
                options = options.add(widget::settings::item(
                    fl!("event-timezone"),
                    widget::text_input(fl!("event-timezone-placeholder"), &state.timezone)
                        .on_input(|value| {
                            Message::Event(EventMessage::EventEditorTimezoneChanged(value))
                        })
                        .width(Length::Fixed(240.0)),
                ));
            }
        }
        options = options.add(
            widget::settings::item::builder(fl!("task")).toggler(state.is_task, |value| {
                Message::Event(EventMessage::EventEditorTaskToggled(value))
            }),
        );
    }
    let mut sections = Vec::new();
    if state.recurring_series {
        sections.push(
            widget::settings::section()
                .add(widget::settings::item(
                    fl!("recurrence-edit-scope"),
                    widget::dropdown(
                        RecurrenceScope::OPTIONS
                            .into_iter()
                            .map(RecurrenceScope::label)
                            .collect::<Vec<_>>(),
                        RecurrenceScope::OPTIONS
                            .iter()
                            .position(|scope| *scope == state.recurrence_scope),
                        |value| Message::Event(EventMessage::EventEditorScopeChanged(value)),
                    ),
                ))
                .into(),
        );
    }
    sections.extend([title.into(), when.into()]);
    if state.recurrence_scope != RecurrenceScope::Occurrence {
        sections.push(recurrence_section(state).into());
    }
    sections.push(alerts_section(state).into());
    sections.extend([details.into(), options.into()]);
    let content = widget::settings::view_column(sections);

    let mut dialog = widget::dialog::dialog()
        .title(if state.event_id.is_some() {
            fl!("edit-event")
        } else {
            fl!("new-event")
        })
        .control(
            widget::scrollable(content)
                .width(Length::Fill)
                .height(Length::Fixed(420.0)),
        )
        .primary_action(
            widget::button::suggested(fl!("save")).on_press_maybe(
                validation_error
                    .is_none()
                    .then_some(Message::Event(EventMessage::SaveEvent)),
            ),
        )
        .secondary_action(
            widget::button::standard(fl!("cancel"))
                .on_press(Message::Event(EventMessage::CloseEventEditor)),
        )
        .width(Length::Fixed(640.0));

    if let Some(event_id) = state.event_id {
        let (label, action) = if state.recurring_series {
            match state.recurrence_scope {
                RecurrenceScope::Occurrence => (
                    fl!("delete-occurrence"),
                    Message::Event(EventMessage::DeleteOccurrence(
                        event_id,
                        state.occurrence_start,
                    )),
                ),
                RecurrenceScope::Following => (
                    fl!("delete-following"),
                    Message::Event(EventMessage::DeleteFollowing(
                        event_id,
                        state.occurrence_start,
                    )),
                ),
                RecurrenceScope::Series => (
                    fl!("delete-series"),
                    Message::Event(EventMessage::DeleteEvent(event_id)),
                ),
            }
        } else {
            (
                fl!("delete"),
                Message::Event(EventMessage::DeleteEvent(event_id)),
            )
        };
        dialog = dialog.tertiary_action(widget::button::destructive(label).on_press(action));
    }

    dialog.into()
}
