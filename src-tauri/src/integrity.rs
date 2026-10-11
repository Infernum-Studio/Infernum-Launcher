//! Mantiene en mods/ unicamente los que incluye el modpack, se ejecuta
//! siempre antes de abrir Minecraft
//!
//! Cualquier otra cosa en mods/ que no sea un mod del modpack se
//! moverá a /quarantine/<eventId>/<timestamp>/

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuarantinedFile {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineReport {
    pub folder: PathBuf,
    pub files: Vec<QuarantinedFile>,
}

pub fn fold_case(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

pub async fn enforce(
    game_dir: &Path,
    allowed: &HashSet<String>,
    quarantine_root: &Path,
) -> AppResult<Option<QuarantineReport>> {
    let mods = game_dir.join("mods");
    let game_dir = game_dir.to_path_buf();
    let allowed = allowed.clone();
    let quarantine_root = quarantine_root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        enforce_blocking(&game_dir, &mods, &allowed, &quarantine_root)
    })
    .await?
}

fn enforce_blocking(
    game_dir: &Path,
    mods: &Path,
    allowed: &HashSet<String>,
    quarantine_root: &Path,
) -> AppResult<Option<QuarantineReport>> {
    if !mods.is_dir() {
        std::fs::create_dir_all(mods)?;
        return Ok(None);
    }
    let allowed: HashSet<String> = allowed.iter().map(|p| fold_case(p)).collect();
    let allowed_dirs: HashSet<String> = allowed
        .iter()
        .flat_map(|p| {
            let parts: Vec<&str> = p.split('/').collect();
            (1..parts.len()).map(move |i| parts[..i].join("/"))
        })
        .collect();

    let mut extras = Vec::new();
    collect_extras(game_dir, mods, &allowed, &allowed_dirs, &mut extras)?;
    if extras.is_empty() {
        return Ok(None);
    }

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut folder = quarantine_root.join(&stamp);
    let mut n = 1;
    while folder.exists() {
        n += 1;
        folder = quarantine_root.join(format!("{stamp}-{n}"));
    }
    let mut files = Vec::new();
    for path in extras {
        let rel = path.strip_prefix(mods).unwrap_or(&path).to_path_buf();
        let size = dir_size(&path);
        let dest = folder.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        move_path(&path, &dest)?;
        log::warn!("quarantined mods/{}", rel.display());
        files.push(QuarantinedFile {
            name: rel.to_string_lossy().replace('\\', "/"),
            size,
        });
    }
    Ok(Some(QuarantineReport { folder, files }))
}

fn rel_key(game_dir: &Path, path: &Path) -> String {
    fold_case(
        &path
            .strip_prefix(game_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

fn collect_extras(
    game_dir: &Path,
    dir: &Path,
    allowed: &HashSet<String>,
    allowed_dirs: &HashSet<String>,
    extras: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let key = rel_key(game_dir, &path);
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() && allowed_dirs.contains(&key) {
            collect_extras(game_dir, &path, allowed, allowed_dirs, extras)?;
        } else if !(meta.is_file() && allowed.contains(&key)) {
            extras.push(path);
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::read_dir(path)
            .map(|d| d.flatten().map(|e| dir_size(&e.path())).sum())
            .unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(from)?;
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            move_path(&entry.path(), &to.join(entry.file_name()))?;
        }
        std::fs::remove_dir(from)
    } else {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[tokio::test]
    async fn keeps_listed_files_and_quarantines_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        let quarantine = dir.path().join("quarantine");
        touch(&game.join("mods/listed.jar"), b"ok");
        touch(&game.join("mods/sub/listed2.jar"), b"ok");
        touch(&game.join("mods/extra.jar"), b"xray");
        touch(&game.join("mods/listed.jar.disabled"), b"x");
        touch(&game.join("mods/sub/extra2.jar"), b"x");
        touch(&game.join("mods/folder/inner.jar"), b"x");
        touch(&game.join("config/keep.toml"), b"cfg");

        let allowed: HashSet<String> = ["mods/listed.jar", "mods/sub/listed2.jar"]
            .into_iter()
            .map(String::from)
            .collect();
        let report = enforce(&game, &allowed, &quarantine)
            .await
            .unwrap()
            .unwrap();

        let mut names: Vec<_> = report.files.iter().map(|f| f.name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "extra.jar",
                "folder",
                "listed.jar.disabled",
                "sub/extra2.jar"
            ]
        );
        assert!(game.join("mods/listed.jar").exists());
        assert!(game.join("mods/sub/listed2.jar").exists());
        assert!(!game.join("mods/extra.jar").exists());
        assert!(game.join("config/keep.toml").exists());
        assert_eq!(
            std::fs::read(report.folder.join("extra.jar")).unwrap(),
            b"xray"
        );
        assert!(report.folder.join("folder/inner.jar").exists());

        assert!(
            enforce(&game, &allowed, &quarantine)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_are_quarantined_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        let elsewhere = dir.path().join("elsewhere");
        touch(&elsewhere.join("cheat.jar"), b"x");
        touch(&game.join("mods/listed.jar"), b"ok");
        std::os::unix::fs::symlink(&elsewhere, game.join("mods/linked")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("cheat.jar"), game.join("mods/listed2.jar"))
            .unwrap();

        let allowed: HashSet<String> = [
            "mods/listed.jar",
            "mods/listed2.jar",
            "mods/linked/cheat.jar",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let report = enforce(&game, &allowed, &dir.path().join("q"))
            .await
            .unwrap()
            .unwrap();
        let mut names: Vec<_> = report.files.iter().map(|f| f.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["linked", "listed2.jar"]);
        assert!(
            elsewhere.join("cheat.jar").exists(),
            "link targets are untouched"
        );
    }

    #[tokio::test]
    async fn same_second_runs_get_separate_folders() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        let quarantine = dir.path().join("q");
        let allowed = HashSet::new();
        touch(&game.join("mods/a.jar"), b"1");
        let first = enforce(&game, &allowed, &quarantine)
            .await
            .unwrap()
            .unwrap();
        touch(&game.join("mods/a.jar"), b"2");
        let second = enforce(&game, &allowed, &quarantine)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first.folder, second.folder);
        assert_eq!(std::fs::read(first.folder.join("a.jar")).unwrap(), b"1");
        assert_eq!(std::fs::read(second.folder.join("a.jar")).unwrap(), b"2");
    }
}
