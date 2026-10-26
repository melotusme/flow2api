# Rust + Tauri 迁移检查表

目标：Rust 业务核心、Tauri 桌面、Windows 托盘状态、macOS 菜单栏状态，以及现有 Flow2API 功能的双端适配。归档基线：`47498d2`。

## 已实现，按下方记录区分验证范围

- Rust workspace、核心存储、HTTP 生命周期、独立服务入口。
- Tauri 窗口、托盘／菜单栏、状态更新、隐藏窗口、后台执行及显式退出。
- 账号登记、专用 Chrome profile、CDP、登录账号校验、余额同步。
- 现行图片、视频 RPC 的原生迁移、任务队列、媒体持久归档。
- OpenAI/Gemini HTTP、鉴权、模型列表、SSE 与媒体读取。
- Windows NSIS 与 macOS App/DMG 的构建工作流配置。

## 必须继续完成并取得证据

- [x] 当前 Mac 上核心、桌面工程编译检查通过，Cargo.lock 锁定依赖；Windows MSVC 交叉编译及 NSIS 打包通过，Windows 原生构建与运行仍待验证。
- [x] 当前核心 27 项测试、1 项真实 Chrome 恢复测试与 3 项 Postman 媒体传输模拟通过；不涵盖完整生成链路。
- [ ] macOS 窗口、菜单栏、隐藏恢复、状态变化、退出释放端口与浏览器实测。
- [ ] Windows 编译、安装、WebView2、中文路径、托盘、隐藏恢复与退出实测。
- [ ] 图片生成、参考图、2K/4K 放大与永久保存对照。
- [ ] 视频文生、参考图、首尾帧、续写、放大、长任务轮询和权限错误对照。
- [ ] 旧版模型简写与 generationConfig 参数解析兼容核对。
- [ ] Cookie 自动刷新、登录失效提示、账号轮询和浏览器崩溃恢复。
- [ ] 旧配置、SQLite 账号／日志／项目与生成媒体导入，导入前备份及回滚。
- [ ] 管理接口／插件同步／监控接口的兼容范围实现与核对。
- [ ] 安装包与更新保留数据，签名／公证与发行说明。

不把写入代码、模型登记或单个平台编译当作双端功能验收通过。每次运行将实际证据补充到下方。

## 验证记录

