#!/usr/bin/env python3
"""The headset launcher's room on the desktop's simulated headset (docs/quest-launcher-plan.md).

Runs the game with HORIZON_SIM_HEADSET=1 and HORIZON_ROOM_STATE=<page> for every page of the
room's TV, dumps the eyes and the panel (HORIZON_SIM_DUMP), and checks pixels: the page's own
colours on the panel, the TV panel laid over the room in the eyes, the room before and after the
game's real block textures, and the hand-off from the room to the game in the same process.

  cargo build --profile fast -p horizon-client
  python3 tools/quest_room_sim.py [--bin target/fast/horizon-client] [--keep DIR]

The game runs in fresh scratch folders (CLAUDE.md: never real player data). Needs a GPU (Vulkan).
The real-texture and hand-off checks need the prepared game files in .local/horizon-client/26.3
and are skipped without them.
"""
import argparse
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

try:
    from . import lab
except ImportError:
    import lab

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RESOURCES = os.path.join(ROOT, ".local/horizon-client/26.3")

BG = (0x0D, 0x0E, 0x13)
ACCENT = (0x9A, 0x86, 0xFF)
DANGER = (0xF0, 0x7A, 0x68)


def read_png(path):
    """(width, height, rows of RGBA tuples-as-bytes) of an 8-bit RGBA PNG (no interlace)."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    pos, idat, width, height = 8, b"", 0, 0
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, color, interlace) == (8, 6, 0), "need 8-bit RGBA, no interlace"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride = width * 4
    rows, prev = [], bytearray(stride)
    for y in range(height):
        filt = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b = prev[i]
            c = prev[i - 4] if i >= 4 else 0
            if filt == 1:
                line[i] = (line[i] + a) & 255
            elif filt == 2:
                line[i] = (line[i] + b) & 255
            elif filt == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
        rows.append(line)
        prev = line
    return width, height, rows


def pixel(img, x, y):
    _, _, rows = img
    return tuple(rows[y][x * 4 : x * 4 + 3])


def count(img, near, tolerance=6):
    """How many pixels are within `tolerance` of the colour `near` on every channel."""
    _, _, rows = img
    n = 0
    for row in rows:
        for i in range(0, len(row), 4):
            if all(abs(row[i + c] - near[c]) <= tolerance for c in range(3)):
                n += 1
    return n


def close(a, b, tolerance=6):
    return all(abs(x - y) <= tolerance for x, y in zip(a, b))


class Sim:
    def __init__(self, binary, keep):
        self.binary = binary
        self.keep = keep
        self.failures = []
        self.checks = 0

    def run(self, name, state, env=None, frame=20, hz=0, timeout=180):
        """Runs one dump; the folder with left.png, panel.png, ... and the log text."""
        scratch = tempfile.mkdtemp(prefix="quest-room-sim-")
        dump = os.path.join(self.keep or scratch, name)
        os.makedirs(dump, exist_ok=True)
        full = lab.game_env(
            HORIZON_HOME=scratch + "/h",
            HORIZON_LAUNCHER_HOME=scratch + "/l",
            XDG_DATA_HOME=scratch + "/d",
            XDG_CONFIG_HOME=scratch + "/c",
            HORIZON_LAUNCHER_NO_SELF_UPDATE="1",
            HORIZON_LAUNCHER_TRASH=scratch + "/t",
            HORIZON_LAUNCHER_CLIPBOARD=scratch + "/cl",
            HORIZON_LAUNCHER_DESKTOP=scratch + "/de",
            HORIZON_SIM_HEADSET="1",
            HORIZON_SIM_HZ=str(hz),
            HORIZON_SIM_DUMP=dump,
            HORIZON_SIM_DUMP_FRAME=str(frame),
            HORIZON_SIM_DUMP_EXIT="1",
            HORIZON_ROOM_STATE=state,
            RUST_LOG="info",
        )
        full.update(env or {})
        try:
            done = subprocess.run(
                [self.binary, "--offscreen", "--home", scratch + "/h", "--resources", scratch + "/none"],
                cwd=scratch,
                env=full,
                capture_output=True,
                text=True,
                timeout=timeout,
            )
            log = done.stdout + done.stderr
        finally:
            shutil.rmtree(scratch, ignore_errors=True) if not self.keep else None
        return dump, log

    def check(self, ok, what):
        self.checks += 1
        if not ok:
            self.failures.append(what)
            print("FAIL", what)
        else:
            print("ok  ", what)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.path.join(ROOT, "target/fast/horizon-client"))
    ap.add_argument("--keep", help="keep the dumps in this folder")
    args = ap.parse_args()
    if not os.path.exists(args.bin):
        sys.exit(f"build it first: cargo build --profile fast -p horizon-client ({args.bin})")
    sim = Sim(args.bin, args.keep)

    # Every page: the panel is the page's own colours.
    pages = {}
    for state in ["idle", "signin", "download", "prepare", "error", "noaccount", "checking"]:
        dump, log = sim.run(state, state)
        panel = read_png(os.path.join(dump, "panel.png"))
        pages[state] = (dump, panel)
        sim.check(panel[:2] == (1280, 720), f"{state}: the panel is 1280 by 720")
        sim.check(close(pixel(panel, 4, 715), BG), f"{state}: the TV's background is the page colour")
        sim.check("panicked" not in log, f"{state}: no panic")
    sim.check(count(pages["idle"][1], ACCENT) > 20000, "idle: a big accent Play button")
    sim.check(count(pages["noaccount"][1], ACCENT) > 15000, "no account: a big accent Sign in button")
    sim.check(count(pages["error"][1], DANGER) > 3000, "error: the headline is in the danger colour")
    download = pages["download"][1]
    sim.check(close(pixel(download, 200, 314), ACCENT), "download: the bar is filled at 30 percent")
    sim.check(not close(pixel(download, 900, 314), ACCENT), "download: and not filled at 90")
    signin = pages["signin"][1]
    sim.check(count(signin, (255, 255, 255), 0) > 80000, "sign in: the QR card is white")
    sim.check(count(signin, (0, 0, 0), 0) > 15000, "sign in: the QR modules are black")
    sim.check(count(signin, ACCENT) > 4000, "sign in: the code is in the accent colour")

    # The TV in the room: the panel over the room where the pointer ray would meet it.
    for state in ["idle", "signin"]:
        dump = pages[state][0]
        room = read_png(os.path.join(dump, "left.png"))
        tv = read_png(os.path.join(dump, "left_tv.png"))
        w, h = room[:2]
        sim.check(pixel(room, 8, 8) == pixel(tv, 8, 8), f"{state}: outside the TV the room is as it was")
        sim.check(pixel(room, w // 2, h // 2) != pixel(tv, w // 2, h // 2) or state == "idle",
                  f"{state}: the TV's middle shows the page, not the black screen")
        changed = sum(1 for y in range(0, h, 4) for x in range(0, w, 4) if pixel(room, x, y) != pixel(tv, x, y))
        sim.check(changed > 2000, f"{state}: the page covers a good part of the eye ({changed} samples)")

    # The pointer: both controllers as grey boxes, the ray of the hand that points (Visor's light
    # grey, 228) to the TV, and the cursor dot on the TV's page where the ray meets it.
    aim = {"HORIZON_SIM_HANDS": "0.4", "HORIZON_SIM_HAND_PITCH": "3.3", "HORIZON_SIM_HAND_YAW": "20.5",
           "HORIZON_SIM_POINTER": "aim"}
    dump, log = sim.run("pointer", "idle", aim, frame=30)
    tv = read_png(os.path.join(dump, "left_tv.png"))
    plain = read_png(os.path.join(pages["idle"][0], "left_tv.png"))
    sim.check("panicked" not in log, "pointer: no panic")
    sim.check(count(tv, (228, 228, 228), 4) > 40, "pointer: the ray is drawn, light grey")
    sim.check(count(tv, (228, 228, 228), 4) > count(plain, (228, 228, 228), 4) + 100,
              "pointer: the ray is not the room's own pixels")
    sim.check(count(tv, (64, 64, 64), 3) > 3000, "pointer: both controllers are grey boxes")
    panel = read_png(os.path.join(dump, "panel.png"))
    # The ray meets the TV on the Play button, 0.195 across and 0.43 down: the dot is white there.
    sim.check(count(panel, (255, 255, 255), 0) > count(pages["idle"][1], (255, 255, 255), 0) + 80,
              "pointer: the cursor dot is on the page")
    near = sum(1 for y in range(295, 326) for x in range(236, 276) if pixel(panel, x, y) == (255, 255, 255))
    sim.check(near > 60, f"pointer: the dot sits on the Play button, where the ray meets the TV ({near})")

    # Language: the same page in the headset's language (the dev host says ru_ru).
    # (checked by eye in the dumps: the Russian idle page; no pixel assert for glyphs)

    if os.path.isdir(RESOURCES):
        # Before and after the game's textures: the floor seen from above.
        look = {"HORIZON_ROOM_LOOK": "0,-70"}
        a, _ = sim.run("floor_before", "idle", look)
        b, log = sim.run("floor_after", "idle", dict(look, HORIZON_ROOM_REAL=RESOURCES))
        sim.check("the room turns real" in log, "real textures: the room says it turned real")
        ia, ib = read_png(os.path.join(a, "left.png")), read_png(os.path.join(b, "left.png"))
        w, h = ia[:2]
        diff = sum(1 for y in range(0, h, 8) for x in range(0, w, 8) if pixel(ia, x, y) != pixel(ib, x, y))
        sim.check(diff > 500, f"real textures: the room looks different once they are bound ({diff})")
        # The hand-off: Play after 40 frames, the same process goes on to the loading screen and
        # the game's title.
        dump, log = sim.run("handoff", "idle", {"HORIZON_ROOM_AUTOPLAY": "40", "HORIZON_ROOM_REAL": RESOURCES},
                            frame=700, hz=90)
        sim.check("Horizon room: Play on" in log, "hand-off: Play was pressed")
        sim.check("startup handoff" in log, "hand-off: the game started from the room in the same process")
        sim.check("panicked" not in log and "frame failed" not in log, "hand-off: no panic, no failed frame")
    else:
        print("skip real textures and the hand-off: no", RESOURCES)

    print(f"{sim.checks - len(sim.failures)} of {sim.checks} checks passed")
    sys.exit(1 if sim.failures else 0)


if __name__ == "__main__":
    main()
