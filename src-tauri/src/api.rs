use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::events::is_valid_event_id;

const MAX_ANSWER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    LoginRequired,
    NotWhitelisted,
    Ended,
    Denied,
}

#[derive(Deserialize)]
struct AccessResponse {
    allowed: bool,
    #[serde(default)]
    reason: String,
}

fn interpret(status: StatusCode, body: &[u8]) -> Option<Access> {
    if status == StatusCode::UNAUTHORIZED {
        return Some(Access::LoginRequired);
    }
    if status != StatusCode::OK && status != StatusCode::FORBIDDEN {
        return None;
    }
    let Ok(response) = serde_json::from_slice::<AccessResponse>(body) else {
        return (status == StatusCode::FORBIDDEN).then_some(Access::Denied);
    };
    if response.allowed {
        return Some(if status == StatusCode::OK {
            Access::Allowed
        } else {
            Access::Denied
        });
    }
    Some(match response.reason.as_str() {
        "login_required" => Access::LoginRequired,
        "whitelist" => Access::NotWhitelisted,
        "ended" => Access::Ended,
        _ => Access::Denied,
    })
}

fn check_failed(message: impl Into<String>) -> AppError {
    AppError::new("access_check_failed", message)
}

/// Si la API no da respuesta válida, se cuenta como error
pub async fn event_access(event_id: &str, bearer: Option<&str>) -> AppResult<Access> {
    let base = crate::config::api_base_url();
    if !base.starts_with("https://") {
        return Err(AppError::api_unavailable());
    }
    if !is_valid_event_id(event_id) {
        return Err(AppError::new("invalid_event", "invalid event id"));
    }
    let url = format!("{}/v1/events/{event_id}/access", base.trim_end_matches('/'));
    let mut request = crate::net::CLIENT
        .get(&url)
        .timeout(Duration::from_secs(15));
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let mut resp = request.send().await?;
    let status = resp.status();
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return Err(check_failed("the access answer is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    interpret(status, &body)
        .ok_or_else(|| check_failed(format!("unusable access answer: {status}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_access_answers() {
        let ok = StatusCode::OK;
        let forbidden = StatusCode::FORBIDDEN;
        let body = |allowed: bool, reason: &str| {
            format!(r#"{{"allowed":{allowed},"reason":"{reason}"}}"#).into_bytes()
        };
        assert_eq!(interpret(ok, &body(true, "ok")), Some(Access::Allowed));
        assert_eq!(
            interpret(ok, &body(false, "login_required")),
            Some(Access::LoginRequired)
        );
        assert_eq!(
            interpret(forbidden, &body(false, "whitelist")),
            Some(Access::NotWhitelisted)
        );
        assert_eq!(interpret(ok, &body(false, "ended")), Some(Access::Ended));
        assert_eq!(
            interpret(ok, &body(false, "something_new")),
            Some(Access::Denied)
        );
        assert_eq!(
            interpret(StatusCode::UNAUTHORIZED, b""),
            Some(Access::LoginRequired)
        );
        assert_eq!(
            interpret(forbidden, &body(true, "ok")),
            Some(Access::Denied),
            "a 403 never means allowed"
        );
        assert_eq!(
            interpret(forbidden, b"Forbidden"),
            Some(Access::Denied),
            "a 403 without JSON still denies"
        );
        assert_eq!(interpret(StatusCode::TOO_MANY_REQUESTS, b""), None);
        assert_eq!(interpret(StatusCode::INTERNAL_SERVER_ERROR, b""), None);
        assert_eq!(interpret(ok, b"<html>"), None);
    }
}
