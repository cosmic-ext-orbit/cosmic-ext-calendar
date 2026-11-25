use cosmic::{
    Element,
    iced::{
        Length, Task,
        alignment::Horizontal,
        widget::{Id, scrollable, stack, text::Wrapping},
    },
    theme::spacing,
    widget,
};
use time::{Duration, OffsetDateTime, Time};

use super::{
    CalendarAnchor, CalendarSlot, Occurrence, colored_event_card_style, event_target, slot_style,
    slot_target,
};
use crate::app::{AppModel, Message};

const HALF_HOUR_SLOTS: u8 = 48;
const CARD_LINE_HEIGHT: f32 = 16.0;
const SLOT_DURATION: Duration = Duration::minutes(30);

pub(super) const WEEK_SLOT_HEIGHT: f32 = 60.0;
pub(super) const DAY_SLOT_HEIGHT: f32 = 80.0;

pub(super) fn scroll_id(week: bool) -> Id {
    Id::new(if week {
        "calendar-week-grid"
    } else {
        "calendar-day-grid"
    })
}

pub(super) fn scroll_to_time<T>(time: Time, week: bool) -> Task<T> {
    let slot_height = if week {
        WEEK_SLOT_HEIGHT
    } else {
        DAY_SLOT_HEIGHT
    };
    let y = time_position(Duration::hours(i64::from(time.hour())), slot_height);
    scrollable::scroll_to(
        scroll_id(week),
        scrollable::AbsoluteOffset {
            x: None,
            y: Some(y),
        },
    )
}

fn time_position(elapsed: Duration, slot_height: f32) -> f32 {
    (elapsed.as_seconds_f64() / SLOT_DURATION.as_seconds_f64() * f64::from(slot_height)) as f32
}

pub(super) fn view<'a>(
    first: OffsetDateTime,
    days: u8,
    label_width: u16,
    slot_height: f32,
    events: &[Occurrence],
    selected: Option<CalendarSlot>,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let show_half_hours = days == 1;
    let mut labels = widget::column(Vec::new()).width(label_width);
    for slot in 0..HALF_HOUR_SLOTS {
        let label = if show_half_hours || slot % 2 == 0 {
            format!("{:02}:{:02}", slot / 2, slot % 2 * 30)
        } else {
            String::new()
        };
        labels = labels.push(
            widget::column(Vec::new())
                .push(slot_divider(slot))
                .push(
                    widget::container(widget::text::body(label))
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                )
                .height(slot_height),
        );
    }

    let mut grid = widget::row(Vec::new()).push(labels);
    for day in 0..days {
        let date = first + Duration::days(i64::from(day));
        grid = grid
            .push(widget::divider::vertical::default())
            .push(day_column(date, slot_height, events, selected, app));
    }
    grid.width(Length::Fill)
        .height(slot_height * f32::from(HALF_HOUR_SLOTS))
        .into()
}

fn slot_divider(slot: u8) -> Element<'static, Message> {
    if slot % 2 == 0 {
        widget::divider::horizontal::default().into()
    } else {
        widget::divider::horizontal::light().into()
    }
}

