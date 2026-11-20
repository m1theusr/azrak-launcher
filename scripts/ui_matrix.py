#!/usr/bin/env python3
"""Screenshot every screen of the launcher at several screen sizes, for visual review.

Runs the real launcher fullscreen under a real window manager (xfwm4) on hidden X displays,
with a throwaway home folder seeded from your cache (no network wait, nothing personal).

    scripts/ui_matrix.py                      # all sizes
    scripts/ui_matrix.py 1280x720 3440x1440   # just these

Writes dist/ui-matrix/<size>/<screen>.png and a contact sheet per size.
Needs: Xvfb, xfwm4, xdotool, ImageMagick.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from ui_fixture import NO_UPDATES, install_game, seed_cache

ROOT = Path(__file__).resolve().parent.parent
SIZES = ["1280x720", "1366x768", "1920x1080", "1920x1200", "2560x1440", "3440x1440", "3840x2160", "1280x800"]

# (screen name, keys to press before the shot)
STEPS = [
    ("00-boot", []),                                       # the start screen (held: SWARMPLAY_BOOT_HOLD_MS)
    ("01-home", ["sleep:9"]),
    ("01b-welcome", ["Left", "Down", "sleep:0.8"]),         # the Welcome tile's widgets
    ("01c-top-buttons", ["Up", "Up", "Right", "Right", "sleep:0.5"]),  # Search, round, focused
    ("01d-avatar", ["Right", "Right", "sleep:0.5"]),        # past Settings: you (the avatar)
    ("02-home-new-game", ["Down", "Right", "Right", "Right"]),
    ("02b-home-peek", ["Down", "Down", "sleep:0.8"]),  # Home scrolled to the Hub's cards
    ("03-hub", ["Return", "sleep:1.5"]),   # the Compatibility card opens the Hub (never Play)
    ("03b-hub-up-to-home", ["Up", "sleep:1"]),              # Up from the Hub's top: back to the cards
    ("04-hub-scrolled", ["Down", "sleep:1.2", "Down", "Down", "sleep:0.8"]),
    ("04b-hub-details", ["Down", "Return", "sleep:0.8"]),
    ("05-library", ["Escape", "Escape", "Up", "Up", "Right", "Return", "sleep:1.5"]),
    ("06-library-down", ["Down", "Down", "Down", "sleep:1"]),
    ("07-search", ["slash", "r", "e", "s", "i", "sleep:1"]),
    ("08-menu", ["Escape", "Escape", "sleep:0.5", "Escape", "sleep:0.8", "o", "sleep:0.6"]),
    ("08b-menu-beside", ["Escape", "Down", "Right", "Return", "sleep:0.6"]),           # from the ··· button: beside it
    ("09-settings", ["Escape", "Escape", "Escape", "sleep:0.5", "s", "sleep:0.8"]),   # the first page: sections
    ("09b-settings-games", ["Return", "sleep:0.6"]),                                 # a section's page
    ("09b2-notification", ["Down", "Down", "Down", "Return", "sleep:0.9"]),          # Rescan: a notification
    ("09c-settings-theme", ["Escape", "Down", "Down", "Down", "Down", "Return", "Right", "sleep:0.8"]),  # Wallpaper: Deep blue
    ("09d-settings-themed", ["Escape", "sleep:0.6"]),                                # back on the first page, themed
    ("10-settings-bottom", ["Down"] * 12 + ["sleep:0.8"]),                           # Quit, at the end
    ("11-toasts", ["Escape", "sleep:0.5", "Home", "Down", "Right", "Right", "Right", "Return", "sleep:0.4",
                   "Up", "Right", "Right", "Down", "Return", "sleep:0.8"]),
    ("12-power", ["Escape", "Escape", "F1", "sleep:0.7", "Down", "Right", "Right", "Right", "Return", "sleep:0.6"]),  # Control Center → Power
]


def run(cmd, **kw):
    return subprocess.run(cmd, **kw)


def shoot(size: str, n: int, out: Path, binary: Path, game: Path | None):
    w, h = size.split("x")
    disp = f":{90 + n}"
    work = Path(tempfile.mkdtemp(prefix="ps5-ui-"))
    home = work / "home"
    (home / ".config/azrak").mkdir(parents=True)
    seed_cache(home)
    install_game(home / "Games", game)
    # A stand-in emulator that exits at once: screenshots must never start a real game.
    fake = work / "kyty_emulator"
    fake.write_text("#!/bin/sh\nexit 0\n")
    fake.chmod(0o755)
    (home / ".config/azrak/config.json").write_text(json.dumps(
        {"game_dirs": ["~/Games"], "sounds": False, **NO_UPDATES, "emulator": str(fake)}))
    # No browsers or players may pop up; pretend to be an old version so the update notification shows.
    stubs = work / "stubs"
    stubs.mkdir()
    for tool in ("xdg-open", "mpv", "vlc", "ffplay", "firefox"):
        (stubs / tool).write_text("#!/bin/sh\nexit 0\n")
        (stubs / tool).chmod(0o755)
    env = {**os.environ, "DISPLAY": disp, "HOME": str(home), "PATH": f"{stubs}:{os.environ['PATH']}",
           "AZRAK_PRETEND_VERSION": "1.3.0", "SWARMPLAY_BOOT_HOLD_MS": "6000"}
    for k in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME"):
        env.pop(k, None)
    procs = [subprocess.Popen(["Xvfb", disp, "-screen", "0", f"{w}x{h}x24", "+extension", "GLX", "-noreset"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)]
    time.sleep(1.5)
    procs.append(subprocess.Popen(["xfwm4", "--compositor=off"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    time.sleep(1.5)
    app = subprocess.Popen([str(binary)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    procs.append(app)
    d = out / size
    d.mkdir(parents=True, exist_ok=True)
    try:
        # Its window by process, else by class (some X setups leave _NET_WM_PID unset); a start
        # with a big seeded cache can take several seconds.
        ids = []
        for _ in range(30):
            ids = (run(["xdotool", "search", "--pid", str(app.pid)], env=env, capture_output=True, text=True).stdout.split()
                   or run(["xdotool", "search", "--class", "swarmplay|azrak"], env=env, capture_output=True, text=True).stdout.split())
            if ids or app.poll() is not None:
                break
            time.sleep(1)
        if not ids:
            raise SystemExit(f"the launcher showed no window (exit code {app.poll()})")
        time.sleep(2)
        win = ids[-1]
        for name, keys in STEPS:
            for k in keys:
                if k.startswith("sleep:"):
                    time.sleep(float(k[6:]))
                    continue
                run(["xdotool", "windowfocus", win], env=env)
                run(["xdotool", "key", "--window", win, k], env=env)
                time.sleep(0.25)
            time.sleep(0.7)
            run(["import", "-window", "root", str(d / f"{name}.png")], env=env)
        pngs = sorted(str(p) for p in d.glob("[0-9]*.png"))
        run(["montage", *pngs, "-tile", "3x", "-geometry", "960x+6+6", "-background", "#333", str(out / f"sheet-{size}.png")])
        print("done", size, flush=True)
    finally:
        for p in reversed(procs):
            p.terminate()
        time.sleep(0.5)
        shutil.rmtree(work, ignore_errors=True)


def main():
    sizes = sys.argv[1:] or SIZES
    # scripts/check.sh shoots the quick build; by itself, the release one.
    binary = Path(os.environ.get("SWARMPLAY_BIN", ROOT / "target/release/swarmplay"))
    out = ROOT / "dist/ui-matrix"
    out.mkdir(parents=True, exist_ok=True)
    game = Path(os.environ["DEMO_GAME"]) if os.environ.get("DEMO_GAME") else None
    for i, s in enumerate(sizes):
        shoot(s, i, out, binary, game)


if __name__ == "__main__":
    main()
