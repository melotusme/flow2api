//! Private release updates. The GitHub token stays in this process and is only
//! sent to asset IDs under the fixed releases repository.

use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const REPO: &str = "repos/melotusme/flow2api-releases";
const API: &str = "https://api.github.com/";
const LIMIT: usize = 256 * 1024 * 1024;

#[derive(Default)]
pub struct State {
    pending: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    status: std::sync::Mutex<UpdateStatus>,
}

#[derive(Clone, Default, Serialize)]
pub struct UpdateStatus {
    phase: String,
    message: String,
    checked_at: Option<u64>,
}

fn report(app: &tauri::AppHandle, phase: &str, message: impl Into<String>) {
    let state = app.state::<State>();
    let mut status = state.status.lock().unwrap();
    status.phase = phase.into();
    status.message = message.into();
    if phase == "available" || phase == "current" {
        status.checked_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
    }
    let _ = app.emit("private-update-status", status.clone());
}

#[tauri::command]
pub fn private_update_status(state: tauri::State<'_, State>) -> UpdateStatus {
    state.status.lock().unwrap().clone()
}

#[derive(Serialize)]
pub struct Available {
    version: String,
    current_version: String,
    body: Option<String>,
}

fn hidden(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut command = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn gh_program() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let program_files = std::env::var_os("ProgramFiles")
            .map(std::path::PathBuf::from)
            .map(|path| path.join("GitHub CLI/gh.exe"))
            .filter(|path| path.is_file());
        return Ok(program_files
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "gh.exe".into()));
    }
    #[cfg(not(target_os = "windows"))]
    {
        ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
            .into_iter()
            .find(|path| std::path::Path::new(path).is_file())
            .map(str::to_string)
            .ok_or_else(|| "私有更新需要 GitHub CLI：请安装 gh 并执行 gh auth login。".into())
    }
}

async fn token() -> Result<String, String> {
    let program = gh_program()?;
    tauri::async_runtime::spawn_blocking(move || {
        let output = hidden(&program)
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            .map_err(|_| "无法读取本机 GitHub 登录，请执行 gh auth login。")?;
        if !output.status.success() {
            return Err(
                "GitHub 尚未登录，请执行 gh auth login，并确认账号可读取更新仓库。".to_string(),
            );
        }
        let token = String::from_utf8(output.stdout)
            .map_err(|_| "GitHub 凭据格式异常。")?
            .trim()
            .to_owned();
        if token.is_empty() {
            return Err("GitHub 登录凭据为空，请重新登录。".into());
        }
        Ok(token)
    })
    .await
    .map_err(|_| "读取 GitHub 登录状态失败。".to_string())?
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Flow2API-private-updater")
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "无法初始化私有更新连接。".into())
}

pub(crate) fn asset_name(platform: &str) -> Result<&'static str, String> {
    match platform {
        "windows-x86_64" => Ok("Flow2API.windows.x64.exe"),
        "linux-x86_64" => Ok("Flow2API.linux.x64.AppImage"),
        "darwin-aarch64" | "darwin-x86_64" => Ok("Flow2API.app.tar.gz"),
        _ => Err(format!("私有更新清单包含不支持的平台 {platform}。")),
    }
}

fn asset(release: &Value, name: &str) -> Result<String, String> {
    let prefix = format!("{API}{REPO}/releases/assets/");
    release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == name))
        .and_then(|asset| asset["url"].as_str())
        .filter(|url| {
            url.strip_prefix(&prefix)
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .map(str::to_string)
        .ok_or_else(|| format!("私有 Release 缺少有效的 {name}，请检查发布是否完整。"))
}

async fn get(
    client: &reqwest::Client,
    token: &str,
    url: &str,
    binary: bool,
) -> Result<reqwest::Response, String> {
    if !valid_api_url(url) {
        return Err("拒绝向更新仓库以外的地址发送 GitHub 凭据。".into());
    }
    let mut response = client
        .get(url)
        .bearer_auth(token)
        .header(
            reqwest::header::ACCEPT,
            if binary {
                "application/octet-stream"
            } else {
                "application/vnd.github+json"
            },
        )
        .send()
        .await
        .map_err(|_| "GitHub 更新连接失败或超时，请检查网络。")?;
    if binary {
        for _ in 0..3 {
            if !response.status().is_redirection() {
                break;
            }
            let target = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| reqwest::Url::parse(value).ok())
                .filter(valid_cdn_url)
                .ok_or("GitHub 返回了不受信任的更新下载地址。")?;
            // Never forward Authorization to the signed CDN URL.
            response = client
                .get(target)
                .send()
                .await
                .map_err(|_| "更新包下载连接失败或超时。")?;
        }
    }
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 => Err("GitHub 登录已失效，请执行 gh auth login。".into()),
        403 | 404 => {
            Err("无法读取私有更新：请确认 GitHub 账号拥有仓库权限，且已有正式 Release。".into())
        }
        code => Err(format!("GitHub 更新请求失败（HTTP {code}），请稍后重试。")),
    }
}

