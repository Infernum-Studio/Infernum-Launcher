//! Instala y actualiza el .mrpack de un evento en su instancia
//!
//! Theseus solo instala Minecraft y el loader.
//! Los archivos se sincronizan para que la primera instalación
//! y las actualizaciones futuras del mrpack sean consistentes.
//! Los archivos se vuelven a descargar si han cambiado, archivos
//! anteriores que ya no existen se eliminan, y no se sobreescriben
//! las configs del usuario.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use theseus::install::InstallJobStatus;
use theseus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::events::{Event, Loader};
use crate::integrity::fold_case;
use crate::net::{self, Expected, Size};
use crate::paths::{self, Paths};
use crate::progress::{Reporter, Step};

const STATE_SCHEMA_VERSION: u32 = 1;
const MAX_PARALLEL_DOWNLOADS: usize = 6;
const MAX_PARALLEL_HASHES: usize = 4;
const MAX_PACK_BYTES: u64 = 1024 * 1024 * 1024;
const INSTALL_STALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventState {
    pub schema_version: u32,
    pub instance_id: Option<String>,
    pub pack_version: Option<String>,
    pub pack_sha512: Option<String>,
    pub minecraft_version: Option<String>,
    pub loader: Option<Loader>,
    pub loader_version: Option<String>,
    pub files: BTreeMap<String, String>,
}

