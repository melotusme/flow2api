use chrono::Utc;
use serde::{Deserialize, Serialize};

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn after_minutes(minutes: i64) -> String {
    (Utc::now() + chrono::Duration::minutes(minutes)).to_rfc3339()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub port: u16,
    pub api_key: String,
    pub browser_executable: String,
    pub max_concurrent: usize,
    pub image_timeout_seconds: u64,
    pub video_timeout_seconds: u64,
    pub proxy_url: String,
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 8000,
            api_key: uuid::Uuid::new_v4().to_string(),
            browser_executable: String::new(),
            max_concurrent: 1,
            image_timeout_seconds: 300,
            video_timeout_seconds: 1500,
            proxy_url: String::new(),
            auto_update: true,
        }
    }
}

pub fn default_true() -> bool {
    true
}

#[derive(Clone, Debug)]
pub struct SyncedAccount {
    pub id: String,
    pub email: String,
    pub enabled: bool,
    pub project_id: String,
    pub priority: i32,
    pub note: String,
    pub image_enabled: bool,
    pub video_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub enabled: bool,
    pub project_id: String,
    pub last_synced: Option<String>,
    pub credits: Option<i64>,
    #[serde(default)]
    pub browser_data_dir: String,
    #[serde(default)]
    pub browser_profile: String,
    #[serde(default)]
    pub browser_bridge: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub disabled_until: Option<String>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_true")]
    pub image_enabled: bool,
    #[serde(default = "default_true")]
    pub video_enabled: bool,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub consecutive_failures: u32,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            id: String::new(),
            email: String::new(),
            enabled: true,
            project_id: String::new(),
            last_synced: None,
            credits: None,
            browser_data_dir: String::new(),
            browser_profile: String::new(),
            browser_bridge: false,
            priority: 0,
            disabled_until: None,
            disabled_reason: None,
            note: String::new(),
            image_enabled: true,
            video_enabled: true,
            last_error: None,
            consecutive_failures: 0,
        }
    }
}

impl Account {
    pub fn temporarily_disabled_at(&self, now_rfc3339: &str) -> bool {
        self.disabled_until
            .as_deref()
            .is_some_and(|until| !until.is_empty() && until > now_rfc3339)
    }
    pub fn temporarily_disabled(&self) -> bool {
        self.temporarily_disabled_at(&now())
    }
    pub fn available(&self) -> bool {
        self.enabled && !self.temporarily_disabled()
    }
    pub fn supports_kind(&self, is_video: bool) -> bool {
        if is_video {
            self.video_enabled
        } else {
            self.image_enabled
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub images: Vec<String>,
    #[serde(default)]
    pub video_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub request: GenerationRequest,
    pub state: String,
    pub stage: String,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
    pub results: Vec<Media>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Media {
    pub filename: String,
    pub kind: String,
    pub size: u64,
    pub created_at: String,
    pub task_id: String,
    pub media_id: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Status {
    pub service: String,
    pub address: Option<String>,
    pub queued: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub accounts: usize,
    pub total_accounts: usize,
    pub ready_accounts: usize,
    pub browser: String,
    pub last_error: Option<String>,
    pub data_dir: String,
}

impl Status {
    pub fn tray_summary(&self) -> String {
        let service = match self.service.as_str() {
            "running" => "API 运行中",
            "starting" => "API 启动中",
            "failed" => "API 异常",
            "stopping" => "API 停止中",
            _ => "API 已停止",
        };
        format!("{service} · 执行 {} · 排队 {}", self.running, self.queued)
    }
    pub fn busy(&self) -> bool {
        self.running + self.queued > 0
    }

    /// Text beside the macOS menu-bar icon. Windows uses the tray icon color instead.
    pub fn menu_bar_label(&self) -> String {
        let count = self.running + self.queued;
        if self.service == "failed" {
            "Flow !".into()
        } else if count > 0 {
            format!("Flow {count}")
        } else if self.service == "running" {
            "Flow".into()
        } else if self.service == "starting" || self.service == "stopping" {
            "Flow …".into()
        } else {
            "Flow ·".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(service: &str, running: usize, queued: usize) -> Status {
        Status {
            service: service.into(),
            running,
            queued,
            ..Status::default()
        }
    }

    #[test]
    fn menu_bar_label_names_the_resident_app() {
        assert_eq!(status("running", 0, 0).menu_bar_label(), "Flow");
        assert_eq!(status("running", 1, 2).menu_bar_label(), "Flow 3");
        assert_eq!(status("failed", 0, 0).menu_bar_label(), "Flow !");
        assert_eq!(status("starting", 0, 0).menu_bar_label(), "Flow …");
        assert_eq!(status("stopped", 0, 0).menu_bar_label(), "Flow ·");
    }
}
