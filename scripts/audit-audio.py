#!/usr/bin/env python3
"""Linux output-only check: two game effects separated by silence. No microphone."""
import argparse
import array
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument("--binary", type=Path, default=root / "target/release/dicegame")
args = parser.parse_args()

with tempfile.TemporaryDirectory(prefix="dice-wars-audio-") as data:
    # PipeWire versions can ignore per-stream monitor selection. A temporary
    # sink isolates the test from other games/music without changing the default.
    sink_name = f"dice_wars_audit_{os.getpid()}"
    module = subprocess.check_output(["pactl", "load-module", "module-null-sink",
        f"sink_name={sink_name}"], timeout=3).decode().strip()
    env = {k: v for k, v in os.environ.items() if not k.startswith("DW_")}
    env.update(XDG_DATA_HOME=data, DW_FRAME_LIMIT="240", DW_INPUT="60:key:F10;150:key:F10")
    game = None
    recorder = None
    try:
        game = subprocess.Popen([str(args.binary.resolve())], cwd=root, env=env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        deadline = time.monotonic() + 3
        stream = None
        while time.monotonic() < deadline and game.poll() is None:
            streams = json.loads(subprocess.check_output(
                ["pactl", "--format=json", "list", "sink-inputs"], timeout=2))
            stream = next((s for s in streams if s.get("properties", {}).get(
                "application.process.id") == str(game.pid)), None)
            if stream:
                break
            time.sleep(0.05)
        assert stream is not None, "game did not open an audio output"
        subprocess.run(["pactl", "move-sink-input", str(stream["index"]), sink_name],
                       check=True, timeout=2)
        path = Path(data) / "effects.raw"
        with path.open("wb") as output:
            recorder = subprocess.Popen(["parec", f"--device={sink_name}.monitor",
                f"--monitor-stream={stream['index']}", "--format=s16le", "--rate=44100",
                "--channels=1", "--latency-msec=20"], stdout=output, stderr=subprocess.PIPE)
            _, error = game.communicate(timeout=12)
            assert game.returncode == 0, error.decode()
            time.sleep(0.15)
            recorder.terminate()
            recorder.communicate(timeout=2)
        samples = array.array("h", path.read_bytes())
        groups = 0
        active = False
        for i in range(0, len(samples), 441):
            audible = max(map(abs, samples[i:i + 441]), default=0) > 100
            if audible and not active:
                groups += 1
            active = audible
        assert groups >= 2, f"effects were lost after silence: {groups} audible groups"
        print(f"Verified short effects before and after silence; loopback peak {max(map(abs, samples))}.")
    finally:
        for process in [recorder, game]:
            if process and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        subprocess.run(["pactl", "unload-module", module], check=True, timeout=3)
