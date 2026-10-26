# Flow2API Desktop（Rust + Tauri）

重写进行中。Python 旧版归档：`archive/python-before-tauri-20261027`。当前桌面代码不启动 Python sidecar。

## 开发与运行

开发机器需要 Rust、Tauri 2 CLI，以及对应系统的编译工具。Windows 使用 MSVC C++ 工具与 WebView2；macOS 使用 Xcode Command Line Tools。使用者安装成品应用后不需要 Rust、Python、Node 或 Docker。Flow 登录与验证码仍依赖本机 Chrome。

```bash
# 从项目根目录运行测试
rtk proxy cargo test --manifest-path desktop/Cargo.toml -p flow2api-core
# 编译整个桌面工程
rtk proxy cargo check --manifest-path desktop/Cargo.toml --workspace
# 从 desktop 目录启动桌面窗口
rtk proxy cargo tauri dev
# 关闭窗口后继续后台运行；从托盘／菜单栏选择“退出 Flow2API”停止
```

首次启动会创建应用数据目录及随机 API Key，默认尝试监听 `127.0.0.1:8000`。端口冲突会显示错误，可在设置中停止服务、修改端口并重新启动。通过“账号”添加邮箱、打开专用 Chrome 登录，再点击“同步登录”。每个账号独立 profile，不直接读取日常 Chrome 的资料。

```bash
# 不启动界面的纯 Rust 服务（独立测试数据目录，避免和桌面版同时打开同一数据库）
rtk proxy cargo run --manifest-path desktop/Cargo.toml -p flow2api-core --bin flow2api-server -- /tmp/flow2api-native-check
# 验证本机监听
rtk proxy curl -fsS http://127.0.0.1:8000/health
# 纯服务模式停止：在启动终端按 Ctrl+C；存在生成任务时需等待其完成
```

## 双端后台状态

- Windows：任务栏通知区域托盘图标，左键打开主窗口，菜单查看 API 状态、执行／排队任务、完成／失败数、账号数、浏览器状态和最近错误。
- macOS：菜单栏图标，任务执行时附带任务数量，菜单项目与 Windows 相同。
- 执行中显示 Windows 任务栏／macOS Dock 不确定进度；macOS Dock 同步任务数量角标。上游没有可靠百分比时不猜测完成进度。
- 关闭窗口隐藏窗口并继续运行；从菜单显式退出。存在未完成任务时拒绝退出，避免静默中断。
- 重复启动唤起已有窗口，避免用户重复双击产生数据库锁错误；使用官方 Tauri 单实例插件，macOS 已实测。
- 状态来自实际 Rust HTTP 监听器与持久化任务记录，不依据 UI 按钮点击推测。

## Chrome 与账号恢复

- 每次复用 Chrome 会话前探测调试连接，页面／连接已关闭时结束旧会话并从原资料目录重新启动，保留 Chrome 自己维护的登录资料。
- 恢复发生在新操作开始前，已提交且结果未知的生成不会自动重试。
- 账号校验失败会取消其可用状态，在账号页重新登录同步后再参与分配。自动分配优先空闲账号，同等负载下选择最久未使用的账号。
- 以下显式测试会打开临时专用 Chrome，验证中文与空格资料路径、失效登录状态、断开连接和原资料重新打开；不访问 Google、不进行真实生成：

```bash
rtk proxy cargo test --manifest-path desktop/Cargo.toml -p flow2api-core browser_transport_detects_closed_page_and_releases_profile -- --ignored --nocapture
```

## 构建安装包

在 `desktop` 目录：

```bash
# macOS
rtk proxy cargo tauri build --bundles app,dmg
# Windows（在 Windows MSVC 开发环境中）
rtk proxy cargo tauri build --bundles nsis
```

Windows 安装器使用当前用户模式；缺少 WebView2 时联网安装。macOS `.app` / `.dmg` 与 Windows `.exe` 由 `.github/workflows/desktop-build.yml` 构建。未配置代码签名与公证前，不能将未签名开发包当作正式发行版。

## 迁移验收

完整目标仍包括现有图片／视频全部工作流、外部 OpenAI/Gemini 接入、账号刷新、旧数据迁移，以及 Windows/macOS 实机与打包验收。迁移检查表见 `../docs/tauri-migration-progress.md`。模型清单来自旧版基线，不等于所有登记能力已在新版通过验证。
