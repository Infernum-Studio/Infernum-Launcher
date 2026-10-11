use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::accounts::Account;

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_RAM_MB: u32 = 2048;
pub const MIN_RAM_MB: u32 = 1024;
pub const LANGUAGES: &[&str] = &["es"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub onboarding_done: bool,
    pub language: String,
    pub ram_mb: u32,
    pub jvm_args: String,
    pub discord_rpc: bool,
    pub close_on_launch: bool,
    pub log_reports: bool,
    pub active_account: Option<String>,
    pub accounts: Vec<Account>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            onboarding_done: false,
            language: "es".to_string(),
            ram_mb: DEFAULT_RAM_MB,
            jvm_args: String::new(),
            discord_rpc: true,
            close_on_launch: false,
            log_reports: true,
            active_account: None,
            accounts: Vec::new(),
        }
    }
}

const LARGE_RAM_MB: u64 = 8192;

pub fn max_ram_mb(total_mb: u64) -> u32 {
    let max = if total_mb < LARGE_RAM_MB {
        total_mb.saturating_sub(1024)
    } else {
        total_mb * 3 / 4
    };
    (max as u32).max(MIN_RAM_MB)
}

pub fn validate_jvm_args(args: &str) -> Result<Vec<String>, &'static str> {
    let parts = shlex::split(args).ok_or("jvm_args_unparsable")?;
    if parts
        .iter()
        .any(|a| a.starts_with("-Xmx") || a.starts_with("-Xms"))
    {
        return Err("jvm_args_memory");
    }
    if !parts.iter().all(|a| is_allowed_jvm_arg(a)) {
        return Err("jvm_args_blocked");
    }
    Ok(parts)
}

/// Se bloquean argumentos que permitan modificar comose carga el juego
///  o comportamiento del modloader, ya que podrían usar mods bypasseando el sistema
fn is_allowed_jvm_arg(arg: &str) -> bool {
    const BLOCKED_XX: &[&str] = &[
        "VMOptionsFile",
        "Flags",
        "SharedArchiveFile",
        "ArchiveClassesAtExit",
    ];
    const X_PREFIXES: &[&str] = &["-Xss", "-Xmn", "-Xlog:", "-Xshare:"];
    const PROPERTIES: &[&str] = &[
        "file.encoding",
        "stdout.encoding",
        "stderr.encoding",
        "sun.stdout.encoding",
        "sun.stderr.encoding",
        "user.language",
        "user.country",
        "java.net.preferIPv4Stack",
        "java.net.preferIPv6Addresses",
    ];
    if let Some(flag) = arg.strip_prefix("-XX:") {
        let name = flag.trim_start_matches(['+', '-']);
        let name = name.split('=').next().unwrap_or_default();
        return !name.is_empty() && !BLOCKED_XX.contains(&name);
    }
    if let Some(property) = arg.strip_prefix("-D") {
        let key = property.split('=').next().unwrap_or_default();
        return PROPERTIES.contains(&key);
    }
    X_PREFIXES.iter().any(|p| arg.starts_with(p))
}

impl Settings {
    pub fn sanitize(&mut self, total_ram_mb: u64) {
        self.schema_version = SCHEMA_VERSION;
        if !LANGUAGES.contains(&self.language.as_str()) {
            self.language = "es".to_string();
        }
        self.ram_mb = self.ram_mb.clamp(MIN_RAM_MB, max_ram_mb(total_ram_mb));
        if validate_jvm_args(&self.jvm_args).is_err() {
            self.jvm_args.clear();
        }
        self.accounts.retain(Account::is_valid);
        let mut seen = std::collections::HashSet::new();
        self.accounts.retain(|a| seen.insert(a.id.clone()));
        if self
            .active_account
            .as_ref()
            .is_some_and(|id| !self.accounts.iter().any(|a| &a.id == id))
        {
            self.active_account = self.accounts.first().map(|a| a.id.clone());
        }
    }

    pub fn from_json_lenient(bytes: &[u8]) -> Option<Self> {
        let serde_json::Value::Object(file) = serde_json::from_slice(bytes).ok()? else {
            return None;
        };
        let serde_json::Value::Object(mut merged) =
            serde_json::to_value(Self::default()).expect("settings serialize")
        else {
            unreachable!()
        };
        for (key, value) in file {
            if !merged.contains_key(&key) {
                continue;
            }
            let mut candidate = merged.clone();
            candidate.insert(key.clone(), value.clone());
            if serde_json::from_value::<Self>(candidate.into()).is_ok() {
                merged.insert(key, value);
            }
        }
        serde_json::from_value(merged.into()).ok()
    }

