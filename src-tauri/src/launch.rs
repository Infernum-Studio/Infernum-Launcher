use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use theseus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::accounts::{AccountKind, microsoft};
use crate::error::{AppError, AppResult};
use crate::events::{self, EventStatus};
use crate::game_process::{self, SavedGame};
use crate::progress::{Reporter, Step};
use crate::state::{AppState, GameHandle, RunningGame};
use crate::{access, integrity, java, pack_sync, private_mods, settings};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StartedPayload<'a> {
    event_id: &'a str,
    process_id: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ExitedPayload<'a> {
    event_id: &'a str,
    code: Option<i32>,
    crashed: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CrashedPayload<'a> {
    event_id: &'a str,
    log_path: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct QuarantinedPayload<'a> {
    event_id: &'a str,
    folder: String,
    files: &'a [integrity::QuarantinedFile],
}

const MAX_MONITOR_ERRORS: u32 = 120;

fn check_cancel(cancel: &CancellationToken) -> AppResult<()> {
    if cancel.is_cancelled() {
        Err(AppError::cancelled())
    } else {
        Ok(())
    }
}

pub async fn play(app: AppHandle, event_id: String) -> AppResult<()> {
    let state = app.state::<AppState>();
    let _guard = state
        .launch_lock
        .try_lock()
        .map_err(|_| AppError::new("already_running", "a launch is already in progress"))?;
    restore(&app).await;
    if state.running().is_some() {
        return Err(AppError::new(
            "already_running",
            "the game is already running",
        ));
    }
    let cancel = state.new_launch_token();
    let reporter = Reporter::new(app.clone(), &event_id);
    reporter.step(Step::Checking);

    let event = events::get(&state.paths, &event_id).await?;
    match event.status {
        EventStatus::Ended => return Err(AppError::new("event_ended", "this event has ended")),
        EventStatus::Hidden => return Err(AppError::new("event_not_found", "unknown event")),
        EventStatus::Testing | EventStatus::Live => {}
    }
    let settings = state.settings.read().await.clone();
    let account = settings
        .active_account
        .as_ref()
        .and_then(|id| settings.accounts.iter().find(|a| &a.id == id))
        .cloned()
        .ok_or_else(|| AppError::new("no_account", "no active account"))?;
    let session = access::check(&state, &event).await?;
    check_cancel(&cancel)?;

    state.theseus().await?;
    let pack = pack_sync::load_pack(&state.paths, &event, &reporter, &cancel).await?;
    check_cancel(&cancel)?;
    let versions = pack_sync::resolve_versions(&event, &pack.versions()?)?;
    let major = java::required_major(&state.paths, &versions.minecraft).await?;
    let java_exe = java::ensure(&state.paths, major, &reporter, &cancel).await?;
    check_cancel(&cancel)?;
    let mut outcome = pack_sync::sync(&state.paths, &event, &pack, &reporter, &cancel).await?;
    check_cancel(&cancel)?;
    let private = private_mods::sync(
        &state.paths,
        &event,
        &outcome.game_dir,
        &outcome.allowed_mods,
        session.as_ref().map(|s| s.token.as_str()),
        &reporter,
        &cancel,
    )
    .await?;
    outcome.allowed_mods.extend(private);
    check_cancel(&cancel)?;

    reporter.step(Step::Integrity);
    if let Some(report) = integrity::enforce(
        &outcome.game_dir,
        &outcome.allowed_mods,
        &state.paths.quarantine_dir(&event.id),
    )
    .await?
    {
        let _ = app.emit(
            "integrity://quarantined",
            QuarantinedPayload {
                event_id: &event.id,
                folder: report.folder.to_string_lossy().into_owned(),
                files: &report.files,
            },
        );
    }

    reporter.step(Step::Launching);
    apply_launch_overrides(&outcome.instance_id, &settings, &java_exe).await?;
    let quick_play = theseus::instance::QuickPlayType::None;
    let launched_at = chrono::Utc::now();
    let process = match account.kind {
        AccountKind::Offline => {
            theseus::instance::run_with_credentials(
                &outcome.instance_id,
                &account.offline_credentials(),
                quick_play,
            )
            .await?
        }
        AccountKind::Microsoft => {
            microsoft::prepare_launch(&account.id).await?;
            theseus::instance::run(&outcome.instance_id, quick_play).await?
        }
    };
    match game_process::find_launched_java(launched_at.timestamp().max(0) as u64) {
        Some(java) => {
            let saved = SavedGame {
                event_id: event.id.clone(),
                instance_id: outcome.instance_id.clone(),
                game_dir: outcome.game_dir.clone(),
                process: java,
            };
            game_process::save(&state.paths, &saved).await;
        }
        None => {
            log::warn!("no se puedo encontrr el proceso del juego, el launcher no lo encontrará")
        }
    }
    state.set_running(Some(RunningGame {
        event_id: event.id.clone(),
        instance_id: outcome.instance_id.clone(),
        handle: GameHandle::Spawned(process.uuid),
        kill_requested: false,
    }));
    let _ = app.emit(
        "launch://started",
        StartedPayload {
            event_id: &event.id,
            process_id: process.uuid.to_string(),
        },
    );
    if settings.close_on_launch
        && let Some(window) = app.get_webview_window("main")
    {
        let _ = window.hide();
    }

    tauri::async_runtime::spawn(monitor(app.clone(), outcome.game_dir, launched_at));
    Ok(())
}

