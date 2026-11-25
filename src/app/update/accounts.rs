// SPDX-License-Identifier: GPL-3.0-only

use super::super::{AppModel, CalendarPopup, Message};
use crate::config::Config;
use crate::features::accounts::{self as accounts_feature, AccountsAction, FormField};
use crate::features::calendar_management::model::CalendarKey;
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::prelude::*;
use cosmic::Application;

fn rediscover() -> Task<cosmic::Action<Message>> {
    cosmic::task::message(Message::Accounts(AccountsAction::Discover))
}

impl AppModel {
    pub(in crate::app) fn update_accounts(
        &mut self,
        action: AccountsAction,
    ) -> Task<cosmic::Action<Message>> {
        let mut tasks = Vec::new();
        match action {
            AccountsAction::Discover => {
                if self.discovering_accounts {
                    return Task::none();
                }
                self.discovering_accounts = true;
                return Task::perform(accounts_feature::discover(), |result| {
                    cosmic::action::app(Message::Accounts(AccountsAction::Discovered(result)))
                });
            }
            AccountsAction::Discovered(result) => {
                self.discovering_accounts = false;
                self.accounts_checked = true;
                let alert = match result {
                    Ok(discovery) => {
                        self.accounts = discovery.accounts;
                        self.watched_account_ids = discovery.watched_ids;
                        None
                    }
                    Err(error) => {
                        self.accounts.clear();
                        Some(error.text())
                    }
                };
                self.refresh_active_accounts();
                tasks.push(self.sync_active_accounts());
                if let Some(message) = alert {
                    tasks.push(self.toast(message));
                }
                return Task::batch(tasks);
            }
            AccountsAction::Changed => return rediscover(),
            AccountsAction::OpenAccounts => {
                self.account_adding = true;
                return Task::none();
            }
            AccountsAction::CloseAccounts => {
                self.account_adding = false;
                return Task::none();
            }
            AccountsAction::AddProvider(provider) => {
                return Task::perform(accounts_feature::begin_provider(provider), |result| {
                    cosmic::action::app(Message::Accounts(AccountsAction::AuthStarted(
                        result.map_err(|error| error.text()),
                    )))
                });
            }
            AccountsAction::OpenBrowser => {
                if let Some(request) = self.account_auth.clone() {
                    if let Err(error) = accounts::ui::open_sign_in_url(&request) {
                        return self.toast(error.to_string());
                    }
                }
                return Task::none();
            }
            AccountsAction::AuthStarted(result) => match result {
                Ok(request) => {
                    self.account_auth = Some(request.clone());
                    self.account_adding = false;
                    let completion = request.clone();
                    let mut tasks = vec![Task::perform(
                        accounts_feature::complete(completion),
                        |result| {
                            cosmic::action::app(Message::Accounts(AccountsAction::AuthCompleted(
                                result.map_err(|error| error.text()),
                            )))
                        },
                    )];
                    if let Err(error) = accounts::ui::open_sign_in_url(&request) {
                        tasks.push(self.toast(error.to_string()));
                    }
                    return Task::batch(tasks);
                }
                Err(error) => return self.toast(error),
            },
            AccountsAction::AuthCompleted(result) => {
                self.account_auth = None;
                self.account_adding = false;
                self.account_form = None;
                match result {
                    Ok(_) => return rediscover(),
                    Err(error) => return self.toast(error),
                }
            }
            AccountsAction::CancelAuth(state) => {
                accounts_feature::cancel(&state);
                self.account_adding = false;
                if self
                    .account_auth
                    .as_ref()
                    .is_some_and(|request| request.state == state)
                {
                    self.account_auth = None;
                }
            }
            AccountsAction::Reconnect(id) => {
                return Task::perform(accounts_feature::reconnect(id), |result| {
                    cosmic::action::app(Message::Accounts(AccountsAction::AuthStarted(
                        result.map_err(|error| error.text()),
                    )))
                });
            }
            AccountsAction::Remove(id) => {
                return Task::perform(accounts_feature::remove(id), |result| {
                    cosmic::action::app(Message::Accounts(AccountsAction::Removed(
                        result.map_err(|error| error.text()),
                    )))
                });
            }
            AccountsAction::Removed(result) => match result {
                Ok(id) => {
                    self.storage.lock().unwrap().clear_remote_snapshot(id);
                    self.account_sync.remove(&id);
                    self.refresh_active_accounts();
                    return rediscover();
                }
                Err(error) => return self.toast(error),
            },
            AccountsAction::AddCalDav(form) => {
                self.account_form = Some(form);
                self.account_adding = false;
            }
            AccountsAction::FormFieldChanged(field, value) => {
                if let Some(form) = self.account_form.as_mut() {
                    match field {
                        FormField::Url => form.url = value,
                        FormField::Username => form.username = value,
                        FormField::Password => form.password = value,
                        _ => {}
                    }
                }
            }
            AccountsAction::SubmitCalDav => {
                if let Some(form) = self.account_form.clone() {
                    return Task::perform(accounts_feature::add_caldav(form), |result| {
                        cosmic::action::app(Message::Accounts(AccountsAction::AuthCompleted(
                            result.map_err(|error| error.text()),
                        )))
                    });
                }
            }
            AccountsAction::CancelForm => self.account_form = None,
            AccountsAction::Sync(id) => return self.sync_account(id),
            AccountsAction::Synced(id, request_id, result) => {
                if !matches!(self.account_sync.get(&id), Some(accounts_feature::SyncStatus::Running { request_id: current, .. }) if *current == request_id)
                {
                    return Task::none();
                }

                if self.calendar_popup.as_ref().is_some_and(|popup| matches!(
                    &popup.content,
                    CalendarPopup::Details(_, event, _) if matches!(event.calendar, CalendarKey::Remote { account_id, .. } if account_id == id)
                )) {
                    self.calendar_popup = None;
                }
                let error = match result {
                    Ok(snapshot) => {
                        self.storage
                            .lock()
                            .unwrap()
                            .replace_remote_snapshot(id, snapshot);
                        None
                    }
                    Err(error) => {
                        if matches!(
                            error,
                            accounts_feature::Error::ServerAccessDenied
                                | accounts_feature::Error::GoogleAccessDenied
                                | accounts_feature::Error::GoogleApiDisabled
                                | accounts_feature::Error::GoogleInsufficientScope
                                | accounts_feature::Error::MicrosoftAccessDenied
                                | accounts_feature::Error::NeedsReauth
                        ) {
                            self.storage.lock().unwrap().clear_remote_snapshot(id);
                        }
                        Some(error)
                    }
                };
                self.account_sync.insert(
                    id,
                    accounts_feature::SyncStatus::Finished(std::time::Instant::now(), error),
                );
                self.refresh_calendar_visibility();
            }
            AccountsAction::SetLocallyEnabled(id, enabled) => {
                let mut disabled = self.config.disabled_accounts.clone();
                if enabled {
                    disabled.remove(&id);
                } else {
                    disabled.insert(id);
                }
                if disabled != self.config.disabled_accounts {
                    let mut config = self.config.clone();
                    match cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                        .and_then(|context| config.set_disabled_accounts(&context, disabled))
                    {
                        Ok(_) => {
                            self.config = config;
                            self.refresh_active_accounts();
                            tasks.push(self.sync_active_accounts());
                        }
                        Err(err) => tasks
                            .push(self.toast(fl!("account-save-error", error = err.to_string()))),
                    }
                }
                tasks.push(Task::perform(
                    accounts_feature::set_enabled(id, enabled),
                    |result| {
                        cosmic::action::app(Message::Accounts(AccountsAction::ActionFailed(
                            result.err().map_or_else(String::new, |error| error.text()),
                        )))
                    },
                ));
            }
            AccountsAction::ActionFailed(error) => {
                if !error.is_empty() {
                    tasks.push(self.toast(fl!("account-save-error", error = error)));
                }
            }
        }
        self.update_drawer_layout();
        Task::batch(tasks)
    }
}
