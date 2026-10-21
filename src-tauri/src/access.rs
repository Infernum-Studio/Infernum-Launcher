//! Acceso a un evento antes de jugar
//!
//! Se llama a la API en cada inicio del juego. Solo puede
//! jugar si hace poco ya se accedio al evento y la sesión es
//! válida.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::api::{self, Access};
use crate::discord_auth::{self, Session};
use crate::error::{AppError, AppResult};
use crate::events::{Event, EventStatus};
use crate::pack_sync::EventState;
use crate::state::AppState;

const OFFLINE_GRANT: chrono::TimeDelta = chrono::TimeDelta::days(7);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGrant {
    pub discord_id: String,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

fn grant_allows(
    grant: Option<&AccessGrant>,
    session: Option<&Session>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    match (grant, session) {
        (Some(grant), Some(session)) => {
            grant.discord_id == session.user.id
                && grant.checked_at <= now
                && now - grant.checked_at < OFFLINE_GRANT
        }
        _ => false,
    }
}

pub async fn check(state: &AppState, event: &Event) -> AppResult<Option<Session>> {
    let session = discord_auth::current(&state.paths).await;
    let token = session.as_ref().map(|s| s.token.as_str());
    let access = match api::event_access(&event.id, token).await {
        Ok(access) => access,
        Err(err) if cfg!(debug_assertions) && err.code == "api_unavailable" => {
            log::warn!("debug build without an API: access not checked");
            return Ok(session);
        }
        Err(err) if event.status == EventStatus::Live => {
            log::warn!("access check for {} failed ({err}); continuing", event.id);
            return Ok(session);
        }
        Err(err) => {
            let saved = EventState::load(&state.paths, &event.id).await;
            if grant_allows(
                saved.access_grant.as_ref(),
                session.as_ref(),
                chrono::Utc::now(),
            ) {
                log::warn!(
                    "access check for {} failed ({err}); using the last one",
                    event.id
                );
                return Ok(session);
            }
            return Err(err);
        }
    };
    match access {
        Access::Allowed => {
            if event.status == EventStatus::Testing
                && let Some(session) = &session
            {
                remember_grant(state, event, session).await;
            }
            Ok(session)
        }
        Access::LoginRequired => Err(AppError::new(
            "login_required",
            "this event needs a Discord login",
        )),
        Access::NotWhitelisted => {
            revoke(state, event).await;
            Err(AppError::new(
                "not_whitelisted",
                "not on this event's test list",
            ))
        }
        Access::Ended => Err(AppError::new("event_ended", "this event has ended")),
        Access::Denied => Err(AppError::new("access_denied", "the API denied access")),
    }
}

async fn remember_grant(state: &AppState, event: &Event, session: &Session) {
    let mut saved = EventState::load(&state.paths, &event.id).await;
    saved.access_grant = Some(AccessGrant {
        discord_id: session.user.id.clone(),
        checked_at: chrono::Utc::now(),
    });
    if let Err(err) = saved.save(&state.paths, &event.id).await {
        log::warn!("could not save the access check for {}: {err}", event.id);
    }
}

/// Si la cuenta ya no esta en whitelist, se elimina el mod
/// del evento y se quita acceso a jugar la instancia
async fn revoke(state: &AppState, event: &Event) {
    let mut saved = EventState::load(&state.paths, &event.id).await;
    if saved.access_grant.is_none() && saved.private_files.is_empty() {
        return;
    }
    let game_dir = match &saved.instance_id {
        Some(id) if state.theseus().await.is_ok() => {
            theseus::instance::get_full_path(id).await.ok()
        }
        _ => None,
    };
    if let Some(game_dir) = &game_dir {
        remove_private_files(game_dir, &saved).await;
    }
    saved.access_grant = None;
    if game_dir.is_some() {
        saved.private_files.clear();
    }
    if let Err(err) = saved.save(&state.paths, &event.id).await {
        log::warn!("could not save the state of {}: {err}", event.id);
    }
    log::info!("removed private mods of {} (no tester role)", event.id);
}

async fn remove_private_files(game_dir: &Path, saved: &EventState) {
    for rel in saved.private_files.keys() {
        let Some(path) = crate::paths::safe_join(game_dir, rel) else {
            continue;
        };
        if tokio::fs::symlink_metadata(&path)
            .await
            .is_ok_and(|m| m.is_file())
            && let Err(err) = tokio::fs::remove_file(&path).await
        {
            log::warn!("could not remove {rel}: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord_auth::DiscordUser;

    fn session(id: &str) -> Session {
        Session {
            token: "t".into(),
            expires_at: chrono::Utc::now() + chrono::TimeDelta::days(7),
            user: DiscordUser {
                id: id.into(),
                name: "n".into(),
            },
        }
    }

    fn grant(id: &str, age: chrono::TimeDelta) -> AccessGrant {
        AccessGrant {
            discord_id: id.into(),
            checked_at: chrono::Utc::now() - age,
        }
    }

    #[test]
    fn offline_play_needs_a_recent_grant_for_the_same_account() {
        let now = chrono::Utc::now();
        let day = chrono::TimeDelta::days(1);
        let a = session("1");
        assert!(grant_allows(Some(&grant("1", day)), Some(&a), now));
        assert!(
            !grant_allows(Some(&grant("2", day)), Some(&a), now),
            "another Discord account"
        );
        assert!(
            !grant_allows(Some(&grant("1", day * 8)), Some(&a), now),
            "too old"
        );
        assert!(
            !grant_allows(Some(&grant("1", -day)), Some(&a), now),
            "from the future"
        );
        assert!(
            !grant_allows(Some(&grant("1", day)), None, now),
            "no session"
        );
        assert!(!grant_allows(None, Some(&a), now), "never checked");
    }

    #[tokio::test]
    async fn removes_only_the_private_files() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::create_dir_all(game.join("config")).unwrap();
        for f in ["mods/hc2.jar", "mods/sodium.jar", "config/hc2.toml"] {
            std::fs::write(game.join(f), b"x").unwrap();
        }
        let mut saved = EventState::default();
        saved
            .private_files
            .insert("mods/hc2.jar".into(), "ab".into());
        saved
            .private_files
            .insert("../outside.jar".into(), "ab".into());

        remove_private_files(game, &saved).await;
        assert!(!game.join("mods/hc2.jar").exists());
        assert!(game.join("mods/sodium.jar").exists());
        assert!(game.join("config/hc2.toml").exists());
    }
}