fn valid_api_url(url: &str) -> bool {
    let prefix = format!("{API}{REPO}/releases/");
    url == format!("{prefix}latest")
        || url
            .strip_prefix(&format!("{prefix}assets/"))
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

fn valid_cdn_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "github-releases.githubusercontent.com"
            )
        )
}

#[tauri::command]
pub async fn check_private_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<Option<Available>, String> {
    let result = check(&app, &state).await;
    if let Err(error) = &result {
        report(&app, "error", error);
    }
    result
}

async fn check(app: &tauri::AppHandle, state: &State) -> Result<Option<Available>, String> {
    let mut pending = state.pending.lock().await;
    report(app, "checking", "正在检查私有更新…");
    let token = token().await?;
    let client = http()?;
    let release: Value = get(
        &client,
        &token,
        &format!("{API}{REPO}/releases/latest"),
        false,
    )
    .await?
    .json()
    .await
    .map_err(|_| "GitHub Release 元数据格式异常。")?;
    let mut manifest: Value = get(&client, &token, &asset(&release, "latest.json")?, true)
        .await?
        .json()
        .await
        .map_err(|_| "私有更新清单格式异常。")?;
    if release["draft"] == true
        || release["prerelease"] == true
        || release["tag_name"].as_str()
            != manifest["version"]
                .as_str()
                .map(|v| format!("v{v}"))
                .as_deref()
    {
        return Err("正式 Release 与更新清单版本不一致。".into());
    }
    if let Some(platforms) = manifest["platforms"].as_object_mut() {
        for (key, platform) in platforms.iter_mut() {
            platform["url"] = Value::String(asset(&release, asset_name(key)?)?);
        }
    }
    let update = app
        .updater_builder()
        .restart_after_install(true)
        .build()
        .map_err(|_| "更新器配置异常。")?
        .restore_update(manifest)
        .map_err(|_| "私有更新清单无效或不支持当前架构。")?;
    let available = update.as_ref().map(|update| Available {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        body: update.body.clone(),
    });
    *pending = update;
    if let Some(found) = &available {
        report(
            app,
            "available",
            format!("发现新版本 v{}，将在任务空闲时安装。", found.version),
        );
    } else {
        report(app, "current", "已检查，当前是最新版本。");
    }
    Ok(available)
}

#[tauri::command]
pub async fn install_private_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let result = install(&app, &state, false).await;
    if let Err(error) = &result {
        report(&app, "error", error);
    }
    result
}

