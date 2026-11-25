// SPDX-License-Identifier: GPL-3.0-only

use accounts::{
    provider::providers_for, ui::provider_label, Provider as ManagedProvider, Service,
};
use cosmic::iced::{widget::text::Wrapping, Alignment, Length};
use cosmic::{widget, Element};

use crate::{
    app::{AppModel, Message},
    fl,
};

use super::{AccountsAction, FormField, SyncStatus};

/// Accounts section rendered inside Calendar's Settings drawer.
///
/// Accounts are created, reconnected, and removed here. This section lists
/// accounts exposing the Calendar service and lets each one be paused locally.
pub fn settings_view(app: &AppModel) -> Element<'_, Message> {
    let mut section = widget::settings::section().title(fl!("accounts"));
    if !app.accounts_checked() {
        section = section.add(widget::text::body(fl!("accounts-checking")));
    } else if let Some(error) = app.accounts_error() {
        section = section.add(widget::text::body(error.text()));
    } else if app.accounts().is_empty() {
        section = section.add(widget::text::body(fl!("no-accounts-connected")));
    }

    if let Some(error) = app.account_action_error() {
        section = section.add(widget::text::body(fl!(
            "account-action-error",
            error = error
        )));
    }

    if let Some(request) = app.account_auth() {
        let state = request.state.clone();
        section = section.add(
            widget::settings::item::builder(fl!(
                "accounts-waiting",
                provider = provider_label(request.provider)
            ))
            .description(fl!("accounts-waiting-description"))
            .control(
                widget::button::standard(fl!("accounts-cancel"))
                    .on_press(Message::Accounts(AccountsAction::CancelAuth(state))),
            ),
        );
    }

    for account in app.accounts() {
        let id = account.id;
        let enabled = app.account_locally_enabled(id);
        let mut controls = widget::row::with_capacity(3)
            .spacing(cosmic::theme::spacing().space_xs)
            .align_y(Alignment::Center);
        if enabled {
            let indicator = match app.account_sync_status(id) {
                Some(SyncStatus::Running {
                    previous_error: Some(error),
                    ..
                }) => Some((
                    "dialog-warning-symbolic",
                    fl!("accounts-retrying", error = error.text()),
                )),
                Some(SyncStatus::Finished(_, Some(error))) => {
                    Some(("dialog-warning-symbolic", error.text()))
                }
                Some(SyncStatus::Running { .. }) => {
                    Some(("dialog-information-symbolic", fl!("accounts-syncing")))
                }
                Some(SyncStatus::Finished(_, None)) if !app.account_has_calendars(id) => {
                    Some(("dialog-information-symbolic", fl!("accounts-no-calendars")))
                }
                _ => None,
            };
            if let Some((icon, message)) = indicator {
                controls = controls.push(widget::tooltip(
                    widget::icon::from_name(icon).size(16).icon(),
                    widget::text::body(message)
                        .width(Length::Fixed(240.0))
                        .wrapping(Wrapping::Word),
                    widget::tooltip::Position::Bottom,
                ));
            }
        }
        let toggle = widget::toggler(enabled).on_toggle(move |enabled| {
            Message::Accounts(AccountsAction::SetLocallyEnabled(id, enabled))
        });
        let address = account
            .address()
            .map_or_else(|| fl!("sidebar-online-account"), str::to_owned);
        section = section.add(
            widget::settings::item::builder(address)
                .description(account.provider_label())
                .control(
                    controls
                        .push(
                            widget::button::standard(fl!("accounts-reconnect"))
                                .on_press(Message::Accounts(AccountsAction::Reconnect(id))),
                        )
                        .push(
                            widget::button::destructive(fl!("accounts-remove"))
                                .on_press(Message::Accounts(AccountsAction::Remove(id))),
                        )
                        .push(toggle),
                ),
        );
    }

    let mut providers = widget::row::with_capacity(4)
        .spacing(cosmic::theme::spacing().space_xs);
    for provider in providers_for(Service::Calendar) {
        let label = if provider == ManagedProvider::CustomCalDav {
            fl!("accounts-custom-caldav")
        } else {
            fl!("accounts-sign-in", provider = provider_label(provider))
        };
        let message = if provider == ManagedProvider::CustomCalDav {
            Message::Accounts(AccountsAction::AddCalDav(Default::default()))
        } else {
            Message::Accounts(AccountsAction::AddProvider(provider))
        };
        providers = providers.push(widget::button::standard(label).on_press(message));
    }
    section = section.add(
        widget::settings::item::builder(fl!("accounts-add"))
            .description(fl!("manage-accounts-description"))
            .control(providers),
    );

    if let Some(form) = app.account_form() {
        section = section
            .add(
                widget::settings::item::builder(fl!("accounts-url"))
                    .control(widget::text_input(fl!("accounts-url"), &form.url).on_input(|value| {
                        Message::Accounts(AccountsAction::FormFieldChanged(
                            FormField::Url,
                            value,
                        ))
                    })),
            )
            .add(
                widget::settings::item::builder(fl!("accounts-username"))
                    .control(
                        widget::text_input(fl!("accounts-username"), &form.username).on_input(
                            |value| {
                                Message::Accounts(AccountsAction::FormFieldChanged(
                                    FormField::Username,
                                    value,
                                ))
                            },
                        ),
                    ),
            )
            .add(
                widget::settings::item::builder(fl!("accounts-password")).control(
                    widget::text_input::secure_input(fl!("accounts-password"), &form.password, None, true)
                        .on_input(|value| {
                            Message::Accounts(AccountsAction::FormFieldChanged(
                                FormField::Password,
                                value,
                            ))
                        }),
                ),
            )
            .add(
                widget::row::with_capacity(2)
                    .spacing(cosmic::theme::spacing().space_xs)
                    .push(
                        widget::button::standard(fl!("accounts-cancel"))
                            .on_press(Message::Accounts(AccountsAction::CancelForm)),
                    )
                    .push(
                        widget::button::suggested(fl!("accounts-connect"))
                            .on_press(Message::Accounts(AccountsAction::SubmitCalDav)),
                    ),
            );
    }

    section.into()
}
