# 单人创作流程优化（2026-11-04）

## 范围与判断

当前产品入口是 `desktop/` 的 Rust + Tauri 桌面版。Python Web 版是旧实现，本轮不复制两套改动。保留本地 API 供外部工具使用；日常工作台围绕「连接账号 → 描述与参考图 → 选择规格 → 生成 → 预览与复用」。不增加用户、角色或兼容迁移流程。

已阅读项目 AGENTS.md、desktop/README.md、桌面使用及浏览器连接文档。界面使用原生 HTML、JavaScript 与 CSS 主题变量。

## 已确认的问题与改动契约

| 优先级 | 位置 | 当前行为 | 目标行为 |
| --- | --- | --- | --- |
| 高 | `desktop/ui/app.js` renderModels | 82 个公开模型变体堆在一个下拉中 | 按创作类型、模型系列、用途、比例、清晰度/时长逐步选择，只组合后端实际提供的规格 |
| 高 | updateStatus / submit | 账号未就绪仍显示可生成；提交中轮询可能重新启用按钮 | 工作台持续显示阻塞原因及恢复入口，提交期间不能重复提交；字段错误在提交时显示 |
| 高 | fillStudioFromRecord | 复用只还原文字和模型，旧参考图/视频可能残留 | 任务详情完整还原提示词、模型、账号、参考图和视频；切换用途仅提交适用输入 |
| 高 | generation.rs | 排队任务不可取消；开始执行与取消之间没有状态准入约束 | 新增仅本地 Tauri `cancel_task(id)`；只有 queued → cancelled，和 queued → running 共用锁；已执行任务不能假装取消 |
| 中 | 工作台 | 草稿仅在内存；离开/重启丢失 | 文本和规格本机保存；参考图用 IndexedDB 保存，失败时明确提示；不保存 API Key 等凭据 |
| 中 | 最近任务 | 多卡片重复状态且无作品预览 | 工作台展示当前任务与作品，任务历史仍用统一筛选/排序/分页组件 |
| 中 | 账号卡片 | 浏览器桥接账号仍展示资料切换等高级操作 | 常用动作优先，高级操作收起，连接状态变更时及时刷新 |
| 中 | store.rs / Tauri tasks | 每次轮询把所有任务的 Base64 参考图带回界面；单任务查询扫描全表 | 列表返回不含参考图的摘要；复用时单独读取完整任务，单任务使用主键查询 |
| 高 | app.css | 小窗口布局溢出，提示词截断但无法展开 | 320px/缩放下保持可操作；提供完整描述展开；补全焦点和表单错误提示 |

### 前后端契约

- `submit(GenerationRequest)` 结构保持不变。后端校验提示词、参考图数量和源视频，错误给出可执行的修改建议。
- `tasks()` 仅提供展示所需摘要（`request.images=[]`）；`task_detail(id)` 返回原始完整任务，供再次编辑。
- `cancel_task(id)` 只允许取消尚未执行的任务。取消后保留历史记录，`state=cancelled`，阶段说明「已取消，未提交到 Flow」，不计入失败数。上游任务不会被自动重试。
- OpenAI/Gemini 路由和模型 ID 不变。等待任务与 SSE 已将所有非 queued/running 状态作为终态，取消需返回明确错误，不产生成功完成标记。
- 前端依据模型 capabilities 选择规格和校验，不猜测模型 ID；未核价规格始终显示未知。

## 验证记录

### 界面复核范围

| Domain | Evidence inspected | Result |
| --- | --- | --- |
| Accessibility | 原生窗口 AX 树；描述到文件选择的键盘操作；label/summary/dialog；就绪及错误提示 | 已补禁用原因、错误定位、描述展开；焦点实测为 2px 可见环。VoiceOver 逐项朗读未验证 |
| Layout | 五页 320/590/900/1180 宽度布局；320 截图；原生 1180×800 窗口 | 20 组页面/宽度均无横向溢出；590 宽度的 200% CSS 缩放无横向溢出 |
| Writing | 连接步骤、任务终态、参考图要求和断线提示 | 错误附恢复动作；取消明确未提交到 Flow；超时不暗示可安全自动重试 |
| Typography | 描述展开、长 ID 换行、数字、浏览器及原生截图 | 沿用项目字体；完整描述可展开，动态数字使用 tabular-nums。系统字体回退未逐项验证 |
| Colors | 浏览器 computed style 与主题变量 | 边界 3.82:1；次要文字 7.01:1；弱化文字 5.45:1；主按钮 9.15:1；错误 6.28:1；焦点对面板 9.63:1。默认 placeholder 过暗，已改为主题 muted 色 |
| UI | 账号更多操作、草稿、预览、弹窗及 reduced-motion | 使用主题和原生控件；高级操作收起；弹窗限制滚动传播。没有新增自动播放或动效 |