async fn install(app: &tauri::AppHandle, state: &State, automatic: bool) -> Result<(), String> {
    let mut pending = state.pending.lock().await;
    let update = pending.as_ref().ok_or("请先检查新版本。")?.clone();
    let desktop = app.state::<super::Desktop>();
    if desktop.core.status().busy() {
        return Err("仍有执行或排队任务，更新将在任务完成后重试。".into());
    }
    report(app, "downloading", format!("正在下载 v{}…", update.version));
    let token = token().await?;
    let client = http()?;
    let mut response = get(&client, &token, update.download_url.as_str(), true).await?;
    let total = response.content_length();
    if total.is_some_and(|size| size > LIMIT as u64) {
        return Err("更新包超过 256 MiB 限制。".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "更新包下载中断，请重试。")?
    {
        if bytes.len() + chunk.len() > LIMIT {
            return Err("更新包超过 256 MiB 限制。".into());
        }
        bytes.extend_from_slice(&chunk);
        let _ = app.emit(
            "private-update-progress",
            serde_json::json!({"received": bytes.len(), "total": total}),
        );
    }
    update
        .verify(&bytes)
        .map_err(|_| "更新包签名校验失败，当前应用继续运行。")?;
    if automatic
        && !desktop
            .core
            .store
            .settings()
            .map_err(|e| e.to_string())?
            .auto_update
    {
        return Err("本机自动更新已关闭，取消自动安装。".into());
    }
    let was_running = desktop.core.status().service == "running";
    // shutdown holds task admission and rejects queued/running work atomically.
    desktop.core.shutdown().await.map_err(|e| e.to_string())?;
    desktop.quitting.store(true, Ordering::SeqCst);
    report(app, "installing", "签名验证通过，正在安装并重启…");
    let _ = app.emit("private-update-installing", ());
    let installed = tauri::async_runtime::spawn_blocking(move || update.install(&bytes)).await;
    if !matches!(installed, Ok(Ok(()))) {
        desktop.quitting.store(false, Ordering::SeqCst);
        desktop
            .core
            .resume_after_failed_update(was_running)
            .await
            .map_err(|_| "更新安装失败，API 恢复失败；请重新打开应用。")?;
        return Err("更新安装失败，已恢复当前版本运行，请重试。".into());
    }
    *pending = None;
    app.restart();
}

/// Native scheduler also runs when the window is hidden in the tray.
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut next_check = tokio::time::Instant::now();
        loop {
            let desktop = app.state::<super::Desktop>();
            let enabled = desktop
                .core
                .store
                .settings()
                .map(|s| s.auto_update)
                .unwrap_or(false);
            if enabled
                && !desktop.quit_in_progress.load(Ordering::SeqCst)
                && !desktop.quitting.load(Ordering::SeqCst)
            {
                let state = app.state::<State>();
                if tokio::time::Instant::now() >= next_check {
                    let result = check(&app, &state).await;
                    next_check = tokio::time::Instant::now()
                        + Duration::from_secs(if result.is_ok() { 6 * 3600 } else { 1800 });
                    if let Err(error) = result {
                        report(&app, "error", error);
                    }
                }
                let available = state.pending.lock().await.is_some();
                if available {
                    if desktop.core.status().busy() {
                        report(&app, "waiting", "有生成任务，完成后自动安装更新。");
                    } else if let Err(error) = install(&app, &state, true).await {
                        report(&app, "error", error);
                        tokio::time::sleep(Duration::from_secs(30 * 60)).await;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_only_follow_fixed_repository_asset_ids() {
        let allowed = format!("{API}{REPO}/releases/assets/123");
        let release = serde_json::json!({"assets":[{"name":"latest.json","url":allowed}]});
        assert!(asset(&release, "latest.json").is_ok());
        for url in [
            "https://evil.example/123",
            "https://api.github.com/repos/other/private/releases/assets/123",
            "https://api.github.com/repos/melotusme/flow2api-releases/releases/assets/123?token=bad",
            "https://api.github.com/repos/melotusme/flow2api-releases/releases/assets/",
        ] {
            let release = serde_json::json!({"assets":[{"name":"latest.json","url":url}]});
            assert!(asset(&release, "latest.json").is_err());
        }
    }

    #[test]
    fn credentials_and_redirects_are_restricted() {
        assert!(valid_api_url(&format!("{API}{REPO}/releases/latest")));
        for url in [
            "https://api.github.com/repos/other/repo/releases/latest",
            "https://api.github.com/repos/melotusme/flow2api-releases/releases/assets/12?x=1",
        ] {
            assert!(!valid_api_url(url));
        }
        for url in [
            "https://release-assets.githubusercontent.com/a?sig=abc",
            "https://objects.githubusercontent.com/a",
        ] {
            assert!(valid_cdn_url(&reqwest::Url::parse(url).unwrap()));
        }
        for url in [
            "http://release-assets.githubusercontent.com/a",
            "https://release-assets.githubusercontent.com.evil.test/a",
            "https://user@objects.githubusercontent.com/a",
            "https://objects.githubusercontent.com:8443/a",
            "https://github.com/other/a",
        ] {
            assert!(!valid_cdn_url(&reqwest::Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn platform_assets_do_not_cross() {
        assert_eq!(
            asset_name("windows-x86_64").unwrap(),
            "Flow2API.windows.x64.exe"
        );
        assert_eq!(asset_name("darwin-aarch64").unwrap(), "Flow2API.app.tar.gz");
        assert_eq!(asset_name("darwin-x86_64").unwrap(), "Flow2API.app.tar.gz");
        assert!(asset_name("windows-aarch64").is_err());
    }
}
