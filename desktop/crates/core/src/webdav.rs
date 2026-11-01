//! Manual WebDAV sync for preferences that travel between computers.
//! Passwords stay in the OS credential store. The JSON payload is not end-to-end encrypted.

use crate::{model::SyncedAccount, store::Store};
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::{collections::HashMap, sync::Mutex};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const FILE: &str = "flow2api-v1.json";
const LIMIT: usize = 8 * 1024 * 1024;
const SERVICE: &str = "Flow2API WebDAV";

#[derive(Clone, Serialize)]
pub struct PublicConfig {
    pub url: String,
    pub username: String,
    pub has_password: bool,
    pub last_sync: String,
    pub last_result: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Config {
    url: String,
    username: String,
    etag: Option<String>,
    remote_hash: Option<String>,
    last_sync: String,
    last_result: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Portable {
    schema: u8,
    max_concurrent: usize,
    image_timeout_seconds: u64,
    video_timeout_seconds: u64,
    accounts: Vec<PortableAccount>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableAccount {
    id: String,
    email: String,
    enabled: bool,
    project_id: String,
}

trait Secrets: Send + Sync {
    fn get(&self, account: &str) -> Result<String, String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
}

struct Keyring;
impl Secrets for Keyring {
    fn get(&self, account: &str) -> Result<String, String> {
        keyring::Entry::new(SERVICE, account)
            .map_err(|_| "系统凭据库不可用".to_string())?
            .get_password()
            .map_err(|_| "未找到 WebDAV 应用密码，请重新填写或从 Clash Verge 导入".to_string())
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, account)
            .map_err(|_| "系统凭据库不可用".to_string())?
            .set_password(secret)
            .map_err(|_| "无法保存到系统凭据库".to_string())
    }
}

#[cfg(test)]
#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<String, String>>);
#[cfg(test)]
impl Secrets for MemorySecrets {
    fn get(&self, account: &str) -> Result<String, String> {
        self.0
            .lock()
            .unwrap()
            .get(account)
            .cloned()
            .ok_or_else(|| "未找到 WebDAV 应用密码，请重新填写或从 Clash Verge 导入".to_string())
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }
}

pub fn public_config(root: &Path) -> Result<PublicConfig, String> {
    view(root, &Keyring)
}
pub fn save_connection(
    root: &Path,
    url: String,
    username: String,
    secret: String,
) -> Result<PublicConfig, String> {
    configure(root, &Keyring, url, username, secret, false)?;
    view(root, &Keyring)
}
pub fn import_verge(root: &Path) -> Result<PublicConfig, String> {
    let directory = verge_directory()?;
    let yaml: serde_yaml::Value = serde_yaml::from_slice(
        &std::fs::read(directory.join("verge.yaml"))
            .map_err(|_| "无法读取 Clash Verge verge.yaml")?,
    )
    .map_err(|_| "Clash Verge verge.yaml 格式无效")?;
    let key = std::fs::read(directory.join(".encryption_key"))
        .map_err(|_| "无法读取 Clash Verge 本机加密密钥")?;
    let read = |name: &str| {
        decrypt(
            &key,
            yaml[name].as_str().ok_or("Clash Verge 尚未配置 WebDAV")?,
        )
    };
    configure(
        root,
        &Keyring,
        read("webdav_url")?,
        read("webdav_username")?,
        read("webdav_password")?,
        false,
    )?;
    view(root, &Keyring)
}
pub async fn perform(store: &Store, action: &str) -> Result<String, String> {
    run(store, action, &Keyring, false).await
}

fn view(root: &Path, secrets: &dyn Secrets) -> Result<PublicConfig, String> {
    let config = load(root)?;
    Ok(PublicConfig {
        has_password: !config.username.is_empty() && secrets.get(&secret_id(&config)).is_ok(),
        url: config.url,
        username: config.username,
        last_sync: config.last_sync,
        last_result: config.last_result,
    })
}
fn load(root: &Path) -> Result<Config, String> {
    match std::fs::read(root.join("webdav.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "本机 WebDAV 配置损坏".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config {
            url: "https://dav.jianguoyun.com/dav/".into(),
            ..Config::default()
        }),
        Err(_) => Err("无法读取 WebDAV 配置".into()),
    }
}
fn secret_id(config: &Config) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{}|{}", config.url, config.username))
    )
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "无法创建本机配置目录")?;
    }
    let name = path
        .file_name()
        .ok_or("无法保存本机配置")?
        .to_string_lossy();
    let temporary = path.with_file_name(format!("{name}.tmp"));
    std::fs::write(&temporary, bytes).map_err(|_| "无法写入临时配置")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "无法限制本机配置权限")?;
    }
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path).map_err(|_| "无法替换本机配置")?;
    }
    std::fs::rename(&temporary, path).map_err(|_| "无法保存本机配置")?;
    Ok(())
}
fn store_config(root: &Path, config: &Config) -> Result<(), String> {
    atomic_write(
        &root.join("webdav.json"),
        &serde_json::to_vec_pretty(config).map_err(|_| "无法序列化 WebDAV 配置")?,
    )
}
fn base_url(config: &Config, allow_loopback: bool) -> Result<url::Url, String> {
    let mut url = url::Url::parse(config.url.trim()).map_err(|_| "WebDAV 地址格式无效")?;
    let host = url.host_str().unwrap_or("");
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "[::1]");
    let allowed = url.scheme() == "https" || (allow_loopback && url.scheme() == "http" && loopback);
    if !allowed
        || host.is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || config.username.trim().is_empty()
    {
        return Err("请填写无内嵌认证的 HTTPS WebDAV 目录和用户名".into());
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
fn collection(config: &Config, allow_loopback: bool) -> Result<url::Url, String> {
    let mut url = base_url(config, allow_loopback)?
        .join("Flow2API/")
        .map_err(|_| "WebDAV 文件地址无效")?;
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
fn remote_file(config: &Config, name: &str, allow_loopback: bool) -> Result<url::Url, String> {
    collection(config, allow_loopback)?
        .join(name)
        .map_err(|_| "WebDAV 文件地址无效".into())
}
fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent("Flow2API-WebDAV/1")
        .build()
        .map_err(|_| "无法创建 WebDAV 连接".into())
}
fn configure(
    root: &Path,
    secrets: &dyn Secrets,
    url: String,
    username: String,
    secret: String,
    allow_loopback: bool,
) -> Result<Config, String> {
    let old = load(root)?;
    let same = old.url == url && old.username == username;
    let config = Config {
        url,
        username,
        etag: if same { old.etag } else { None },
        remote_hash: if same { old.remote_hash } else { None },
        last_sync: if same { old.last_sync } else { String::new() },
        last_result: if same { old.last_result } else { String::new() },
    };
    base_url(&config, allow_loopback)?;
    if !secret.is_empty() {
        secrets.set(&secret_id(&config), &secret)?;
    }
    if secrets.get(&secret_id(&config)).is_err() {
        return Err("请填写 WebDAV 应用密码".into());
    }
    store_config(root, &config)?;
    Ok(config)
}
fn decrypt(key: &[u8], value: &str) -> Result<String, String> {
    let data = STANDARD
        .decode(value.trim())
        .map_err(|_| "Clash Verge WebDAV 密文格式无效")?;
    if key.len() != 32 || data.len() < 28 {
        return Err("Clash Verge WebDAV 密文过短".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Clash Verge 本机加密密钥无效")?;
    let plain = cipher
        .decrypt(aes_gcm::Nonce::from_slice(&data[..12]), &data[12..])
        .map_err(|_| "无法解密 Clash Verge WebDAV，请在本机手动填写")?;
    serde_json::from_slice(&plain).map_err(|_| "Clash Verge WebDAV 字段格式无效".into())
}
fn verge_directory() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let root = std::env::var_os("HOME")
        .map(|path| PathBuf::from(path).join("Library/Application Support"));
    root.map(|path| path.join("io.github.clash-verge-rev.clash-verge-rev"))
        .filter(|path| path.is_dir())
        .ok_or_else(|| "未找到 Clash Verge Rev 配置目录".into())
}
fn portable_from(store: &Store) -> Result<Portable, String> {
    let settings = store
        .settings()
        .map_err(|_| "无法读取本机设置".to_string())?;
    let accounts = store
        .accounts()
        .map_err(|_| "无法读取本机账号".to_string())?
        .into_iter()
        .map(|account| PortableAccount {
            id: account.id,
            email: account.email,
            enabled: account.enabled,
            project_id: account.project_id,
        })
        .collect();
    let portable = Portable {
        schema: 1,
        max_concurrent: settings.max_concurrent,
        image_timeout_seconds: settings.image_timeout_seconds,
        video_timeout_seconds: settings.video_timeout_seconds,
        accounts,
    };
    portable.validate()?;
    Ok(portable)
}
impl Portable {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || !(1..=8).contains(&self.max_concurrent)
            || self.image_timeout_seconds < 30
            || self.video_timeout_seconds < 30
            || self.accounts.len() > 1000
        {
            return Err("同步配置版本或范围无效".into());
        }
        let mut seen = std::collections::HashSet::new();
        for account in &self.accounts {
            if account.id.is_empty()
                || !seen.insert(account.id.clone())
                || !account.email.contains('@')
                || account.project_id.chars().count() > 200
            {
                return Err("同步账号记录无效".into());
            }
        }
        Ok(())
    }
    fn synced(&self) -> Vec<SyncedAccount> {
        self.accounts
            .iter()
            .map(|account| SyncedAccount {
                id: account.id.clone(),
                email: account.email.clone(),
                enabled: account.enabled,
                project_id: account.project_id.clone(),
            })
            .collect()
    }
}
async fn read_remote(
    config: &Config,
    secret: &str,
    name: &str,
    allow_loopback: bool,
) -> Result<(Option<Vec<u8>>, Option<String>), String> {
    let response = http()?
        .get(remote_file(config, name, allow_loopback)?)
        .basic_auth(&config.username, Some(secret))
        .send()
        .await
        .map_err(|_| "WebDAV 连接失败或超时")?;
    if response.status() == 404 {
        return Ok((None, None));
    }
    if response.status() != 200 {
        return Err(format!(
            "WebDAV 下载失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(|_| "WebDAV 下载中断")? {
        if bytes.len() + chunk.len() > LIMIT {
            return Err("云端配置超过 8 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((Some(bytes), etag))
}
async fn write_remote(
    config: &Config,
    secret: &str,
    bytes: &[u8],
    name: &str,
    allow_loopback: bool,
) -> Result<Option<String>, String> {
    if bytes.len() > LIMIT {
        return Err("本机同步配置超过 8 MiB".into());
    }
    let created = http()?
        .request(
            reqwest::Method::from_bytes(b"MKCOL").unwrap(),
            collection(config, allow_loopback)?,
        )
        .basic_auth(&config.username, Some(secret))
        .send()
        .await
        .map_err(|_| "无法创建 WebDAV 同步目录")?;
    if !matches!(created.status().as_u16(), 201 | 405) {
        return Err(format!(
            "WebDAV 创建目录失败（HTTP {}）",
            created.status().as_u16()
        ));
    }
    let (current, current_etag) = read_remote(config, secret, name, allow_loopback).await?;
    if let Some(current) = current {
        let hash = format!("{:x}", Sha256::digest(&current));
        let known = config
            .remote_hash
            .as_ref()
            .map(|value| value == &hash)
            .unwrap_or_else(|| config.etag.is_some() && config.etag == current_etag);
        if !known {
            return Err("云端已有配置或已被其他设备更新，请先下载核对后再上传".into());
        }
        let backup_name = format!(
            "backup-{}-{}.json",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.9f"),
            &hash[..12]
        );
        let backup = http()?
            .put(remote_file(config, &backup_name, allow_loopback)?)
            .basic_auth(&config.username, Some(secret))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(current)
            .send()
            .await
            .map_err(|_| "云端备份失败，已中止上传")?;
        if !matches!(backup.status().as_u16(), 200 | 201 | 204) {
            return Err("云端备份失败，已中止上传".into());
        }
    } else if config.remote_hash.is_some() || config.etag.is_some() {
        return Err("云端配置已被删除，请重新保存连接并核对同步目录".into());
    }
    let mut request = http()?
        .put(remote_file(config, name, allow_loopback)?)
        .basic_auth(&config.username, Some(secret))
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    request = if let Some(etag) = &config.etag {
        if etag.starts_with("W/") {
            request
        } else {
            request.header(reqwest::header::IF_MATCH, etag)
        }
    } else {
        request.header(reqwest::header::IF_NONE_MATCH, "*")
    };
    let response = request
        .body(bytes.to_vec())
        .send()
        .await
        .map_err(|_| "WebDAV 上传失败或超时")?;
    if response.status().as_u16() == 412 {
        return Err("云端已有配置或已被其他设备更新，请先下载核对后再上传".into());
    }
    if !matches!(response.status().as_u16(), 200 | 201 | 204) {
        return Err(format!(
            "WebDAV 上传失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let (remote, etag) = read_remote(config, secret, name, allow_loopback).await?;
    if remote.as_deref() != Some(bytes) {
        return Err("上传后云端已变化，请下载核对".into());
    }
    Ok(etag)
}
async fn test_connection(
    config: &Config,
    secret: &str,
    allow_loopback: bool,
) -> Result<String, String> {
    let response = http()?
        .request(
            reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
            base_url(config, allow_loopback)?,
        )
        .basic_auth(&config.username, Some(secret))
        .header("Depth", "0")
        .send()
        .await
        .map_err(|_| "WebDAV 连接失败或超时")?;
    if response.status().as_u16() != 207 {
        return Err(format!(
            "WebDAV 目录检查失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    Ok("WebDAV 目录可以访问（HTTP 207）。这只说明目录存在，还没有上传配置。".into())
}
fn backup_local(store: &Store) -> Result<PathBuf, String> {
    let settings = store
        .settings()
        .map_err(|_| "无法读取本机设置".to_string())?;
    let accounts = store
        .accounts()
        .map_err(|_| "无法读取本机账号".to_string())?;
    let path = store.root.join("backups").join(format!(
        "webdav-{}.json",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.9f")
    ));
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(&serde_json::json!({
            "kind": "flow2api-local-backup",
            "note": "本机完整配置备份，含 API Key。不要上传或外传。",
            "settings": settings,
            "accounts": accounts
        }))
        .map_err(|_| "无法序列化本机备份")?,
    )?;
    Ok(path)
}
async fn run(
    store: &Store,
    action: &str,
    secrets: &dyn Secrets,
    allow_loopback: bool,
) -> Result<String, String> {
    if !matches!(action, "test" | "push" | "pull") {
        return Err("未知同步操作".into());
    }
    let mut config = load(&store.root)?;
    let secret = secrets.get(&secret_id(&config))?;
    let message = if action == "test" {
        test_connection(&config, &secret, allow_loopback).await?
    } else if action == "push" {
        let portable = portable_from(store)?;
        let bytes = serde_json::to_vec_pretty(&portable).map_err(|_| "同步配置序列化失败")?;
        config.etag = write_remote(&config, &secret, &bytes, FILE, allow_loopback).await?;
        config.remote_hash = Some(format!("{:x}", Sha256::digest(&bytes)));
        "配置已上传。请避免两台电脑同时上传；坚果云不一定执行条件写入，所以上传前会比对已知版本并先备份云端文件。".into()
    } else {
        let (bytes, etag) = read_remote(&config, &secret, FILE, allow_loopback).await?;
        let bytes = bytes.ok_or("云端尚无 Flow2API 配置，请先从已有配置的设备上传")?;
        let portable: Portable =
            serde_json::from_slice(&bytes).map_err(|_| "云端不是有效的 Flow2API v1 配置")?;
        portable.validate()?;
        let backup = backup_local(store)?;
        store
            .merge_synced(
                portable.max_concurrent,
                portable.image_timeout_seconds,
                portable.video_timeout_seconds,
                &portable.synced(),
            )
            .map_err(|error| {
                format!(
                    "下载未应用，本机配置未改：{error}。备份在 {}",
                    backup.display()
                )
            })?;
        config.etag = etag;
        config.remote_hash = Some(format!("{:x}", Sha256::digest(&bytes)));
        format!(
            "云端配置已合并。端口、调用密钥、代理、浏览器路径和开机自启仍用这台电脑的。备份在 {}",
            backup.display()
        )
    };
    config.last_sync = chrono::Utc::now().to_rfc3339();
    config.last_result = message.clone();
    store_config(&store.root, &config)?;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Account, Settings};

    #[test]
    fn payload_keeps_machine_settings_and_rejects_unknown_fields() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut settings = store.settings().unwrap();
        settings.api_key = "local-api-key-value".into();
        settings.proxy_url = "http://127.0.0.1:7890".into();
        settings.port = 8123;
        settings.browser_executable = "/Applications/Google Chrome.app".into();
        store.save_settings(&settings).unwrap();
        store
            .save_account(&Account {
                id: "account-1".into(),
                email: "one@example.test".into(),
                enabled: true,
                project_id: "project".into(),
                last_synced: Some("2026-01-01T00:00:00Z".into()),
                credits: Some(10),
                browser_data_dir: "/tmp/chrome profile".into(),
                browser_profile: "Default".into(),
            })
            .unwrap();
        let payload = serde_json::to_string(&portable_from(&store).unwrap()).unwrap();
        assert!(!payload.contains("local-api-key-value"));
        assert!(!payload.contains("7890"));
        assert!(!payload.contains("8123"));
        assert!(!payload.contains("Chrome"));
        assert!(!payload.contains("chrome profile"));
        assert!(!payload.contains("password"));
        let mut extra: serde_json::Value = serde_json::from_str(&payload).unwrap();
        extra["proxy_url"] = "http://127.0.0.1:1".into();
        assert!(serde_json::from_value::<Portable>(extra).is_err());
    }

    #[test]
    fn merge_preserves_local_secrets_paths_and_rolls_back() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let original = store.settings().unwrap();
        store
            .save_account(&Account {
                id: "local".into(),
                email: "local@example.test".into(),
                enabled: true,
                project_id: String::new(),
                last_synced: Some("2026-01-01T00:00:00Z".into()),
                credits: Some(4),
                browser_data_dir: directory.path().join("profile").display().to_string(),
                browser_profile: "Default".into(),
            })
            .unwrap();
        store
            .merge_synced(
                2,
                90,
                120,
                &[
                    SyncedAccount {
                        id: "local".into(),
                        email: "moved@example.test".into(),
                        enabled: false,
                        project_id: "p1".into(),
                    },
                    SyncedAccount {
                        id: "cloud".into(),
                        email: "cloud@example.test".into(),
                        enabled: true,
                        project_id: String::new(),
                    },
                ],
            )
            .unwrap();
        let settings = store.settings().unwrap();
        assert_eq!(settings.api_key, original.api_key);
        assert_eq!(settings.port, original.port);
        assert_eq!(settings.proxy_url, original.proxy_url);
        assert_eq!(settings.max_concurrent, 2);
        let local = store.account("local").unwrap();
        assert_eq!(local.email, "moved@example.test");
        assert!(local.last_synced.is_none());
        assert!(local.credits.is_none());
        assert!(local.browser_data_dir.contains("profile"));
        let cloud = store.account("cloud").unwrap();
        assert!(cloud.browser_data_dir.is_empty());
        assert!(store
            .merge_synced(0, 90, 120, &[])
            .unwrap_err()
            .to_string()
            .contains("并发"));
        assert_eq!(store.settings().unwrap().max_concurrent, 2);
        assert_eq!(store.settings().unwrap().api_key, original.api_key);
    }

    #[test]
    fn requires_clean_https_directory() {
        for url in [
            "http://example.com/dav",
            "https://user:secret@example.com/dav",
            "https://example.com/dav?token=1",
        ] {
            assert!(base_url(
                &Config {
                    url: url.into(),
                    username: "person".into(),
                    ..Config::default()
                },
                false
            )
            .is_err());
        }
        assert_eq!(
            remote_file(
                &Config {
                    url: "https://dav.jianguoyun.com/dav/".into(),
                    username: "person".into(),
                    ..Config::default()
                },
                FILE,
                false
            )
            .unwrap()
            .path(),
            "/dav/Flow2API/flow2api-v1.json"
        );
    }

    #[test]
    fn decrypts_only_json_string_ciphertext() {
        let key = [9u8; 32];
        let nonce = [3u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut bytes = nonce.to_vec();
        bytes.extend(
            cipher
                .encrypt(
                    aes_gcm::Nonce::from_slice(&nonce),
                    &br#""app-password""#[..],
                )
                .unwrap(),
        );
        assert_eq!(
            decrypt(&key, &STANDARD.encode(&bytes)).unwrap(),
            "app-password"
        );
        bytes[13] ^= 1;
        assert!(decrypt(&key, &STANDARD.encode(&bytes)).is_err());
        assert!(decrypt(&key, "plain").is_err());
    }

    #[test]
    fn blank_password_keeps_saved_secret_without_echo() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        configure(
            directory.path(),
            &secrets,
            "https://dav.jianguoyun.com/dav/".into(),
            "person".into(),
            "app-password".into(),
            false,
        )
        .unwrap();
        configure(
            directory.path(),
            &secrets,
            "https://dav.jianguoyun.com/dav/".into(),
            "person".into(),
            String::new(),
            false,
        )
        .unwrap();
        let stored = std::fs::read_to_string(directory.path().join("webdav.json")).unwrap();
        assert!(!stored.contains("app-password"));
        let config = load(directory.path()).unwrap();
        assert_eq!(secrets.get(&secret_id(&config)).unwrap(), "app-password");
        assert!(configure(
            directory.path(),
            &secrets,
            "https://dav.example.test/dav/".into(),
            "person".into(),
            String::new(),
            false
        )
        .unwrap_err()
        .contains("应用密码"));
    }

    #[tokio::test]
    async fn mock_server_roundtrip_conflict_and_merge() {
        let files = std::sync::Arc::new(Mutex::new(
            HashMap::<String, (Vec<u8>, Option<String>)>::new(),
        ));
        let router = axum::Router::new()
            .fallback(dav_handler)
            .with_state(files.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut settings = store.settings().unwrap();
        settings.api_key = "local-api-key-value".into();
        settings.proxy_url = "socks5://127.0.0.1:7890".into();
        store.save_settings(&settings).unwrap();
        let secrets = MemorySecrets::default();
        configure(
            directory.path(),
            &secrets,
            format!("http://127.0.0.1:{port}/dav/"),
            "person".into(),
            "app-password".into(),
            true,
        )
        .unwrap();
        let config = load(directory.path()).unwrap();
        let secret = secrets.get(&secret_id(&config)).unwrap();
        assert!(test_connection(&config, &secret, true)
            .await
            .unwrap()
            .contains("207"));
        let redirected = http()
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/redirect"))
            .send()
            .await
            .unwrap();
        assert_eq!(redirected.status().as_u16(), 302);
        assert!(run(&store, "push", &secrets, true)
            .await
            .unwrap()
            .contains("已上传"));
        let saved = load(directory.path()).unwrap();
        let mut stale = saved.clone();
        stale.remote_hash = Some("0".repeat(64));
        stale.etag = Some("\"stale\"".into());
        store_config(directory.path(), &stale).unwrap();
        assert!(run(&store, "push", &secrets, true)
            .await
            .unwrap_err()
            .contains("云端"));
        store_config(directory.path(), &saved).unwrap();
        store
            .save_settings(&Settings {
                max_concurrent: 3,
                ..store.settings().unwrap()
            })
            .unwrap();
        assert!(run(&store, "push", &secrets, true)
            .await
            .unwrap()
            .contains("已上传"));
        let other = tempfile::tempdir().unwrap();
        let second = Store::open(other.path()).unwrap();
        let mut second_settings = second.settings().unwrap();
        second_settings.port = 9000;
        second_settings.api_key = "other-api-key-value".into();
        second.save_settings(&second_settings).unwrap();
        configure(
            other.path(),
            &secrets,
            format!("http://127.0.0.1:{port}/dav/"),
            "person".into(),
            String::new(),
            true,
        )
        .unwrap();
        assert!(run(&second, "push", &secrets, true)
            .await
            .unwrap_err()
            .contains("云端"));
        let merged = run(&second, "pull", &secrets, true).await.unwrap();
        assert!(merged.contains("已合并"));
        assert_eq!(second.settings().unwrap().port, 9000);
        assert_eq!(second.settings().unwrap().api_key, "other-api-key-value");
        assert_eq!(second.settings().unwrap().max_concurrent, 3);
        assert!(second.settings().unwrap().proxy_url.is_empty());
        assert!(files
            .lock()
            .unwrap()
            .keys()
            .any(|key| key.contains("backup-")));
    }

    async fn dav_handler(
        axum::extract::State(files): axum::extract::State<
            std::sync::Arc<Mutex<HashMap<String, (Vec<u8>, Option<String>)>>>,
        >,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let method = request.method().clone();
        let path = request.uri().path().to_string();
        if path == "/redirect" {
            return (
                axum::http::StatusCode::FOUND,
                [("location", "https://evil.example/secret")],
                "",
            )
                .into_response();
        }
        if request.headers().get("authorization").is_none() {
            return (axum::http::StatusCode::UNAUTHORIZED, "auth").into_response();
        }
        let if_match = request
            .headers()
            .get(axum::http::header::IF_MATCH)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if method.as_str() == "PROPFIND" && path == "/dav/" {
            return (axum::http::StatusCode::MULTI_STATUS, "directory").into_response();
        }
        if method.as_str() == "MKCOL" && path.trim_end_matches('/') == "/dav/Flow2API" {
            return (axum::http::StatusCode::METHOD_NOT_ALLOWED, "exists").into_response();
        }
        let body = axum::body::to_bytes(request.into_body(), LIMIT)
            .await
            .unwrap_or_default();
        let mut files = files.lock().unwrap();
        if method == axum::http::Method::GET {
            return match files.get(&path) {
                Some((bytes, _)) => {
                    let tag = format!("\"{:x}\"", Sha256::digest(bytes));
                    ([(axum::http::header::ETAG, tag)], bytes.clone()).into_response()
                }
                None => (axum::http::StatusCode::NOT_FOUND, "missing").into_response(),
            };
        }
        if method == axum::http::Method::PUT {
            if let Some(expected) = &if_match {
                if let Some((bytes, _)) = files.get(&path) {
                    let tag = format!("\"{:x}\"", Sha256::digest(bytes));
                    if expected != &tag && expected != "*" {
                        return (axum::http::StatusCode::PRECONDITION_FAILED, "conflict")
                            .into_response();
                    }
                }
            }
            // If-None-Match is intentionally ignored, matching observed Jianguoyun behavior.
            let tag = format!("\"{:x}\"", Sha256::digest(&body));
            files.insert(path, (body.to_vec(), Some(tag)));
            return (axum::http::StatusCode::CREATED, "stored").into_response();
        }
        (axum::http::StatusCode::METHOD_NOT_ALLOWED, "no").into_response()
    }
}