- 2026-10-27：现有本机 Cargo 旧 USTC Git 镜像返回 404；隔离构建缓存，切换官方 sparse 源验证中。未修改全局 Cargo 配置。
- 2026-10-27：`node --check desktop/ui/app.js` 通过。
- 2026-10-27：`cargo check -p flow2api-desktop --locked --offline` 通过，解析并锁定 Tauri 2.12.5。
- 2026-10-27：9 项核心测试通过，覆盖实际监听／停止、端口冲突、鉴权、无可用账号、SSE 错误无成功终止标记、媒体 Range、单实例目录锁、较新数据库拒绝降级与未完成任务中断。
- 2026-10-27：Postman `desktop-media` 模拟与 Rust 保存实现真实交互，503 与伪图片 HTML 被拒绝，PNG 保存后哈希／元数据／SQLite 一致；模拟请求日志确认三条路径均被调用。
- 2026-10-27：原生 macOS 桌面工程实际编译、运行并打包为未签名 debug `.app`；启动后 `/health` 返回 healthy，监听 `127.0.0.1:8000`。
- 2026-10-27：通过原生窗口操作验证停止／启动 API，停止后 curl 无法连接；关闭窗口后 API 保持 healthy；重新打开 `.app` 恢复窗口；点击“退出应用”后应用进程不存在、端口释放。菜单栏菜单和活动任务角标仍待实测，故上方整体验收项保持未勾选。
- 2026-10-27：修复定时刷新作品库重置视频播放；补齐 macOS Reopen 恢复窗口；图片放大的 Base64 返回通过格式校验后保存。新增 Base64 归档测试通过，核心共 10 项通过、1 项模拟测试默认忽略。
- 2026-10-27：补入视频轮询时的 `as29s` 媒体查询，避免上游已发布作品但轮询暂未返回地址；该分支尚未通过真实生成验证。
- 2026-10-27：从归档 Python 解析器导出 7 组静态映射，Rust 补齐图片／视频简写、横竖屏、放大档位、`generationConfig`／`extra_body`／OpenAI `size` 和 `quality`，测试确认图片默认横屏及显式内部模型名保持原样。参考图自动推断比例尚未迁移。核心共 12 项测试通过、1 项模拟测试默认忽略。
- 2026-10-27：接入 Tauri single-instance 2.5.4。Mac 上隐藏窗口后直接运行第二个可执行进程，第二进程退出码 0，已有窗口恢复，仅一个应用进程，`/health` 保持 healthy。Windows 插件分支尚未实测。
- 2026-10-27：真实专用 Chrome 测试通过：中文／空格 profile、无登录页面使旧同步状态失效、关闭 CDP 连接后结束旧进程、从相同 profile 再启动成功。测试不访问 Google、不验证真实登录与生成。
- 2026-10-27：自动分配改为按未完成任务数和最近使用时间排序，防止顺序请求始终使用首个账号；核心 13 项测试通过，Chrome／Postman 两项默认忽略，Chrome 项已显式执行通过。
- 2026-10-27：GitHub `melotusme` 对原仓库 `TheSmallHanCat/flow2api` 权限为 READ，SSH 推送 dry-run 被拒绝，未上传代码也未触发 CI。Windows 构建仍需可写仓库或可用 Windows 机器。
- 2026-10-27：新增旧版迁移：原生目录选择、数量预览、可选设置导入、账号按邮箱去重、旧任务／项目／日志摘要归档、永久作品格式与 SHA256 校验、导入前双库 SQLite backup、撤销与新数据保护。数据库升级为 schema 2，保留 schema 1 设置。
- 2026-10-27：5 项新增测试覆盖含 WAL 的旧库快照、配置优先级与短 Key 保留、已有账号保留、幂等导入、损坏作品失败清理、回滚恢复、新数据阻止撤销、schema 1 升级。核心共 18 项通过，Chrome／Postman 两项默认忽略。
- 2026-10-27：Mac 原生 UI 选择独立测试目录后显示正确数量（账号 1，其余 0），设置 Key 使用安全文本框显示。未点击实际导入，未迁移用户数据；合成库导入／撤销由 Rust 测试实际执行。临时预览夹具已清理。
- 待继续：实际旧数据迁移验收、旧 Cookie／登录资料迁入策略、完整旧项目工作流与 Windows 上迁移 UI／中文路径实测，因此迁移整体验收项仍未勾选。

- 2026-10-27：OpenAI/Gemini 简写模型接入参考图尺寸推断，考虑 JPEG EXIF 5–8 的横竖旋转；显式比例和内部模型 ID 保持优先。自动推断时缓存首图下载字节，避免临时 URL 二次读取；Data URI 解码前和 HTTP 分块读取中均限制 20 MB。
- 2026-10-27：Postman 参考图传输模拟显式执行通过。一次性 URL 第一次返回 68 字节 PNG、第二次 503，任务缓存仍可上传；无 Content-Length 的 21,037,124 字节响应被 20 MB 限制拒绝，HTML 伪图片被拒绝。模拟随后已停止，不涵盖 Google 上传、验证码和实际生成。
- 2026-10-27：视频按 operation ID 跟踪全部提交任务，批量轮询；忽略无关项目／任务的地址与错误，保留最新媒体 ID，完成结果不会被后续活动记录覆盖。只读 as29s 查询按指定任务匹配；每个结果分别放大并归档。仅直接地址而无媒体 ID 时明确拒绝放大，不猜测 ID。3 项协议测试通过，真实视频仍待验证。
- 2026-10-27：完整核心测试 23 项通过、3 项环境测试默认忽略（其中新参考图模拟已显式通过）；macOS 桌面 build 和未签名 debug app 重新打包成功。新包界面停止 API 后 8000 无法连接，重新启动后 health healthy，随后退出测试应用。桌面工具只暴露主窗口及应用菜单，未取得状态栏入口，故菜单栏交互／活动角标仍待实测。

