# Flow2API Desktop（Rust + Tauri）

安装后使用步骤见 [桌面版使用说明](../docs/desktop-quickstart.md)。

重写进行中。Python 旧版归档：`archive/python-before-tauri-20261027`。当前桌面代码不启动 Python sidecar。

## 开发与运行

开发机器需要 Rust、Tauri 2 CLI，以及对应系统的编译工具。Windows 使用 MSVC C++ 工具与 WebView2；macOS 使用 Xcode Command Line Tools。使用者安装成品应用后不需要 Rust、Python、Node 或 Docker。Flow 登录与验证码仍依赖本机 Chrome。 **桌面版不需要浏览器插件**，旧版 Cookie 同步插件不用于此版本。账号页提供 Chrome 官方下载入口和首次登录步骤。

```bash
# 从项目根目录运行测试
rtk proxy cargo test --manifest-path desktop/Cargo.toml -p flow2api-core
# 编译整个桌面工程
rtk proxy cargo check --manifest-path desktop/Cargo.toml --workspace
# 从 desktop 目录启动桌面窗口
rtk proxy cargo tauri dev
# 关闭窗口后继续后台运行；从托盘／菜单栏选择“退出 Flow2API”停止
```

首次启动会创建应用数据目录及随机 API Key，默认尝试监听 `127.0.0.1:8000`。端口冲突会显示错误，可在设置中停止服务、修改端口并重新启动。通过“账号”手动填写邮箱或点选常用邮箱、打开专用 Chrome 登录，再点击“同步登录”。默认每个账号使用独立资料；也可在账号卡片点击“选择已有登录资料”，应用自动扫描本机标准位置的 Chrome 资料，按名称和邮箱展示，匹配当前邮箱的优先显示；无需查找文件夹。自定义位置可通过“其他位置：手动选择”指定 User Data 根目录和 Default / Profile 1 子目录名。资料名称不代表登录有效，仍需同步校验。请在 chrome://version 核对路径，选择前正常退出使用该根目录的浏览器。应用会直接更新选中的资料，并在退出时关闭它启动的浏览器；不会复制 Cookie 或删除锁。同一 User Data 只能绑定一个账号。Chrome 新版可能禁止默认日常目录调试，失败时改用独立登录。EgoLite 直接启动尚未通过测试，当前不作为支持浏览器。

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

- Windows：任务栏通知区域托盘图标，左键打开主窗口，菜单查看 API 状态、执行／排队任务、完成／失败数、账号数、浏览器状态和最近错误。
- macOS：菜单栏图标，任务执行时附带任务数量，菜单项目与 Windows 相同。
- 执行中显示 Windows 任务栏／macOS Dock 不确定进度；macOS Dock 同步任务数量角标。上游没有可靠百分比时不猜测完成进度。
- 关闭窗口隐藏窗口并继续运行；从菜单显式退出。存在未完成任务时拒绝退出，避免静默中断。
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

本机也已通过 cargo-xwin 交叉编译 Windows x64 程序，并产出 `../dist/Flow2API_0.1.0_x64-setup.exe`。那是 0.1.0 未签名开发包，不是 0.2.0 私有发布。交叉编译步骤见 `../docs/windows-cross-build.md`。

## WebDAV

设置页提供手动测试、上传和下载。默认识别坚果云目录 `https://dav.jianguoyun.com/dav/`，专用文件是 `Flow2API/flow2api-v1.json`。应用密码写入 macOS 钥匙串或 Windows 凭据管理器，编辑框留空表示保留。可以从本机 Clash Verge Rev 只读导入，不改它的配置。

同步内容是超时、并发，以及账号邮箱、启用状态和项目编号。端口、调用密钥、代理、浏览器路径、开机自启、登录时间和余额留在本机。下载前先把完整本机配置备份到数据目录的 `backups/`，保存失败会回滚这次事务。云端 JSON 只有 HTTPS，没有端到端加密。两台电脑不要同时上传。

## 私有发布与安装

上游源码仓库是公开的 `TheSmallHanCat/flow2api`。当前账号不能向它推送，所以发布构建读取公开 fork `melotusme/flow2api` 上的同一套提交。安装包只发布到私有 `melotusme/flow2api-releases`，Homebrew tap 是私有 `melotusme/homebrew-flow2api`。审阅用流程在 `.github/release-workflow.yml`，执行副本在发布仓库的 `.github/workflows/release.yml`，只接受手动触发。签名私钥在仓库外的 `~/.local/share/flow2api/signing/`，并作为发布仓库 Secret `TAURI_SIGNING_PRIVATE_KEY`。更新签名不是 Apple 公证，也不是 Windows Authenticode。macOS 使用 ad-hoc 签名。Windows 只有 x64 安装包，没有原生 ARM64。

