//! Estos mods solo los da la API y no van en el mrpack

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::events::{Event, PrivateMod, is_valid_event_id};
use crate::net::{self, Expected, Size};
use crate::pack_sync::{EventState, safe_version};
use crate::paths::Paths;
use crate::progress::{Reporter, Step};

pub const MAX_PRIVATE_MOD_BYTES: u64 = 25 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("invalid_event", message)
}

fn is_valid_file_name(name: &str) -> bool {
    const RESERVED: [&str; 4] = ["con", "prn", "aux", "nul"];
    let lower = name.to_ascii_lowercase();
    let stem = lower.split('.').next().unwrap_or("");
    let reserved = RESERVED.contains(&stem)
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit());
    name.len() <= 128
        && lower.ends_with(".jar")
        && name.len() > ".jar".len()
        && !name.starts_with('.')
        && !reserved
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'.' | b'-' | b'_' | b'+' | b'(' | b')' | b'[' | b']' | b' '
                )
        })
}

fn same_name_key(path: &str) -> String {
    path.to_ascii_lowercase()
}

fn validate(event: &Event, m: &PrivateMod) -> AppResult<()> {
    if !is_valid_event_id(&m.id) {
        return Err(invalid(format!("Mod privado con id inválido {:?}", m.id)));
    }
    if !is_valid_file_name(&m.file_name) {
        return Err(invalid(format!(
            "El mod privado {} tiene nombre inválido {:?}",
            m.id, m.file_name
        )));
    }
    if m.sha512.len() != 128 || !m.sha512.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(format!(
            "El mod privado {} no tiene un sha512 válido",
            m.id
        )));
    }
    if m.size == 0 || m.size > MAX_PRIVATE_MOD_BYTES {
        return Err(invalid(format!(
            "El mod privado {} tiene un tamaño de {}",
            m.id, m.size
        )));
    }
    safe_version(&event.pack_version)?;
    Ok(())
}

fn mod_url(base: &str, event: &Event, m: &PrivateMod) -> String {
    format!(
        "{}/v1/events/{}/private-mods/{}/{}",
        base.trim_end_matches('/'),
        event.id,
        event.pack_version,
        m.id
    )
}

async fn is_current(path: &Path, m: &PrivateMod) -> bool {
    match tokio::fs::symlink_metadata(path).await {
        Ok(meta) if meta.is_file() && meta.len() == m.size => net::sha512_file(path)
            .await
            .is_ok_and(|h| h.eq_ignore_ascii_case(&m.sha512)),
        _ => false,
    }
}

async fn clear_target(paths: &Paths, event: &Event, target: &Path) -> AppResult<()> {
    let Ok(meta) = tokio::fs::symlink_metadata(target).await else {
        return Ok(());
    };
    if meta.is_symlink() {
        log::warn!("Eliminando link {}", target.display());
        if tokio::fs::remove_file(target).await.is_err() {
            tokio::fs::remove_dir(target).await?;
        }
    } else if meta.is_dir() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let name = target.file_name().unwrap_or_default();
        let dest = paths
            .quarantine_dir(&event.id)
            .join(format!("{stamp}-private"))
            .join(name);
        log::warn!(
            "Moviendo carpeta de {} a {}",
            target.display(),
            dest.display()
        );
        tokio::fs::create_dir_all(dest.parent().unwrap_or(Path::new("."))).await?;
        tokio::fs::rename(target, &dest).await?;
    }
    Ok(())
}