pub async fn restore(app: &AppHandle) {
    let state = app.state::<AppState>();
    state
        .restored
        .get_or_init(|| async {
            let Some(saved) = game_process::load(&state.paths).await else {
                return;
            };
            if state.running().is_some() || !saved.process.is_alive() {
                game_process::clear(&state.paths).await;
                return;
            }
            log::info!("el juego de {} aun esta abierto", saved.event_id);
            let started = chrono::DateTime::from_timestamp(saved.process.start_time as i64, 0)
                .unwrap_or_else(chrono::Utc::now);
            state.set_running(Some(RunningGame {
                event_id: saved.event_id,
                instance_id: saved.instance_id,
                handle: GameHandle::Restored(saved.process),
                kill_requested: false,
            }));
            tauri::async_runtime::spawn(monitor(app.clone(), saved.game_dir, started));
        })
        .await;
}

async fn is_alive(running: &RunningGame) -> theseus::Result<bool> {
    match running.handle {
        GameHandle::Spawned(uuid) => Ok(theseus::process::get_by_instance_id(&running.instance_id)
            .await?
            .iter()
            .any(|p| p.uuid == uuid)),
        GameHandle::Restored(process) => Ok(process.is_alive()),
    }
}

async fn apply_launch_overrides(
    instance_id: &str,
    settings: &settings::Settings,
    java_exe: &Path,
) -> AppResult<()> {
    let mut args = vec![format!("-Xms{}M", settings.ram_mb / 2)];
    args.extend(settings::validate_jvm_args(&settings.jvm_args).unwrap_or_default());
    theseus::instance::edit(
        instance_id,
        EditInstance {
            launch_overrides: Some(InstanceLaunchOverridesPatch {
                java_path: Some(Some(java_exe.to_string_lossy().into_owned())),
                extra_launch_args: Some(Some(args)),
                memory: Some(Some(MemorySettings {
                    maximum: settings.ram_mb,
                })),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

async fn monitor(app: AppHandle, game_dir: PathBuf, started: chrono::DateTime<chrono::Utc>) {
    let state = app.state::<AppState>();
    let Some(running) = state.running() else {
        return;
    };
    let mut errors = 0;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match is_alive(&running).await {
            Ok(true) => errors = 0,
            Ok(false) => break,
            Err(err) => {
                errors += 1;
                if errors >= MAX_MONITOR_ERRORS {
                    log::error!("giving up on the process list: {err}");
                    break;
                }
                log::warn!("could not read the process list: {err}");
            }
        }
    }

    let mut status = None;
    for _ in 0..10 {
        status = read_exit_status(&game_dir.join("logs").join("launcher_log.txt")).await;
        if status.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let kill_requested = state.running().is_some_and(|r| r.kill_requested);
    state.set_running(None);
    game_process::clear(&state.paths).await;

    let code = status.as_ref().and_then(|s| s.code);
    let new_crash_report = has_crash_report_since(&game_dir, started).await;
    let crashed =
        !kill_requested && (new_crash_report || status.as_ref().is_some_and(|s| !s.success));
    match status.as_ref().and_then(|s| s.code) {
        Some(code) if crashed => log::warn!("game crashed (exit code {code})"),
        Some(code) => log::info!("game exited with code {code}"),
        None if crashed => log::warn!("game crashed (no exit code)"),
        None => log::info!("game exited"),
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
    let _ = app.emit(
        "launch://exited",
        ExitedPayload {
            event_id: &running.event_id,
            code,
            crashed,
        },
    );
    if crashed {
        let _ = app.emit(
            "launch://crashed",
            CrashedPayload {
                event_id: &running.event_id,
                log_path: game_dir
                    .join("logs")
                    .join("latest.log")
                    .to_string_lossy()
                    .into_owned(),
            },
        );
    }
}

#[derive(Debug, PartialEq)]
struct ExitStatus {
    code: Option<i32>,
    success: bool,
}

fn parse_exit_status(log_tail: &str) -> Option<ExitStatus> {
    let current = log_tail
        .rfind("# Minecraft launcher log started")
        .map_or(log_tail, |i| &log_tail[i..]);
    let line = current
        .lines()
        .rev()
        .find(|l| l.starts_with("# Process exited with status:"))?;
    let status = line
        .trim_start_matches("# Process exited with status:")
        .trim();
    let code = ["exit status:", "exit code:"]
        .iter()
        .find_map(|p| status.strip_prefix(p))
        .and_then(|n| n.trim().parse::<i32>().ok());
    Some(ExitStatus {
        code,
        success: code == Some(0),
    })
}

async fn read_exit_status(path: &Path) -> Option<ExitStatus> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    file.seek(std::io::SeekFrom::Start(len.saturating_sub(4096)))
        .await
        .ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await.ok()?;
    parse_exit_status(&String::from_utf8_lossy(&buf))
}

async fn has_crash_report_since(game_dir: &Path, since: chrono::DateTime<chrono::Utc>) -> bool {
    let Ok(mut entries) = tokio::fs::read_dir(game_dir.join("crash-reports")).await else {
        return false;
    };
    let since: std::time::SystemTime = since.into();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await
            && meta.modified().is_ok_and(|m| m >= since)
        {
            return true;
        }
    }
    false
}

pub async fn kill(state: &AppState) -> AppResult<()> {
    let Some(mut running) = state.running() else {
        return Ok(());
    };
    running.kill_requested = true;
    state.set_running(Some(running.clone()));
    match running.handle {
        GameHandle::Spawned(_) => theseus::instance::kill(&running.instance_id).await?,
        GameHandle::Restored(process) => {
            if !process.kill() {
                log::warn!("no se pudo detener el proceso {}", process.pid);
            }
        }
    }
    Ok(())
}

pub async fn cancel(state: &AppState) -> AppResult<()> {
    state.cancel_launch();
    if state.theseus().await.is_ok() {
        for job in theseus::install::list_jobs(false).await? {
            if job.can_cancel
                && let Ok(id) = job.job_id.parse()
            {
                let _ = theseus::install::cancel_job(id).await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exit_status_lines() {
        let log = "stuff\n# Process exited with status: exit status: 0\n";
        assert_eq!(
            parse_exit_status(log),
            Some(ExitStatus {
                code: Some(0),
                success: true
            })
        );
        let log = "a\n# Process exited with status: exit code: 1\n";
        assert_eq!(
            parse_exit_status(log),
            Some(ExitStatus {
                code: Some(1),
                success: false
            })
        );
        let log = "# Process exited with status: signal: 9 (SIGKILL)";
        assert_eq!(
            parse_exit_status(log),
            Some(ExitStatus {
                code: None,
                success: false
            })
        );
        assert_eq!(parse_exit_status("no status here"), None);
        let log =
            "# Process exited with status: exit status: 0\n# Minecraft launcher log started at x\n";
        assert_eq!(
            parse_exit_status(log),
            None,
            "a previous run's status is ignored"
        );
    }
}