- 2026-10-27：接入请求语义继续对照归档 Python：OpenAI 取最后消息的提示词／参考图，支持顶层 image、contents 回退及 extend://；图片编辑从历史 assistant Markdown 找最近可下载图片，不混入旧文字或旧用户附件。Gemini 取最后 user（缺省角色视为 user），过滤媒体模型不适用的工具脚手架，保留简短风格指令；拒绝视频 MIME 作为图片参考。
- 2026-10-27：新增模型别名列表、Gemini 单模型查询、描述／版本等元数据及 Gemini code/message/status 错误形状，含非法 JSON／鉴权／未知模型。3 项新增常规测试通过，完整核心共 26 项通过、4 项环境测试默认忽略。历史参考图 Postman 测试已显式执行通过，日志确认依次 503 与 200 PNG，未访问真实 Flow。
- 2026-10-27：修正双端 CI 的锁文件参数转发，并在上传 macOS app 前使用 ditto 压缩，避免 artifact 丢失可执行权限／符号链接；新增前端语法检查。CI 未触发。Mac arm64 本地 build／重新打包通过，生成开发 ZIP 并核对其中主程序模式为 0755。
- 2026-10-27：Intel Mac x86_64 目标实际编译通过，使用 lipo 合成 arm64 + x86_64 通用程序并由 Tauri 打包。file 确认两种架构；当前 Apple Silicon Mac 上启动通用 app，进程路径指向通用包，health healthy，随后显式退出。生成 dist/flow2api-macos-universal-debug-20261027.zip 并验证 ZIP 内主程序可执行权限与 fat-binary 头。Intel Mac 实机、签名、公证仍未验证。

- 2026-10-27：通过独立 Rust 1.93.0 工具链、cargo-xwin 0.23.1、LLVM/LLD 23.1.4、Microsoft CRT/SDK 完成 Windows x86_64-pc-windows-msvc release 编译。首次发现库与 EXE 的 PDB 文件重名，改库名为 flow2api_desktop_lib 后重编译不再冲突；Mac 桌面也重新编译通过。
- 2026-10-27：使用 makensis 3.13 在 Mac 上通过 Tauri 交叉生成 NSIS 安装器并复制至 dist/Flow2API_0.1.0_x64-setup.exe。生成脚本确认为 currentUser、WebView2 下载引导；卸载删除应用数据需要勾选。PE 检查桌面程序为 x64 GUI，无外置 VC++／SQLite／Python DLL 导入。安装器引导本身为 32 位 NSIS，内含 x64 应用。没有 Windows 实际运行证据，Windows 验收项保持未勾选。
- 2026-10-27：新增 desktop/scripts/cross-windows.sh 与 docs/windows-cross-build.md，记录旧镜像 404 的隔离缓存／工具链处理、NSIS 插件 TLS EOF 后固定校验值的下载恢复及 Windows 实机验收步骤。未修改全局 Cargo 配置和默认 Rust 工具链。
- 2026-10-27：从 /tmp 使用完整环境变量重新运行 cross-windows.sh --offline，release 构建与 NSIS 打包成功，安装器约 5 MB；dist 中提供对应 SHA256 文件。该脚本重跑不需要 GitHub 推送权限，不依赖尚未触发的 CI。

- 2026-10-27：健康接口区分总账号／启用账号／已校验登录账号，失效登录检测后立即刷新可用数；托盘显示可用／启用账号。新增公开 Prometheus `/metrics`，仅提供聚合 gauge，无账号标识或凭据。27 项核心测试通过、4 项环境测试默认忽略，包含禁用账号、登录失效与真实监听上的指标请求；登录状态为最后校验缓存，不保证当前 Google 会话仍有效。
- 2026-10-27：本机 Parallels 注册的 Windows 10 虚拟机状态为 invalid，配置指向的 PVM 目录不存在；未找到可用 Windows SSH 会话，未执行 Windows 实机安装／运行。未修复或修改虚拟机配置。
- 2026-10-27：登录可用数失效刷新通过真实专用 Chrome 测试；Mac arm64／x86_64 桌面重新编译并打包通用 app，Windows x64 release 与 NSIS 安装器重新构建，dist 安装器 SHA256 校验通过。本次包含监控更新；未新增 Windows 实机或真实 Flow 生成证据。
