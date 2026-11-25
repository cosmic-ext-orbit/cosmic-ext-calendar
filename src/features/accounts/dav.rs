use std::collections::{HashMap, HashSet};
use std::time::Duration;

use accounts::CalendarCredentials;
#[cfg(test)]
use accounts::SecretString;
use reqwest::{header, Client, Method, StatusCode, Url};
use roxmltree::{Document, Node};
use uuid::Uuid;

use super::{Account, Calendar, Error, Snapshot, manager};
use crate::features::{
    calendar_management::model::{CalendarKey, Source},
    event::model::{Event, EventKind, ExternalRef},
    ics,
};
use crate::shared::ids::EventId;

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const DISCOVERY: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
    <d:prop><d:resourcetype/><d:displayname/><d:current-user-principal/>
        <c:calendar-home-set/><c:supported-calendar-component-set/><d:sync-token/></d:prop>
</d:propfind>"#;
const EVENTS: &str = r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
    <d:prop><d:getetag/><c:calendar-data/></d:prop>
    <c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"/></c:comp-filter></c:filter>
</c:calendar-query>"#;

#[derive(Debug)]
enum RequestError {
    Unauthorized,
    InvalidSyncToken,
    Other(Error),
}

impl From<Error> for RequestError {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

pub async fn sync(account: &Account, previous: Snapshot) -> Result<Snapshot, Error> {
    let http = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| Error::Failed)?;

    let credentials = manager().await?.calendar_credentials(account.id).await?;
    match fetch(&http, account, &credentials, &previous).await {
        Err(RequestError::Unauthorized)
            if matches!(credentials, CalendarCredentials::OAuth { .. }) =>
        {
            let refreshed = refresh_credentials(account).await?;
            fetch(&http, account, &refreshed, &previous)
                .await
                .map_err(request_error)
        }
        result => result.map_err(request_error),
    }
}

async fn refresh_credentials(account: &Account) -> Result<CalendarCredentials, Error> {
    let manager = manager().await?;
    manager
        .access_token(account.id, accounts::Service::Calendar, true)
        .await?;
    manager.calendar_credentials(account.id).await.map_err(Error::from)
}

fn request_error(error: RequestError) -> Error {
    match error {
        RequestError::Unauthorized => Error::NeedsReauth,
        RequestError::InvalidSyncToken => Error::Failed,
        RequestError::Other(error) => error,
    }
}

struct Session<'a> {
    client: &'a Client,
    credentials: &'a CalendarCredentials,
    origin: Url,
    google: bool,
}

