#!/usr/bin/env python3
"""Bounded screenshot audit. Uses temporary preferences; requires Pillow for verification."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
from PIL import Image

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, default=Path("/tmp/dice-wars-ui-audit"))
parser.add_argument("--binary", type=Path, default=root / "target/release/dicegame")
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryDirectory(prefix="dice-wars-audit-") as data:
    base = dict(os.environ, XDG_DATA_HOME=data, APPDATA=data, DW_HEADLESS="1",
                DW_SEED="31337", DW_PLAYERS="6", DW_FRAME_LIMIT="2", DW_THEME="light")
    # Do not inherit automation switches from the caller.
    for key in list(base):
        if key.startswith("DW_") and key not in {"DW_HEADLESS", "DW_SEED", "DW_PLAYERS", "DW_FRAME_LIMIT", "DW_THEME"}:
            del base[key]

    def render(name, **changes):
        path = out / f"{name}.png"
        env = dict(base, DW_CAPTURE=str(path), **changes)
        subprocess.run([str(args.binary.resolve())], cwd=root, env=env, check=True, timeout=15)
        with Image.open(path) as image:
            assert image.width > 200 and image.height > 200
            assert len(image.getcolors(image.width * image.height) or []) > 30, name
        return path

    render("menu-light")
    render("menu-dark", DW_THEME="dark")
    render("battle-light", DW_AUTOSTART="1", DW_BATTLE="1")
    render("battle-dark", DW_AUTOSTART="1", DW_BATTLE="1", DW_THEME="dark")
    render("nine-player-teams", DW_AUTOSTART="1", DW_TEAMS="1", DW_PLAYERS="9", DW_THEME="dark", DW_ACCESSIBLE="1", DW_FRAME_LIMIT="15")
    render("team-setup", DW_TEAMS="1", DW_PLAYERS="9", DW_THEME="dark")
    render("small-window", DW_SIZE="640x564")
    render("wide-window", DW_SIZE="1200x820", DW_THEME="dark")
    render("confirmation", DW_AUTOSTART="1", DW_SCENE="confirm", DW_THEME="dark")
    render("game-over", DW_AUTOSTART="1", DW_SCENE="over")
    render("join", DW_SCENE="join", DW_THEME="dark")
    render("replay", DW_AUTOSTART="1", DW_SCENE="replay", DW_FRAME_LIMIT="30", DW_THEME="dark")
    render("gif-export", DW_AUTOSTART="1", DW_SCENE="gif", DW_FRAME_LIMIT="3")
    gifs = list(Path(data).rglob("replay-*.gif"))
    assert len(gifs) == 1, "GIF export failed"
    with Image.open(gifs[0]) as gif:
        assert gif.size == (1395, 1230)
        # The panel divider is below the board, not upside-down above it.
        image = gif.convert("RGB")
        assert image.getpixel((60, 936)) != image.getpixel((60, 900)), "GIF panel orientation"

    # Verify mouse and keyboard switching on the very frame of the input.
    for name, action, size in [("theme-mouse", "2:click:842:32", "930x820"),
                               ("theme-keyboard", "2:key:F10", "1200x820")]:
        render(name, DW_FRAME_LIMIT="3", DW_CAPTURE_FRAMES="1,2", DW_INPUT=action, DW_SIZE=size)
        with Image.open(out / f"{name}-1.png") as before, Image.open(out / f"{name}-2.png") as after:
            assert before.getpixel((0, 0)) != after.getpixel((0, 0)), "theme delayed by a frame"
            assert after.getpixel((0, 0))[0] < 60, "dark background not updated"

    render("keyboard-focus", DW_AUTOSTART="1", DW_INPUT="1:key:Right;2:key:Enter")
    render("reduced-motion", DW_AUTOSTART="1", DW_BATTLE="1", DW_INPUT="1:key:F8")
    print(f"Verified 17 scenes, immediate theme switching, and GIF orientation. Screenshots: {out}")
