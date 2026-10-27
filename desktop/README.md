# Flow2API Desktop（Rust + Tauri）

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
# 验证本机监听
rtk proxy curl -fsS http://127.0.0.1:8000/health
# 纯服务模式停止：在启动终端按 Ctrl+C；存在生成任务时需等待其完成
```

## 接入兼容性进度

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

本机也已通过 cargo-xwin 交叉编译 Windows x64 程序，并产出 `../dist/Flow2API_0.1.0_x64-setup.exe`。复制到 Windows 后双击安装；安装器按用户安装，缺少 WebView2 时联网补齐。该包未签名；2b9338f 已由 Grok Bot 回报在 Windows 测试机 当前用户升级成功，health/metrics、常用邮箱填入和资料选择预览通过；真实生成与托盘交互待验。交叉编译步骤和待验证项见 `../docs/windows-cross-build.md`。

## 从 Python 版迁移

1. 停止旧版服务，在桌面版“设置与接入 → 从 Python 旧版迁移”选择项目目录或 `data` 目录。
2. 核对账号／项目／任务／日志／永久作品数。点击停止桌面 API，再点击“备份并导入”。可选导入端口、API Key、代理与图片／视频超时。
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
