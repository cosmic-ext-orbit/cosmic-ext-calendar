use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;

use cosmic::widget::combo_box;
use serde::Deserialize;

use super::model::Event;

const MAX_ONLINE_SUGGESTIONS: usize = 6;
pub const PUBLIC_PHOTON_ENDPOINT: &str = "https://photon.komoot.io";

pub struct LocationSuggestions {
    pub choices: combo_box::State<String>,
    pub online: Option<OnlinePlaces>,
    pub online_failed: bool,
    recent: Vec<String>,
}

impl LocationSuggestions {
    pub fn new<'a>(events: impl IntoIterator<Item = &'a Event>, value: &str) -> Self {
        let mut locations: Vec<_> = events
            .into_iter()
            .filter_map(|event| {
                let name = event.location.as_deref()?.trim();
                (!name.is_empty()).then_some((event.updated_at, name))
            })
            .collect();
        locations.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let recent = unique_locations(locations.into_iter().map(|(_, name)| name));
        Self {
            choices: combo_box::State::with_selection(recent.clone(), Some(&value.to_owned())),
            recent,
            online: None,
            online_failed: false,
        }
    }

    pub fn clear_online(&mut self, value: &str) {
        if self.online.take().is_some() {
            self.reset_search(value);
        }
        self.online_failed = false;
    }

    pub fn select(&mut self, value: &str) {
        self.online = None;
        self.online_failed = false;
        self.reset_search(value);
    }

    fn reset_search(&mut self, value: &str) {
        self.choices =
            combo_box::State::with_selection(self.recent.clone(), Some(&value.to_owned()));
    }

    pub fn set_online(&mut self, places: OnlinePlaces, value: &str) {
        let options =
            unique_locations(self.recent.iter().chain(&places.labels).map(String::as_str));
        self.choices = combo_box::State::with_selection(options, Some(&value.to_owned()));
        self.online = Some(places);
    }
}

fn unique_locations<'a>(locations: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    locations
        .into_iter()
        .map(str::trim)
        .filter(|name| !name.is_empty() && seen.insert(name.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Debug)]
pub struct OnlinePlaces {
    labels: Vec<String>,
}

impl OnlinePlaces {
    pub fn has_results(&self) -> bool {
        !self.labels.is_empty()
    }
}

#[derive(Deserialize)]
struct Response {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    properties: Place,
}

#[derive(Deserialize)]
struct Place {
    name: Option<String>,
    housenumber: Option<String>,
    street: Option<String>,
    district: Option<String>,
    city: Option<String>,
    county: Option<String>,
    state: Option<String>,
    postcode: Option<String>,
    country: Option<String>,
}

impl Place {
    fn label(&self) -> String {
        let street_address = [self.housenumber.as_deref(), self.street.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let locality = self
            .city
            .as_deref()
            .or(self.district.as_deref())
            .or(self.county.as_deref());
        let locality = [locality, self.postcode.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let name = self.name.as_deref().filter(|name| {
            !self
                .street
                .as_deref()
                .is_some_and(|street| street.eq_ignore_ascii_case(name))
        });
        unique_locations(
            [
                name,
                Some(street_address.as_str()),
                Some(locality.as_str()),
                self.state.as_deref(),
                self.country.as_deref(),
            ]
            .into_iter()
            .flatten(),
        )
        .join(", ")
    }
}

impl Response {
    fn suggestions(self) -> OnlinePlaces {
        let labels: Vec<_> = self
            .features
            .into_iter()
            .take(MAX_ONLINE_SUGGESTIONS)
            .map(|feature| feature.properties.label())
            .collect();
        OnlinePlaces {
            labels: unique_locations(labels.iter().map(String::as_str)),
        }
    }
}

pub fn photon_api_url(endpoint: &str) -> Option<reqwest::Url> {
    let mut url = reqwest::Url::parse(endpoint.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }

    let mut path = url.path_segments_mut().ok()?;
    path.pop_if_empty();
    drop(path);
    if !url.path().ends_with("/api") {
        url.path_segments_mut().ok()?.push("api");
    }
    url.path_segments_mut().ok()?.push("");
    Some(url)
}

pub async fn find_online(query: String, endpoint: reqwest::Url) -> Result<OnlinePlaces, ()> {
    tokio::time::sleep(Duration::from_millis(400)).await;
    static CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("cosmic-ext-calendar/", env!("CARGO_PKG_VERSION")))
            .build()
    });
    let response = CLIENT
        .as_ref()
        .map_err(|_| ())?
        .get(endpoint)
        .query(&[("q", query.as_str())])
        .query(&[("limit", MAX_ONLINE_SUGGESTIONS)])
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?;
    let places = response.json::<Response>().await.map_err(|_| ())?;
    Ok(places.suggestions())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_deduplication_keeps_first_spelling_and_order() {
        assert_eq!(
            unique_locations(["  Main Office ", "main office", "Room 201", " "]),
            ["Main Office", "Room 201"]
        );
    }

    #[test]
    fn photon_formats_place_names_and_addresses() {
        let places: Response = serde_json::from_str(
            r#"{"type":"FeatureCollection","features":[{"properties":{"name":"Corner Cafe","street":"Main Street","housenumber":"12","city":"Tucson","state":"Arizona","country":"United States"}},{"properties":{"name":"Broadway","street":"Broadway","housenumber":"8","city":"Tucson"}}]}"#,
        )
        .unwrap();
        let places = places.suggestions();
        assert_eq!(
            places.labels,
            [
                "Corner Cafe, 12 Main Street, Tucson, Arizona, United States",
                "8 Broadway, Tucson"
            ]
        );
        let mut suggestions = LocationSuggestions::new([], "");
        suggestions.set_online(places, "tucson");
        assert_eq!(suggestions.choices.options().len(), 2);
    }

    #[test]
    fn photon_endpoint_accepts_base_and_api_urls() {
        assert_eq!(
            photon_api_url(PUBLIC_PHOTON_ENDPOINT).unwrap().as_str(),
            "https://photon.komoot.io/api/",
        );
        assert_eq!(
            photon_api_url("http://localhost:2322/maps/api/")
                .unwrap()
                .as_str(),
            "http://localhost:2322/maps/api/",
        );
        assert!(photon_api_url("https://example.com/?q=ignored").is_none());
        assert!(photon_api_url("file:///tmp/photon").is_none());
    }
}