    pub async fn load(path: &Path, total_ram_mb: u64) -> Self {
        let mut settings = match tokio::fs::read(path).await {
            Ok(bytes) => match Self::from_json_lenient(&bytes) {
                Some(s) => s,
                None => {
                    log::warn!("settings file is unreadable; keeping a .bak copy");
                    let _ = tokio::fs::rename(path, path.with_extension("json.bak")).await;
                    Self::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                log::warn!("could not read settings: {err}");
                Self::default()
            }
        };
        settings.sanitize(total_ram_mb);
        if let Err(err) = settings.save(path).await {
            log::warn!("could not write settings: {err}");
        }
        settings
    }

    pub async fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(path, &bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAM: u64 = 16 * 1024;

    #[test]
    fn invalid_field_falls_back_alone() {
        let s = Settings::from_json_lenient(
            br#"{"closeOnLaunch":true,"ramMb":"lots","discordRpc":false,"unknown":1}"#,
        )
        .unwrap();
        assert!(s.close_on_launch);
        assert_eq!(s.ram_mb, DEFAULT_RAM_MB);
        assert!(!s.discord_rpc);
    }

    #[test]
    fn sanitize_clamps_values() {
        let mut s = Settings {
            ram_mb: 999_999,
            language: "fr".into(),
            jvm_args: "-Xmx8G -XX:+UseG1GC".into(),
            active_account: Some("missing".into()),
            ..Settings::default()
        };
        s.sanitize(RAM);
        assert_eq!(s.ram_mb, max_ram_mb(RAM));
        assert_eq!(s.language, "es");
        assert_eq!(s.jvm_args, "");
        assert_eq!(s.active_account, None);

        let mut low = Settings {
            ram_mb: 10,
            ..Settings::default()
        };
        low.sanitize(RAM);
        assert_eq!(low.ram_mb, MIN_RAM_MB);
    }

    #[test]
    fn max_ram_by_pc_size() {
        assert_eq!(max_ram_mb(4096), 3072);
        assert_eq!(max_ram_mb(6144), 5120);
        assert_eq!(max_ram_mb(7800), 6776);
        assert_eq!(max_ram_mb(8192), 6144);
        assert_eq!(max_ram_mb(16384), 12288);
        assert_eq!(max_ram_mb(1500), MIN_RAM_MB);
    }

    #[test]
    fn jvm_args_reject_memory_flags() {
        assert!(validate_jvm_args("-XX:+UseZGC -Dfile.encoding=UTF-8").is_ok());
        assert_eq!(validate_jvm_args("-Xms1G"), Err("jvm_args_memory"));
        assert_eq!(validate_jvm_args("\"unclosed"), Err("jvm_args_unparsable"));
    }

    #[test]
    fn jvm_args_allow_only_tuning() {
        for ok in [
            "",
            "-XX:+UseG1GC -XX:MaxGCPauseMillis=50 -XX:-UseAdaptiveSizePolicy",
            "-Xss2M -Xmn512M -Xlog:gc -Xshare:auto",
            "-Djava.net.preferIPv4Stack=true -Dfile.encoding=UTF-8",
        ] {
            assert!(validate_jvm_args(ok).is_ok(), "{ok}");
        }
        for blocked in [
            "-javaagent:cheat.jar",
            "-agentlib:jdwp=transport=dt_socket",
            "-agentpath:/x.so",
            "-cp extra.jar",
            "--class-path extra.jar",
            "-Xbootclasspath/a:cheat.jar",
            "@args.txt",
            "-Dfabric.addMods=cheat.jar",
            "-Dfml.modFolders=x",
            "-Djava.system.class.loader=X",
            "-XX:VMOptionsFile=opts.txt",
            "-XX:Flags=flags.txt",
            "-XX:SharedArchiveFile=x.jsa",
            "-XX:",
            "--add-opens java.base/java.lang=ALL-UNNAMED",
        ] {
            assert_eq!(
                validate_jvm_args(blocked),
                Err("jvm_args_blocked"),
                "{blocked}"
            );
        }
    }

    #[tokio::test]
    async fn corrupt_file_is_backed_up_and_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        tokio::fs::write(&path, b"{ not json").await.unwrap();

        let s = Settings::load(&path, RAM).await;
        assert_eq!(s, {
            let mut d = Settings::default();
            d.sanitize(RAM);
            d
        });
        assert_eq!(
            tokio::fs::read(path.with_extension("json.bak"))
                .await
                .unwrap(),
            b"{ not json"
        );
        let rewritten = tokio::fs::read(&path).await.unwrap();
        assert!(Settings::from_json_lenient(&rewritten).is_some());
    }
}
