# Flow2API Desktop（Rust + Tauri）

安装后使用步骤见 [桌面版使用说明](../docs/desktop-quickstart.md)。关闭窗口后应用留在菜单栏或通知区域，API 继续运行。

## 开发与运行

开发机器需要 Rust、Tauri 2 CLI，以及对应系统的编译工具。Windows 使用 MSVC C++ 工具与 WebView2；macOS 使用 Xcode Command Line Tools。使用者安装成品应用后不需要 Rust、Python、Node 或 Docker。v0.3.0 默认通过连接脚本复用系统默认浏览器里的 Flow 登录，支持 Ego Lite、Chrome。Cookie 留在原浏览器；需 Tampermonkey。专用 Chrome 独立登录保留在高级选项。详见 [浏览器连接](../docs/browser-connection.md)。

```bash
# 从项目根目录运行测试
rtk proxy cargo test --manifest-path desktop/Cargo.toml -p flow2api-core
# 编译整个桌面工程
rtk proxy cargo check --manifest-path desktop/Cargo.toml --workspace
# 从 desktop 目录启动桌面窗口
rtk proxy cargo tauri dev
# 关闭窗口后继续后台运行；从托盘／菜单栏选择“退出 Flow2API”停止
```

首次启动会创建应用数据目录及随机 API Key，默认监听 `127.0.0.1:8000`。在账号页安装新版脚本、打开 Flow、获取连接码，并在 Flow 标签页连接。只有实际邮箱和余额校验通过的在线账号可生成；不会搬运日常浏览器的 Cookie 或占用它的资料目录。专用 Chrome 登录见高级选项。
```bash
# 不启动界面的纯 Rust 服务（独立测试数据目录，避免和桌面版同时打开同一数据库）
rtk proxy cargo run --manifest-path desktop/Cargo.toml -p flow2api-core --bin flow2api-server -- /tmp/flow2api-native-check
# 验证本机监听与接口契约（可加 --key <API_KEY> --stage image|video）
rtk proxy node desktop/scripts/verify-e2e.mjs
# 纯服务模式停止：在启动终端按 Ctrl+C；存在生成任务时需等待其完成
```

## 接入兼容性进度

旧版 75 个路由、桌面替代入口和未兼容项见 [接口兼容核对](../docs/desktop-api-compatibility.md)。旧 Web 管理／插件 HTTP 不随桌面功能自动兼容。

OpenAI/Gemini 简写模型支持显式比例、分辨率参数，以及首张参考图的 EXIF 方向和最近比例推断。自动推断后复用已下载字节，避免临时图片 URL 二次读取；参考图限制 20 MB。视频以任务 ID 匹配批量状态，分别保存每个返回结果，并在放大前核对媒体 ID。

多轮请求保留旧版语义：OpenAI 只读取最后消息，图片编辑补入最近可下载的 assistant 图片；Gemini 读取最后 user 内容，并去除工具调用脚手架。OpenAI 还支持顶层 `image` 和 Gemini `contents` 请求格式。模型发现支持 `/v1/models/aliases`、`/models/{model}` 与 `/v1beta/models/{model}`；Gemini 错误返回 `code`、`message` 和 `status`。

这些路径已通过解析与传输测试；真实 Flow 图片／视频生成和放大仍待登录后的端到端验证。

## 健康检查与监控

`GET /health` 与 `GET /metrics` 可在本机免鉴权读取。健康接口区分已登记、已启用和已同步登录账号；`active_tokens` 只统计启用且最后一次登录校验成功的账号。它是缓存状态（`readiness_source=last_verified_login`），不会在每次探测时访问 Google；登录失效被检测到后立即取消可用状态。`status=healthy` 表示 API 可响应，不保证账号可生成。

`/metrics` 返回 Prometheus 文本，提供 API 监听、账号总数／可用数／待登录数及持久化任务状态的 gauge。任务数包含历史记录，失败与中断合并；不提供旧版完整指标集、Token 到期时间或刷新成功率。公共指标不包含账号标识或凭据。

```bash
rtk proxy curl -fsS http://127.0.0.1:8000/health
rtk proxy curl -fsS http://127.0.0.1:8000/metrics
```

## 双端后台状态

