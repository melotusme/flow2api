# 默认浏览器连接

## 使用

1. 应用 → Google 账号 → 安装 / 更新连接脚本。默认浏览器需安装 Tampermonkey。
2. 点“打开 Flow 页面”，由系统默认浏览器打开。支持 Ego Lite、Chrome 等能运行脚本的浏览器。
3. 在该页面登录 Flow；应用获取连接码（有效 5 分钟、仅可使用一次），粘到页面右下角并连接。
4. 页面显示“身份与余额校验通过”后回应用创作。保持此标签页打开，应用会用同一页面的会话、网络和验证码。

不导出 Google Cookie，不启动第二个 Chrome 复制登录。应用重启或页面刷新后自动恢复连接并重新校验。关闭标签页超过 15 秒后账号不可用于新任务；重新打开新标签页需重新配对。应用的代理设置只影响专用 Chrome 和原生媒体下载，默认浏览器请求跟随浏览器自己的网络配置。

## 本机接口契约 v2

只监听 127.0.0.1。无需为浏览器开放局域网访问。连接码由 Tauri 本地命令提供，HTTP 不提供读取连接码的入口。

| 请求 | 入参 | 响应与规则 |
| --- | --- | --- |
| `POST /api/browser-bridge/pair` | `{code,email}` | `{account_id,token}`。配对只表示已建立连接；尚未可用。连接码使用一次后失效。 |
| `POST /api/browser-bridge/poll` | Bearer 连接令牌；`{result?,heartbeat?}` | `{commands,connection}`。心跳不领取命令。结果 `{id,value}` 或 `{id,error}`；只接受尚在等待的命令结果。 |
| `POST /api/browser-login/sync` | 旧版 Cookie 请求 | `410`，说明更新脚本步骤，不再导入 Cookie。 |
| `GET /extension/flow2api-flow-login-sync.user.js` | 无 | v2 脚本，旧脚本可通过同地址安装更新。 |

连接状态为 validating / ready / failed；connected 表示最近 15 秒有心跳，只有 ready 且在线且账号启用才允许生成。实际页面邮箱必须与连接邮箱一致，余额 RPC 必须成功并返回非负整数，0 余额是合法值。应用只保存连接令牌的 SHA-256，脚本将令牌保存在脚本管理器存储中；不会放进 URL、页面 DOM 或日志。

浏览器只接受 identity、fetch、navigate、captcha 四种命令。fetch 限 Flow 同源页面与现有 batchexecute RPC；navigate 限项目页；captcha 限三种生成动作。没有通用脚本执行或任意网址代理入口。每次执行前检查当前邮箱。

命令从队列取出后不再投递，超时不自动重试生成。应用/页面退出后的请求结果可能未知，必须核对 Flow 中的作品，避免重复扣费。

## 验证

```bash
cargo test --manifest-path desktop/Cargo.toml -p flow2api-core bridge::tests
rtk proxy curl -fsS http://127.0.0.1:8000/health
```

若系统全局 Cargo 镜像无效，从 `/private/tmp` 执行上面的 Cargo 命令，避免继承用户主目录下的旧镜像配置。连接测试覆盖应用自身的命令与状态；真实 Google 认证与生成需单独记录实测结果。