### 处理结果

| Severity | Domain | Location | Before | After | Why |
| --- | --- | --- | --- | --- | --- |
| HIGH | Writing / 业务 | `desktop/ui/app.js:309` | 未就绪仍可提交，刷新可能重开提交按钮 | 持续展示阻塞原因及恢复入口；提交全程锁定 | 避免无效和重复提交 |
| HIGH | 业务 | `desktop/ui/app.js:389` | 回填丢参考图、残留旧输入 | 完整任务回填、替换确认、按用途提交；续写保留源账号 | 避免使用错误输入或账号 |
| HIGH | Typography / Layout | `desktop/ui/app.js:405`、`desktop/ui/app.css:36` | 描述截断不可读全；窄屏溢出 | details 展开、响应布局、可见提交按钮 | 保持内容和操作可达 |
| MEDIUM | Layout | `desktop/ui/app.js:259` | 82 个模型 ID 混成下拉 | 按类型/系列/用途/规格选择 | 减少辨认和重复选择 |
| MEDIUM | 业务 | `desktop/ui/app.js:334`、`:359` | 重开丢草稿 | 文本/规格用 localStorage，图片用 IndexedDB | 减少输入损失；保存失败明确提示 |
| MEDIUM | 业务 | `desktop/crates/core/src/generation.rs:322` | 排队不可撤回 | 取消和执行准入共用锁；已执行不能取消 | 防止取消后仍启动，不假装撤销上游任务 |
| MEDIUM | 性能 | `desktop/crates/core/src/store.rs:267` | 全历史图片反复传输、详情扫全表 | 列表只取摘要，详情按主键读 | 减少数据库/IPC/前端内存负担 |
| MEDIUM | Layout | `desktop/ui/app.js:464`、`:596` | 查看作品需跨页；高级操作挤占主流程 | 就地预览、作品用作参考图、单账号隐藏分配器 | 缩短日常操作 |

### 已执行

- Rust 核心：**42 passed / 5 ignored / 0 failed**。新增测试覆盖取消后不启动、幂等取消、拒绝取消执行中/终态、取消与启动竞争、摘要不含图片但详情完整，以及本机作品读取边界。SSE 测试扩展至 cancelled：无 `[DONE]` 和成功终止标记。
- 前端：`rtk proxy node --test tests/desktop_studio.test.cjs`，**3 passed**；遍历全部 82 个公开模型、真实规格组合、切换用途及 UTF-8 字节/参考图/源视频校验。`node --check`、`cargo fmt --check`、`git diff --check` 通过。
- 隔离夹具：`rtk proxy node desktop/scripts/preview-ui.mjs`，端口 **8173**，无真实账号/凭据/Google 请求。实际操作通过：空描述错误定位、规格/描述/参考图重开恢复、完整回填、断线阻止提交、提交中刷新仍锁定（仅 1 次提交）、取消排队、作品加入参考图、末页、无结果提示和分页记忆。
- macOS：`rtk proxy sh desktop/scripts/build-local-macos.sh` 实际编译、打包并打开原生窗口；账号恢复入口/返回创作/已有作品预览正常。本机开发包位于 `~/Library/Caches/flow2api-local-build/Flow2API.app`。
- 本机 HTTP：`desktop/scripts/verify-e2e.mjs` readiness 阶段通过 health、metrics、82 个 OpenAI 模型、59 个别名和 82 个 Gemini 模型。Key 从本机只读 SQLite 交给子进程环境变量，没有写入脚本或输出。
- Cargo 从 `/private/tmp` 运行，使用 `CARGO_HOME=~/Library/Caches/flow2api-cargo` 避开祖先目录的旧 USTC 源；没有修改全局 Cargo 配置。

### 未验证与下一步

- 当前已登记账号 1、就绪账号 0；Flow 标签页连接断开。本轮没有调用真实 Google 生成，未消耗生成额度。连接后仍需验收图片、参考图编辑、视频及续写。
- Windows 本轮未编译、安装或实测；未发布远程 Release，版本号保持 0.3.0。本次本机构建不代表更新服务器已包含这些修改。
- 范围不包含全部模型权限、验证码供应方、旧 Python Web 页面、长期登录稳定性，以及 VoiceOver 全量朗读。

### Verdict

**Approve（限已验证的本机交互与核心逻辑）**：上述 HIGH 问题已修复。真实 Google 生成、Windows、VoiceOver 和长期连接为 **Not verified**。前端夹具、Rust 测试和健康检查不能替代 Google Flow 实际生成证据。
