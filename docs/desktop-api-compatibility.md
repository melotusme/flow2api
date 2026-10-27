# 桌面版与 Python 版接口兼容核对

核对日期：2026-10-27。依据当前源码，不代表所有接口已经实机验收。

## 调用方需要知道的差异

- OpenAI/Gemini 模型发现、聊天生成和 Gemini 两种生成路径已在 Rust 路由中提供；鉴权支持 Bearer、x-goog-api-key 或 key 查询参数。真实 Google 生成链路仍待登录后验证。
- `/health`、`/metrics` 和 `/media/{filename}` 无需 API Key；默认仅监听本机。`/health` 返回 healthy 表示监听可用，active_tokens 才表示启用且最近完成登录校验的账号数，不能据此保证 Google 会话现在仍有效。
- 桌面内置账号、设置、任务和作品管理通过 Tauri 命令调用。旧版 Web 管理 HTTP 路径未实现，现有管理脚本不能直接沿用；桌面功能替代不等于 HTTP 兼容。
- 无需浏览器插件。旧插件同步接口及验证码 WebSocket 未实现；本机 Chrome 登录、同步和 CDP 会话替代该流程。
- 旧日志／项目／任务摘要可经迁移归档查看，不等于继续执行旧任务或提供原来的日志管理接口。

## 桌面功能与剩余差异

| 旧功能 | 桌面入口／实现 | 兼容边界 |
|---|---|---|
| 账号增删、启停 | save_account / delete_account | 账号使用邮箱和本机 Chrome 资料；不接受旧 HTTP 的 token 对象 |
| 余额和登录刷新 | open_login / sync_account | 手工登录后同步；旧 ST→AT 转换、批量 Token 导入 HTTP 未实现 |
| 代理、并发、超时、Key | settings / save_settings | Settings 仅含 port、api_key、browser_executable、max_concurrent、image_timeout_seconds、video_timeout_seconds、proxy_url；旧 proxy/test、debug、缓存和验证码供应商配置未迁移 |
| 统计、任务 | status / tasks | 当前任务状态和计数；旧请求日志详情、删除及完整历史统计接口未实现 |
| 作品库 | library；HTTP media | 永久归档；旧缓存 TTL／开关／base-url 配置未实现 |
| 旧数据迁移 | preview_legacy / import_legacy / rollback_legacy / legacy_history | 预览、备份、可选设置与回滚；不迁入旧浏览器 Cookie，真实日常导入仍待验 |
| 监控 | health / metrics；托盘／菜单栏 | 当前指标列于下方；旧请求耗时、错误等完整监控指标未复刻 |

Rust 路由：`desktop/crates/core/src/api.rs`；本地命令：`desktop/src-tauri/src/lib.rs`；设置：`desktop/crates/core/src/model.rs`。

## 当前 Prometheus 指标

来源：`desktop/crates/core/src/monitoring.rs`。全部为 gauge，无账号标识／凭据。

- flow2api_up
- flow2api_tokens_total
- flow2api_tokens_active
- flow2api_tokens_inactive
- flow2api_native_accounts_enabled
- flow2api_native_accounts_awaiting_login
- flow2api_native_tasks{state="queued|running|succeeded|failed_or_interrupted"}（四组独立标签值）

## Python 路由逐项清单

从三个 Python 路由文件的装饰器提取。表中的“同路径”仅证明 Rust 有该入口；响应字段、错误状态及完整生成效果须以对应测试和实机证据为准。未实现项是兼容缺口，未据此宣告完整迁移通过。

