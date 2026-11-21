# 默认浏览器连接

## 使用

1. **方式一（推荐，免配置一键拖入）**：应用 → Google 账号 → 在「本机 Chrome 配置与插件状态」点击对应账号配置的「一键安装插件 ↗」（或顶部「安装 Chrome 插件 ↗」），应用会自动打开该 Chrome 配置的 `chrome://extensions/` 页面、在文件管理器中高亮选中已解压的 `chrome-extension`（`v0.4.1`）文件夹并复制路径。开启右上角「开发者模式」后，直接把高亮的 `chrome-extension` 文件夹拖进扩展页即可立即生效——**全部站点与本机通信权限自动授予，无需手动设置，且已打开的 Flow / Flow Music 标签页无需刷新即自动直连**。
2. **方式二（油猴脚本，需手动授权）**：点击「安装油猴脚本 ↗」安装 `v0.4.1` 连接脚本。**Chrome 138+ 需手动在 Tampermonkey 扩展详情页开启「允许运行用户脚本」，并在首次运行时允许 `@connect 127.0.0.1` 跨域请求**。若同时安装了 Chrome 插件与油猴脚本，页面会自动去重，不会重复挂载。
3. 点对应账号的“打开 Flow ↗”或“打开 Flow Music ↗”，应用会优先显式调用系统 **Google Chrome** 并按邮箱自动打开对应的用户配置（即使系统默认浏览器设为其他浏览器也不会误打开，且绝不启动无登录态的独立受控调试 Chrome）。
4. 页面显示“已连接”后回应用创作。保持该标签页打开，应用会复用同一页面的会话、网络和验证码。

不导出 Google Cookie，不启动第二个 Chrome 复制登录。应用重启或页面刷新后自动恢复连接并重新校验。关闭标签页超过 15 秒后账号不可用于新任务；重新打开新标签页需重新配对。应用的代理设置只影响专用 Chrome 和原生媒体下载，默认浏览器请求跟随浏览器自己的网络配置。

## 本机接口契约 v2

只监听 127.0.0.1。无需为浏览器开放局域网访问。连接码由 Tauri 本地命令提供，HTTP 不提供读取连接码的入口。

| 请求 | 入参 | 响应与规则 |
| --- | --- | --- |
| `POST /api/browser-bridge/pair` | `{code,email}` | `{account_id,token}`。本机直连支持空 `code`；配对后由服务端校验身份与余额。 |
| `POST /api/browser-bridge/poll` | Bearer 连接令牌；`{result?,heartbeat?,resume?}` | `{commands,connection}`。心跳不领取命令。结果 `{id,value}` 或 `{id,error}`；只接受尚在等待的命令结果。 |
| `POST /api/music-bridge/pair` | `{email}` | `{account_id,token}`。连接 `flowmusic.app` 标签页并复用同邮箱账号记录。 |
| `POST /api/music-bridge/poll` | Bearer 连接令牌；`{result?,heartbeat?,resume?}` | `{commands,connection}`。只允许 `identity` 与白名单 `/__api/*` 请求，Supabase 令牌不离开浏览器。 |
| `POST /api/browser-login/sync` | 旧版 Cookie 请求 | `410`，说明更新脚本步骤，不再导入 Cookie。 |
| `GET /extension/flow2api-chrome-extension.zip` | 无 | `v0.4.1` Manifest V3 Chrome 扩展压缩包（启动时亦自动解压至 `<数据目录>/chrome-extension`）。 |
| `GET /extension/flow2api-flow-login-sync.user.js` | 无 | `v0.4.1` 油猴连接脚本，同时匹配 `flow.google.com` 与 `flowmusic.app`。 |

连接状态为 validating / ready / failed；connected 表示最近 15 秒有心跳，只有 ready 且在线且账号启用才允许生成。实际页面邮箱必须与连接邮箱一致，余额 RPC 必须成功并返回非负整数，0 余额是合法值。应用只保存连接令牌的 SHA-256，脚本将令牌保存在脚本管理器存储中；不会放进 URL、页面 DOM 或日志。

浏览器只接受 identity、fetch、navigate、captcha 四种命令。fetch 限 Flow 同源页面与现有 batchexecute RPC；navigate 限项目页；captcha 限三种生成动作。没有通用脚本执行或任意网址代理入口。每次执行前检查当前邮箱。

命令从队列取出后不再投递，超时不自动重试生成。应用/页面退出后的请求结果可能未知，必须核对 Flow 中的作品，避免重复扣费。

## 验证

```bash
cargo test --manifest-path desktop/Cargo.toml -p flow2api-core bridge::tests
rtk proxy curl -fsS http://127.0.0.1:8000/health
```

若系统全局 Cargo 镜像无效，从 `/private/tmp` 执行上面的 Cargo 命令，避免继承用户主目录下的旧镜像配置。连接测试覆盖应用自身的命令与状态；真实 Google 认证与生成需单独记录实测结果。
