// SPDX-License-Identifier: GPL-3.0-only

use accounts::{Event, Service};
use cosmic::iced::{futures::stream, Subscription};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::app::Message;

use super::{AccountsAction, manager};

/// Reloads Calendar's account list when the embedded manager adds, removes,
/// reconnects, or changes an account. A periodic retry covers early startup
/// before the store has been opened.
pub fn subscription(_ids: Vec<Uuid>) -> Subscription<Message> {
    Subscription::run_with((), |()| {
        stream::unfold((), |()| async move {
            match manager().await {
                Ok(accounts) => {
                    let mut events = accounts.subscribe();
                    loop {
                        match events.recv().await {
                            Ok(Event::AccountsChanged(Service::Calendar))
                            | Err(RecvError::Lagged(_)) => break,
                            Ok(_) => continue,
                            Err(RecvError::Closed) => {
                                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                break;
                            }
                        }
                    }
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_secs(30)).await,
            }
            Some((Message::Accounts(AccountsAction::Changed), ()))
        })
    })
}
