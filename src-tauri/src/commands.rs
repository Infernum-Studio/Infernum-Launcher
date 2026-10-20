use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::accounts::{self, Account};
use crate::discord_auth::{self, LoginOutcome, SessionInfo};
use crate::error::{AppError, AppResult};
use crate::events::{self, EventsList};
use crate::settings::{self, Settings};
use crate::state::AppState;

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.read().await.clone())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    onboarding_done: Option<bool>,
    language: Option<String>,
    ram_mb: Option<u32>,
    jvm_args: Option<String>,
    discord_rpc: Option<bool>,
    close_on_launch: Option<bool>,
    log_reports: Option<bool>,
}

#[tauri::command]
pub async fn settings_set(state: State<'_, AppState>, patch: SettingsPatch) -> AppResult<Settings> {
    if let Some(args) = &patch.jvm_args {
        settings::validate_jvm_args(args)
            .map_err(|code| AppError::new(code, "invalid JVM arguments"))?;
    }
    if let Some(lang) = &patch.language
        && !settings::LANGUAGES.contains(&lang.as_str())
    {
        return Err(AppError::new(
            "invalid_input",
            format!("unsupported language {lang}"),
        ));
    }
    let mut current = state.settings.write().await;
    let mut updated = current.clone();
    if let Some(v) = patch.onboarding_done {
        updated.onboarding_done = v;
    }
    if let Some(v) = patch.language {
        updated.language = v;
    }
    if let Some(v) = patch.ram_mb {
        updated.ram_mb = v;
    }
    if let Some(v) = patch.jvm_args {
        updated.jvm_args = v;
    }
    if let Some(v) = patch.discord_rpc {
        updated.discord_rpc = v;
    }
    if let Some(v) = patch.close_on_launch {
        updated.close_on_launch = v;
    }
    if let Some(v) = patch.log_reports {
        updated.log_reports = v;
    }
    updated.sanitize(state.total_ram_mb);
    updated.save(&state.paths.settings_file()).await?;
    *current = updated.clone();
    Ok(updated)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    total_ram_mb: u64,
    max_ram_mb: u32,
    min_ram_mb: u32,
    os: &'static str,
    version: &'static str,
    msa_enabled: bool,
}

#[tauri::command]
pub fn system_info(state: State<'_, AppState>) -> SystemInfo {
    SystemInfo {
        total_ram_mb: state.total_ram_mb,
        max_ram_mb: settings::max_ram_mb(state.total_ram_mb),
        min_ram_mb: settings::MIN_RAM_MB,
        os: std::env::consts::OS,
        version: env!("CARGO_PKG_VERSION"),
        msa_enabled: crate::config::MSA_ENABLED,
    }
}