impl Session<'_> {
    fn resolve(&self, base: &Url, href: &str) -> Result<Url, Error> {
        let url = base.join(href.trim()).map_err(|_| Error::Unsupported)?;
        if url.origin() != self.origin.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Unsupported);
        }
        Ok(url)
    }

    async fn request(
        &self,
        url: &Url,
        method: &str,
        depth: &str,
        body: &str,
    ) -> Result<(Url, String), RequestError> {
        let method = Method::from_bytes(method.as_bytes()).map_err(|_| Error::Failed)?;
        let mut url = url.clone();
        for _ in 0..5 {
            let request = self
                .client
                .request(method.clone(), url.clone())
                .header("Depth", depth)
                .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                .body(body.to_owned());
            let request = match self.credentials {
                CalendarCredentials::Basic {
                    username, password, ..
                } => request.basic_auth(username, Some(password.expose())),
                CalendarCredentials::OAuth { token, .. } => request.bearer_auth(token.expose()),
            };
            let mut response = request.send().await.map_err(|_| Error::Failed)?;
            let status = response.status();
            match status {
                StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT => {
                    let location = response
                        .headers()
                        .get(header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or(Error::Failed)?;
                    url = self.resolve(&url, location)?;
                    continue;
                }
                StatusCode::UNAUTHORIZED => return Err(RequestError::Unauthorized),
                StatusCode::FORBIDDEN | StatusCode::MULTI_STATUS => {}
                _ => return Err(Error::Failed.into()),
            }
            let insufficient_scope = status == StatusCode::FORBIDDEN
                && response
                    .headers()
                    .get_all(header::WWW_AUTHENTICATE)
                    .iter()
                    .any(|value| {
                        value
                            .to_str()
                            .is_ok_and(|value| value.contains("insufficient_scope"))
                    });
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| Error::Failed)? {
                if bytes.len() + chunk.len() > 32 * 1024 * 1024 {
                    return Err(Error::Failed.into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let xml = String::from_utf8(bytes).map_err(|_| Error::Failed)?;
            if status == StatusCode::FORBIDDEN {
                return Err(forbidden_error(self.google, insufficient_scope, &xml));
            }
            return Ok((url, xml));
        }
        Err(Error::Failed.into())
    }

    async fn collections(&self) -> Result<Vec<Calendar>, RequestError> {
        let (url, xml) = self
            .request(&self.origin, "PROPFIND", "0", DISCOVERY)
            .await?;
        let doc = multistatus(&xml)?;
        let mut calendars = self.read_collections(&url, &doc)?;
        if !calendars.is_empty() {
            return Ok(calendars);
        }
        let home = property_href(&doc, CALDAV, "calendar-home-set");
        let home = if let Some(home) = home {
            self.resolve(&url, home)?
        } else if let Some(principal) = property_href(&doc, DAV, "current-user-principal") {
            let principal = self.resolve(&url, principal)?;
            let (principal, xml) = self.request(&principal, "PROPFIND", "0", DISCOVERY).await?;
            let doc = multistatus(&xml)?;
            let home =
                property_href(&doc, CALDAV, "calendar-home-set").ok_or(Error::Unsupported)?;
            self.resolve(&principal, home)?
        } else {
            url
        };

        let (home, xml) = self.request(&home, "PROPFIND", "1", DISCOVERY).await?;
        let doc = multistatus(&xml)?;
        calendars = self.read_collections(&home, &doc)?;
        calendars.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(calendars)
    }

    fn read_collections(&self, base: &Url, doc: &Document<'_>) -> Result<Vec<Calendar>, Error> {
        let mut seen = HashSet::new();
        let mut calendars = Vec::new();
        for response in responses(doc) {
            if !property(response, DAV, "resourcetype")
                .is_some_and(|kind| child(kind, CALDAV, "calendar").is_some())
            {
                continue;
            }
            if property(response, CALDAV, "supported-calendar-component-set").is_some_and(|set| {
                !set.children().any(|entry| {
                    entry.has_tag_name((CALDAV, "comp"))
                        && entry.attribute("name") == Some("VEVENT")
                })
            }) {
                continue;
            }
            let href = child(response, DAV, "href")
                .and_then(|node| node.text())
                .ok_or(Error::Failed)?;
            let url = self.resolve(base, href)?;
            let name = property(response, DAV, "displayname")
                .and_then(|node| node.text())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(url.path())
                .to_owned();
            if seen.insert(url.clone()) {
                let sync_token = property(response, DAV, "sync-token")
                    .and_then(|node| node.text())
                    .filter(|token| !token.is_empty())
                    .map(str::to_owned);
                calendars.push(Calendar {
                    id: url.to_string(),
                    name,
                    sync_token,
                });
            }
        }
        Ok(calendars)
    }
}

fn forbidden_error(google: bool, insufficient_scope: bool, body: &str) -> RequestError {
    if Document::parse(body)
        .ok()
        .is_some_and(|doc| child(doc.root_element(), DAV, "valid-sync-token").is_some())
    {
        return RequestError::InvalidSyncToken;
    }
    if !google {
        return Error::ServerAccessDenied.into();
    }
    if insufficient_scope {
        return Error::GoogleInsufficientScope.into();
    }

    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(body) {
        let error = &payload["error"];
        for field in ["details", "errors"] {
            for entry in error[field].as_array().into_iter().flatten() {
                match entry["reason"].as_str() {
                    Some("SERVICE_DISABLED" | "accessNotConfigured") => {
                        return Error::GoogleApiDisabled.into();
                    }
                    Some("ACCESS_TOKEN_SCOPE_INSUFFICIENT" | "insufficientPermissions") => {
                        return Error::GoogleInsufficientScope.into();
                    }
                    _ => {}
                }
            }
        }
    }
    Error::GoogleAccessDenied.into()
}

fn endpoint(account: &Account, credentials: &CalendarCredentials) -> Result<Url, Error> {
    let uri = match credentials {
        CalendarCredentials::Basic { url, .. } => url,
        CalendarCredentials::OAuth { uri, .. } => uri,
    };
    let mut origin = Url::parse(uri).map_err(|_| Error::Unsupported)?;
    if origin.scheme() != "https"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.fragment().is_some()
    {
        return Err(Error::Unsupported);
    }
    if account.provider == "google"
        && origin.as_str() == "https://apidata.googleusercontent.com/caldav/v2/"
    {
        let email = account
            .email
            .as_deref()
            .filter(|email| !email.is_empty())
            .ok_or(Error::Unsupported)?;
        origin
            .path_segments_mut()
            .map_err(|_| Error::Unsupported)?
            .pop_if_empty()
            .push(email)
            .push("user");
    }
    Ok(origin)
}

async fn fetch(
    client: &Client,
    account: &Account,
    credentials: &CalendarCredentials,
    previous: &Snapshot,
) -> Result<Snapshot, RequestError> {
    let origin = endpoint(account, credentials)?;
    let google =
        account.provider == "google" && origin.host_str() == Some("apidata.googleusercontent.com");
    let session = Session {
        client,
        credentials,
        origin,
        google,
    };
    let mut snapshot = Snapshot::default();
    for mut calendar in session.collections().await? {
        let url = Url::parse(&calendar.id).map_err(|_| Error::Failed)?;
        let prior_token = previous
            .calendars
            .iter()
            .find(|prior| prior.id == calendar.id)
            .and_then(|prior| prior.sync_token.as_deref());

        if let Some(token) = prior_token {
            if calendar.sync_token.as_deref() == Some(token) {
                snapshot
                    .events
                    .extend(calendar_events(previous, &calendar).cloned());
                snapshot.calendars.push(calendar);
                continue;
            }
            match sync_changes(&session, account, &url, token).await {
                Ok((token, changed, events)) => {
                    snapshot.events.extend(
                        calendar_events(previous, &calendar)
                            .filter(|event| {
                                event.external.as_ref().is_some_and(|remote| {
                                    !changed.contains(&remote.provider_event_id)
                                })
                            })
                            .cloned(),
                    );
                    snapshot.events.extend(events);
                    calendar.sync_token = Some(token);
                    snapshot.calendars.push(calendar);
                    continue;
                }
                Err(RequestError::InvalidSyncToken) => {}
                Err(error) => return Err(error),
            }
        }
        let (response_url, xml) = session.request(&url, "REPORT", "1", EVENTS).await?;
        snapshot
            .events
            .extend(parse_event_changes(&session, account, &url, &response_url, &xml)?.1);
        snapshot.calendars.push(calendar);
    }
    Ok(snapshot)
}

fn calendar_events<'a>(
    previous: &'a Snapshot,
    calendar: &Calendar,
) -> impl Iterator<Item = &'a Event> {
    let id = calendar.id.clone();
    previous.events.iter().filter(move |event| {
        matches!(&event.calendar, CalendarKey::Remote { calendar_id, .. } if *calendar_id == id)
    })
}

