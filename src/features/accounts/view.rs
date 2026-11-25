// SPDX-License-Identifier: GPL-3.0-only

use accounts::{
    ui::{
        provider_label, AccountRow, AccountStatus, Form as AccountsForm, FormState, Labels,
        View as AccountsView,
    },
    Provider, Service,
};
use cosmic::Element;

use crate::{
    app::{AppModel, Message},
    fl,
};

use super::{AccountsAction, SyncStatus};

/// Accounts page rendered inside Calendar's dedicated context drawer.
///
/// The shared accounts panel owns the narrow layout: account cards, a full
/// width provider picker, CalDAV fields, and the browser sign-in wait state.
pub fn accounts_view(app: &AppModel) -> Element<'_, Message> {
    let rows: Vec<AccountRow<'_>> = app
        .accounts()
        .iter()
        .map(|account| {
            let id = account.id;
            let enabled = app.account_locally_enabled(id);
            AccountRow {
                id,
                provider: managed_provider(&account.provider),
                label: account
                    .address()
                    .map_or_else(|| fl!("sidebar-online-account"), str::to_owned)
                    .into(),
                detail: account.provider_label().to_owned().into(),
                enabled,
                status: if enabled {
                    account_status(app, id)
                } else {
                    None
                },
            }
        })
        .collect();

    let labels = Labels {
        title: fl!("accounts").into(),
        checking: fl!("accounts-checking").into(),
        empty: fl!("no-accounts-connected").into(),
        no_providers: fl!("accounts-no-providers").into(),
        add: fl!("accounts-add").into(),
        add_title: fl!("accounts-add-title").into(),
        add_body: fl!("accounts-add-body").into(),
        back: fl!("accounts-cancel").into(),
        finish_title: fl!("accounts-finish-title").into(),
        finish_body: &finish_body,
        open_browser_again: fl!("accounts-open-browser").into(),
        cancel: fl!("accounts-cancel").into(),
        reconnect: fl!("accounts-reconnect").into(),
        remove: fl!("accounts-remove").into(),
        url: fl!("accounts-url").into(),
        username: fl!("accounts-username").into(),
        password: fl!("accounts-password").into(),
        connect: fl!("accounts-connect").into(),
        sign_in: &provider_action,
        ..Labels::default()
    };

    let form = app.account_form().map(|form| AccountsForm {
        state: FormState {
            provider: Provider::CustomCalDav,
            service: Service::Calendar,
            url: &form.url,
            email: "",
            username: &form.username,
            password: &form.password,
            imap_host: "",
            imap_port: "",
            smtp_host: "",
            smtp_port: "",
            base_dn: "",
            starttls: false,
        },
        on_change: &change_form,
        on_starttls: None,
        on_submit: &submit_form,
        on_cancel: &cancel_form,
    });

    accounts::ui::view(AccountsView {
        service: Service::Calendar,
        accounts: rows,
        pending: app.account_auth(),
        loading: !app.accounts_checked(),
        adding: app.account_adding(),
        providers: None,
        form,
        footer: None,
        labels,
        on_add_open: &open_accounts,
        on_add_close: &close_accounts,
        on_open_url: &open_browser,
        on_add: &add_provider,
        on_cancel: &cancel_auth,
        on_remove: &remove,
        on_reconnect: &reconnect,
        on_toggle: &set_locally_enabled,
        on_sync: None,
    })
}

fn account_status(app: &AppModel, id: uuid::Uuid) -> Option<AccountStatus<'static>> {
    match app.account_sync_status(id) {
        Some(SyncStatus::Running {
            previous_error: Some(error),
            ..
        }) => Some(AccountStatus {
            icon: "dialog-warning-symbolic",
            text: fl!("accounts-retrying", error = error.text()).into(),
        }),
        Some(SyncStatus::Finished(_, Some(error))) => Some(AccountStatus {
            icon: "dialog-warning-symbolic",
            text: error.text().into(),
        }),
        Some(SyncStatus::Running { .. }) => Some(AccountStatus {
            icon: "dialog-information-symbolic",
            text: fl!("accounts-syncing").into(),
        }),
        Some(SyncStatus::Finished(_, None)) if !app.account_has_calendars(id) => {
            Some(AccountStatus {
                icon: "dialog-information-symbolic",
                text: fl!("accounts-no-calendars").into(),
            })
        }
        _ => None,
    }
}

fn managed_provider(provider: &str) -> Provider {
    match provider {
        "microsoft" => Provider::Microsoft,
        "custom-caldav" => Provider::CustomCalDav,
        _ => Provider::Google,
    }
}

fn finish_body(provider: Provider) -> String {
    fl!("accounts-finish-body", provider = provider_label(provider))
}

fn provider_action(provider: Provider) -> String {
    match provider {
        Provider::CustomCalDav => fl!("accounts-custom-caldav"),
        provider => fl!("accounts-sign-in", provider = provider_label(provider)),
    }
}

fn open_accounts() -> Message {
    Message::Accounts(AccountsAction::OpenAccounts)
}

fn close_accounts() -> Message {
    Message::Accounts(AccountsAction::CloseAccounts)
}

fn open_browser() -> Message {
    Message::Accounts(AccountsAction::OpenBrowser)
}

fn add_provider(provider: Provider) -> Message {
    let action = match provider {
        Provider::CustomCalDav => AccountsAction::AddCalDav(Default::default()),
        provider => AccountsAction::AddProvider(provider),
    };
    Message::Accounts(action)
}

fn cancel_auth(state: &str) -> Message {
    Message::Accounts(AccountsAction::CancelAuth(state.to_owned()))
}

fn remove(id: uuid::Uuid) -> Message {
    Message::Accounts(AccountsAction::Remove(id))
}

fn reconnect(id: uuid::Uuid) -> Message {
    Message::Accounts(AccountsAction::Reconnect(id))
}

fn set_locally_enabled(id: uuid::Uuid, enabled: bool) -> Message {
    Message::Accounts(AccountsAction::SetLocallyEnabled(id, enabled))
}

fn change_form(field: accounts::ui::FormField, value: String) -> Message {
    Message::Accounts(AccountsAction::FormFieldChanged(field, value))
}

fn submit_form() -> Message {
    Message::Accounts(AccountsAction::SubmitCalDav)
}

fn cancel_form() -> Message {
    Message::Accounts(AccountsAction::CancelForm)
}
