use tokio::sync::{Mutex, OnceCell, RwLock};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::paths::Paths;
use crate::settings::Settings;

#[derive(Debug, Clone, Copy)]
pub enum GameHandle {
    Spawned {
        uuid: uuid::Uuid,
        java: Option<crate::game_process::GameProcess>,
        launched_at: u64,
    },
    Restored(crate::game_process::GameProcess),
}

#[derive(Debug, Clone)]
pub struct RunningGame {
    pub event_id: String,
    pub instance_id: String,
    pub handle: GameHandle,
    pub kill_requested: bool,
}

pub struct AppState {
    pub paths: Paths,
    pub total_ram_mb: u64,
    pub settings: RwLock<Settings>,
    pub launch_lock: Mutex<()>,
    pub running: std::sync::Mutex<Option<RunningGame>>,
    cancel: std::sync::Mutex<CancellationToken>,
    discord_login: std::sync::Mutex<(u64, CancellationToken)>,
    theseus: OnceCell<()>,
    pub restored: OnceCell<()>,
}

impl AppState {
    pub async fn new(paths: Paths) -> Self {
        let total_ram_mb = total_ram_mb();
        let settings = Settings::load(&paths.settings_file(), total_ram_mb).await;
        Self {
            paths,
            total_ram_mb,
            settings: RwLock::new(settings),
            launch_lock: Mutex::new(()),
            running: std::sync::Mutex::new(None),
            cancel: std::sync::Mutex::new(CancellationToken::new()),
            discord_login: std::sync::Mutex::new((0, CancellationToken::new())),
            theseus: OnceCell::new(),
            restored: OnceCell::new(),
        }
    }

    pub async fn theseus(&self) -> AppResult<()> {
        self.theseus.get_or_try_init(init_theseus).await?;
        Ok(())
    }

    pub fn new_launch_token(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.cancel.lock().expect("cancel lock") = token.clone();
        token
    }

    pub fn cancel_launch(&self) {
        self.cancel.lock().expect("cancel lock").cancel();
    }

    pub fn new_discord_login(&self) -> (u64, CancellationToken) {
        let mut current = self.discord_login.lock().expect("discord lock");
        current.1.cancel();
        *current = (current.0 + 1, CancellationToken::new());
        current.clone()
    }

    pub fn is_current_discord_login(&self, id: u64) -> bool {
        self.discord_login.lock().expect("discord lock").0 == id
    }

    pub fn cancel_discord_login(&self) {
        self.discord_login.lock().expect("discord lock").1.cancel();
    }

    pub fn running(&self) -> Option<RunningGame> {
        self.running.lock().expect("running lock").clone()
    }

    pub fn set_running(&self, value: Option<RunningGame>) {
        *self.running.lock().expect("running lock") = value;
    }
}

async fn init_theseus() -> Result<(), AppError> {
    theseus::State::init(crate::config::APP_IDENTIFIER.to_string()).await?;
    let mut s = theseus::settings::get().await?;
    s.telemetry = false;
    s.discord_rpc = false;
    s.personalized_ads = false;
    s.hide_on_process_start = false;
    theseus::settings::set(s).await?;
    log::info!("launcher core ready");
    Ok(())
}

pub fn total_ram_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory() / (1024 * 1024)
}
