//! Local Magpie provider setup. Credentials never leave the filesystem/backend.
use crate::{catalog, model::Settings};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct Connection {
    pub configured: bool,
    pub base_url: String,
    pub models: usize,
    pub config_path: String,
    pub backup_path: Option<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .context("无法找到用户目录，请先安装并打开 Magpie")?;
    let mut candidates = vec![PathBuf::from(home).join(".config/magpie/providers.json")];
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            candidates.push(PathBuf::from(appdata).join("magpie/providers.json"));
        }
    }
    let found: Vec<_> = candidates.into_iter().filter(|p| p.is_file()).collect();
    match found.len() {
        1 => Ok(found[0].clone()),
        0 => bail!("未找到 Magpie 配置，请先安装并打开 Magpie，再点击一键接入"),
        _ => bail!("找到多份 Magpie 配置，请先确认正在使用的配置目录"),
    }
}

fn image_models() -> Vec<String> {
    catalog::catalog()
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["config"]["listed"] != false && m["capabilities"]["type"] == "image")
        .map(|(id, _)| id.clone())
        .collect()
}

fn providers(config: &Value) -> Result<&Vec<Value>> {
    config
        .get("providers")
        .and_then(Value::as_array)
        .context("Magpie 配置格式不兼容：缺少 providers 列表")
}

pub fn inspect(path: &Path, settings: &Settings) -> Result<Connection> {
    let config: Value =
        serde_json::from_slice(&fs::read(path).context("无法读取 Magpie 配置，请检查文件权限")?)
            .map_err(|_| anyhow::anyhow!("Magpie 配置无法解析，请在 Magpie 中修复配置后重试"))?;
    let base_url = format!("http://127.0.0.1:{}", settings.port);
    let models = image_models();
    let matches: Vec<_> = providers(&config)?
        .iter()
        .filter(|p| p["id"] == "flow2api")
        .collect();
    let configured = matches.len() == 1 && {
        let p = matches[0];
        p["chat"] == format!("{base_url}/v1")
            && p["gemini"] == format!("{base_url}/v1beta")
            && p["key"] == settings.api_key
            && p["disabled"] != true
            && models.iter().all(|id| {
                p["models"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|m| m == id))
            })
    };
    Ok(Connection {
        configured,
        base_url,
        models: models.len(),
        config_path: path.display().to_string(),
        backup_path: None,
    })
}

fn local_endpoint(value: &Value) -> bool {
    value.as_str().is_none_or(|s| {
        s.is_empty()
            || url::Url::parse(s).is_ok_and(|u| {
                u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                    && u.username().is_empty()
                    && u.password().is_none()
            })
    })
}

pub fn connect(path: &Path, settings: &Settings) -> Result<Connection> {
    if settings.api_key.trim().len() < 16 {
        bail!("请先保存有效的 Flow2API API Key");
    }
    let original = fs::read(path).context("无法读取 Magpie 配置，请检查文件权限")?;
    let mut config: Value = serde_json::from_slice(&original)
        .map_err(|_| anyhow::anyhow!("Magpie 配置无法解析，请在 Magpie 中修复配置后重试"))?;
    let all = providers(&config)?;
    let indices: Vec<_> = all
        .iter()
        .enumerate()
        .filter(|(_, p)| p["id"] == "flow2api")
        .map(|(i, _)| i)
        .collect();
    if indices.len() > 1 {
        bail!("Magpie 中存在重复的 flow2api，请先在 Magpie 中处理");
    }
    if let Some(i) = indices.first() {
        let p = &all[*i];
        if !["chat", "gemini", "responses", "anthropic"]
            .iter()
            .all(|key| local_endpoint(&p[*key]))
        {
            bail!("Magpie 的 flow2api 已指向其他服务器，请先在 Magpie 中重命名该提供商");
        }
    }
    let base = format!("http://127.0.0.1:{}", settings.port);
    let list = config["providers"].as_array_mut().unwrap();
    let p = if let Some(i) = indices.first() {
        &mut list[*i]
    } else {
        list.push(json!({"id":"flow2api","name":"Flow2API"}));
        list.last_mut().unwrap()
    };
    p["chat"] = json!(format!("{base}/v1"));
    p["gemini"] = json!(format!("{base}/v1beta"));
    p["key"] = json!(settings.api_key);
    p["disabled"] = json!(false);
    // Keep this provider on Flow2API: failed generations must not spend another provider's quota.
    p.as_object_mut().unwrap().remove("fallback");
    p.as_object_mut().unwrap().remove("fallbacks");
    p["models"] = json!(image_models());
    let updated = serde_json::to_vec_pretty(&config)?;
    if serde_json::from_slice::<Value>(&original)? == config {
        return inspect(path, settings);
    }
    let parent = path.parent().context("Magpie 配置目录无效")?;
    let backup_dir = parent.join("backups");
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .recursive(true)
        .create(&backup_dir)
        .context("无法创建 Magpie 备份目录，请检查文件权限")?;
    let backup = backup_dir.join(format!(
        "providers-before-flow2api-{}.json",
        uuid::Uuid::new_v4()
    ));
    let mut copy = tempfile::NamedTempFile::new_in(&backup_dir)?;
    copy.write_all(&original)?;
    copy.as_file().sync_all()?;
    copy.persist_noclobber(&backup)
        .map_err(|_| anyhow::anyhow!("无法保存 Magpie 配置备份"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&updated)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    if fs::read(path)? != original {
        bail!("Magpie 配置刚被修改，已保留备份，请再次点击接入");
    }
    temp.persist(path)
        .map_err(|_| anyhow::anyhow!("无法保存 Magpie 配置，请检查文件权限"))?;
    let mut result = inspect(path, settings)?;
    if !result.configured {
        bail!("Magpie 配置写入后校验失败，请使用备份恢复");
    }
    result.backup_path = Some(backup.display().to_string());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_other_providers_backs_up_and_is_idempotent() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("providers.json");
        let original =
            br#"{"providers":[{"id":"other","key":"private","custom":true}],"noAutoGroups":true}"#;
        fs::write(&path, original)?;
        let settings = Settings::default();
        assert!(!inspect(&path, &settings)?.configured);
        let first = connect(&path, &settings)?;
        assert!(first.configured);
        assert_eq!(fs::read(first.backup_path.unwrap())?, original);
        let saved: Value = serde_json::from_slice(&fs::read(&path)?)?;
        assert_eq!(
            saved["providers"][0],
            json!({"id":"other","key":"private","custom":true})
        );
        assert_eq!(saved["noAutoGroups"], true);
        assert!(connect(&path, &settings)?.backup_path.is_none());
        let mut rotated = settings.clone();
        rotated.port = 9321;
        rotated.api_key = "new-key-long-enough".into();
        assert!(!inspect(&path, &rotated)?.configured);
        assert!(connect(&path, &rotated)?.configured);
        assert!(inspect(&path, &rotated)?.configured);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    }
    #[test]
    fn refuses_remote_collision_duplicate_or_corrupt_without_writing() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("providers.json");
        for input in [
            r#"{"providers":[{"id":"flow2api","chat":"https://other.example/v1"}]}"#,
            r#"{"providers":[{"id":"flow2api"},{"id":"flow2api"}]}"#,
            "invalid",
        ] {
            fs::write(&path, input)?;
            assert!(connect(&path, &Settings::default()).is_err());
            assert_eq!(fs::read_to_string(&path)?, input);
        }
        Ok(())
    }
}