#[tauri::command]
pub async fn events_list(state: State<'_, AppState>, refresh: bool) -> AppResult<EventsList> {
    events::list(&state.paths, refresh).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsList {
    accounts: Vec<Account>,
    active: Option<String>,
}

async fn accounts_snapshot(state: &AppState) -> AccountsList {
    let settings = state.settings.read().await;
    AccountsList {
        accounts: settings.accounts.clone(),
        active: settings.active_account.clone(),
    }
}

async fn update_accounts(
    state: &AppState,
    f: impl FnOnce(&mut Settings),
) -> AppResult<AccountsList> {
    {
        let mut current = state.settings.write().await;
        let mut updated = current.clone();
        f(&mut updated);
        updated.sanitize(state.total_ram_mb);
        updated.save(&state.paths.settings_file()).await?;
        *current = updated;
    }
    Ok(accounts_snapshot(state).await)
}

#[tauri::command]
pub async fn accounts_list(state: State<'_, AppState>) -> AppResult<AccountsList> {
    Ok(accounts_snapshot(&state).await)
}

#[tauri::command]
pub async fn accounts_add_offline(
    state: State<'_, AppState>,
    name: String,
) -> AppResult<AccountsList> {
    let account = accounts::offline::new_account(name.trim()).ok_or_else(|| {
        AppError::new(
            "invalid_nickname",
            "nickname must match ^[A-Za-z0-9_]{3,16}$",
        )
    })?;
    update_accounts(&state, |settings| {
        if !settings.accounts.iter().any(|a| a.id == account.id) {
            settings.accounts.push(account.clone());
        }
        settings.active_account = Some(account.id.clone());
    })
    .await
}

#[tauri::command]
pub async fn accounts_login_microsoft(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<AccountsList>> {
    if !crate::config::MSA_ENABLED {
        return Err(AppError::new(
            "msa_disabled",
            "Microsoft login is disabled in this build",
        ));
    }
    state.theseus().await?;
    let Some(account) = accounts::microsoft::login(&app).await? else {
        return Ok(None);
    };
    let list = update_accounts(&state, |settings| {
        settings.accounts.retain(|a| a.id != account.id);
        settings.accounts.push(account.clone());
        settings.active_account = Some(account.id.clone());
    })
    .await?;
    Ok(Some(list))
}

#[tauri::command]
pub async fn account_skin(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<accounts::skins::SkinData> {
    let account = state
        .settings
        .read()
        .await
        .accounts
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or_else(|| AppError::new("account_not_found", "unknown account"))?;
    if account.kind == accounts::AccountKind::Microsoft {
        state.theseus().await?;
    }
    accounts::skins::get(&state.paths, &account).await
}

#[tauri::command]
pub async fn accounts_remove(state: State<'_, AppState>, id: String) -> AppResult<AccountsList> {
    let is_microsoft = state
        .settings
        .read()
        .await
        .accounts
        .iter()
        .any(|a| a.id == id && a.kind == accounts::AccountKind::Microsoft);
    if is_microsoft {
        state.theseus().await?;
        accounts::microsoft::remove(&id).await?;
    }
    update_accounts(&state, |settings| {
        settings.accounts.retain(|a| a.id != id);
        if settings.active_account.as_deref() == Some(id.as_str()) {
            settings.active_account = settings.accounts.first().map(|a| a.id.clone());
        }
    })
    .await
}

#[tauri::command]
pub async fn accounts_set_active(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<AccountsList> {
    if !state
        .settings
        .read()
        .await
        .accounts
        .iter()
        .any(|a| a.id == id)
    {
        return Err(AppError::new("account_not_found", "unknown account"));
    }
    update_accounts(&state, |settings| settings.active_account = Some(id)).await
}

#[tauri::command]
pub async fn discord_session(state: State<'_, AppState>) -> AppResult<Option<SessionInfo>> {
    Ok(discord_auth::current(&state.paths).await.map(|s| s.info()))
}

#[tauri::command]
pub async fn discord_login(state: State<'_, AppState>) -> AppResult<LoginOutcome> {
    let (id, cancel) = state.new_discord_login();
    let outcome = discord_auth::login(&state.paths, &cancel).await?;
    Ok(match outcome {
        LoginOutcome::Cancelled if !state.is_current_discord_login(id) => LoginOutcome::Replaced,
        other => other,
    })
}

#[tauri::command]
pub fn discord_cancel_login(state: State<'_, AppState>) {
    state.cancel_discord_login();
}

#[tauri::command]
pub async fn discord_logout(state: State<'_, AppState>) -> AppResult<()> {
    state.cancel_discord_login();
    discord_auth::logout(&state.paths).await
}

#[tauri::command]
pub async fn play(app: AppHandle, event_id: String) -> AppResult<()> {
    let result = crate::launch::play(app, event_id).await;
    if let Err(err) = &result {
        log::error!("play failed: {err}");
    }
    result
}

#[tauri::command]
pub async fn cancel_install(state: State<'_, AppState>) -> AppResult<()> {
    crate::launch::cancel(&state).await
}

#[tauri::command]
pub async fn kill_game(state: State<'_, AppState>) -> AppResult<()> {
    crate::launch::kill(&state).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    running_event: Option<String>,
}

#[tauri::command]
pub async fn game_status(app: AppHandle, state: State<'_, AppState>) -> AppResult<GameStatus> {
    crate::launch::restore(&app).await;
    Ok(GameStatus {
        running_event: state.running().map(|r| r.event_id),
    })
}

async fn game_dir(state: &AppState, event_id: &str) -> AppResult<std::path::PathBuf> {
    let saved = crate::pack_sync::EventState::load(&state.paths, event_id).await;
    let id = saved
        .instance_id
        .ok_or_else(|| AppError::new("not_installed", "this event is not installed yet"))?;
    state.theseus().await?;
    Ok(theseus::instance::get_full_path(&id).await?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderKind {
    Game,
    Logs,
    Screenshots,
    Quarantine,
    LauncherLogs,
}

#[tauri::command]
pub async fn open_folder(
    state: State<'_, AppState>,
    kind: FolderKind,
    event_id: Option<String>,
) -> AppResult<()> {
    let event_id = || {
        event_id
            .clone()
            .filter(|id| events::is_valid_event_id(id))
            .ok_or_else(|| AppError::new("invalid_input", "event id required"))
    };
    let path = match kind {
        FolderKind::Game => game_dir(&state, &event_id()?).await?,
        FolderKind::Logs => game_dir(&state, &event_id()?).await?.join("logs"),
        FolderKind::Screenshots => game_dir(&state, &event_id()?).await?.join("screenshots"),
        FolderKind::Quarantine => state.paths.quarantine_dir(&event_id()?),
        FolderKind::LauncherLogs => state.paths.logs_dir(),
    };
    tokio::fs::create_dir_all(&path).await?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| AppError::io(e.to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    Discord,
    Twitter,
}

#[tauri::command]
pub fn open_link(link: LinkKind) -> AppResult<()> {
    let url = match link {
        LinkKind::Discord => crate::config::DISCORD_INVITE_URL,
        LinkKind::Twitter => crate::config::TWITTER_URL,
    };
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| AppError::io(e.to_string()))
}
