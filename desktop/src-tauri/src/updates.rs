//! Private release updates. The GitHub token stays in this process and is only
//! sent to asset IDs under the fixed releases repository.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tauri::Emitter;
use tauri_plugin_updater::UpdaterExt;

const REPO: &str = "repos/melotusme/flow2api-releases";
const API: &str = "https://api.github.com/";
const LIMIT: usize = 256 * 1024 * 1024;

#[derive(Default)]
pub struct State(tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>);

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
    let response = client
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
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 => Err("GitHub 登录已失效，请执行 gh auth login。".into()),
        403 | 404 => {
            Err("无法读取私有更新：请确认 GitHub 账号拥有仓库权限，且已有正式 Release。".into())
        }
        code => Err(format!("GitHub 更新请求失败（HTTP {code}），请稍后重试。")),
    }
}

#[tauri::command]
pub async fn check_private_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<Option<Available>, String> {
    let mut pending = state.0.lock().await;
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
    if let Some(platforms) = manifest["platforms"].as_object_mut() {
        for (key, platform) in platforms.iter_mut() {
            platform["url"] = Value::String(asset(&release, asset_name(key)?)?);
        }
    }
    let update = app
        .updater_builder()
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
    Ok(available)
}

#[tauri::command]
pub async fn install_private_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let mut pending = state.0.lock().await;
    let update = pending.as_ref().ok_or("请先检查新版本。")?;
    let token = token().await?;
    let client = http()?;
    let mut response = get(&client, &token, update.download_url.as_str(), true).await?;
    let total = response.content_length();
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
        let _ = app.emit_to(
            "main",
            "private-update-progress",
            serde_json::json!({"received": bytes.len(), "total": total}),
        );
    }
    let _ = app.emit_to("main", "private-update-installing", ());
    update
        .install(&bytes)
        .map_err(|_| "更新包验签或安装失败；当前版本仍保留，请重试。")?;
    *pending = None;
    Ok(())
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
