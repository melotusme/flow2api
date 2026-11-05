# 私有更新认证

0.3.2 起，更新器先使用 GitHub CLI；CLI 缺失、未登录、凭据失效或没有发行仓库权限时，只读尝试本机 GitHub Desktop 的 GitHub.com 登录。每个候选凭据必须实际读取固定仓库的 latest Release 成功，才用于下载。设置页显示成功的认证来源，令牌不经过 Tauri IPC。

## 存储契约

- macOS：系统钥匙串 generic password，service 精确等于 `GitHub - https://api.github.com`。
- Windows：用户凭据管理器 generic credential，target 为上述 service 加 `/account`，blob 为 UTF-8。只枚举这个 service 前缀。
- 普通 token 和 `github-desktop-oauth:` version 1 中的 accessToken 均可读取。刷新令牌不用于请求或轮换；登录仍由 Desktop 管理。
- Linux：当前只支持 CLI，GUI 社区版不视为官方 Desktop 存储契约。

来源：[GitHub Desktop auth.ts](https://github.com/desktop/desktop/blob/5c43eb4fce40bfb7c586543565e9d52ba12d0ed8/app/src/lib/auth.ts)、[TokenStore](https://github.com/desktop/desktop/blob/5c43eb4fce40bfb7c586543565e9d52ba12d0ed8/app/src/lib/stores/token-store.ts)、[版本化 OAuth](https://github.com/desktop/desktop/blob/5c43eb4fce40bfb7c586543565e9d52ba12d0ed8/app/src/lib/account-credential.ts)，并对照本机安装包 source map；Windows UTF-8 blob 与 target 拼接依据 keytar 的 src/keytar_win.cc。此接口由 Desktop 拥有，未来格式变化会明确报错。

## 边界

只安装 Desktop 不代表已经登录或有私有仓库权限。系统凭据库锁定/拒绝访问、退出登录、token 过期、仓库 403/404 和网络失败分别反馈。读取不会修改/删除凭据、绕过系统授权或复制到 Flow2API 的配置与数据库。 macOS 首次读取可能需要用户在手动检查更新时授权；后台不主动弹授权窗口。

凭据只发给 `api.github.com/repos/melotusme/flow2api-releases/releases/latest` 或该仓库的数字 asset ID；CDN 重定向不带 Authorization。安装仍要求有效的版本绑定签名。

## 版本引导

0.3.1 及更早客户端尚不具备 Desktop 回退，若该机没有可用 CLI 登录，需要手工安装一次 0.3.2，然后才能通过 Desktop 登录检查后续更新。
