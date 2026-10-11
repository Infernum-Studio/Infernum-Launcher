use serde::Serialize;
use tauri::{AppHandle, Emitter};

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
}

impl Reporter {
    pub fn new(app: AppHandle, event_id: &str) -> Self {
        Self {
            app: Some(app),
            event_id: event_id.to_string(),
        }
    }

    #[cfg(test)]
    pub fn silent() -> Self {
        Self {
            app: None,
            event_id: String::new(),
        }
    }

    pub fn emit(&self, step: Step, percent: Option<f32>, message: Option<String>) {
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
