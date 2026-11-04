//! Read-only import of the archived Python data layout. SQLite backups include WAL data.
use crate::{generation::media_extension, model::*, store::Store};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, DatabaseName, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

const TABLES: &[&str] = &["settings", "accounts", "tasks", "media", "legacy_records"];
#[derive(Clone, Serialize, Deserialize)]
pub struct Preview {
    pub source: String,
    pub accounts: usize,
    pub projects: usize,
    pub tasks: usize,
    pub logs: usize,
    pub media: usize,
    pub settings_available: bool,
    pub notes: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub created_at: String,
    pub source: String,
    pub accounts_added: usize,
    pub accounts_existing: usize,
    pub tasks_added: usize,
    pub media_added: usize,
    pub records_added: usize,
    pub settings_imported: bool,
    pub backup_dir: String,
    pub notes: Vec<String>,
    fingerprint: String,
}
struct Legacy {
    root: PathBuf,
    data: PathBuf,
    db: Connection,
}
fn open(path: &Path) -> Result<Legacy> {
    let root = path.canonicalize().context("旧版目录不存在")?;
    let (root, data) = if root.join("data/flow.db").is_file() {
        (root.clone(), root.join("data"))
    } else if root.join("flow.db").is_file() {
        (root.parent().unwrap_or(&root).to_owned(), root.clone())
    } else {
        bail!("请选择包含 data/flow.db 的旧版项目目录，或包含 flow.db 的 data 目录");
    };
    let db = Connection::open_with_flags(data.join("flow.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    if !has_table(&db, "tokens")? {
        bail!("该数据库不是支持的 Python Flow2API 数据库");
    }
    Ok(Legacy { root, data, db })
}
fn has_table(db: &Connection, name: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )? > 0)
}
fn rows(db: &Connection, table: &str, fields: &[&str]) -> Result<Vec<Value>> {
    if !has_table(db, table)? {
        return Ok(vec![]);
    }
    let mut stmt = db.prepare(&format!("SELECT * FROM {table}"))?;
    let columns: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|v| (*v).to_owned())
        .collect();
    let values = stmt.query_map([], |row| {
        let mut object = serde_json::Map::new();
        for field in fields {
            if let Some(index) = columns.iter().position(|c| c == field) {
                use rusqlite::types::ValueRef;
                let value = match row.get_ref(index)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => json!(n),
                    ValueRef::Real(n) => json!(n),
                    ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                    ValueRef::Blob(_) => Value::Null,
                };
                object.insert((*field).to_owned(), value);
            }
        }
        Ok(Value::Object(object))
    })?;
    Ok(values.collect::<rusqlite::Result<_>>()?)
}
fn archived_rows(db: &Connection) -> Result<Vec<(String, Value)>> {
    let mut all = vec![];
    for (table, fields) in [
        (
            "projects",
            vec![
                "id",
                "project_id",
                "token_id",
                "project_name",
                "tool_name",
                "is_active",
                "created_at",
            ],
        ),
        (
            "tasks",
            vec![
                "id",
                "task_id",
                "token_id",
                "model",
                "prompt",
                "status",
                "progress",
                "error_message",
                "scene_id",
                "created_at",
                "completed_at",
            ],
        ),
        // Raw request/response bodies may contain credentials or signed URLs. Keep them
        // only in the complete source backup; the searchable archive uses summary fields.
        (
            "request_logs",
            vec![
                "id",
                "token_id",
                "operation",
                "status_code",
                "duration",
                "status_text",
                "progress",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "token_stats",
            vec![
                "id",
                "token_id",
                "image_count",
                "video_count",
                "success_count",
                "error_count",
                "last_success_at",
                "last_error_at",
            ],
        ),
    ] {
        for row in rows(db, table, &fields)? {
            all.push((table.to_owned(), row));
        }
    }
    Ok(all)
}
fn media_paths(data: &Path) -> Result<Vec<PathBuf>> {
    let directory = data.join("generated");
    if !directory.is_dir() {
        return Ok(vec![]);
    }
    let mut paths = vec![];
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some((hash, ext)) = name.split_once('.') {
            if hash.len() == 64
                && hash.bytes().all(|v| v.is_ascii_hexdigit())
                && ["png", "jpg", "webp", "gif", "mp4", "webm"].contains(&ext)
            {
                paths.push(entry.path());
            }
        }
    }
    paths.sort();
    Ok(paths)
}
pub fn preview(path: &Path) -> Result<Preview> {
    let legacy = open(path)?;
    let count = |table: &str| -> Result<usize> {
        if !has_table(&legacy.db, table)? {
            return Ok(0);
        }
        Ok(legacy
            .db
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })? as usize)
    };
    Ok(Preview {
        source: legacy.root.display().to_string(),
        accounts: count("tokens")?,
        projects: count("projects")?,
        tasks: count("tasks")?,
        logs: count("request_logs")?,
        media: media_paths(&legacy.data)?.len(),
        settings_available: legacy.root.join("config/setting.toml").is_file(),
        notes: vec![
            "导入前请停止旧版服务，避免生成中的数据库与作品继续变化。".into(),
            "账号与项目会迁入，登录状态须在专用 Chrome 中重新同步。".into(),
            "旧版未完成任务仅作为中断历史导入，不自动重新提交。".into(),
            "完整旧数据库会备份；日志正文与旧密码不写入新版任务和检索记录。".into(),
            "只导入永久作品 data/generated，临时缓存与 Chrome 资料目录不跨端复制。".into(),
        ],
    })
}
fn fingerprint(db: &Connection) -> Result<String> {
    let mut hash = Sha256::new();
    for table in TABLES {
        hash.update(table.as_bytes());
        let mut stmt = db.prepare(&format!("SELECT value FROM {table} ORDER BY value"))?;
        for value in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let value = value?;
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn legacy_id(source: &str, kind: &str, id: &Value) -> String {
    let hash = Sha256::digest(format!("{source}:{kind}:{id}"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    uuid::Uuid::from_bytes(bytes).to_string()
}
fn text(row: &Value, field: &str) -> String {
    row[field].as_str().unwrap_or_default().to_owned()
}
fn read_settings(
    legacy: &Legacy,
    config_path: &Path,
    mut settings: Settings,
    notes: &mut Vec<String>,
) -> Result<Settings> {
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(config_path).context("无法读取旧版 setting.toml")?)
            .context("旧版 setting.toml 格式错误")?;
    if let Some(port) = config
        .get("server")
        .and_then(|v| v.get("port"))
        .and_then(toml::Value::as_integer)
    {
        settings.port = u16::try_from(port).context("旧版端口无效")?;
    }
    // Database settings override TOML in the archived service.
    let admin = rows(&legacy.db, "admin_config", &["api_key"])?;
    let key = admin
        .first()
        .and_then(|r| r["api_key"].as_str())
        .or_else(|| {
            config
                .get("global")
                .and_then(|v| v.get("api_key"))
                .and_then(toml::Value::as_str)
        });
    if let Some(key) = key {
        if key.trim().len() >= 16 {
            settings.api_key = key.into();
        } else {
            notes.push("旧 API Key 少于 16 字符，保留桌面版 API Key；外部工具需更新 Key。".into());
        }
    }
    let generation = rows(
        &legacy.db,
        "generation_config",
        &["image_timeout", "video_timeout"],
    )?;
    for (field, target) in [
        ("image_timeout", &mut settings.image_timeout_seconds),
        ("video_timeout", &mut settings.video_timeout_seconds),
    ] {
        let value = generation
            .first()
            .and_then(|r| r[field].as_u64())
            .or_else(|| {
                config
                    .get("generation")
                    .and_then(|v| v.get(field))
                    .and_then(toml::Value::as_integer)
                    .and_then(|v| u64::try_from(v).ok())
            });
        if let Some(value) = value {
            if value >= 30 {
                *target = value;
            }
        }
    }
    let proxy = rows(&legacy.db, "proxy_config", &["enabled", "proxy_url"])?;
    if let Some(proxy) = proxy.first() {
        if proxy["enabled"].as_i64().unwrap_or(0) != 0 {
            settings.proxy_url = text(proxy, "proxy_url");
        } else {
            settings.proxy_url.clear();
        }
    } else if config
        .get("proxy")
        .and_then(|v| v.get("proxy_enabled"))
        .and_then(toml::Value::as_bool)
        == Some(true)
    {
        settings.proxy_url = config["proxy"]["proxy_url"]
            .as_str()
            .unwrap_or_default()
            .into();
    }
    if !settings.proxy_url.is_empty() {
        let proxy = url::Url::parse(&settings.proxy_url).context("旧版代理地址无效")?;
        if proxy.host_str().is_none() {
            bail!("旧版代理缺少主机地址；可先不导入设置");
        }
        if !proxy.username().is_empty() || proxy.password().is_some() {
            bail!("旧版代理带用户名密码，当前桌面 Chrome 需要先改用本机代理转发；可先不导入设置");
        }
    }
    Ok(settings)
}

pub fn import(store: &Store, path: &Path, import_settings: bool) -> Result<Report> {
    let legacy = open(path)?;
    if store.root.canonicalize()? == legacy.data {
        bail!("旧版与桌面版不能使用同一数据目录");
    }
    let source = legacy.root.display().to_string();
    let source_id = legacy.data.join("flow.db").display().to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let backup = store.root.join("backups").join(&id);
    fs::create_dir_all(&backup)?;
    let mut db = store.connection.lock().unwrap();
    db.backup(DatabaseName::Main, backup.join("desktop-before.db"), None)?;
    legacy
        .db
        .backup(DatabaseName::Main, backup.join("python-source.db"), None)?;
    let config_path = backup.join("setting.toml");
    if legacy.root.join("config/setting.toml").is_file() {
        fs::copy(legacy.root.join("config/setting.toml"), &config_path)?;
    }
    let legacy = Legacy {
        db: Connection::open_with_flags(
            backup.join("python-source.db"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?,
        root: legacy.root,
        data: legacy.data,
    };
    let mut report = Report {
        id,
        created_at: now(),
        source: source.clone(),
        accounts_added: 0,
        accounts_existing: 0,
        tasks_added: 0,
        media_added: 0,
        records_added: 0,
        settings_imported: import_settings,
        backup_dir: backup.display().to_string(),
        notes: preview(path)?.notes,
        fingerprint: String::new(),
    };
    let settings: Settings = serde_json::from_str(&db.query_row(
        "SELECT value FROM settings WHERE id=1",
        [],
        |r| r.get::<_, String>(0),
    )?)?;
    let settings = if import_settings {
        Some(read_settings(
            &legacy,
            &config_path,
            settings,
            &mut report.notes,
        )?)
    } else {
        None
    };
    // Validate the migrated fields before beginning the metadata transaction.
    if let Some(settings) = &settings {
        if settings.port == 0
            || settings.api_key.trim().len() < 16
            || settings.image_timeout_seconds < 30
            || settings.video_timeout_seconds < 30
        {
            bail!("旧设置不符合桌面版要求；可先不导入设置");
        }
        if !settings.proxy_url.is_empty()
            && !["http", "https", "socks5", "socks5h"]
                .contains(&url::Url::parse(&settings.proxy_url)?.scheme())
        {
            bail!("旧版代理协议不受支持");
        }
    }
    let tx = db.transaction()?;
    let mut created = vec![];
    let result = (|| -> Result<()> {
        let mut existing: Vec<Account> = {
            let mut stmt = tx.prepare("SELECT value FROM accounts")?;
            let values = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            values
                .into_iter()
                .map(|v| Ok(serde_json::from_str(&v)?))
                .collect::<Result<_>>()?
        };
        let mut mapping = HashMap::new();
        for row in rows(
            &legacy.db,
            "tokens",
            &["id", "email", "is_active", "credits", "current_project_id"],
        )? {
            let email = text(&row, "email").trim().to_lowercase();
            if !email.contains('@') {
                report.notes.push("跳过一条缺少有效邮箱的旧账号。".into());
                continue;
            }
            let account = if let Some(account) = existing
                .iter()
                .find(|a| a.email.eq_ignore_ascii_case(&email))
            {
                report.accounts_existing += 1;
                account.clone()
            } else {
                let project = text(&row, "current_project_id");
                let account = Account {
                    id: legacy_id(&source_id, "account", &row["id"]),
                    email,
                    enabled: row["is_active"].as_i64().unwrap_or(1) != 0,
                    project_id: if uuid::Uuid::parse_str(&project).is_ok() {
                        project
                    } else {
                        String::new()
                    },
                    last_synced: None,
                    credits: row["credits"].as_i64(),
                    browser_data_dir: String::new(),
                    browser_profile: String::new(),
                    browser_bridge: false,
                };
                tx.execute(
                    "INSERT INTO accounts(id,value) VALUES(?1,?2)",
                    params![account.id, serde_json::to_string(&account)?],
                )?;
                report.accounts_added += 1;
                existing.push(account.clone());
                account
            };
            mapping.insert(row["id"].to_string(), account.id);
        }
        for (table, row) in archived_rows(&legacy.db)? {
            let record_id = legacy_id(&source_id, &table, &row["id"]);
            report.records_added += tx.execute(
                "INSERT OR IGNORE INTO legacy_records(id,value) VALUES(?1,?2)",
                params![
                    record_id,
                    serde_json::to_string(&json!({"source":source,"table":table,"record":row}))?
                ],
            )?;
            if table == "tasks" {
                let old_state = text(&row, "status");
                let task = Task {
                    id: record_id,
                    request: GenerationRequest {
                        model: text(&row, "model"),
                        prompt: text(&row, "prompt"),
                        images: vec![],
                        account_id: mapping.get(&row["token_id"].to_string()).cloned(),
                        video_id: None,
                    },
                    state: if old_state == "completed" {
                        "archived"
                    } else if old_state == "failed" {
                        "failed"
                    } else {
                        "interrupted"
                    }
                    .into(),
                    stage: if old_state == "completed" {
                        "旧版已完成；结果与来源保留在迁移记录中"
                    } else {
                        "旧版历史；未重新提交"
                    }
                    .into(),
                    created_at: row["created_at"]
                        .as_str()
                        .unwrap_or(&report.created_at)
                        .into(),
                    updated_at: row["completed_at"]
                        .as_str()
                        .unwrap_or(&report.created_at)
                        .into(),
                    error: if old_state == "failed" {
                        Some(text(&row, "error_message"))
                    } else if old_state == "completed" {
                        None
                    } else {
                        Some("旧版未完成任务没有自动重新提交，请先核对 Flow 结果".into())
                    },
                    results: vec![],
                };
                report.tasks_added += tx.execute(
                    "INSERT OR IGNORE INTO tasks(id,value) VALUES(?1,?2)",
                    params![task.id, serde_json::to_string(&task)?],
                )?;
            }
        }
        for path in media_paths(&legacy.data)? {
            if fs::metadata(&path)?.len() > 512 * 1024 * 1024 {
                bail!("旧版作品超过 512 MB，已取消导入");
            }
            let bytes = fs::read(&path)?;
            let kind = if [Some("mp4"), Some("webm")]
                .contains(&path.extension().and_then(|v| v.to_str()))
            {
                "video"
            } else {
                "image"
            };
            let ext = media_extension(&bytes, kind)?;
            let filename = format!("{:x}.{ext}", Sha256::digest(&bytes));
            if path.file_name().and_then(|v| v.to_str()) != Some(filename.as_str()) {
                bail!("旧版作品哈希或格式不匹配，已取消导入");
            }
            let target = store.root.join("generated").join(&filename);
            if target.exists() && fs::read(&target)? != bytes {
                bail!("桌面版已有同名但内容不一致的作品，已取消导入");
            }
            if !target.exists() {
                let temp = target.with_extension(format!("{}.import-part", uuid::Uuid::new_v4()));
                if let Err(error) = fs::write(&temp, &bytes) {
                    let _ = fs::remove_file(&temp);
                    return Err(error.into());
                }
                if let Err(error) = fs::rename(&temp, &target) {
                    let _ = fs::remove_file(temp);
                    return Err(error.into());
                }
                created.push(target.clone());
            }
            let record: Value = fs::read(path.with_extension("json"))
                .ok()
                .and_then(|v| serde_json::from_slice(&v).ok())
                .unwrap_or(Value::Null);
            let media = Media {
                filename,
                kind: kind.into(),
                size: bytes.len() as u64,
                created_at: record["created_at"]
                    .as_str()
                    .unwrap_or(&report.created_at)
                    .into(),
                task_id: format!("legacy:{}", report.id),
                media_id: record["media_id"].as_str().unwrap_or_default().into(),
                prompt: record["prompt"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(2000)
                    .collect(),
                model: record["model"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect(),
            };
            let metadata = target.with_extension("json");
            if !metadata.exists() {
                created.push(metadata.clone());
                fs::write(&metadata, serde_json::to_vec_pretty(&media)?)?;
            }
            report.media_added += tx.execute(
                "INSERT OR IGNORE INTO media(filename,value) VALUES(?1,?2)",
                params![media.filename, serde_json::to_string(&media)?],
            )?;
        }
        if let Some(settings) = settings {
            tx.execute(
                "UPDATE settings SET value=?1 WHERE id=1",
                [serde_json::to_string(&settings)?],
            )?;
        }
        report.fingerprint = fingerprint(&tx)?;
        tx.execute(
            "INSERT INTO migration_runs(id,value) VALUES(?1,?2)",
            params![report.id, serde_json::to_string(&report)?],
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        drop(tx);
        for path in created {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }
    if let Err(error) = tx.commit() {
        for path in created {
            let _ = fs::remove_file(path);
        }
        return Err(error.into());
    }
    // The authoritative report is committed in SQLite. Sidecar failure must not make a successful import look failed.
    let _ = fs::write(
        backup.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    );
    Ok(report)
}
pub fn reports(store: &Store) -> Result<Vec<Report>> {
    let db = store.connection.lock().unwrap();
    let mut stmt = db.prepare("SELECT value FROM migration_runs ORDER BY rowid DESC")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|r| Ok(serde_json::from_str(&r)?))
        .collect()
}
pub fn rollback(store: &Store, id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id).context("导入记录 ID 无效")?;
    let mut db = store.connection.lock().unwrap();
    let report: Report = serde_json::from_str(
        &db.query_row("SELECT value FROM migration_runs WHERE id=?1", [id], |r| {
            r.get::<_, String>(0)
        })
        .context("找不到导入记录")?,
    )?;
    if fingerprint(&db)? != report.fingerprint {
        bail!(
            "导入后已有账号、设置、任务或作品变化，不能直接撤销；备份保留在数据目录的 backups 中"
        );
    }
    let path = store
        .root
        .join("backups")
        .join(id)
        .join("desktop-before.db");
    let before = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let backup = rusqlite::backup::Backup::new(&before, &mut db)?;
    backup.run_to_completion(100, std::time::Duration::from_millis(20), None)?;
    // Imported media bytes remain unindexed for recovery; never delete subsequent user files.
    Ok(())
}

pub fn history(store: &Store, table: &str) -> Result<Vec<Value>> {
    if !["projects", "tasks", "request_logs", "token_stats"].contains(&table) {
        bail!("未知历史类型");
    }
    let db = store.connection.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT value FROM legacy_records WHERE json_extract(value,'$.table')=?1 ORDER BY rowid DESC",
    )?;
    let values = stmt
        .query_map([table], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    values
        .into_iter()
        .map(|v| Ok(serde_json::from_str(&v)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> Result<Connection> {
        fs::create_dir_all(root.join("data/generated"))?;
        fs::create_dir_all(root.join("config"))?;
        fs::write(root.join("config/setting.toml"),"[server]\nport=8901\n[global]\napi_key='fixture-api-key-long-enough'\n[generation]\nimage_timeout=120\nvideo_timeout=900\n")?;
        let db = Connection::open(root.join("data/flow.db"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            CREATE TABLE tokens(id INTEGER PRIMARY KEY,email TEXT,is_active INTEGER,credits INTEGER,current_project_id TEXT,google_cookies TEXT,login_password TEXT);
            INSERT INTO tokens VALUES(1,'one@example.test',1,123,'00000000-0000-0000-0000-000000000001','SECRET_COOKIE_SENTINEL','SECRET_PASSWORD_SENTINEL');
            CREATE TABLE projects(id INTEGER PRIMARY KEY,project_id TEXT,token_id INTEGER,project_name TEXT);
            INSERT INTO projects VALUES(1,'00000000-0000-0000-0000-000000000001',1,'旧项目');
            CREATE TABLE tasks(id INTEGER PRIMARY KEY,task_id TEXT,token_id INTEGER,model TEXT,prompt TEXT,status TEXT,created_at TEXT,completed_at TEXT);
            INSERT INTO tasks VALUES(1,'op1',1,'model','old prompt','completed','2026-10-20T00:00:00Z','2026-10-20T00:01:00Z');
            INSERT INTO tasks VALUES(2,'op2',1,'model','unfinished','processing','2026-10-21T00:00:00Z',NULL);
            CREATE TABLE request_logs(id INTEGER PRIMARY KEY,token_id INTEGER,operation TEXT,status_code INTEGER,request_body TEXT,response_body TEXT);
            INSERT INTO request_logs VALUES(1,1,'generate',200,'SECRET_REQUEST_SENTINEL','SECRET_RESPONSE_SENTINEL');")?;
        let bytes = b"\x89PNG\r\n\x1a\nfixture";
        fs::write(
            root.join("data/generated")
                .join(format!("{:x}.png", Sha256::digest(bytes))),
            bytes,
        )?;
        Ok(db)
    }
    #[test]
    fn import_wal_accounts_history_media_settings_and_rollback() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("旧版 项目");
        let old = fixture(&source)?;
        assert!(source.join("data/flow.db-wal").is_file());
        let store = Store::open(&root.path().join("desktop"))?;
        let old_key = store.settings()?.api_key;
        let report = import(&store, &source, true)?;
        assert_eq!(
            (
                report.accounts_added,
                report.tasks_added,
                report.media_added,
                report.records_added
            ),
            (1, 2, 1, 4)
        );
        assert_eq!(store.settings()?.port, 8901);
        assert_eq!(store.settings()?.image_timeout_seconds, 120);
        assert!(store.accounts()?[0].last_synced.is_none());
        assert!(store.tasks()?.iter().any(|t| t.state == "archived"));
        assert!(store.tasks()?.iter().any(|t| t.state == "interrupted"));
        let logs = serde_json::to_string(&history(&store, "request_logs")?)?;
        assert!(!logs.contains("SECRET_"));
        let snapshot = Connection::open(Path::new(&report.backup_dir).join("python-source.db"))?;
        assert_eq!(
            snapshot.query_row("SELECT COUNT(*) FROM tokens", [], |r| r.get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            snapshot.query_row("SELECT login_password FROM tokens", [], |r| r
                .get::<_, String>(0))?,
            "SECRET_PASSWORD_SENTINEL"
        );
        assert_eq!(
            old.query_row("SELECT COUNT(*) FROM tokens", [], |r| r.get::<_, i64>(0))?,
            1
        );
        rollback(&store, &report.id)?;
        assert!(store.accounts()?.is_empty());
        assert!(store.tasks()?.is_empty());
        assert!(store.media()?.is_empty());
        assert_eq!(store.settings()?.api_key, old_key);
        assert!(reports(&store)?.is_empty());
        Ok(())
    }
    #[test]
    fn repeat_import_deduplicates_and_rollback_refuses_new_data() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("old");
        let _old = fixture(&source)?;
        let store = Store::open(&root.path().join("desktop"))?;
        import(&store, &source, false)?;
        let second = import(&store, &source.join("data"), false)?;
        assert_eq!(
            (
                second.accounts_added,
                second.accounts_existing,
                second.tasks_added,
                second.media_added,
                second.records_added
            ),
            (0, 1, 0, 0, 0)
        );
        let mut settings = store.settings()?;
        settings.port = 8123;
        store.save_settings(&settings)?;
        assert!(rollback(&store, &second.id).is_err());
        assert_eq!(store.settings()?.port, 8123);
        assert_eq!(store.accounts()?.len(), 1);
        Ok(())
    }
    #[test]
    fn corrupt_media_rolls_back_metadata_and_retains_backup() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("old");
        let _old = fixture(&source)?;
        fs::write(
            source
                .join("data/generated")
                .join(format!("{}.png", "f".repeat(64))),
            b"not a picture",
        )?;
        let store = Store::open(&root.path().join("desktop"))?;
        assert!(import(&store, &source, true).is_err());
        assert!(store.accounts()?.is_empty());
        assert!(store.tasks()?.is_empty());
        assert!(store.media()?.is_empty());
        assert!(reports(&store)?.is_empty());
        assert_eq!(fs::read_dir(store.root.join("generated"))?.count(), 0);
        let backup = fs::read_dir(store.root.join("backups"))?
            .next()
            .unwrap()?
            .path();
        assert!(backup.join("desktop-before.db").is_file());
        assert!(backup.join("python-source.db").is_file());
        Ok(())
    }
    #[test]
    fn existing_account_and_current_key_survive_legacy_import() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("old");
        let old = fixture(&source)?;
        old.execute_batch("CREATE TABLE admin_config(id INTEGER PRIMARY KEY,api_key TEXT); INSERT INTO admin_config VALUES(1,'short'); CREATE TABLE generation_config(image_timeout INTEGER,video_timeout INTEGER); INSERT INTO generation_config VALUES(222,999);")?;
        let store = Store::open(&root.path().join("desktop"))?;
        let account = Account {
            id: uuid::Uuid::new_v4().to_string(),
            email: "ONE@example.test".into(),
            enabled: false,
            project_id: String::new(),
            last_synced: Some(now()),
            credits: Some(800),
            browser_data_dir: String::new(),
            browser_profile: String::new(),
            browser_bridge: false,
        };
        store.save_account(&account)?;
        let key = store.settings()?.api_key;
        let report = import(&store, &source, true)?;
        assert_eq!((report.accounts_added, report.accounts_existing), (0, 1));
        assert_eq!(store.accounts()?[0].id, account.id);
        assert!(!store.accounts()?[0].enabled);
        assert_eq!(store.accounts()?[0].last_synced, account.last_synced);
        assert!(store
            .tasks()?
            .iter()
            .all(|t| t.request.account_id.as_deref() == Some(&account.id)));
        assert_eq!(store.settings()?.api_key, key);
        assert_eq!(store.settings()?.image_timeout_seconds, 222);
        assert!(report.notes.iter().any(|n| n.contains("少于 16")));
        Ok(())
    }
}
