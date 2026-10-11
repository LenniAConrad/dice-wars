#!/usr/bin/env python3
"""Bounded frame-time checks, isolated from the player's saved settings."""
import argparse
import os
from pathlib import Path
import resource
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument("--binary", type=Path, default=root / "target/release/dicegame")
parser.add_argument("--native", action="store_true", help="open two short native-window checks")
args = parser.parse_args()

with tempfile.TemporaryDirectory(prefix="dice-wars-runtime-") as data:
    base = {k: v for k, v in os.environ.items() if not k.startswith("DW_")}
    base.update(XDG_DATA_HOME=data, APPDATA=data, DW_SEED="31337", DW_PROFILE="1")
    cases = [("idle", None, {}), ("battle", None, {"DW_AUTOSTART": "1", "DW_BATTLE": "1"})] if args.native else [
        ("idle", "930x820", {}),
        *[("battle", size, {"DW_AUTOSTART": "1", "DW_BATTLE": "1"})
          for size in ["930x820", "1920x1080", "2560x1600", "3840x2160"]],
    ]
    for name, size, switches in cases:
        env = dict(base, DW_FRAME_LIMIT="120", **switches)
        if size:
            env.update(DW_HEADLESS="1", DW_SIZE=size)
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        started = time.monotonic()
        result = subprocess.run([str(args.binary.resolve())], cwd=root, env=env,
                                capture_output=True, text=True, timeout=15, check=True)
        wall = time.monotonic() - started
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        cpu = after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime
        print(f"{name} {size or 'native'}: wall {wall:.2f}s, CPU {cpu:.2f}s")
        print(result.stderr.strip())
    # getrusage reports the largest child peak across this audit, not a per-case value.
    print(f"Peak child memory across audit: {resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss} KiB")
