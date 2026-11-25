// SPDX-License-Identifier: GPL-3.0-only

use super::super::{AppModel, Message};
use crate::features::calendar_management::model::LocalCalendarMeta;
use crate::features::calendar_management::state::CalendarForm;
use crate::features::calendar_management::CalendarManagementMessage;
use crate::fl;
use crate::shared::ids::CalendarId;
use cosmic::prelude::*;
use cosmic::widget::color_picker::ColorPickerUpdate;

impl AppModel {
    pub(in crate::app) fn update_calendar_management(
        &mut self,
        message: CalendarManagementMessage,
    ) -> Task<cosmic::Action<Message>> {
        let mut tasks = Vec::new();
        match message {
            CalendarManagementMessage::OpenNewCalendarForm => {
                if self.calendar_form.as_ref().is_some_and(|form| form.writing) {
                    return Task::none();
                }
                self.calendar_form = Some(CalendarForm::new(None, String::new(), None));
            }
            CalendarManagementMessage::StartEditCalendar(id) => {
                if self.calendar_form.as_ref().is_some_and(|form| form.writing) {
                    return Task::none();
                }
                if let Some(meta) = self.storage.lock().unwrap().get_calendar(id) {
                    self.calendar_form = Some(CalendarForm::new(
                        Some(id),
                        meta.name.clone(),
                        meta.color.clone(),
                    ));
                }
            }
            CalendarManagementMessage::CalendarNameChanged(name) => {
                if let Some(form) = self.calendar_form.as_mut().filter(|form| !form.writing) {
                    form.name = name;
                }
            }
            CalendarManagementMessage::SetCalendarColor(color) => {
                if let Some(form) = self.calendar_form.as_mut().filter(|form| !form.writing) {
                    form.picker = CalendarForm::color_picker(color.as_deref());
                    form.color = color;
                }
            }
            CalendarManagementMessage::CalendarColorPicker(update) => {
                if let Some(form) = self.calendar_form.as_mut().filter(|form| !form.writing) {
                    match update {
                        ColorPickerUpdate::ActionFinished => {}
                        ColorPickerUpdate::Cancel => {
                            form.picker = CalendarForm::color_picker(form.color.as_deref());
                        }
                        ColorPickerUpdate::ToggleColorPicker if form.picker.get_is_active() => {
                            form.picker = CalendarForm::color_picker(form.color.as_deref());
                        }
                        update => {
                            use cosmic::cosmic_theme::palette::{FromColor, Srgb};
                            let color = match &update {
                                ColorPickerUpdate::ActiveColor(hsv) => {
                                    Some(cosmic::iced::Color::from(Srgb::from_color(*hsv)))
                                }
                                ColorPickerUpdate::Input(value) => {
                                    value.parse::<css_color::Srgb>().ok().map(|rgb| {
                                        cosmic::iced::Color::from_rgb(rgb.red, rgb.green, rgb.blue)
                                    })
                                }
                                _ => None,
                            };
                            if let Some(color) = color {
                                let [r, g, b, _] = color.into_rgba8();
                                form.color = Some(format!("#{r:02x}{g:02x}{b:02x}"));
                            }
                            tasks.push(
                                form.picker
                                    .update::<Message>(update)
                                    .map(cosmic::action::app),
                            );
                        }
                    }
                }
            }
            CalendarManagementMessage::SaveCalendarForm => {
                if let Some(form) = self.calendar_form.as_mut().filter(|form| form.can_save()) {
                    form.writing = true;
                    let id = form.id;
                    let name = form.name.trim().to_owned();
                    let color = form.color.clone();
                    let storage = self.storage.clone();
                    tasks.push(Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                let mut storage = storage.lock().unwrap();
                                let mut meta = if let Some(id) = id {
                                    storage
                                        .get_calendar(id)
                                        .cloned()
                                        .ok_or("calendar not found")?
                                } else {
                                    LocalCalendarMeta {
                                        id: CalendarId::new(),
                                        name: String::new(),
                                        description: None,
                                        color: None,
                                        visible: true,
                                        archived: false,
                                    }
                                };
                                meta.name = name;
                                meta.color = color;
                                storage.update_calendar(meta).map_err(|err| err.to_string())
                            })
                            .await
                            .map_err(|err| err.to_string())?
                        },
                        |result| {
                            cosmic::action::app(Message::CalendarManagement(
                                CalendarManagementMessage::CalendarFormSaved(result),
                            ))
                        },
                    ));
                }
            }
            CalendarManagementMessage::CalendarFormSaved(result) => {
                if let Some(form) = self.calendar_form.as_mut() {
                    form.writing = false;
                    let editing = form.id.is_some();
                    match result {
                        Ok(()) => {
                            self.calendar_form = None;
                            self.refresh_calendar_visibility();
                        }
                        Err(err) => {
                            let notice = if editing {
                                fl!("calendar-save-error", error = err.as_str())
                            } else {
                                fl!("calendar-create-error", error = err.as_str())
                            };
                            tasks.push(self.toast(notice));
                        }
                    }
                }
            }
            CalendarManagementMessage::CloseCalendarForm => {
                if self
                    .calendar_form
                    .as_ref()
                    .is_some_and(|form| !form.writing)
                {
                    self.calendar_form = None;
                }
            }
            CalendarManagementMessage::ToggleCalendarVisibility(id, visible) => {
                let storage = self.storage.clone();
                tasks.push(Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut storage = storage.lock().unwrap();
                            let Some(mut meta) = storage.get_calendar(id).cloned() else {
                                return Err("calendar not found".to_string());
                            };
                            meta.visible = visible;
                            storage.update_calendar(meta).map_err(|err| err.to_string())
                        })
                        .await
                        .map_err(|err| err.to_string())?
                    },
                    |result| {
                        cosmic::action::app(Message::CalendarManagement(
                            CalendarManagementMessage::CalendarVisibilityToggled(result),
                        ))
                    },
                ));
            }
            CalendarManagementMessage::CalendarVisibilityToggled(result) => match result {
                Ok(()) => self.refresh_calendar_visibility(),
                Err(err) => {
                    tasks.push(self.toast(fl!("calendar-visibility-error", error = err.as_str())))
                }
            },
            CalendarManagementMessage::DeleteCalendar(id) => {
                if self.editing_event.is_none() && !self.calendar_archive_busy {
                    let storage = self.storage.lock().unwrap();
                    self.calendar_to_archive =
                        storage.get_calendar(id).map(|meta| (id, meta.name.clone()));
                    self.calendar_popup = None;
                }
            }
            CalendarManagementMessage::CancelArchiveCalendar => {
                if !self.calendar_archive_busy {
                    self.calendar_to_archive = None;
                }
            }
            CalendarManagementMessage::ConfirmArchiveCalendar
            | CalendarManagementMessage::RestoreCalendar(_) => {
                let archive = matches!(message, CalendarManagementMessage::ConfirmArchiveCalendar);
                let id = match message {
                    CalendarManagementMessage::RestoreCalendar(id)
                        if self.calendar_to_archive.is_none() =>
                    {
                        Some(id)
                    }
                    CalendarManagementMessage::ConfirmArchiveCalendar => {
                        self.calendar_to_archive.as_ref().map(|(id, _)| *id)
                    }
                    _ => None,
                };
                if let Some(id) = id.filter(|_| !self.calendar_archive_busy) {
                    self.calendar_archive_busy = true;
                    let storage = self.storage.clone();
                    tasks.push(Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                let mut storage = storage.lock().unwrap();
                                if archive {
                                    storage.archive_calendar(id)
                                } else {
                                    storage.restore_calendar(id)
                                }
                            })
                            .await
                            .map_err(|err| err.to_string())?
                            .map_err(|err| err.to_string())
                        },
                        |result| {
                            cosmic::action::app(Message::CalendarManagement(
                                CalendarManagementMessage::CalendarArchiveChanged(result),
                            ))
                        },
                    ));
                }
            }
            CalendarManagementMessage::CalendarArchiveChanged(result) => {
                self.calendar_archive_busy = false;
                let archived = self.calendar_to_archive.take().is_some();
                match result {
                    Ok(()) => {
                        self.calendar.selected_event = None;
                        self.calendar_popup = None;
                        self.refresh_calendar_visibility();
                        if archived {
                            tasks.push(self.toast(fl!("calendar-archived")));
                        }
                    }
                    Err(err) => {
                        tasks.push(self.toast(fl!("calendar-archive-error", error = err.as_str())))
                    }
                }
            }
        }
        self.update_drawer_layout();
        Task::batch(tasks)
    }
}
