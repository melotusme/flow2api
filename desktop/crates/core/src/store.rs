use crate::model::*;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

pub struct Store {
    connection: Mutex<Connection>,
    pub root: PathBuf,
    _instance: std::fs::File,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let instance = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(root.join("instance.lock"))?;
        instance
            .try_lock()
            .context("该数据目录已被另一个 Flow2API 实例使用")?;
        std::fs::create_dir_all(root.join("generated"))?;
        std::fs::create_dir_all(root.join("profiles"))?;
        let connection = Connection::open(root.join("desktop.db"))?;
        let schema: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if schema > 1 {
            bail!("数据库由较新版本创建，请升级应用；没有修改原数据库");
        }
        connection.busy_timeout(std::time::Duration::from_secs(10))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS media (filename TEXT PRIMARY KEY, value TEXT NOT NULL);
            PRAGMA user_version=1;")?;
        let store = Self {
            connection: Mutex::new(connection),
            root: root.to_owned(),
            _instance: instance,
        };
        if store
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get::<_, i64>(0))?
            == 0
        {
            store.save_settings(&Settings::default())?;
        }
        // A process restart cannot prove an accepted upstream request failed. Never resubmit it automatically.
        for mut task in store.tasks()? {
            if task.state == "queued" || task.state == "running" {
                task.state = "interrupted".into();
                task.stage = "进程退出，需核对 Flow 中的任务结果".into();
                task.error = Some("上次运行被中断；为避免重复扣费，没有自动重试".into());
                task.updated_at = now();
                store.save_task(&task)?;
            }
        }
        Ok(store)
    }
    pub fn settings(&self) -> Result<Settings> {
        let value: String = self.connection.lock().unwrap().query_row(
            "SELECT value FROM settings WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&value)?)
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        if settings.port == 0
            || settings.max_concurrent == 0
            || settings.max_concurrent > 8
            || settings.api_key.trim().len() < 16
        {
            bail!("端口须为 1–65535，并发须为 1–8，API Key 至少 16 字符");
        }
        if settings.image_timeout_seconds < 30 || settings.video_timeout_seconds < 30 {
            bail!("生成超时至少 30 秒");
        }
        if !settings.proxy_url.is_empty() {
            let url = url::Url::parse(&settings.proxy_url).context("代理地址格式错误")?;
            if !["http", "https", "socks5", "socks5h"].contains(&url.scheme())
                || url.host_str().is_none()
            {
                bail!("代理地址必须为 HTTP(S)/SOCKS5 URL");
            }
        }
        self.connection.lock().unwrap().execute("INSERT INTO settings(id,value) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value", [serde_json::to_string(settings)?])?;
        Ok(())
    }
    pub fn accounts(&self) -> Result<Vec<Account>> {
        self.read_values("SELECT value FROM accounts ORDER BY id")
    }
    pub fn account(&self, id: &str) -> Result<Account> {
        self.accounts()?
            .into_iter()
            .find(|v| v.id == id)
            .context("账号不存在")
    }
    pub fn save_account(&self, account: &Account) -> Result<()> {
        if account.email.trim().is_empty() || !account.email.contains('@') {
            bail!("请输入 Google 账号邮箱");
        }
        self.connection.lock().unwrap().execute("INSERT INTO accounts(id,value) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET value=excluded.value", params![account.id, serde_json::to_string(account)?])?;
        Ok(())
    }
    pub fn delete_account(&self, id: &str) -> Result<()> {
        self.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM accounts WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        let mut tasks: Vec<Task> = self.read_values("SELECT value FROM tasks")?;
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }
    pub fn task(&self, id: &str) -> Result<Task> {
        self.tasks()?
            .into_iter()
            .find(|v| v.id == id)
            .context("任务不存在")
    }
    pub fn save_task(&self, task: &Task) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO tasks(id,value) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET value=excluded.value", params![task.id, serde_json::to_string(task)?])?;
        Ok(())
    }
    pub fn media(&self) -> Result<Vec<Media>> {
        let mut result: Vec<Media> = self.read_values("SELECT value FROM media")?;
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result)
    }
    pub fn save_media(&self, media: &Media) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO media(filename,value) VALUES(?1,?2) ON CONFLICT(filename) DO UPDATE SET value=excluded.value", params![media.filename, serde_json::to_string(media)?])?;
        Ok(())
    }
    fn read_values<T: serde::de::DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_data_directory_has_one_owner() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        assert!(Store::open(directory.path()).is_err());
        drop(store);
        assert!(Store::open(directory.path()).is_ok());
        Ok(())
    }
    #[test]
    fn newer_database_is_rejected_without_downgrade() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("desktop.db");
        let db = Connection::open(&path)?;
        db.execute_batch("PRAGMA user_version=20;")?;
        drop(db);
        assert!(Store::open(directory.path()).is_err());
        let db = Connection::open(path)?;
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            20
        );
        Ok(())
    }
    #[test]
    fn restart_marks_incomplete_tasks_without_resubmitting() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(dir.path())?;
        let key = store.settings()?.api_key;
        store.save_task(&Task {
            id: "job".into(),
            request: GenerationRequest {
                model: "model".into(),
                prompt: "hello".into(),
                images: vec![],
                account_id: None,
                video_id: None,
            },
            state: "running".into(),
            stage: "submitted".into(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![],
        })?;
        drop(store);
        let reopened = Store::open(dir.path())?;
        assert_eq!(reopened.task("job")?.state, "interrupted");
        assert_eq!(reopened.settings()?.api_key, key);
        Ok(())
    }
}
