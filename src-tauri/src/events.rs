//! events.json contiene la url y el sha512 de los modpacks
//! Esta firmado y se comprueba con la key pública del launcher

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::paths::Paths;

pub type Localized = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventStatus {
    Hidden,
    Testing,
    Live,
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RamHint {
    pub min_mb: u32,
    pub recommended_mb: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventArt {
    pub background: Option<String>,
    pub logo: Option<String>,
    pub accent: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDiscord {
    pub large_image: Option<String>,
    pub large_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    NeoForge,
    Fabric,
}

impl Loader {
    pub fn display_name(self) -> Option<&'static str> {
        match self {
            Self::Vanilla => None,
            Self::NeoForge => Some("NeoForge"),
            Self::Fabric => Some("Fabric"),
        }
    }

    pub fn to_theseus(self) -> theseus::prelude::ModLoader {
        use theseus::prelude::ModLoader;
        match self {
            Self::Vanilla => ModLoader::Vanilla,
            Self::NeoForge => ModLoader::NeoForge,
            Self::Fabric => ModLoader::Fabric,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(default)]
    pub name: Localized,
    pub status: EventStatus,
    pub pack_url: String,
    pub pack_sha512: String,
    pub pack_version: String,
    pub pack_size: Option<u64>,
    pub minecraft: Option<String>,
    pub loader: Option<Loader>,
    pub loader_version: Option<String>,
    pub private_mods_url: Option<String>,
    pub ram: Option<RamHint>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub news_url: Option<String>,
    #[serde(default)]
    pub art: EventArt,
    #[serde(default)]
    pub discord: EventDiscord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsFile {
    pub schema_version: u32,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsList {
    pub events: Vec<Event>,
    pub offline: bool,
}

pub fn is_valid_event_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn parse(bytes: &[u8]) -> AppResult<EventsFile> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        schema_version: u32,
    }
    let header: Header = serde_json::from_slice(bytes)?;
    if header.schema_version > crate::config::SUPPORTED_SCHEMA_VERSION {
        return Err(AppError::new(
            "launcher_outdated",
            format!(
                "events.json schema {} is newer than supported",
                header.schema_version
            ),
        ));
    }
    let mut file: EventsFile = serde_json::from_slice(bytes)?;
    file.events.retain(|e| {
        let ok = is_valid_event_id(&e.id);
        if !ok {
            log::warn!("ignoring event with invalid id {:?}", e.id);
        }
        ok
    });
    Ok(file)
}

fn visible(file: EventsFile) -> Vec<Event> {
    file.events
        .into_iter()
        .filter(|e| e.status != EventStatus::Hidden)
        .collect()
}

pub async fn cached(paths: &Paths) -> Option<EventsFile> {
    let bytes = tokio::fs::read(paths.events_cache()).await.ok()?;
    parse(&bytes).ok()
}

pub struct Source {
    pub url: String,
    pub public_key: String,
}

impl Source {
    pub fn configured() -> Self {
        Self {
            url: crate::config::events_url(),
            public_key: crate::config::events_public_key(),
        }
    }
}

pub async fn list(paths: &Paths, refresh: bool) -> AppResult<EventsList> {
    list_from(paths, refresh, &Source::configured()).await
}

async fn list_from(paths: &Paths, refresh: bool, source: &Source) -> AppResult<EventsList> {
    if !refresh {
        return Ok(EventsList {
            events: cached(paths).await.map(visible).unwrap_or_default(),
            offline: false,
        });
    }
    match fetch(paths, source).await {
        Ok(file) => Ok(EventsList {
            events: visible(file),
            offline: false,
        }),
        Err(err) if err.code == "launcher_outdated" => Err(err),
        Err(err) => {
            log::warn!("events refresh failed: {err}");
            match cached(paths).await {
                Some(file) => Ok(EventsList {
                    events: visible(file),
                    offline: true,
                }),
                None => Err(err),
            }
        }
    }
}

async fn fetch(paths: &Paths, source: &Source) -> AppResult<EventsFile> {
    let bytes = crate::net::fetch_bytes(&source.url, 1024 * 1024).await?;
    if source.public_key.is_empty() {
        log::warn!("debug build without an events.json key: signature not checked");
    } else {
        let signature = crate::net::fetch_bytes(&format!("{}.sig", source.url), 16 * 1024).await?;
        verify_signature(&bytes, &signature, &source.public_key)?;
    }
    let file = parse(&bytes)?;
    crate::paths::write_atomic(&paths.events_cache(), &bytes).await?;
    Ok(file)
}

fn bad_signature(message: impl Into<String>) -> AppError {
    AppError::new("events_signature", message)
}

fn minisign_text(raw: &[u8]) -> Option<String> {
    use base64::Engine;
    let text = std::str::from_utf8(raw).ok()?.trim();
    if text.starts_with("untrusted comment:") {
        return Some(text.to_string());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(text)
        .ok()?;
    String::from_utf8(decoded).ok()
}

pub fn verify_signature(bytes: &[u8], signature: &[u8], public_key: &str) -> AppResult<()> {
    let key = minisign_text(public_key.as_bytes())
        .and_then(|t| minisign_verify::PublicKey::decode(&t).ok())
        .ok_or_else(|| bad_signature("the built-in events.json public key is invalid"))?;
    let signature = minisign_text(signature)
        .and_then(|t| minisign_verify::Signature::decode(&t).ok())
        .ok_or_else(|| bad_signature("events.json.sig is not a minisign signature"))?;
    key.verify(bytes, &signature, false)
        .map_err(|err| bad_signature(format!("events.json signature: {err}")))
}

pub async fn get(paths: &Paths, id: &str) -> AppResult<Event> {
    cached(paths)
        .await
        .and_then(|f| f.events.into_iter().find(|e| e.id == id))
        .ok_or_else(|| AppError::new("event_not_found", format!("unknown event {id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schemaVersion": 1,
      "events": [
        { "id": "hellcraft2", "name": { "es": "HellCraft 2", "en": "HellCraft 2" },
          "status": "testing", "packUrl": "https://x/1.mrpack", "packSha512": "ab",
          "packVersion": "1.0.0", "ram": { "minMb": 2048, "recommendedMb": 3072 },
          "startsAt": "2026-11-20T18:00:00Z" },
        { "id": "Bad-Id", "name": {}, "status": "live", "packUrl": "", "packSha512": "", "packVersion": "" }
      ]
    }"#;

    #[test]
    fn parses_and_drops_invalid_ids() {
        let file = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(file.events.len(), 1);
        assert_eq!(file.events[0].status, EventStatus::Testing);
    }

    #[test]
    fn rejects_newer_schema() {
        let err = parse(br#"{"schemaVersion": 2, "events": []}"#).unwrap_err();
        assert_eq!(err.code, "launcher_outdated");
    }

    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDA4MDM2QzNFRUU0MTQyNkYKUldSdlFrSHVQbXdEQ085azFGdkNoTkI4Z3hRT294TWdsOVdmcGJWR2JlQkRYOHJuSFpmMWVBTEYK";
    const SIGNED: &str = r#"{"schemaVersion":1,"events":[{"id":"signed_event","status":"live","packUrl":"https://x/p.mrpack","packSha512":"ab","packVersion":"1.0.0"}]}"#;
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSdlFrSHVQbXdEQ0RDempvU1djdVR5MHFPeVpmUTZXa0l4UHRlRm9oOHp5NkZnV3M5Y29mN09QMVBSNXVUMjN4Q3RHYTR5cmYxQjF1MXRmSVZTdmg4UWIwWDBYVlR5YWdZPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNjY1NzIyCWZpbGU6ZXZlbnRzLmpzb24KVjFYeXpoWGFmYmo0OWMwYTNjMFg0K0tJWlhIWENIb2RzaXBHRWpFM0ZWWjBsQWsyVVZ0RTZHZm82UW15bGJjMlZLekZHcVRZbzNoRnU5cEJBQ2lTQWc9PQo=";

    #[test]
    fn accepts_only_validly_signed_events() {
        verify_signature(SIGNED.as_bytes(), SIGNATURE.as_bytes(), TEST_KEY).unwrap();

        let tampered = SIGNED.replace("\"ab\"", "\"cd\"");
        let err = verify_signature(tampered.as_bytes(), SIGNATURE.as_bytes(), TEST_KEY);
        assert_eq!(err.unwrap_err().code, "events_signature");

        let err = verify_signature(SIGNED.as_bytes(), b"garbage", TEST_KEY);
        assert_eq!(err.unwrap_err().code, "events_signature");
        let err = verify_signature(SIGNED.as_bytes(), SIGNATURE.as_bytes(), "bad key");
        assert_eq!(err.unwrap_err().code, "events_signature");
    }

    fn source(dir: &std::path::Path, public_key: &str) -> Source {
        Source {
            url: format!("file://{}", dir.join("events.json").display()),
            public_key: public_key.to_string(),
        }
    }

    #[tokio::test]
    async fn signed_list_is_cached_and_bad_signature_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().join("root"));
        std::fs::write(dir.path().join("events.json"), SIGNED).unwrap();
        std::fs::write(dir.path().join("events.json.sig"), SIGNATURE).unwrap();

        let list = list_from(&paths, true, &source(dir.path(), TEST_KEY))
            .await
            .unwrap();
        assert!(!list.offline);
        assert_eq!(list.events[0].id, "signed_event");

        std::fs::write(
            dir.path().join("events.json"),
            SIGNED.replace("signed_event", "evil_event"),
        )
        .unwrap();
        let list = list_from(&paths, true, &source(dir.path(), TEST_KEY))
            .await
            .unwrap();
        assert!(list.offline);
        assert_eq!(list.events[0].id, "signed_event");
        assert!(get(&paths, "evil_event").await.is_err());
    }

    #[tokio::test]
    async fn unreachable_list_without_cache_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().join("root"));
        let err = list_from(&paths, true, &source(dir.path(), TEST_KEY))
            .await
            .unwrap_err();
        assert_eq!(err.code, "io");
    }

    #[tokio::test]
    async fn newer_schema_is_reported_even_with_a_cache() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().join("root"));
        crate::paths::write_atomic(&paths.events_cache(), SIGNED.as_bytes())
            .await
            .unwrap();
        std::fs::write(
            dir.path().join("events.json"),
            r#"{"schemaVersion": 99, "events": []}"#,
        )
        .unwrap();
        let err = list_from(&paths, true, &source(dir.path(), ""))
            .await
            .unwrap_err();
        assert_eq!(err.code, "launcher_outdated");
    }
}