- Windows：通知区域托盘图标，左键打开主窗口，菜单查看 API 状态、执行／排队任务、完成／失败数、账号数、浏览器状态和最近错误。
- macOS：菜单栏模板图标和文字 Flow。关闭窗口后离开 Dock，只留在菜单栏；有执行或排队任务时文字带上数量。
- 主窗口打开时，执行中显示 Windows 任务栏／macOS Dock 不确定进度；macOS Dock 同步任务数量角标。上游没有可靠百分比时不猜测完成进度。
- 关闭窗口只隐藏窗口；从菜单「退出 Flow2API」才停止。存在未完成任务时拒绝退出，避免静默中断。
- 退出清理期间拒绝新任务、登录同步和重新启动 API；普通“停止 API”仍可再次启动。连续点击退出只执行一次清理；有未完成任务时保留服务，等待完成后可重试退出。
- 重复启动唤起已有窗口，避免用户重复双击产生数据库锁错误；使用官方 Tauri 单实例插件，macOS 已实测。
- 托盘和设置里的“登录后自动启动”读取系统开机项。手动打开显示窗口；带 `--autostart` 的自启只留在托盘或菜单栏。这个开关按设备保存，不进 WebDAV。
- 状态来自实际 Rust HTTP 监听器与持久化任务记录，不依据 UI 按钮点击推测。

## Chrome 与账号恢复

- 每次复用 Chrome 会话前探测调试连接，页面／连接已关闭时结束旧会话并从原资料目录重新启动，保留 Chrome 自己维护的登录资料。
- 恢复发生在新操作开始前，已提交且结果未知的生成不会自动重试。
- 账号校验失败会取消其可用状态，在账号页重新登录同步后再参与分配。自动分配优先空闲账号，同等负载下选择最久未使用的账号。
- 以下显式测试会打开临时专用 Chrome，验证中文与空格资料路径、失效登录状态、断开连接和原资料重新打开；不访问 Google、不进行真实生成：

```bash
rtk proxy cargo test --manifest-path desktop/Cargo.toml -p flow2api-core browser_transport_detects_closed_page_and_releases_profile -- --ignored --nocapture
```

## 本机设置（不进 git）

个人邮箱等只放在本机，不进 git、不打进安装包。

| 变量 | 用途 |
| --- | --- |
| `FLOW2API_PREFERRED_EMAILS` | 「常用账号」快捷按钮，逗号分隔。未设置时不显示这一栏。 |

桌面应用先读环境变量，读不到再读应用数据目录下的 `local.env`（`KEY=value` 格式，可写 `#` 注释）。从 Finder / 开始菜单启动时拿不到 shell 里的环境变量，所以桌面应用请用 `local.env`：

- macOS：`~/Library/Application Support/dev.flow2api.desktop/local.env`
- Windows：`%APPDATA%\dev.flow2api.desktop\local.env`

```sh
FLOW2API_PREFERRED_EMAILS=you@example.com,another@example.com
```

## 构建安装包

在 `desktop` 目录：

```bash
# macOS 通用包，支持 Intel 与 Apple Silicon
rtk proxy rustup target add aarch64-apple-darwin x86_64-apple-darwin
rtk proxy cargo tauri build --target universal-apple-darwin --bundles app,dmg -- --locked
# Windows（在 Windows MSVC 开发环境中）
rtk proxy cargo tauri build --bundles nsis -- --locked
```

Windows 安装器使用当前用户模式；缺少 WebView2 时联网安装。macOS `.app` / `.dmg` 与 Windows `.exe` 由 `.github/workflows/desktop-build.yml` 配置构建。工作流未实际运行；macOS app 先用 ditto 压缩再上传，下载 ZIP 后解压使用，以保留权限和符号链接。未配置代码签名与公证前，不能将未签名开发包当作正式发行版。

本机已生成未签名的通用开发 ZIP：`../dist/flow2api-macos-universal-debug-20261027.zip`。它包含 arm64 与 x86_64 程序，当前 Apple Silicon Mac 已启动验证；Intel Mac 实机与正式签名尚未验证。开发包不代表完整 Flow 功能验收通过。

本机也已通过 cargo-xwin 交叉编译 Windows x64 程序，并产出 `../dist/Flow2API_0.1.0_x64-setup.exe`。那是 0.1.0 未签名开发包，不是正式发布。交叉编译步骤见 `../docs/windows-cross-build.md`。

## WebDAV

设置页提供手动测试、上传和下载。默认识别坚果云目录 `https://dav.jianguoyun.com/dav/`，专用文件是 `Flow2API/flow2api-v1.json`。应用密码写入 macOS 钥匙串或 Windows 凭据管理器，编辑框留空表示保留。可以从本机 Clash Verge Rev 只读导入，不改它的配置。

