use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::net::{self, Expected, Size};
use crate::paths::Paths;
use crate::progress::{Reporter, Step};

const MAX_JRE_BYTES: u64 = 512 * 1024 * 1024;

fn adoptium_platform() -> AppResult<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "mac",
        other => return Err(no_jre(format!("no Temurin builds for {other}"))),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "x86" => "x32",
        "arm" => "arm",
        other => return Err(no_jre(format!("no Temurin builds for {other}"))),
    };
    Ok((os, arch))
}

fn no_jre(message: String) -> AppError {
    AppError::new("java_unavailable", message)
}

pub fn java_exe(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("bin").join("javaw.exe")
    } else {
        home.join("bin").join("java")
    }
}

#[derive(Deserialize)]
struct Asset {
    binary: Binary,
}
#[derive(Deserialize)]
struct Binary {
    package: Package,
}
#[derive(Deserialize)]
struct Package {
    link: String,
    checksum: String,
    size: Option<u64>,
    name: String,
}

pub async fn ensure(
    paths: &Paths,
    major: u32,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<PathBuf> {
    reporter.step(Step::Java);
    let home = paths.java_dir(major);
    let exe = java_exe(&home);

    let version = match check(&exe, major).await {
        Some(v) => v,
        None => {
            install(paths, major, reporter, cancel).await?;
            check(&exe, major).await.ok_or_else(|| {
                AppError::new(
                    "java_install_failed",
                    format!("Java {major} does not run after install"),
                )
            })?
        }
    };
    theseus::jre::set_java_version(version).await?;
    Ok(exe)
}

async fn check(exe: &Path, major: u32) -> Option<theseus::prelude::JavaVersion> {
    if !tokio::fs::try_exists(exe).await.unwrap_or(false) {
        return None;
    }
    match theseus::jre::check_jre(exe.to_path_buf()).await {
        Ok(v) if v.parsed_version == major => Some(v),
        Ok(v) => {
            log::warn!(
                "{} is Java {}, expected {major}",
                exe.display(),
                v.parsed_version
            );
            None
        }
        Err(err) => {
            log::warn!("could not run {}: {err}", exe.display());
            None
        }
    }
}

async fn install(
    paths: &Paths,
    major: u32,
    reporter: &Reporter,
    cancel: &CancellationToken,
) -> AppResult<()> {
    let (os, arch) = adoptium_platform()?;
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={arch}&image_type=jre&os={os}&vendor=eclipse"
    );
    let assets: Vec<Asset> = serde_json::from_slice(&net::fetch_bytes(&url, 1024 * 1024).await?)?;
    let package = assets
        .into_iter()
        .next()
        .ok_or_else(|| no_jre(format!("no Temurin JRE {major} for {os}/{arch}")))?
        .binary
        .package;
    log::info!("installing Java {major} from {}", package.name);

    let file_name = Path::new(&package.name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_archive(n))
        .ok_or_else(|| {
            AppError::new(
                "invalid_archive",
                format!("unexpected Java package {:?}", package.name),
            )
        })?;
    let tmp_dir = paths.tmp_dir();
    tokio::fs::create_dir_all(&tmp_dir).await?;
    let archive = tmp_dir.join(file_name);
    let total = package.size.unwrap_or(0);
    let done = AtomicU64::new(0);
    net::download_verified(
        &package.link,
        &archive,
        Expected::Sha256(&package.checksum),
        package
            .size
            .map_or(Size::AtMost(MAX_JRE_BYTES), Size::Exact),
        cancel,
        |n| {
            let d = done.fetch_add(n, Ordering::Relaxed) + n;
            reporter.fraction(Step::Java, d, total);
        },
    )
    .await?;

    reporter.step(Step::Java);
    let staging = tmp_dir.join(format!("java-{major}-{}", uuid::Uuid::new_v4().simple()));
    let target = paths.java_dir(major);
    let result = {
        let (archive, staging) = (archive.clone(), staging.clone());
        tokio::task::spawn_blocking(move || extract(&archive, &staging)).await
    };
    let _ = tokio::fs::remove_file(&archive).await;
    let home = match result {
        Ok(Ok(home)) => home,
        Ok(Err(err)) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return Err(err);
        }
        Err(err) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return Err(err.into());
        }
    };

    // La versión vieja no se elimina hasta que la nueva esta completamente
    // descargada y extraída, y cuando la nueva la sustituya.
    if tokio::fs::try_exists(&target).await? {
        let old = tmp_dir.join(format!("java-old-{}", uuid::Uuid::new_v4().simple()));
        tokio::fs::rename(&target, &old).await?;
        let _ = tokio::fs::remove_dir_all(&old).await;
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let moved = tokio::fs::rename(&home, &target).await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    moved?;
    Ok(())
}

