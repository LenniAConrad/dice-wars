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
        scene_data = str(Path(data) / name)
        env = dict(base, XDG_DATA_HOME=scene_data, APPDATA=scene_data,
                   DW_CAPTURE=str(path), **changes)
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
    for name, size in [("full-hd", "1920x1080"), ("laptop-resolution", "2560x1600")]:
        path = render(name, DW_SIZE=size, DW_AUTOSTART="1", DW_BATTLE="1", DW_THEME="dark")
        with Image.open(path) as image:
            assert image.size == tuple(map(int, size.split("x"))), "output lost native resolution"
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
            if name == "theme-mouse":
                region = (157, 335, 445, 513)
                light = before.crop(region).convert("RGB")
                dark = after.crop(region).convert("RGB")
                # Compare flat pastel territory interiors, excluding the sea
                # and antialiased edges that touch the themed background.
                colors = {color for count, color in light.getcolors(light.width * light.height)
                          if count > 300 and min(color) > 180 and max(color) - min(color) > 15}
                pairs = [(a, b) for a, b in zip(light.getdata(), dark.getdata()) if a in colors]
                assert len(pairs) > 3000, "missing pastel territory samples"
                assert sum(a == b for a, b in pairs) / len(pairs) > 0.99, "theme recolored map art"

    render("keyboard-focus", DW_AUTOSTART="1", DW_INPUT="1:key:Right;2:key:Enter")
    render("reduced-motion", DW_AUTOSTART="1", DW_BATTLE="1", DW_INPUT="1:key:F8")
    # Move without clicking: catch invisible neutral hover fills, selected
    # options that fail to react, and stale status-panel caches in both themes.
    for theme in ("light", "dark"):
        for name, changes, points in [
            ("menu-hover", {"DW_TEAMS": "1"},
             [(440, 560), (390, 708), (200, 258), (365, 258),
              (490, 320), (590, 730), (830, 24), (330, 106)]),
            ("join-hover", {"DW_SCENE": "join"},
             [(490, 588), (308, 322), (650, 322)]),
            ("game-hover", {"DW_AUTOSTART": "1"},
             [(726, 702), (820, 702)]),
            ("confirm-hover", {"DW_AUTOSTART": "1", "DW_SCENE": "confirm"},
             [(490, 408), (352, 408)]),
        ]:
            frames = len(points) + 1
            scene = f"{name}-{theme}"
            actions = ";".join(f"{i + 2}:move:{x}:{y}" for i, (x, y) in enumerate(points))
            render(scene, DW_THEME=theme, DW_INPUT=actions, DW_FRAME_LIMIT=str(frames + 1),
                   DW_CAPTURE_FRAMES=",".join(map(str, range(1, frames + 1))), **changes)
            with Image.open(out / f"{scene}-1.png") as before:
                for frame, point in enumerate(points, 2):
                    with Image.open(out / f"{scene}-{frame}.png") as after:
                        difference = max(abs(a - b) for a, b in
                                         zip(before.getpixel(point), after.getpixel(point)))
                        assert difference >= 8, f"invisible hover: {scene}, {point}"

        # Bookmark, leave the button, then hover its separate delete target.
        scene = f"bookmark-{theme}"
        render(scene, DW_THEME=theme, DW_FRAME_LIMIT="5", DW_CAPTURE_FRAMES="1,2,3,4",
               DW_INPUT="2:click:497:333;3:move:0:0;4:move:859:337")
        with Image.open(out / f"{scene}-1.png") as before, Image.open(out / f"{scene}-3.png") as saved:
            assert before.getpixel((497, 330)) != saved.getpixel((497, 330)), "bookmark state invisible"
            assert max(abs(a - b) for a, b in zip(before.getpixel((497, 323)),
                                                before.getpixel((497, 330)))) <= 2, "uneven star fill"
        with Image.open(out / f"{scene}-3.png") as saved, Image.open(out / f"{scene}-4.png") as hovered:
            assert saved.getpixel((854, 330)) != hovered.getpixel((854, 330)), "delete hover invisible"
    print(f"Verified 29 scenes, button hover feedback in both themes, bookmark states, native output resolution, immediate theme switching, and GIF orientation. Screenshots: {out}")
