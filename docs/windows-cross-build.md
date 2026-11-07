# Windows x64 构建与安装验证

## 给使用者

0.1.0 开发安装器：`dist/Flow2API_0.1.0_x64-setup.exe`。它未签名，不是正式发布。正式安装走公开发行仓库 `melotusme/flow2api-releases` 的 `Flow2API.windows.x64.exe`，当前用户 NSIS，参数是大写 `/S`。缺少 WebView2 时安装器联网补齐，Flow 登录仍需本机 Google Chrome。还没有 WinGet 包，也没有原生 Windows ARM64 包。

安装路径默认为当前用户的 `%LOCALAPPDATA%\Flow2API`，桌面数据由 Tauri 保存在 `%APPDATA%\dev.flow2api.desktop`。升级安装与退出均应保留数据；卸载器仅在用户勾选删除应用数据时清除数据目录。此处是生成安装脚本的检查结果，尚未经 Windows 实机安装／升级／卸载验证。

本包未签名，Windows 安装与运行尚未实测。不能把交叉编译成功当作 Windows 验收通过。

## 已取得的证据（2026-10-27）

- cargo-xwin 0.23.1、Rust 1.93.0、LLVM/LLD 23.1.4、Microsoft CRT/SDK 和 makensis 3.13 在当前 Mac 上完成 Windows MSVC release 构建及 Tauri NSIS 打包。
- 桌面 EXE 是 x86_64 PE，使用 Windows GUI 子系统；导入表没有外置 VC++、SQLite 或 Python DLL。WebView2 运行时仍是系统依赖。
- 安装器是 NSIS 自解压 PE；32 位安装引导程序携带 64 位应用属于 NSIS 的正常结构。
- 检查生成的 `installer.nsi`：currentUser 模式、WebView2 下载引导、开始菜单入口及可选删除应用数据。
- 修复桌面库／EXE 的 PDB 输出重名：库名改为 `flow2api_desktop_lib`，重新构建后不再报告冲突。

## 在 Mac 上重建

通常开发环境：

```bash
rtk proxy brew install llvm lld makensis
rtk proxy cargo install cargo-xwin --version 0.23.1 --locked
rtk proxy rustup target add x86_64-pc-windows-msvc
rtk proxy bash desktop/scripts/cross-windows.sh
```

脚本只构建和打包，保留 `Cargo.lock`，复制安装器及 SHA256 校验文件到 `dist`；不安装或运行 Windows 应用。需要 Tauri CLI。`FLOW2API_CARGO_XWIN` 可指定 cargo-xwin 绝对路径；其他 Cargo、Rustup、xwin 环境变量由调用者传入。脚本参数传给 Cargo，例如 `--offline`。

本机旧 USTC Cargo 索引和已安装 Rust 组件清单的清华镜像均返回 404。验证使用独立配置，未修改全局 Cargo 配置或默认 Rust 工具链：

- Cargo install 缓存：`/tmp/flow2api-cross-cargo`，registry 复用原有缓存；目录不放在用户主目录之下，以免继承祖先 `.cargo/config.toml`。
- Rust 工具链：`$HOME/Library/Caches/flow2api-rustup`，从官方源重新安装 1.93.0 与 Windows 标准库。
- cargo-xwin：`$HOME/Library/Caches/flow2api-cross-tools/bin/cargo-xwin`。
- SDK：`$HOME/Library/Caches/flow2api-xwin`，使用 `XWIN_ARCH=x86_64`。
- Tauri 插件下载遇到 TLS EOF；系统 curl 下载成功后，核对 tauri-bundler 2.8.1 固定的 SHA1 `75197FEE3C6A814FE035788D1C34EAD39349B860`，再放入其 NSIS 缓存。不跳过校验。

## Windows 实机验收待办

1. 在普通用户及中文路径下安装；确认 WebView2 已存在／不存在两种情况。
2. 启动后检查 `http://127.0.0.1:8000/health`；端口被占用时应在界面显示错误并允许改端口。
3. 检查托盘菜单状态、左键恢复窗口、关闭后后台运行、重复启动恢复已有窗口。
4. 执行任务时检查任务栏进度、托盘计数；任务完成／失败后状态应与记录一致。
5. Chrome 登录、同步、图片／视频／放大、永久作品保存与浏览器恢复。
6. 退出后确认应用进程和端口释放；升级保留数据；卸载不勾选删除数据时保留账号与作品。
