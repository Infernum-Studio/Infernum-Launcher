#![recursion_limit = "512"]

mod access;
mod accounts;
mod api;
mod commands;
mod config;
mod discord_auth;
mod error;
mod events;
mod game_process;
mod integrity;
mod java;
mod launch;
mod net;
mod pack_sync;
mod paths;
mod private_mods;
mod progress;
mod settings;
mod state;

use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

pub fn data_root() -> std::path::PathBuf {
    if cfg!(debug_assertions)
        && let Some(dir) = std::env::var_os("INFERNUM_DATA_DIR")
    {
        return dir.into();
    }
    dirs::data_dir()
        .expect("no user data directory")
        .join(config::APP_IDENTIFIER)
}

pub fn run() {
    let root = data_root();
    if let Err(err) = paths::make_private_dir(&root) {
        eprintln!("could not restrict the data folder: {err}");
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
        .expect("tokio runtime");
    tauri::async_runtime::set(runtime.handle().clone());

    let paths = paths::Paths::new(root.clone());
    let app_state = runtime.block_on(state::AppState::new(paths));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::Folder {
                        path: root.join("logs"),
                        file_name: Some("launcher".into()),
                    }),
                ])
                .level(log::LevelFilter::Info)
                .level_for("sqlx", log::LevelFilter::Warn)
                // Solo registra los avisos y NUNCA tokens o cosas
                // relacionadas al OAuth
                .level_for("theseus", log::LevelFilter::Warn)
                .level_for("tracing", log::LevelFilter::Off)
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(RotationStrategy::KeepSome(5))
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .setup(|app| {
            log::info!(
                "Infernum Launcher {} on {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            );
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = handle.state::<state::AppState>().theseus().await {
                    log::error!("theseus init failed: {err}");
                }
                if cfg!(debug_assertions)
                    && let Ok(event_id) = std::env::var("INFERNUM_AUTOPLAY")
                {
                    let paths = handle.state::<state::AppState>().paths.clone();
                    if let Err(err) = events::list(&paths, true).await {
                        log::error!("autoplay: events refresh failed: {err}");
                    }
                    match launch::play(handle.clone(), event_id).await {
                        Ok(()) => log::info!("autoplay: game started"),
                        Err(err) => log::error!("autoplay: play failed: {err}"),
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_get,
            commands::settings_set,
            commands::system_info,
            commands::events_list,
            commands::accounts_list,
            commands::accounts_add_offline,
            commands::accounts_remove,
            commands::accounts_login_microsoft,
            commands::account_skin,
            commands::accounts_set_active,
            commands::discord_session,
            commands::discord_login,
            commands::discord_cancel_login,
            commands::discord_logout,
            commands::play,
            commands::cancel_install,
            commands::kill_game,
            commands::game_status,
            commands::open_folder,
            commands::open_link,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the launcher");

    drop(runtime);
}
