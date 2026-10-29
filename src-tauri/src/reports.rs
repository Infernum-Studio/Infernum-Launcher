//! Informes de errores para el staff
//!
//! Solo si en ajustes está activado. Antes de comprimir se quita
//! info personal (tokens, rutas, nombres, IPs)

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use regex::{Regex, RegexBuilder};

use crate::integrity::QuarantinedFile;

const MAX_LINES: usize = 200;
const MAX_LINE_CHARS: usize = 2000;
const MAX_BODY_BYTES: usize = 128 * 1024;
const TAIL_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Crash,
    Quarantine,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "crash",
            Self::Quarantine => "quarantine",
        }
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid pattern")
}

static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (re(r"(?i)\[CHAT\].*"), "[CHAT] <chat>"),
        (
            re(r"(?i)(--(?:accessToken|xuid|clientId))\s+\S+"),
            "$1 <redacted>",
        ),
        (
            re(r"eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*"),
            "<jwt>",
        ),
        (re(r#"\bM\.C\d+_[^\s"'&<>]+"#), "<code>"),
        (re(r#"(?i)\bbearer\s+[^\s"',]+"#), "Bearer <redacted>"),
        (
            re(r#"(?i)\b((?:proxy-)?authorization"?\s*[:=]\s*"?(?:basic|digest)\s+)[^\s"',}]+"#),
            "$1<redacted>",
        ),
        (
            re(
                r#"(?i)\b([a-z_-]*(?:token|password|passwd|secret|api[_-]?key)|session[a-z_]*)("?\s*[:=]\s*"?)[^\s"',&;}]+"#,
            ),
            "$1$2<redacted>",
        ),
        (
            re(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
            "<email>",
        ),
        (re(r#"(?i)\b[a-z]:[\\/]+users[\\/]+[^\\/\r\n"']+"#), "~"),
        (re(r#"/(?:home|Users)/[^/\s"']+"#), "~"),
        (re(r"/root\b"), "~"),
        (
            re(r"\b(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}\b"),
            "<ip>",
        ),
        (re(r"(?i)\b(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}\b"), "<ip>"),
        (
            re(r"(?i)\b(?:[0-9a-f]{1,4}:){1,6}:(?:[0-9a-f]{1,4}(?::[0-9a-f]{1,4}){0,5})?\b"),
            "<ip>",
        ),
    ]
});

pub struct Redactor {
    words: Vec<(Regex, &'static str)>,
}

impl Redactor {
    /// players es (nombre, uuid) de cada cuenta
    pub fn new(
        os_user: Option<String>,
        host: Option<String>,
        players: &[(String, String)],
    ) -> Self {
        let mut words = Vec::new();
        let mut add = |word: &str, with: &'static str| {
            if word.chars().count() >= 3
                && let Ok(re) = RegexBuilder::new(&regex::escape(word))
                    .case_insensitive(true)
                    .build()
            {
                words.push((re, with));
            }
        };
        for (name, uuid) in players {
            add(name, "<player>");
            add(uuid, "<player>");
            add(&uuid.replace('-', ""), "<player>");
        }
        if let Some(user) = os_user {
            add(&user, "<user>");
        }
        if let Some(host) = host.filter(|h| !h.eq_ignore_ascii_case("localhost")) {
            add(&host, "<host>");
        }
        Self { words }
    }

    pub fn for_this_pc(players: &[(String, String)]) -> Self {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok();
        Self::new(user, sysinfo::System::host_name(), players)
    }

    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (re, with) in PATTERNS.iter() {
            out = re.replace_all(&out, *with).into_owned();
        }
        for (re, with) in &self.words {
            out = replace_word(re, &out, with);
        }
        out
    }
}

fn replace_word(re: &Regex, text: &str, with: &str) -> String {
    let joined = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in re.find_iter(text) {
        if joined(text[..m.start()].chars().next_back()) || joined(text[m.end()..].chars().next()) {
            continue;
        }
        out.push_str(&text[last..m.start()]);
        out.push_str(with);
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

fn tail_lines(redactor: &Redactor, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(MAX_LINES)..]
        .iter()
        .map(|l| redactor.redact(l).chars().take(MAX_LINE_CHARS).collect())
        .collect()
}

// Si el log es más largo que TAIL_BYTES, la primera
//  línea leida se descarta
async fn read_log_tail(path: &Path) -> String {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return String::new();
    };
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(TAIL_BYTES);
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf).await;
    let text = String::from_utf8_lossy(&buf);
    match text.find('\n') {
        Some(i) if start > 0 => text[i + 1..].to_string(),
        _ if start > 0 => String::new(),
        _ => text.into_owned(),
    }
}

fn report_header(event_id: &str, reason: Reason, quarantined: &[QuarantinedFile]) -> String {
    let mut text = format!(
        "Infernum Launcher {} ({} {})\nevent: {event_id}\nreason: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        reason.as_str(),
    );
    if !quarantined.is_empty() {
        text.push_str("quarantined:\n");
        for file in quarantined {
            let _ = writeln!(text, "  {} ({} bytes)", file.name, file.size);
        }
    }
    text
}

fn report_text(header: &str, log: &[String]) -> String {
    let mut text = header.to_string();
    let _ = writeln!(text, "--- latest.log (last {} lines) ---", log.len());
    for line in log {
        text.push_str(line);
        text.push('\n');
    }
    text
}

fn gzip(text: &str) -> Option<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).ok()?;
    encoder.finish().ok().filter(|body| !body.is_empty())
}

/// El informe ya comprimido. Si no cabe se quitan las líneas del principio
pub fn build_body(
    redactor: &Redactor,
    event_id: &str,
    reason: Reason,
    quarantined: &[QuarantinedFile],
    log: &str,
) -> Option<Vec<u8>> {
    let header = redactor.redact(&report_header(event_id, reason, quarantined));
    let mut lines = tail_lines(redactor, log);
    loop {
        let body = gzip(&report_text(&header, &lines))?;
        if body.len() <= MAX_BODY_BYTES || lines.is_empty() {
            return Some(body);
        }
        lines.drain(..lines.len().div_ceil(2));
    }
}

pub async fn send(
    event_id: String,
    reason: Reason,
    game_dir: &Path,
    quarantined: Vec<QuarantinedFile>,
    players: Vec<(String, String)>,
) {
    let base = crate::config::api_base_url();
    if !base.starts_with("https://") {
        log::info!("no API: {} report not sent", reason.as_str());
        return;
    }
    send_to(&base, event_id, reason, game_dir, quarantined, players).await
}

async fn send_to(
    base: &str,
    event_id: String,
    reason: Reason,
    game_dir: &Path,
    quarantined: Vec<QuarantinedFile>,
    players: Vec<(String, String)>,
) {
    let log = read_log_tail(&game_dir.join("logs").join("latest.log")).await;
    let event = event_id.clone();
    let body = tokio::task::spawn_blocking(move || {
        let redactor = Redactor::for_this_pc(&players);
        build_body(&redactor, &event, reason, &quarantined, &log)
    })
    .await;
    let Ok(Some(body)) = body else {
        log::warn!("{} report not built", reason.as_str());
        return;
    };
    let url = format!("{}/v1/logs", base.trim_end_matches('/'));
    let result = crate::net::CLIENT
        .post(&url)
        .query(&[("event", event_id.as_str())])
        .header("Content-Type", "application/gzip")
        .timeout(Duration::from_secs(30))
        .body(body)
        .send()
        .await;
    match result {
        Ok(resp) if resp.status().is_success() => {
            log::info!("{} report sent", reason.as_str())
        }
        Ok(resp) => log::warn!("{} report answered {}", reason.as_str(), resp.status()),
        Err(err) => log::warn!("{} report not sent: {err}", reason.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new(
            Some("alvaro842".into()),
            None,
            &[(
                "InfernumTest".into(),
                "c9e3c8b0-b1a6-3d94-bb44-30655e40c01a".into(),
            )],
        )
    }

    fn clean(text: &str) -> String {
        redactor().redact(text)
    }

    #[test]
    fn removes_jwts() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI0MiJ9.abcDEF_-123";
        assert_eq!(clean(&format!("token {jwt} end")), "token <jwt> end");
    }

    #[test]
    fn removes_microsoft_oauth_codes() {
        assert_eq!(
            clean("code M.C507_BAY.2.U.a1b2c3-d4e5!f6*g7$h8 done"),
            "code <code> done"
        );
    }

    #[test]
    fn removes_bearer_tokens() {
        assert_eq!(
            clean("Authorization: Bearer abc.def-ghi"),
            "Authorization: Bearer <redacted>"
        );
        assert_eq!(clean("bearer xyz"), "Bearer <redacted>");
    }

    #[test]
    fn removes_token_fields() {
        assert_eq!(clean("accessToken=abc123"), "accessToken=<redacted>");
        assert_eq!(clean("access_token: abc123"), "access_token: <redacted>");
        assert_eq!(
            clean(r#"{"refresh_token":"abc123","x":1}"#),
            r#"{"refresh_token":"<redacted>","x":1}"#
        );
        assert_eq!(clean("url?token=abc&x=1"), "url?token=<redacted>&x=1");
        assert_eq!(clean("sessionId = 99887766"), "sessionId = <redacted>");
        assert_eq!(clean("refreshToken=abc"), "refreshToken=<redacted>");
    }

    #[test]
    fn removes_other_secrets() {
        assert_eq!(
            clean("Authorization: Basic dXNlcjpwYXNz"),
            "Authorization: Basic <redacted>"
        );
        assert_eq!(clean("x-api-key: k123"), "x-api-key: <redacted>");
        assert_eq!(clean("apiKey=k123"), "apiKey=<redacted>");
        assert_eq!(
            clean("url?password=hunter2&x=1"),
            "url?password=<redacted>&x=1"
        );
        assert_eq!(
            clean("-Djavax.net.ssl.trustStorePassword=changeit"),
            "-Djavax.net.ssl.trustStorePassword=<redacted>"
        );
        assert_eq!(
            clean(r#"{"client_secret":"s"}"#),
            r#"{"client_secret":"<redacted>"}"#
        );
        assert_eq!(clean("Basic settings loaded"), "Basic settings loaded");
    }

    #[test]
    fn removes_chat_and_emails() {
        assert_eq!(
            clean("[Render thread/INFO]: [System] [CHAT] <Otro> hola, mi tlf es 600"),
            "[Render thread/INFO]: [System] [CHAT] <chat>"
        );
        assert_eq!(clean("mail pepe.lopez@gmail.com ok"), "mail <email> ok");
    }

    #[test]
    fn removes_launch_arguments() {
        assert_eq!(
            clean("--username InfernumTest --accessToken abc.def --xuid 123 --clientId xyz"),
            "--username <player> --accessToken <redacted> --xuid <redacted> --clientId <redacted>"
        );
    }

    #[test]
    fn replaces_home_folders() {
        assert_eq!(
            clean("/home/alvaro842/.local/share/x.jar"),
            "~/.local/share/x.jar"
        );
        assert_eq!(clean("/Users/maria/Library/x.log"), "~/Library/x.log");
        assert_eq!(
            clean(r"C:\Users\Pepe Lopez\AppData\x.jar"),
            r"~\AppData\x.jar"
        );
        assert_eq!(clean("c:/users/pepe/x"), "~/x");
        assert_eq!(clean("/root/.minecraft"), "~/.minecraft");
    }

    #[test]
    fn removes_the_os_user_anywhere() {
        assert_eq!(
            clean("hostname ALVARO842-pc, user alvaro842"),
            "hostname <user>-pc, user <user>"
        );
    }

    #[test]
    fn removes_player_names_and_uuids() {
        assert_eq!(
            clean("Setting user: InfernumTest (c9e3c8b0b1a63d94bb4430655e40c01a)"),
            "Setting user: <player> (<player>)"
        );
        assert_eq!(
            clean("uuid c9e3c8b0-b1a6-3d94-bb44-30655e40c01a"),
            "uuid <player>"
        );
    }

    #[test]
    fn removes_ip_addresses() {
        assert_eq!(
            clean("Connecting to 192.168.1.20, 25565"),
            "Connecting to <ip>, 25565"
        );
        assert_eq!(
            clean("from 2001:0db8:85a3:0000:0000:8a2e:0370:7334 and fe80::1"),
            "from <ip> and <ip>"
        );
    }

    #[test]
    fn keeps_ordinary_log_text() {
        let line = "[22:18:10] [main/INFO] [net.neoforged.fml/]: Loading 125 mods, \
                    Minecraft 1.21.11, NeoForge 21.11.45, java 21.0.5";
        assert_eq!(clean(line), line);
        assert_eq!(clean("token count 3"), "token count 3", "no = or :");
    }

    #[test]
    fn underscore_does_not_hide_names() {
        let r = Redactor::new(None, None, &[("Steve_1".into(), "x".into())]);
        assert_eq!(r.redact("foo_Steve_1 Steve_1."), "foo_<player> <player>.");
        assert_eq!(r.redact("Steve_10 aSteve_1"), "Steve_10 aSteve_1");
    }

    #[test]
    fn removes_the_host_name() {
        let r = Redactor::new(None, Some("DESKTOP-AB12".into()), &[]);
        assert_eq!(r.redact("host desktop-ab12 up"), "host <host> up");
        let r = Redactor::new(None, Some("localhost".into()), &[]);
        assert_eq!(r.redact("localhost"), "localhost");
    }

    #[test]
    fn redacts_before_cutting_long_lines() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI0MiJ9.abcDEF_-123";
        let line = format!("{}{jwt}", "x".repeat(MAX_LINE_CHARS - 25));
        let lines = tail_lines(&redactor(), &line);
        assert!(
            !lines[0].contains("eyJ"),
            "{}",
            &lines[0][MAX_LINE_CHARS - 30..]
        );
    }

    #[tokio::test]
    async fn log_tail_skips_the_cut_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latest.log");
        let mut log = "a".repeat(TAIL_BYTES as usize);
        log.push_str("\nlast line\n");
        std::fs::write(&path, &log).unwrap();
        assert_eq!(read_log_tail(&path).await, "last line\n");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        assert_eq!(read_log_tail(&path).await, "one\ntwo\n");
    }

    #[tokio::test]
    async fn sends_the_redacted_report() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        std::fs::write(
            dir.path().join("logs").join("latest.log"),
            "boom InfernumTest --accessToken abc\n",
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            let body_at = loop {
                let n = s.read(&mut buf).await.unwrap();
                req.extend_from_slice(&buf[..n]);
                if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let head = String::from_utf8_lossy(&req[..body_at]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            while req.len() < body_at + len {
                let n = s.read(&mut buf).await.unwrap();
                req.extend_from_slice(&buf[..n]);
            }
            s.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            (head, req[body_at..].to_vec())
        });
        send_to(
            &base,
            "hellcraft2".into(),
            Reason::Crash,
            dir.path(),
            Vec::new(),
            vec![(
                "InfernumTest".into(),
                "c9e3c8b0-b1a6-3d94-bb44-30655e40c01a".into(),
            )],
        )
        .await;
        let (head, body) = server.await.unwrap();
        assert!(
            head.starts_with("post /v1/logs?event=hellcraft2 "),
            "{head}"
        );
        assert!(head.contains("content-type: application/gzip"));
        let text = gunzip(&body);
        assert!(text.contains("reason: crash"));
        assert!(
            text.contains("boom <player> --accessToken <redacted>"),
            "{text}"
        );
    }

    #[test]
    fn short_names_are_not_redacted() {
        let r = Redactor::new(Some("al".into()), None, &[("Bo".into(), "x".into())]);
        assert_eq!(r.redact("all bold"), "all bold");
    }

    fn gunzip(body: &[u8]) -> String {
        use std::io::Read;
        let mut out = String::new();
        flate2::read::GzDecoder::new(body)
            .read_to_string(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn report_has_the_last_200_lines_redacted() {
        let log: String = (0..500)
            .map(|i| format!("line {i} /home/alvaro842/x\n"))
            .collect();
        let quarantined = [QuarantinedFile {
            name: "cheat.jar".into(),
            size: 1234,
        }];
        let body = build_body(
            &redactor(),
            "hellcraft2",
            Reason::Quarantine,
            &quarantined,
            &log,
        )
        .unwrap();
        let text = gunzip(&body);
        assert!(text.contains("event: hellcraft2\nreason: quarantine\n"));
        assert!(text.contains("  cheat.jar (1234 bytes)\n"));
        assert!(text.contains("last 200 lines"));
        assert!(!text.contains("line 299 "));
        assert!(text.contains("line 300 ~/x"));
        assert!(text.contains("line 499 ~/x"));
        assert!(!text.contains("alvaro842"));
    }

    #[test]
    fn report_never_exceeds_128_kb() {
        let mut seed = 1u64;
        let log: String = (0..200)
            .map(|_| {
                (0..1900)
                    .map(|_| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        char::from(b'!' + (seed >> 58) as u8)
                    })
                    .collect::<String>()
                    + "\n"
            })
            .collect();
        let body = build_body(&redactor(), "hellcraft2", Reason::Crash, &[], &log).unwrap();
        assert!(body.len() <= MAX_BODY_BYTES, "{}", body.len());
        assert!(gunzip(&body).contains("reason: crash"));
    }
}
