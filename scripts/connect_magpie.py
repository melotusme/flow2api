"""Use the native desktop Magpie connector; never read the legacy Python database."""
import argparse
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="Back up and save Magpie provider")
    parser.add_argument("--data-dir", type=Path, help="Flow2API desktop data directory")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    binary = root / "desktop/target/debug" / ("flow2api-magpie.exe" if os.name == "nt" else "flow2api-magpie")
    if not binary.is_file():
        raise SystemExit("Build first: rtk cargo build --manifest-path desktop/Cargo.toml -p flow2api-core --bin flow2api-magpie")
    if args.data_dir:
        data = args.data_dir
    elif sys.platform == "darwin":
        data = Path.home() / "Library/Application Support/dev.flow2api.desktop"
    elif os.name == "nt":
        data = Path(os.environ["APPDATA"]) / "dev.flow2api.desktop"
    else:
        data = Path(os.environ.get("XDG_DATA_HOME", str(Path.home()/".local/share"))) / "dev.flow2api.desktop"
    command = [str(binary), str(data)]
    if args.apply:
        command.append("--apply")
    raise SystemExit(subprocess.call(command))


if __name__ == "__main__":
    main()
