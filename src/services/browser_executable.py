"""Resolve installed desktop Chrome without downloading a separate Chromium."""

import os
import sys
from pathlib import Path
from typing import Optional


def resolve_local_browser_executable() -> Optional[str]:
    configured = os.environ.get("BROWSER_EXECUTABLE_PATH", "").strip()
    if configured:
        if not Path(configured).is_file() or not os.access(configured, os.X_OK):
            raise RuntimeError(f"BROWSER_EXECUTABLE_PATH 不存在或不可执行: {configured}")
        return configured
    if sys.platform == "darwin":
        for applications in (Path("/Applications"), Path.home() / "Applications"):
            chrome = applications / "Google Chrome.app/Contents/MacOS/Google Chrome"
            if chrome.is_file() and os.access(chrome, os.X_OK):
                return str(chrome)
        raise RuntimeError("未找到已安装的 Google Chrome，请设置 BROWSER_EXECUTABLE_PATH；不会自动下载 Chromium。")
    if sys.platform == "win32":
        for variable in ("PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"):
            root = os.environ.get(variable)
            if root:
                chrome = Path(root) / "Google/Chrome/Application/chrome.exe"
                if chrome.is_file():
                    return str(chrome)
        raise RuntimeError("未找到已安装的 Google Chrome，请安装 Chrome 或设置 BROWSER_EXECUTABLE_PATH；不会自动下载 Chromium。")
    return None
