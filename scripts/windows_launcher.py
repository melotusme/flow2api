"""Windows double-click entry point; bootstrap uses only the Python standard library."""

import argparse
import hashlib
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
import tomllib
import urllib.request
import venv
import webbrowser

ROOT = Path(__file__).resolve().parents[1]
ENV = ROOT / ".venv-windows"
PYTHON = ENV / "Scripts/python.exe"


def initialize_config():
    target = ROOT / "config/setting.toml"
    if not target.exists():
        text = (ROOT / "config/setting_example.toml").read_text(encoding="utf-8")
        text = text.replace('host = "0.0.0.0"', 'host = "127.0.0.1"', 1)
        text = text.replace('captcha_method = "yescaptcha"', 'captcha_method = "personal"', 1)
        # Exclusive creation prevents replacing an existing configuration.
        with target.open("x", encoding="utf-8") as output:
            output.write(text)
        print("Created config/setting.toml (localhost + local Chrome).", flush=True)
    with target.open("rb") as source:
        return tomllib.load(source)


def dependency_key():
    return hashlib.sha256((ROOT / "requirements.txt").read_bytes()).hexdigest()


def prepare_environment(repair=False):
    if not PYTHON.is_file():
        print("Creating .venv-windows ...", flush=True)
        venv.EnvBuilder(with_pip=True).create(ENV)
    marker = ENV / ".requirements.sha256"
    key = dependency_key()
    if repair or not marker.exists() or marker.read_text().strip() != key:
        print("Installing dependencies (first launch/update needs Internet) ...", flush=True)
        # Failed installs never leave a success marker.
        marker.unlink(missing_ok=True)
        subprocess.run([str(PYTHON), "-m", "pip", "install", "-r", str(ROOT / "requirements.txt")], check=True)
        subprocess.run([str(PYTHON), "-m", "pip", "check"], check=True)
        marker.write_text(key, encoding="ascii")


def check_browser(settings):
    # No application import: this check must work before pip installation.
    if settings.get("captcha", {}).get("captcha_method") not in {"personal", "browser"}:
        return
    sys.path.insert(0, str(ROOT))
    from src.services.browser_executable import resolve_local_browser_executable

    print(f"Chrome: {resolve_local_browser_executable()}", flush=True)


def check_port(host, port):
    family = socket.AF_INET6 if ":" in host else socket.AF_INET
    with socket.socket(family, socket.SOCK_STREAM) as listener:
        # On Windows, avoid treating a port owned by another process as available.
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        try:
            listener.bind((host, port))
        except OSError as error:
            raise RuntimeError(f"Cannot bind {host}:{port}. Close the other instance or change [server] port in config/setting.toml.") from error


def open_when_ready(process, base_url):
    # Ignore system HTTP proxies for a local readiness probe.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    deadline = time.monotonic() + 180
    while process.poll() is None and time.monotonic() < deadline:
        try:
            with opener.open(base_url + "/health", timeout=2) as response:
                if response.status == 200:
                    print(f"Ready: {base_url}/manage", flush=True)
                    webbrowser.open(base_url + "/manage")
                    return
        except (OSError, ValueError):
            pass
        time.sleep(1)
    if process.poll() is None:
        print(f"Startup is taking longer than expected; inspect this console, then visit {base_url}/manage", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Check local prerequisites without installing or starting")
    parser.add_argument("--repair", action="store_true", help="Run dependency installation again")
    args = parser.parse_args()
    if sys.platform != "win32" or sys.version_info[:2] != (3, 11):
        raise RuntimeError("Use Windows x64 with Python 3.11 and start-windows.cmd.")
    os.chdir(ROOT)
    os.environ["PYTHONUTF8"] = "1"
    os.environ["PYTHONUNBUFFERED"] = "1"
    # Keep a byte locked for the entire launch, including installation.
    import msvcrt

    with (ROOT / ".windows-launch.lock").open("a+b") as lock:
        lock.seek(0)
        if not lock.read(1):
            lock.write(b"0")
            lock.flush()
        lock.seek(0)
        try:
            msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
        except OSError as error:
            raise RuntimeError("Flow2API is already starting or running in another window.") from error
        # --check is read-only, including configuration creation.
        config_path = ROOT / "config/setting.toml"
        if args.check:
            with (config_path if config_path.exists() else ROOT / "config/setting_example.toml").open("rb") as source:
                settings = tomllib.load(source)
            if not config_path.exists():
                settings["server"]["host"] = "127.0.0.1"
                settings["captcha"]["captcha_method"] = "personal"
        else:
            settings = initialize_config()
        check_browser(settings)
        host = settings["server"]["host"]
        port = int(settings["server"]["port"])
        check_port(host, port)
        if args.check:
            print(f"Python: {sys.version.split()[0]}; port {port}: available")
            print(f"Environment installed: {PYTHON.is_file()}")
            if PYTHON.is_file():
                subprocess.run([str(PYTHON), "-m", "pip", "check"], check=True)
            return 0
        prepare_environment(args.repair)
        url_host = "127.0.0.1" if host == "0.0.0.0" else "::1" if host == "::" else host
        if ":" in url_host:
            url_host = f"[{url_host}]"
        base_url = f"http://{url_host}:{port}"
        print(f"Starting Flow2API: {base_url}/manage\nPress Ctrl+C in this window to stop.", flush=True)
        process = subprocess.Popen([str(PYTHON), "main.py"], cwd=ROOT)
        threading.Thread(target=open_when_ready, args=(process, base_url), daemon=True).start()
        try:
            return process.wait()
        except KeyboardInterrupt:
            # The console sends Ctrl+C to the child too, allowing lifespan cleanup.
            print("Stopping Flow2API ...", flush=True)
            try:
                return process.wait(timeout=20)
            except (subprocess.TimeoutExpired, KeyboardInterrupt):
                process.terminate()
                process.wait()
                return 130


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
    except Exception as error:
        print(f"\nERROR: {error}\nSee docs/windows.md for troubleshooting.", file=sys.stderr, flush=True)
        sys.exit(1)
