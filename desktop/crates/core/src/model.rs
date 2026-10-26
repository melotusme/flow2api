use chrono::Utc;
use serde::{Deserialize, Serialize};

pub fn now() -> String {
    Utc::now().to_rfc3339()
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
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub enabled: bool,
    pub project_id: String,
    pub last_synced: Option<String>,
    pub credits: Option<i64>,
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
}