/// Deja en mods/ los mods privados del evento y quita
///  los de versiones anteriores
pub async fn sync(
    paths: &Paths,
    event: &Event,
    game_dir: &Path,
    pack_mods: &HashSet<String>,
    bearer: Option<&str>,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<Vec<String>> {
    let base = crate::config::api_base_url();
    sync_from(
        paths, &base, event, game_dir, pack_mods, bearer, reporter, cancel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn sync_from(
    paths: &Paths,
    base: &str,
    event: &Event,
    game_dir: &Path,
    pack_mods: &HashSet<String>,
    bearer: Option<&str>,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<Vec<String>> {
    let pack_keys: HashSet<String> = pack_mods.iter().map(|p| same_name_key(p)).collect();
    let mut wanted = BTreeMap::new();
    let mut seen = HashSet::new();
    for m in &event.private_mods {
        validate(event, m)?;
        let key = format!("mods/{}", m.file_name);
        if pack_keys.contains(&same_name_key(&key)) {
            return Err(invalid(format!(
                "{key} esta en el mrpack y como mod privado"
            )));
        }
        if !seen.insert(same_name_key(&key)) {
            return Err(invalid(format!("{key} esta 2 veces en la lista")));
        }
        wanted.insert(key, m);
    }

    let mut missing = Vec::new();
    for m in wanted.values() {
        let target = game_dir.join("mods").join(&m.file_name);
        if !is_current(&target, m).await {
            missing.push((*m, target));
        }
    }
    if !missing.is_empty() && base.is_empty() {
        return Err(AppError::network("Esta build no tiene API URL"));
    }

    let total: u64 = missing.iter().map(|(m, _)| m.size).sum();
    let mut done = 0;
    for (m, target) in missing {
        log::info!("Descargando el mod {}", m.id);
        clear_target(paths, event, &target).await?;
        net::download_from_api(
            &mod_url(base, event, m),
            bearer,
            &target,
            Expected::Sha512(&m.sha512),
            Size::Exact(m.size),
            cancel,
            |n| {
                done += n;
                reporter.fraction(Step::Mods, done, total);
            },
        )
        .await?;
    }

    let mut state = EventState::load(paths, &event.id).await;
    let current: HashSet<String> = wanted.keys().map(|k| same_name_key(k)).collect();
    for old in state.private_files.keys() {
        let key = same_name_key(old);
        if current.contains(&key) || pack_keys.contains(&key) {
            continue;
        }
        if let Some(target) = crate::paths::safe_join(game_dir, old)
            && tokio::fs::symlink_metadata(&target)
                .await
                .is_ok_and(|m| m.is_file())
        {
            log::info!("Eliminando {old} (ya no es un mod privado del modpack)");
            tokio::fs::remove_file(&target).await?;
        }
    }
    state.private_files = wanted
        .iter()
        .map(|(k, m)| (k.clone(), m.sha512.to_lowercase()))
        .collect();
    state.save(paths, &event.id).await?;
    Ok(wanted.into_keys().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAR: &[u8] = b"private jar bytes";

    fn event(version: &str, mods: Vec<PrivateMod>) -> Event {
        let mut e: Event = serde_json::from_value(serde_json::json!({
            "id": "hc", "status": "testing", "packUrl": "https://x/p.mrpack",
            "packSha512": "ab", "packVersion": version,
        }))
        .unwrap();
        e.private_mods = mods;
        e
    }

    fn private_mod(file_name: &str, bytes: &[u8]) -> PrivateMod {
        PrivateMod {
            id: "hc2".into(),
            file_name: file_name.into(),
            size: bytes.len() as u64,
            sha512: net::sha512_hex(bytes),
        }
    }

    struct Setup {
        dir: tempfile::TempDir,
        paths: Paths,
        base: String,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            Self {
                paths: Paths::new(dir.path().join("root")),
                base: format!("file://{}", dir.path().join("api").display()),
                dir,
            }
        }

        fn serve(&self, version: &str, bytes: &[u8]) -> std::path::PathBuf {
            let served = self
                .dir
                .path()
                .join(format!("api/v1/events/hc/private-mods/{version}/hc2"));
            std::fs::create_dir_all(served.parent().unwrap()).unwrap();
            std::fs::write(&served, bytes).unwrap();
            served
        }

        fn game_dir(&self) -> std::path::PathBuf {
            self.dir.path().join("game")
        }

        fn mods(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(self.game_dir().join("mods"))
                .map(|d| {
                    d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names
        }

        async fn run(&self, event: &Event, pack_mods: &[&str]) -> AppResult<Vec<String>> {
            let pack_mods = pack_mods.iter().map(|s| s.to_string()).collect();
            sync_from(
                &self.paths,
                &self.base,
                event,
                &self.game_dir(),
                &pack_mods,
                None,
                &Reporter::silent(),
                &CancellationToken::new(),
            )
            .await
        }
    }

    #[tokio::test]
    async fn downloads_into_mods_and_reuses() {
        let s = Setup::new();
        let served = s.serve("1.0.0", JAR);
        let event = event("1.0.0", vec![private_mod("hc2-1.0.0.jar", JAR)]);
        let keys = s.run(&event, &[]).await.unwrap();
        assert_eq!(keys, ["mods/hc2-1.0.0.jar"]);
        assert_eq!(
            std::fs::read(s.game_dir().join("mods/hc2-1.0.0.jar")).unwrap(),
            JAR
        );
        let state = EventState::load(&s.paths, "hc").await;
        assert!(state.private_files.contains_key("mods/hc2-1.0.0.jar"));

        std::fs::remove_file(served).unwrap();
        s.run(&event, &[]).await.unwrap();
        assert_eq!(
            s.mods(),
            ["hc2-1.0.0.jar"],
            "Un jar válido no se descarga de nuevo"
        );
    }

    #[tokio::test]
    async fn damaged_jar_is_downloaded_again() {
        let s = Setup::new();
        s.serve("1.0.0", JAR);
        let event = event("1.0.0", vec![private_mod("hc2.jar", JAR)]);
        s.run(&event, &[]).await.unwrap();
        let path = s.game_dir().join("mods/hc2.jar");
        std::fs::write(&path, b"private jar bytez").unwrap();
        s.run(&event, &[]).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), JAR);
    }

    #[tokio::test]
    async fn folder_or_link_in_the_way_does_not_block_every_launch() {
        let s = Setup::new();
        s.serve("1.0.0", JAR);
        let event = event("1.0.0", vec![private_mod("hc2.jar", JAR)]);
        let target = s.game_dir().join("mods/hc2.jar");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        s.run(&event, &[]).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), JAR);
        assert!(
            s.paths.quarantine_dir("hc").exists(),
            "la carpeta se mantiene"
        );

        #[cfg(unix)]
        {
            let elsewhere = s.dir.path().join("elsewhere.jar");
            std::fs::write(&elsewhere, b"not ours").unwrap();
            std::fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &target).unwrap();
            s.run(&event, &[]).await.unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), JAR);
            assert!(!std::fs::symlink_metadata(&target).unwrap().is_symlink());
            assert_eq!(std::fs::read(&elsewhere).unwrap(), b"not ours");
        }
    }

    #[tokio::test]
    async fn new_pack_version_replaces_the_old_jar() {
        let s = Setup::new();
        s.serve("1.0.0", JAR);
        s.serve("1.1.0", b"newer jar");
        s.run(
            &event("1.0.0", vec![private_mod("hc2-1.0.0.jar", JAR)]),
            &[],
        )
        .await
        .unwrap();
        s.run(
            &event("1.1.0", vec![private_mod("hc2-1.1.0.jar", b"newer jar")]),
            &[],
        )
        .await
        .unwrap();
        assert_eq!(s.mods(), ["hc2-1.1.0.jar"]);

        s.run(&event("1.2.0", vec![]), &["mods/hc2-1.1.0.jar"])
            .await
            .unwrap();
        assert_eq!(
            s.mods(),
            ["hc2-1.1.0.jar"],
            "a jar the pack now ships itself is not removed"
        );
    }

    #[tokio::test]
    async fn hash_or_size_mismatch_keeps_nothing() {
        let s = Setup::new();
        s.serve("1.0.0", b"tampered jar bytes");
        let err = s
            .run(&event("1.0.0", vec![private_mod("hc2.jar", JAR)]), &[])
            .await
            .unwrap_err();
        assert_eq!(err.code, "hash_mismatch");
        assert!(s.mods().is_empty());

        let mut wrong_size = private_mod("hc2.jar", b"tampered jar bytes");
        wrong_size.size += 1;
        let err = s
            .run(&event("1.0.0", vec![wrong_size]), &[])
            .await
            .unwrap_err();
        assert_eq!(err.code, "hash_mismatch");
        assert!(s.mods().is_empty());
    }

    #[tokio::test]
    async fn rejects_bad_entries_before_downloading() {
        let s = Setup::new();
        s.serve("1.0.0", JAR);
        let ok = || private_mod("hc2.jar", JAR);
        let mut bad = Vec::new();
        for name in [
            "../hc2.jar",
            "sub/hc2.jar",
            "hc2.zip",
            ".jar",
            ".hidden.jar",
            "sub\\hc2.jar",
            "hc2?.jar",
            "hc2*.jar",
            "a<b>.jar",
            "a\"b.jar",
            "a|b.jar",
            "C:hc2.jar",
            "nul.jar",
            "COM1.jar",
            "héllo.jar",
        ] {
            bad.push(private_mod(name, JAR));
        }
        let mut m = ok();
        m.id = "../x".into();
        bad.push(m);
        let mut m = ok();
        m.sha512 = "zz".into();
        bad.push(m);
        let mut m = ok();
        m.size = MAX_PRIVATE_MOD_BYTES + 1;
        bad.push(m);
        for m in bad {
            let err = s.run(&event("1.0.0", vec![m]), &[]).await.unwrap_err();
            assert_eq!(err.code, "invalid_event");
        }
        let err = s
            .run(&event("1.0/../2", vec![ok()]), &[])
            .await
            .unwrap_err();
        assert_eq!(err.code, "invalid_event");
        let err = s
            .run(&event("1.0.0", vec![ok()]), &["mods/hc2.jar"])
            .await
            .unwrap_err();
        assert_eq!(err.code, "invalid_event", "same name as a pack mod");
        let err = s
            .run(&event("1.0.0", vec![ok()]), &["mods/HC2.jar"])
            .await
            .unwrap_err();
        assert_eq!(err.code, "invalid_event", "names differ only in case");
        let err = s
            .run(&event("1.0.0", vec![ok(), ok()]), &[])
            .await
            .unwrap_err();
        assert_eq!(err.code, "invalid_event", "listed twice");
        assert!(s.mods().is_empty());
    }
}
