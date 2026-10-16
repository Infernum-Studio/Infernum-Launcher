use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn events_cache(&self) -> PathBuf {
        self.cache_dir().join("events.json")
    }
    pub fn pack_cache_dir(&self, event_id: &str) -> PathBuf {
        self.cache_dir().join("packs").join(event_id)
    }
    pub fn java_dir(&self, major: u32) -> PathBuf {
        self.root.join("java").join(major.to_string())
    }
    pub fn state_file(&self, event_id: &str) -> PathBuf {
        self.root.join("state").join(format!("{event_id}.json"))
    }
    pub fn quarantine_dir(&self, event_id: &str) -> PathBuf {
        self.root.join("quarantine").join(event_id)
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("theseus").join("profiles")
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
}

#[derive(Debug)]
pub struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    pub fn beside(dest: &Path, tag: &str) -> Self {
        let parent = dest.parent().unwrap_or(Path::new("."));
        Self {
            path: parent.join(format!(".{tag}.{}.tmp", uuid::Uuid::new_v4().simple())),
            armed: true,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn persist(mut self, dest: &Path) -> std::io::Result<()> {
        tokio::fs::rename(&self.path, dest).await?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    tokio::fs::create_dir_all(parent).await?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = TempFile::beside(path, name);
    tokio::fs::write(tmp.path(), bytes).await?;
    tmp.persist(path).await
}

pub fn normalize_rel(relative: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            p if p.contains(':') => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let relative = normalize_rel(relative)?;
    Some(
        relative
            .split('/')
            .fold(base.to_path_buf(), |p, part| p.join(part)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_escapes() {
        let base = Path::new("/game");
        assert_eq!(
            safe_join(base, "mods/a.jar").unwrap(),
            Path::new("/game/mods/a.jar")
        );
        assert!(safe_join(base, "../evil.jar").is_none());
        assert!(safe_join(base, "mods/../../evil").is_none());
        assert!(safe_join(base, "C:/Windows/x").is_none());
        assert!(safe_join(base, "").is_none());
        assert_eq!(
            safe_join(base, "/config/x.toml").unwrap(),
            Path::new("/game/config/x.toml")
        );
    }

    #[test]
    fn normalizes_like_safe_join() {
        assert_eq!(normalize_rel("mods/./a.jar").as_deref(), Some("mods/a.jar"));
        assert_eq!(normalize_rel("/mods//a.jar").as_deref(), Some("mods/a.jar"));
        assert_eq!(normalize_rel("mods\\a.jar").as_deref(), Some("mods/a.jar"));
        assert_eq!(normalize_rel("mods/../a.jar"), None);
        assert_eq!(normalize_rel("./"), None);
    }

    #[tokio::test]
    async fn temp_file_is_removed_unless_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.txt");

        let dropped = TempFile::beside(&dest, "test");
        tokio::fs::write(dropped.path(), b"x").await.unwrap();
        let dropped_path = dropped.path().to_path_buf();
        drop(dropped);
        assert!(!dropped_path.exists(), "dropped temp file is deleted");

        let kept = TempFile::beside(&dest, "test");
        tokio::fs::write(kept.path(), b"y").await.unwrap();
        kept.persist(&dest).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"y");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn write_atomic_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/settings.json");
        write_atomic(&path, b"one").await.unwrap();
        write_atomic(&path, b"two").await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}