同步内容是超时、并发，以及账号邮箱、启用状态和项目编号。端口、调用密钥、代理、浏览器路径、开机自启、登录时间和余额留在本机。下载前先把完整本机配置备份到数据目录的 `backups/`，保存失败会回滚这次事务。云端 JSON 只有 HTTPS，没有端到端加密。两台电脑不要同时上传。

## 自动更新（0.3.1 起）

默认开启本机自动更新：启动 30 秒后检查，之后每 6 小时检查一次；失败 30 分钟后重试。原生后台调度在关闭窗口后继续运行。检查时间、下载状态和错误显示在「设置与接入 → 软件版本与更新」。可以在运行设置中关闭自动更新。

0.3.3 起，更新来自公开的 `melotusme/flow2api-releases`，不需要 GitHub 登录。应用匿名读取最新 Release 和 `latest.json`，只接受该仓库的下载地址，跳转只允许到 GitHub 的发布 CDN。安装前校验包签名及版本，等待执行/排队任务全部结束，再停止 API、安装并重启；安装失败恢复 API。账号资料和数据库留在原目录。0.3.2 及以前的客户端仍用本机 GitHub 登录读取，公开仓库同样能读，所以会照常升级到 0.3.3。

旧版本需要本次从正式 Release 引导安装一次；以后由 App 检查和更新。Linux x64 提供 AppImage，放在可写的用户应用目录（如 `~/.local/opt/flow2api/`），保留可执行权限；运行环境需 WebKitGTK 4.1、GTK3、libayatana-appindicator、FUSE 2（或 `APPIMAGE_EXTRACT_AND_RUN=1`）及已登录的桌面 Secret Service。默认浏览器连接也适用于 Linux Chrome/Chromium。

## 发布与安装

上游源码仓库是公开的 `TheSmallHanCat/flow2api`。当前账号不能向它推送，所以发布构建读取公开 fork `melotusme/flow2api` 上的同一套提交。安装包发布到公开的 `melotusme/flow2api-releases`，Homebrew tap 是公开的 `melotusme/homebrew-flow2api`。审阅用流程在 `.github/release-workflow.yml`，执行副本在发布仓库的 `.github/workflows/release.yml`，只接受手动触发；公开仓库的 Actions 免费，macOS、Windows、Linux 都在 CI 上构建。签名私钥在仓库外的 `~/.local/share/flow2api/signing/`，并作为发布仓库 Secret `TAURI_SIGNING_PRIVATE_KEY`。更新签名不是 Apple 公证，也不是 Windows Authenticode。macOS 使用 ad-hoc 签名。Windows 只有 x64 安装包，没有原生 ARM64。

发布步骤：版本号和 `release-notes.md` 提交推送到 fork 的 `main`，然后运行 `gh workflow run "Publish signed release" -R melotusme/flow2api-releases -f source_ref=main`。三个平台都构建成功才会发布，缺一个平台不能成为 latest。发布后在 tap 仓库执行 `python3 scripts/update-cask.py` 更新版本和校验值并提交。

macOS：

```sh
brew tap melotusme/flow2api
brew install --cask melotusme/flow2api/flow2api
```

Windows x64（PowerShell）：

```powershell
$base = 'https://github.com/melotusme/flow2api-releases/releases/latest/download'
$asset = 'Flow2API.windows.x64.exe'
$dir = Join-Path $env:TEMP ('flow2api-setup-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $dir | Out-Null
$setup = Join-Path $dir $asset
Invoke-WebRequest "$base/$asset" -OutFile $setup
Invoke-WebRequest "$base/SHA256SUMS" -OutFile (Join-Path $dir 'SHA256SUMS')
$pattern = '^([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($asset) + '$'
$lines = @(Get-Content (Join-Path $dir 'SHA256SUMS') | Where-Object { $_ -match $pattern })
if ($lines.Count -ne 1) { throw '缺少唯一的安装包校验记录' }
$expected = ($lines[0] -split '\s+')[0]
if ((Get-FileHash $setup -Algorithm SHA256).Hash -ne $expected) { throw 'SHA-256 校验失败' }
$p = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "安装失败：$($p.ExitCode)" }
```

之后由应用内“检查更新”升级。没有 WinGet 包。
