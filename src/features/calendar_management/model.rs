use cosmic::iced::Color;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::shared::ids::CalendarId;

/// Where a [`crate::features::event::model::Event`] originates from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Source {
    /// Created and stored only in this app.
    Local,
    /// Backed by a remote account.
    Synced { provider: String, account_id: Uuid },
}

/// The current on-disk shape of a [`LocalCalendarMeta`]. Bump when the
/// struct's fields change shape and add a matching arm in
/// `src/storage/migration.rs`.
pub const CURRENT_CALENDAR_SCHEMA_VERSION: u32 = 1;

/// A calendar that exists only in local storage (not backed by a provider
/// account), e.g. the default "Personal" calendar seeded on first run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCalendarMeta {
    pub id: CalendarId,
    pub name: String,
    pub description: Option<String>,
    /// Hex color, e.g. "#3584e4".
    pub color: Option<String>,
    pub visible: bool,
    #[serde(default)]
    pub archived: bool,
}

/// A common key that [`crate::features::event::model::Event`] uses to reference its parent
/// calendar, spanning both locally-created calendars and remote provider
/// calendars so both can be addressed uniformly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CalendarKey {
    Local(CalendarId),
    Remote {
        account_id: Uuid,
        /// The calendar ID assigned by the provider.
        calendar_id: String,
    },
}

pub fn calendar_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::from_rgb8(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
    ))
}
