# AGENTS.md

## 工作规则（必须遵守）

**每次完成任务后，必须在回复末尾输出一段「命令式总结」，让人一眼看清我做了什么。**

要求：
- 用 markdown 表格或命令行 `# --- 本次改动摘要 ---` 风格的清单列出
- 每条包含：**动作**（新增/修改/删除/运行/安装）、**路径或命令**、**目的**
- 列出启动/停止/验证命令，方便用户直接复制
- 列出遗留问题与下一步（如果有）
- 不要只说"已完成"，必须给出可核对的路径、命令、端口、进程

示例格式：

```
# --- 本次改动摘要 ---
[运行] cargo tauri dev                     # 在 desktop 目录启动桌面应用
[修改] desktop/src-tauri/src/lib.rs        # 菜单栏常驻
[验证] curl -fsS http://127.0.0.1:8000/health
# --- 验证 ---
curl -s localhost:8000/health
# --- 遗留 ---
- 未添加 Flow Token，无法实际生成
```

## GitHub Actions 额度约束（必须遵守）

账户是 GitHub Free，私有仓库每月只有 $12 的 Actions 额度，用完后 Actions 会被暂停（预算为 $0，不会扣费）。macOS runner 单价约为 Linux 的 10 倍，是主要开销。

- **macOS 版本只在本机构建**：构建、测试、签名、打包、上传都在本机 Mac 上做。不要在私有仓库的工作流里使用 `macos-*` runner，检查、测试、发布工作流都一样。
- **不要定时任务**：不要添加 `schedule` / cron 触发的工作流，发布只能手动触发。
- 新增或修改工作流前，先确认不会加回 macOS runner 或定时触发；拿不准就先问用户。
- 公开仓库的 Actions 免费，不受以上额度限制。
- 本项目现状（2026-11-07 起）：相关仓库全部公开——源码 fork `melotusme/flow2api`、发行仓库 `melotusme/flow2api-releases`、tap `melotusme/homebrew-flow2api`。公开仓库 Actions 免费，发布直接走发行仓库的 CI（含 macOS），不需要本机打包上传。应用更新匿名读取公开 Release，不需要 GitHub 登录。
- 不要把个人邮箱、本机绝对路径、机器名写进代码或文档：这个仓库是公开的。

## 浏览器实机测试约束（必须遵守）

- **必须使用 Google Chrome 测试与直连，禁止使用 Ego lite（`ego-browser`）**：
  - 用户的 Google Flow（`flow.google.com`）与 Flow Music（`flowmusic.app`）登录态、高阶订阅配额及直连扩展均保存在日常使用的 **Google Chrome** 中。
  - 本项目的一切浏览器实机调试、连接排查与端到端验收**必须使用已登录的 Google Chrome**（例如通过 `chrome-devtools` `--autoConnect` 连接真实 Google Chrome 实例），**严禁使用 `ego-browser`（Ego lite）**，也严禁拉起无登录态的独立受控调试浏览器（`--user-data-dir` 隔离目录）。
  - 桌面端内所有打开浏览器的入口（打开 Flow、打开 Flow Music、安装 Chrome 插件、安装油猴脚本、账号同步自动唤起页面）必须优先显式调用系统 **Google Chrome**（并按账号邮箱自动匹配对应的 `--profile-directory`），不得直接依赖系统默认浏览器（避免在默认浏览器设为 Ego lite 的设备上误打开 Ego lite）。