fn is_archive(name: &str) -> bool {
    name.ends_with(".zip") || name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

fn extract(archive: &Path, dest: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dest)?;
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let out = dest.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out)?;
            } else {
                if let Some(p) = out.parent() {
                    std::fs::create_dir_all(p)?;
                }
                let mut f = std::fs::File::create(&out)?;
                std::io::copy(&mut entry, &mut f)?;
                #[cfg(unix)]
                if let Some(mode) = entry.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
                }
            }
        }
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut tar = tar::Archive::new(gz);
        tar.set_preserve_permissions(true);
        // Archive::unpack mantiene cada entrada dentro del desttino, incluidas
        //  las escrituras a través de symlinks y hard links
        tar.unpack(dest)?;
    } else {
        return Err(AppError::new(
            "invalid_archive",
            format!("unknown archive type {name}"),
        ));
    }
    single_top_dir(dest)
}

fn single_top_dir(dir: &Path) -> AppResult<PathBuf> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    match <[PathBuf; 1]>::try_from(dirs) {
        Ok([home]) => Ok(home),
        Err(dirs) => Err(AppError::new(
            "invalid_archive",
            format!(
                "expected one top-level folder in the Java archive, found {}",
                dirs.len()
            ),
        )),
    }
}

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<ManifestVersion>,
}
#[derive(Deserialize)]
struct ManifestVersion {
    id: String,
    url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionJson {
    java_version: Option<JavaVersionInfo>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavaVersionInfo {
    major_version: u32,
}

pub async fn required_major(paths: &Paths, minecraft: &str) -> AppResult<u32> {
    let safe: String = minecraft
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let cache = paths
        .cache_dir()
        .join("java-major")
        .join(format!("{safe}.txt"));
    if let Ok(text) = tokio::fs::read_to_string(&cache).await
        && let Ok(major) = text.trim().parse()
    {
        return Ok(major);
    }
    let manifest: Manifest = serde_json::from_slice(
        &net::fetch_bytes(
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            8 * 1024 * 1024,
        )
        .await?,
    )?;
    let version = manifest
        .versions
        .into_iter()
        .find(|v| v.id == minecraft)
        .ok_or_else(|| {
            AppError::new(
                "invalid_pack",
                format!("unknown Minecraft version {minecraft}"),
            )
        })?;
    let json: VersionJson =
        serde_json::from_slice(&net::fetch_bytes(&version.url, 4 * 1024 * 1024).await?)?;
    let major = json.java_version.map_or(8, |j| j.major_version);
    crate::paths::write_atomic(&cache, major.to_string().as_bytes()).await?;
    Ok(major)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_gz(
        path: &Path,
        build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>),
    ) {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut builder = tar::Builder::new(gz);
        build(&mut builder);
        builder.into_inner().unwrap().finish().unwrap();
    }

    fn header(kind: tar::EntryType, size: u64) -> tar::Header {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(kind);
        h.set_size(size);
        h.set_mode(0o755);
        h
    }

    #[test]
    fn extracts_the_top_level_folder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("jre.tar.gz");
        tar_gz(&archive, |b| {
            let mut h = header(tar::EntryType::Regular, 4);
            b.append_data(&mut h, "jdk-21-jre/bin/java", &b"java"[..])
                .unwrap();
            let mut h = header(tar::EntryType::Regular, 1);
            b.append_data(&mut h, "NOTICE", &b"n"[..]).unwrap();
        });
        let home = extract(&archive, &dir.path().join("out")).unwrap();
        assert!(home.ends_with("jdk-21-jre"));
        assert_eq!(std::fs::read(home.join("bin/java")).unwrap(), b"java");
    }

    #[cfg(unix)]
    #[test]
    fn tar_cannot_write_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let archive = dir.path().join("evil.tar.gz");
        tar_gz(&archive, |b| {
            let mut h = header(tar::EntryType::Symlink, 0);
            b.append_link(&mut h, "jdk/lib", &outside).unwrap();
            let mut h = header(tar::EntryType::Regular, 4);
            b.append_data(&mut h, "jdk/lib/evil", &b"evil"[..]).unwrap();
        });
        assert!(extract(&archive, &dir.path().join("out")).is_err());
        assert!(!outside.join("evil").exists());
    }

    #[test]
    fn zip_skips_escaping_entries() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("jre.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("jdk-21-jre/bin/javaw.exe", options).unwrap();
        zip.write_all(b"exe").unwrap();
        zip.start_file("../evil.txt", options).unwrap();
        zip.write_all(b"evil").unwrap();
        zip.finish().unwrap();

        let out = dir.path().join("out");
        let home = extract(&archive, &out).unwrap();
        assert_eq!(std::fs::read(home.join("bin/javaw.exe")).unwrap(), b"exe");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn needs_exactly_one_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(single_top_dir(dir.path()).is_err());
        std::fs::create_dir(dir.path().join("a")).unwrap();
        assert!(single_top_dir(dir.path()).is_ok());
        std::fs::create_dir(dir.path().join("b")).unwrap();
        assert!(single_top_dir(dir.path()).is_err());
    }
}