async fn sync_changes(
    session: &Session<'_>,
    account: &Account,
    url: &Url,
    token: &str,
) -> Result<(String, HashSet<String>, Vec<Event>), RequestError> {
    let token = xml_text(token);
    let body = format!(
        r#"<d:sync-collection xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
        <d:sync-token>{token}</d:sync-token><d:sync-level>1</d:sync-level>
        <d:prop><d:getetag/></d:prop>
    </d:sync-collection>"#
    );
    let (response_url, xml) = session.request(url, "REPORT", "0", &body).await?;
    let doc = multistatus(&xml)?;
    let token = child(doc.root_element(), DAV, "sync-token")
        .and_then(|node| node.text())
        .filter(|token| !token.is_empty())
        .ok_or(Error::Failed)?;
    let token = token.to_owned();
    let mut changed = HashSet::new();
    let mut downloads = Vec::new();
    for response in responses(&doc) {
        let resource = response_url_for(session, &response_url, response)?;
        let status = child(response, DAV, "status").and_then(status_code);
        if status != Some("404") && property(response, DAV, "getetag").is_none() {
            return Err(Error::Failed.into());
        }
        if changed.insert(resource.to_string()) && status != Some("404") {
            downloads.push(resource);
        }
    }

    let mut events = Vec::new();
    for batch in downloads.chunks(100) {
        let mut body = String::from(
            r#"<c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop>"#,
        );
        for resource in batch {
            body.push_str("<d:href>");
            body.push_str(&xml_text(resource.path()));
            if let Some(query) = resource.query() {
                body.push('?');
                body.push_str(&xml_text(query));
            }
            body.push_str("</d:href>");
        }
        body.push_str("</c:calendar-multiget>");
        let (response_url, xml) = session.request(url, "REPORT", "0", &body).await?;
        let (received, mut records) =
            parse_event_changes(session, account, url, &response_url, &xml)?;
        if received.len() != batch.len() || !batch.iter().all(|url| received.contains(url.as_str()))
        {
            return Err(Error::Failed.into());
        }
        events.append(&mut records);
    }
    Ok((token, changed, events))
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn response_url_for(
    session: &Session<'_>,
    base: &Url,
    response: Node<'_, '_>,
) -> Result<Url, Error> {
    let href = child(response, DAV, "href")
        .and_then(|node| node.text())
        .ok_or(Error::Failed)?;
    session.resolve(base, href)
}

fn parse_event_changes(
    session: &Session<'_>,
    account: &Account,
    url: &Url,
    response_url: &Url,
    xml: &str,
) -> Result<(HashSet<String>, Vec<Event>), Error> {
    let doc = multistatus(xml)?;
    let mut events = Vec::new();
    let mut resources = HashSet::new();
    let calendar_id = url.to_string();
    let target = CalendarKey::Remote {
        account_id: account.id,
        calendar_id: calendar_id.clone(),
    };
    for response in responses(&doc) {
        let resource = response_url_for(session, response_url, response)?.to_string();
        if !resources.insert(resource.clone()) {
            continue;
        }
        if child(response, DAV, "status").and_then(status_code) == Some("404") {
            continue;
        }
        let data = property(response, CALDAV, "calendar-data")
            .and_then(|node| node.text())
            .ok_or(Error::Failed)?;
        let etag = property(response, DAV, "getetag")
            .and_then(|node| node.text())
            .map(str::to_owned);
        let mut parsed = ics::parse_import(data, target.clone())
            .map_err(|_| Error::Failed)?
            .events;
        parsed.retain(|event| matches!(event.kind, EventKind::Event));
        let mut ids = HashMap::new();
        for event in &mut parsed {
            let identity = format!(
                "{calendar_id}\n{resource}\n{:?}\n{:?}",
                event.ics_uid,
                event
                    .recurrence_id
                    .map(time::OffsetDateTime::unix_timestamp_nanos)
            );
            let id = EventId(Uuid::new_v5(&account.id, identity.as_bytes()));
            ids.insert(event.id, id);
            event.id = id;
            event.source = Source::Synced {
                provider: account.provider.clone(),
                account_id: account.id,
            };
            event.external = Some(ExternalRef {
                account_id: account.id,
                provider: account.provider.clone(),
                calendar_id: calendar_id.clone(),
                provider_event_id: resource.clone(),
                etag: etag.clone(),
                last_synced: Some(time::OffsetDateTime::now_utc()),
            });
        }
        for event in &mut parsed {
            event.series_id = event.series_id.and_then(|id| ids.get(&id).copied());
        }
        events.extend(parsed);
    }
    Ok((resources, events))
}

fn multistatus(xml: &str) -> Result<Document<'_>, Error> {
    let doc = Document::parse(xml).map_err(|_| Error::Failed)?;
    if !doc.root_element().has_tag_name((DAV, "multistatus")) {
        return Err(Error::Failed);
    }
    Ok(doc)
}