源码提交到远程并且两个平台都构建成功之后，才运行发布仓库的 Publish signed release。缺一个平台不能成为 latest。tap 里目前只有下载策略和 `scripts/update-cask.py`，没有 cask。第一次正式发布后，在 tap 仓库执行 `python3 scripts/update-cask.py`，核对生成的 `Casks/flow2api.rb` 再提交。在那之前 `brew install --cask` 找不到配方。

macOS，在已有正式 Release 之后：

```sh
brew install gh
gh auth login
gh auth setup-git
brew tap melotusme/flow2api https://github.com/melotusme/homebrew-flow2api.git
brew trust --cask melotusme/flow2api/flow2api
brew install --cask melotusme/flow2api/flow2api
```

Windows x64，在已有正式 Release 之后。先安装 GitHub CLI 并 `gh auth login`，必要时重开 PowerShell：

```powershell
$repo = 'melotusme/flow2api-releases'
$tag = 'v0.2.0'
$asset = 'Flow2API.windows.x64.exe'
$dir = Join-Path $env:TEMP ('desktop-setup-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $dir | Out-Null
gh release download $tag --repo $repo --pattern $asset --pattern SHA256SUMS --dir $dir
if ($LASTEXITCODE -ne 0) { throw '安装包下载失败' }
$setup = Join-Path $dir $asset
$pattern = '^([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($asset) + '$'
$lines = @(Get-Content (Join-Path $dir 'SHA256SUMS') | Where-Object { $_ -match $pattern })
if ($lines.Count -ne 1) { throw '缺少唯一的安装包校验记录' }
$expected = ($lines[0] -split '\s+')[0]
if ((Get-FileHash $setup -Algorithm SHA256).Hash -ne $expected) { throw 'SHA-256 校验失败' }
$p = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "安装失败：$($p.ExitCode)" }
```

这两段命令还没有在目标电脑上跑过。应用内“检查更新”用本机 `gh` 登录读取固定的私有仓库资产，令牌不进界面。没有 WinGet 包。

## 从 Python 版迁移

1. 停止旧版服务，在桌面版“设置与接入 → 从 Python 旧版迁移”选择项目目录或 `data` 目录。
2. 核对账号／项目／任务／日志／永久作品数，直接点击“备份并导入”（无执行中任务时，桌面端会自动平滑重启 API）。可选导入端口、API Key、代理与图片／视频超时。
3. 账号按邮箱去重，已存在的桌面账号保持原样；新账号保留当前项目和额度记录，但必须打开专用 Chrome 登录并同步后使用。旧版 Cookie／密码与 Chrome profile 不直接迁入新会话。
4. `data/generated` 的永久作品验证格式和 SHA256 后复制，临时缓存不导入。旧任务不重新提交；已完成记录显示“旧版已完成”，未完成记录显示“已中断”。项目、请求日志摘要和累计统计可在迁移面板查看最近 200 条。
5. 导入记录显示备份目录。其中 `desktop-before.db` 是桌面版导入前快照，`python-source.db` 是包含 WAL 数据的完整旧库快照，另保留旧配置及报告。备份可能包含旧凭据，不要分享该目录。
6. 停止 API 后可“撤销这次导入”，恢复导入前数据库。导入后若已有新账号、设置、任务或作品记录，会拒绝直接撤销。已复制作品文件保留用于恢复，撤销后不在作品库显示。

旧数据库以只读方式打开，不修改原项目。重复导入同一来源按账号、旧记录 ID 和作品哈希去重，不覆盖已有记录。设置读取优先级保留旧版规则：数据库覆盖 TOML；少于 16 字符的旧 API Key 不导入，外部工具需改用桌面版 Key。带用户名密码的旧代理需要先改为本机转发，或不勾选设置导入。

当前迁移实现已通过合成旧库／WAL／作品夹具与 Mac 原生目录预览验证，尚未对用户实际数据执行导入，也未在 Windows 实测。

### 隔离验证旧数据

可先用临时桌面库验证当前旧数据的导入、作品校验与回滚。该命令只读旧库，不操作正在使用的桌面库，不导入设置、不打开浏览器；临时库和备份随程序退出清理，只输出数量。

从能正常使用官方 Cargo 源的工作目录执行（旧 USTC 镜像环境的处理见交叉构建文档）：

```bash
rtk proxy cargo run --manifest-path $HOME/Documents/flow2api/desktop/Cargo.toml -p flow2api-core --example migration_preview_audit --locked -- $HOME/Documents/flow2api
```

2026-10-27 已对本项目旧数据执行通过：账号 1、项目 4、请求日志 6、永久作品 8，隔离导入与撤销后空库校验均通过。此证据不等于已导入日常桌面数据，也不覆盖 Windows 迁移界面。

## 迁移验收

完整目标仍包括现有图片／视频全部工作流、外部 OpenAI/Gemini 接入、账号刷新、旧数据迁移，以及 Windows/macOS 实机与打包验收。迁移检查表见 `../docs/tauri-migration-progress.md`。模型清单来自旧版基线，不等于所有登记能力已在新版通过验证。
