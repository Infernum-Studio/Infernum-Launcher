use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use futures::StreamExt;
use sha2::{Digest, Sha256, Sha512};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::paths::TempFile;

pub static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(crate::config::user_agent())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .redirect(redirect_policy())
        .build()
        .expect("HTTP client")
});

/// las redirecciones también deben mantenerse en https
fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else if attempt.url().scheme() != "https" {
            attempt.error("redirect to a non-HTTPS URL")
        } else {
            attempt.follow()
        }
    })
}

#[derive(Debug, Clone)]
pub enum Expected<'a> {
    Sha512(&'a str),
    Sha256(&'a str),
}

#[derive(Debug, Clone, Copy)]
pub enum Size {
    Exact(u64),
    AtMost(u64),
}

impl Size {
    fn limit(self) -> u64 {
        match self {
            Self::Exact(n) | Self::AtMost(n) => n,
        }
    }
}

pub fn sha512_hex(bytes: &[u8]) -> String {
    hex::encode(Sha512::digest(bytes))
}

pub async fn sha512_file(path: &Path) -> std::io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha512::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(std::io::Error::other)?
}

fn local_path(url: &str) -> Option<&str> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let path = url.strip_prefix("file://")?;
    let bytes = path.as_bytes();
    if cfg!(windows) && bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' {
        return Some(&path[1..]);
    }
    Some(path)
}

fn check_scheme(url: &str) -> AppResult<()> {
    if url.starts_with("https://") || local_path(url).is_some() {
        Ok(())
    } else {
        Err(AppError::network(format!("refusing non-HTTPS URL {url}")))
    }
}

pub async fn fetch_bytes(url: &str, max_bytes: usize) -> AppResult<Vec<u8>> {
    check_scheme(url)?;
    if let Some(path) = local_path(url) {
        return Ok(tokio::fs::read(path).await?);
    }
    let resp = CLIENT
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?;
    let mut out = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > max_bytes {
            return Err(AppError::network(format!(
                "{url} is larger than {max_bytes} bytes"
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

pub async fn download_verified(
    url: &str,
    dest: &Path,
    expected: Expected<'_>,
    size: Size,
    cancel: &CancellationToken,
    mut on_bytes: impl FnMut(u64) + Send,
) -> AppResult<()> {
    check_scheme(url)?;
    if cancel.is_cancelled() {
        return Err(AppError::cancelled());
    }
    let parent = dest.parent().unwrap_or(Path::new("."));
    tokio::fs::create_dir_all(parent).await?;
    let tmp = TempFile::beside(dest, "download");
    let mut file = tokio::fs::File::create(tmp.path()).await?;
    let mut sha512 = Sha512::new();
    let mut sha256 = Sha256::new();
    let mut total: u64 = 0;
    let too_big = || AppError::integrity(format!("{url}: more bytes than expected"));

    if let Some(path) = local_path(url) {
        let bytes = tokio::fs::read(path).await?;
        total = bytes.len() as u64;
        if total > size.limit() {
            return Err(too_big());
        }
        sha512.update(&bytes);
        sha256.update(&bytes);
        file.write_all(&bytes).await?;
        on_bytes(total);
    } else {
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::cancelled()),
            resp = CLIENT.get(url).send() => resp?.error_for_status()?,
        };
        let mut stream = resp.bytes_stream();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::cancelled()),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk?;
            total += chunk.len() as u64;
            if total > size.limit() {
                return Err(too_big());
            }
            sha512.update(&chunk);
            sha256.update(&chunk);
            file.write_all(&chunk).await?;
            on_bytes(chunk.len() as u64);
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    if let Size::Exact(size) = size
        && size != total
    {
        return Err(AppError::integrity(format!(
            "{url}: size {total} does not match expected {size}"
        )));
    }
    let (actual, wanted) = match expected {
        Expected::Sha512(h) => (hex::encode(sha512.finalize()), h),
        Expected::Sha256(h) => (hex::encode(sha256.finalize()), h),
    };
    if !actual.eq_ignore_ascii_case(wanted) {
        return Err(AppError::integrity(format!("{url}: hash mismatch")));
    }
    tmp.persist(dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    async fn download(
        src: &Path,
        dest: &Path,
        sha512: &str,
        size: Size,
        cancel: &CancellationToken,
    ) -> AppResult<()> {
        download_verified(
            &file_url(src),
            dest,
            Expected::Sha512(sha512),
            size,
            cancel,
            |_| {},
        )
        .await
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "src.bin")
            .collect()
    }

    #[tokio::test]
    async fn verifies_size_and_hash_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"hello").unwrap();
        let dest = dir.path().join("out.bin");
        let good = sha512_hex(b"hello");
        let none = CancellationToken::new();

        for (sha, size) in [
            (good.as_str(), Size::Exact(4)),
            (good.as_str(), Size::Exact(6)),
            (good.as_str(), Size::AtMost(4)),
            (&"0".repeat(128)[..], Size::Exact(5)),
        ] {
            let err = download(&src, &dest, sha, size, &none).await.unwrap_err();
            assert_eq!(err.code, "hash_mismatch", "{size:?}");
            assert!(leftovers(dir.path()).is_empty(), "no temp file is kept");
        }

        download(&src, &dest, &good, Size::AtMost(5), &none)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(leftovers(dir.path()), ["out.bin"]);
    }

    #[tokio::test]
    async fn cancelled_download_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"hello").unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = download(
            &src,
            &dir.path().join("out.bin"),
            &sha512_hex(b"hello"),
            Size::Exact(5),
            &cancel,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "cancelled");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn only_https_and_debug_file_urls() {
        assert!(check_scheme("https://example.com/a").is_ok());
        assert!(check_scheme("http://example.com/a").is_err());
        assert!(check_scheme("ftp://example.com/a").is_err());
        assert!(check_scheme("file:///tmp/a").is_ok());
    }
}
