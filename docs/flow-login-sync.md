# Flow 登录态同步

当前 Flow 地址：[https://flow.google.com/](https://flow.google.com/)。扩展会保存邮箱和服务地址。常用邮箱只放在本机，见 desktop/README.md 的「本机设置（不进 git）」。

## 已实测可用：Chrome / ego lite 扩展

1. 打开 `chrome://extensions`，启用开发者模式，选择“加载未打包的扩展程序”，选择 `$HOME/Documents/flow2api/extension` **文件夹**。不能直接把 ZIP 当作 CRX 导入。
2. 已安装旧版时点击该扩展的“重新加载”，确认版本 `1.0.1`。
3. 在同一浏览器配置中登录 Flow 和 [本机后台](http://localhost:8000/manage)。扩展服务地址必须与后台登录地址相同，`localhost` 与 `127.0.0.1` 的登录会话不共享。
4. 点击“Flow2API Flow 登录同步”扩展图标，再点“同步当前登录态”。相同邮箱重复同步会更新现有账号。

扩展通过 `chrome.cookies` 读取 Flow 及父域 Google Cookie，包括 HttpOnly Cookie，并保留域、路径和过期时间。Google 的部分账号 Cookie 没有 Secure 标记，因此扩展必须同时声明 `http://*.google.com/*` 和 `https://*.google.com/*` 的访问权限；只有 HTTPS 权限会漏读 SID、HSID、APISID。同步请求仍通过 HTTPS 读取 Flow 页面进行验证，不向 Google 发送 HTTP 请求。

扩展读取当前后台的 `admin_session` 作为管理员认证，调用 `POST /api/browser-login/sync`。后端真实请求 Flow，核对页面返回的账号邮箱，再新增账号或仅更新现有凭证，不创建项目。管理员密码不会被扩展读取或保存。Cookie 只发往扩展中显示的服务地址。

### 同步接口

请求为一个对象：`{ "login_account": "账号邮箱", "google_cookies": "浏览器 Cookie 数组的 JSON 字符串", "protocol_mode": "protocol" }`。

需要有效管理员 Bearer 会话或同源后台 Cookie。成功返回 `success`、`email`、`token_id`、`added`、`updated`；未认证返回 401，Cookie 无效或账号不符返回 400，验证失败不会写入账号。

## Tampermonkey / GM_cookie 的实际限制

脚本文件：`userscripts/flow2api-flow-login-sync.user.js`。在 Tampermonkey 中新建脚本，粘贴完整源码后使用编辑器“文件 → 保存”，再刷新 Flow。默认邮箱无需反复填写。

`GM_cookie` 是 Tampermonkey 提供的 API，通过 `@grant GM_cookie` 声明；不是 npm 包。它访问的 URL 必须命中脚本的 `@match` 或 `@include`。`@connect` 只控制网络请求，不能修复 “the URL needs to be included…” 的 Cookie 权限错误。

本脚本仅查询当前 Flow 地址，并匹配本地后台地址，解决了上述 URL 匹配错误。不过，当前用户 ego lite 的 Tampermonkey 实测未返回 Flow 必需的 HttpOnly OSID Cookie，单独使用油猴仍不能完成登录态同步。脚本会明确提示改用扩展，不能靠重复授权或填写邮箱补齐。当前推荐上面的扩展方案。

如果其他脚本管理器能够返回完整 Cookie，脚本通过短时 GM 存储与本地后台交换数据，再使用同源后台会话调用同步接口；URL 仅携带随机标识，不携带 Cookie。该兼容路径未在当前 Tampermonkey 中成功同步。

## 验证与运行

```bash
cd $HOME/Documents/flow2api
rtk proxy ./start.sh
# 另一个终端检查服务
rtk proxy ./status.sh
rtk proxy ./venv/bin/python -m unittest discover -s tests -p test_browser_login_sync.py -q
rtk proxy node --check extension/popup.js
rtk proxy node --check userscripts/flow2api-flow-login-sync.user.js
```

前台运行时 Ctrl+C 停止服务；后台运行时先通过 `status.sh` 确认 PID，再执行 `rtk proxy kill -TERM <PID>`。

2026-10-20 在用户已登录的 ego lite 中实测扩展：首次“新增 1，更新 0”，再次“新增 0，更新 1”；数据库账号 ID 为 2。同步接口的 4 项本地测试通过（管理员认证、验证失败不写入、现有账号保留设置、新账号不创建项目）。同步成功证明登录 Cookie 已通过邮箱校验并保存；不代表图片或视频生成已经验证。

## 2026-10-20 余额与图片生成修复

官网实际余额为 1050。旧配置将 ego_browser 指向已经不存在的空间 14，余额刷新吞掉异常并返回 0，还会使账号被自动禁用。已改为用户授权的空间 2，恢复账号，并让刷新失败明确报错、保留原余额。创建项目时现在携带 token_id，通过同账号校验。

当前运行模式为 ego_browser：登录态同步完成后，余额与生成调用仍需要 ego lite 运行，并保留绑定空间中的 Flow 登录标签页。测试任务结束返回给用户的空间会在用户发起 API 调用时重新接管，并恢复唯一 Flow 标签页的绑定；不会创建其他浏览器配置或绕过邮箱校验。关闭整个空间后须重新绑定，刷新接口会报错，不会假报 0 积分。

实际通过 `/v1/chat/completions` 使用 `gemini-3.1-flash-image-square` 生成了 1024×1024 红苹果图片，保存在 `data/generated/459ab5975f968ea87b51d7bc967e8a73982d01b841022d07df86146d2d2bb82d.jpg`。

```bash
# 查询真实余额，预期正数；认证信息只在本机内存读取，不打印
rtk proxy ./venv/bin/python scripts/verify_account_runtime.py
# 再次生成一张测试图（会提交真实任务）
rtk proxy ./venv/bin/python scripts/verify_account_runtime.py --generate
# 运行余额与账号绑定回归测试
rtk proxy ./venv/bin/python -m unittest discover -s tests -p test_account_runtime.py -q
```
