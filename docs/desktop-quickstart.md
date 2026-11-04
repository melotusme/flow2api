# Flow2API 桌面版使用说明

适用：Windows x64、Apple Silicon Mac（当前 M3 已验证）。Intel Mac 已构建通用程序，尚无实机验收。

## 1. 安装与打开

| 电脑 | 安装包 | 打开方式 |
|---|---|---|
| Windows | dist/Flow2API_0.1.0_x64-setup.exe | 按当前用户安装后，从开始菜单或桌面快捷方式打开 |
| Mac | dist/flow2api-macos-universal-debug-20261027.zip | 解压后的 Flow2API.app 放到用户 Applications，双击打开 |

当前是未签名开发包，不能当作已经签名、公证的正式发行版。缺少 WebView2 时 Windows 安装器会联网安装。使用者不需要安装 Rust、Python、Node 或 Docker。

Windows 测试机 与 M3 已安装应用。无需再运行旧版 start-windows.cmd 或 Python 服务，以免占用同一 API 端口。重复打开桌面应用会恢复已有窗口。

## 2. 连接 Google 账号（v0.3.0）

1. 账号页点“安装 / 更新连接脚本”。默认浏览器需已安装 Tampermonkey；旧版脚本需更新至 2.0.0。
2. 点“打开 Flow 页面”，通过系统默认浏览器打开，支持 Ego Lite、Chrome 等。
3. 在 Flow 登录后，回应用获取连接码，将其粘到 Flow 页面右下角并点“连接当前 Flow 账号”。
4. 等待身份与余额校验通过，再回应用创作。创作时保持该标签页打开。

Google Cookie 留在当前浏览器，不再复制到另一个 Chrome。连接令牌保存在本机，页面刷新和应用重启后会自动恢复并重新校验。连接码 5 分钟有效且只能使用一次。旧 Cookie 同步插件已停用，旧请求返回更新提示。

高级选项仍提供专用 Chrome 独立登录；它使用单独的浏览器资料。默认浏览器连接无需设置 Chrome 路径，网络使用当前浏览器的配置。

协议、断线和超时边界见 [默认浏览器连接](browser-connection.md)。

## 3. 日常创作（本机优化版）

1. 在工作台选择图片/视频、模型系列和用途，再选择比例、清晰度或时长。只显示真实存在的规格组合。
2. 输入描述，可逐张添加/移除参考图。描述、规格和参考图自动保存在本机草稿，重开后恢复。`⌘ / Ctrl + Enter` 提交。
3. 工作台会先显示账号就绪情况；未连接时可直接进入账号页恢复连接。只有一个账号时自动使用，无需分配。
4. 右侧查看当前任务和生成结果。排队任务可取消；已开始执行的任务需等待结果，超时后先核对 Flow，避免重复扣费。
5. 从任务“再次编辑”恢复完整输入；图片作品可“用作参考图”；视频“一键续写”保留源账号。已有草稿被替换前会确认。

本轮验证及范围见 [单人创作流程复核](product-review-20261104.md)。

## 4. 查看后台状态

| 操作 | Windows | Mac |
|---|---|---|
| 查看状态菜单 | 任务栏右下角通知区域，必要时展开隐藏图标，右键 Flow2API 图标 | 点击菜单栏 Flow2API 图标（F） |
| 恢复主窗口 | 左键托盘图标，或菜单“打开 Flow2API” | 菜单“打开 Flow2API”，或重新打开应用 |
| 保持后台运行 | 关闭主窗口 | 关闭主窗口 |
| 停止 API / 再次启动 | 状态菜单中的 API 停启项，或主窗口按钮 | 同左 |
| 完全退出 | “退出 Flow2API”或窗口内“退出应用” | 同左 |

状态菜单显示 API、执行/排队任务、完成/失败、可用账号、浏览器与最近错误。启停过程中菜单禁用重复操作。任务进行中拒绝正常退出，请等任务完成后再退出。