| 方法 | 旧路径 | 源码位置 | 桌面现状 |
|---|---|---|---|
| GET | `/v1/models` | `src/api/routes.py:823` | 同路径已实现；生成结果仍待真实验收 |
| GET | `/v1/models/aliases` | `src/api/routes.py:841` | 同路径已实现；生成结果仍待真实验收 |
| GET | `/v1beta/models` | `src/api/routes.py:858` | 同路径已实现；生成结果仍待真实验收 |
| GET | `/models` | `src/api/routes.py:858` | 同路径已实现；生成结果仍待真实验收 |
| GET | `/v1beta/models/{model}` | `src/api/routes.py:871` | 同路径已实现；生成结果仍待真实验收 |
| GET | `/models/{model}` | `src/api/routes.py:871` | 同路径已实现；生成结果仍待真实验收 |
| POST | `/v1/chat/completions` | `src/api/routes.py:885` | 同路径已实现；生成结果仍待真实验收 |
| POST | `/v1beta/models/{model}:generateContent` | `src/api/routes.py:928` | 同路径已实现；生成结果仍待真实验收 |
| POST | `/models/{model}:generateContent` | `src/api/routes.py:928` | 同路径已实现；生成结果仍待真实验收 |
| POST | `/v1beta/models/{model}:streamGenerateContent` | `src/api/routes.py:974` | 同路径已实现；生成结果仍待真实验收 |
| POST | `/models/{model}:streamGenerateContent` | `src/api/routes.py:974` | 同路径已实现；生成结果仍待真实验收 |
| WEBSOCKET | `/captcha_ws` | `src/api/routes.py:1011` | 未实现；原生 Chrome/CDP 替代插件桥接 |
| POST | `/api/admin/login` | `src/api/admin.py:893` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/admin/logout` | `src/api/admin.py:923` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/admin/change-password` | `src/api/admin.py:931` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/tokens` | `src/api/admin.py:961` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens` | `src/api/admin.py:1045` | 旧 HTTP 未实现；见下方功能差异 |
| PUT | `/api/tokens/{token_id}` | `src/api/admin.py:1098` | 旧 HTTP 未实现；见下方功能差异 |
| DELETE | `/api/tokens/{token_id}` | `src/api/admin.py:1173` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens/{token_id}/enable` | `src/api/admin.py:1185` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens/{token_id}/disable` | `src/api/admin.py:1192` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens/{token_id}/refresh-credits` | `src/api/admin.py:1199` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens/{token_id}/refresh-at` | `src/api/admin.py:1209` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens/st2at` | `src/api/admin.py:1258` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/tokens/import` | `src/api/admin.py:1277` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/browser-login/sync` | `src/api/admin.py:1421` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/config/proxy` | `src/api/admin.py:1466` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/proxy/config` | `src/api/admin.py:1481` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/proxy/config` | `src/api/admin.py:1493` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/config/proxy` | `src/api/admin.py:1510` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/proxy/test` | `src/api/admin.py:1527` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/config/generation` | `src/api/admin.py:1583` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/config/generation` | `src/api/admin.py:1597` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/call-logic/config` | `src/api/admin.py:1614` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/call-logic/config` | `src/api/admin.py:1634` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/system/info` | `src/api/admin.py:1661` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/login` | `src/api/admin.py:1680` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/logout` | `src/api/admin.py:1686` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/health` | `src/api/admin.py:1692` | 保留路径；账号可用数为最近登录校验缓存 |
| GET | `/api/stats` | `src/api/admin.py:1701` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/logs` | `src/api/admin.py:1707` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/logs/{log_id}` | `src/api/admin.py:1745` | 旧 HTTP 未实现；见下方功能差异 |
| DELETE | `/api/logs` | `src/api/admin.py:1772` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/admin/config` | `src/api/admin.py:1782` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/admin/config` | `src/api/admin.py:1795` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/admin/password` | `src/api/admin.py:1806` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/admin/apikey` | `src/api/admin.py:1814` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/admin/debug` | `src/api/admin.py:1828` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/generation/timeout` | `src/api/admin.py:1850` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/generation/timeout` | `src/api/admin.py:1856` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/token-refresh/config` | `src/api/admin.py:1876` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/token-refresh/enabled` | `src/api/admin.py:1890` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/token-refresh/config` | `src/api/admin.py:1901` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/cache/config` | `src/api/admin.py:1931` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/cache/enabled` | `src/api/admin.py:1954` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/cache/config` | `src/api/admin.py:1967` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/cache/base-url` | `src/api/admin.py:1993` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/captcha/config` | `src/api/admin.py:2008` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/captcha/runtime-status` | `src/api/admin.py:2151` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/captcha/config` | `src/api/admin.py:2164` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/captcha/browser-stats` | `src/api/admin.py:2196` | 旧 HTTP 未实现；见下方功能差异 |
| POST | `/api/captcha/score-test` | `src/api/admin.py:2208` | 旧 HTTP 未实现；见下方功能差异 |
| GET | `/api/plugin/config` | `src/api/admin.py:2625` | 未实现；原生 Chrome/CDP 替代插件桥接 |
| POST | `/api/plugin/config` | `src/api/admin.py:2673` | 未实现；原生 Chrome/CDP 替代插件桥接 |
| POST | `/api/plugin/update-token` | `src/api/admin.py:2695` | 未实现；原生 Chrome/CDP 替代插件桥接 |
| POST | `/api/plugin/check-tokens` | `src/api/admin.py:2791` | 未实现；原生 Chrome/CDP 替代插件桥接 |
| GET | `/` | `src/main.py:314` | 旧网页未提供；使用桌面窗口 |
| GET | `/login` | `src/main.py:325` | 旧网页未提供；使用桌面窗口 |
| GET | `/manage` | `src/main.py:334` | 旧网页未提供；使用桌面窗口 |
| GET | `/test` | `src/main.py:346` | 旧网页未提供；使用桌面窗口 |
| GET | `/guide` | `src/main.py:358` | 旧网页未提供；使用桌面窗口 |
| GET | `/library` | `src/main.py:366` | 旧网页未提供；使用桌面窗口 |
| GET | `/api/library` | `src/main.py:374` | HTTP 未实现；桌面 library 命令替代查看 |
| GET | `/media/{filename}` | `src/main.py:384` | 保留路径；仅归档作品，支持 Range |
| GET | `/metrics` | `src/main.py:395` | 保留路径；仅聚合 gauge，指标不完全兼容 |
