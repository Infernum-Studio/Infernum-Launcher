//! Inicio de sesión con Discord para la API de Infernum
//!
//! El navegador vuelve a 127.0.0.1:53682/callback con el
//! código, y la API lo cambia por un token de sesión propio.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::paths::Paths;

pub const DISCORD_CLIENT_ID: &str = "1557089735132975124";
const PORT: u16 = 53682;
const REDIRECT_URI: &str = "http://127.0.0.1:53682/callback";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_CONNECTIONS: usize = 16;
const EXPIRY_MARGIN: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

/// Leer, guardar y borrar session.json no va en paralelo, para que un
/// cierre de sesión no se mezcle con un inicio de sesión a la vez
static SESSION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordUser {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub user: DiscordUser,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("user", &self.user)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub user: DiscordUser,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl Session {
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            user: self.user.clone(),
            expires_at: self.expires_at,
        }
    }

    fn is_valid(&self) -> bool {
        !self.token.is_empty() && self.expires_at - EXPIRY_MARGIN > chrono::Utc::now()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LoginOutcome {
    LoggedIn { session: SessionInfo },
    Cancelled,
    Denied,
    TimedOut,
    Replaced,
}

fn failed(message: impl Into<String>) -> AppError {
    AppError::new("discord_login_failed", message)
}

async fn remove_session_file(paths: &Paths) -> AppResult<()> {
    match tokio::fs::remove_file(paths.session_file()).await {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

pub async fn current(paths: &Paths) -> Option<Session> {
    let _guard = SESSION_LOCK.lock().await;
    let bytes = tokio::fs::read(paths.session_file()).await.ok()?;
    match serde_json::from_slice::<Session>(&bytes) {
        Ok(session) if session.is_valid() => return Some(session),
        Ok(_) => log::info!("discord session expired"),
        Err(_) => log::warn!("session.json is unreadable; removing it"),
    }
    if let Err(err) = remove_session_file(paths).await {
        log::warn!("could not remove session.json: {err}");
    }
    None
}

/// Solo borra la sesión del PC. El token sigue siendo válido en la API
/// hasta que caduca (máx 7 días)
pub async fn logout(paths: &Paths) -> AppResult<()> {
    let _guard = SESSION_LOCK.lock().await;
    remove_session_file(paths).await
}

async fn save_unless_cancelled(
    paths: &Paths,
    session: &Session,
    cancel: &CancellationToken,
) -> AppResult<bool> {
    let _guard = SESSION_LOCK.lock().await;
    if cancel.is_cancelled() {
        return Ok(false);
    }
    crate::paths::write_atomic_private(&paths.session_file(), &serde_json::to_vec(session)?)
        .await?;
    Ok(true)
}

fn random_token() -> AppResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| AppError::new("internal", format!("no system randomness: {e}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn code_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn authorize_url(state: &str, challenge: &str) -> String {
    reqwest::Url::parse_with_params(
        "https://discord.com/oauth2/authorize",
        [
            ("response_type", "code"),
            ("client_id", DISCORD_CLIENT_ID),
            ("scope", "identify"),
            ("redirect_uri", REDIRECT_URI),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .expect("static URL")
    .into()
}

#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Denied,
    Ignored,
}

fn parse_callback(request: &str, expected_state: &str) -> Callback {
    let Some(target) = request
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|rest| rest.split(' ').next())
    else {
        return Callback::Ignored;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::Ignored;
    };
    if url.path() != "/callback" {
        return Callback::Ignored;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if param("state").as_deref() != Some(expected_state) {
        return Callback::Ignored;
    }
    match (param("code"), param("error")) {
        (Some(code), None) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Denied,
    }
}

const PAGE_DONE: &str = "Sesión iniciada. Ya puedes cerrar esta pestaña y volver al launcher.";
const PAGE_DENIED: &str = "Inicio de sesión cancelado. Puedes cerrar esta pestaña.";

async fn respond(stream: &mut TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Infernum Launcher</title>\
         <body style=\"font-family:sans-serif;background:#111;color:#eee;\
         display:grid;place-items:center;height:100vh;margin:0\"><p>{text}</p></body>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let read = async {
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_REQUEST_BYTES {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(())
    };
    tokio::time::timeout(REQUEST_TIMEOUT, read).await.ok()??;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

async fn handle_connection(mut stream: TcpStream, state: Arc<str>) -> Callback {
    let Some(request) = read_request(&mut stream).await else {
        return Callback::Ignored;
    };
    let callback = parse_callback(&request, &state);
    match callback {
        Callback::Code(_) => respond(&mut stream, "200 OK", PAGE_DONE).await,
        Callback::Denied => respond(&mut stream, "200 OK", PAGE_DENIED).await,
        Callback::Ignored => respond(&mut stream, "404 Not Found", "").await,
    }
    callback
}

async fn bind(cancel: &CancellationToken) -> AppResult<Option<TcpListener>> {
    for _ in 0..10 {
        match TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(listener) => return Ok(Some(listener)),
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(err) => {
                return Err(AppError::io(format!(
                    "could not listen on 127.0.0.1:{PORT}: {err}"
                )));
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return Ok(None),
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    Err(AppError::new(
        "discord_port_busy",
        format!("port {PORT} is in use"),
    ))
}

#[derive(Debug, PartialEq, Eq)]
enum Wait {
    Code(String),
    Denied,
    Cancelled,
    TimedOut,
}

async fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Wait {
    let state: Arc<str> = state.into();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Wait::Cancelled,
            _ = tokio::time::sleep_until(deadline) => return Wait::TimedOut,
            // Con el máximo, esperan en la cola del sistema
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                if let Ok((stream, _)) = accepted {
                    connections.spawn(handle_connection(stream, state.clone()));
                }
            }
            Some(Ok(callback)) = connections.join_next(), if !connections.is_empty() => {
                match callback {
                    Callback::Code(code) => return Wait::Code(code),
                    Callback::Denied => return Wait::Denied,
                    Callback::Ignored => {}
                }
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeRequest<'a> {
    code: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeResponse {
    session_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    discord_user: DiscordUser,
}

async fn exchange(api_base: &str, code: &str, verifier: &str) -> AppResult<Session> {
    let url = format!(
        "{}/v1/auth/discord/exchange",
        api_base.trim_end_matches('/')
    );
    let resp = crate::net::CLIENT
        .post(&url)
        .timeout(Duration::from_secs(30))
        .json(&ExchangeRequest {
            code,
            code_verifier: verifier,
            redirect_uri: REDIRECT_URI,
        })
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(failed(format!("exchange answered {}", resp.status())));
    }
    let body: ExchangeResponse = resp
        .json()
        .await
        .map_err(|_| failed("exchange returned an unexpected body"))?;
    let session = Session {
        token: body.session_token,
        expires_at: body.expires_at,
        user: body.discord_user,
    };
    if !session.is_valid() {
        return Err(failed("exchange returned an expired session"));
    }
    Ok(session)
}

pub async fn login(paths: &Paths, cancel: &CancellationToken) -> AppResult<LoginOutcome> {
    let api_base = crate::config::api_base_url();
    if api_base.is_empty() {
        return Err(AppError::api_unavailable());
    }
    let Some(listener) = bind(cancel).await? else {
        return Ok(LoginOutcome::Cancelled);
    };
    let state = random_token()?;
    let verifier = random_token()?;
    tauri_plugin_opener::open_url(
        authorize_url(&state, &code_challenge(&verifier)),
        None::<&str>,
    )
    .map_err(|_| failed("could not open the browser"))?;

    let code = match wait_for_code(&listener, &state, cancel, LOGIN_TIMEOUT).await {
        Wait::Code(code) => code,
        Wait::Denied => return Ok(LoginOutcome::Denied),
        Wait::Cancelled => return Ok(LoginOutcome::Cancelled),
        Wait::TimedOut => return Ok(LoginOutcome::TimedOut),
    };
    drop(listener);
    let session = exchange(&api_base, &code, &verifier).await?;
    if !save_unless_cancelled(paths, &session, cancel).await? {
        return Ok(LoginOutcome::Cancelled);
    }
    log::info!("discord login ok");
    Ok(LoginOutcome::LoggedIn {
        session: session.info(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256() {
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mtLAhUH4ji1JnbRcBEERYT5DsrYOR1Os"),
            "4onkLltTfeXCZHlCoZFAGkS7uovoXNnk46SiLoDNQTY"
        );
        let verifier = random_token().unwrap();
        assert_eq!(verifier.len(), 43, "RFC 7636 asks for 43–128 characters");
        assert_ne!(verifier, random_token().unwrap());
    }

    #[test]
    fn authorize_url_has_pkce_and_state() {
        let url = reqwest::Url::parse(&authorize_url("st", "ch")).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.host_str(), Some("discord.com"));
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], REDIRECT_URI);
        assert_eq!(q["client_id"], DISCORD_CLIENT_ID);
    }

    #[test]
    fn parses_only_our_callback() {
        let req = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(
            parse_callback(&req("/callback?code=abc&state=st"), "st"),
            Callback::Code("abc".into())
        );
        assert_eq!(
            parse_callback(&req("/callback?error=access_denied&state=st"), "st"),
            Callback::Denied
        );
        assert_eq!(
            parse_callback(&req("/callback?code=abc&state=other"), "st"),
            Callback::Ignored,
            "a callback with another state is not ours"
        );
        assert_eq!(
            parse_callback(&req("/callback?code=abc"), "st"),
            Callback::Ignored
        );
        assert_eq!(
            parse_callback(&req("/favicon.ico"), "st"),
            Callback::Ignored
        );
        assert_eq!(
            parse_callback("POST /callback?code=a&state=st HTTP/1.1\r\n\r\n", "st"),
            Callback::Ignored
        );
        assert_eq!(parse_callback("garbage", "st"), Callback::Ignored);
    }

    async fn send(addr: std::net::SocketAddr, target: &str) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn silent_connection_does_not_delay_the_callback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let client = tokio::spawn(async move {
            let _silent = TcpStream::connect(addr).await.unwrap();
            let stray = send(addr, "/favicon.ico").await;
            let done = send(addr, "/callback?code=the_code&state=st").await;
            (stray, done)
        });
        let started = std::time::Instant::now();
        let wait = wait_for_code(&listener, "st", &cancel, LOGIN_TIMEOUT).await;
        assert_eq!(wait, Wait::Code("the_code".into()));
        assert!(started.elapsed() < Duration::from_secs(2), "not blocked");
        let (stray, done) = client.await.unwrap();
        assert!(stray.starts_with("HTTP/1.1 404"));
        assert!(done.starts_with("HTTP/1.1 200"));
    }

    #[tokio::test]
    async fn callback_waits_for_a_free_slot_instead_of_being_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let client = tokio::spawn(async move {
            let mut silent = Vec::new();
            for _ in 0..MAX_CONNECTIONS {
                silent.push(TcpStream::connect(addr).await.unwrap());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            let callback = tokio::spawn(send(addr, "/callback?code=c&state=st"));
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(silent.pop());
            callback.await.unwrap()
        });
        let wait = wait_for_code(&listener, "st", &cancel, LOGIN_TIMEOUT).await;
        assert_eq!(wait, Wait::Code("c".into()));
        assert!(client.await.unwrap().starts_with("HTTP/1.1 200"));
    }

    #[tokio::test]
    async fn cancel_is_immediate_even_with_a_silent_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let _silent = TcpStream::connect(addr).await.unwrap();
        let canceller = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            canceller.cancel();
        });
        let started = std::time::Instant::now();
        let wait = wait_for_code(&listener, "st", &cancel, LOGIN_TIMEOUT).await;
        assert_eq!(wait, Wait::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn denied_and_timed_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let client =
            tokio::spawn(async move { send(addr, "/callback?error=access_denied&state=st").await });
        let wait = wait_for_code(&listener, "st", &cancel, LOGIN_TIMEOUT).await;
        assert_eq!(wait, Wait::Denied);
        assert!(client.await.unwrap().starts_with("HTTP/1.1 200"));

        let wait = wait_for_code(&listener, "st", &cancel, Duration::from_millis(50)).await;
        assert_eq!(wait, Wait::TimedOut);
    }

    #[tokio::test]
    async fn bind_gives_up_on_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        if let Some(listener) = bind(&cancel).await.unwrap() {
            let busy = bind(&cancel).await.unwrap();
            assert!(busy.is_none(), "cancelled while the port is in use");
            drop(listener);
        }
    }

    fn session(expires_in: chrono::TimeDelta) -> Session {
        Session {
            token: "secret-token".into(),
            expires_at: chrono::Utc::now() + expires_in,
            user: DiscordUser {
                id: "1".into(),
                name: "tester".into(),
            },
        }
    }

    #[tokio::test]
    async fn session_file_round_trip_and_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let live = CancellationToken::new();
        assert!(current(&paths).await.is_none());

        assert!(
            save_unless_cancelled(&paths, &session(chrono::TimeDelta::days(7)), &live)
                .await
                .unwrap()
        );
        assert_eq!(current(&paths).await.unwrap().user.name, "tester");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(paths.session_file())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "only the user can read the token");
        }

        save_unless_cancelled(&paths, &session(chrono::TimeDelta::minutes(1)), &live)
            .await
            .unwrap();
        assert!(current(&paths).await.is_none(), "about to expire");
        assert!(
            !paths.session_file().exists(),
            "an expired token is removed"
        );

        std::fs::write(paths.session_file(), b"{not json").unwrap();
        assert!(current(&paths).await.is_none());
        assert!(!paths.session_file().exists(), "a damaged file is removed");

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let saved = save_unless_cancelled(&paths, &session(chrono::TimeDelta::days(7)), &cancelled)
            .await
            .unwrap();
        assert!(!saved, "a cancelled login is not saved");
        assert!(!paths.session_file().exists());

        save_unless_cancelled(&paths, &session(chrono::TimeDelta::days(7)), &live)
            .await
            .unwrap();
        logout(&paths).await.unwrap();
        assert!(!paths.session_file().exists());
        logout(&paths).await.unwrap();
    }

    #[test]
    fn token_never_reaches_the_ui_or_the_log() {
        let s = session(chrono::TimeDelta::days(1));
        let ui = serde_json::to_string(&LoginOutcome::LoggedIn { session: s.info() }).unwrap();
        assert!(!ui.contains("secret-token"), "{ui}");
        assert!(ui.contains(r#""status":"loggedIn""#), "{ui}");
        let debug = format!("{s:?}");
        assert!(!debug.contains("secret-token"), "{debug}");
    }
}
