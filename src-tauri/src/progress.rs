use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const MIN_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    Checking,
    Java,
    Pack,
    Minecraft,
    Mods,
    Integrity,
    Launching,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload<'a> {
    event_id: &'a str,
    step: Step,
    percent: Option<f32>,
    message: Option<String>,
}

#[derive(Clone)]
pub struct Reporter {
    app: Option<AppHandle>,
    event_id: String,
    last: Arc<Mutex<Option<(Step, Instant)>>>,
}

impl Reporter {
    pub fn new(app: AppHandle, event_id: &str) -> Self {
        Self {
            app: Some(app),
            event_id: event_id.to_string(),
            last: Arc::default(),
        }
    }

    #[cfg(test)]
    pub fn silent() -> Self {
        Self {
            app: None,
            event_id: String::new(),
            last: Arc::default(),
        }
    }

    fn should_emit(&self, step: Step, percent: Option<f32>) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let partial = percent.is_some_and(|p| p < 100.0);
        if partial
            && let Some((last_step, at)) = *last
            && last_step == step
            && now.duration_since(at) < MIN_INTERVAL
        {
            return false;
        }
        *last = Some((step, now));
        true
    }

    pub fn emit(&self, step: Step, percent: Option<f32>, message: Option<String>) {
        if !self.should_emit(step, percent) {
            return;
        }
        if let Some(app) = &self.app {
            let _ = app.emit(
                "launch://progress",
                ProgressPayload {
                    event_id: &self.event_id,
                    step,
                    percent: percent.map(|p| p.clamp(0.0, 100.0)),
                    message,
                },
            );
        }
    }

    pub fn step(&self, step: Step) {
        self.emit(step, None, None);
    }

    pub fn fraction(&self, step: Step, done: u64, total: u64) {
        let percent = if total == 0 {
            None
        } else {
            Some(done as f32 * 100.0 / total as f32)
        };
        self.emit(step, percent, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttles_partial_progress_only() {
        let r = Reporter::silent();
        assert!(r.should_emit(Step::Mods, Some(1.0)));
        assert!(!r.should_emit(Step::Mods, Some(2.0)), "too soon");
        assert!(
            r.should_emit(Step::Mods, Some(100.0)),
            "completion always shown"
        );
        assert!(
            r.should_emit(Step::Java, Some(3.0)),
            "a new step always shown"
        );
        assert!(r.should_emit(Step::Java, None), "step changes always shown");
        std::thread::sleep(MIN_INTERVAL);
        assert!(r.should_emit(Step::Java, Some(4.0)));
    }
}