impl EventState {
    pub async fn load(paths: &Paths, event_id: &str) -> Self {
        match tokio::fs::read(paths.state_file(event_id)).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                log::warn!("state for {event_id} is unreadable ({err}); starting fresh");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub async fn save(&mut self, paths: &Paths, event_id: &str) -> AppResult<()> {
        self.schema_version = STATE_SCHEMA_VERSION;
        paths::write_atomic(
            &paths.state_file(event_id),
            &serde_json::to_vec_pretty(self)?,
        )
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackIndex {
    pub format_version: u32,
    pub game: String,
    #[serde(default)]
    pub files: Vec<PackFile>,
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    pub path: String,
    pub hashes: HashMap<String, String>,
    pub env: Option<PackFileEnv>,
    pub downloads: Vec<String>,
    pub file_size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackFileEnv {
    pub client: String,
}

impl PackFile {
    fn is_for_client(&self) -> bool {
        !self.env.as_ref().is_some_and(|e| e.client == "unsupported")
    }
}

pub struct Mrpack {
    pub index: PackIndex,
    pub overrides: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameVersions {
    pub minecraft: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
}

const LOADER_KEYS: [(&str, Loader); 2] = [
    ("neoforge", Loader::NeoForge),
    ("fabric-loader", Loader::Fabric),
];

impl Mrpack {
    pub fn versions(&self) -> AppResult<GameVersions> {
        let deps = &self.index.dependencies;
        let minecraft = deps
            .get("minecraft")
            .cloned()
            .ok_or_else(|| AppError::new("invalid_pack", "pack has no minecraft dependency"))?;
        if let Some(other) = deps
            .keys()
            .find(|k| *k != "minecraft" && !LOADER_KEYS.iter().any(|(key, _)| key == k))
        {
            return Err(AppError::new(
                "invalid_pack",
                format!("unknown dependency {other}"),
            ));
        }
        let mut loaders = LOADER_KEYS
            .iter()
            .filter_map(|(key, loader)| deps.get(*key).map(|v| (*loader, v.clone())));
        let (loader, loader_version) = match (loaders.next(), loaders.next()) {
            (None, _) => (Loader::Vanilla, None),
            (Some((loader, version)), None) => (loader, Some(version)),
            (Some(_), Some(_)) => {
                return Err(AppError::new(
                    "invalid_pack",
                    "pack declares more than one loader",
                ));
            }
        };
        Ok(GameVersions {
            minecraft,
            loader,
            loader_version,
        })
    }
}

pub fn resolve_versions(event: &Event, pack: &GameVersions) -> AppResult<GameVersions> {
    let minecraft = event
        .minecraft
        .clone()
        .unwrap_or_else(|| pack.minecraft.clone());
    let loader = event.loader.unwrap_or(pack.loader);
    let loader_version = match (&event.loader_version, loader) {
        (_, Loader::Vanilla) => None,
        (Some(v), _) => Some(v.clone()),
        (None, l) if l == pack.loader => pack.loader_version.clone(),
        (None, _) => {
            return Err(AppError::new(
                "invalid_event",
                "loaderVersion is required when loader differs from the pack",
            ));
        }
    };
    Ok(GameVersions {
        minecraft,
        loader,
        loader_version,
    })
}

const MAX_OVERRIDES_BYTES: u64 = 512 * 1024 * 1024;

pub fn read_mrpack(path: &Path) -> AppResult<Mrpack> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let index: PackIndex = {
        let entry = zip
            .by_name("modrinth.index.json")
            .map_err(|_| AppError::new("invalid_pack", "missing modrinth.index.json"))?;
        serde_json::from_reader(entry)?
    };
    if index.format_version != 1 || index.game != "minecraft" {
        return Err(AppError::new("invalid_pack", "unsupported mrpack format"));
    }

    let mut overrides = BTreeMap::new();
    let mut client_overrides = BTreeMap::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let (target, rel) = if let Some(rel) = name.strip_prefix("overrides/") {
            (&mut overrides, rel.to_string())
        } else if let Some(rel) = name.strip_prefix("client-overrides/") {
            (&mut client_overrides, rel.to_string())
        } else {
            continue;
        };
        let mut bytes = Vec::new();
        let remaining = MAX_OVERRIDES_BYTES - total;
        std::io::Read::read_to_end(
            &mut std::io::Read::take(&mut entry, remaining + 1),
            &mut bytes,
        )?;
        total += bytes.len() as u64;
        if total > MAX_OVERRIDES_BYTES {
            return Err(AppError::new("invalid_pack", "overrides are too large"));
        }
        target.insert(rel, bytes);
    }
    overrides.extend(client_overrides);
    Ok(Mrpack { index, overrides })
}

fn is_managed_override(path: &str) -> bool {
    path.starts_with("mods/")
}

pub struct SyncedInstance {
    pub instance_id: String,
    pub game_dir: PathBuf,
    pub allowed_mods: HashSet<String>,
}

pub async fn load_pack(
    paths: &Paths,
    event: &Event,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<Mrpack> {
    reporter.step(Step::Pack);
    let mrpack_path = fetch_mrpack(paths, event, reporter, cancel).await?;
    let pack = tokio::task::spawn_blocking(move || read_mrpack(&mrpack_path)).await??;
    pack.versions()?;
    check_unique_paths(&pack)?;
    Ok(pack)
}

fn check_unique_paths(pack: &Mrpack) -> AppResult<()> {
    let index = pack
        .index
        .files
        .iter()
        .filter(|f| f.is_for_client())
        .map(|f| f.path.as_str());
    let overrides = pack
        .overrides
        .keys()
        .filter(|p| is_managed_override(p))
        .map(String::as_str);
    let mut seen = HashSet::new();
    for path in index.chain(overrides) {
        let key = paths::normalize_rel(path)
            .ok_or_else(|| AppError::new("invalid_pack", format!("unsafe path {path}")))?;
        if !seen.insert(fold_case(&key)) {
            return Err(AppError::new(
                "invalid_pack",
                format!("{path} is in the pack more than once"),
            ));
        }
    }
    Ok(())
}

/// No se actualiza nada sin verificar el hash del mrpack
pub async fn sync(
    paths: &Paths,
    event: &Event,
    pack: &Mrpack,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<SyncedInstance> {
    let mut state = EventState::load(paths, &event.id).await;
    let versions = resolve_versions(event, &pack.versions()?)?;
    let instance_id =
        ensure_instance(paths, &event.id, &mut state, &versions, reporter, cancel).await?;
    let game_dir = theseus::instance::get_full_path(&instance_id).await?;

    let managed_files = sync_files(&game_dir, pack, &state.files, reporter, cancel).await?;

    if state.pack_version.as_deref() != Some(event.pack_version.as_str()) {
        apply_install_overrides(&game_dir, &pack.overrides).await?;
    }

    let allowed_mods = managed_files
        .keys()
        .filter(|p| fold_case(p).starts_with("mods/"))
        .cloned()
        .collect();
    state.files = managed_files;
    state.minecraft_version = Some(versions.minecraft);
    state.loader = Some(versions.loader);
    state.loader_version = versions.loader_version;
    state.pack_version = Some(event.pack_version.clone());
    state.pack_sha512 = Some(event.pack_sha512.to_lowercase());
    state.save(paths, &event.id).await?;
    Ok(SyncedInstance {
        instance_id,
        game_dir,
        allowed_mods,
    })
}

async fn sync_files(
    game_dir: &Path,
    pack: &Mrpack,
    previous: &BTreeMap<String, String>,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<BTreeMap<String, String>> {
    let mut managed_files: BTreeMap<String, String> = BTreeMap::new();
    let mut listed = Vec::new();
    for file in pack.index.files.iter().filter(|f| f.is_for_client()) {
        let sha512 = file
            .hashes
            .get("sha512")
            .ok_or_else(|| AppError::new("invalid_pack", format!("{} has no sha512", file.path)))?
            .to_lowercase();
        let unsafe_path = || AppError::new("invalid_pack", format!("unsafe path {}", file.path));
        let key = paths::normalize_rel(&file.path).ok_or_else(unsafe_path)?;
        let target = paths::safe_join(game_dir, &key).ok_or_else(unsafe_path)?;
        managed_files.insert(key, sha512.clone());
        listed.push((file.clone(), target, sha512));
    }
    let to_download: Vec<_> = futures::stream::iter(listed)
        .map(|(file, target, sha512)| async move {
            let ok = matches_hash(&target, &sha512).await;
            (!ok).then_some((file, target, sha512))
        })
        .buffered(MAX_PARALLEL_HASHES)
        .filter_map(|changed| async move { changed })
        .collect()
        .await;
    for (path, bytes) in pack
        .overrides
        .iter()
        .filter(|(p, _)| is_managed_override(p))
    {
        let sha512 = net::sha512_hex(bytes);
        let unsafe_path = || AppError::new("invalid_pack", format!("unsafe path {path}"));
        let key = paths::normalize_rel(path).ok_or_else(unsafe_path)?;
        let target = paths::safe_join(game_dir, &key).ok_or_else(unsafe_path)?;
        if !matches_hash(&target, &sha512).await {
            paths::write_atomic(&target, bytes).await?;
        }
        managed_files.insert(key, sha512);
    }

    download_all(to_download, reporter, cancel).await?;

    let current: HashSet<String> = managed_files.keys().map(|k| fold_case(k)).collect();
    for old in previous.keys().filter(|p| !current.contains(&fold_case(p))) {
        if let Some(target) = paths::safe_join(game_dir, old)
            && is_file(&target).await
        {
            log::info!("removing {old} (no longer in the pack)");
            tokio::fs::remove_file(&target).await?;
        }
    }
    Ok(managed_files)
}

pub async fn apply_install_overrides(
    game_dir: &Path,
    overrides: &BTreeMap<String, Vec<u8>>,
) -> AppResult<()> {
    for (path, bytes) in overrides.iter().filter(|(p, _)| !is_managed_override(p)) {
        let target = paths::safe_join(game_dir, path)
            .ok_or_else(|| AppError::new("invalid_pack", format!("unsafe path {path}")))?;
        if !tokio::fs::try_exists(&target).await? {
            paths::write_atomic(&target, bytes).await?;
        }
    }
    Ok(())
}

async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok_and(|m| m.is_file())
}

async fn matches_hash(path: &Path, sha512: &str) -> bool {
    is_file(path).await
        && net::sha512_file(path)
            .await
            .is_ok_and(|h| h.eq_ignore_ascii_case(sha512))
}

fn safe_version(version: &str) -> AppResult<&str> {
    let ok = !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        && !version.starts_with('.');
    ok.then_some(version)
        .ok_or_else(|| AppError::new("invalid_event", format!("bad pack version {version:?}")))
}

async fn fetch_mrpack(
    paths: &Paths,
    event: &Event,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<PathBuf> {
    let dir = paths.pack_cache_dir(&event.id);
    let path = dir.join(format!("{}.mrpack", safe_version(&event.pack_version)?));
    if !matches_hash(&path, &event.pack_sha512).await {
        let done = AtomicU64::new(0);
        net::download_verified(
            &event.pack_url,
            &path,
            Expected::Sha512(&event.pack_sha512),
            event
                .pack_size
                .map_or(Size::AtMost(MAX_PACK_BYTES), Size::Exact),
            cancel,
            |n| {
                let d = done.fetch_add(n, Ordering::Relaxed) + n;
                reporter.emit(Step::Pack, None, Some(format!("{} KB", d / 1024)));
            },
        )
        .await?;
    }
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path() != path {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }
    Ok(path)
}

async fn download_all(
    files: Vec<(PackFile, PathBuf, String)>,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<()> {
    if files.is_empty() {
        return Ok(());
    }
    let total: u64 = files.iter().map(|(f, _, _)| f.file_size).sum();
    let done = AtomicU64::new(0);
    reporter.fraction(Step::Mods, 0, total);
    futures::stream::iter(files.into_iter().map(Ok::<_, AppError>))
        .try_for_each_concurrent(MAX_PARALLEL_DOWNLOADS, |(file, target, sha512)| {
            let done = &done;
            async move {
                let mut last_err =
                    AppError::new("invalid_pack", format!("{} has no download URL", file.path));
                for url in &file.downloads {
                    let mut received = 0;
                    match net::download_verified(
                        url,
                        &target,
                        Expected::Sha512(&sha512),
                        Size::Exact(file.file_size),
                        cancel,
                        |n| {
                            received += n;
                            let d = done.fetch_add(n, Ordering::Relaxed) + n;
                            reporter.fraction(Step::Mods, d, total);
                        },
                    )
                    .await
                    {
                        Ok(()) => return Ok(()),
                        Err(err) if err.code == "cancelled" => return Err(err),
                        Err(err) => {
                            done.fetch_sub(received, Ordering::Relaxed);
                            log::warn!("download of {} from {url} failed: {err}", file.path);
                            last_err = err;
                        }
                    }
                }
                Err(last_err)
            }
        })
        .await
}

async fn ensure_instance(
    paths: &Paths,
    event_id: &str,
    state: &mut EventState,
    versions: &GameVersions,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<String> {
    let loader = versions.loader.to_theseus();
    let found = match find_instance(event_id, state).await? {
        Some(meta) if !has_game_dir(paths, &meta.instance.path).await? => {
            log::warn!("la carpeta del evento {event_id} no esta, reinstalando");
            if let Err(err) = theseus::instance::remove(&meta.instance.id).await {
                log::warn!("no se pudo eliminar la antigua instancia: {err}");
            }
            state.instance_id = None;
            state.files.clear();
            state.pack_version = None;
            state.save(paths, event_id).await?;
            None
        }
        found => found,
    };
    let Some(meta) = found else {
        reporter.step(Step::Minecraft);
        let job = theseus::install::create_instance(
            event_id.to_string(),
            versions.minecraft.clone(),
            loader,
            versions.loader_version.clone(),
            None,
            None,
            InstanceLink::Unmanaged,
        )
        .await?;
        let id = wait_for_job(
            &job.job_id,
            reporter,
            versions.loader.display_name(),
            cancel,
        )
        .await?
        .ok_or_else(|| AppError::new("launcher_core", "install job returned no instance"))?;
        state.instance_id = Some(id.clone());
        state.save(paths, event_id).await?;
        return Ok(id);
    };

    let id = meta.instance.id.clone();
    if state.instance_id.as_deref() != Some(id.as_str()) {
        state.instance_id = Some(id.clone());
        state.save(paths, event_id).await?;
    }

    let content = &meta.applied_content_set;
    let versions_match = content.game_version == versions.minecraft
        && content.loader == loader
        && (versions.loader == Loader::Vanilla
            || content.loader_version == versions.loader_version);
    if !versions_match {
        log::info!(
            "updating {event_id} to Minecraft {} / {:?} {:?}",
            versions.minecraft,
            versions.loader,
            versions.loader_version
        );
        theseus::instance::edit(
            &id,
            EditInstance {
                content_set_patch: Some(theseus::prelude::AppliedContentSetPatch {
                    game_version: Some(versions.minecraft.clone()),
                    loader: Some(loader),
                    loader_version: Some(versions.loader_version.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await?;
    }
    if !versions_match || meta.instance.install_stage.as_str() != "installed" {
        reporter.step(Step::Minecraft);
        let job = theseus::install::install_existing_instance(id.clone(), false).await?;
        wait_for_job(
            &job.job_id,
            reporter,
            versions.loader.display_name(),
            cancel,
        )
        .await?;
    }
    Ok(id)
}

/// Solo se cuenta como borrado si no existe la carpeta.
/// En caso de algun error de acceso o algo, no se reinstala
async fn has_game_dir(paths: &Paths, instance_path: &str) -> AppResult<bool> {
    match tokio::fs::metadata(paths.instances_dir().join(instance_path)).await {
        Ok(meta) => Ok(meta.is_dir()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

async fn find_instance(
    event_id: &str,
    state: &EventState,
) -> AppResult<Option<theseus::prelude::InstanceMetadata>> {
    if let Some(id) = &state.instance_id {
        if let Some(meta) = theseus::instance::get(id).await? {
            return Ok(Some(meta));
        }
        log::warn!("saved instance {id} for {event_id} no longer exists");
    }
    let mut matches: Vec<_> = theseus::instance::list()
        .await?
        .into_iter()
        .filter(|m| m.instance.name == event_id)
        .collect();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        n => Err(AppError::new(
            "instance_ambiguous",
            format!("{n} instances are named {event_id}"),
        )),
    }
}

async fn wait_for_job(
    job_id: &str,
    reporter: &Reporter,
    loader: Option<&str>,
    cancel: &CancellationToken,
) -> AppResult<Option<String>> {
    let job_id = job_id
        .parse()
        .map_err(|_| AppError::new("launcher_core", format!("bad job id {job_id}")))?;
    let mut last_progress = None;
    let mut last_change = Instant::now();
    loop {
        let snap = theseus::install::get_job(job_id).await?;
        match snap.status {
            InstallJobStatus::Queued | InstallJobStatus::Running => {
                if cancel.is_cancelled() {
                    let _ = theseus::install::cancel_job(job_id).await;
                    return Err(AppError::cancelled());
                }
                let progress = snap.progress.as_ref().map(|p| (p.current, p.total));
                if progress != last_progress {
                    last_progress = progress;
                    last_change = Instant::now();
                } else if last_change.elapsed() > INSTALL_STALL_TIMEOUT {
                    let _ = theseus::install::cancel_job(job_id).await;
                    return Err(install_failed(
                        format!(
                            "install made no progress for {} minutes",
                            INSTALL_STALL_TIMEOUT.as_secs() / 60
                        ),
                        loader,
                    ));
                }
                let percent = snap
                    .progress
                    .as_ref()
                    .filter(|p| p.total > 0)
                    .map(|p| p.current as f32 * 100.0 / p.total as f32);
                reporter.emit(Step::Minecraft, percent, loader.map(String::from));
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            InstallJobStatus::Succeeded => return Ok(snap.instance_id),
            InstallJobStatus::Canceled => {
                return Err(AppError::new("cancelled", "install was cancelled"));
            }
            status => {
                let detail = snap
                    .error
                    .as_ref()
                    .map(|e| format!("{e:?}"))
                    .unwrap_or_default();
                return Err(install_failed(
                    format!("install job {status:?}: {detail}"),
                    loader,
                ));
            }
        }
    }
}

fn install_failed(message: String, loader: Option<&str>) -> AppError {
    let err = AppError::new("minecraft_install_failed", message);
    match loader {
        Some(name) => err.with_details(serde_json::json!({ "loader": name })),
        None => err,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(pack_url: String, sha512: &str) -> Event {
        serde_json::from_value(serde_json::json!({
            "id": "test_event", "name": {}, "status": "live",
            "packUrl": pack_url, "packSha512": sha512, "packVersion": "1.0.0"
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn game_dir_check_tells_deleted_from_present() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        assert!(
            !has_game_dir(&paths, "hellcraft2").await.unwrap(),
            "a deleted folder is not an error"
        );
        tokio::fs::create_dir_all(paths.instances_dir().join("hellcraft2"))
            .await
            .unwrap();
        assert!(has_game_dir(&paths, "hellcraft2").await.unwrap());
        tokio::fs::write(paths.instances_dir().join("file"), b"x")
            .await
            .unwrap();
        assert!(
            !has_game_dir(&paths, "file").await.unwrap(),
            "a file is not a game folder"
        );
    }

    #[tokio::test]
    async fn overrides_never_overwrite_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        tokio::fs::create_dir_all(game.join("config"))
            .await
            .unwrap();
        tokio::fs::write(game.join("config/a.toml"), b"player edit")
            .await
            .unwrap();

        let mut overrides = BTreeMap::new();
        overrides.insert("config/a.toml".to_string(), b"pack default".to_vec());
        overrides.insert("config/b.toml".to_string(), b"new file".to_vec());
        overrides.insert("mods/managed.jar".to_string(), b"jar".to_vec());
        apply_install_overrides(game, &overrides).await.unwrap();

        assert_eq!(
            tokio::fs::read(game.join("config/a.toml")).await.unwrap(),
            b"player edit"
        );
        assert_eq!(
            tokio::fs::read(game.join("config/b.toml")).await.unwrap(),
            b"new file"
        );
        assert!(!game.join("mods/managed.jar").exists());
    }

    #[tokio::test]
    async fn mrpack_hash_mismatch_aborts_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pack.mrpack");
        tokio::fs::write(&source, b"not the expected pack")
            .await
            .unwrap();
        let paths = Paths::new(dir.path().join("root"));
        let event = event(format!("file://{}", source.display()), &"0".repeat(128));

        let err = fetch_mrpack(
            &paths,
            &event,
            &Reporter::silent(),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "hash_mismatch");
        let cache = paths.pack_cache_dir("test_event");
        let leftover = std::fs::read_dir(&cache).map(|d| d.count()).unwrap_or(0);
        assert_eq!(leftover, 0, "no partial or unverified file is kept");
        assert!(!paths.state_file("test_event").exists());
    }

    #[tokio::test]
    async fn mrpack_with_matching_hash_is_cached() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pack.mrpack");
        tokio::fs::write(&source, b"pack bytes").await.unwrap();
        let paths = Paths::new(dir.path().join("root"));
        let event = event(
            format!("file://{}", source.display()),
            &net::sha512_hex(b"pack bytes"),
        );

        let path = fetch_mrpack(
            &paths,
            &event,
            &Reporter::silent(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"pack bytes");
    }

    fn pack_with(deps: &[(&str, &str)]) -> Mrpack {
        Mrpack {
            index: PackIndex {
                format_version: 1,
                game: "minecraft".into(),
                files: vec![],
                dependencies: deps
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            },
            overrides: BTreeMap::new(),
        }
    }

    #[test]
    fn reads_loader_from_pack() {
        let v = pack_with(&[("minecraft", "1.21.11"), ("neoforge", "21.11.45")])
            .versions()
            .unwrap();
        assert_eq!(
            (v.loader, v.loader_version.as_deref()),
            (Loader::NeoForge, Some("21.11.45"))
        );
        let v = pack_with(&[("minecraft", "1.21.1"), ("fabric-loader", "0.16.0")])
            .versions()
            .unwrap();
        assert_eq!(v.loader, Loader::Fabric);
        let v = pack_with(&[("minecraft", "1.21.1")]).versions().unwrap();
        assert_eq!((v.loader, v.loader_version), (Loader::Vanilla, None));
        for deps in [
            &[
                ("minecraft", "1"),
                ("fabric-loader", "a"),
                ("neoforge", "b"),
            ][..],
            &[("minecraft", "1"), ("forge", "a")][..],
            &[("minecraft", "1"), ("quilt-loader", "a")][..],
        ] {
            assert!(pack_with(deps).versions().is_err());
        }
        assert!(pack_with(&[("neoforge", "21")]).versions().is_err());
    }

    #[test]
    fn event_fields_override_the_pack() {
        let pack = GameVersions {
            minecraft: "1.21.11".into(),
            loader: Loader::NeoForge,
            loader_version: Some("21.11.45".into()),
        };
        let mut e = event(String::new(), "");
        assert_eq!(resolve_versions(&e, &pack).unwrap(), pack);

        e.loader_version = Some("21.11.50".into());
        assert_eq!(
            resolve_versions(&e, &pack)
                .unwrap()
                .loader_version
                .as_deref(),
            Some("21.11.50")
        );

        e.loader = Some(Loader::Fabric);
        e.loader_version = None;
        assert_eq!(
            resolve_versions(&e, &pack).unwrap_err().code,
            "invalid_event"
        );

        e.loader = Some(Loader::Vanilla);
        e.minecraft = Some("1.21.10".into());
        let v = resolve_versions(&e, &pack).unwrap();
        assert_eq!(
            (v.minecraft.as_str(), v.loader, v.loader_version),
            ("1.21.10", Loader::Vanilla, None)
        );
    }

    #[test]
    fn rejects_unsafe_versions() {
        assert!(safe_version("1.0.3").is_ok());
        assert!(safe_version("../x").is_err());
        assert!(safe_version("a/b").is_err());
        assert!(safe_version("").is_err());
    }

    fn pack_file(path: &str, url: String, bytes: &[u8]) -> PackFile {
        PackFile {
            path: path.into(),
            hashes: [("sha512".to_string(), net::sha512_hex(bytes))].into(),
            env: None,
            downloads: vec![url],
            file_size: bytes.len() as u64,
        }
    }

    #[tokio::test]
    async fn sync_files_downloads_writes_and_removes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.jar");
        std::fs::write(&src, b"mod a").unwrap();
        let game = dir.path().join("game");
        std::fs::create_dir_all(game.join("mods")).unwrap();
        std::fs::write(game.join("mods/old.jar"), b"old").unwrap();

        let mut pack = pack_with(&[("minecraft", "1.21.1")]);
        pack.index.files.push(pack_file(
            "mods/./a.jar",
            format!("file://{}", src.display()),
            b"mod a",
        ));
        pack.overrides
            .insert("mods/b.jar".into(), b"override b".to_vec());
        let previous: BTreeMap<String, String> =
            [("mods/old.jar".to_string(), "x".to_string())].into();

        let files = sync_files(
            &game,
            &pack,
            &previous,
            &Reporter::silent(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["mods/a.jar", "mods/b.jar"],
            "keys are normalized"
        );
        assert_eq!(std::fs::read(game.join("mods/a.jar")).unwrap(), b"mod a");
        assert_eq!(
            std::fs::read(game.join("mods/b.jar")).unwrap(),
            b"override b"
        );
        assert!(!game.join("mods/old.jar").exists(), "stale file removed");
        let mut names: Vec<_> = std::fs::read_dir(game.join("mods"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["a.jar", "b.jar"], "no temp files left");

        std::fs::write(game.join("mods/a.jar"), b"tampered").unwrap();
        sync_files(
            &game,
            &pack,
            &files,
            &Reporter::silent(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(game.join("mods/a.jar")).unwrap(), b"mod a");
    }

    #[tokio::test]
    async fn cancelled_sync_leaves_no_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.jar");
        std::fs::write(&src, b"mod a").unwrap();
        let game = dir.path().join("game");
        let mut pack = pack_with(&[("minecraft", "1.21.1")]);
        pack.index.files.push(pack_file(
            "mods/a.jar",
            format!("file://{}", src.display()),
            b"mod a",
        ));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = sync_files(&game, &pack, &BTreeMap::new(), &Reporter::silent(), &cancel)
            .await
            .unwrap_err();
        assert_eq!(err.code, "cancelled");
        let leftovers = std::fs::read_dir(game.join("mods")).map_or(0, |d| d.count());
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn rejects_paths_listed_twice() {
        let mut pack = pack_with(&[("minecraft", "1.21.1")]);
        pack.index
            .files
            .push(pack_file("mods/a.jar", String::new(), b"a"));
        pack.overrides.insert("config/a.toml".into(), b"x".to_vec());
        assert!(check_unique_paths(&pack).is_ok());

        let mut twice = pack_with(&[("minecraft", "1.21.1")]);
        twice
            .index
            .files
            .push(pack_file("mods/a.jar", String::new(), b"a"));
        twice.overrides.insert("mods//a.jar".into(), b"b".to_vec());
        assert_eq!(check_unique_paths(&twice).unwrap_err().code, "invalid_pack");

        let mut escaping = pack_with(&[("minecraft", "1.21.1")]);
        escaping
            .index
            .files
            .push(pack_file("../a.jar", String::new(), b"a"));
        assert!(check_unique_paths(&escaping).is_err());
    }

    #[test]
    fn reads_overrides_and_client_overrides_win() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.mrpack");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("modrinth.index.json", options).unwrap();
        zip.write_all(br#"{"formatVersion":1,"game":"minecraft","files":[],"dependencies":{"minecraft":"1.21.1"}}"#)
            .unwrap();
        zip.start_file("overrides/config/a.toml", options).unwrap();
        zip.write_all(b"common").unwrap();
        zip.start_file("client-overrides/config/a.toml", options)
            .unwrap();
        zip.write_all(b"client").unwrap();
        zip.finish().unwrap();

        let pack = read_mrpack(&path).unwrap();
        assert_eq!(pack.overrides["config/a.toml"], b"client");
    }
}
