use anyhow::Result;
use flow2api_core::App;

#[tokio::main]
async fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: flow2api-server <data-directory>"))?;
    let app = App::open(std::path::Path::new(&root))?;
    let status = app.start_service().await?;
    println!("{}", serde_json::to_string(&status)?);
    loop {
        tokio::signal::ctrl_c().await?;
        match app.shutdown().await {
            Ok(()) => return Ok(()),
            Err(error) => eprintln!("{error}；本机服务仍在运行，任务完成后再次按 Ctrl+C 退出"),
        }
    }
}