双端已验证空闲时的菜单、窗口恢复、API 停启与端口释放；实际生成期间任务栏/Dock 进度和数量变化仍待验证。

## 5. 接入外部工具

在“设置与接入”复制：
- Base URL：默认 http://127.0.0.1:8000/v1
- API Key：复制应用当前 Key，不要使用旧版 Key 或把 Key 发到聊天中。

地址只监听当前电脑。Windows 工具连接 Windows 本机地址，Mac 工具连接 Mac 本机地址；两台电脑的 127.0.0.1 互不相通。端口修改后，以应用显示的地址为准。

健康检查（不需要 API Key）：

```bash
rtk proxy curl -fsS http://127.0.0.1:8000/health
rtk proxy curl -fsS http://127.0.0.1:8000/metrics
```

### 一键接入 Magpie

先安装并打开 Magpie，再在 Flow2API 的“设置与接入 → 外部工具接入”点击“一键接入 Magpie”。按钮使用桌面版当前保存的端口和 Key，自动启动本机 API，备份并更新 Magpie 的 `flow2api` 提供商。Key 不会出现在接入结果或命令行中。端口 / Key 改动后再次点击即可更新；重复点击不会增加提供商。

接入包含 OpenAI Chat `/v1` 与 Gemini `/v1beta`，并从应用模型目录登记当前 30 个图片规格。Magpie 生图使用 Gemini 接口；不需要另外实现 `/v1/images/generations`。同名提供商如果指向远程服务器，按钮会拒绝覆盖。备份保存在 Magpie 配置目录的 `backups/providers-before-flow2api-*.json`。

本机已安装技能 `hzr-flow2api-image`，由 Magpie 资源库管理，并以符号链接分发给 Agent。新建 Agent 会话后可以说：

> 使用 $hzr-flow2api-image 画一只黄色陶瓷茶壶，1:1，保存到当前项目 generated-images/teapot.png。

也可通过 CLI 直接调用；模型必须带 `flow2api/` 前缀，避免使用其他提供商：

```bash
rtk proxy python3 "$HOME/.codex/skills/hzr-flow2api-image/scripts/image.py" check
rtk proxy python3 "$HOME/.codex/skills/hzr-flow2api-image/scripts/image.py" generate \
  --prompt '黄色陶瓷茶壶，木桌，窗边自然光，产品摄影' \
  --ratio 1:1 --output "$PWD/generated-images/teapot.png"
```

技能支持文生图和最多 5 张本机参考图，提供 5 种比例、Lite / Flash 系列和基础 / 2K / 4K 规格。Magpie 返回等待中的任务 ID 时，用技能的 `result --id ... --output 原路径` 查询同一任务。创建超时或状态未知时先核对应用任务和作品，不自动重新生成。

开发者也可运行 `rtk proxy python3 scripts/connect_magpie.py --apply`，复用同一个 Rust 接入实现。首次需构建 `flow2api-magpie`；脚本读取桌面版数据库，不再读取旧版 `data/flow.db`。普通桌面用户只需点击按钮，无需 Python 或 Cargo。

2026-11-04 本机已验证配置备份、幂等更新、端口 / Key 更新、冲突保护、Magpie 热加载与桌面按钮。Magpie 调用已经到达 Flow2API；当时可用账号为 0，返回未连接提示，尚未完成本轮真实出图验证。Windows 接入代码已适配配置路径与原子写入，但本轮未做 Windows 实机验收。

health 的 healthy 只表示本机 API 能响应。active_tokens 为 0 表示没有通过登录同步的可用账号，暂不能生成；可用状态是最后一次登录校验缓存。

当前日常界面已移除 Python 旧版迁移入口；新使用无需执行迁移。

## 验收边界

安装、菜单操作、健康检查通过，不能证明图片/视频工作流全部可用。真实文生图、参考图、放大、视频各模式与长期会话刷新，仍需手动登录后逐项验证。完整进度见 docs/tauri-migration-progress.md。
