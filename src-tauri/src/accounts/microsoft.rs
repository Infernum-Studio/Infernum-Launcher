use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use super::{Account, AccountKind};
use crate::error::{AppError, AppResult};

const SIGNIN_WINDOW: &str = "msa-signin";
const REDIRECT_PREFIX: &str = "https://login.live.com/oauth20_desktop.srf";
const TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub async fn login(app: &AppHandle) -> AppResult<Option<Account>> {
    let flow = theseus::minecraft_auth::begin_login().await?;
    let url = flow
        .auth_request_uri
        .parse()
        .map_err(|_| AppError::new("msa_failed", "invalid sign-in URL"))?;

    if let Some(old) = app.get_webview_window(SIGNIN_WINDOW) {
        let _ = old.close();
    }
    let window = WebviewWindowBuilder::new(app, SIGNIN_WINDOW, WebviewUrl::External(url))
        .title("Microsoft")
        .inner_size(520.0, 680.0)
        .min_inner_size(420.0, 560.0)
        .center()
        .focused(true)
        .build()
        .map_err(|e| AppError::new("msa_failed", e.to_string()))?;

    let started = std::time::Instant::now();
    while started.elapsed() < TIMEOUT {
        let Ok(current) = window.url() else {
            return Ok(None);
        };
        if current.as_str().starts_with(REDIRECT_PREFIX) {
            let code = current
                .query_pairs()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.into_owned());
            let _ = window.close();
            let code =
                code.ok_or_else(|| AppError::new("msa_failed", "Microsoft returned no code"))?;
            let credentials = theseus::minecraft_auth::finish_login(&code, flow)
                .await
                .map_err(|e| AppError::new("msa_failed", e.to_string()))?;
            let profile = credentials.offline_profile;
            return Ok(Some(Account {
                id: profile.id.hyphenated().to_string(),
                name: profile.name,
                kind: AccountKind::Microsoft,
            }));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = window.close();
    Ok(None)
}

pub async fn prepare_launch(id: &str) -> AppResult<()> {
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|_| AppError::new("account_not_found", "bad account id"))?;
    theseus::minecraft_auth::set_default_user(uuid)
        .await
        .map_err(|_| AppError::new("msa_login_required", "sign in with Microsoft again"))?;
    match theseus::minecraft_auth::get_default_user().await {
        Ok(Some(active)) if active == uuid => Ok(()),
        Ok(_) => Err(AppError::new(
            "msa_login_required",
            "Microsoft session expired",
        )),
        Err(err) => Err(AppError::new("msa_login_required", err.to_string())),
    }
}

pub async fn remove(id: &str) -> AppResult<()> {
    let Ok(uuid) = uuid::Uuid::parse_str(id) else {
        return Ok(());
    };
    theseus::minecraft_auth::remove_user(uuid)
        .await
        .map_err(|err| {
            log::warn!("could not remove Microsoft account from theseus: {err}");
            err.into()
        })
}

pub async fn skin(id: &str) -> Option<(String, bool)> {
    let uuid = uuid::Uuid::parse_str(id).ok()?;
    let users = theseus::minecraft_auth::users().await.ok()?;
    let creds = users.into_iter().find(|c| c.offline_profile.id == uuid)?;
    let profile = creds.online_profile().await?;
    let skin = profile.skins.first()?;
    let slim = format!("{:?}", skin.variant).eq_ignore_ascii_case("slim");
    let url = skin.url.to_string().replacen(
        "http://textures.minecraft.net/",
        "https://textures.minecraft.net/",
        1,
    );
    Some((url, slim))
}
