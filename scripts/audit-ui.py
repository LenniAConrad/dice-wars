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
    base = dict(os.environ, XDG_DATA_HOME=data, APPDATA=data, DW_HEADLESS="1", DW_FORBID_WINDOW="1",
                DW_SEED="31337", DW_PLAYERS="6", DW_FRAME_LIMIT="2", DW_THEME="light")
    # Do not inherit automation switches from the caller.
    for key in list(base):
        if key.startswith("DW_") and key not in {"DW_HEADLESS", "DW_FORBID_WINDOW", "DW_SEED", "DW_PLAYERS", "DW_FRAME_LIMIT", "DW_THEME"}:
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
    # HOST GAME from the menu opens the lobby on the same grid as the menu.
    for theme in ("light", "dark"):
        render(f"host-lobby-{theme}", DW_THEME=theme, DW_PORT="17793", DW_FRAME_LIMIT="40",
               DW_CAPTURE_FRAMES="1", DW_INPUT="2:click:436:716")
        with Image.open(out / f"host-lobby-{theme}-1.png") as menu, \
                Image.open(out / f"host-lobby-{theme}.png") as lobby:
            region = (570, 112, 890, 252)  # saved maps card -> room code card
            assert menu.crop(region).tobytes() != lobby.crop(region).tobytes(), "HOST GAME did not open the lobby"
    render("host-lobby-teams", DW_THEME="dark", DW_TEAMS="1", DW_PLAYERS="9", DW_PORT="17793",
           DW_FRAME_LIMIT="40", DW_INPUT="2:click:436:716")
    # Repainting only the lands that changed must give exactly the picture a
    # full repaint gives (dice pinned so both runs play the same game).
    for frames in ("61", "157", "263", "389"):
        for mode in ("partial", "full"):
            extra = {"DW_FULL_LAYERS": "1"} if mode == "full" else {}
            render(f"layers-{mode}-{frames}", DW_AUTOSTART="1", DW_TEST_AUTOEND="1",
                   DW_DICE_SEED="7", DW_FRAME_LIMIT=frames, **extra)
        with Image.open(out / f"layers-partial-{frames}.png") as partial, \
                Image.open(out / f"layers-full-{frames}.png") as full:
            assert partial.tobytes() == full.tobytes(), f"partial board repaint differs at frame {frames}"
    # After a finished game the info card shows the record instead of the rules.
    record_dir = Path(data) / "record" / "dice-wars"
    record_dir.mkdir(parents=True)
    (record_dir / "settings.txt").write_text("record 3 2 7 4 1 0\n")
    render("record")
    render("replay", DW_AUTOSTART="1", DW_SCENE="replay", DW_FRAME_LIMIT="30", DW_THEME="dark")
    render("gif-export", DW_AUTOSTART="1", DW_SCENE="gif", DW_FRAME_LIMIT="3")
    gifs = list(Path(data).rglob("replay-*.gif"))
    assert len(gifs) == 1, "GIF export failed"
    with Image.open(gifs[0]) as gif:
        assert gif.size == (1395, 1230)
        # The status panel surface is below the board, not upside-down above it.
        image = gif.convert("RGB")
        def inked(box):
            return sum(min(px) < 200 for px in image.crop(box).getdata())
        # player cards sit in the bottom band; the band above the board is empty
        assert inked((270, 1110, 1130, 1190)) > 2000, "GIF panel orientation"
        assert inked((270, 20, 1130, 80)) == 0, "GIF panel orientation"

    # Verify mouse and keyboard switching on the very frame of the input.
    # theme-mouse opens Settings on frame 2 and picks the Midnight swatch on frame 3.
    for name, action, size in [("theme-mouse", "2:click:860:31;3:click:674:116", "930x820"),
                               ("theme-keyboard", "3:key:F10", "1200x820")]:
        render(name, DW_FRAME_LIMIT="4", DW_CAPTURE_FRAMES="2,3", DW_INPUT=action, DW_SIZE=size)
        with Image.open(out / f"{name}-2.png") as before, Image.open(out / f"{name}-3.png") as after:
            assert before.getpixel((0, 0)) != after.getpixel((0, 0)), "theme delayed by a frame"
            assert after.getpixel((0, 0))[0] < 60, "dark background not updated"
            if name == "theme-mouse":
                region = (160, 160, 440, 360)
                light = before.crop(region).convert("RGB")
                dark = after.crop(region).convert("RGB")
                # Compare flat pastel territory interiors, excluding the sea
                # and antialiased edges that touch the themed background.
                colors = {color for count, color in light.getcolors(light.width * light.height)
                          if count > 300 and min(color) > 180 and max(color) - min(color) > 15}
                pairs = [(a, b) for a, b in zip(light.getdata(), dark.getdata()) if a in colors]
                assert len(pairs) > 3000, "missing pastel territory samples"
                assert sum(a == b for a, b in pairs) / len(pairs) > 0.99, "theme recolored map art"

    # The settings panel opens over the menu in every theme.
    for theme in ("Daylight", "Midnight", "Sakura", "Sage", "Paper", "Ocean", "Ember"):
        render(f"settings-{theme.lower()}", DW_THEME=theme, DW_FRAME_LIMIT="4",
               DW_CAPTURE_FRAMES="1", DW_INPUT="2:click:860:31")
        with Image.open(out / f"settings-{theme.lower()}-1.png") as closed, \
                Image.open(out / f"settings-{theme.lower()}.png") as opened:
            region = (620, 80, 900, 400)
            assert closed.crop(region).tobytes() != opened.crop(region).tobytes(), f"settings panel did not open in {theme}"
        render("keyboard-focus", DW_AUTOSTART="1", DW_INPUT="1:key:Right;2:key:Enter")
    render("reduced-motion", DW_AUTOSTART="1", DW_BATTLE="1", DW_INPUT="1:key:F8")
    for theme in ("light", "dark"):
        path = render(f"selection-{theme}", DW_AUTOSTART="1", DW_SCENE="selection", DW_THEME=theme)
        with Image.open(path) as image:
            board = image.crop((30, 80, 900, 615)).convert("RGB")
            white_pixels = sum(pixel == (255, 255, 255) for pixel in board.getdata())
            assert white_pixels > 100000, f"non-target territories are not white in {theme} mode"
        for players in ("2", "9"):
            path = render(f"battle-replay-{players}-{theme}", DW_AUTOSTART="1", DW_BATTLE="1",
                          DW_SCENE="battle-replay", DW_TEAMS="1", DW_PLAYERS=players, DW_THEME=theme)
            with Image.open(path) as image:
                # Results end above this gap; team labels begin below it.
                gap = image.crop((420, 727, 570, 734))
                assert len(gap.getcolors(gap.width * gap.height) or []) == 1, "replay result overlaps team cards"
    render("menu-refresh", DW_FRAME_LIMIT="5", DW_CAPTURE_FRAMES="1,2,3,4",
           DW_INPUT="2:click:412:414;3:key:F9;4:click:227:628")
    with Image.open(out / "menu-refresh-1.png") as first, Image.open(out / "menu-refresh-2.png") as seed, Image.open(out / "menu-refresh-3.png") as assist, Image.open(out / "menu-refresh-4.png") as color:
        region = (160, 160, 440, 360)
        assert first.crop(region).tobytes() != seed.crop(region).tobytes(), "cached preview ignored seed change"
        assert seed.crop(region).tobytes() != assist.crop(region).tobytes(), "cached preview ignored color assist"
        assert assist.getpixel((100, 710)) != color.getpixel((100, 710)), "color selection ignored"
    # Move without clicking: catch invisible neutral hover fills, selected
    # options that fail to react, and stale status-panel caches in both themes.
    for theme in ("light", "dark"):
        for name, changes, points in [
            ("menu-hover", {"DW_TEAMS": "1"},
             [(518, 400), (370, 705), (178, 462), (360, 462),
              (486, 130), (650, 580), (830, 24), (187, 628)]),
            ("join-hover", {"DW_SCENE": "join"},
             [(484, 508), (330, 154), (650, 512)]),
            ("game-hover", {"DW_AUTOSTART": "1"},
             [(98, 24), (760, 24)]),
            ("confirm-hover", {"DW_AUTOSTART": "1", "DW_SCENE": "confirm"},
             [(242, 408), (404, 408), (566, 408)]),
        ]:
            # hover eases in over a few frames: give every point six
            step = 6
            scene = f"{name}-{theme}"
            actions = ";".join(f"{2 + i * step}:move:{x}:{y}" for i, (x, y) in enumerate(points))
            shots = [1 + (i + 1) * step for i in range(len(points))]
            render(scene, DW_THEME=theme, DW_INPUT=actions, DW_FRAME_LIMIT=str(shots[-1] + 1),
                   DW_CAPTURE_FRAMES=",".join(map(str, [1, *shots])), **changes)
            with Image.open(out / f"{scene}-1.png") as before:
                for frame, point in zip(shots, points):
                    with Image.open(out / f"{scene}-{frame}.png") as after:
                        difference = max(abs(a - b) for a, b in
                                         zip(before.getpixel(point), after.getpixel(point)))
                        assert difference >= 8, f"invisible hover: {scene}, {point}"

        # Bookmark, leave the button, then hover its separate delete target.
        scene = f"bookmark-{theme}"
        render(scene, DW_THEME=theme, DW_FRAME_LIMIT="5", DW_CAPTURE_FRAMES="1,2,3,4",
               DW_INPUT="2:click:499:143;3:move:0:0;4:move:842:182")
        with Image.open(out / f"{scene}-1.png") as before, Image.open(out / f"{scene}-3.png") as saved:
            assert before.getpixel((499, 143)) != saved.getpixel((499, 143)), "bookmark state invisible"
            assert max(abs(a - b) for a, b in zip(before.getpixel((499, 137)),
                                                before.getpixel((499, 143)))) <= 2, "uneven star fill"
        with Image.open(out / f"{scene}-3.png") as saved, Image.open(out / f"{scene}-4.png") as hovered:
            assert saved.getpixel((842, 170)) != hovered.getpixel((842, 170)), "delete hover invisible"
    print(f"Verified 55 scenes, partial board repaints, the host lobby, the record card, replay battle layout, menu cache refresh, white non-target territories and hover feedback in both themes, bookmark states, native output resolution, immediate theme switching, and GIF orientation. Screenshots: {out}")
