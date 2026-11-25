// SPDX-License-Identifier: GPL-3.0-only

mod dav;
mod graph;
pub mod subscription;
pub mod view;

use accounts::{
    Account as ManagedAccount, Accounts, CalDavSettings, Config, Endpoint, Error as AccountsError,
    Provider, SecretString, Service,
};
use tokio::sync::OnceCell;
use uuid::Uuid;

pub use accounts::AuthRequest;

use crate::{features::event::model::Event, fl};

const APP_ID: &str = "dev.edfloreshz.Calendar";
static MANAGER: OnceCell<Accounts> = OnceCell::const_new();

/// Calendar's local view of an embedded account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    pub id: Uuid,
    pub email: Option<String>,
    pub provider: String,
    pub server: Option<String>,
}

impl Account {
    pub fn address(&self) -> Option<&str> {
        self.email
            .as_deref()
            .filter(|email| !email.is_empty())
            .or(self.server.as_deref())
    }

    pub fn provider_label(&self) -> &str {
        match self.provider.as_str() {
            "google" => "Google",
            "microsoft" => "Microsoft",
            "custom-caldav" => "CalDAV",
            provider => provider,
        }
    }

    fn from_managed(account: &ManagedAccount) -> Self {
        let server = match &account.endpoint {
            Endpoint::CalDav(settings) => reqwest::Url::parse(&settings.url)
                .ok()
                .and_then(|url| {
                    url.host_str().map(|host| {
                        url.port()
                            .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"))
                    })
                }),
            _ => None,
        };
        Self {
            id: account.id,
            email: account.email.clone(),
            provider: account.provider.as_str().to_owned(),
            server,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    StorageUnavailable,
    Configuration(String),
    Authentication(String),
    NeedsReauth,
    ServerAccessDenied,
    GoogleAccessDenied,
    GoogleApiDisabled,
    GoogleInsufficientScope,
    Cancelled,
    Failed,
    Unsupported,
    MicrosoftAccessDenied,
    Storage(String),
}

impl From<AccountsError> for Error {
    fn from(error: AccountsError) -> Self {
        let description = error.to_string();
        match error {
            AccountsError::StorageUnavailable(_) => Self::StorageUnavailable,
            AccountsError::StorageCorrupt(message) => Self::Storage(message),
            AccountsError::InvalidClientConfig(message) => Self::Configuration(message),
            AccountsError::MissingClientConfig(provider, service) => {
                Self::Configuration(format!("no client configuration for {provider:?}/{service}"))
            }
            AccountsError::AuthenticationFailed(message) => Self::Authentication(message),
            AccountsError::InvalidState | AccountsError::CallbackTimeout => {
                Self::Authentication(description)
            }
            AccountsError::FeatureDisabled(_) | AccountsError::UnsupportedProvider(_, _) => {
                Self::Unsupported
            }
            AccountsError::AccountNotFound(_) | AccountsError::AccountAlreadyExists(_, _) => {
                Self::Failed
            }
            AccountsError::NeedsReauth(_) => Self::NeedsReauth,
            AccountsError::Cancelled => Self::Cancelled,
            AccountsError::InvalidEndpoint(message)
            | AccountsError::Network(message)
            | AccountsError::ProviderResponse(message) => Self::Storage(message),
        }
    }
}

impl Error {
    pub fn text(&self) -> String {
        match self {
            Self::StorageUnavailable => fl!("accounts-error-unavailable"),
            Self::NeedsReauth => fl!("accounts-error-needs-reauth"),
            Self::ServerAccessDenied => fl!("accounts-error-server-denied"),
            Self::GoogleAccessDenied => fl!("accounts-error-google-denied"),
            Self::GoogleApiDisabled => fl!("accounts-error-google-api-disabled"),
            Self::GoogleInsufficientScope => fl!("accounts-error-google-scope"),
            Self::Failed => fl!("accounts-error-failed"),
            Self::Unsupported => fl!("accounts-error-unsupported"),
            Self::MicrosoftAccessDenied => fl!("accounts-error-microsoft-denied"),
            Self::Configuration(message)
            | Self::Authentication(message)
            | Self::Storage(message) => message.clone(),
            Self::Cancelled => "Sign-in was cancelled.".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub sync_token: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub calendars: Vec<Calendar>,
    pub events: Vec<Event>,
}

pub async fn sync(account: &Account, previous: Snapshot) -> Result<Snapshot, Error> {
    match account.provider.as_str() {
        "microsoft" => graph::sync(account).await,
        _ => dav::sync(account, previous).await,
    }
}

#[derive(Clone, Debug)]
pub enum AccountsAction {
    Discover,
    Discovered(Result<Discovery, Error>),
    Changed,
    SetLocallyEnabled(Uuid, bool),
    Sync(Uuid),
    Synced(Uuid, u64, Result<Snapshot, Error>),
    AddProvider(Provider),
    AuthStarted(Result<AuthRequest, String>),
    AuthCompleted(Result<ManagedAccount, String>),
    CancelAuth(String),
    Reconnect(Uuid),
    Remove(Uuid),
    Removed(Result<Uuid, String>),
    AddCalDav(AccountForm),
    FormFieldChanged(FormField, String),
    SubmitCalDav,
    CancelForm,
    ActionFailed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormField {
    Url,
    Username,
    Password,
}

#[derive(Clone, Debug, Default)]
pub struct AccountForm {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug)]
pub struct Discovery {
    pub accounts: Vec<Account>,
    pub watched_ids: Vec<Uuid>,
}

pub async fn manager() -> Result<&'static Accounts, Error> {
    MANAGER
        .get_or_try_init(|| async {
            let config = Config::new(APP_ID)
                .with_client_from_env(Provider::Google, Service::Calendar)
                .with_client_from_env(Provider::Microsoft, Service::Calendar);
            Accounts::new(config).await
        })
        .await
        .map_err(Error::from)
}

fn open_url(request: &AuthRequest) -> Result<(), Error> {
    open::that(&request.url)
        .map_err(|error| Error::Authentication(format!("could not open browser: {error}")))
}

pub async fn discover() -> Result<Discovery, Error> {
    let accounts = manager().await?;
    let managed = accounts.list(Service::Calendar).await;
    let watched_ids: Vec<_> = managed.iter().map(|account| account.id).collect();
    let views: Vec<_> = managed.iter().map(Account::from_managed).collect();
    Ok(Discovery {
        accounts: views,
        watched_ids,
    })
}

pub async fn begin_provider(provider: Provider) -> Result<AuthRequest, Error> {
    let request = manager().await?.begin_auth(provider, Service::Calendar).await?;
    open_url(&request)?;
    Ok(request)
}

pub async fn reconnect(id: Uuid) -> Result<AuthRequest, Error> {
    let request = manager().await?.reconnect(id).await?;
    open_url(&request)?;
    Ok(request)
}

pub async fn complete(request: AuthRequest) -> Result<ManagedAccount, Error> {
    manager().await?.complete_auth(request).await.map_err(Error::from)
}

pub fn cancel(state: &str) {
    if let Some(manager) = MANAGER.get() {
        let state = state.to_owned();
        tokio::spawn(async move {
            manager.cancel_auth(&state).await;
        });
    }
}

pub async fn remove(id: Uuid) -> Result<Uuid, Error> {
    manager().await?.remove(id).await?;
    Ok(id)
}

pub async fn set_enabled(id: Uuid, enabled: bool) -> Result<(), Error> {
    manager()
        .await?
        .set_enabled(id, enabled)
        .await
        .map_err(Error::from)
}

pub async fn add_caldav(form: AccountForm) -> Result<ManagedAccount, Error> {
    manager()
        .await?
        .add_caldav_account(
            CalDavSettings { url: form.url },
            form.username,
            SecretString::new(form.password),
        )
        .await
        .map_err(Error::from)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncStatus {
    Running {
        request_id: u64,
        previous_error: Option<Error>,
    },
    Finished(std::time::Instant, Option<Error>),
}

impl SyncStatus {
    pub fn error(&self) -> Option<Error> {
        match self {
            Self::Running { previous_error, .. } => previous_error.clone(),
            Self::Finished(_, error) => error.clone(),
        }
    }
}