fn responses<'a, 'input>(doc: &'a Document<'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    doc.root_element()
        .children()
        .filter(|node| node.has_tag_name((DAV, "response")))
}

fn child<'a, 'input>(
    node: Node<'a, 'input>,
    namespace: &str,
    name: &str,
) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.has_tag_name((namespace, name)))
}

fn property<'a, 'input>(
    response: Node<'a, 'input>,
    namespace: &str,
    name: &str,
) -> Option<Node<'a, 'input>> {
    response
        .children()
        .filter(|node| node.has_tag_name((DAV, "propstat")))
        .find_map(|status| {
            let code = child(status, DAV, "status")?
                .text()?
                .split_whitespace()
                .nth(1)?;
            if code != "200" {
                return None;
            }
            child(child(status, DAV, "prop")?, namespace, name)
        })
}

fn status_code<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    node.text()?.split_whitespace().nth(1)
}

fn property_href<'a>(doc: &'a Document<'_>, namespace: &str, name: &str) -> Option<&'a str> {
    responses(doc)
        .find_map(|response| child(property(response, namespace, name)?, DAV, "href")?.text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::storage::LocalStorage;

    #[test]
    fn identifies_google_forbidden_responses_without_exposing_the_body() {
        for (scope, body, expected) in [
            (
                false,
                r#"{"error":{"details":[{"reason":"SERVICE_DISABLED"}]}}"#,
                Error::GoogleApiDisabled,
            ),
            (
                false,
                r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#,
                Error::GoogleInsufficientScope,
            ),
            (true, "", Error::GoogleInsufficientScope),
            (false, "Forbidden", Error::GoogleAccessDenied),
        ] {
            assert_eq!(request_error(forbidden_error(true, scope, body)), expected);
        }
        assert_eq!(
            request_error(forbidden_error(false, false, "Forbidden")),
            Error::ServerAccessDenied
        );
        assert!(matches!(
            forbidden_error(
                true,
                false,
                r#"<error xmlns="DAV:"><valid-sync-token/></error>"#
            ),
            RequestError::InvalidSyncToken
        ));
    }

    #[test]
    fn google_uses_the_primary_calendar_principal() {
        let account = Account {
            id: Uuid::nil(),
            email: Some("person+calendar@example.org".into()),
            provider: "google".into(),
            server: None,
        };
        let credentials = CalendarCredentials::OAuth {
            uri: "https://apidata.googleusercontent.com/caldav/v2/".into(),
            token: SecretString::new(String::new()),
            expires_at: None,
        };
        assert_eq!(
            endpoint(&account, &credentials).unwrap().as_str(),
            "https://apidata.googleusercontent.com/caldav/v2/person+calendar@example.org/user",
        );
    }

    #[tokio::test]
    async fn replacing_a_snapshot_preserves_identity_and_removes_deleted_events() {
        let url = Url::parse("https://example.org/calendar/").unwrap();
        let credentials = CalendarCredentials::Basic {
            url: url.to_string(),
            username: "me".into(),
            password: SecretString::new("test"),
        };
        let client = Client::new();
        let session = Session {
            client: &client,
            credentials: &credentials,
            origin: url.clone(),
            google: false,
        };
        let account = Account {
            id: Uuid::new_v4(),
            email: None,
            provider: "caldav".into(),
            server: None,
        };
        let response = |summary: &str| {
            format!(
                r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
<d:response><d:href>/calendar/one.ics</d:href><d:propstat><d:prop>
<d:getetag>"etag"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:shared-uid
DTSTART:20260101T100000Z
DTEND:20260101T110000Z
SUMMARY:{summary}
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#
            )
        };
        let mut storage = LocalStorage::unavailable("test store".into());
        let first = parse_event_changes(&session, &account, &url, &url, &response("First"))
            .unwrap()
            .1;
        let id = first[0].id;
        storage.replace_remote_snapshot(
            account.id,
            Snapshot {
                calendars: vec![],
                events: first,
            },
        );

        let updated = parse_event_changes(&session, &account, &url, &url, &response("Updated"))
            .unwrap()
            .1;
        storage.replace_remote_snapshot(
            account.id,
            Snapshot {
                calendars: vec![],
                events: updated,
            },
        );
        assert_eq!(storage.all_events().len(), 1);
        assert_eq!(storage.get_event(id).unwrap().summary, "Updated");

        let other_origin =
            response("Updated").replace("/calendar/one.ics", "https://other.example/one.ics");
        assert!(matches!(
            parse_event_changes(&session, &account, &url, &url, &other_origin),
            Err(Error::Unsupported)
        ));
        let deletion = r#"<d:multistatus xmlns:d="DAV:">
            <d:response><d:href>/calendar/one.ics</d:href>
            <d:status>HTTP/1.1 404 Not Found</d:status></d:response>
        </d:multistatus>"#;
        let (changed, events) =
            parse_event_changes(&session, &account, &url, &url, deletion).unwrap();
        assert!(changed.contains("https://example.org/calendar/one.ics"));
        assert!(events.is_empty());
        storage.replace_remote_snapshot(
            account.id,
            Snapshot {
                calendars: vec![],
                events,
            },
        );
        assert!(storage.get_event(id).is_none());
    }
}
