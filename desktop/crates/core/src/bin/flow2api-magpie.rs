//! Read-only settings access, sharing exactly the desktop button's setup logic.
use anyhow::{bail, Context, Result};
use flow2api_core::{magpie, model::Settings};
use rusqlite::{Connection, OpenFlags};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(root) = args.first().filter(|s| !s.starts_with('-')) else {
        bail!("Usage: flow2api-magpie <desktop-data-directory> [--apply]");
    };
    if args.len() > 2 || args.get(1).is_some_and(|s| s != "--apply") {
        bail!("Unknown argument");
    }
    let db = Connection::open_with_flags(
        std::path::Path::new(root).join("desktop.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("无法读取桌面版数据库，请先打开 Flow2API；不要使用旧版 data/flow.db")?;
    let value: String = db.query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))?;
    let settings: Settings =
        serde_json::from_str(&value).map_err(|_| anyhow::anyhow!("无法解析桌面版设置"))?;
    let path = magpie::config_path()?;
    let info = if args.get(1).is_some() {
        magpie::connect(&path, &settings)?
    } else {
        magpie::inspect(&path, &settings)?
    };
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}
