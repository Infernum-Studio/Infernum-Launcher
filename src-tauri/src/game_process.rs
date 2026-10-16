use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProcess {
    pub pid: u32,
    pub start_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedGame {
    pub event_id: String,
    pub instance_id: String,
    pub game_dir: PathBuf,
    pub process: GameProcess,
}

pub fn find_launched_java(since: u64) -> Option<GameProcess> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let me = Pid::from_u32(std::process::id());
    sys.processes()
        .iter()
        .filter(|(_, p)| {
            p.parent() == Some(me)
                && p.name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("java")
                && p.start_time() + 2 >= since
        })
        .max_by_key(|(_, p)| p.start_time())
        .map(|(pid, p)| GameProcess {
            pid: pid.as_u32(),
            start_time: p.start_time(),
        })
}

impl GameProcess {
    fn with_process<T>(&self, f: impl FnOnce(&Process) -> T) -> Option<T> {
        let pid = Pid::from_u32(self.pid);
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.process(pid)
            .filter(|p| p.start_time().abs_diff(self.start_time) <= 2)
            .map(f)
    }

    pub fn is_alive(&self) -> bool {
        self.with_process(|_| ()).is_some()
    }

    pub fn kill(&self) -> bool {
        self.with_process(Process::kill).unwrap_or(false)
    }
}

fn file(paths: &Paths) -> PathBuf {
    paths.root.join("running.json")
}

pub async fn save(paths: &Paths, game: &SavedGame) {
    let result = match serde_json::to_vec(game) {
        Ok(bytes) => crate::paths::write_atomic(&file(paths), &bytes).await,
        Err(err) => Err(err.into()),
    };
    if let Err(err) = result {
        log::warn!("No se pudo guardar el proceso del juego: {err}");
    }
}

pub async fn load(paths: &Paths) -> Option<SavedGame> {
    let bytes = tokio::fs::read(file(paths)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub async fn clear(paths: &Paths) {
    let _ = tokio::fs::remove_file(file(paths)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> GameProcess {
        let pid = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        GameProcess {
            pid: pid.as_u32(),
            start_time: sys.process(pid).unwrap().start_time(),
        }
    }

    #[test]
    fn alive_only_with_the_same_start_time() {
        let me = me();
        assert!(me.is_alive());
        let reused = GameProcess {
            start_time: me.start_time - 60,
            ..me
        };
        assert!(!reused.is_alive(), "un PID reusado no es el juegos");
        assert!(!reused.kill());
    }

    #[cfg(unix)]
    #[test]
    fn finds_the_java_child() {
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java");
        std::fs::copy("/bin/sleep", &java).unwrap();
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut child = std::process::Command::new(&java).arg("30").spawn().unwrap();
        let found = find_launched_java(since);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found.map(|p| p.pid), Some(child.id()));
        assert!(
            find_launched_java(since + 60).is_none(),
            "procesos antiguos son ignorados"
        );
    }

    #[tokio::test]
    async fn save_load_clear() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert!(load(&paths).await.is_none());
        let game = SavedGame {
            event_id: "hellcraft2".into(),
            instance_id: "local:x".into(),
            game_dir: dir.path().join("game"),
            process: me(),
        };
        save(&paths, &game).await;
        let loaded = load(&paths).await.unwrap();
        assert_eq!(loaded.process, game.process);
        assert_eq!(loaded.event_id, "hellcraft2");
        clear(&paths).await;
        assert!(load(&paths).await.is_none());
    }
}