fn day_column<'a>(
    date: OffsetDateTime,
    slot_height: f32,
    events: &[Occurrence],
    selected: Option<CalendarSlot>,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let mut slots = widget::column(Vec::new()).width(Length::Fill);
    for slot in 0..HALF_HOUR_SLOTS {
        let hour = slot / 2;
        let minute = slot % 2 * 30;
        let cell = CalendarSlot::Timed(date.date(), Time::from_hms(hour, minute, 0).unwrap());
        slots = slots.push(
            widget::column(Vec::new())
                .push(slot_divider(slot))
                .push(slot_target(
                    app,
                    widget::container(widget::space::vertical())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .class(slot_style(selected == Some(cell))),
                    cell,
                    date.offset(),
                ))
                .height(slot_height)
                .width(Length::Fill),
        );
    }

    let height = slot_height * f32::from(HALF_HOUR_SLOTS);
    let day = date.date();
    let mut blocks: Vec<_> = events
        .iter()
        .filter(|item| item.time.is_some() && item.first <= day && day <= item.last)
        .map(|item| {
            let start = if item.first == day {
                time_position(item.display_start.time() - Time::MIDNIGHT, slot_height)
                    .min(height - CARD_LINE_HEIGHT)
            } else {
                0.0
            };
            let end = if item.last == day {
                time_position(
                    item.display_end.time() - Time::MIDNIGHT + Duration::NANOSECOND,
                    slot_height,
                )
            } else {
                height
            };
            EventBlock {
                item,
                start,
                end: end.max(start + CARD_LINE_HEIGHT).min(height),
            }
        })
        .collect();
    if blocks.is_empty() {
        return slots.into();
    }
    blocks.sort_by(|a, b| a.start.total_cmp(&b.start).then(b.end.total_cmp(&a.end)));

    let mut cards = widget::column(Vec::new()).width(Length::Fill);
    let mut group_end = 0.0_f32;
    let mut group_start = 0.0;
    let mut lanes: Vec<Vec<EventBlock<'_>>> = Vec::new();
    for block in blocks {
        if block.start >= group_end {
            if !lanes.is_empty() {
                cards = cards.push(event_group(&lanes, group_start, day, app));
                lanes.clear();
            }
            cards = cards.push(widget::space::vertical().height(block.start - group_end));
            group_start = block.start;
        }
        group_end = group_end.max(block.end);

        match lanes.iter_mut().find(|lane| {
            lane.last()
                .is_some_and(|previous| previous.end <= block.start)
        }) {
            Some(lane) => lane.push(block),
            None => lanes.push(vec![block]),
        }
    }
    cards = cards.push(event_group(&lanes, group_start, day, app));

    stack(vec![slots.into(), cards.into()])
        .width(Length::Fill)
        .height(height)
        .into()
}

struct EventBlock<'a> {
    item: &'a Occurrence,
    start: f32,
    end: f32,
}

fn event_group<'a>(
    lanes: &[Vec<EventBlock<'_>>],
    start: f32,
    date: time::Date,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let mut row = widget::row(Vec::new()).spacing(1);
    for lane in lanes {
        let mut column = widget::column(Vec::new()).width(Length::Fill);
        let mut cursor = start;
        for block in lane {
            let item = block.item;
            let anchor = CalendarAnchor::Grid(date, item.key());
            let height = block.end - block.start;
            let (content, label) = card_content(item, height);
            let button = widget::button::custom(content)
                .width(Length::Fill)
                .height(height)
                .padding([0, spacing().space_xxxs])
                .class(colored_event_card_style(item, true))
                .name(label)
                .on_press(item.open(anchor));

            column = column
                .push(widget::space::vertical().height(block.start - cursor))
                .push(event_target(app, button, item, anchor));
            cursor = block.end;
        }
        row = row.push(column);
    }
    row.width(Length::Fill).into()
}

fn card_content<'a>(item: &Occurrence, height: f32) -> (Element<'a, Message>, String) {
    let clock = |date: OffsetDateTime| format!("{:02}:{:02}", date.hour(), date.minute());
    let start = clock(item.display_start);
    let end = clock(item.display_end_exclusive);
    let time = if item.display_start == item.display_end_exclusive {
        start
    } else {
        format!("{start}–{end}")
    };
    let details = [Some(time), item.location.clone(), item.calendar.clone()];
    let mut content = widget::column(Vec::new()).push(
        widget::text::caption_heading(item.title.clone())
            .line_height(cosmic::iced::Pixels(CARD_LINE_HEIGHT))
            .wrapping(Wrapping::None),
    );
    let mut label = item.title.clone();
    let detail_lines = ((height / CARD_LINE_HEIGHT) as usize).saturating_sub(1);
    for (index, detail) in details.into_iter().flatten().enumerate() {
        label.push_str(", ");
        label.push_str(&detail);
        if index < detail_lines {
            content = content.push(
                widget::text::caption(detail)
                    .size(if index == 0 { 10 } else { 12 })
                    .line_height(cosmic::iced::Pixels(CARD_LINE_HEIGHT))
                    .wrapping(Wrapping::None),
            );
        }
    }

    (
        widget::container(content)
            .width(Length::Fill)
            .clip(true)
            .into(),
        label,
    )
}
