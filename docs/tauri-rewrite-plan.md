# Rust + Tauri 重写评估

日期：2026-10-27。状态：可行性与实施建议，尚未创建 Tauri 工程或完成 Windows 验收。

## 结论与交付目标

适合改为 Tauri 2 桌面应用，业务逻辑迁到 Rust。目标是 Windows 10/11 x64 安装后直接打开，不要求使用者安装 Python、pip、Node、Rust 或 Docker。构建环境仍需 Rust、MSVC 和 Windows SDK；若使用前端构建工具，还需 Node。本机 Chrome 和 Google/Flow 登录仍有业务用途。

建议交付当前用户安装模式的 NSIS `setup.exe`；安装器检查 WebView2，普通版在缺失时联网安装，另按需要提供内含 WebView2 安装器的离线安装版。离线安装不代表 Flow 可以离线生成。最终安装包大小、签名和 SmartScreen 表现以构建及实机验证为准。

已归档基线：提交 `47498d2`，标签 `archive/python-before-tauri-20261027`。源码包为 `dist/flow2api-python-20261027.zip`。归档不包含本机数据库、登录态、媒体和真实配置。

## 现有代码说明了什么

| 当前路径 | 迁移判断 |
| --- | --- |
| `static/`、`static/assets/` | 页面视觉、交互和 SSE 解析可参考复用；现有相对路径 fetch、登录跳转和 location.origin 依赖 HTTP 同源，不能原封不动放进 Tauri |
| `src/api/routes.py` | 保留 OpenAI/Gemini 调用契约，Rust 提供相同路径、鉴权、错误和流式完成语义 |
| `src/services/flow_current.py`、`flow_frontend.py`、`flow_client.py` | 将现行 Flow 请求组织、上传、轮询、放大、续写逐项迁移；不能只迁模型列表和表单 |
| `src/services/token_manager.py`、`protocol_login.py` | 需要迁移完整 Cookie、刷新、过期判断和账号匹配 |
| `src/services/browser_captcha_personal.py`、`browser_captcha.py` | 分别约 1.67 万和 0.43 万行，是主要迁移风险；包含 Python 浏览器库、注入、会话、并发、重启和清理逻辑 |
| `src/core/database.py`、`media_archive.py` | SQLite、配置和媒体归档可以迁移；要设计版本化迁移、备份和回滚，不能直接打开旧库并假设兼容 |
| `src/services/ego_browser_bridge.py` | 依赖外部 ego-browser 命令和已授权工作区；不适合作为 Windows 默认运行前提 |

行数用于定位复杂性，不用于估算重写工期。

## 建议的模块与接口

```text
Tauri 桌面页面
    │ commands / events
    ▼
Rust 业务模块（共享同一个应用状态）
    ├─ 账号、配置、凭据和版本化 SQLite
    ├─ 生成任务、并发、重试、状态和作品归档
    ├─ Flow 请求与响应处理
    └─ 浏览器会话：启动 Chrome、登录、Cookie、验证码、释放进程
    ▲
    │ 同一业务接口
本机 Axum HTTP 模块 → OpenAI / Gemini / SSE / health
```

- 候选基础库：Tauri 2、Tokio、Axum、Serde、SQLite 库。具体版本和数据库库在建工程时锁定。
- 桌面页面通过 Tauri commands/events 操作本机业务；外部工具继续使用本机 HTTP。两条入口调用同一份生成逻辑，避免出现两套任务状态。
- HTTP 默认监听 `127.0.0.1:8000`，保留 API Key；端口占用给出可操作提示。局域网访问做显式选项。
- 数据写入 Tauri 的应用数据目录，媒体目录可配置，升级保留数据；凭据采用 Windows 安全存储，导出备份显式处理敏感内容。
- 桌面关闭行为需要明确：默认退出并释放应用拥有的 Chrome 与端口；需要后台 API 时选择托盘运行。

## 两个必须先验证的技术风险

### 1. Tauri WebView2 与 Flow 浏览器会话分开

Windows 上 Tauri 使用 WebView2 显示界面。不能据此认定 Google 登录、验证码、已有 Chrome Cookie 和现行注入脚本会直接兼容。

首选验证 Rust 通过 CDP 操作专用 Chrome profile：用户在 Chrome 登录，业务模块绑定该账号和完整 Cookie，执行所需验证码动作及请求，应用统一管理进程生命周期。现有 JavaScript 注入可作为迁移输入，但其时机和协议必须重测。CDP 方案是否足够覆盖现有 personal/browser 行为尚未验证。

优先实现一种验证通过的浏览器方案，再决定旧模式的兼容范围；不把删除模式算作功能迁移完成。

### 2. HTTP 请求行为不能用普通 reqwest 直接假定等价

现有 `flow_client.py` 和 `protocol_login.py` 使用 `curl_cffi` 的 `impersonate`、Cookie 处理、代理及浏览器指纹上下文。普通 Rust HTTP 客户端不自动提供相同 TLS/HTTP 指纹行为。

对无特殊要求的请求可考虑 reqwest；关键 Flow 请求需验证浏览器内发送或其他满足所需行为的传输实现。选择以真实登录刷新、图片和视频请求的结果为依据。若必须依赖额外原生库，应纳入 Windows 打包，不让用户自行补 DLL。

## 实施顺序与验收门槛

1. **浏览器与请求验证**：在 Windows 验证 Rust 启动 Chrome、人工登录、取得正确账号状态、验证码及一条完整图片生成请求。先解决最高风险，再扩展桌面工程。
2. **桌面与存储**：Tauri 窗口、配置向导、日志、端口状态、SQLite、媒体目录；证明不装 Python 的干净 Windows 能启动。
3. **图片闭环**：账号导入/刷新、模型元数据、文生图、参考图、放大、任务进度、失败提示、作品永久保存。
4. **API 兼容**：OpenAI/Gemini 路径、鉴权、SSE、断流/失败、请求大小和媒体地址。用归档版契约与去敏响应样本做对照，控制上游错误时用模拟服务验证。
5. **视频与多账号**：文生/参考图/首尾帧/续写/放大、长任务轮询、并发、限流、Cookie 刷新和浏览器崩溃恢复。
6. **迁移与安装**：旧数据只读导入、导入前备份、媒体搬迁、安装/升级/卸载、中文和空格路径、代理、缺 Chrome/缺 WebView2/端口占用的提示，以及退出不留自有后台进程。

第一步没有通过前，不承诺能够完整取消旧 Python 运行时。若先提供 Tauri + Python sidecar，只能标记为过渡包装，不能视为 Rust 重写验收通过。

## 归档验证与限制

- 已运行浏览器路径现有单元测试 4 项，全部通过；这些测试覆盖 macOS/配置/Linux 分支，没有证明 Windows 检测通过。
- 已运行创作工作台 SSE 测试 3 项，全部通过。
- Windows 启动初稿与浏览器路径模块通过 Python 语法编译。
- 未执行全量回归、真实图片/视频生成、Windows 启动器实机测试或 Tauri 构建。
- 当前归档仅存在本地 Git 和本地 ZIP，未推送远端。

## 官方依据

2026-10-27 阅读：

- [Tauri Windows Installer](https://v2.tauri.app/distribute/windows-installer/)：NSIS/MSI、WebView2 安装方式、当前用户安装和 Windows 构建要求。
- [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)：Windows MSVC 开发工具及 WebView2 前提。

上述页面支持打包路线；项目的 Flow 登录与生成兼容性仍需第一阶段实测。
